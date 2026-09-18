use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Barrier};
use std::thread;

use multplx_domain::lifecycle::subagent_model::{
    ArtifactKind, AssignmentRole, TaskRecord, write_meta,
};
use multplx_domain::project_registry::{self, CheckoutOwnership};

fn run(command: &mut Command) -> Output {
    command.output().expect("run command")
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = run(Command::new("git").current_dir(repo).args(args));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8")
        .trim()
        .to_owned()
}

fn mx(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    command
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", home.join("state"));
    command
}

fn owned_mx(home: &Path) -> Command {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/codex-wake-owner.py");
    let mut command = Command::new("python3");
    command
        .arg(fixture)
        .arg(home.join("state"))
        .arg("retain")
        .arg(env!("CARGO_BIN_EXE_mx"))
        .env("MX_HOME", home);
    command
}

fn owned_command(home: &Path, program: &Path) -> Command {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/codex-wake-owner.py");
    let mut command = Command::new("python3");
    command
        .arg(fixture)
        .arg(home.join("state"))
        .arg("retain")
        .arg(program)
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", home.join("state"));
    command
}

fn submit(home: &Path, binding: &project_registry::ProjectBinding, request: &str) -> Command {
    let mut command = mx(home);
    command.args([
        "request",
        "submit",
        "--batch",
        "batch-1",
        "--request",
        request,
        "--task",
        request,
        "--client",
        "terminal-1",
        "--project",
        &binding.project_id,
        "--checkout",
        &binding.checkout_id,
        "--start",
        &binding.starting_revision,
        "--brief",
        "2",
        "--scope",
        "implement accepted work",
        "--artifact",
        "data/brief-2.md",
    ]);
    command
}

fn registered(home: &Path, root: &Path, name: &str) -> project_registry::ProjectBinding {
    let repo = root.join(name);
    fs::create_dir(&repo).expect("repo");
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["config", "user.name", "Test"]);
    fs::write(repo.join("README.md"), format!("{name}\n")).expect("readme");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "fixture"]);
    project_registry::register_project(home, &repo, Some(name), CheckoutOwnership::UserOwned)
        .expect("register")
}

#[test]
fn terminal_request_is_project_bound_repeat_safe_and_recovers_notification() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let repo = temp.path().join("repo");
    fs::create_dir_all(home.join("state")).expect("home");
    fs::create_dir(&repo).expect("repo");
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["config", "user.name", "Test"]);
    fs::write(repo.join("README.md"), "fixture\n").expect("readme");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "fixture"]);
    let binding = project_registry::register_project(
        &home,
        &repo,
        Some("fixture"),
        CheckoutOwnership::UserOwned,
    )
    .expect("register");

    let first = run(&mut submit(&home, &binding, "request-1"));
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let accepted: serde_json::Value = serde_json::from_slice(&first.stdout).expect("receipt");
    assert_eq!(accepted["project_id"], binding.project_id);
    assert_eq!(accepted["checkout_id"], binding.checkout_id);
    assert_eq!(accepted["starting_revision"], binding.starting_revision);
    assert_eq!(accepted["brief_revision"], 2);
    assert!(accepted["recipient_owner"].is_null());
    assert!(accepted["notification_event_id"].as_str().is_some());

    fs::write(repo.join("README.md"), "fixture\nadvanced\n").expect("advance readme");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "advance after acceptance"]);
    let repeated = run(&mut submit(&home, &binding, "request-1"));
    assert!(repeated.status.success());
    assert_eq!(first.stdout, repeated.stdout);
    assert_eq!(
        fs::read_to_string(home.join("state/.wake-queue"))
            .expect("wake")
            .lines()
            .count(),
        1
    );

    let mut changed = binding.clone();
    changed.starting_revision = "0".repeat(40);
    let conflict = run(&mut submit(&home, &changed, "request-1"));
    assert!(!conflict.status.success());
    assert!(
        String::from_utf8_lossy(&conflict.stderr).contains("conflicts with durable submission")
    );

    let connection = run(mx(&home).args(["request", "connection"]));
    assert!(connection.status.success());
    let connection: serde_json::Value =
        serde_json::from_slice(&connection.stdout).expect("connection");
    assert_eq!(connection["connected"], false);
    assert_eq!(connection["attachment"], "unavailable");
    assert!(connection["endpoint"].is_null());

    let lifecycle = temp.path().join("request-lifecycle.sh");
    fs::write(
        &lifecycle,
        format!(
            "#!/bin/sh\nset -eu\nMX='{}'\n\"$MX\" request acknowledge request-1 > '{}'\n\"$MX\" request response request-1 --id response-1 --summary accepted > '{}'\n\"$MX\" request complete request-1 --id completion-1 --summary complete --artifact data/result.md > '{}'\n",
            env!("CARGO_BIN_EXE_mx"),
            temp.path().join("ack.json").display(),
            temp.path().join("response.json").display(),
            temp.path().join("completion.json").display(),
        ),
    )
    .expect("lifecycle script");
    fs::set_permissions(&lifecycle, fs::Permissions::from_mode(0o755)).expect("script mode");
    let lifecycle_result = run(&mut owned_command(&home, &lifecycle));
    assert!(
        lifecycle_result.status.success(),
        "{}",
        String::from_utf8_lossy(&lifecycle_result.stderr)
    );
    let completed: serde_json::Value = serde_json::from_slice(
        &fs::read(temp.path().join("completion.json")).expect("completion receipt"),
    )
    .expect("completion JSON");
    assert!(completed["acknowledged_at"].as_u64().is_some());
    assert_eq!(completed["response"]["id"], "response-1");
    assert_eq!(completed["completion"]["id"], "completion-1");
    assert_eq!(completed["envelope"]["acknowledgement"], "completed");
}

#[test]
fn startup_claim_recovers_request_accepted_before_wake_publication() {
    use multplx_core::filesystem::TransitionFault;
    use multplx_domain::operational_input::{RequestStore, RequestSubmission};

    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let state = home.join("state");
    fs::create_dir_all(&state).expect("state");
    let store = RequestStore::new(&state, &home);
    let submission = RequestSubmission {
        batch_id: "batch-1",
        request_id: "request-1",
        task_id: "task-1",
        client_id: "terminal-1",
        recipient_owner: None,
        parent_task_id: None,
        parent_home: None,
        attempt_id: None,
        attempt_generation: None,
        project_id: "project-1",
        checkout_id: "checkout-1",
        starting_revision: "0123456789abcdef0123456789abcdef01234567",
        brief_revision: 1,
        scope: "accepted before notification",
        dependencies: &[],
        context_artifact: None,
    };
    store
        .submit(
            &submission,
            std::time::SystemTime::now(),
            Some(TransitionFault::AfterCommit),
        )
        .expect_err("crash after authoritative inbox write");
    assert!(!state.join(".wake-queue").exists());

    let claimed = run(owned_mx(&home).args(["wake", "claim"]));
    assert!(
        claimed.status.success(),
        "{}",
        String::from_utf8_lossy(&claimed.stderr)
    );
    let wake: serde_json::Value = serde_json::from_slice(&claimed.stdout).expect("wake");
    assert_eq!(wake["record"]["key"], "request-request-1");
    assert!(
        store
            .get("request-1")
            .expect("request")
            .notification_event_id
            .is_some()
    );
}

#[test]
fn request_help_and_option_validation_are_nonmutating_and_strict() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    fs::create_dir_all(home.join("state")).expect("home");
    let help = run(mx(&home).args(["request", "--help"]));
    assert!(help.status.success());
    assert!(
        String::from_utf8_lossy(&help.stdout)
            .contains("Delivery, acknowledgement, response and completion")
    );
    assert!(!home.join("state/request-inbox").exists());

    let invalid = run(mx(&home).args(["request", "submit", "--unknown", "value"]));
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("unknown submit option"));
    assert!(!home.join("state/request-inbox").exists());
}

#[test]
fn canonical_task_send_persists_shared_envelope_and_delivery_separately() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let state = home.join("state");
    let fakebin = temp.path().join("fakebin");
    fs::create_dir_all(&state).expect("state");
    fs::create_dir(&fakebin).expect("fakebin");
    let mut record = TaskRecord::new(
        "child".into(),
        AssignmentRole::Implementer,
        ArtifactKind::Implementation,
        false,
        "parent".into(),
        "parent".into(),
        home.to_string_lossy().into_owned(),
    );
    record.runtime.provider = "tmux".into();
    record.runtime.endpoint = Some("test:mx-child".into());
    let meta = write_meta(
        "window=test:mx-child\nkind=delivery\nharness=codex\n",
        &record,
    )
    .expect("canonical meta");
    fs::write(state.join("child.meta"), meta).expect("meta");
    multplx_domain::lifecycle::subagent_model::read_meta(
        "child",
        &fs::read_to_string(state.join("child.meta")).expect("read meta"),
    )
    .expect("read canonical meta");
    let tmux = fakebin.join("tmux");
    fs::write(
        &tmux,
        "#!/bin/sh\ncase \"${1:-}\" in display-message) echo fake-pane;; capture-pane) echo '│ > │';; send-keys) :;; *) exit 1;; esac\n",
    )
    .expect("tmux");
    fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755)).expect("tmux mode");
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let sent = run(mx(&home)
        .env("PATH", path)
        .env("MX_TASK_ID", "parent")
        .env("MX_SEND_SLEEP", "0")
        .env("MX_SEND_SETTLE", "0")
        .args(["send", "mx-child", "please", "review"]));
    assert!(
        sent.status.success(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );
    let receipts = fs::read_dir(state.join("message-outbox"))
        .expect("message outbox")
        .collect::<Result<Vec<_>, _>>()
        .expect("receipts");
    assert_eq!(receipts.len(), 1);
    let envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(receipts[0].path()).expect("envelope receipt"))
            .expect("envelope JSON");
    assert_eq!(envelope["sender"], "parent");
    assert_eq!(envelope["recipient"], "child");
    assert_eq!(envelope["task_id"], "child");
    assert_eq!(envelope["acknowledgement"], "delivered");
    let correlation = envelope["correlation_id"].as_str().expect("correlation");
    let pending =
        fs::read_to_string(state.join("pending-replies").join(correlation)).expect("pending reply");
    assert!(pending.contains(&format!("message_id=reply-{correlation}")));
    assert!(pending.contains("delivered_epoch="));
}

#[test]
fn concurrent_client_retry_and_partial_three_project_batch_preserve_each_item() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    fs::create_dir_all(home.join("state")).expect("home");
    let first = registered(&home, temp.path(), "api");
    let second = registered(&home, temp.path(), "web");
    let third = registered(&home, temp.path(), "docs");
    let barrier = Arc::new(Barrier::new(2));
    let outputs = thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let home = home.clone();
            let first = first.clone();
            handles.push(scope.spawn(move || {
                barrier.wait();
                run(&mut submit(&home, &first, "batch-item-api"))
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("client thread"))
            .collect::<Vec<_>>()
    });
    for output in &outputs {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(outputs[0].stdout, outputs[1].stdout);

    let accepted_web = run(&mut submit(&home, &second, "batch-item-web"));
    assert!(
        accepted_web.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted_web.stderr)
    );
    let mut unresolved = third.clone();
    unresolved.starting_revision = "f".repeat(40);
    let rejected_docs = run(&mut submit(&home, &unresolved, "batch-item-docs"));
    assert!(!rejected_docs.status.success());
    assert!(
        !home
            .join("state/request-inbox/batch-item-docs.json")
            .exists()
    );
    assert!(
        home.join("state/request-inbox/batch-item-api.json")
            .is_file()
    );
    assert!(
        home.join("state/request-inbox/batch-item-web.json")
            .is_file()
    );
    let accepted_docs = run(&mut submit(&home, &third, "batch-item-docs"));
    assert!(
        accepted_docs.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted_docs.stderr)
    );

    let foreign_home = temp.path().join("foreign-home");
    fs::create_dir_all(foreign_home.join("state/request-inbox")).expect("foreign inbox");
    fs::create_dir_all(foreign_home.join("state/request-outbox")).expect("foreign outbox");
    for directory in ["request-inbox", "request-outbox"] {
        fs::copy(
            home.join(format!("state/{directory}/batch-item-api.json")),
            foreign_home.join(format!("state/{directory}/batch-item-api.json")),
        )
        .expect("copy foreign receipt");
    }
    let foreign = run(mx(&foreign_home).args(["request", "get", "batch-item-api"]));
    assert!(!foreign.status.success());
    assert!(String::from_utf8_lossy(&foreign.stderr).contains("different recipient home"));

    for (request, binding) in [
        ("batch-item-api", &first),
        ("batch-item-web", &second),
        ("batch-item-docs", &third),
    ] {
        let receipt: serde_json::Value = serde_json::from_slice(
            &fs::read(home.join(format!("state/request-inbox/{request}.json")))
                .expect("request receipt"),
        )
        .expect("receipt JSON");
        assert_eq!(receipt["project_id"], binding.project_id);
        assert_eq!(receipt["checkout_id"], binding.checkout_id);
        assert_eq!(receipt["starting_revision"], binding.starting_revision);
    }
}

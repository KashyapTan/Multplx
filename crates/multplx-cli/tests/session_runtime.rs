use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(command: &mut Command) -> Output {
    command.output().expect("run mx")
}

fn mx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mx"))
}

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent");
    fs::write(path, body).expect("write script");
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("mode");
}

fn fake_tools(root: &Path) -> PathBuf {
    let path = root.join("tools");
    executable(&path.join("jq"), "#!/bin/sh\nexit 0\n");
    executable(&path.join("node"), "#!/bin/sh\nexit 0\n");
    path
}

fn snapshot_fixture(home: &Path) -> serde_json::Value {
    serde_json::json!({
        "schema": "mx-system-snapshot.v1",
        "generated": "2026-08-12T12:00:00Z",
        "mx_home": home,
        "roots": {
            "mx_root": home,
            "state": home.join("state"),
            "data": home.join("data"),
            "config": home.join("config"),
            "projects": home.join("projects")
        },
        "backlog": {
            "path": home.join("data/backlog.md"),
            "present": true,
            "records": [
                {"state":"queued","id":"next","title":"Next task","repo":"demo","kind":"delivery","blocked_by":"hold-1","blocked_reason":"approval","pr_url":null,"report_path":null,"local_note":null},
                {"state":"done","id":"landed","title":"Landed task","repo":"demo","kind":"delivery","blocked_by":null,"pr_url":"https://example.invalid/pr/1","report_path":null,"local_note":null}
            ]
        },
        "tasks": [
            {
                "id":"actor-1","kind":"delivery","project":"demo","backend":"tmux",
                "current_state":{"state":"working","source":"native"},
                "endpoint":{"target":"pane-1","exists":true,"agent_alive":"not_checked"},
                "pr":{"url":null},
                "paths":{"home":{"path":null,"present":false},"worktree":{"path":"/work/actor-1","present":true},"report":{"path":"/reports/actor-1.md","present":true}},
                "actions":{"watch":"bin/mx-peek actor-1","send":null},
                "backlog":{"state":"in_flight","id":"actor-1","title":"Actor","repo":"demo","kind":"delivery"}
            },
            {
                "id":"daemon-1","kind":"daemon","project":"demo","backend":"herdr",
                "current_state":{"state":"paused","source":"report"},
                "endpoint":{"target":"pane-2","exists":false,"agent_alive":"dead"},
                "pr":{"url":"https://example.invalid/pr/2"},
                "paths":{"home":{"path":"/homes/daemon-1","present":false},"worktree":{"path":null,"present":false},"report":{"path":null,"present":false}},
                "actions":{"watch":"bin/mx-peek daemon-1","send":"bin/mx-send daemon-1"},
                "backlog":null
            }
        ],
        "main_inventory":{"valid":true,"reason":null,"orphan_in_flight":[],"unstructured_current_count":0},
        "scout_reports":[],
        "watcher":{"lock_present":false,"pid":null,"identity_verified":false,"alive":false,"beacon_age_secs":null,"stale":false,"afk":false},
        "wake_queue":{"depth":0,"oldest_age_secs":null},
        "dispatch_queue":{"depth":0,"records":[],"available":true,"reason":null},
        "headroom":null,
        "headroom_reason":"not requested",
        "vplan_reviews":{"records":[]},
        "later_feeds":{
            "gate_runs":{"supported":true,"available":true,"records":[]},
            "workflow_runs":{"supported":true,"available":true,"records":[]},
            "deliveries":{"supported":true,"available":true,"records":[]},
            "upstream_drift":{},
            "doctor":{"available":true},
            "timeline":{"available":true}
        },
        "daemon_current":{"registry":{},"records":[],"total_registered":0,"total":0,"shown":0,"truncated":0},
        "daemon_landed":{"records":[],"truncated":[],"unreadable":[],"partial":[]},
        "daemon_guidance":{"note":"No registered daemons."},
        "future_additive_field":{"safe":"ignored"}
    })
}

#[test]
fn session_start_dispatch_is_native_and_rejects_legacy_probe_arguments() {
    let temp = tempfile::tempdir().expect("tempdir");
    executable(
        &temp.path().join("bin/mx-session-start.sh"),
        "#!/bin/sh\nprintf 'retained session shell ran\\n'\n",
    );
    let output = run(mx()
        .env("MX_ROOT_OVERRIDE", temp.path())
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args(["session", "mx-session-start.sh", "probe"]));
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not accept arguments"));
}

#[test]
fn installed_runtime_binding_reaches_session_lock_and_explicit_paths_win() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("runtime with spaces");
    let home = temp.path().join("configured home");
    let config = temp.path().join("launcher config");
    let binary = root.join("target/release/mx");
    fs::create_dir_all(binary.parent().expect("binary parent")).expect("binary directory");
    fs::create_dir_all(root.join("bin")).expect("runtime bin");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&config).expect("config");
    fs::copy(env!("CARGO_BIN_EXE_mx"), &binary).expect("copy packaged binary");
    fs::write(
        root.join(".multplx-release"),
        format!("{}\n", env!("CARGO_PKG_VERSION")),
    )
    .expect("version record");
    fs::write(
        root.join(".multplx-config"),
        format!("{}\n", config.display()),
    )
    .expect("runtime binding");
    fs::write(config.join("root"), format!("{}\n", root.display())).expect("root record");
    fs::write(config.join("home"), format!("{}\n", home.display())).expect("home record");
    let log = temp.path().join("lock-paths.log");
    executable(
        &root.join("bin/mx-lock.sh"),
        "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$MX_ROOT_OVERRIDE\" \"$MX_HOME\" \"$MX_STATE_OVERRIDE\" >>\"$PROBE_LOG\"\nprintf 'fixture lock unavailable\\n'\nexit 1\n",
    );
    executable(
        &root.join("bin/mx-bootstrap.sh"),
        "#!/bin/sh\nprintf 'fixture bootstrap\\n'\n",
    );
    executable(&root.join("bin/mx-guard.sh"), "#!/bin/sh\nexit 0\n");

    let output = run(mx_with_path(&binary)
        .env_remove("MX_ROOT_OVERRIDE")
        .env_remove("MX_RUST_SOURCE_ROOT")
        .env_remove("MX_HOME")
        .env_remove("MX_STATE_OVERRIDE")
        .env("PROBE_LOG", &log)
        .args(["session", "mx-session-start.sh"]));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&log).expect("default lock paths").trim(),
        format!(
            "{}|{}|{}",
            root.canonicalize().expect("canonical root").display(),
            home.canonicalize().expect("canonical home").display(),
            home.canonicalize()
                .expect("canonical home")
                .join("state")
                .display()
        )
    );

    fs::write(config.join("home"), "relative-home\n").expect("invalid home record");
    let invalid = run(mx_with_path(&binary).args(["backlog", "add", "must-not-write", "Unsafe"]));
    assert_eq!(invalid.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid installed runtime binding"));
    assert!(!temp.path().join("data/backlog.md").exists());

    let explicit_root = temp.path().join("worker runtime");
    let explicit_home = temp.path().join("worker home");
    let explicit_state = temp.path().join("worker state");
    executable(
        &explicit_root.join("bin/mx-lock.sh"),
        "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$MX_ROOT_OVERRIDE\" \"$MX_HOME\" \"$MX_STATE_OVERRIDE\" >>\"$PROBE_LOG\"\nprintf 'fixture lock unavailable\\n'\nexit 1\n",
    );
    executable(
        &explicit_root.join("bin/mx-bootstrap.sh"),
        "#!/bin/sh\nprintf 'fixture bootstrap\\n'\n",
    );
    executable(
        &explicit_root.join("bin/mx-guard.sh"),
        "#!/bin/sh\nexit 0\n",
    );
    let output = run(mx_with_path(&binary)
        .env_remove("MX_RUST_SOURCE_ROOT")
        .env("MX_ROOT_OVERRIDE", &explicit_root)
        .env("MX_HOME", &explicit_home)
        .env("MX_STATE_OVERRIDE", &explicit_state)
        .env("PROBE_LOG", &log)
        .args(["session", "mx-session-start.sh"]));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = fs::read_to_string(&log).expect("override lock paths");
    assert_eq!(
        lines.lines().nth(1),
        Some(
            format!(
                "{}|{}|{}",
                explicit_root.display(),
                explicit_home.display(),
                explicit_state.display()
            )
            .as_str()
        )
    );
}

fn mx_with_path(path: &Path) -> Command {
    Command::new(path)
}

#[test]
fn bootstrap_dispatch_is_native_before_any_legacy_script_can_run() {
    let temp = tempfile::tempdir().expect("tempdir");
    executable(
        &temp.path().join("bin/mx-bootstrap.sh"),
        "#!/bin/sh\nprintf 'legacy bootstrap ran\\n'\n",
    );
    let output = run(mx()
        .env("MX_ROOT_OVERRIDE", temp.path())
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args(["session", "mx-bootstrap.sh", "surprise"]));
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("legacy bootstrap ran"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage: mx-bootstrap.sh"));
}

#[test]
fn native_supervision_renderer_is_deterministic_and_path_aware() {
    let temp = tempfile::tempdir().expect("tempdir");
    let protocols = temp.path().join("docs/supervision-protocols");
    fs::create_dir_all(&protocols).expect("protocols");
    fs::write(
        protocols.join("pi.md"),
        "Mode: Pi. __MX_PI_TURNEND_EXT__ __MX_PI_EXT__\n",
    )
    .expect("pi protocol");
    fs::write(protocols.join("unknown.md"), "Mode: Unknown.\n").expect("unknown protocol");
    let output = run(mx()
        .env("MX_ROOT_OVERRIDE", "/logical/home")
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args([
            "session",
            "mx-supervision-instructions.sh",
            "--harness",
            "pi",
            "--read-only",
            "1",
        ]));
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("UTF-8");
    assert!(text.contains("primary harness: pi"));
    assert!(text.contains("- Lock: read-only"));
    assert!(text.contains("/logical/home/.pi/extensions/mx-primary-turnend-guard.ts"));
    assert!(text.contains("/logical/home/.pi/extensions/mx-primary-pi-watch.ts"));
}

#[test]
fn unknown_and_missing_session_entries_fail_before_execution() {
    let unknown = run(mx().args(["session", "not-an-entry"]));
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown session entry point"));
}

fn supervision_status_command(home: &Path, harness: &str) -> Command {
    let mut command = mx();
    command
        .env(
            "MX_RUST_SOURCE_ROOT",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        )
        .env("MX_ROOT_OVERRIDE", home)
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", home.join("state"))
        .env_remove("MX_CODEX_IDLE_CLI")
        .env_remove("MX_GUARD_GRACE")
        .args([
            "session",
            "mx-supervision-instructions.sh",
            "--harness",
            harness,
            "--status",
        ]);
    command
}

fn status_fixture_bytes(state: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(state)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                PathBuf::from(entry.file_name()),
                fs::read(entry.path()).expect("fixture state remains regular files"),
            )
        })
        .collect()
}

fn read_supervision_status(command: &mut Command) -> serde_json::Value {
    let output = run(command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "status is a read-only projection");
    serde_json::from_slice(&output.stdout).expect("status JSON")
}

#[test]
fn native_status_retains_pending_work_without_arming_or_consuming_it() {
    let home = tempfile::tempdir().expect("home");
    let state = home.path().join("state");
    fs::create_dir(&state).unwrap();
    let empty = read_supervision_status(&mut supervision_status_command(home.path(), "pi"));
    assert_eq!(
        empty["needed"], false,
        "an available idle home has no invented work"
    );
    assert_eq!(empty["watcher_fresh"], false);
    assert_eq!(empty["queue_pending"], false);
    assert!(empty["failure"].is_null());
    assert_eq!(
        fs::read_dir(&state).unwrap().count(),
        0,
        "status started or registered supervision"
    );

    // A real pending record must remain visible without a healthy watcher.
    let wake = "1\t1\tsignal\tworker\tdone\n";
    fs::write(state.join(".wake-queue"), wake).unwrap();
    fs::write(state.join(".lock"), std::process::id().to_string()).unwrap();
    let unchanged = status_fixture_bytes(&state);
    let pending = read_supervision_status(&mut supervision_status_command(home.path(), "pi"));
    assert_eq!(pending["needed"], true);
    assert_eq!(pending["queue_pending"], true);
    assert_eq!(pending["watcher_fresh"], false);
    assert_eq!(status_fixture_bytes(&state), unchanged);
    assert_eq!(fs::read_to_string(state.join(".wake-queue")).unwrap(), wake);
    assert_eq!(
        fs::read_dir(&state).unwrap().count(),
        2,
        "read-only status created claims or watcher state"
    );

    // Corrupt retained wake bytes require reconciliation, rather than a quiet
    // or healthy projection, and must not be rewritten by this inspector.
    fs::write(state.join(".wake-queue"), "unparseable retained wake\n").unwrap();
    let unchanged = status_fixture_bytes(&state);
    let malformed = read_supervision_status(&mut supervision_status_command(home.path(), "pi"));
    assert_eq!(malformed["needed"], true);
    assert_eq!(malformed["queue_pending"], true);
    assert_eq!(malformed["watcher_fresh"], false);
    assert_eq!(status_fixture_bytes(&state), unchanged);
    assert_eq!(
        fs::read_to_string(state.join(".wake-queue")).unwrap(),
        "unparseable retained wake\n"
    );
}

#[test]
fn native_status_separates_watcher_freshness_from_delivery_failure() {
    let home = tempfile::tempdir().expect("home");
    let state = home.path().join("state");
    fs::create_dir(&state).unwrap();
    fs::write(state.join("actor.meta"), "id=actor\n").unwrap();
    fs::write(state.join(".last-watcher-beat"), "fixture heartbeat\n").unwrap();
    let beacon = fs::File::options()
        .write(true)
        .open(state.join(".last-watcher-beat"))
        .unwrap();
    beacon
        .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
        .unwrap();
    let unchanged = status_fixture_bytes(&state);
    let stale = read_supervision_status(&mut supervision_status_command(home.path(), "pi"));
    assert_eq!(stale["needed"], true);
    assert_eq!(status_fixture_bytes(&state), unchanged);
    assert_eq!(
        stale["watcher_fresh"], false,
        "retained stale beacon was called fresh"
    );

    fs::write(
        state.join(".last-watcher-beat"),
        "fresh fixture heartbeat\n",
    )
    .unwrap();
    let failure = "Pi follow-up delivery failed: fixture transport rejected notification\n";
    fs::write(state.join(".pi-watch-failure"), failure).unwrap();
    let mut command = supervision_status_command(home.path(), "pi");
    command
        .env_remove("MX_STATE_OVERRIDE")
        .env("MX_GUARD_GRACE", "invalid");
    let unchanged = status_fixture_bytes(&state);
    let fresh = read_supervision_status(&mut command);
    assert_eq!(status_fixture_bytes(&state), unchanged);
    assert_eq!(
        fresh["watcher_fresh"], true,
        "invalid grace lost the documented default"
    );
    assert_eq!(
        fresh["failure"], failure,
        "ready watcher hid retained transport failure"
    );
    assert!(
        fresh["native_delivery"]
            .as_str()
            .unwrap()
            .contains("watcher freshness alone is insufficient")
    );
    assert_eq!(
        fs::read_to_string(state.join(".pi-watch-failure")).unwrap(),
        failure
    );

    let claude = read_supervision_status(&mut supervision_status_command(home.path(), "claude"));
    assert!(
        claude["failure"].is_null(),
        "Pi's failure was attributed to another provider"
    );
    assert_eq!(claude["watcher_fresh"], true);
    fs::write(state.join(".afk"), "fixture away owner\n").unwrap();
    let mut command = supervision_status_command(home.path(), "unverified-provider");
    command.env("MX_GUARD_GRACE", "0");
    let unchanged = status_fixture_bytes(&state);
    let away = read_supervision_status(&mut command);
    assert_eq!(status_fixture_bytes(&state), unchanged);
    assert_eq!(away["harness"], "unknown");
    assert_eq!(away["away"], true);
    assert_eq!(
        away["watcher_fresh"], false,
        "explicit zero grace was ignored"
    );
    assert!(away["failure"].is_null());
}

#[test]
fn native_nudge_and_supervision_cover_scope_lock_and_usage_edges() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let state = root.join("state");
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir(&state).expect("state");
    fs::write(root.join("AGENTS.md"), "# contract\n").expect("contract");
    fs::write(root.join(".mx-daemon-home"), "daemon-1\n").expect("marker");

    let nudge = run(mx()
        .env_remove("DEEP_REVIEW_GATE")
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", &root)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["session", "mx-sessionstart-nudge.sh"]));
    assert!(nudge.status.success());
    assert!(String::from_utf8_lossy(&nudge.stdout).contains(
        "Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions."
    ));

    fs::write(state.join(".lock"), format!("{}\n", std::process::id())).expect("lock");
    let locked = run(mx()
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", &root)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["session", "mx-sessionstart-nudge.sh"]));
    assert!(locked.status.success());
    assert!(locked.stdout.is_empty());

    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let help = run(mx().env("MX_RUST_SOURCE_ROOT", &source).args([
        "session",
        "mx-supervision-instructions.sh",
        "--help",
    ]));
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
    let invalid = run(mx().env("MX_RUST_SOURCE_ROOT", &source).args([
        "session",
        "mx-supervision-instructions.sh",
        "--unexpected",
    ]));
    assert_eq!(invalid.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("unknown argument"));
    let repair = run(mx()
        .env("MX_RUST_SOURCE_ROOT", &source)
        .env_remove("MX_CODEX_IDLE_CLI")
        .env("MX_CODEX_WATCH_CHECKPOINT", "45")
        .args([
            "session",
            "mx-supervision-instructions.sh",
            "--harness",
            "codex",
            "--repair-line",
            "--queue-pending",
            "1",
        ]));
    assert!(repair.status.success());
    assert_eq!(
        String::from_utf8_lossy(&repair.stdout),
        "After claiming queued wakes and durably recording disposition plus acknowledgement, the Codex queue bridge is inactive or native hooks are not ready here; restore bounded foreground supervision with bin/mx-watch-checkpoint.sh --seconds 45. Managed Multplx CLI launches set activation automatically; direct Codex CLI requires explicit MX_CODEX_IDLE_CLI=1 opt-in. Complete the provider's native hook trust review and ensure hooks are enabled; a matched SessionStart readiness receipt is required before queue delivery. Desktop event delivery is unverified.\n"
    );
    for (harness, extra, expected) in [
        (
            "claude",
            vec!["--read-only", "1"],
            "Watcher repair belongs to the session holding the system lock",
        ),
        (
            "pi",
            vec!["--afk", "1"],
            "Away mode owns watcher supervision",
        ),
        (
            "unknown-harness",
            Vec::new(),
            "according to the session-start block",
        ),
    ] {
        let mut arguments = vec![
            "session",
            "mx-supervision-instructions.sh",
            "--harness",
            harness,
            "--repair-line",
        ];
        arguments.extend(extra);
        let output = run(mx().env("MX_RUST_SOURCE_ROOT", &source).args(arguments));
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
    }
}

#[test]
fn native_system_view_ignores_shell_bodies_and_preserves_rendering_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    for directory in ["bin", "config", "data", "projects", "state"] {
        fs::create_dir_all(root.join(directory)).expect("runtime directory");
    }
    let marker = root.join("legacy-snapshot-ran");
    let native = root.join("state/native-delegations");
    fs::create_dir(&native).expect("native evidence directory");
    fs::write(
        native.join("native-one.json"),
        br#"{"schema":"mx-native-delegation-evidence.v1","task_id":null,"accepted":true,"rejection":null,"observation":{"observation_id":"native-one","provider":"codex","child_id":"child-one","parent_session_id":"session-one","turn_id":"turn-one","parent_attempt":null,"state":"started","observed_at":"2026-09-15T00:00:00Z","artifact":null,"recovery":"session-bound"}}"#,
    )
    .expect("native evidence");
    fs::write(native.join("malformed.json"), b"{").expect("malformed evidence");
    fs::write(native.join("ignored.txt"), b"ignored").expect("ignored evidence");
    let inbox = root.join("state/wake-inbox");
    fs::create_dir(&inbox).expect("wake inbox");
    let waiting_path = inbox.join("wake-00000000000000000001.json");
    let claimed_path = inbox.join("wake-00000000000000000002.json");
    fs::write(
        &waiting_path,
        serde_json::to_vec(&serde_json::json!({
            "schema":"mx-wake-inbox.v1",
            "event_id":"wake-00000000000000000001",
            "record":{"epoch":1,"sequence":1,"kind":"signal","key":"waiting","payload":"waiting payload"},
            "claim":{"owner":{"pid":std::process::id(),"marker":"snapshot-test"},"claimed_at":2},
            "disposition":{"kind":"waiting","recorded_at":3,"detail":"awaiting review","condition":"review","resume_trigger":"review-1","recheck_after_epoch":9999999999_u64,"follow_up_id":null},
            "disposition_history":[],
            "acknowledged_at":4
        }))
        .expect("waiting JSON"),
    )
    .expect("waiting inbox item");
    fs::write(
        &claimed_path,
        serde_json::to_vec(&serde_json::json!({
            "schema":"mx-wake-inbox.v1",
            "event_id":"wake-00000000000000000002",
            "record":{"epoch":2,"sequence":2,"kind":"check","key":"claimed","payload":"claimed payload"},
            "claim":{"owner":{"pid":std::process::id(),"marker":"snapshot-test"},"claimed_at":3},
            "disposition":null,
            "disposition_history":[],
            "acknowledged_at":null
        }))
        .expect("claimed JSON"),
    )
    .expect("claimed inbox item");
    let waiting_before = fs::read(&waiting_path).expect("waiting before snapshot");
    let claimed_before = fs::read(&claimed_path).expect("claimed before snapshot");
    executable(
        &root.join("bin/mx-system-snapshot.sh"),
        "#!/bin/sh\ntouch \"$MX_LEGACY_MARKER\"\nexit 91\n",
    );
    let tools = fake_tools(&root);
    let fixture = snapshot_fixture(&root);
    let bytes = serde_json::to_vec(&fixture).expect("snapshot JSON");
    let parsed = multplx_domain::snapshot::parse_system_snapshot(&bytes).expect("typed snapshot");
    let text = multplx_domain::snapshot::render_system_view(&parsed);
    assert!(text.contains("| actor-1 | working / native | delivery | demo | tmux | present |"));
    assert!(
        text.contains("| daemon-1 | paused / report | daemon | demo | herdr | absent / dead |")
    );
    assert!(text.contains("| next | Next task | demo | delivery | hold-1 - approval | - |"));
    assert!(
        text.contains(
            "| landed | Landed task | demo | delivery | - | https://example.invalid/pr/1 |"
        )
    );
    assert!(text.ends_with("## Standing agents\nNo registered daemons.\n"));

    let json = run(mx()
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", &root)
        .env("MX_RUST_SOURCE_ROOT", &root)
        .env("MX_LEGACY_MARKER", &marker)
        .args(["session", "mx-system-view.sh", "--json"]));
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let snapshot =
        serde_json::from_slice::<serde_json::Value>(&json.stdout).expect("native snapshot JSON");
    assert_eq!(snapshot["schema"], "mx-system-snapshot.v1");
    assert_eq!(
        snapshot["native_delegations"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        snapshot["native_delegations"][0]["observation"]["observation_id"],
        "native-one"
    );
    assert_eq!(snapshot["wake_queue"]["depth"], 2);
    assert_eq!(snapshot["wake_queue"]["available"], true);
    assert_eq!(
        snapshot["wake_queue"]["records"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        fs::read(&waiting_path).expect("waiting after snapshot"),
        waiting_before
    );
    assert_eq!(
        fs::read(&claimed_path).expect("claimed after snapshot"),
        claimed_before
    );
    assert!(!root.join("state/.wake-queue.lock").exists());
    assert!(
        !marker.exists(),
        "system view executed a retained shell body"
    );

    let view = run(mx()
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", &root)
        .env("MX_RUST_SOURCE_ROOT", &root)
        .env("MX_LEGACY_MARKER", &marker)
        .args(["session", "mx-system-view.sh"]));
    assert!(
        view.status.success(),
        "{}",
        String::from_utf8_lossy(&view.stderr)
    );
    assert!(String::from_utf8_lossy(&view.stdout).contains("No live task metadata found."));
    assert!(
        !marker.exists(),
        "system view executed a retained shell body"
    );

    let help = run(mx().args(["session", "mx-system-view.sh", "--help"]));
    assert!(help.status.success());
    let usage = run(mx().args(["session", "mx-system-view.sh", "--bad"]));
    assert_eq!(usage.status.code(), Some(2));

    let empty_tools = root.join("empty-tools");
    fs::create_dir(&empty_tools).expect("empty tools");
    let no_jq = run(mx()
        .env("PATH", &empty_tools)
        .args(["session", "mx-system-view.sh"]));
    assert_eq!(no_jq.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&no_jq.stderr),
        "mx-system-view: jq not found\n"
    );

    let invalid = multplx_domain::snapshot::parse_system_snapshot(b"{}\n");
    assert!(invalid.is_err());

    let mut empty = fixture.clone();
    empty["tasks"] = serde_json::json!([]);
    empty["backlog"]["records"] = serde_json::json!([]);
    let empty_bytes = serde_json::to_vec(&empty).expect("empty JSON");
    let empty_snapshot =
        multplx_domain::snapshot::parse_system_snapshot(&empty_bytes).expect("empty snapshot");
    let empty_text = multplx_domain::snapshot::render_system_view(&empty_snapshot);
    assert!(empty_text.contains("No live task metadata found."));
    assert!(empty_text.contains("No queued backlog records found."));
    assert!(empty_text.contains("No done backlog records found."));

    let mut edges = fixture;
    edges["tasks"][0]["endpoint"]["exists"] = serde_json::Value::Null;
    edges["tasks"][0]["paths"]["home"] =
        serde_json::json!({"path":"/homes/actor-1","present":true});
    let mut absent_worktree = edges["tasks"][0].clone();
    absent_worktree["id"] = serde_json::json!("actor-2");
    absent_worktree["paths"]["home"] = serde_json::json!({"path":null,"present":false});
    absent_worktree["paths"]["worktree"] =
        serde_json::json!({"path":"/work/actor-2","present":false});
    let mut no_path = absent_worktree.clone();
    no_path["id"] = serde_json::json!("actor-3");
    no_path["paths"]["worktree"] = serde_json::json!({"path":null,"present":false});
    edges["tasks"]
        .as_array_mut()
        .expect("tasks")
        .extend([absent_worktree, no_path]);
    edges["backlog"]["records"][0]["blocked_reason"] = serde_json::Value::Null;
    let edge_bytes = serde_json::to_vec(&edges).expect("edge JSON");
    let edge_snapshot =
        multplx_domain::snapshot::parse_system_snapshot(&edge_bytes).expect("edge snapshot");
    let edge_text = multplx_domain::snapshot::render_system_view(&edge_snapshot);
    assert!(
        edge_text.contains("| actor-1 | working / native | delivery | demo | tmux | unknown |")
    );
    assert!(edge_text.contains("/work/actor-2 (absent)"));
    assert!(edge_text.contains("| next | Next task | demo | delivery | hold-1 | - |"));
}

#[test]
fn native_timeline_covers_filters_malformed_rows_and_private_html() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let state = root.join("state");
    let data = root.join("data");
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir(&state).expect("state");
    fs::create_dir(&data).expect("data");
    let fixture = include_str!("../../../tests/fixtures/timeline.journal.jsonl");
    fs::write(state.join("timeline-fixture.journal"), fixture).expect("journal");
    executable(&root.join("bin/mx-vplan.sh"), "#!/bin/sh\nexit 0\n");
    let tools = fake_tools(&root);
    let command = || {
        let mut command = mx();
        command
            .env("PATH", &tools)
            .env("MX_ROOT_OVERRIDE", &root)
            .env("MX_HOME", &root)
            .env("MX_STATE_OVERRIDE", &state)
            .env("MX_DATA_OVERRIDE", &data)
            .env("MX_RUST_SOURCE_ROOT", &root)
            .args(["session", "mx-timeline.sh", "timeline-fixture"]);
        command
    };

    let text = run(&mut command());
    assert!(text.status.success());
    assert_eq!(
        String::from_utf8_lossy(&text.stdout),
        include_str!("../../../tests/fixtures/timeline.golden")
    );
    let filtered = run(command().args(["--event", "gate.*", "--json"]));
    assert!(filtered.status.success());
    assert_eq!(String::from_utf8_lossy(&filtered.stdout).lines().count(), 1);
    assert!(String::from_utf8_lossy(&filtered.stdout).contains("gate.step.finished"));
    let since = run(command().args(["--since", "2026-07-30T12:06:00Z"]));
    assert!(since.status.success());
    assert_eq!(String::from_utf8_lossy(&since.stdout).lines().count(), 3);
    let duration = run(command()
        .env("MX_TIMELINE_NOW_MS", "1785413700000")
        .args(["--since", "5m"]));
    assert!(duration.status.success());
    assert_eq!(String::from_utf8_lossy(&duration.stdout).lines().count(), 2);

    fs::write(
        state.join("timeline-fixture.journal"),
        format!("{fixture}{{broken\n"),
    )
    .expect("malformed journal");
    let malformed = run(&mut command());
    assert!(malformed.status.success());
    assert_eq!(
        String::from_utf8_lossy(&malformed.stderr),
        "mx-timeline: skipped 1 malformed journal line(s)\n"
    );

    let html = run(command().args(["--html"]));
    assert!(html.status.success());
    let artifact = PathBuf::from(String::from_utf8_lossy(&html.stdout).trim());
    assert!(
        fs::read_to_string(&artifact)
            .expect("artifact")
            .contains("<!DOCTYPE html>")
    );
    assert_eq!(
        fs::metadata(&artifact)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let invalid = run(command().args(["--since", "never"]));
    assert_eq!(invalid.status.code(), Some(1));
    let conflict = run(command().args(["--json", "--html"]));
    assert_eq!(conflict.status.code(), Some(1));
    let unknown = run(command().args(["--unknown"]));
    assert_eq!(unknown.status.code(), Some(1));
    let missing_since = run(command().args(["--since"]));
    assert_eq!(missing_since.status.code(), Some(1));
    let help = run(mx().args(["session", "mx-timeline.sh", "--help"]));
    assert!(help.status.success());
    let no_id = run(mx().args(["session", "mx-timeline.sh"]));
    assert_eq!(no_id.status.code(), Some(2));
    let missing = run(mx()
        .env("PATH", &tools)
        .env("MX_HOME", &root)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["session", "mx-timeline.sh", "absent"]));
    assert_eq!(missing.status.code(), Some(1));

    executable(&root.join("bin/mx-vplan.sh"), "#!/bin/sh\nexit 1\n");
    let invalid_vplan = run(command().args(["--html"]));
    assert_eq!(invalid_vplan.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid_vplan.stderr).contains("unavailable or invalid"));
    fs::remove_file(root.join("bin/mx-vplan.sh")).expect("remove vplan");
    let missing_vplan = run(command().args(["--html"]));
    assert_eq!(missing_vplan.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing_vplan.stderr).contains("vplan module is unavailable"));

    let jq_only = root.join("jq-only");
    fs::create_dir(&jq_only).expect("jq-only");
    executable(&jq_only.join("jq"), "#!/bin/sh\nexit 0\n");
    let no_node = run(mx()
        .env("PATH", &jq_only)
        .env("MX_HOME", &root)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["session", "mx-timeline.sh", "timeline-fixture"]));
    assert_eq!(no_node.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&no_node.stderr).contains("node is required"));
}

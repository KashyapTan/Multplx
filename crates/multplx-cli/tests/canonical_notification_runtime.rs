use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use multplx_domain::lifecycle::subagent_model::{
    ArtifactKind, AssignmentRole, TaskRecord, write_meta,
};

fn run(command: &mut Command) -> Output {
    command.output().expect("run mx")
}

fn mx(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    command
        .env("MX_HOME", home)
        .env("MX_ROOT_OVERRIDE", home)
        .env("MX_STATE_OVERRIDE", home.join("state"))
        .env("MX_REPORT_STATE_OVERRIDE", home.join("state"))
        .env("MX_NUDGE", "0");
    command
}

fn canonical_task(home: &Path, id: &str) -> TaskRecord {
    fs::create_dir_all(home.join("state")).expect("state");
    let root = format!("root-home:{}", home.display());
    let mut record = TaskRecord::new(
        id.into(),
        AssignmentRole::Implementer,
        ArtifactKind::Implementation,
        false,
        root.clone(),
        root,
        home.to_string_lossy().into_owned(),
    );
    record.owner_state = Some(home.join("state").to_string_lossy().into_owned());
    record.parent_state = record.owner_state.clone();
    let text = write_meta("kind=delivery\n", &record).expect("canonical metadata");
    fs::write(home.join("state").join(format!("{id}.meta")), text).expect("metadata");
    record
}

fn write_task(home: &Path, record: &TaskRecord) {
    fs::create_dir_all(home.join("state")).expect("state");
    fs::write(
        home.join("state").join(format!("{}.meta", record.task_id)),
        write_meta("", record).expect("canonical metadata"),
    )
    .expect("metadata");
}

// This target isolates a canonical handoff from unrelated concurrent parent
// subprocess fixtures. Its production wrappers and one-second bound stay real.
#[test]
fn canonical_question_and_done_close_watcher_without_republishing_wakes() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let mut task = canonical_task(&home, "worker");
    task.artifact = ArtifactKind::Report;
    write_task(&home, &task);
    fs::create_dir_all(home.join("bin")).unwrap();
    fs::write(home.join("AGENTS.md"), "isolated fixture").unwrap();
    fs::write(home.join(".mx-daemon-home"), "fixture-parent\n").unwrap();
    let artifact = home.join("result.md");
    fs::write(&artifact, "completed report").unwrap();
    let attempt = task.attempt.as_ref().unwrap();
    let report = |state: &str, id: &str| {
        let mut command = mx(&home);
        command
            .env("MX_TASK_ID", "worker")
            .env("MX_ATTEMPT_ID", &attempt.id)
            .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
            .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string());
        command.args([
            "supervision",
            "mx-report",
            "--state",
            state,
            "--message",
            "isolated report",
            "--message-id",
            id,
        ]);
        if state == "needs-decision" {
            command.args(["--key", "choice"]);
        }
        if state == "done" {
            command.arg("--artifact").arg(&artifact);
        }
        let result = run(&mut command);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        result
    };
    let watch = || {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        run(mx(&home)
            .env("MX_RUST_SOURCE_ROOT", source)
            .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
            .env("MX_SIGNAL_GRACE", "0")
            .env("MX_POLL", "0.05")
            .env("MX_HEARTBEAT", "999999")
            .env("MX_CHECK_INTERVAL", "999999")
            .args(["supervision", "mx-watch-checkpoint.sh", "--seconds", "1"]))
    };
    for (state, id) in [("needs-decision", "question"), ("done", "completion")] {
        report(state, id);
        let before = fs::read(home.join("state/.wake-queue")).unwrap();
        let output = watch();
        assert!(
            output.status.success(),
            "{state}: code={:?}; stdout={}; stderr={}; queue={}; metadata={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&before),
            fs::read_to_string(home.join("state/worker.meta")).unwrap(),
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("signal:"));
        assert_eq!(
            fs::read(home.join("state/.wake-queue")).unwrap(),
            before,
            "native handoff duplicated canonical wake"
        );
        use multplx_core::process::{ProcessProbe, SystemProcessProbe};
        use multplx_core::wake::{WakeDisposition, WakeDispositionKind, WakeQueue};
        let probe = SystemProcessProbe::default();
        let owner = probe.identity(std::process::id()).unwrap();
        let queue = WakeQueue::new(home.join("state"));
        let now = std::time::SystemTime::now();
        for item in queue.claim_available(&owner, now, 64, &probe).unwrap() {
            queue
                .record_disposition(
                    &item.event_id,
                    &owner,
                    WakeDisposition {
                        kind: WakeDispositionKind::Handled,
                        recorded_at: 1,
                        detail: "verified canonical handoff".into(),
                        condition: None,
                        resume_trigger: None,
                        recheck_after_epoch: None,
                        follow_up_id: None,
                    },
                    &probe,
                )
                .unwrap();
            queue
                .acknowledge(&item.event_id, &owner, now, &probe)
                .unwrap();
        }
        // Acknowledged reports must stay quiet even across endpoint rescan.
        fs::remove_file(home.join("state/.seen-worker_status")).unwrap();
        let quiet = watch();
        assert_eq!(
            quiet.status.code(),
            Some(124),
            "{}",
            String::from_utf8_lossy(&quiet.stdout)
        );
        assert!(!String::from_utf8_lossy(&quiet.stdout).contains("signal:"));
    }
}

// A durable accepted report must not be hidden behind unrelated maintenance.
// This reproduces the production ordering cause while preserving the same
// one-second checkpoint, real report/watcher owners and no duplicate wake.
#[test]
fn canonical_wake_precedes_blocked_dispatch_maintenance() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let mut task = canonical_task(&home, "worker");
    task.artifact = ArtifactKind::Report;
    write_task(&home, &task);
    let artifact = home.join("result.md");
    fs::write(&artifact, "completed fixture").unwrap();
    let attempt = task.attempt.as_ref().unwrap();
    let result = run(mx(&home)
        .env("MX_TASK_ID", "worker")
        .env("MX_ATTEMPT_ID", &attempt.id)
        .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
        .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string())
        .args([
            "supervision",
            "mx-report",
            "--state",
            "done",
            "--message",
            "durable before maintenance",
            "--message-id",
            "blocked-maintenance-completion",
            "--artifact",
        ])
        .arg(&artifact));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let before = fs::read(home.join("state/.wake-queue")).unwrap();
    let source = temp.path().join("runtime");
    fs::create_dir_all(source.join("bin")).unwrap();
    for (name, body) in [
        (
            "mx-watch.sh",
            "exec \"$MX_RUST_BIN\" supervision mx-watch.sh",
        ),
        ("mx-pr-check-migrate.sh", "exit 0"),
        (
            "mx-headroom.sh",
            "printf 'entered\\n' > \"$MX_HOME/maintenance-entered\"; exec sleep 5",
        ),
    ] {
        let path = source.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = run(mx(&home)
        .env("MX_RUST_SOURCE_ROOT", &source)
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .env("MX_SIGNAL_GRACE", "0")
        .env("MX_POLL", "0.05")
        .env("MX_HEARTBEAT", "999999")
        .env("MX_CHECK_INTERVAL", "999999")
        .args(["supervision", "mx-watch-checkpoint.sh", "--seconds", "1"]));
    assert!(
        output.status.success(),
        "code={:?}; stdout={}; stderr={}; maintenance_entered={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        home.join("maintenance-entered").exists()
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("signal:"));
    assert!(
        !home.join("maintenance-entered").exists(),
        "accepted wake waited for unrelated maintenance"
    );
    assert_eq!(
        fs::read(home.join("state/.wake-queue")).unwrap(),
        before,
        "handoff republished canonical wake"
    );
}

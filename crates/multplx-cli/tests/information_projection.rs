//! Informational report acceptance must preserve the actor/Viz lifecycle projection.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use multplx_domain::lifecycle::pending_reply::{self, ReplyBinding};
use multplx_domain::lifecycle::subagent_model::{
    ArtifactKind, AssignmentRole, TaskRecord, write_meta,
};
use serde_json::{Value, json};

fn run(command: &mut Command) -> Output {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn accepted_information_preserves_actor_and_viz_state_without_hiding_real_work() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let state = home.join("state");
    let fakebin = temp.path().join("fakebin");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&fakebin).unwrap();
    let tmux = fakebin.join("tmux");
    fs::write(&tmux, "#!/bin/sh\ncase \"$1\" in\nlist-windows) printf 'mx-worker\\n';;\ndisplay-message) printf '%%1\\n';;\ncapture-pane) printf 'idle prompt\\n';;\nesac\n").unwrap();
    fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755)).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mx = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
        command
            .env(
                "PATH",
                format!("{}:{}", fakebin.display(), std::env::var("PATH").unwrap()),
            )
            .env("MX_HOME", &home)
            .env("MX_ROOT_OVERRIDE", &home)
            .env("MX_STATE_OVERRIDE", &state)
            .env("MX_REPORT_STATE_OVERRIDE", &state)
            .env("MX_RUST_SOURCE_ROOT", &source)
            .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
            .env("MX_NUDGE", "0");
        command
    };
    let mut record = TaskRecord::new(
        "worker".into(),
        AssignmentRole::Researcher,
        ArtifactKind::Report,
        false,
        "root".into(),
        "root".into(),
        home.to_string_lossy().into_owned(),
    );
    record.runtime.endpoint = Some("broker:mx-worker".into());
    let metadata = write_meta(
        &format!(
            "kind=scout\nwindow=broker:mx-worker\nworktree={}\n",
            home.display()
        ),
        &record,
    )
    .unwrap();
    fs::write(state.join("worker.meta"), metadata).unwrap();
    let attempt = record.attempt.as_ref().unwrap();
    let report = |args: &[&str]| {
        run(mx()
            .env("MX_TASK_ID", "worker")
            .env("MX_ATTEMPT_ID", &attempt.id)
            .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
            .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string())
            .args(["supervision", "mx-report"])
            .args(args))
    };
    let request = |id: &str| {
        let correlation = pending_reply::create_bound(
            &home,
            &state,
            "worker",
            "question",
            &ReplyBinding {
                message_id: Some(id),
                parent_task_id: Some("root"),
                recipient_task_id: Some("worker"),
                recipient_home: Some(&home),
                attempt_id: Some(&attempt.id),
                attempt_generation: Some(attempt.generation),
                brief_revision: Some(attempt.brief_revision),
            },
        )
        .unwrap();
        pending_reply::confirm_delivery(&state, &correlation).unwrap();
        correlation
    };
    let assert_projection = |expected: &str| {
        let actor = run(mx().args(["actor-state", "worker"]));
        assert!(
            String::from_utf8_lossy(&actor.stdout)
                .starts_with(&format!("state: {expected} · source: status-log")),
            "{}",
            String::from_utf8_lossy(&actor.stdout)
        );
        let snapshot = run(mx().args(["session", "mx-system-view.sh", "--json"]));
        let snapshot: Value = serde_json::from_slice(&snapshot.stdout).unwrap();
        let task = snapshot["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == "worker")
            .unwrap();
        assert_eq!(task["current_state"]["state"], expected);
        task.clone()
    };
    let artifact = home.join("result.md");
    fs::write(&artifact, "research result").unwrap();
    report(&[
        "--state",
        "done",
        "--message",
        "ready",
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    assert_projection("done");
    let correlation = request("after-done");
    for (disposition, id) in [("acknowledged", "ack"), ("answered", "answer")] {
        let args = [
            "--state",
            "working",
            "--message",
            "  response: exact whitespace",
            "--correlation-id",
            &correlation,
            "--reply-disposition",
            disposition,
            "--message-id",
            id,
        ];
        let receipt: Value = serde_json::from_slice(&report(&args).stdout).unwrap();
        assert_eq!(receipt["completion_proven"], true);
        let task = assert_projection("done");
        assert_eq!(
            task["paths"]["status_log"]["last_event"]["state"],
            "working"
        );
        assert!(
            state
                .join("message-outbox")
                .join(format!("{id}.json"))
                .is_file(),
            "informational wake envelope retained"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&report(&args).stdout).unwrap()["replayed"],
            true
        );
        let wakes = fs::read_to_string(state.join(".wake-queue")).unwrap();
        assert_eq!(
            wakes
                .lines()
                .map(|line| multplx_core::wake::WakeRecord::parse(line).unwrap())
                .filter(|wake| wake.key == format!("message-{id}"))
                .count(),
            1,
            "informational report publishes exactly one durable wake across replay"
        );
    }
    assert!(
        pending_reply::outstanding(&state, "worker")
            .unwrap()
            .is_empty()
    );

    // Neither malformed proof, stale identity, nor a copied marker hides work.
    let evidence_path = state.join("evidence/worker-answer.json");
    let evidence_bytes = fs::read(&evidence_path).unwrap();
    let evidence: Value = serde_json::from_slice(&evidence_bytes).unwrap();
    for field in ["accepted", "status_line", "reply_disposition", "attempt"] {
        let mut damaged = evidence.clone();
        match field {
            "accepted" => damaged["accepted"] = json!(false),
            "status_line" => {
                damaged["status_line"] = json!("working [reply=answer]: another report")
            }
            "reply_disposition" => damaged["reply_disposition"] = json!("unexpected"),
            _ => damaged["envelope"]["attempt"]["generation"] = json!(999),
        }
        fs::write(&evidence_path, serde_json::to_vec(&damaged).unwrap()).unwrap();
        assert_projection("working");
    }
    fs::write(&evidence_path, "malformed").unwrap();
    assert_projection("working");
    fs::write(&evidence_path, &evidence_bytes).unwrap();
    assert_projection("done");
    let receipt_path = state.join(".transitions/report-worker-answer.json");
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    fs::write(&receipt_path, "{}").unwrap();
    assert_projection("working");
    fs::write(&receipt_path, receipt_bytes).unwrap();

    use std::io::Write;
    let status_path = state.join("worker.status");
    let accepted_status = fs::read(&status_path).unwrap();
    let metadata = fs::read(state.join("worker.meta")).unwrap();
    fs::write(
        state.join("worker.meta"),
        format!(
            "kind=scout\nwindow=broker:mx-worker\nworktree={}\n",
            home.display()
        ),
    )
    .unwrap();
    assert_projection("working");
    fs::write(state.join("worker.meta"), metadata).unwrap();
    for spoof in [
        "working [reply=answer]: copied marker",
        "working: ordinary prose [reply=answer]",
        "working [reply=missing]: response",
    ] {
        let mut status = fs::OpenOptions::new()
            .append(true)
            .open(&status_path)
            .unwrap();
        writeln!(status, "{spoof}").unwrap();
        assert_projection("working");
        fs::write(&status_path, &accepted_status).unwrap();
    }
    assert_projection("done");
    let working = report(&["--state", "working", "--message", "real new work"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&working.stdout).unwrap()["completion_proven"],
        false
    );
    assert_projection("working");
    let correlation = request("while-working");
    report(&[
        "--state",
        "working",
        "--message",
        "response",
        "--correlation-id",
        &correlation,
        "--reply-disposition",
        "answered",
        "--message-id",
        "working-answer",
    ]);
    assert_projection("working");
    let checkpoint = || {
        let output = mx()
            .env("MX_POLL", "0.05")
            .env("MX_SIGNAL_GRACE", "0")
            .env("MX_CHECK_INTERVAL", "999999")
            .env("MX_HEARTBEAT", "999999")
            .env("MX_PAUSE_RESURFACE_SECS", "10")
            .args(["supervision", "mx-watch-checkpoint.sh", "--seconds", "1"])
            .output()
            .unwrap();
        assert!(
            matches!(output.status.code(), Some(0 | 124)),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    report(&["--state", "paused", "--message", "waiting for release"]);
    let pause_marker = state.join(".paused-broker_mx-worker");
    let recheck_marker = state.join(".paused-rechecked-broker_mx-worker");
    let resurface_marker = state.join(".paused-resurfaced-broker_mx-worker");
    for _ in 0..4 {
        checkpoint();
        if pause_marker.exists() {
            break;
        }
    }
    assert!(pause_marker.exists(), "watcher observes the initial pause");
    let initial_pause_identity = fs::read_to_string(&pause_marker).unwrap();
    let old_pause = std::time::SystemTime::now() - std::time::Duration::from_secs(30);
    for marker in [&pause_marker, &recheck_marker, &resurface_marker] {
        if *marker != pause_marker {
            fs::write(marker, "retained pause tracking").unwrap();
        }
        fs::File::open(marker)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old_pause))
            .unwrap();
    }
    let correlation = request("while-paused");
    report(&[
        "--state",
        "working",
        "--message",
        "response",
        "--correlation-id",
        &correlation,
        "--reply-disposition",
        "answered",
        "--message-id",
        "paused-answer",
        "--key",
        "info",
    ]);
    assert_projection("paused");
    let mut resurfaced = false;
    for _ in 0..4 {
        let output = checkpoint();
        if String::from_utf8_lossy(&output.stdout)
            .contains("declared pause, rechecked on a long cadence")
        {
            resurfaced = true;
            break;
        }
    }
    assert!(
        resurfaced,
        "informational status must retain the established pause resurfacing cadence"
    );
    assert_eq!(
        fs::metadata(&pause_marker).unwrap().modified().unwrap(),
        old_pause
    );
    assert_eq!(
        fs::read_to_string(&recheck_marker).unwrap(),
        "retained pause tracking"
    );
    assert!(fs::metadata(&resurface_marker).unwrap().modified().unwrap() > old_pause);
    // A fresh ordinary pause, followed by information before watcher observation,
    // starts a new cadence even when its prose repeats the previous pause.
    for marker in [&pause_marker, &resurface_marker] {
        fs::File::open(marker)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old_pause))
            .unwrap();
    }
    report(&["--state", "paused", "--message", "waiting for release"]);
    let correlation = request("fresh-pause");
    report(&[
        "--state",
        "working",
        "--message",
        "response",
        "--correlation-id",
        &correlation,
        "--reply-disposition",
        "answered",
        "--message-id",
        "fresh-pause-answer",
    ]);
    for _ in 0..4 {
        let output = checkpoint();
        assert!(
            !String::from_utf8_lossy(&output.stdout)
                .contains("declared pause, rechecked on a long cadence"),
            "fresh ordinary pause must reset cadence"
        );
        if fs::metadata(&pause_marker).unwrap().modified().unwrap() > old_pause {
            break;
        }
    }
    assert!(fs::metadata(&pause_marker).unwrap().modified().unwrap() > old_pause);
    assert_ne!(
        fs::read_to_string(&pause_marker).unwrap(),
        initial_pause_identity
    );
    assert_eq!(
        fs::metadata(&resurface_marker).unwrap().modified().unwrap(),
        old_pause
    );
    report(&["--state", "working", "--message", "release arrived"]);
    for _ in 0..4 {
        checkpoint();
        if !pause_marker.exists() {
            break;
        }
    }
    assert!(
        !pause_marker.exists(),
        "ordinary work still clears pause tracking"
    );
    assert!(!recheck_marker.exists());
    assert!(!resurface_marker.exists());
}

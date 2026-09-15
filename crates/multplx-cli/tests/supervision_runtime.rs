use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn run(command: &mut Command) -> Output {
    command.output().expect("run mx")
}

fn mx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mx"))
}

fn wake(state: &std::path::Path) -> Command {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/codex-wake-owner.py");
    let mut command = Command::new("python3");
    command
        .arg(fixture)
        .arg(state)
        .arg("handle")
        .arg(env!("CARGO_BIN_EXE_mx"))
        .env("MX_ROOT_OVERRIDE", state.parent().unwrap_or(state));
    command
}

fn wake_retain(state: &std::path::Path) -> Command {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/codex-wake-owner.py");
    let mut command = Command::new("python3");
    command
        .arg(fixture)
        .arg(state)
        .arg("retain")
        .env("MX_ROOT_OVERRIDE", state.parent().unwrap_or(state));
    command
}

#[test]
fn report_and_policy_dispatch_are_native() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let report = run(mx()
        .env("MX_TASK_ID", "task")
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .env("MX_NUDGE", "0")
        .args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "done",
            "--message",
            "complete",
        ]));
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    assert_eq!(
        fs::read_to_string(state.join("task.status")).expect("status"),
        "done: complete\n"
    );

    let denial = run(mx().args([
        "supervision",
        "mx-arm-pretool-check.sh",
        "--command",
        "bin/mx-watch-arm.sh &",
    ]));
    assert_eq!(denial.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&denial.stderr).contains("watcher-background"));
}

#[test]
fn report_binding_fallback_and_nudge_failure_paths_are_observable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    let worktree = temp.path().join("worktree");
    fs::create_dir(&state).expect("state");
    fs::create_dir(&worktree).expect("worktree");
    fs::write(
        state.join("task.meta"),
        format!("worktree={}\n", worktree.display()),
    )
    .expect("meta");
    let output = run(mx()
        .current_dir(&worktree)
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .env("MX_NUDGE_DEBUG", "1")
        .args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "blocked",
            "--message",
            "waiting",
            "--key",
            "review",
        ]));
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no valid watcher pid"));
    assert_eq!(
        fs::read_to_string(state.join("task.status")).expect("status"),
        "blocked [key=review]: waiting\n"
    );

    let invalid = run(mx()
        .env("MX_TASK_ID", "bad/id")
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "done",
            "--message",
            "complete",
        ]));
    assert_eq!(invalid.status.code(), Some(3));

    fs::create_dir(state.join(".watch.lock")).expect("watch lock");
    fs::write(
        state.join(".watch.lock/pid"),
        format!("{}\n", std::process::id()),
    )
    .expect("pid");
    fs::write(state.join(".watch.lock/pid-identity"), "wrong\n").expect("identity");
    let mismatch = run(mx()
        .env("MX_TASK_ID", "task")
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .env("MX_NUDGE_DEBUG", "1")
        .args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "working",
            "--message",
            "again",
        ]));
    assert!(mismatch.status.success());
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("identity does not match"));
}

#[test]
fn report_refuses_mismatch_missing_state_ambiguous_binding_and_unsafe_append() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing");
    let missing_state = run(mx()
        .env("MX_TASK_ID", "task")
        .env("MX_REPORT_STATE_OVERRIDE", &missing)
        .args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "done",
            "--message",
            "ok",
        ]));
    assert_eq!(missing_state.status.code(), Some(3));

    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let mismatch = run(mx()
        .env("MX_TASK_ID", "other")
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "done",
            "--message",
            "ok",
        ]));
    assert_eq!(mismatch.status.code(), Some(3));

    let worktree = temp.path().join("worktree");
    fs::create_dir(&worktree).expect("worktree");
    for task in ["one", "two"] {
        fs::write(
            state.join(format!("{task}.meta")),
            format!("worktree={}\n", worktree.display()),
        )
        .expect("meta");
    }
    let ambiguous = run(mx()
        .current_dir(&worktree)
        .env_remove("MX_TASK_ID")
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .args([
            "supervision",
            "mx-report",
            "--id",
            "one",
            "--state",
            "done",
            "--message",
            "ok",
        ]));
    assert_eq!(ambiguous.status.code(), Some(3));

    symlink(temp.path().join("outside"), state.join("task.status")).expect("status symlink");
    let unsafe_append = run(mx()
        .env("MX_TASK_ID", "task")
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "done",
            "--message",
            "ok",
        ]));
    assert_eq!(unsafe_append.status.code(), Some(1));
}

#[test]
fn hook_stdin_transports_and_missing_jq_fail_open_are_native() {
    let payload = b"{\"tool_input\":{\"command\":\"bin/mx-watch-arm.sh &\"}}";
    let mut child = mx()
        .args(["supervision", "mx-arm-pretool-check.sh"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn hook");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload)
        .expect("write");
    assert_eq!(child.wait().expect("wait").code(), Some(2));

    let mut no_jq = mx()
        .env("PATH", "")
        .args(["supervision", "mx-subagent-pretool-check.sh"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn no-jq hook");
    no_jq
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"{\"tool_name\":\"Agent\"}")
        .expect("write");
    assert!(no_jq.wait().expect("wait").success());
}

#[test]
fn wake_drain_covers_empty_records_annotations_recovery_and_parse_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let empty = run(wake(&state).args(["supervision", "mx-wake-drain.sh"]));
    assert!(
        empty.status.success(),
        "{}",
        String::from_utf8_lossy(&empty.stderr)
    );

    fs::write(
        state.join(".wake-queue.drain.424242"),
        "1\t1\tsignal\ttask.status\tsignal: task.status\n",
    )
    .expect("abandoned");
    fs::write(
        state.join(".wake-queue"),
        "2\t2\theartbeat\tall\theartbeat\n",
    )
    .expect("queue");
    fs::write(state.join("task.status"), "done: complete\n").expect("status");
    let perl_log = temp.path().join("reads");
    let drained = run(wake(&state)
        .env("MX_WAKE_ENRICH_PERL_LOG", &perl_log)
        .args(["supervision", "mx-wake-drain.sh"]));
    assert!(drained.status.success());
    let stdout = String::from_utf8(drained.stdout).expect("UTF-8");
    assert!(stdout.contains("signal: task.status"));
    assert!(stdout.contains("heartbeat"));
    assert!(stdout.contains("done: complete"));
    assert_eq!(fs::read_to_string(perl_log).expect("read log"), "read\n");

    fs::write(state.join(".wake-queue"), "malformed\n").expect("bad queue");
    let malformed = run(wake(&state).args(["supervision", "mx-wake-drain.sh"]));
    assert_eq!(malformed.status.code(), Some(1));

    let state_file = temp.path().join("not-a-directory");
    fs::write(&state_file, "file").expect("state file");
    let create_error = run(wake(&state_file).args(["supervision", "mx-wake-drain.sh"]));
    assert_eq!(create_error.status.code(), Some(1));
}

#[test]
fn wake_cli_requires_owner_and_help_does_not_consume_claims() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    fs::write(
        state.join(".wake-queue"),
        "1\t1\tsignal\ttask.status\tsignal: task.status\n",
    )
    .expect("queue");
    fs::write(state.join(".wake-queue.seq"), "1\n").expect("sequence");

    let help = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "supervision",
        "mx-wake-drain.sh",
        "--help",
    ]));
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("does not acknowledge"));
    assert!(state.join(".wake-queue").metadata().expect("queue").len() > 0);

    fs::write(state.join(".lock"), format!("{}\n", std::process::id())).expect("foreign lock");
    let foreign = run(mx()
        .env("MX_STATE_OVERRIDE", &state)
        .args(["wake", "claim"]));
    assert!(!foreign.status.success());
    assert!(String::from_utf8_lossy(&foreign.stderr).contains("verified orchestrator session"));
    assert!(state.join(".wake-queue").metadata().expect("queue").len() > 0);
    fs::remove_file(state.join(".lock")).expect("remove fixture lock");

    let claimed = run(wake(&state).args(["wake", "claim"]));
    assert!(
        claimed.status.success(),
        "{}",
        String::from_utf8_lossy(&claimed.stderr)
    );
    let item: serde_json::Value =
        serde_json::from_slice(&claimed.stdout).expect("claimed JSON line");
    assert!(item["event_id"].as_str().is_some());
    let pending = run(mx()
        .env("MX_STATE_OVERRIDE", &state)
        .args(["wake", "pending"]));
    assert_eq!(String::from_utf8_lossy(&pending.stdout).trim(), "0");
}

#[test]
fn wake_follow_up_requires_a_valid_receipt_in_the_owning_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("owner-home");
    let state = temp.path().join("durable-state");
    let foreign = temp.path().join("foreign");
    fs::create_dir(&home).expect("home");
    fs::create_dir(&state).expect("state");
    fs::create_dir(&foreign).expect("foreign");
    let requests = multplx_domain::operational_input::RequestStore::new(&state, &home);
    requests
        .submit(
            &multplx_domain::operational_input::RequestSubmission {
                batch_id: "follow-up-batch",
                request_id: "follow-up-1",
                task_id: "follow-up-task",
                client_id: "test-client",
                recipient_owner: None,
                parent_task_id: None,
                parent_home: None,
                attempt_id: None,
                attempt_generation: None,
                project_id: "project",
                checkout_id: "checkout",
                starting_revision: "0123456789012345678901234567890123456789",
                brief_revision: 1,
                scope: "durable follow-up",
                dependencies: &[],
                context_artifact: None,
            },
            std::time::SystemTime::now(),
            None,
        )
        .expect("local request receipt");
    requests
        .record_notification("follow-up-1", "linked-wake")
        .expect("notification link");
    multplx_core::filesystem::recoverable_transition(
        &foreign,
        "foreign-1",
        &[multplx_core::filesystem::TransitionWrite {
            path: "foreign.txt".into(),
            before: None,
            after: b"foreign\n".to_vec(),
        }],
        None,
    )
    .expect("foreign receipt");
    fs::write(
        state.join(".wake-queue"),
        "1\t1\tsignal\ttask.status\tsignal: task.status\n",
    )
    .expect("queue");
    fs::write(state.join(".wake-queue.seq"), "1\n").expect("sequence");
    let script = temp.path().join("follow-up-lifecycle.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\nMX='{}'\n\"$MX\" wake claim >/dev/null\nevent=$(basename \"$MX_STATE_OVERRIDE\"/wake-inbox/wake-*.json .json)\nif \"$MX\" wake disposition \"$event\" follow-up --detail bad --follow-up foreign-1 >/dev/null 2>&1; then exit 9; fi\n\"$MX\" wake disposition \"$event\" follow-up --detail durable --follow-up follow-up-1 >/dev/null\n\"$MX\" wake ack \"$event\" >/dev/null\n",
            env!("CARGO_BIN_EXE_mx"),
        ),
    )
    .expect("script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("mode");
    let result = run(wake_retain(&state).env("MX_HOME", &home).arg(&script));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let pending = run(mx()
        .env("MX_HOME", &home)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["wake", "pending"]));
    assert_eq!(String::from_utf8_lossy(&pending.stdout).trim(), "0");
}

#[test]
fn report_mcp_preserves_newline_json_rpc_framing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let mut child = mx()
        .arg("report-mcp")
        .env("MX_TASK_ID", "task")
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .env("MX_NUDGE", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn MCP");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            b"\nnot-json\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"report_status\",\"arguments\":{\"state\":\"done\",\"message\":\"complete\",\"key\":\"review\"}}}\n",
        )
        .expect("write request");
    let output = child.wait_with_output().expect("MCP output");
    assert!(output.status.success());
    let lines = String::from_utf8(output.stdout).expect("UTF-8");
    let responses = lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("JSON"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[2]["result"]["tools"][0]["name"], "report_status");
    assert_eq!(responses[3]["result"]["isError"], serde_json::Value::Null);
    assert_eq!(
        fs::read_to_string(state.join("task.status")).expect("status"),
        "done [key=review]: complete\n"
    );
}

#[test]
fn report_mcp_converts_report_refusal_to_tool_error_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing-state");
    let mut child = mx()
        .arg("report-mcp")
        .env("MX_TASK_ID", "task")
        .env("MX_REPORT_STATE_OVERRIDE", &missing)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn MCP");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"report_status\",\"arguments\":{\"state\":\"done\",\"message\":\"complete\"}}}\n")
        .expect("write");
    let output = child.wait_with_output().expect("wait");
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).expect("reply");
    assert_eq!(reply["result"]["isError"], true);
}

#[test]
fn unknown_supervision_entry_is_rejected_without_execution() {
    let output = run(mx().args(["supervision", "not-an-entry"]));
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown supervision entry point"));
}

#[test]
fn native_observations_bind_current_attempt_and_retain_stale_evidence() {
    use multplx_domain::lifecycle::subagent_model::{
        ArtifactKind, AssignmentRole, TaskRecord, read_meta, write_meta,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let record = TaskRecord::new(
        "task".into(),
        AssignmentRole::Implementer,
        ArtifactKind::Implementation,
        false,
        "parent".into(),
        "root".into(),
        temp.path().to_string_lossy().into_owned(),
    );
    let attempt = record.attempt.clone().expect("attempt");
    fs::write(
        state.join("task.meta"),
        write_meta("", &record).expect("render metadata"),
    )
    .expect("metadata");
    let observe = |observed_state: &std::path::Path,
                   provider: &str,
                   event: &str,
                   payload: &str,
                   attempt_id: &str| {
        let mut command = mx();
        command
            .env("MX_TASK_ID", "task")
            .env("MX_ATTEMPT_ID", attempt_id)
            .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
            .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string())
            .env("MX_STATE_OVERRIDE", observed_state)
            .args([
                "supervision",
                "mx-native-observe.sh",
                "--provider",
                provider,
                "--event",
                event,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn native observer");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(payload.as_bytes())
            .expect("payload");
        child.wait_with_output().expect("native observe")
    };
    let payload = r#"{"agent_id":"child-1","turn_id":"turn-1"}"#;
    assert!(
        observe(&state, "codex", "start", payload, &attempt.id)
            .status
            .success()
    );
    assert!(
        observe(&state, "codex", "start", payload, &attempt.id)
            .status
            .success()
    );
    assert!(
        observe(&state, "claude", "start", payload, &attempt.id)
            .status
            .success()
    );
    assert!(
        observe(&state, "codex", "result", payload, &attempt.id)
            .status
            .success()
    );
    assert!(
        observe(
            &state,
            "codex",
            "reconcile",
            r#"{"session_id":"resumed"}"#,
            &attempt.id
        )
        .status
        .success()
    );
    let stale = observe(&state, "codex", "result", payload, "obsolete-attempt");
    assert_eq!(stale.status.code(), Some(3));
    let wrong_state = temp.path().join("foreign-state");
    fs::create_dir(&wrong_state).expect("foreign state");
    fs::copy(state.join("task.meta"), wrong_state.join("task.meta")).expect("copied metadata");
    let wrong_home = observe(&wrong_state, "codex", "interrupted", payload, &attempt.id);
    assert_eq!(wrong_home.status.code(), Some(3));
    let current = read_meta(
        "task",
        &fs::read_to_string(state.join("task.meta")).expect("metadata"),
    )
    .expect("canonical task");
    assert_eq!(current.native_observations.len(), 4);
    assert_eq!(
        current.native_observations[0].child_id.as_deref(),
        Some("child-1")
    );
    assert_eq!(current.native_observations[0].recovery, "session-bound");
    assert_eq!(current.native_observations[1].provider, "claude");
    assert_eq!(
        current.native_observations[2].state,
        multplx_domain::lifecycle::subagent_model::NativeObservationState::Result
    );
    assert_eq!(
        current.native_observations[3].state,
        multplx_domain::lifecycle::subagent_model::NativeObservationState::Interrupted
    );
    assert_eq!(current.native_observations[3].provider, "claude");
    assert_eq!(current.schedule.state, record.schedule.state);
    assert!(!state.join("task.status").exists());
    let rejected = fs::read_dir(state.join("native-delegations"))
        .expect("evidence directory")
        .flatten()
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .any(|value| value["accepted"] == false);
    assert!(rejected, "stale native result evidence was not retained");
    let wrong_home_evidence = fs::read_dir(wrong_state.join("native-delegations"))
        .expect("foreign evidence directory")
        .flatten()
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .any(|value| {
            value["accepted"] == false
                && value["rejection"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty())
        });
    assert!(
        wrong_home_evidence,
        "wrong-home observation was not retained"
    );
}

#[test]
fn unbound_native_observation_declares_session_bound_recovery_and_interruption() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    let mut child = mx()
        .env_remove("MX_TASK_ID")
        .env("MX_STATE_OVERRIDE", &state)
        .args([
            "supervision",
            "mx-native-observe.sh",
            "--provider",
            "cursor",
            "--event",
            "start",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn native observer");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(br#"{"session_id":"parent-only"}"#)
        .expect("payload");
    assert!(child.wait().expect("wait").success());
    let mut reconcile = mx()
        .env_remove("MX_TASK_ID")
        .env("MX_STATE_OVERRIDE", &state)
        .args([
            "supervision",
            "mx-native-observe.sh",
            "--provider",
            "cursor",
            "--event",
            "reconcile",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn native reconciliation");
    reconcile
        .stdin
        .take()
        .expect("stdin")
        .write_all(br#"{"session_id":"resumed-parent"}"#)
        .expect("payload");
    assert!(reconcile.wait().expect("wait").success());
    let values = fs::read_dir(state.join("native-delegations"))
        .expect("evidence directory")
        .flatten()
        .map(|entry| {
            serde_json::from_slice::<serde_json::Value>(&fs::read(entry.path()).expect("bytes"))
                .expect("JSON")
        })
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 2);
    assert!(values.iter().all(|value| value["task_id"].is_null()));
    assert!(
        values
            .iter()
            .all(|value| value["observation"]["recovery"] == "session-bound")
    );
    assert!(
        values
            .iter()
            .all(|value| value["observation"]["child_id"].is_null())
    );
    assert!(
        values
            .iter()
            .any(|value| value["observation"]["state"] == "started")
    );
    assert!(
        values
            .iter()
            .any(|value| value["observation"]["state"] == "interrupted")
    );
}

#[test]
fn native_observer_rejects_invalid_transport_and_stale_reconciliation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let help = run(mx().args(["supervision", "mx-native-observe.sh", "--help"]));
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--event"));
    for arguments in [
        vec!["--event", "start"],
        vec!["--provider", "bad/name", "--event", "start"],
        vec!["--provider", "codex", "--event", "unknown"],
        vec!["--provider"],
        vec!["--unknown"],
    ] {
        let output = run(mx().args(
            ["supervision", "mx-native-observe.sh"]
                .into_iter()
                .chain(arguments),
        ));
        assert_eq!(output.status.code(), Some(2));
    }
    let mut malformed = mx()
        .env("MX_STATE_OVERRIDE", &state)
        .args([
            "supervision",
            "mx-native-observe.sh",
            "--provider",
            "codex",
            "--event",
            "start",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .expect("malformed observer");
    malformed
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"[]")
        .expect("payload");
    assert_eq!(malformed.wait().expect("wait").code(), Some(2));

    use multplx_domain::lifecycle::subagent_model::{
        ArtifactKind, AssignmentRole, TaskRecord, write_meta,
    };
    let record = TaskRecord::new(
        "task".into(),
        AssignmentRole::Implementer,
        ArtifactKind::Implementation,
        false,
        "parent".into(),
        "root".into(),
        temp.path().to_string_lossy().into_owned(),
    );
    let attempt = record.attempt.clone().expect("attempt");
    fs::write(
        state.join("task.meta"),
        write_meta("", &record).expect("metadata"),
    )
    .expect("metadata");
    let mut stale = mx()
        .env("MX_TASK_ID", "task")
        .env("MX_ATTEMPT_ID", "stale")
        .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
        .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string())
        .env("MX_STATE_OVERRIDE", &state)
        .args([
            "supervision",
            "mx-native-observe.sh",
            "--provider",
            "codex",
            "--event",
            "reconcile",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .expect("stale reconciliation");
    stale
        .stdin
        .take()
        .expect("stdin")
        .write_all(br#"{"session_id":"new"}"#)
        .expect("payload");
    assert_eq!(stale.wait().expect("wait").code(), Some(3));
}

#[test]
fn concurrent_report_and_native_events_wait_for_brief_state_contention() {
    use multplx_core::locks::DirectoryLock;
    use multplx_core::process::SystemProcessProbe;
    use multplx_domain::lifecycle::subagent_model::{
        ArtifactKind, AssignmentRole, TaskRecord, read_meta, write_meta,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let record = |task: &str| {
        TaskRecord::new(
            task.into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            "parent".into(),
            "root".into(),
            temp.path().to_string_lossy().into_owned(),
        )
    };
    let report_record = record("report-task");
    let native_record = record("native-task");
    let report_attempt = report_record.attempt.clone().expect("report attempt");
    let native_attempt = native_record.attempt.clone().expect("native attempt");
    fs::write(
        state.join("report-task.meta"),
        write_meta("", &report_record).expect("report metadata"),
    )
    .expect("report metadata file");
    fs::write(
        state.join("native-task.meta"),
        write_meta("", &native_record).expect("native metadata"),
    )
    .expect("native metadata file");

    let spawn_native = |event: &str, child_id: &str| {
        let mut command = mx();
        command
            .env("MX_TASK_ID", "native-task")
            .env("MX_ATTEMPT_ID", &native_attempt.id)
            .env(
                "MX_ATTEMPT_GENERATION",
                native_attempt.generation.to_string(),
            )
            .env(
                "MX_BRIEF_REVISION",
                native_attempt.brief_revision.to_string(),
            )
            .env("MX_STATE_OVERRIDE", &state)
            .args([
                "supervision",
                "mx-native-observe.sh",
                "--provider",
                "codex",
                "--event",
                event,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn native observer");
        child
            .stdin
            .take()
            .expect("native stdin")
            .write_all(
                format!(
                    r#"{{"agent_id":"{child_id}","session_id":"same-parent","turn_id":"same-turn"}}"#
                )
                .as_bytes(),
            )
            .expect("native payload");
        child
    };

    let processes = SystemProcessProbe::default();
    let report_lock =
        DirectoryLock::try_acquire(state.join(".report-task.identity.lock"), &processes)
            .expect("report identity lock");
    let native_lock =
        DirectoryLock::try_acquire(state.join(".native-task.identity.lock"), &processes)
            .expect("native identity lock");
    let transition_lock = DirectoryLock::try_acquire(state.join(".transition.lock"), &processes)
        .expect("transition lock");

    let mut report = mx();
    report
        .env("MX_TASK_ID", "report-task")
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .env("MX_NUDGE", "0")
        .args([
            "supervision",
            "mx-report",
            "--id",
            "report-task",
            "--state",
            "working",
            "--message",
            "parallel report",
            "--message-id",
            "parallel-report",
            "--attempt-id",
            &report_attempt.id,
            "--generation",
            &report_attempt.generation.to_string(),
            "--brief-revision",
            &report_attempt.brief_revision.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut report = report.spawn().expect("spawn report");
    let mut native = spawn_native("start", "different-task-child");
    thread::sleep(Duration::from_millis(250));
    assert!(
        report.try_wait().expect("report wait probe").is_none(),
        "report did not wait for its brief identity lock"
    );
    assert!(
        native.try_wait().expect("native wait probe").is_none(),
        "native event did not wait for its brief identity lock"
    );
    drop(report_lock);
    drop(native_lock);
    thread::sleep(Duration::from_millis(250));
    assert!(
        report
            .try_wait()
            .expect("report transition probe")
            .is_none(),
        "report did not wait for the shared transition lock"
    );
    assert!(
        native
            .try_wait()
            .expect("native transition probe")
            .is_none(),
        "native event did not wait for the shared transition lock"
    );
    drop(transition_lock);

    let report = report.wait_with_output().expect("report output");
    let native = native.wait_with_output().expect("native output");
    assert!(
        report.status.success(),
        "report lost during brief contention: {}",
        String::from_utf8_lossy(&report.stderr)
    );
    assert!(
        native.status.success(),
        "native event lost during brief contention: {}",
        String::from_utf8_lossy(&native.stderr)
    );

    let native_lock =
        DirectoryLock::try_acquire(state.join(".native-task.identity.lock"), &processes)
            .expect("same-parent identity lock");
    let transition_lock = DirectoryLock::try_acquire(state.join(".transition.lock"), &processes)
        .expect("same-parent transition lock");
    let mut start = spawn_native("start", "same-parent-child");
    let mut result = spawn_native("result", "same-parent-child");
    thread::sleep(Duration::from_millis(250));
    assert!(start.try_wait().expect("start wait probe").is_none());
    assert!(result.try_wait().expect("result wait probe").is_none());
    drop(native_lock);
    thread::sleep(Duration::from_millis(250));
    assert!(start.try_wait().expect("start transition probe").is_none());
    assert!(
        result
            .try_wait()
            .expect("result transition probe")
            .is_none()
    );
    drop(transition_lock);
    let start = start.wait_with_output().expect("start output");
    let result = result.wait_with_output().expect("result output");
    assert!(
        start.status.success(),
        "same-parent start lost: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    assert!(
        result.status.success(),
        "same-parent result lost: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    assert_eq!(
        fs::read_to_string(state.join("report-task.status")).expect("report status"),
        "working: parallel report\n"
    );
    let current = read_meta(
        "native-task",
        &fs::read_to_string(state.join("native-task.meta")).expect("native metadata"),
    )
    .expect("current native task");
    assert_eq!(current.native_observations.len(), 3);
    assert_eq!(
        current
            .native_observations
            .iter()
            .filter(|event| event.child_id.as_deref() == Some("same-parent-child"))
            .count(),
        2
    );

    let unavailable_lock =
        DirectoryLock::try_acquire(state.join(".native-task.identity.lock"), &processes)
            .expect("unavailable identity lock");
    let started = Instant::now();
    let unavailable = spawn_native("start", "unavailable-child")
        .wait_with_output()
        .expect("unavailable output");
    assert!(!unavailable.status.success());
    assert!(
        String::from_utf8_lossy(&unavailable.stderr).contains("lock is held"),
        "long-held lock returned the wrong failure: {}",
        String::from_utf8_lossy(&unavailable.stderr)
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "long-held owner exceeded the bounded hook wait"
    );
    drop(unavailable_lock);
    let after_unavailable = read_meta(
        "native-task",
        &fs::read_to_string(state.join("native-task.meta")).expect("native metadata"),
    )
    .expect("native task after unavailable owner");
    assert_eq!(after_unavailable.native_observations.len(), 3);
}

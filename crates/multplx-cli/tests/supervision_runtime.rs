use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::{Command, Output, Stdio};

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
    let empty = run(mx()
        .env("MX_STATE_OVERRIDE", &state)
        .args(["supervision", "mx-wake-drain.sh"]));
    assert!(empty.status.success());

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
    let drained = run(mx()
        .env("MX_STATE_OVERRIDE", &state)
        .env("MX_WAKE_ENRICH_PERL_LOG", &perl_log)
        .args(["supervision", "mx-wake-drain.sh"]));
    assert!(drained.status.success());
    let stdout = String::from_utf8(drained.stdout).expect("UTF-8");
    assert!(stdout.contains("signal: task.status"));
    assert!(stdout.contains("heartbeat"));
    assert!(stdout.contains("done: complete"));
    assert_eq!(fs::read_to_string(perl_log).expect("read log"), "read\n");

    fs::write(state.join(".wake-queue"), "malformed\n").expect("bad queue");
    let malformed = run(mx()
        .env("MX_STATE_OVERRIDE", &state)
        .args(["supervision", "mx-wake-drain.sh"]));
    assert_eq!(malformed.status.code(), Some(1));

    let state_file = temp.path().join("not-a-directory");
    fs::write(&state_file, "file").expect("state file");
    let create_error = run(mx()
        .env("MX_STATE_OVERRIDE", &state_file)
        .args(["supervision", "mx-wake-drain.sh"]));
    assert_eq!(create_error.status.code(), Some(1));
}

#[test]
fn compatibility_dispatch_pins_the_child_to_legacy_before_execution() {
    let temp = tempfile::tempdir().expect("tempdir");
    executable(
        &temp.path().join("bin/mx-watch.sh"),
        "#!/bin/sh\nprintf '%s|%s\\n' \"${MX_SUPERVISION_IMPLEMENTATION:-unset}\" \"${1:-}\"\n",
    );
    let output = run(mx()
        .env("MX_ROOT_OVERRIDE", temp.path())
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args(["supervision", "mx-watch.sh", "probe"]));
    assert!(output.status.success());
    assert_eq!(output.stdout, b"legacy|probe\n");
}

#[test]
fn missing_compatibility_body_is_a_typed_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    fs::create_dir(temp.path().join("bin")).expect("bin");
    let output = run(mx()
        .env("MX_ROOT_OVERRIDE", temp.path())
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args(["supervision", "mx-watch.sh"]));
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("compatibility body is unavailable"));
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

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn shell(script: &str) -> Output {
    Command::new("/bin/bash")
        .arg("-c")
        .arg(script)
        .env("ROOT", repo_root())
        .output()
        .expect("run legacy primitive")
}

fn rust(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mx"))
        .arg("primitive")
        .args(args)
        .output()
        .expect("run Rust primitive")
}

fn assert_same(legacy: &Output, rust: &Output) {
    assert_eq!(rust.status.code(), legacy.status.code(), "exit status");
    assert_eq!(rust.stdout, legacy.stdout, "stdout");
    assert_eq!(rust.stderr, legacy.stderr, "stderr");
}

#[test]
fn pure_classifier_transition_marker_and_gate_commands_match_legacy() {
    let cases = [
        ("blocked", "working", "paused", "busy"),
        ("unknown", "done", "working", "idle"),
        ("", "", "blocked", "busy"),
        ("unknown", "unknown", "malformed", "unknown"),
    ];
    for (native, run_step, report, heuristic) in cases {
        let legacy = shell(&format!(
            ". \"$ROOT/bin/mx-classify-lib.sh\"; mx_signal_resolve '{native}' '{run_step}' '{report}' '{heuristic}'"
        ));
        let output = rust(&["signal-resolve", native, run_step, report, heuristic]);
        assert_same(&legacy, &output);
    }

    for status in ["blocked", "working", "idle", "done", "unknown"] {
        let legacy = shell(&format!(
            ". \"$ROOT/bin/mx-transition-lib.sh\"; mx_transition_policy '{status}'"
        ));
        assert_same(&legacy, &rust(&["transition-policy", status]));
    }

    let message = "do the work";
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg(". \"$ROOT/bin/mx-marker-lib.sh\"; mx_message_mark_from_broker \"$MESSAGE\" output; printf '%s' \"$output\"")
        .env("ROOT", repo_root())
        .env("MESSAGE", message)
        .output()
        .expect("legacy marker");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args(["primitive", "marker-mark"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Rust marker");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(message.as_bytes())
        .expect("write message");
    assert_same(&legacy, &child.wait_with_output().expect("marker output"));

    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg(". \"$ROOT/bin/mx-gate-refuse-lib.sh\"; mx_refuse_if_gate_agent")
        .env("ROOT", repo_root())
        .env("DEEP_REVIEW_GATE", "1")
        .output()
        .expect("legacy gate");
    let rust = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args(["primitive", "gate-refuse"])
        .env("DEEP_REVIEW_GATE", "1")
        .output()
        .expect("Rust gate");
    assert_same(&legacy, &rust);
}

#[test]
fn composer_content_and_ghost_bytes_match_legacy() {
    let cases = [
        ("0", "$", "", "sensitive", "$"),
        ("1", ">", "", "sensitive", ">"),
        (
            "0",
            "❯ Type a message...",
            "^Type a message\\.\\.\\.$",
            "sensitive",
            "❯ Type a message...",
        ),
        ("1", "real text", "", "sensitive", "real text"),
    ];
    for (bordered, content, idle, case_mode, plain) in cases {
        let legacy = Command::new("/bin/bash")
            .arg("-c")
            .arg(". \"$ROOT/bin/mx-composer-lib.sh\"; mx_composer_classify_content \"$BORDERED\" \"$CONTENT\" \"$IDLE\" \"$CASE_MODE\" \"$PLAIN\"")
            .env("ROOT", repo_root())
            .env("BORDERED", bordered)
            .env("CONTENT", content)
            .env("IDLE", idle)
            .env("CASE_MODE", case_mode)
            .env("PLAIN", plain)
            .output()
            .expect("legacy composer");
        let mut args = vec!["primitive", "composer-classify", bordered, content];
        if !idle.is_empty() {
            args.extend(["--idle-regex", idle]);
        }
        if case_mode == "insensitive" {
            args.push("--insensitive");
        }
        args.extend(["--plain-content", plain]);
        let output = Command::new(env!("CARGO_BIN_EXE_mx"))
            .args(args)
            .output()
            .expect("Rust composer");
        assert_same(&legacy, &output);
    }

    let styled = b"\xe2\x9d\xaf real\x1b[2m ghost\x1b[0m \x1b[38;2;50;47;70mplaceholder\x1b[0m";
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg(". \"$ROOT/bin/mx-composer-lib.sh\"; mx_composer_strip_ghost")
        .env("ROOT", repo_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("legacy ghost");
    let rust = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args(["primitive", "composer-strip-ghost"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Rust ghost");
    fn finish(mut child: std::process::Child, input: &[u8]) -> Output {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input)
            .expect("write styled row");
        child.wait_with_output().expect("ghost output")
    }
    assert_same(&finish(legacy, styled), &finish(rust, styled));
}

#[test]
fn home_tag_transition_and_status_folds_match_legacy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let home = temp.path().join("home");
    fs::create_dir(&root).expect("root");
    fs::create_dir(&home).expect("home");
    fs::write(home.join(".mx-daemon-home"), b" daemon-1 \n").expect("marker");
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg("MX_ROOT=\"$TEST_ROOT\" MX_HOME=\"$TEST_HOME\"; . \"$ROOT/bin/mx-backend-hometag-lib.sh\"; mx_backend_hometag")
        .env("ROOT", repo_root())
        .env("TEST_ROOT", &root)
        .env("TEST_HOME", &home)
        .output()
        .expect("legacy home tag");
    assert_same(
        &legacy,
        &rust(&[
            "backend-home-tag",
            root.to_str().expect("root UTF-8"),
            home.to_str().expect("home UTF-8"),
        ]),
    );

    let legacy = shell(
        ". \"$ROOT/bin/mx-transition-lib.sh\"; mx_transition_record $'p\\tid' ws '' $'blocked\\n' claude",
    );
    assert_same(
        &legacy,
        &rust(&[
            "transition-record",
            "p\tid",
            "ws",
            "",
            "blocked\n",
            "claude",
        ]),
    );

    let status = temp.path().join("task.status");
    fs::write(
        &status,
        b"needs-decision [key=a]: first\nblocked [key=b]: second\nresolved [key=a]: yes\nneeds-decision [key=a]: third\n",
    )
    .expect("status");
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg(". \"$ROOT/bin/mx-classify-lib.sh\"; status_open_decisions \"$STATUS\"")
        .env("ROOT", repo_root())
        .env("STATUS", &status)
        .output()
        .expect("legacy fold");
    assert_same(
        &legacy,
        &rust(&[
            "status-open-decisions",
            status.to_str().expect("status UTF-8"),
        ]),
    );
}

#[test]
fn journal_and_wake_disk_bytes_match_legacy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let legacy_state = temp.path().join("legacy-state");
    let rust_state = temp.path().join("rust-state");
    fs::create_dir(&legacy_state).expect("legacy state");
    fs::create_dir(&rust_state).expect("Rust state");
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg("umask 077; . \"$ROOT/bin/mx-journal-lib.sh\"; MX_STATE_OVERRIDE=\"$STATE\" MX_JOURNAL_SOURCE=mx-test MX_JOURNAL_NOW=2026-08-10T12:00:00Z mx_journal_emit task-1 status.reported '{\"raw\":\"done: yes\",\"validated\":true}'")
        .env("ROOT", repo_root())
        .env("STATE", &legacy_state)
        .output()
        .expect("legacy journal");
    let output = rust(&[
        "journal-emit",
        rust_state.to_str().expect("state UTF-8"),
        "task-1",
        "status.reported",
        r#"{"raw":"done: yes","validated":true}"#,
        "mx-test",
        "2026-08-10T12:00:00Z",
    ]);
    assert_same(&legacy, &output);
    let legacy_journal = legacy_state.join("task-1.journal");
    let rust_journal = rust_state.join("task-1.journal");
    assert_eq!(
        fs::read(&rust_journal).expect("Rust journal"),
        fs::read(&legacy_journal).expect("legacy journal")
    );
    assert_eq!(
        fs::metadata(&rust_journal)
            .expect("Rust mode")
            .permissions()
            .mode()
            & 0o777,
        fs::metadata(&legacy_journal)
            .expect("legacy mode")
            .permissions()
            .mode()
            & 0o777
    );

    let fakebin = temp.path().join("fakebin");
    fs::create_dir(&fakebin).expect("fakebin");
    let date = fakebin.join("date");
    fs::write(&date, b"#!/bin/sh\nprintf '1786363200\\n'\n").expect("fake date");
    fs::set_permissions(&date, fs::Permissions::from_mode(0o755)).expect("date mode");
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg("umask 077; MX_STATE_OVERRIDE=\"$STATE\"; . \"$ROOT/bin/mx-wake-lib.sh\"; mx_wake_append signal task.status $'signal: one\\nline'")
        .env("ROOT", repo_root())
        .env("STATE", &legacy_state)
        .env("PATH", format!("{}:{}", fakebin.display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .expect("legacy wake");
    let output = rust(&[
        "wake-append",
        rust_state.to_str().expect("state UTF-8"),
        "signal",
        "task.status",
        "signal: one\nline",
        "1786363200",
    ]);
    assert_same(&legacy, &output);
    assert_eq!(
        fs::read(rust_state.join(".wake-queue")).expect("Rust queue"),
        fs::read(legacy_state.join(".wake-queue")).expect("legacy queue")
    );
    assert_eq!(
        fs::read(rust_state.join(".wake-queue.seq")).expect("Rust sequence"),
        fs::read(legacy_state.join(".wake-queue.seq")).expect("legacy sequence")
    );
}

#[test]
fn supervisor_and_probe_rendering_match_legacy() {
    let environments = [
        vec![("MX_SUPERVISOR_TARGET", "explicit:pane")],
        vec![("TMUX_PANE", "%7")],
        vec![
            ("HERDR_ENV", "1"),
            ("HERDR_PANE_ID", "pane-1"),
            ("HERDR_SESSION", "lab"),
        ],
    ];
    for environment in environments {
        let mut legacy = Command::new("/bin/bash");
        legacy
            .arg("-c")
            .arg(". \"$ROOT/bin/mx-supervisor-target-lib.sh\"; discover_supervisor_target")
            .env("ROOT", repo_root())
            .env_remove("TMUX_PANE")
            .env_remove("HERDR_ENV")
            .env_remove("HERDR_PANE_ID")
            .env_remove("HERDR_SESSION")
            .env_remove("MX_SUPERVISOR_TARGET");
        let mut rust = Command::new(env!("CARGO_BIN_EXE_mx"));
        rust.args(["primitive", "supervisor-target"])
            .env_remove("TMUX_PANE")
            .env_remove("HERDR_ENV")
            .env_remove("HERDR_PANE_ID")
            .env_remove("HERDR_SESSION")
            .env_remove("MX_SUPERVISOR_TARGET");
        for (key, value) in environment {
            legacy.env(key, value);
            rust.env(key, value);
        }
        assert_same(
            &legacy.output().expect("legacy target"),
            &rust.output().expect("Rust target"),
        );
    }

    for tool in ["tmux", "cmux", "treehouse", "herdr"] {
        let legacy = shell(&format!(
            ". \"$ROOT/bin/mx-probe-lib.sh\"; mx_probe_install_cmd '{tool}' 2>/dev/null || mx_probe_manual_install_url '{tool}'"
        ));
        assert_same(&legacy, &rust(&["probe-install", tool]));
    }
}

#[test]
fn check_primary_scope_and_tangle_results_match_legacy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let check = state.join("task-1.check.sh");
    fs::write(&check, b"#!/bin/sh\nprintf 'ok\\n'\n").expect("check");
    fs::set_permissions(&check, fs::Permissions::from_mode(0o700)).expect("check mode");
    let hash = shell(&format!(
        ". \"$ROOT/bin/mx-check-lib.sh\"; mx_custom_check_sha256 '{}'",
        check.display()
    ));
    assert!(hash.status.success());
    fs::write(
        state.join("task-1.check-trust"),
        format!(
            "mx-custom-check-v1\n{}\n",
            String::from_utf8_lossy(&hash.stdout).trim()
        ),
    )
    .expect("trust");
    fs::set_permissions(
        state.join("task-1.check-trust"),
        fs::Permissions::from_mode(0o600),
    )
    .expect("trust mode");
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg(". \"$ROOT/bin/mx-pr-lib.sh\"; . \"$ROOT/bin/mx-check-lib.sh\"; mx_custom_check_registered \"$STATE\" task-1")
        .env("ROOT", repo_root())
        .env("STATE", &state)
        .output()
        .expect("legacy check");
    assert_same(
        &legacy,
        &rust(&[
            "check-registered",
            state.to_str().expect("state UTF-8"),
            "task-1",
        ]),
    );

    let checkout = temp.path().join("checkout");
    fs::create_dir(&checkout).expect("checkout");
    let init = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&checkout)
        .output()
        .expect("git init");
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    fs::create_dir(checkout.join("bin")).expect("bin");
    fs::write(checkout.join("AGENTS.md"), b"# contract\n").expect("contract");
    let commit = Command::new("git")
        .args(["-C", checkout.to_str().expect("checkout UTF-8"), "add", "."])
        .status()
        .expect("git add");
    assert!(commit.success());
    let commit = Command::new("git")
        .args([
            "-C",
            checkout.to_str().expect("checkout UTF-8"),
            "-c",
            "user.name=Multplx Test",
            "-c",
            "user.email=multplx-test@example.invalid",
            "commit",
            "-m",
            "fixture",
        ])
        .output()
        .expect("git commit");
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg(". \"$ROOT/bin/mx-primary-scope-lib.sh\"; mx_primary_scope_matches \"$CHECKOUT\" \"$STATE\"")
        .env("ROOT", repo_root())
        .env("CHECKOUT", &checkout)
        .env("STATE", &state)
        .output()
        .expect("legacy primary scope");
    assert_same(
        &legacy,
        &rust(&[
            "primary-scope",
            checkout.to_str().expect("checkout UTF-8"),
            state.to_str().expect("state UTF-8"),
        ]),
    );

    for branch in ["main", "feature"] {
        let checkout_result = Command::new("git")
            .args([
                "-C",
                checkout.to_str().expect("checkout UTF-8"),
                "checkout",
                "-B",
                branch,
            ])
            .output()
            .expect("git checkout");
        assert!(checkout_result.status.success());
        let legacy = Command::new("/bin/bash")
            .arg("-c")
            .arg(". \"$ROOT/bin/mx-tangle-lib.sh\"; mx_primary_tangle_branch \"$CHECKOUT\"")
            .env("ROOT", repo_root())
            .env("CHECKOUT", &checkout)
            .output()
            .expect("legacy tangle");
        assert_same(
            &legacy,
            &rust(&["tangle", checkout.to_str().expect("checkout UTF-8")]),
        );
    }
}

#[test]
fn supervision_and_session_lock_status_match_legacy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    fs::write(state.join("task.meta"), b"id=task\n").expect("meta");
    fs::write(state.join(".wake-queue"), b"one row\n").expect("queue");
    let fakebin = temp.path().join("fakebin");
    fs::create_dir(&fakebin).expect("fakebin");
    let date = fakebin.join("date");
    fs::write(&date, b"#!/bin/sh\nprintf '1786363200\\n'\n").expect("fake date");
    fs::set_permissions(&date, fs::Permissions::from_mode(0o755)).expect("date mode");
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg(". \"$ROOT/bin/mx-supervision-lib.sh\"; mx_supervision_status \"$STATE\" 300; printf '%s\\t%s\\t%s\\t%s\\t%s\\n' \"$MX_SUP_IN_FLIGHT\" \"$MX_SUP_NEEDED\" \"$MX_SUP_WATCHER_FRESH\" \"$MX_SUP_BEACON_DESC\" \"$MX_SUP_QUEUE_PENDING\"")
        .env("ROOT", repo_root())
        .env("STATE", &state)
        .env(
            "PATH",
            format!("{}:{}", fakebin.display(), std::env::var("PATH").unwrap_or_default()),
        )
        .output()
        .expect("legacy supervision");
    assert_same(
        &legacy,
        &rust(&[
            "supervision-status",
            state.to_str().expect("state UTF-8"),
            "300",
            "1786363200",
        ]),
    );

    for lock_contents in [None, Some("not-a-pid\n"), Some("4294967294\n")] {
        let lock = state.join(".lock");
        match lock_contents {
            Some(contents) => fs::write(&lock, contents).expect("lock"),
            None => {
                let _ = fs::remove_file(&lock);
            }
        }
        let legacy = Command::new(repo_root().join("bin/mx-lock.sh"))
            .arg("status")
            .env("MX_STATE_OVERRIDE", &state)
            .output()
            .expect("legacy lock status");
        assert_same(
            &legacy,
            &rust(&["session-lock-status", lock.to_str().expect("lock UTF-8")]),
        );
    }
}

#[test]
fn current_process_identity_matches_legacy_marker() {
    let pid = std::process::id().to_string();
    let legacy = Command::new("/bin/bash")
        .arg("-c")
        .arg(". \"$ROOT/bin/mx-wake-lib.sh\"; mx_pid_identity \"$PID\"")
        .env("ROOT", repo_root())
        .env("PID", &pid)
        .output()
        .expect("legacy process marker");
    assert_same(&legacy, &rust(&["process-identity", &pid]));
}

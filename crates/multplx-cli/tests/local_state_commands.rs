use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn mx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mx"))
}

fn run(args: &[&str]) -> Output {
    mx().args(args).output().expect("run mx")
}

fn run_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = mx()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run mx with stdin");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    child.wait_with_output().expect("output")
}

fn write_executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent");
    fs::write(path, body).expect("script");
    fs::set_permissions(path, Permissions::from_mode(0o755)).expect("mode");
}

fn seeded_daemon(path: &Path, id: &str) {
    for name in ["data", "state", "config", "projects", "bin"] {
        fs::create_dir_all(path.join(name)).expect("daemon surface");
    }
    fs::write(path.join(".mx-daemon-home"), format!("{id}\n")).expect("marker");
    fs::write(path.join("AGENTS.md"), "# daemon\n").expect("agents");
    fs::write(
        path.join("data/backlog.md"),
        "## In flight\n\n## Queued\n\n## Done\n",
    )
    .expect("backlog");
}

#[test]
fn operational_input_command_covers_success_nonmatch_and_usage() {
    let mut encoded = mx()
        .args(["operational-input", "encode", "watcher"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("encode");
    use std::io::Write;
    encoded
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"payload")
        .expect("payload");
    let encoded = encoded.wait_with_output().expect("output");
    assert!(encoded.status.success());
    for command in ["kind", "classify", "body"] {
        let mut child = mx()
            .args(["operational-input", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("parse");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&encoded.stdout)
            .expect("message");
        assert!(child.wait_with_output().expect("output").status.success());
    }
    assert!(
        run(&["operational-input", "--help", "ignored"])
            .status
            .success()
    );
    assert_eq!(run(&["operational-input", "encode"]).status.code(), Some(2));
    assert_eq!(
        run(&["operational-input", "unknown"]).status.code(),
        Some(2)
    );
}

#[test]
fn config_inherit_transport_covers_every_public_operation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source_home = temp.path().join("source-home");
    let source_config = source_home.join("config");
    let source_data = source_home.join("data");
    let daemon = temp.path().join("daemon");
    fs::create_dir_all(&source_config).expect("source config");
    fs::create_dir_all(&source_data).expect("source data");
    seeded_daemon(&daemon, "worker");
    fs::write(source_config.join("actor-harness"), "codex\n").expect("config");
    let shared = "# Shared maintainer preferences\n\nThis file is main-authoritative in the main Multplx home.\nIn daemon homes it is read-only in daemon homes and must not be edited there.\nRoute discoveries to the main broker through marked status or a document pointer.\n";
    fs::write(source_data.join("maintainer-shared.md"), shared).expect("shared");
    let file = source_config.join("actor-harness");
    for command in ["file-mode", "file-device", "file-links", "sha256"] {
        assert!(
            run(&[command_prefix(), command, file.to_str().expect("file")])
                .status
                .success()
        );
    }
    let report = temp.path().join("report");
    let mut propagate = mx();
    propagate
        .args([
            command_prefix(),
            "propagate-config",
            source_config.to_str().expect("source"),
            daemon.join("config").to_str().expect("destination"),
        ])
        .env("MX_CONFIG_INHERIT_REPORT", &report);
    assert!(propagate.output().expect("propagate").status.success());
    assert!(
        run(&[
            command_prefix(),
            "propagate-shared",
            source_data.to_str().expect("source data"),
            daemon.join("data").to_str().expect("destination data"),
        ])
        .status
        .success()
    );
    for command in ["pending-stages", "pending-reports"] {
        assert!(
            run(&[
                command_prefix(),
                command,
                source_home.to_str().expect("source home"),
                "worker",
            ])
            .status
            .success()
        );
    }
    assert!(
        run(&[
            command_prefix(),
            "propagate-daemon",
            source_home.to_str().expect("source home"),
            daemon.to_str().expect("daemon"),
            source_config.to_str().expect("source config"),
            source_data.to_str().expect("source data"),
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            command_prefix(),
            "changed-items",
            report.to_str().expect("report"),
        ])
        .status
        .success()
    );
    let runtime = source_home.join("runtime");
    write_executable(&runtime.join("bin/mx-send.sh"), "#!/bin/sh\nexit 0\n");
    let mut send = mx();
    send.args([
        command_prefix(),
        "send-reread",
        "worker",
        daemon.to_str().expect("daemon"),
        report.to_str().expect("report"),
    ])
    .env("MX_HOME", &source_home)
    .env("MX_ROOT_OVERRIDE", &runtime);
    assert!(send.output().expect("send reread").status.success());
    let mut retry = mx();
    retry
        .args([
            command_prefix(),
            "retry-pending",
            "worker",
            daemon.to_str().expect("daemon"),
        ])
        .env("MX_HOME", &source_home)
        .env("MX_ROOT_OVERRIDE", &runtime);
    assert!(retry.output().expect("retry pending").status.success());

    for (command, args) in [
        ("lock-path", vec![source_home.to_str().expect("home")]),
        (
            "retry-dir",
            vec![source_home.to_str().expect("home"), "worker"],
        ),
        (
            "pending-stages",
            vec![source_home.to_str().expect("home"), "worker"],
        ),
        (
            "pending-reports",
            vec![source_home.to_str().expect("home"), "worker"],
        ),
        (
            "has-staged",
            vec![source_home.to_str().expect("home"), "worker"],
        ),
        (
            "queue-full",
            vec![source_home.to_str().expect("home"), "worker"],
        ),
    ] {
        let mut call = vec![command_prefix(), command];
        call.extend(args);
        let _ = run(&call);
    }

    let stage_output = run(&[
        command_prefix(),
        "new-stage",
        source_home.to_str().expect("source home"),
        "worker",
    ]);
    assert!(stage_output.status.success());
    let stage = PathBuf::from(String::from_utf8_lossy(&stage_output.stdout).trim());
    let explicit_pending = temp.path().join("explicit.pending");
    assert!(
        run(&[
            command_prefix(),
            "mark-pending",
            stage.to_str().expect("stage"),
            explicit_pending.to_str().expect("pending"),
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            command_prefix(),
            "write-instruction",
            daemon.to_str().expect("daemon"),
            report.to_str().expect("report"),
            stage.to_str().expect("stage"),
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            command_prefix(),
            "save-report",
            report.to_str().expect("report"),
            stage.to_str().expect("stage"),
        ])
        .status
        .success()
    );
    for command in ["pending-stages", "pending-reports"] {
        let output = run(&[
            command_prefix(),
            command,
            source_home.to_str().expect("source home"),
            "worker",
        ]);
        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
    }
    assert!(
        run(&[
            command_prefix(),
            "publish-stage",
            daemon.to_str().expect("daemon"),
            stage.to_str().expect("stage"),
        ])
        .status
        .success()
    );
    for command in ["has-pending", "pending-instructions", "cleanup-sent"] {
        let _ = run(&[command_prefix(), command, daemon.to_str().expect("daemon")]);
    }
    assert!(
        run(&[
            command_prefix(),
            "quarantine-pending",
            daemon.to_str().expect("daemon"),
            "worker",
            source_home.to_str().expect("source home"),
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            command_prefix(),
            "discard-pending",
            daemon.to_str().expect("daemon"),
            "worker",
            source_home.to_str().expect("source home"),
        ])
        .status
        .success()
    );
    assert_eq!(run(&[command_prefix(), "unknown"]).status.code(), Some(2));
    assert_eq!(
        run(&[command_prefix(), "sha256", "missing"]).status.code(),
        Some(1)
    );
}

fn command_prefix() -> &'static str {
    "config-inherit"
}

#[test]
fn backlog_handoff_and_config_push_cover_top_level_orchestration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let active = temp.path().join("active");
    let daemon = temp.path().join("daemon");
    fs::create_dir_all(root.join("bin")).expect("root bin");
    fs::create_dir_all(active.join("data")).expect("active data");
    fs::create_dir_all(active.join("state")).expect("active state");
    fs::create_dir_all(active.join("config")).expect("active config");
    seeded_daemon(&daemon, "worker");
    write_executable(&root.join("bin/mx-send.sh"), "#!/bin/sh\nexit 0\n");
    fs::write(
        active.join("data/daemons.md"),
        format!(
            "- worker - test (home: {}; scope: test)\n",
            daemon.display()
        ),
    )
    .expect("registry");
    fs::write(
        active.join("data/backlog.md"),
        "## In flight\n\n## Queued\n- [ ] task - Task\n  body\n\n## Done\n",
    )
    .expect("backlog");
    let common = |command: &mut Command| {
        command
            .env("MX_ROOT_OVERRIDE", &root)
            .env("MX_HOME", &active)
            .env("MX_DATA_OVERRIDE", active.join("data"))
            .env("MX_STATE_OVERRIDE", active.join("state"))
            .env("MX_CONFIG_OVERRIDE", active.join("config"))
            .env("MX_RUST_SOURCE_ROOT", &root);
    };
    let mut handoff = mx();
    handoff.args(["backlog-handoff", "worker", "task"]);
    common(&mut handoff);
    assert!(handoff.output().expect("handoff").status.success());

    fs::write(active.join("config/actor-harness"), "codex\n").expect("config");
    fs::write(
        active.join("state/worker.meta"),
        format!("kind=daemon\nhome={}\n", daemon.display()),
    )
    .expect("meta");
    let mut push = mx();
    push.arg("config-push");
    common(&mut push);
    let output = push.output().expect("config push");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("config-reread: sent"));
    assert_eq!(
        fs::read_to_string(daemon.join("config/actor-harness")).expect("pushed"),
        "codex\n"
    );

    let mut empty = mx();
    empty.arg("config-push");
    empty
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", temp.path().join("empty"));
    assert!(empty.output().expect("empty push").status.success());
    assert!(run(&["config-push", "--help", "ignored"]).status.success());
    assert_eq!(run(&["config-push", "bad"]).status.code(), Some(2));
}

#[test]
fn command_refusal_and_primitive_error_matrix_is_observable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    fs::create_dir_all(home.join("data")).expect("data");
    fs::write(
        home.join("data/backlog.md"),
        "## In flight\n\n## Queued\n\n## Done\n",
    )
    .expect("backlog");
    for args in [
        vec!["backlog"],
        vec!["backlog", "show", "missing"],
        vec!["backlog", "list", "--unknown"],
    ] {
        let mut command = mx();
        command.args(args).env("MX_HOME", &home);
        assert!(!command.output().expect("backlog refusal").status.success());
    }
    let mut handoff = mx();
    handoff
        .args(["backlog-handoff", "missing", "task"])
        .env("MX_HOME", &home)
        .env("MX_ROOT_OVERRIDE", temp.path().join("root"));
    assert_eq!(
        handoff.output().expect("handoff refusal").status.code(),
        Some(1)
    );
    assert_eq!(run(&["operational-input"]).status.code(), Some(2));
    assert_eq!(
        run_stdin(&["operational-input", "encode", "invalid"], b"body")
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run_stdin(&["operational-input", "encode", "watcher"], b"")
            .status
            .code(),
        Some(2)
    );
    assert!(
        run(&[
            "backlog-backend",
            home.join("config").to_str().expect("config"),
        ])
        .status
        .success()
    );

    let atomic = temp.path().join("atomic");
    let invalid_utf8 = temp.path().join("invalid-utf8");
    fs::write(&invalid_utf8, [0xff]).expect("invalid utf8");
    for args in [
        vec!["primitive", "task-id", "bad/id"],
        vec![
            "primitive",
            "atomic-replace",
            atomic.to_str().expect("atomic"),
            "invalid",
        ],
        vec!["primitive", "process-identity", "0"],
        vec![
            "primitive",
            "check-registered",
            temp.path().to_str().expect("state"),
            "bad/id",
        ],
        vec![
            "primitive",
            "status-open-decisions",
            invalid_utf8.to_str().expect("invalid utf8"),
        ],
        vec!["primitive", "probe-install", "unknown"],
        vec![
            "primitive",
            "wake-dedupe",
            temp.path().join("missing").to_str().expect("missing"),
        ],
    ] {
        assert_eq!(run(&args).status.code(), Some(1), "{args:?}");
    }
    assert!(
        run_stdin(&["primitive", "composer-strip-ansi"], b"\x1b[31mred")
            .status
            .success()
    );
    assert!(
        run_stdin(
            &["primitive", "composer-strip-ghost", "--luma-max", "128"],
            b"\x1b[2mghost",
        )
        .status
        .success()
    );
    assert_eq!(
        run_stdin(&["primitive", "marker-is"], b"plain")
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        run_stdin(&["primitive", "marker-mark"], &[0xff])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        run(&["primitive", "supervisor-target"]).status.code(),
        Some(1)
    );
    assert_eq!(
        run(&["primitive", "supervisor-backend"]).status.code(),
        Some(1)
    );
    assert_eq!(
        run(&["primitive", "tangle", temp.path().to_str().expect("temp")])
            .status
            .code(),
        Some(1)
    );
    let mut gate = mx();
    gate.args(["primitive", "gate-refuse"])
        .env("DEEP_REVIEW_GATE", "1");
    assert!(!gate.output().expect("gate refusal").status.success());
}

#[test]
fn config_push_surfaces_registry_fallback_unsafe_dirty_full_and_delivery_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let active = temp.path().join("active");
    let daemon = temp.path().join("registry-daemon");
    let full = temp.path().join("full-daemon");
    fs::create_dir_all(root.join("bin")).expect("root bin");
    for name in ["data", "state", "config"] {
        fs::create_dir_all(active.join(name)).expect("active surface");
    }
    seeded_daemon(&daemon, "worker");
    seeded_daemon(&full, "full");
    fs::write(active.join("config/actor-harness"), b"codex\n").expect("config");
    fs::write(
        active.join("data/daemons.md"),
        format!(
            "- worker - fallback (home: {}; scope: test)\n",
            daemon.display()
        ),
    )
    .expect("registry");
    fs::write(active.join("state/worker.meta"), b"kind=daemon\n").expect("worker meta");
    fs::write(active.join("state/missing.meta"), b"kind=daemon\n").expect("missing meta");
    fs::write(
        active.join("state/unsafe.meta"),
        format!("kind=daemon\nhome={}\n", active.display()),
    )
    .expect("unsafe meta");
    fs::write(active.join("state/actor.meta"), b"kind=actor\n").expect("actor meta");
    fs::write(
        active.join("state/full.meta"),
        format!("kind=daemon\nhome={}\n", full.display()),
    )
    .expect("full meta");

    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(&daemon)
            .args(args)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.test"]);
    fs::write(daemon.join(".gitignore"), b"config/actor-harness\n").expect("gitignore");
    git(&["add", "AGENTS.md", ".gitignore"]);
    git(&["commit", "-m", "fixture"]);
    fs::write(daemon.join("AGENTS.md"), b"# dirty daemon\n").expect("dirty file");

    let retry = active.join("state/.mx-inherited-config-reread-retry/full");
    fs::create_dir_all(&retry).expect("retry dir");
    for index in 0..multplx_domain::inheritance::MAX_PENDING {
        fs::write(
            retry.join(format!(".mx-inherited-config-reread.{index:03}")),
            b"queued\n",
        )
        .expect("queued stage");
    }
    write_executable(
        &root.join("bin/mx-send.sh"),
        "#!/bin/sh\necho delivery-refused >&2\nexit 9\n",
    );
    let configure = |command: &mut Command| {
        command
            .env("MX_ROOT_OVERRIDE", &root)
            .env("MX_HOME", &active)
            .env("MX_DATA_OVERRIDE", active.join("data"))
            .env("MX_STATE_OVERRIDE", active.join("state"))
            .env("MX_CONFIG_OVERRIDE", active.join("config"))
            .env("MX_RUST_SOURCE_ROOT", &root);
    };
    let mut push = mx();
    push.arg("config-push");
    configure(&mut push);
    let output = push.output().expect("push failures");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&daemon.display().to_string()));
    assert!(stdout.contains("no home="));
    assert!(stdout.contains("unsafe home"));
    assert!(stdout.contains("dirty working tree"));
    assert!(stdout.contains("retry instruction queue is full"));
    assert!(stdout.contains("delivery-refused"));

    write_executable(&root.join("bin/mx-send.sh"), "#!/bin/sh\nexit 0\n");
    fs::remove_file(active.join("state/full.meta")).expect("remove full daemon");
    fs::remove_file(active.join("config/actor-harness")).expect("remove source config");
    let mut retry_push = mx();
    retry_push.arg("config-push");
    configure(&mut retry_push);
    let output = retry_push.output().expect("retry push");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("mirrored primary absence"));
    assert!(stdout.contains("config-reread: sent"));

    let mut bad_allowlist = mx();
    bad_allowlist
        .arg("config-push")
        .env("MX_INHERITABLE_CONFIG", "../bad");
    configure(&mut bad_allowlist);
    let output = bad_allowlist.output().expect("invalid allowlist");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("home: error"));
}

#[test]
fn primitive_success_paths_cover_files_scope_locks_environment_and_wake_dedupe() {
    let temp = tempfile::tempdir().expect("tempdir");
    let task = run(&["primitive", "task-id", "task-1"]);
    assert!(task.status.success());
    assert_eq!(task.stdout, b"task-1");

    let atomic = temp.path().join("atomic");
    let output = run_stdin(
        &[
            "primitive",
            "atomic-replace",
            atomic.to_str().expect("atomic"),
            "0600",
        ],
        b"atomic bytes",
    );
    assert!(output.status.success());
    assert_eq!(fs::read(&atomic).expect("atomic bytes"), b"atomic bytes");

    let lock = temp.path().join("index.lock");
    fs::write(&lock, b"").expect("lock");
    let lock_result = run(&[
        "primitive",
        "git-lock-stale",
        lock.to_str().expect("lock"),
        "0",
        "4102444800",
    ]);
    assert!(matches!(lock_result.status.code(), Some(0) | Some(1)));

    let marked = run_stdin(
        &["primitive", "marker-is"],
        "[mx-from-broker]\u{2063}message".as_bytes(),
    );
    assert!(marked.status.success());
    assert!(run(&["primitive", "gate-refuse"]).status.success());

    let daemon = temp.path().join("daemon");
    seeded_daemon(&daemon, "worker");
    assert!(
        run(&[
            "primitive",
            "primary-scope",
            daemon.to_str().expect("daemon"),
            daemon.join("state").to_str().expect("state"),
        ])
        .status
        .success()
    );

    let queue = temp.path().join("queue");
    fs::write(
        &queue,
        b"1\t1\tsignal\ttask.status\told\n2\t2\tsignal\ttask.status\tnew\n3\t3\theartbeat\tone\tbeat\n",
    )
    .expect("queue");
    let output = run(&["primitive", "wake-dedupe", queue.to_str().expect("queue")]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\tnew\n"));
    assert!(!stdout.contains("\told\n"));

    let mut supervisor = mx();
    supervisor
        .args(["primitive", "supervisor-backend"])
        .env("MX_SUPERVISOR_BACKEND", "tmux");
    assert!(supervisor.output().expect("backend").status.success());
}

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent");
    fs::write(path, body).expect("script");
    fs::set_permissions(path, Permissions::from_mode(0o755)).expect("mode");
}

fn command(home: &Path, root: &Path, fake_bin: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    let path = std::env::var_os("PATH").unwrap_or_default();
    command
        .env("MX_HOME", home)
        .env("MX_ROOT_OVERRIDE", root)
        .env("MX_RUST_SOURCE_ROOT", root)
        .env("MX_SEND_SETTLE", "0")
        .env(
            "PATH",
            format!("{}:{}", fake_bin.display(), PathBuf::from(path).display()),
        );
    command
}

fn run(command: &mut Command) -> Output {
    command.output().expect("run mx")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_teardown_reports_owned_and_manual_backlog_follow_up() {
    for manual in [false, true] {
        let temp = tempfile::tempdir().expect("tempdir");
        let (home, root, fake) = fixture(&temp);
        fs::create_dir_all(home.join("config")).expect("config");
        if manual {
            fs::write(home.join("config/backlog-backend"), "manual\n").expect("backend");
        }
        fs::write(
            home.join("state/task.meta"),
            "window=missing\nkind=delivery\nmode=deep-review\npr=https://example.invalid/pull/7\n",
        )
        .expect("meta");
        let output = run(command(&home, &root, &fake).args(["teardown", "task"]));
        assert_success(&output);
        let text = String::from_utf8(output.stdout).expect("UTF-8");
        assert!(text.starts_with("teardown task complete"));
        if manual {
            assert!(text.contains("Update data/backlog.md - move task to Done"));
            assert!(!text.contains("bin/mx-backlog.sh done"));
        } else {
            assert!(
                text.contains("bin/mx-backlog.sh done task --pr https://example.invalid/pull/7")
            );
            assert!(text.contains("bin/mx-backlog.sh ready"));
        }
        assert!(!home.join("state/task.meta").exists());
    }
}

fn fixture(temp: &tempfile::TempDir) -> (PathBuf, PathBuf, PathBuf) {
    let home = temp.path().join("home");
    let root = temp.path().join("root");
    let fake = temp.path().join("fake-bin");
    fs::create_dir_all(home.join("state")).expect("state");
    fs::create_dir_all(home.join("data")).expect("data");
    fs::create_dir_all(&root).expect("root");
    executable(
        &fake.join("tmux"),
        r#"#!/bin/sh
case "${1:-}" in
  display-message)
    if [ "${MX_TMUX_DEAD:-0}" = 1 ]; then exit 1; fi
    case "$*" in *cursor_y*) printf '0\n' ;; *) printf '%%1\n' ;; esac ;;
  capture-pane)
    if [ "${MX_TMUX_PENDING:-0}" = 1 ]; then printf '│ left pending │\n'; else printf '│ │\n'; fi ;;
  send-keys) ;;
  *) exit 0 ;;
esac
"#,
    );
    executable(&fake.join("sleep"), "#!/bin/sh\nexit 0\n");
    executable(&root.join("bin/mx-guard.sh"), "#!/bin/sh\nexit 0\n");
    (home, root, fake)
}

#[test]
fn send_command_covers_environment_success_keys_and_daemon_delivery() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (home, root, fake) = fixture(&temp);
    fs::write(
        home.join("state/task.meta"),
        "kind=actor\nwindow=session:task\nbackend=tmux\nharness=codex\n",
    )
    .expect("task meta");
    fs::write(
        home.join("state/daemon.meta"),
        "kind=daemon\nwindow=session:daemon\nbackend=tmux\nharness=codex\n",
    )
    .expect("daemon meta");
    fs::write(
        home.join("state/no-target.meta"),
        "kind=actor\nbackend=tmux\n",
    )
    .expect("missing target meta");

    for arguments in [
        vec!["send", "mx-task", "hello"],
        vec!["send", "mx-task", "$status"],
        vec!["send", "mx-task", "--key", "Enter"],
        vec!["send", "mx-daemon", "/review", "now"],
    ] {
        assert_success(&run(command(&home, &root, &fake).args(arguments)));
    }
    let pending = home.join("state/pending-replies");
    let record = fs::read_dir(&pending)
        .expect("pending replies")
        .filter_map(Result::ok)
        .find(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .expect("record");
    let correlation = record.file_name().to_string_lossy().into_owned();
    assert_success(&run(command(&home, &root, &fake).args([
        "send",
        "mx-daemon",
        &format!("corr={correlation} follow up"),
    ])));

    let mut no_home = Command::new(env!("CARGO_BIN_EXE_mx"));
    no_home
        .args(["send", "target", "text"])
        .env_remove("MX_HOME");
    assert_eq!(run(&mut no_home).status.code(), Some(1));
    assert_eq!(
        run(command(&temp.path().join("missing"), &root, &fake).args(["send", "target", "text"]))
            .status
            .code(),
        Some(1)
    );
    let no_state = temp.path().join("no-state");
    fs::create_dir(&no_state).expect("no state home");
    assert_eq!(
        run(command(&no_state, &root, &fake).args(["send", "target", "text"]))
            .status
            .code(),
        Some(1)
    );

    assert_eq!(
        run(command(&home, &root, &fake)
            .env("DEEP_REVIEW_GATE", "1")
            .args(["send", "mx-task", "refused"]))
        .status
        .code(),
        Some(3)
    );
    assert_eq!(
        run(command(&home, &root, &fake)
            .env("MX_TMUX_PENDING", "1")
            .env("MX_SEND_RETRIES", "0")
            .args(["send", "mx-task", "left pending"]))
        .status
        .code(),
        Some(1)
    );
    assert_success(&run(command(&home, &root, &fake)
        .env("MX_SEND_SETTLE", "0.01")
        .args(["send", "mx-task", "settled"])));
    assert_eq!(
        run(command(&home, &root, &fake).args(["send", "mx-no-target", "text"]))
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        run(command(&home, &root, &fake).env("MX_TMUX_DEAD", "1").args([
            "send",
            "missing:target",
            "text"
        ]))
        .status
        .code(),
        Some(1)
    );

    let create_failure = temp.path().join("create-failure");
    fs::create_dir_all(create_failure.join("state")).expect("failure state");
    fs::write(
        create_failure.join("state/daemon.meta"),
        "kind=daemon\nwindow=session:daemon\nbackend=tmux\n",
    )
    .expect("failure meta");
    fs::write(
        create_failure.join("state/pending-replies"),
        "not a directory",
    )
    .expect("pending file");
    assert_eq!(
        run(command(&create_failure, &root, &fake).args(["send", "mx-daemon", "request",]))
            .status
            .code(),
        Some(1)
    );

    let broken = temp.path().join("broken-home");
    fs::create_dir(&broken).expect("broken home");
    fs::write(broken.join("state"), "not a directory").expect("state file");
    assert_eq!(
        run(command(&broken, &root, &fake).args(["send", "target", "text"]))
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn lifecycle_dispatch_covers_briefs_reports_update_and_compatibility_refusals() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (home, root, fake) = fixture(&temp);
    fs::write(home.join("data/projects.md"), "- app [direct-PR] - app\n").expect("registry");
    assert_success(&run(
        command(&home, &root, &fake).args(["brief", "task", "app"])
    ));
    assert_eq!(
        run(command(&home, &root, &fake).args(["brief", "task", "app"]))
            .status
            .code(),
        Some(1)
    );

    let status = temp.path().join("reports/daemon.status");
    assert_success(&run(command(&home, &root, &fake).args([
        "daemon-report",
        status.to_str().expect("status"),
        "done",
        "0123456789abcdef",
        "complete",
    ])));
    assert_success(&run(command(&home, &root, &fake).args([
        "daemon-report",
        "--doc",
        status.to_str().expect("status"),
        "done",
        "0123456789abcdef",
        "data/report.md",
    ])));
    assert_success(&run(command(&home, &root, &fake).args([
        "daemon-report",
        status.to_str().expect("status"),
        "done",
        "0123456789abcdef",
        "",
    ])));
    assert_eq!(
        run(command(&home, &root, &fake).args([
            "daemon-report",
            "",
            "done",
            "0123456789abcdef",
            "note",
        ]))
        .status
        .code(),
        Some(1)
    );
    let parent_file = temp.path().join("parent-file");
    fs::write(&parent_file, "x").expect("parent file");
    assert_eq!(
        run(command(&home, &root, &fake).args([
            "daemon-report",
            parent_file.join("status").to_str().expect("bad status"),
            "done",
            "0123456789abcdef",
            "note",
        ]))
        .status
        .code(),
        Some(1)
    );
    assert_eq!(
        run(command(&home, &root, &fake).args([
            "daemon-report",
            "/",
            "done",
            "0123456789abcdef",
            "note",
        ]))
        .status
        .code(),
        Some(1)
    );

    let broker = temp.path().join("broker");
    fs::create_dir(&broker).expect("broker");
    let init = Command::new("git")
        .arg("-C")
        .arg(&broker)
        .args(["init", "-b", "main", "--quiet"])
        .output()
        .expect("git init");
    assert!(init.status.success());
    assert_success(&run(command(&home, &broker, &fake)
        .env("MX_STATE_OVERRIDE", home.join("state"))
        .args(["update"])));

    for (lifecycle, expected) in [
        ("home-seed", 0),
        ("spawn", 1),
        ("teardown", 2),
        ("upstream-diff", 1),
    ] {
        assert_eq!(
            run(command(&home, &broker, &fake).args([lifecycle]))
                .status
                .code(),
            Some(expected),
            "unexpected zero-argument status for {lifecycle}"
        );
    }

    assert_eq!(
        run(command(&home, &broker, &fake).args(["supervise-daemon", "unexpected"]))
            .status
            .code(),
        Some(2)
    );
    let watcher = temp.path().join("watcher");
    executable(&watcher, "#!/bin/sh\nexit 0\n");
    let supervisor_state = temp.path().join("supervisor-state");
    let mut supervisor_command = command(&home, &broker, &fake);
    supervisor_command
        .env("MX_STATE_OVERRIDE", &supervisor_state)
        .env("MX_SUPERVISE_WATCH_EXEC", &watcher)
        .arg("supervise-daemon");
    let mut supervisor = supervisor_command.spawn().expect("spawn supervisor");
    let pidfile = supervisor_state.join(".supervise-daemon.pid");
    for _ in 0..100 {
        if pidfile.is_file() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(pidfile.is_file(), "supervisor did not publish its pid");
    assert!(
        supervisor.try_wait().expect("poll supervisor").is_none(),
        "zero-argument supervisor did not remain in the foreground"
    );
    assert!(
        Command::new("kill")
            .args(["-TERM", &supervisor.id().to_string()])
            .status()
            .expect("signal supervisor")
            .success()
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let supervisor_status = loop {
        if let Some(status) = supervisor.try_wait().expect("wait supervisor") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = supervisor.kill();
            panic!("supervisor did not stop after SIGTERM");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert!(
        supervisor_status.success() || supervisor_status.signal() == Some(15),
        "supervisor exited unexpectedly after SIGTERM: {supervisor_status:?}"
    );
    assert!(!pidfile.exists(), "supervisor retained its pidfile");

    for arguments in [
        vec!["fast-forward", "default-branch", "/definitely/missing"],
        vec!["fast-forward", "primary-head", "/definitely/missing"],
        vec![
            "fast-forward",
            "validate-home",
            root.to_str().expect("root"),
            home.to_str().expect("home"),
            "id",
            "/definitely/missing",
        ],
        vec![
            "pending-reply",
            "prepare",
            home.to_str().expect("home"),
            "missing",
        ],
    ] {
        assert_eq!(
            run(command(&home, &root, &fake).args(arguments))
                .status
                .code(),
            Some(1)
        );
    }
    assert_success(&run(command(&home, &broker, &fake).args([
        "fast-forward",
        "target",
        broker.to_str().expect("broker"),
        "broker",
        "origin",
        "no",
        "no",
    ])));
}

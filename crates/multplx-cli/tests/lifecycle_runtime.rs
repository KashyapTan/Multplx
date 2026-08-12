use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
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

    for lifecycle in [
        "home-seed",
        "spawn",
        "supervise-daemon",
        "teardown",
        "upstream-diff",
    ] {
        assert_eq!(
            run(command(&home, &broker, &fake).args([lifecycle]))
                .status
                .code(),
            Some(1)
        );
    }

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

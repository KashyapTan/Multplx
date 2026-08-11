use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write executable");
    fs::set_permissions(path, Permissions::from_mode(0o755)).expect("mode");
}

#[allow(clippy::too_many_arguments)]
fn run_adapter(
    script: &str,
    implementation: &str,
    fakebin: &Path,
    home: &Path,
    state: &Path,
    log: &Path,
    args: &[&str],
    extra: &[(&str, &str)],
) -> Output {
    let neutral_root = home.join("neutral-root");
    fs::create_dir_all(&neutral_root).expect("neutral root");
    let mut command = Command::new(root().join("bin").join(script));
    command
        .args(args)
        .env(
            "PATH",
            format!(
                "{}:{}",
                fakebin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("MX_ROOT_OVERRIDE", neutral_root)
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", state)
        .env("MX_BACKEND_IMPLEMENTATION", implementation)
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .env("MX_TMUX_LOG", log);
    for (key, value) in extra {
        command.env(key, value);
    }
    command.output().expect("adapter output")
}

fn run_mx(fakebin: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mx"))
        .args(args)
        .env(
            "PATH",
            format!(
                "{}:{}",
                fakebin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env_remove("TMUX")
        .output()
        .expect("mx output")
}

#[test]
fn peek_matches_status_streams_bytes_and_tmux_arguments() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fakebin = temp.path().join("fakebin");
    let home = temp.path().join("home");
    let state = home.join("state");
    fs::create_dir_all(&fakebin).expect("fakebin");
    fs::create_dir_all(&state).expect("state");
    executable(
        &fakebin.join("tmux"),
        r#"#!/bin/sh
printf 'tmux' >> "$MX_TMUX_LOG"
for argument in "$@"; do printf '\037%s' "$argument" >> "$MX_TMUX_LOG"; done
printf '\n' >> "$MX_TMUX_LOG"
case "$1" in
  capture-pane) printf 'line one\nline two\n' ;;
  list-windows) printf 'broker:other\n' ;;
esac
"#,
    );
    let legacy_log = temp.path().join("legacy.log");
    let rust_log = temp.path().join("rust.log");
    let legacy = run_adapter(
        "mx-peek.sh",
        "legacy",
        &fakebin,
        &home,
        &state,
        &legacy_log,
        &["broker:mx-one", "7"],
        &[],
    );
    let rust = run_adapter(
        "mx-peek.sh",
        "rust",
        &fakebin,
        &home,
        &state,
        &rust_log,
        &["broker:mx-one", "7"],
        &[],
    );
    assert_eq!(rust.status.code(), legacy.status.code());
    assert_eq!(rust.stdout, legacy.stdout);
    assert_eq!(rust.stderr, legacy.stderr);
    assert_eq!(
        fs::read(rust_log).expect("Rust log"),
        fs::read(legacy_log).expect("legacy log")
    );
    assert_eq!(rust.stdout, b"line one\nline two\n");
}

fn git_fixture(path: &Path) {
    fs::create_dir_all(path).expect("repo");
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "test"][..],
        &["config", "user.email", "test@example.invalid"][..],
        &["commit", "-q", "--allow-empty", "-m", "base"][..],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(path)
                .args(args)
                .status()
                .expect("git")
                .success()
        );
    }
}

#[test]
fn actor_state_matches_report_busy_gone_and_missing_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fakebin = temp.path().join("fakebin");
    let home = temp.path().join("home");
    let state = home.join("state");
    let worktree = temp.path().join("worktree");
    fs::create_dir_all(&fakebin).expect("fakebin");
    fs::create_dir_all(&state).expect("state");
    git_fixture(&worktree);
    let worktree = worktree.canonicalize().expect("worktree");
    executable(
        &fakebin.join("tmux"),
        r#"#!/bin/sh
case "$1" in
  display-message) [ "${MX_FAKE_GONE:-0}" = 1 ] && exit 1; printf '%%1\n' ;;
  capture-pane) printf '%s\n' "${MX_FAKE_PANE_TEXT:-idle prompt}" ;;
esac
"#,
    );
    fs::write(
        state.join("one.meta"),
        format!(
            "window=broker:mx-one\nworktree={}\nkind=delivery\n",
            worktree.display()
        ),
    )
    .expect("meta");
    fs::write(state.join("one.status"), "paused: release window\n").expect("status");

    for (args, extra) in [
        (vec!["one"], vec![]),
        (vec!["one"], vec![("MX_FAKE_GONE", "1")]),
        (vec!["missing"], vec![]),
    ] {
        let legacy = run_adapter(
            "mx-actor-state.sh",
            "legacy",
            &fakebin,
            &home,
            &state,
            &temp.path().join("unused-legacy"),
            &args,
            &extra,
        );
        let rust = run_adapter(
            "mx-actor-state.sh",
            "rust",
            &fakebin,
            &home,
            &state,
            &temp.path().join("unused-rust"),
            &args,
            &extra,
        );
        assert_eq!(
            rust.status.code(),
            legacy.status.code(),
            "status for {args:?}"
        );
        assert_eq!(rust.stdout, legacy.stdout, "stdout for {args:?}");
        assert_eq!(rust.stderr, legacy.stderr, "stderr for {args:?}");
    }

    fs::remove_file(state.join("one.status")).expect("remove status");
    let legacy = run_adapter(
        "mx-actor-state.sh",
        "legacy",
        &fakebin,
        &home,
        &state,
        &temp.path().join("unused-legacy-busy"),
        &["one"],
        &[("MX_FAKE_PANE_TEXT", "Working... esc to interrupt")],
    );
    let rust = run_adapter(
        "mx-actor-state.sh",
        "rust",
        &fakebin,
        &home,
        &state,
        &temp.path().join("unused-rust-busy"),
        &["one"],
        &[("MX_FAKE_PANE_TEXT", "Working... esc to interrupt")],
    );
    assert_eq!(rust.stdout, legacy.stdout);
    assert_eq!(
        rust.stdout,
        b"state: working \xc2\xb7 source: pane \xc2\xb7 harness busy\n"
    );
}

#[test]
fn invalid_shadow_selector_fails_before_backend_execution() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fakebin = temp.path().join("fakebin");
    let home = temp.path().join("home");
    let state = home.join("state");
    fs::create_dir_all(&fakebin).expect("fakebin");
    fs::create_dir_all(&state).expect("state");
    let marker = temp.path().join("called");
    executable(
        &fakebin.join("tmux"),
        &format!("#!/bin/sh\nprintf called > '{}'\n", marker.display()),
    );
    let output = run_adapter(
        "mx-peek.sh",
        "rust",
        &fakebin,
        &home,
        &state,
        &temp.path().join("unused"),
        &["../escape", "4"],
        &[],
    );
    assert!(!output.status.success());
    assert!(!marker.exists(), "invalid selector reached tmux");

    let output = run_adapter(
        "mx-peek.sh",
        "unknown",
        &fakebin,
        &home,
        &state,
        &temp.path().join("unused-implementation"),
        &["one", "4"],
        &[],
    );
    assert!(!output.status.success());
    assert!(!marker.exists(), "invalid implementation reached tmux");

    executable(
        &fakebin.join("tmux"),
        &format!(
            "#!/bin/sh\nprintf called >> '{}'\nprintf '\\n' >> '{}'\nexit 9\n",
            marker.display(),
            marker.display()
        ),
    );
    let output = run_adapter(
        "mx-peek.sh",
        "rust",
        &fakebin,
        &home,
        &state,
        &temp.path().join("failed-command"),
        &["broker:mx-one", "4"],
        &[],
    );
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(marker)
            .expect("backend call marker")
            .lines()
            .count(),
        1,
        "a failed Rust operation fell back and retried through legacy"
    );
}

#[test]
fn hidden_backend_cli_exercises_the_complete_tmux_facade() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fakebin = temp.path().join("fakebin");
    fs::create_dir_all(&fakebin).expect("fakebin");
    executable(
        &fakebin.join("tmux"),
        r#"#!/bin/sh
case "$1" in
  -V) printf 'tmux 3.6a\n' ;;
  has-session) ;;
  list-windows)
    case " $* " in
      *' -a '*) printf 'broker:mx-one\n' ;;
      *) printf 'mx-one\n' ;;
    esac
    ;;
  new-window) printf '@9\n' ;;
  display-message)
    case "$*" in
      *pane_current_path*) printf '/tmp\n' ;;
      *pane_current_command*) printf 'claude\n' ;;
      *pane_id*) printf '%%1\n' ;;
      *cursor_y*) printf '0\n' ;;
      *'#S'*) printf 'broker\n' ;;
    esac
    ;;
  capture-pane)
    case " $* " in
      *' -e '*) printf '\033[1mtyped\033[0m\n' ;;
      *) printf 'Working... esc to interrupt\n' ;;
    esac
    ;;
esac
"#,
    );
    let commands = [
        vec!["backend", "tool-check"],
        vec!["backend", "version-check"],
        vec!["backend", "container-ensure"],
        vec!["backend", "task-create", "broker", "mx-two", "/tmp"],
        vec!["backend", "target-ready", "broker:mx-one"],
        vec!["backend", "current-path", "broker:mx-one"],
        vec!["backend", "current-command", "broker:mx-one"],
        vec!["backend", "capture", "broker:mx-one", "4"],
        vec!["backend", "composer-state", "broker:mx-one"],
        vec!["backend", "send-literal", "broker:mx-one", "hello"],
        vec!["backend", "send-key", "broker:mx-one", "Enter"],
        vec![
            "backend",
            "send-submit",
            "broker:mx-one",
            "hello",
            "1",
            "0",
            "0",
        ],
        vec!["backend", "send-text-line", "broker:mx-one", "hello"],
        vec!["backend", "kill", "broker:mx-one"],
        vec!["backend", "agent-state", "broker:mx-one"],
        vec!["backend", "agent-alive", "broker:mx-one"],
        vec!["backend", "list-live"],
        vec!["backend", "list-live", "--container", "broker"],
        vec!["backend", "resolve-bare", "mx-one"],
    ];
    for args in commands {
        let output = run_mx(&fakebin, &args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    for args in [
        &["backend", "task-create", "broker", "bad:name", "/tmp"][..],
        &[
            "backend",
            "send-submit",
            "broker:mx-one",
            "hello",
            "1",
            "bad",
            "0",
        ][..],
        &["backend", "resolve-bare", "missing"][..],
    ] {
        assert!(!run_mx(&fakebin, args).status.success(), "{args:?}");
    }
}

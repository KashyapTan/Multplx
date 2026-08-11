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

fn run(fake: &Path, log: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mx"))
        .args(args)
        .env("MX_HERDR_BIN", fake)
        .env("MX_HERDR_LOG", log)
        .env("MX_HOME", home)
        .env("HERDR_SESSION", "named")
        .output()
        .expect("run mx")
}

#[test]
fn rust_herdr_runtime_scopes_named_sessions_and_parses_typed_responses() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let fake = temp.path().join("herdr");
    let log = temp.path().join("herdr.log");
    fs::create_dir(&home).expect("home");
    executable(
        &fake,
        r#"#!/bin/sh
printf 'HERDR_SESSION=%s' "${HERDR_SESSION:-}" >> "$MX_HERDR_LOG"
for argument in "$@"; do printf '\037%s' "$argument" >> "$MX_HERDR_LOG"; done
printf '\n' >> "$MX_HERDR_LOG"
case "$1 ${2:-}" in
  "status --json") printf '%s\n' '{"client":{"version":"0.7.4","protocol":16},"server":{"running":true}}' ;;
  "workspace list") printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"w1","label":"broker"}]}}' ;;
  "tab list") printf '%s\n' '{"result":{"tabs":[]}}' ;;
  "tab create") printf '%s\n' '{"result":{"tab":{"tab_id":"w1:t2"},"root_pane":{"pane_id":"w1:p2"}}}' ;;
  "pane get") printf '%s\n' '{"result":{"pane":{"pane_id":"w1:p2","tab_id":"w1:t2","workspace_id":"w1","foreground_cwd":"/tmp/work"}}}' ;;
  "pane read") printf 'one\ntwo\nthree\n' ;;
  "agent get") printf '%s\n' '{"result":{"agent":{"agent_status":"working"}}}' ;;
esac
"#,
    );
    let container = run(
        &fake,
        &log,
        &home,
        &["herdr", "container-ensure", "/tmp/work"],
    );
    assert!(
        container.status.success(),
        "{}",
        String::from_utf8_lossy(&container.stderr)
    );
    assert_eq!(container.stdout, b"named:w1\t");

    let created = run(
        &fake,
        &log,
        &home,
        &["herdr", "task-create", "named:w1", "mx-task", "/tmp/work"],
    );
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert_eq!(created.stdout, b"w1:t2 w1:p2");

    let path = run(
        &fake,
        &log,
        &home,
        &["herdr", "current-path", "named:w1:p2"],
    );
    assert!(path.status.success());
    assert_eq!(path.stdout, b"/tmp/work");

    let capture = run(
        &fake,
        &log,
        &home,
        &["herdr", "capture", "named:w1:p2", "2"],
    );
    assert!(capture.status.success());
    assert_eq!(capture.stdout, b"two\nthree\n");

    let state = run(
        &fake,
        &log,
        &home,
        &["herdr", "native-state", "named:w1:p2"],
    );
    assert!(state.status.success());
    assert_eq!(state.stdout, b"working");

    let sent = run(
        &fake,
        &log,
        &home,
        &["herdr", "send-literal", "named:w1:p2", "literal text"],
    );
    assert!(sent.status.success());

    let log = fs::read_to_string(log).expect("log");
    for line in log
        .lines()
        .filter(|line| !line.contains("\x1fstatus\x1f--json") || line.contains("\x1f--session"))
    {
        if line.contains("\x1fworkspace")
            || line.contains("\x1ftab")
            || line.contains("\x1fpane")
            || line.contains("\x1fagent")
        {
            assert!(
                line.ends_with("\x1f--session\x1fnamed"),
                "unscoped Herdr call: {line:?}"
            );
            assert!(line.starts_with("HERDR_SESSION=named"));
        }
    }
    assert!(log.contains("\x1fpane\x1fsend-text\x1fw1:p2\x1fliteral text\x1f--session\x1fnamed"));
}

#[test]
fn selected_shell_adapter_uses_rust_without_python_or_jq() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let empty_path = temp.path().join("bin");
    let fake = empty_path.join("herdr");
    fs::create_dir(&home).expect("home");
    fs::create_dir(&empty_path).expect("bin");
    executable(
        &fake,
        r#"#!/bin/sh
case "$1 ${2:-}" in
  "workspace list") printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"w9","label":"broker"}]}}' ;;
esac
"#,
    );
    let forbidden = temp.path().join("forbidden-tool");
    for name in ["jq", "python3"] {
        executable(
            &empty_path.join(name),
            &format!(
                "#!/bin/sh\nprintf '%s' invoked > '{}'\nexit 99\n",
                forbidden.display()
            ),
        );
    }
    let script = format!(
        ". '{}/bin/mx-backend.sh'; mx_backend_source herdr; mx_backend_herdr_workspace_find named",
        root().display()
    );
    let output = Command::new("/bin/bash")
        .args(["-c", &script])
        .env("PATH", format!("{}:/usr/bin:/bin", empty_path.display()))
        .env("MX_BACKEND_IMPLEMENTATION", "rust")
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .env("MX_HERDR_BIN", &fake)
        .env("MX_HOME", &home)
        .output()
        .expect("shell adapter");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"w9");
    assert!(!forbidden.exists(), "Rust adapter invoked jq or Python");
}

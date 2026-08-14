use std::fs::{self, Permissions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

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

fn success_output(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
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

#[test]
fn hidden_herdr_cli_exercises_runtime_journal_focus_and_refusal_surfaces() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let state = home.join("state");
    let fake = temp.path().join("herdr");
    let log = temp.path().join("herdr.log");
    let socket = temp.path().join("named.sock");
    fs::create_dir(&home).expect("home");
    fs::create_dir(&state).expect("state");
    let script = r#"#!/bin/sh
printf 'HERDR_SESSION=%s' "${HERDR_SESSION:-}" >> "$MX_HERDR_LOG"
for argument in "$@"; do printf '\037%s' "$argument" >> "$MX_HERDR_LOG"; done
printf '\n' >> "$MX_HERDR_LOG"
workspace=''
previous=''
for argument in "$@"; do
  if [ "$previous" = '--workspace' ]; then workspace=$argument; fi
  previous=$argument
done
case "$1 ${2:-}" in
  "--version ") printf '%s\n' 'herdr 0.7.4' ;;
  "status --json") printf '%s\n' '{"client":{"version":"0.7.4","protocol":16},"server":{"running":true}}' ;;
  "workspace list") printf '%s\n' '{"result":{"workspaces":[{"workspace_id":"parent","label":"broker","focused":true,"active_tab_id":"parent:t1"},{"workspace_id":"child","label":"└ task · p:abcdefghijklmnopqrstuv","focused":false,"active_tab_id":"child:t1"}]}}' ;;
  "workspace create") printf '%s\n' '{"result":{"workspace":{"workspace_id":"new"},"tab":{"tab_id":"new:seed"},"root_pane":{"pane_id":"new:seed-pane"}}}' ;;
  "tab list")
    case "$workspace" in
      parent) printf '%s\n' '{"result":{"tabs":[{"tab_id":"parent:t1","workspace_id":"parent","label":"maintainer","focused":true},{"tab_id":"parent:t2","workspace_id":"parent","label":"mx-task","focused":false}]}}' ;;
      new) printf '%s\n' '{"result":{"tabs":[{"tab_id":"new:t2","workspace_id":"new","label":"mx-task","focused":false}]}}' ;;
      *) printf '%s\n' '{"result":{"tabs":[{"tab_id":"child:t1","workspace_id":"child","label":"mx-task","focused":false}]}}' ;;
    esac ;;
  "tab create") printf '%s\n' '{"result":{"tab":{"tab_id":"new:t2"},"root_pane":{"pane_id":"new:p2"}}}' ;;
  "tab get")
    case "$3" in
      parent:*) printf '%s\n' "{\"result\":{\"tab\":{\"tab_id\":\"$3\",\"workspace_id\":\"parent\"}}}" ;;
      *) printf '%s\n' "{\"result\":{\"tab\":{\"tab_id\":\"$3\",\"workspace_id\":\"new\"}}}" ;;
    esac ;;
  "pane list")
    case "$workspace" in
      parent) printf '%s\n' '{"result":{"panes":[{"pane_id":"parent:p2","tab_id":"parent:t2","workspace_id":"parent"}]}}' ;;
      new) printf '%s\n' '{"result":{"panes":[{"pane_id":"new:p2","tab_id":"new:t2","workspace_id":"new"}]}}' ;;
      *) printf '%s\n' '{"result":{"panes":[{"pane_id":"child:p1","tab_id":"child:t1","workspace_id":"child"}]}}' ;;
    esac ;;
  "pane get")
    pane=$3
    case "$pane" in
      parent:*) tab='parent:t2'; ws='parent' ;;
      new:*) tab='new:t2'; ws='new' ;;
      *) tab='child:t1'; ws='child' ;;
    esac
    printf '%s\n' "{\"result\":{\"pane\":{\"pane_id\":\"$pane\",\"tab_id\":\"$tab\",\"workspace_id\":\"$ws\",\"foreground_cwd\":\"/tmp/work\"}}}" ;;
  "pane read") printf '%s\n' '│ typed │' ;;
  "agent get")
    case "$3" in
      child:p1|new:seed-pane) printf '%s\n' '{"error":{"code":"agent_not_found"}}' ;;
      idle) printf '%s\n' '{"result":{"agent":{"agent":"codex","agent_status":"idle"}}}' ;;
      blocked) printf '%s\n' '{"result":{"agent":{"agent":"codex","agent_status":"blocked"}}}' ;;
      done) printf '%s\n' '{"result":{"agent":{"agent":"codex","agent_status":"done"}}}' ;;
      unknown) printf '%s\n' '{"result":{"agent":{"agent":"codex","agent_status":"mystery"}}}' ;;
      *) printf '%s\n' '{"result":{"agent":{"agent":"codex","agent_status":"working"}}}' ;;
    esac ;;
  "session list") printf '%s\n' '{"sessions":[{"name":"named","running":true,"socket_path":"__SOCKET__"}]}' ;;
  "api schema") printf '%s\n' '{"methods":{"events.subscribe":{},"workspace.move":{"params":["workspace_id","insert_index"]}},"events":["pane.agent_status_changed"]}' ;;
esac
"#
    .replace("__SOCKET__", &socket.to_string_lossy());
    executable(&fake, &script);

    assert_eq!(
        success_output(run(&fake, &log, &home, &["herdr", "workspace-label"])),
        b"broker"
    );
    success_output(run(&fake, &log, &home, &["herdr", "tool-check"]));
    success_output(run(&fake, &log, &home, &["herdr", "version-check"]));
    success_output(run(
        &fake,
        &log,
        &home,
        &["herdr", "server-ensure", "named"],
    ));
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "workspace-find", "named"]
        )),
        b"parent"
    );
    success_output(run(
        &fake,
        &log,
        &home,
        &["herdr", "target-ready", "named:parent:p2"],
    ));
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "current-path", "named:parent:p2"],
        )),
        b"/tmp/work"
    );
    for command in ["capture", "capture-ansi"] {
        assert_eq!(
            success_output(run(
                &fake,
                &log,
                &home,
                &["herdr", command, "named:parent:p2", "20"],
            )),
            "│ typed │\n".as_bytes()
        );
    }
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "composer-state", "named:parent:p2"],
        )),
        b"pending"
    );
    for args in [
        vec!["herdr", "send-literal", "named:parent:p2", "literal"],
        vec!["herdr", "send-key", "named:parent:p2", "Escape"],
        vec!["herdr", "send-text-line", "named:parent:p2", "line"],
    ] {
        success_output(run(&fake, &log, &home, &args));
    }
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "send-submit",
                "named:parent:p2",
                "text",
                "1",
                "0",
                "0"
            ],
        )),
        b"pending"
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "native-state", "named:parent:p2"],
        )),
        b"working"
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "busy-state", "named:parent:p2"],
        )),
        b"busy"
    );
    for (pane, expected) in [("idle", "idle"), ("blocked", "blocked"), ("done", "done")] {
        assert_eq!(
            success_output(run(
                &fake,
                &log,
                &home,
                &["herdr", "native-state", &format!("named:{pane}")],
            )),
            expected.as_bytes()
        );
    }
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "busy-state", "named:idle"],
        )),
        b"idle"
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "busy-state", "named:unknown"],
        )),
        b"unknown"
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "pane-agent-state", "named", "parent:p2"],
        )),
        b"live"
    );
    for (command, expected) in [
        ("agent-state", b"alive".as_slice()),
        ("agent-alive", b"alive"),
    ] {
        assert_eq!(
            success_output(run(
                &fake,
                &log,
                &home,
                &["herdr", command, "named:parent:p2"],
            )),
            expected
        );
    }
    success_output(run(
        &fake,
        &log,
        &home,
        &["herdr", "kill", "named:parent:p2"],
    ));
    assert!(
        String::from_utf8(success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "list-live", "named"],
        )))
        .expect("inventory")
        .contains("named:parent:p2\tmx-task")
    );
    success_output(run(
        &fake,
        &log,
        &home,
        &["herdr", "events-capable", "named"],
    ));
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "container-ensure", home.to_str().expect("home")],
        )),
        b"named:parent\t"
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "task-create",
                "named:parent",
                "fresh-task",
                home.to_str().expect("home"),
            ],
        )),
        b"new:t2 new:p2"
    );
    assert!(
        String::from_utf8(success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "cli", "named", "status", "--json"],
        )))
        .expect("raw cli")
        .contains("\"protocol\":16")
    );
    let transition_state = state.join("transitions");
    fs::create_dir(&transition_state).expect("transition state");
    success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "transition-commit",
            transition_state.to_str().expect("transition state"),
            "named",
            "parent:p2\tparent\tidle\tblocked\tclaude",
        ],
    ));
    success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "transition-clear",
            transition_state.to_str().expect("transition state"),
            "named:parent:p2",
        ],
    ));

    assert_eq!(
        success_output(run(&fake, &log, &home, &["herdr", "normalize-key", "C-c"])),
        b"ctrl+c"
    );
    assert_eq!(
        success_output(run(&fake, &log, &home, &["herdr", "normalize-key", "F5"])),
        b"F5"
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "journal-path",
                state.to_str().expect("state"),
                "task"
            ]
        )),
        state
            .join("task.herdr-presentation")
            .to_string_lossy()
            .as_bytes()
    );
    assert_eq!(
        success_output(run(&fake, &log, &home, &["herdr", "projection-id"])).len(),
        22
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "projection-label",
                "task",
                "abcdefghijklmnopqrstuv"
            ]
        )),
        "└ task · p:abcdefghijklmnopqrstuv".as_bytes()
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "concise-task-label", "broker/mx-task"]
        )),
        b"task"
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "home-identity", home.to_str().expect("home")],
        )),
        fs::canonicalize(&home)
            .expect("canonical home")
            .to_string_lossy()
            .as_bytes()
    );
    let journal_path = state.join("task.herdr-presentation");
    let token = String::from_utf8(success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "journal-create",
            state.to_str().expect("state"),
            "task",
        ],
    )))
    .expect("token");
    assert_eq!(token.len(), 22);
    assert!(
        String::from_utf8(success_output(run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "journal-snapshot",
                journal_path.to_str().expect("journal"),
                "task"
            ],
        )))
        .expect("snapshot")
        .starts_with("1\ttask\t")
    );
    let workspace_label = format!("└ task · p:{token}");
    success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "journal-bind",
            journal_path.to_str().expect("journal"),
            "task",
            home.to_str().expect("home"),
            "named",
            "child",
            "child:t1",
            "child:p1",
            "parent",
            "broker",
            &workspace_label,
            "mx-task",
        ],
    ));
    success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "journal-replace",
            journal_path.to_str().expect("journal"),
            "task",
            "child:t1",
            "child:p1",
            "child:t2",
            "child:p2",
        ],
    ));
    assert!(
        String::from_utf8(success_output(run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "journal-snapshot",
                journal_path.to_str().expect("journal"),
                "task"
            ],
        )))
        .expect("bound snapshot")
        .starts_with("2\ttask\t")
    );
    let journal_two = state.join("task2.herdr-presentation");
    success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "journal-create",
            state.to_str().expect("state"),
            "task2",
        ],
    ));
    success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "journal-write-v2",
            journal_two.to_str().expect("journal two"),
            "task2",
            "abcdefghijklmnopqrstuv",
            home.to_str().expect("home"),
            "named",
            "child",
            "child:t1",
            "child:p1",
            "parent",
            "broker",
            "└ task2 · p:abcdefghijklmnopqrstuv",
            "mx-task2",
        ],
    ));
    assert!(
        String::from_utf8(success_output(run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "journal-snapshot",
                journal_two.to_str().expect("journal two"),
                "task2"
            ],
        )))
        .expect("second snapshot")
        .starts_with("2\ttask2\tabcdefghijklmnopqrstuv\t")
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "focus-snapshot", "named"]
        )),
        b"parent\tparent:t1"
    );
    success_output(run(
        &fake,
        &log,
        &home,
        &["herdr", "focus-restore", "named", "parent\tparent:t1"],
    ));
    success_output(run(
        &fake,
        &log,
        &home,
        &["herdr", "close-pane-focus", "named", "child:p1", "no-agent"],
    ));
    success_output(run(
        &fake,
        &log,
        &home,
        &["herdr", "close-pane-focus", "named", "parent:p2", "live"],
    ));
    success_output(run(
        &fake,
        &log,
        &home,
        &["herdr", "close-pane-focus", "named", "unknown", "unknown"],
    ));
    assert_eq!(
        run(
            &fake,
            &log,
            &home,
            &["herdr", "close-pane-focus", "named", "child:p1", "invalid"],
        )
        .status
        .code(),
        Some(1)
    );
    assert!(
        String::from_utf8(success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "presentation-lock-path", "named"],
        )))
        .expect("lock")
        .contains("broker-herdr-presentation")
    );
    assert_eq!(
        PathBuf::from(
            String::from_utf8(success_output(run(
                &fake,
                &log,
                &home,
                &["herdr", "presentation-socket-path", "named"],
            )))
            .expect("socket")
        ),
        fs::canonicalize(socket.parent().expect("socket parent"))
            .expect("canonical parent")
            .join("named.sock")
    );
    assert_eq!(
        success_output(run(
            &fake,
            &log,
            &home,
            &["herdr", "parent-workspace", "named", "broker"]
        )),
        b"parent"
    );
    let endpoint = success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "projection-create",
            home.to_str().expect("home"),
            "projection",
            "mx-task",
        ],
    ));
    assert_eq!(
        endpoint,
        b"named\tnew\tnew:seed\tnew:seed-pane\tnew:t2\tnew:p2"
    );
    success_output(run(
        &fake,
        &log,
        &home,
        &["herdr", "projection-order", "named", "child", "broker"],
    ));
    assert_eq!(
        run(
            &fake,
            &log,
            &home,
            &["herdr", "projection-order", "named", "missing", "broker"],
        )
        .status
        .code(),
        Some(1)
    );
    success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "projection-live-binding",
            "named",
            "abcdefghijklmnopqrstuv",
            "child",
            "child:t1",
            "child:p1",
            "parent",
            "broker",
            "└ task · p:abcdefghijklmnopqrstuv",
            "mx-task",
        ],
    ));
    assert_eq!(
        run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "projection-live-binding",
                "named",
                "badbadbadbadbadbadbadb",
                "missing",
                "missing:t1",
                "missing:p1",
                "parent",
                "broker",
                "bad label",
                "mx-task",
            ],
        )
        .status
        .code(),
        Some(1)
    );
    assert_eq!(
        run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "projection-recovery-allows-flat",
                "named",
                temp.path()
                    .join("missing-journal")
                    .to_str()
                    .expect("missing journal"),
                "task",
            ],
        )
        .status
        .code(),
        Some(1)
    );
    success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "projection-recovery-allows-flat",
            "named",
            journal_path.to_str().expect("journal"),
            "task",
        ],
    ));
    assert_eq!(
        run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "projection-endpoint-matches",
                "named",
                "child",
                journal_path.to_str().expect("journal"),
                "task",
            ],
        )
        .status
        .code(),
        Some(1)
    );
    success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "projection-endpoint-matches",
            "named",
            "child",
            journal_two.to_str().expect("journal two"),
            "task2",
        ],
    ));
    assert_eq!(
        run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "projection-reclaim",
                "named",
                journal_path.to_str().expect("journal"),
                "task",
                home.to_str().expect("home"),
                "child",
                "child:t2",
                "child:p2",
                "broker",
                "mx-task",
                home.to_str().expect("home"),
            ],
        )
        .status
        .code(),
        Some(2)
    );
    assert_eq!(
        run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "wait-transition",
                "named",
                "0.01",
                transition_state.to_str().expect("transition state"),
                "named:parent:p2",
            ],
        )
        .status
        .code(),
        Some(2)
    );
    assert_eq!(
        run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "wait-transition",
                "named",
                "0.01",
                transition_state.to_str().expect("transition state"),
            ],
        )
        .status
        .code(),
        Some(2)
    );

    let move_socket = temp.path().join("move.sock");
    let listener = UnixListener::bind(&move_socket).expect("bind move socket");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept move request");
        let mut request = String::new();
        BufReader::new(stream.try_clone().expect("clone move stream"))
            .read_line(&mut request)
            .expect("read move request");
        assert!(request.contains("\"method\":\"workspace.move\""));
        stream
            .write_all(
                b"{\"id\":\"mx-workspace-move\",\"result\":{\"type\":\"workspace_list\",\"workspaces\":[]}}\n",
            )
            .expect("write move response");
    });
    assert!(
        run(
            &fake,
            &log,
            &home,
            &[
                "herdr",
                "workspace-move",
                move_socket.to_str().expect("move socket"),
                "child",
                "1",
            ],
        )
        .status
        .success()
    );
    server.join().expect("move server");

    let event_socket = temp.path().join("events.sock");
    let listener = UnixListener::bind(&event_socket).expect("bind event socket");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept event request");
        let mut request = String::new();
        BufReader::new(stream.try_clone().expect("clone event stream"))
            .read_line(&mut request)
            .expect("read event request");
        assert!(request.contains("\"method\":\"events.subscribe\""));
        thread::sleep(Duration::from_millis(30));
        stream
            .write_all(
                b"{\"id\":\"mx-eventwait\",\"result\":{\"type\":\"subscription_started\"}}\n",
            )
            .expect("write event acknowledgement");
        stream
            .write_all(b"{\"event\":\"pane.agent_status_changed\",\"data\":{\"pane_id\":\"parent:p2\",\"workspace_id\":\"parent\",\"agent_status\":\"blocked\",\"agent\":\"claude\"}}\n")
            .expect("write event");
        thread::sleep(Duration::from_millis(1_200));
    });
    let event = success_output(run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "event-reader",
            event_socket.to_str().expect("event socket"),
            "1",
            "parent:p2",
        ],
    ));
    let event = String::from_utf8(event).expect("event output");
    assert!(event.contains("@subscribed"));
    assert!(event.contains("parent:p2\tparent\tblocked\tclaude"));
    server.join().expect("event server");

    let missing_socket = temp.path().join("missing.sock");
    let event = run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "event-reader",
            missing_socket.to_str().expect("missing"),
            "0.01",
            "parent:p2",
        ],
    );
    assert_eq!(event.status.code(), Some(2));
    let mover = run(
        &fake,
        &log,
        &home,
        &[
            "herdr",
            "workspace-move",
            missing_socket.to_str().expect("missing"),
            "child",
            "1",
        ],
    );
    assert_eq!(mover.status.code(), Some(2));
    let unknown = run(&fake, &log, &home, &["herdr", "not-a-command"]);
    assert_eq!(unknown.status.code(), Some(1));
    for args in [
        vec!["herdr", "workspace-label", "extra"],
        vec!["herdr", "cli"],
        vec![
            "herdr",
            "send-submit",
            "named:parent:p2",
            "text",
            "1",
            "invalid",
            "0",
        ],
        vec!["herdr", "event-reader", "missing", "1"],
    ] {
        assert_eq!(run(&fake, &log, &home, &args).status.code(), Some(1));
    }
    assert_eq!(
        run(&fake, &log, &home, &["actor-state"]).status.code(),
        Some(2)
    );
    let absent_home = temp.path().join("absent-home");
    fs::create_dir(&absent_home).expect("absent home");
    fs::write(absent_home.join(".mx-daemon-home"), b"absent\n").expect("daemon marker");
    assert!(
        success_output(run(
            &fake,
            &log,
            &absent_home,
            &["herdr", "list-live", "named"],
        ))
        .is_empty()
    );
    let empty_home = temp.path().join("empty-home");
    fs::create_dir(&empty_home).expect("empty home");
    assert!(
        run(&fake, &log, &empty_home, &["herdr-session-cleanup"])
            .status
            .success()
    );
    assert_eq!(
        run(&fake, &log, &home, &["install-herdr"]).status.code(),
        Some(1)
    );
}

#[test]
fn hidden_herdr_tools_cover_guarded_lab_ci_cleanup_and_installer_refusal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fake = temp.path().join("herdr");
    let state = temp.path().join("lab-state");
    let deleted = temp.path().join("deleted");
    let ci_present = temp.path().join("ci-present");
    let log = temp.path().join("tool.log");
    let script = r#"#!/bin/sh
printf '%s|%s\n' "${HERDR_SESSION:-}" "$*" >> "$MX_HERDR_LOG"
case "$1 ${2:-}" in
  "session list")
    printf '{"sessions":[{"name":"default","default":true,"running":true,"socket_path":"/tmp/default.sock"}'
    if [ -n "${HERDR_SESSION:-}" ] && [ "${HERDR_SESSION#mx-lab-}" != "$HERDR_SESSION" ] && [ -e "$MX_HERDR_LAB_STATE_DIR/$HERDR_SESSION.system-state.json" ] && [ ! -e "$FAKE_DELETED" ]; then
      printf ',{"name":"%s","default":false,"running":false,"socket_path":"/tmp/%s.sock"}' "$HERDR_SESSION" "$HERDR_SESSION"
    fi
    if [ -e "$FAKE_CI_PRESENT" ]; then
      printf ',{"name":"mx-lab-ci","default":false,"running":true,"socket_path":"/tmp/ci.sock"}'
    fi
    printf ']}\n'
    ;;
  "status --json") printf '%s\n' '{"client":{"version":"0.7.4","protocol":16},"server":{"running":true}}' ;;
  "session delete")
    case "$3" in
      mx-lab-ci) rm -f "$FAKE_CI_PRESENT" ;;
      *) : > "$FAKE_DELETED" ;;
    esac
    printf '%s\n' '{}' ;;
  "session stop") printf '%s\n' '{}' ;;
  "--version ") printf '%s\n' 'herdr 0.7.4' ;;
esac
exit 0
"#;
    executable(&fake, script);
    let tool = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_mx"))
            .args(args)
            .env("MX_HERDR_BIN", &fake)
            .env("MX_HERDR_LOG", &log)
            .env("MX_HERDR_LAB_STATE_DIR", &state)
            .env("FAKE_DELETED", &deleted)
            .env("FAKE_CI_PRESENT", &ci_present)
            .output()
            .expect("tool")
    };

    assert!(tool(&["herdr-lab", "--help"]).status.success());
    let name = tool(&["herdr-lab", "name", "coverage label!"]);
    assert!(name.status.success());
    assert!(String::from_utf8_lossy(&name.stdout).starts_with("mx-lab-coveragelabel-"));
    assert_eq!(
        tool(&["herdr-lab", "prepare", "default"]).status.code(),
        Some(1)
    );
    assert_eq!(
        tool(&["herdr-lab", "run", "mx-lab-test"]).status.code(),
        Some(2)
    );
    assert_eq!(
        tool(&["herdr-lab", "run", "mx-lab-test", "--session", "bad"])
            .status
            .code(),
        Some(1)
    );
    assert!(
        tool(&["herdr-lab", "prepare", "mx-lab-test"])
            .status
            .success()
    );
    assert!(state.join("mx-lab-test.system-state.json").is_file());
    assert!(
        tool(&["herdr-lab", "provision", "mx-lab-test"])
            .status
            .success()
    );
    assert!(
        tool(&["herdr-lab", "run", "mx-lab-test", "status", "--json"])
            .status
            .success()
    );
    assert!(tool(&["herdr-lab", "stop", "mx-lab-test"]).status.success());
    assert!(
        tool(&["herdr-lab", "teardown", "mx-lab-test"])
            .status
            .success()
    );
    assert!(!state.join("mx-lab-test.system-state.json").exists());

    let snapshot = temp.path().join("sessions.json");
    assert!(
        tool(&[
            "herdr-ci-cleanup",
            "snapshot",
            snapshot.to_str().expect("snapshot")
        ])
        .status
        .success()
    );
    fs::write(&ci_present, b"present").expect("ci marker");
    assert!(
        tool(&[
            "herdr-ci-cleanup",
            "teardown",
            snapshot.to_str().expect("snapshot")
        ])
        .status
        .success()
    );
    assert!(!ci_present.exists());

    let fakebin = temp.path().join("fakebin");
    fs::create_dir(&fakebin).expect("fakebin");
    executable(
        &fakebin.join("curl"),
        "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = -o ]; then shift; printf bad > \"$1\"; exit 0; fi\n  shift\ndone\nexit 1\n",
    );
    let install = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args([
            "install-herdr",
            temp.path().join("install").to_str().expect("install"),
        ])
        .env("PATH", format!("{}:/usr/bin:/bin", fakebin.display()))
        .env("RUNNER_TEMP", temp.path())
        .output()
        .expect("installer");
    assert_eq!(install.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&install.stderr).contains("checksum mismatch"));
}

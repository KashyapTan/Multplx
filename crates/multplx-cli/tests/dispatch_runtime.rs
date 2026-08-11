use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write executable");
    fs::set_permissions(path, Permissions::from_mode(0o755)).expect("mode");
}

fn run(home: &Path, args: &[&str], environment: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    command.args(args).env("MX_HOME", home);
    for (name, value) in environment {
        command.env(name, value);
    }
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

fn cmux_fixture(temp: &tempfile::TempDir) -> (PathBuf, PathBuf, PathBuf) {
    let home = temp.path().join("home");
    let config = home.join("config");
    let state = temp.path().join("cmux.state");
    let fake = temp.path().join("cmux");
    fs::create_dir_all(&config).expect("config");
    fs::write(config.join("cmux-socket-password"), "secret\n").expect("password");
    executable(
        &fake,
        r#"#!/bin/sh
set -eu
command=${1:-}
case "$command" in
  version) printf '%s\n' "${MX_CMUX_VERSION:-cmux 0.64.17 (97) [fixture]}" ;;
  ping) printf '%s\n' "${MX_CMUX_PING:-PONG}" ;;
  workspace)
    if [ "${2:-}" = list ]; then
      if [ -n "${MX_CMUX_WORKSPACES:-}" ]; then
        printf '%s\n' "$MX_CMUX_WORKSPACES"
      elif [ -f "${MX_CMUX_STATE}.title" ]; then
        title=$(cat "${MX_CMUX_STATE}.title")
        printf '{"workspaces":[{"id":"w1","title":"%s"}]}\n' "$title"
      elif [ -n "${MX_CMUX_TITLE:-}" ]; then
        printf '{"workspaces":[{"id":"w1","title":"%s"}]}\n' "$MX_CMUX_TITLE"
      else
        printf '%s\n' '{"workspaces":[]}'
      fi
    fi ;;
  list-panes)
    if [ -n "${MX_CMUX_PANES:-}" ]; then
      printf '%s\n' "$MX_CMUX_PANES"
    else
      printf '%s\n' '{"panes":[{"selected_surface_id":"s1","surface_ids":["s1"]}]}'
    fi ;;
  new-workspace)
    if [ "${MX_CMUX_CREATE_FAIL:-0}" = 1 ]; then exit 7; fi
    previous=
    for argument in "$@"; do
      if [ "$previous" = --name ] && [ "${MX_CMUX_CREATE_NO_STATE:-0}" != 1 ]; then printf '%s' "$argument" > "${MX_CMUX_STATE}.title"; fi
      previous=$argument
    done ;;
  read-screen)
    case "${MX_CMUX_READ:-capture}" in
      cwd) printf '%s\n' '{"text":"__MX_CMUX_CWD_BEGIN__\n/tmp/work\n__MX_CMUX_CWD_END__"}' ;;
      composer) printf '%s\n' '{"text":"header\n│ typed text │"}' ;;
      empty) printf '%s\n' '{"text":"│ ❯ │"}' ;;
      *) printf '%s\n' '{"text":"one\ntwo\nthree"}' ;;
    esac ;;
  list-windows)
    if [ -n "${MX_CMUX_WINDOWS:-}" ]; then printf '%s\n' "$MX_CMUX_WINDOWS"; else printf '%s\n' '[{"id":"win1"}]'; fi ;;
  send|send-key|close-workspace) ;;
  *) exit 17 ;;
esac
"#,
    );
    (home, fake, state)
}

#[test]
fn headroom_discovers_linux_signals_and_preserves_failed_queue_records() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let state = home.join("state");
    let config = home.join("config");
    let proc_root = temp.path().join("proc");
    fs::create_dir_all(&state).expect("state");
    fs::create_dir_all(&config).expect("config");
    fs::create_dir_all(&proc_root).expect("proc");
    fs::write(
        proc_root.join("cpuinfo"),
        "processor : 0\nprocessor : 1\nprocessor : 2\nprocessor : 3\n",
    )
    .expect("cpuinfo");
    fs::write(proc_root.join("loadavg"), "0.50 0.25 0.10 1/100 1\n").expect("loadavg");
    fs::write(
        proc_root.join("meminfo"),
        "MemTotal: 1 kB\nMemAvailable: 1048576 kB\n",
    )
    .expect("meminfo");
    fs::write(
        config.join("actor-dispatch.json"),
        r#"{"rules":[{"use":[{"harness":"codex"},{"harness":"pi"}]}],"default":{"harness":"claude"}}"#,
    )
    .expect("dispatch");
    fs::write(config.join("api-capacity"), "9\n").expect("capacity");
    fs::write(config.join("api-capacity-codex"), "2\n").expect("candidate capacity");
    let linux = Path::new("Linux");
    let in_use = Path::new("1");
    let environment = [
        ("MX_STATE_OVERRIDE", state.as_path()),
        ("MX_CONFIG_OVERRIDE", config.as_path()),
        ("MX_HEADROOM_PROC_ROOT", proc_root.as_path()),
        ("MX_HEADROOM_PLATFORM", linux),
        ("MX_HEADROOM_IN_USE", in_use),
    ];
    let output = run(&home, &["headroom", "--json"], &environment);
    assert_success(&output);
    let json = String::from_utf8_lossy(&output.stdout);
    assert!(json.contains("\"cpu_count\":4.0"));
    assert!(json.contains("\"codex\""));

    for (name, value) in [
        ("MX_HEADROOM_CPU_COUNT", "bad"),
        ("MX_HEADROOM_LOAD1", "bad"),
        ("MX_HEADROOM_MEM_AVAILABLE_BYTES", "bad"),
        ("MX_HEADROOM_CPU_PER_ACTOR", "0"),
        ("MX_HEADROOM_MEM_PER_ACTOR_BYTES", "0"),
    ] {
        let mut invalid = environment.to_vec();
        invalid.push((name, Path::new(value)));
        assert!(
            !run(&home, &["headroom", "--json"], &invalid)
                .status
                .success()
        );
    }

    assert_success(&run(
        &home,
        &[
            "headroom",
            "--queue-add",
            "retained",
            "/project",
            "--harness",
            "codex",
        ],
        &environment,
    ));
    let missing_spawn = temp.path().join("missing-spawn");
    let mut drain = environment.to_vec();
    drain.push(("MX_HEADROOM_SPAWN_BIN", missing_spawn.as_path()));
    assert!(
        !run(&home, &["headroom", "--queue-drain"], &drain)
            .status
            .success()
    );
    assert!(
        String::from_utf8_lossy(&run(&home, &["headroom", "--queue"], &environment).stdout)
            .contains("retained")
    );

    let failing_spawn = temp.path().join("failing-spawn");
    executable(&failing_spawn, "#!/bin/sh\nexit 7\n");
    drain.pop();
    drain.push(("MX_HEADROOM_SPAWN_BIN", failing_spawn.as_path()));
    assert!(
        !run(&home, &["headroom", "--queue-drain"], &drain)
            .status
            .success()
    );

    fs::write(config.join("api-capacity"), "0\n").expect("at-limit capacity");
    let at_limit = run(&home, &["headroom", "--queue-drain"], &environment);
    assert_success(&at_limit);
    assert!(at_limit.stdout.is_empty());

    fs::remove_file(state.join(".dispatch-queue/retained.request")).expect("remove record");
    assert_success(&run(&home, &["headroom", "--queue-drain"], &environment));

    fs::write(config.join("api-capacity"), "9\n").expect("capacity");
    assert_success(&run(
        &home,
        &["headroom", "--queue-add", "default-spawn", "/project"],
        &environment,
    ));
    let root = temp.path().join("spawn-root");
    fs::create_dir_all(root.join("bin")).expect("bin");
    executable(&root.join("bin/mx-spawn.sh"), "#!/bin/sh\nexit 0\n");
    let mut default_spawn = environment.to_vec();
    default_spawn.push(("MX_ROOT_OVERRIDE", root.as_path()));
    let drained = run(&home, &["headroom", "--queue-drain"], &default_spawn);
    assert_success(&drained);
    assert!(String::from_utf8_lossy(&drained.stdout).contains("default-spawn"));
}

#[test]
fn headroom_discovers_darwin_signals_and_counts_live_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let state = home.join("state");
    let config = home.join("config");
    let proc_root = temp.path().join("proc");
    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir_all(&state).expect("state");
    fs::create_dir_all(&config).expect("config");
    fs::create_dir_all(&proc_root).expect("proc");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    executable(&fake_bin.join("uname"), "#!/bin/sh\nprintf 'Darwin\\n'\n");
    executable(
        &fake_bin.join("sysctl"),
        "#!/bin/sh\ncase \"$*\" in *hw.logicalcpu*) printf '8\\n' ;; *) printf '{ 1.25 0.5 0.25 }\\n' ;; esac\n",
    );
    executable(
        &fake_bin.join("vm_stat"),
        "#!/bin/sh\nprintf 'Mach Virtual Memory Statistics: (page size of 4096 bytes)\\nPages free: 100.\\nPages inactive: 200.\\nPages speculative: 50.\\nPages purgeable: 25.\\n'\n",
    );
    let tmux = fake_bin.join("tmux");
    executable(&tmux, "#!/bin/sh\nprintf '%%1\\n'\n");
    fs::write(
        state.join("actor.meta"),
        "kind=actor\nwindow=target\nbackend=tmux\nharness=codex\n",
    )
    .expect("actor metadata");
    fs::write(
        state.join("daemon.meta"),
        "kind=daemon\nwindow=ignored\nbackend=tmux\nharness=pi\n",
    )
    .expect("daemon metadata");
    fs::write(state.join("ignored.txt"), "window=target\n").expect("ignored");
    fs::write(config.join("actor-harness"), "codex\n").expect("harness");
    let environment = [
        ("MX_STATE_OVERRIDE", state.as_path()),
        ("MX_CONFIG_OVERRIDE", config.as_path()),
        ("MX_HEADROOM_PROC_ROOT", proc_root.as_path()),
        ("MX_TMUX_BIN", tmux.as_path()),
        ("PATH", fake_bin.as_path()),
    ];
    let output = run(&home, &["headroom", "--json"], &environment);
    assert_success(&output);
    let json = String::from_utf8_lossy(&output.stdout);
    assert!(json.contains("\"cpu_count\":8.0"));
    assert!(json.contains("\"in_use\":1"));
}

#[test]
fn cmux_malformed_and_socket_states_fail_closed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (home, fake, state) = cmux_fixture(&temp);
    let base = [
        ("MX_BACKEND_CMUX_BIN", fake.as_path()),
        ("MX_CMUX_STATE", state.as_path()),
    ];
    for value in [
        "only processes started inside cmux can connect",
        "Authentication required",
        "Socket not found",
        "unexpected",
    ] {
        let mut environment = base.to_vec();
        environment.push(("MX_CMUX_PING", Path::new(value)));
        let ping = run(&home, &["cmux", "ping-state"], &environment);
        assert_success(&ping);
        if value.contains("inside") || value.contains("Authentication") {
            assert!(
                !run(&home, &["cmux", "ensure-running"], &environment)
                    .status
                    .success()
            );
        }
    }
    for version in ["cmux", "cmux invalid", "cmux 0.63.0"] {
        let mut environment = base.to_vec();
        environment.push(("MX_CMUX_VERSION", Path::new(version)));
        assert!(
            !run(&home, &["cmux", "version-check"], &environment)
                .status
                .success()
        );
    }
    let mut malformed = base.to_vec();
    malformed.push(("MX_CMUX_PANES", Path::new("not-json")));
    for args in [
        &["cmux", "surface-id-for-workspace", "w1"][..],
        &["cmux", "surface-exists", "w1", "s1"],
        &["cmux", "target-ready", "w1:s1"],
    ] {
        assert!(!run(&home, args, &malformed).status.success());
    }
    let missing = run(&home, &["cmux", "current-path", "w1:s1"], &base);
    assert!(!missing.status.success());
    let unknown = run(&home, &["cmux", "composer-state", "w1:s1"], &base);
    assert_success(&unknown);
    assert_eq!(unknown.stdout, b"unknown");

    let scoped = run(&home, &["cmux", "scoped-title", "mx-live"], &base);
    assert_success(&scoped);
    fs::write(
        format!("{}.title", state.display()),
        String::from_utf8(scoped.stdout).expect("title"),
    )
    .expect("state title");
    assert_success(&run(
        &home,
        &["cmux", "target-ready", "w1:stale", "mx-live"],
        &base,
    ));
    assert!(
        !run(&home, &["cmux", "target-ready", "w1:s1", "mx-other"], &base,)
            .status
            .success()
    );
    assert!(
        !run(&home, &["cmux", "create-task", "mx-live", "/tmp"], &base,)
            .status
            .success()
    );

    for args in [
        &["cmux", "bin", "extra"][..],
        &["cmux", "password", "extra"],
        &["cmux", "cli"],
        &["cmux", "scoped-title"],
        &["cmux", "surface-exists", "w1"],
        &["cmux", "send-literal", "w1:s1"],
        &["cmux", "send-key", "w1:s1"],
        &["cmux", "send-text-line", "w1:s1"],
        &["cmux", "composer-state"],
        &["cmux", "window-of-workspace"],
    ] {
        assert!(!run(&home, args, &base).status.success());
    }

    let mut pending = base.to_vec();
    pending.push(("MX_CMUX_READ", Path::new("composer")));
    let pending_submit = run(
        &home,
        &["cmux", "send-submit", "w1:s1", "message", "2", "0", "0"],
        &pending,
    );
    assert_success(&pending_submit);
    assert_eq!(pending_submit.stdout, b"pending");

    for (name, value) in [
        ("MX_CMUX_CREATE_FAIL", "1"),
        ("MX_CMUX_CREATE_NO_STATE", "1"),
    ] {
        let _ = fs::remove_file(format!("{}.title", state.display()));
        let mut environment = base.to_vec();
        environment.push((name, Path::new(value)));
        assert!(
            !run(
                &home,
                &["cmux", "create-task", "mx-failure", "/tmp"],
                &environment,
            )
            .status
            .success()
        );
    }

    let mut bad_windows = base.to_vec();
    bad_windows.push(("MX_CMUX_WINDOWS", Path::new("not-json")));
    let window = run(&home, &["cmux", "window-of-workspace", "w1"], &bad_windows);
    assert_success(&window);
    assert!(window.stdout.is_empty());

    let mut mixed_inventory = base.to_vec();
    mixed_inventory.push((
        "MX_CMUX_WORKSPACES",
        Path::new(r#"{"workspaces":[{}, {"id":"missing-title"}, {"title":"missing-id"}]}"#),
    ));
    let inventory = run(&home, &["cmux", "list-live"], &mixed_inventory);
    assert_success(&inventory);
    assert!(inventory.stdout.is_empty());
}

#[test]
fn launcher_validation_and_treehouse_download_refusals_are_observable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    fs::create_dir_all(&home).expect("home");
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &[]).status.code(),
        Some(2)
    );

    let missing_root = temp.path().join("missing-root");
    assert_eq!(
        run(
            &home,
            &["launch-harness", "codex"],
            &[("MX_ROOT_OVERRIDE", missing_root.as_path())],
        )
        .status
        .code(),
        Some(2)
    );
    let relative = Path::new("relative");
    let validated = Path::new("1");
    assert_eq!(
        run(
            &home,
            &["launch-harness", "codex"],
            &[
                ("MX_ROOT_OVERRIDE", relative),
                ("MX_LAUNCH_VALIDATED", validated),
            ],
        )
        .status
        .code(),
        Some(2)
    );

    let root = temp.path().join("root");
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir_all(root.join(".agents/skills")).expect("skills");
    fs::create_dir_all(root.join("share/shell/shims")).expect("shims");
    fs::write(root.join("AGENTS.md"), "fixture\n").expect("agents");
    executable(&root.join("bin/mx-launcher.sh"), "#!/bin/sh\nexit 0\n");
    executable(&root.join("bin/mx-lock.sh"), "#!/bin/sh\nexit 1\n");
    assert!(
        Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    for part in ["config", "data", "projects", "state"] {
        fs::create_dir_all(home.join(part)).expect("home part");
    }
    let real = temp.path().join("real-codex");
    executable(&real, "#!/bin/sh\nexit 0\n");
    let environment = [
        ("MX_ROOT_OVERRIDE", root.as_path()),
        ("MX_REAL_CODEX", real.as_path()),
    ];
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &environment)
            .status
            .code(),
        Some(2)
    );
    executable(
        &root.join("bin/mx-lock.sh"),
        "#!/bin/sh\nprintf 'lock: held by live harness pid 42\\n'\n",
    );
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &environment)
            .status
            .code(),
        Some(3)
    );

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("fake bin");
    executable(&fake_bin.join("curl"), "#!/bin/sh\nexit 22\n");
    let destination = temp.path().join("treehouse");
    let install = run(
        &home,
        &[
            "install-treehouse",
            destination.to_str().expect("destination"),
        ],
        &[("PATH", fake_bin.as_path())],
    );
    assert!(!install.status.success());
    assert!(String::from_utf8_lossy(&install.stderr).contains("download failed"));
}

#[test]
fn launcher_rejects_incomplete_roots_homes_reals_and_recursive_shims() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    for part in ["config", "data", "projects", "state"] {
        fs::create_dir_all(home.join(part)).expect("home part");
    }
    let root = temp.path().join("root");
    fs::create_dir_all(&root).expect("root");
    assert!(
        Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    let root_env = [("MX_ROOT_OVERRIDE", root.as_path())];
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &root_env)
            .status
            .code(),
        Some(2)
    );

    fs::write(root.join("AGENTS.md"), "fixture\n").expect("agents");
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &root_env)
            .status
            .code(),
        Some(2)
    );
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir_all(root.join(".agents/skills")).expect("skills");
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &root_env)
            .status
            .code(),
        Some(2)
    );
    executable(&root.join("bin/mx-launcher.sh"), "#!/bin/sh\nexit 0\n");
    executable(
        &root.join("bin/mx-lock.sh"),
        "#!/bin/sh\nprintf 'lock: free\\n'\n",
    );

    fs::remove_dir_all(home.join("projects")).expect("remove projects");
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &root_env)
            .status
            .code(),
        Some(2)
    );
    fs::create_dir(home.join("projects")).expect("projects");

    let relative = Path::new("relative-real");
    let mut real_env = root_env.to_vec();
    real_env.push(("MX_REAL_CODEX", relative));
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &real_env)
            .status
            .code(),
        Some(127)
    );

    fs::create_dir_all(root.join("share/shell/shims")).expect("shims");
    let shim = root.join("share/shell/shims/codex");
    executable(&shim, "#!/bin/sh\nexit 0\n");
    real_env.pop();
    real_env.push(("MX_REAL_CODEX", shim.as_path()));
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &real_env)
            .status
            .code(),
        Some(127)
    );

    let invalid_git = temp.path().join("invalid-git");
    fs::create_dir_all(invalid_git.join("bin")).expect("bin");
    fs::create_dir_all(invalid_git.join(".agents/skills")).expect("skills");
    fs::create_dir(invalid_git.join(".git")).expect("git directory");
    fs::write(invalid_git.join("AGENTS.md"), "fixture\n").expect("agents");
    executable(
        &invalid_git.join("bin/mx-launcher.sh"),
        "#!/bin/sh\nexit 0\n",
    );
    assert_eq!(
        run(
            &home,
            &["launch-harness", "codex"],
            &[("MX_ROOT_OVERRIDE", invalid_git.as_path())],
        )
        .status
        .code(),
        Some(2)
    );

    let validated = Path::new("1");
    assert_eq!(
        run(
            &home,
            &["launch-harness", "codex"],
            &[
                ("MX_ROOT_OVERRIDE", invalid_git.as_path()),
                ("MX_LAUNCH_VALIDATED", validated),
            ],
        )
        .status
        .code(),
        Some(2)
    );

    let bad_real = temp.path().join("bad-real");
    executable(&bad_real, "#!/definitely/missing/interpreter\n");
    let bad_env = [
        ("MX_ROOT_OVERRIDE", root.as_path()),
        ("MX_REAL_CODEX", bad_real.as_path()),
    ];
    assert_eq!(
        run(&home, &["launch-harness", "codex"], &bad_env)
            .status
            .code(),
        Some(127)
    );

    let cursor = temp.path().join("real-cursor");
    executable(&cursor, "#!/bin/sh\nprintf '%s\\n' \"$*\"\n");
    let cursor_env = [
        ("MX_ROOT_OVERRIDE", root.as_path()),
        ("MX_REAL_CURSOR_AGENT", cursor.as_path()),
    ];
    let cursor_output = run(&home, &["launch-harness", "cursor", "safe"], &cursor_env);
    assert_success(&cursor_output);
    assert_eq!(cursor_output.stdout, b"--sandbox enabled safe\n");

    assert_eq!(
        run(Path::new("/"), &["launch-harness", "codex"], &root_env)
            .status
            .code(),
        Some(2)
    );

    let linked_root = temp.path().join("linked-root");
    fs::create_dir_all(linked_root.join("bin")).expect("bin");
    fs::create_dir_all(linked_root.join(".agents/skills")).expect("skills");
    fs::write(linked_root.join("AGENTS.md"), "fixture\n").expect("agents");
    executable(
        &linked_root.join("bin/mx-launcher.sh"),
        "#!/bin/sh\nexit 0\n",
    );
    std::os::unix::fs::symlink(root.join(".git"), linked_root.join(".git")).expect("git link");
    assert_eq!(
        run(
            &home,
            &["launch-harness", "codex"],
            &[("MX_ROOT_OVERRIDE", linked_root.as_path())],
        )
        .status
        .code(),
        Some(2)
    );
}

#[test]
fn cmux_tool_resolution_and_treehouse_archive_bounds_are_enforced() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    fs::create_dir_all(home.join("config")).expect("config");
    let empty_path = temp.path().join("empty-path");
    fs::create_dir(&empty_path).expect("empty path");
    let missing_bundle = temp.path().join("missing-cmux");
    let unavailable = [
        ("PATH", empty_path.as_path()),
        ("MX_BACKEND_CMUX_BUNDLE_BIN", missing_bundle.as_path()),
    ];
    assert!(
        !run(&home, &["cmux", "tool-check"], &unavailable)
            .status
            .success()
    );
    assert_eq!(
        run(&home, &["cmux", "bin"], &unavailable).status.code(),
        Some(1)
    );
    assert_success(&run(&home, &["cmux", "ping-state"], &unavailable));

    let tools = temp.path().join("tools");
    fs::create_dir(&tools).expect("tools");
    let bundle = tools.join("cmux-bundle");
    executable(&bundle, "#!/bin/sh\nexit 0\n");
    let bundle_only = [
        ("PATH", empty_path.as_path()),
        ("MX_BACKEND_CMUX_BUNDLE_BIN", bundle.as_path()),
    ];
    assert!(
        !run(&home, &["cmux", "tool-check"], &bundle_only)
            .status
            .success()
    );
    executable(&tools.join("jq"), "#!/bin/sh\nexit 0\n");
    let available = [
        ("PATH", tools.as_path()),
        ("MX_BACKEND_CMUX_BUNDLE_BIN", bundle.as_path()),
    ];
    assert_success(&run(&home, &["cmux", "tool-check"], &available));

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("fake bin");
    let destination = temp.path().join("treehouse");
    executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nout=\nwhile [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then shift; out=$1; fi; shift; done\nprintf abc > \"$out\"\n",
    );
    let checksum = run(
        &home,
        &[
            "install-treehouse",
            destination.to_str().expect("destination"),
        ],
        &[("PATH", fake_bin.as_path())],
    );
    assert!(!checksum.status.success());
    assert!(String::from_utf8_lossy(&checksum.stderr).contains("checksum mismatch"));

    executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nout=\nwhile [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then shift; out=$1; fi; shift; done\n/bin/dd if=/dev/zero of=\"$out\" bs=1000000 count=16 2>/dev/null\n",
    );
    let oversized = run(
        &home,
        &[
            "install-treehouse",
            destination.to_str().expect("destination"),
        ],
        &[("PATH", fake_bin.as_path())],
    );
    assert!(!oversized.status.success());
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("size limit"));
}

#[test]
fn hidden_cmux_cli_covers_the_runtime_facade_and_refusals() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (home, fake, state) = cmux_fixture(&temp);
    let fake_env = [
        ("MX_BACKEND_CMUX_BIN", fake.as_path()),
        ("MX_CMUX_STATE", state.as_path()),
    ];

    for args in [
        &["cmux", "bin"][..],
        &["cmux", "password"],
        &["cmux", "cli", "ping"],
        &["cmux", "tool-check"],
        &["cmux", "version-check"],
        &["cmux", "ping-state"],
        &["cmux", "ensure-running"],
        &["cmux", "container-ensure"],
        &["cmux", "home-label"],
        &["cmux", "scoped-title", "mx-task"],
        &["cmux", "workspace-id-for-label", "absent"],
        &["cmux", "surface-id-for-workspace", "w1"],
        &["cmux", "parse-target", "w1:s1"],
        &["cmux", "surface-exists", "w1", "s1"],
        &["cmux", "target-ready", "w1:s1"],
        &["cmux", "send-literal", "w1:s1", "literal"],
        &["cmux", "normalize-key", "Escape"],
        &["cmux", "send-key", "w1:s1", "C-c"],
        &["cmux", "send-text-line", "w1:s1", "line"],
        &["cmux", "capture", "w1:s1", "2"],
        &["cmux", "window-of-workspace", "w1"],
        &["cmux", "kill", "w1:s1", "", "mx-task"],
    ] {
        let output = run(&home, args, &fake_env);
        assert_success(&output);
    }

    let create = run(
        &home,
        &["cmux", "create-task", "mx-task", "/tmp"],
        &fake_env,
    );
    assert_success(&create);
    assert_eq!(create.stdout, b"w1 s1");

    let mut environment = fake_env.to_vec();
    environment.push(("MX_CMUX_READ", Path::new("cwd")));
    let path = run(&home, &["cmux", "current-path", "w1:s1"], &environment);
    assert_success(&path);
    assert_eq!(path.stdout, b"/tmp/work");

    environment.pop();
    environment.push(("MX_CMUX_READ", Path::new("composer")));
    let composer = run(&home, &["cmux", "composer-state", "w1:s1"], &environment);
    assert_success(&composer);
    assert_eq!(composer.stdout, b"pending");

    environment.pop();
    environment.push(("MX_CMUX_READ", Path::new("empty")));
    let submit = run(
        &home,
        &["cmux", "send-submit", "w1:s1", "message", "1", "0", "0"],
        &environment,
    );
    assert_success(&submit);
    assert_eq!(submit.stdout, b"empty");

    let scoped = run(&home, &["cmux", "scoped-title", "mx-live"], &fake_env);
    assert_success(&scoped);
    let title = String::from_utf8(scoped.stdout).expect("title");
    let title_path = temp.path().join("title");
    fs::write(&title_path, &title).expect("title file");
    let list_environment = [
        ("MX_BACKEND_CMUX_BIN", fake.as_path()),
        ("MX_CMUX_STATE", state.as_path()),
        ("MX_CMUX_TITLE", title_path.as_path()),
    ];
    fs::write(format!("{}.title", state.display()), title).expect("state title");
    let listed = run(&home, &["cmux", "list-live"], &list_environment);
    assert_success(&listed);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("w1:s1\tmx-live"));

    for args in [
        &["cmux", "unknown"][..],
        &["cmux", "parse-target", "bad"],
        &["cmux", "send-submit", "w1:s1", "text", "bad", "0", "0"],
        &["cmux", "capture"],
    ] {
        assert!(!run(&home, args, &fake_env).status.success());
    }
}

#[test]
fn harness_headroom_queue_and_launcher_commands_cover_public_outcomes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let state = home.join("state");
    let config = home.join("config");
    let proc_root = temp.path().join("proc");
    fs::create_dir_all(&state).expect("state");
    fs::create_dir_all(&config).expect("config");
    fs::create_dir_all(&proc_root).expect("proc");
    fs::write(config.join("actor-harness"), "codex\n").expect("actor");
    fs::write(config.join("daemon-harness"), "pi model high\n").expect("daemon");

    for (args, expected) in [
        (vec!["harness", "actor"], "codex\n"),
        (vec!["harness", "daemon"], "pi\n"),
        (vec!["harness", "daemon-model"], "model\n"),
        (vec!["harness", "daemon-effort"], "high\n"),
    ] {
        let output = run(&home, &args, &[]);
        assert_success(&output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }

    let platform = Path::new("Linux");
    let cpu = Path::new("8");
    let load = Path::new("1");
    let memory = Path::new("1073741824");
    let in_use = Path::new("2");
    let ignore = Path::new("1");
    let headroom_env = [
        ("MX_STATE_OVERRIDE", state.as_path()),
        ("MX_CONFIG_OVERRIDE", config.as_path()),
        ("MX_HEADROOM_PROC_ROOT", proc_root.as_path()),
        ("MX_HEADROOM_PLATFORM", platform),
        ("MX_HEADROOM_CPU_COUNT", cpu),
        ("MX_HEADROOM_LOAD1", load),
        ("MX_HEADROOM_MEM_AVAILABLE_BYTES", memory),
        ("MX_HEADROOM_IN_USE", in_use),
        ("MX_HEADROOM_IGNORE_DISPATCH_CONFIG", ignore),
    ];
    let evaluated = run(&home, &["headroom", "--json"], &headroom_env);
    assert_success(&evaluated);
    assert!(String::from_utf8_lossy(&evaluated.stdout).contains("\"model\":\"local+api\""));

    let added = run(
        &home,
        &[
            "headroom",
            "--queue-add",
            "task",
            "/project",
            "--harness",
            "codex",
            "--model",
            "gpt",
            "--effort",
            "high",
            "--backend",
            "tmux",
            "--scout",
        ],
        &headroom_env,
    );
    assert_success(&added);
    assert_success(&run(&home, &["headroom", "--queue"], &headroom_env));

    let spawn = temp.path().join("spawn");
    executable(&spawn, "#!/bin/sh\nexit 0\n");
    let mut drain_env = headroom_env.to_vec();
    drain_env.push(("MX_HEADROOM_SPAWN_BIN", spawn.as_path()));
    let drained = run(&home, &["headroom", "--queue-drain"], &drain_env);
    assert_success(&drained);
    assert!(String::from_utf8_lossy(&drained.stdout).contains("launched task"));

    assert_success(&run(
        &home,
        &["headroom", "--queue-add", "cancel", "/project"],
        &headroom_env,
    ));
    assert_success(&run(
        &home,
        &["headroom", "--queue-cancel", "cancel"],
        &headroom_env,
    ));
    for args in [
        &["headroom", "--queue-add"][..],
        &["headroom", "--queue-cancel"],
        &["headroom", "--queue", "extra"],
        &["headroom", "unknown"],
        &["headroom", "--json", "extra"],
        &["headroom", "--queue-drain", "extra"],
        &["headroom", "--queue-add", "bad", "/p", "--model"],
        &["headroom", "--queue-add", "bad", "/p", "--unknown"],
        &["install-treehouse"],
    ] {
        assert!(!run(&home, args, &headroom_env).status.success());
    }

    let root = temp.path().join("root");
    let launch_home = temp.path().join("launch-home");
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir_all(root.join(".agents/skills")).expect("skills");
    fs::create_dir_all(root.join("share/shell/shims")).expect("shims");
    for part in ["config", "data", "projects", "state"] {
        fs::create_dir_all(launch_home.join(part)).expect("home part");
    }
    fs::write(root.join("AGENTS.md"), "# fixture\n").expect("agents");
    executable(&root.join("bin/mx-launcher.sh"), "#!/bin/sh\nexit 0\n");
    executable(
        &root.join("bin/mx-lock.sh"),
        "#!/bin/sh\nprintf 'lock: free\\n'\n",
    );
    assert!(
        Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&root)
            .status()
            .expect("git")
            .success()
    );
    let harness = temp.path().join("codex");
    executable(&harness, "#!/bin/sh\nprintf 'launched\\n'\n");
    let launch_env = [
        ("MX_ROOT_OVERRIDE", root.as_path()),
        ("MX_REAL_CODEX", harness.as_path()),
    ];
    let launched = run(&launch_home, &["launch-harness", "codex"], &launch_env);
    assert_success(&launched);
    assert_eq!(launched.stdout, b"launched\n");

    assert!(
        !run(&launch_home, &["launch-harness", "unknown"], &launch_env)
            .status
            .success()
    );
    let missing = run(&launch_home, &["launch-harness", "pi"], &launch_env);
    assert_eq!(missing.status.code(), Some(127));

    let cursor = temp.path().join("cursor-agent");
    executable(&cursor, "#!/bin/sh\nexit 0\n");
    let cursor_env = [
        ("MX_ROOT_OVERRIDE", root.as_path()),
        ("MX_REAL_CURSOR_AGENT", cursor.as_path()),
    ];
    assert_eq!(
        run(
            &launch_home,
            &["launch-harness", "cursor", "--yolo"],
            &cursor_env,
        )
        .status
        .code(),
        Some(2)
    );
    assert_eq!(
        run(&launch_home, &["launch-harness"], &launch_env)
            .status
            .code(),
        Some(2)
    );
}

//! Real Git and CLI lifecycle contracts with a deterministic cmux mock and
//! failing sentinels for unrelated endpoint/provider tools.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

use multplx_domain::lifecycle::subagent_model::{TaskRecord, read_meta};

fn run(command: &mut Command) -> Output {
    command.output().expect("run command")
}

fn success(command: &mut Command) -> Output {
    let output = command.output().expect("run command");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    fake: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        let project = root.join("project");
        let fake = root.join("fake");
        for path in [
            home.join("state"),
            home.join("config"),
            home.join("data/task"),
            home.join("tmp"),
            project.clone(),
            fake.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            home.join("data/task/brief.md"),
            "Inspect this fixture repository.\n",
        )
        .unwrap();
        fs::write(home.join("config/actor-harness"), "codex\n").unwrap();
        fs::write(
            fake.join("inert-terminal-start.sh"),
            include_str!("../../../tests/inert-terminal-start.sh"),
        )
        .unwrap();
        for tool in ["tmux", "herdr", "treehouse", "curl"] {
            let path = fake.join(tool);
            fs::write(
                &path,
                "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$MX_SENTINEL_LOG\"\nexit 99\n",
            )
            .unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let fixture = Self {
            _temp: temp,
            home,
            project,
            fake,
        };
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ]);
        fixture
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.project)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn spawn(&self, options: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("MX_") {
                command.env_remove(key);
            }
        }
        command
            .current_dir(&self.home)
            .env("MX_HOME", &self.home)
            .env("MX_ROOT_OVERRIDE", &self.home)
            .env(
                "MX_RUST_SOURCE_ROOT",
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
            )
            .env("MX_HEADROOM_SKIP_QUEUE", "1")
            .env("MX_HEADROOM_CPU_COUNT", "8")
            .env("MX_HEADROOM_LOAD1", "0")
            .env("MX_HEADROOM_MEM_AVAILABLE_BYTES", "17179869184")
            .env("MX_HEADROOM_API_CAPACITY", "8")
            .env("MX_HEADROOM_IN_USE", "0")
            .env("TMPDIR", self.home.join("tmp"))
            .env("MX_SENTINEL_LOG", self.home.join("sentinel.log"))
            .env(
                "PATH",
                format!("{}:{}", self.fake.display(), std::env::var("PATH").unwrap()),
            )
            .args(["spawn", "task"])
            .arg(&self.project)
            .args(["--backend", "tmux"])
            .args(options);
        command
    }

    fn refused(&self, output: Output, message: &str) {
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !self.home.join("sentinel.log").exists(),
            "preflight invoked endpoint/provider"
        );
        assert!(!self.home.join("state/task.meta").exists());
        assert_eq!(
            self.git(&["worktree", "list", "--porcelain"])
                .matches("worktree ")
                .count(),
            1
        );
    }
}

fn install_fake_cmux(f: &Fixture) -> PathBuf {
    let cmux = f.fake.join("cmux");
    fs::write(
        &cmux,
        r#"#!/bin/sh
set -eu
case "$1" in
  version) echo 'cmux 0.64.17 (97) [abcdef1]' ;;
  ping) echo PONG ;;
  workspace)
    if [ -f "$MX_CMUX_FIXTURE/titles" ]; then
      title=$(tail -n 1 "$MX_CMUX_FIXTURE/titles")
      printf '{"workspaces":[{"id":"11111111-1111-4111-8111-111111111111","title":"%s"}]}' "$title"
    else echo '{"workspaces":[]}'; fi ;;
  new-workspace)
    shift
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --name) shift; printf '%s\n' "$1" >> "$MX_CMUX_FIXTURE/titles" ;;
        --cwd) shift; printf '%s\n' "$1" >> "$MX_CMUX_FIXTURE/cwds" ;;
      esac
      shift
    done ;;
  list-panes) echo '{"panes":[{"selected_surface_id":"22222222-2222-4222-8222-222222222222","surface_ids":["22222222-2222-4222-8222-222222222222"]}]}' ;;
  send)
    printf '%s\n' "$*" >> "$MX_CMUX_FIXTURE/sent"
    for argument do literal=$argument; done
    printf '%s' "$literal" > "$MX_CMUX_FIXTURE/launch-input"
    script=${literal#\'}
    script=${script%\'}
    cat "$script" > "$MX_CMUX_FIXTURE/launch-script"
    [ "${MX_CMUX_FAIL_SUBMIT:-0}" = 0 ] || exit 99 ;;
  send-key)
    printf '%s\n' "$*" >> "$MX_CMUX_FIXTURE/sent"
    [ "${MX_CMUX_FAIL_SUBMIT:-0}" = 0 ] || exit 99
    bash "$MX_CMUX_FIXTURE/inert-terminal-start.sh" "$(cat "$MX_CMUX_FIXTURE/launch-input")" ;;
  close-workspace) rm -f "$MX_CMUX_FIXTURE/title" ;;
  *) echo "unexpected cmux request: $*" >&2; exit 99 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&cmux, fs::Permissions::from_mode(0o755)).unwrap();
    cmux
}

#[test]
fn herdr_repository_launch_preserves_real_allocation_and_nested_owner_routes() {
    use std::ffi::OsString;
    let f = Fixture::new();
    let herdr = f.fake.join("herdr");
    fs::write(
        &herdr,
        include_str!("../../../tests/fixtures/herdr-project-cli.py"),
    )
    .unwrap();
    fs::set_permissions(&herdr, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(f.fake.join("codex"), "#!/bin/sh\necho fixture-codex\n").unwrap();
    fs::set_permissions(f.fake.join("codex"), fs::Permissions::from_mode(0o755)).unwrap();
    let state = f.home.join("herdr.json");
    fs::write(&state, r#"{"next":0,"workspaces":[{"workspace_id":"human","label":"human","focused":true,"active_tab_id":"human:t"}],"tabs":[{"workspace_id":"human","tab_id":"human:t","label":"human","focused":true}],"panes":[{"workspace_id":"human","tab_id":"human:t","pane_id":"human:p","foreground_cwd":"/"}]}"#).unwrap();
    let command = |id: &str, project: &std::path::Path, options: &[&str]| {
        let base = f.spawn(&["--backend", "herdr", "--harness", "codex"]);
        let mut command = Command::new(base.get_program());
        command.current_dir(base.get_current_dir().unwrap());
        for (name, value) in base.get_envs() {
            if let Some(value) = value {
                command.env(name, value);
            } else {
                command.env_remove(name);
            }
        }
        let mut args = base.get_args().map(OsString::from).collect::<Vec<_>>();
        args[1] = id.into();
        args[2] = project.as_os_str().to_owned();
        command
            .args(args)
            .args(options)
            .env("MX_HERDR_BIN", &herdr)
            .env("HERDR_SESSION", "project-fixture")
            .env("MX_HERDR_FIXTURE", &state)
            .env("MX_INERT_START", f.fake.join("inert-terminal-start.sh"))
            .env("MX_TEST_INERT_PID_LOG", f.home.join("inert-pids"));
        command
    };
    let meta = |home: &std::path::Path, id: &str| {
        fs::read_to_string(home.join(format!("state/{id}.meta"))).unwrap()
    };
    let field = |text: &str, key: &str| {
        text.lines()
            .rev()
            .find_map(|line| line.strip_prefix(key))
            .unwrap()
            .to_owned()
    };
    success(&mut command("task", &f.project, &[]));
    let first = meta(&f.home, "task");
    assert!(first.contains("herdr_project_workspace=1"));
    assert!(std::path::Path::new(&field(&first, "worktree=")).is_dir());
    let workspace = field(&first, "herdr_workspace_id=");
    let private = f.home.parent().unwrap().join("legacy-standing");
    for directory in ["state", "data", "config", "projects", "bin"] {
        fs::create_dir_all(private.join(directory)).unwrap();
    }
    fs::write(private.join(".mx-agent-home"), "standing\n").unwrap();
    fs::write(private.join("AGENTS.md"), "# Fixture standing home\n").unwrap();
    fs::write(
        private.join("data/charter.md"),
        "Inspect the fixture repository.\n",
    )
    .unwrap();
    fs::copy(
        f.home.join("data/projects.json"),
        private.join("data/projects.json"),
    )
    .unwrap();
    success(&mut command(
        "standing",
        &private,
        &["--persistent", "--role", "researcher", "--output", "report"],
    ));
    let parent_text = meta(&f.home, "standing");
    assert_eq!(field(&parent_text, "herdr_workspace_id="), workspace);
    let parent = read_meta("standing", &parent_text).unwrap();
    fs::create_dir_all(private.join("data/nested")).unwrap();
    fs::write(
        private.join("data/nested/brief.md"),
        "Inspect this nested fixture.\n",
    )
    .unwrap();
    let attempt = parent.attempt.as_ref().unwrap();
    success(
        command("nested", &f.project, &[])
            .env("MX_HOME", &private)
            .env("MX_STATE_OVERRIDE", private.join("state"))
            .env("MX_DATA_OVERRIDE", private.join("data"))
            .env("MX_CONFIG_OVERRIDE", private.join("config"))
            .env("MX_PROJECTS_OVERRIDE", private.join("projects"))
            .env("MX_REPORT_STATE_OVERRIDE", f.home.join("state"))
            .env("MX_TASK_ID", "standing")
            .env("MX_ATTEMPT_ID", &attempt.id)
            .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
            .env(
                "MX_BRIEF_REVISION",
                parent.accepted_brief_revision.unwrap().to_string(),
            ),
    );
    let nested_text = meta(&private, "nested");
    assert_eq!(field(&nested_text, "herdr_workspace_id="), workspace);
    let nested = read_meta("nested", &nested_text).unwrap();
    assert_eq!(nested.root_id, parent.root_id);
    assert_eq!(nested.owner_home.as_deref(), private.to_str());
    assert_ne!(nested.owner_home, parent.owner_home);
    let inventory: serde_json::Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    assert_eq!(inventory["workspaces"].as_array().unwrap().len(), 2);
    assert_eq!(inventory["panes"].as_array().unwrap().len(), 4);
    let live = |home: &std::path::Path| {
        let base = command("task", &f.project, &[]);
        let mut cli = Command::new(base.get_program());
        for (name, value) in base.get_envs() {
            if let Some(value) = value {
                cli.env(name, value);
            } else {
                cli.env_remove(name);
            }
        }
        cli.current_dir(home)
            .env("MX_HOME", home)
            .args(["herdr", "list-live", "project-fixture"]);
        String::from_utf8(success(&mut cli).stdout).unwrap()
    };
    let root_live = live(&f.home);
    assert!(root_live.contains(&field(&first, "herdr_task_label=")));
    assert!(root_live.contains(&field(&parent_text, "herdr_task_label=")));
    assert!(!root_live.contains(&field(&nested_text, "herdr_task_label=")));
    let child_live = live(&private);
    assert!(child_live.contains(&field(&nested_text, "herdr_task_label=")));
    assert!(!child_live.contains(&field(&first, "herdr_task_label=")));

    // Automatic cleanup cannot erase a human's restored active shell. Only
    // canonical explicit teardown may close its own active endpoint, and must
    // preserve every sibling and private-home endpoint in the shared workspace.
    let tab = field(&first, "herdr_tab_id=");
    let pane = field(&first, "herdr_pane_id=");
    let mut active = inventory.clone();
    for space in active["workspaces"].as_array_mut().unwrap() {
        space["focused"] = (space["workspace_id"] == workspace).into();
        if space["workspace_id"] == workspace {
            space["active_tab_id"] = tab.clone().into();
        }
    }
    for item in active["tabs"].as_array_mut().unwrap() {
        item["focused"] = (item["tab_id"] == tab).into();
    }
    fs::write(&state, serde_json::to_vec(&active).unwrap()).unwrap();
    let cli = |args: &[&str]| {
        let base = command("task", &f.project, &[]);
        let mut cli = Command::new(base.get_program());
        cli.current_dir(&f.home);
        for (name, value) in base.get_envs() {
            if let Some(value) = value {
                cli.env(name, value);
            } else {
                cli.env_remove(name);
            }
        }
        cli.args(args);
        cli
    };
    success(&mut cli(&["herdr", "kill", &field(&first, "window=")]));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&state).unwrap()).unwrap(),
        active
    );
    assert!(f.home.join("state/task.meta").exists());
    success(&mut cli(&["teardown", "task"]));
    assert!(!f.home.join("state/task.meta").exists());
    let after: serde_json::Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    assert_eq!(after["workspaces"], active["workspaces"]);
    assert_eq!(after["panes"].as_array().unwrap().len(), 3);
    assert!(
        !after["panes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["pane_id"] == pane)
    );
    for retained in [
        "human:p".to_owned(),
        field(&parent_text, "herdr_pane_id="),
        field(&nested_text, "herdr_pane_id="),
    ] {
        assert!(
            after["panes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["pane_id"] == retained)
        );
    }
}

fn evidence_request(record: &TaskRecord, commit: &str, evidence_id: &str) -> serde_json::Value {
    let attempt = record.attempt.as_ref().unwrap();
    serde_json::json!({
        "evidence_id": evidence_id,
        "attempt_id": attempt.id,
        "attempt_generation": attempt.generation,
        "brief_revision": record.accepted_brief_revision,
        "commit": commit,
        "checks": [],
        "review": null,
        "limitations": [],
        "pr_url": null,
        "outcome": "evidence-updated",
        "observed_at": "2026-09-23T12:00:00Z",
        "mark_current": true,
        "expected_current_commit": record.delivery.current_commit,
    })
}

fn write_evidence(f: &Fixture, record: &TaskRecord, commit: &str, evidence_id: &str) -> PathBuf {
    let path = f.home.join(format!("{evidence_id}.json"));
    fs::write(
        &path,
        serde_json::to_vec(&evidence_request(record, commit, evidence_id)).unwrap(),
    )
    .unwrap();
    path
}

fn report_done(f: &Fixture, record: &TaskRecord, commit: &str, revision: u64) -> Output {
    let attempt = record.attempt.as_ref().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    command
        .env("MX_HOME", &f.home)
        .env("MX_ROOT_OVERRIDE", &f.home)
        .env("MX_STATE_OVERRIDE", f.home.join("state"))
        .env("MX_REPORT_STATE_OVERRIDE", f.home.join("state"))
        .env("MX_TASK_ID", "task")
        .env("MX_ATTEMPT_ID", &attempt.id)
        .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
        .env("MX_BRIEF_REVISION", revision.to_string())
        .env("MX_NUDGE", "0")
        .args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "done",
            "--message",
            commit,
        ]);
    command.output().unwrap()
}

fn report_working(f: &Fixture, record: &TaskRecord, revision: u64) -> Output {
    let attempt = record.attempt.as_ref().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    command
        .env("MX_HOME", &f.home)
        .env("MX_ROOT_OVERRIDE", &f.home)
        .env("MX_STATE_OVERRIDE", f.home.join("state"))
        .env("MX_REPORT_STATE_OVERRIDE", f.home.join("state"))
        .env("MX_TASK_ID", "task")
        .env("MX_ATTEMPT_ID", &attempt.id)
        .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
        .env("MX_BRIEF_REVISION", revision.to_string())
        .env("MX_NUDGE", "0")
        .args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "working",
            "--message",
            "continuing implementation",
        ]);
    command.output().unwrap()
}

#[test]
fn routed_dependencies_wait_for_current_completion_and_preserve_the_accepted_start() {
    let f = Fixture::new();
    let cmux = install_fake_cmux(&f);
    fs::create_dir_all(f.home.join("data/task")).unwrap();
    fs::write(f.home.join("data/task/brief.md"), "Predecessor\n").unwrap();
    let predecessor_spawn = cmux_spawn(&f, "task", None, &cmux).output().unwrap();
    assert!(
        predecessor_spawn.status.success(),
        "{}",
        String::from_utf8_lossy(&predecessor_spawn.stderr)
    );
    let state = f.home.join("state");
    let predecessor = read_meta(
        "task",
        &fs::read_to_string(state.join("task.meta")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        predecessor.schedule.state,
        multplx_domain::lifecycle::subagent_model::WorkState::Running
    );

    let base = f.git(&["rev-parse", "HEAD"]);
    fs::create_dir_all(f.home.join("data/dependent")).unwrap();
    fs::write(f.home.join("data/dependent/brief.md"), "Dependent\n").unwrap();
    let mut intake = Command::new(env!("CARGO_BIN_EXE_mx"));
    intake
        .env("MX_HOME", &f.home)
        .env("MX_ROOT_OVERRIDE", &f.home)
        .env("MX_STATE_OVERRIDE", &state)
        .env("MX_DATA_OVERRIDE", f.home.join("data"))
        .args([
            "task",
            "--project",
            f.project.to_str().unwrap(),
            "--request-id",
            "request-dependent",
            "--task-id",
            "dependent",
            "--start",
            &base,
            "--depends",
            "task",
            "Dependent work",
        ]);
    let accepted: serde_json::Value = serde_json::from_slice(&success(&mut intake).stdout).unwrap();
    assert_eq!(
        accepted["request"]["dependencies"],
        serde_json::json!(["task"])
    );

    fs::write(f.project.join("later.txt"), "after intake\n").unwrap();
    f.git(&["add", "later.txt"]);
    f.git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--quiet",
        "-m",
        "after intake",
    ]);

    let queued = cmux_spawn(&f, "dependent", Some("request-dependent"), &cmux)
        .output()
        .unwrap();
    assert!(
        queued.status.success(),
        "{}",
        String::from_utf8_lossy(&queued.stderr)
    );
    assert!(String::from_utf8_lossy(&queued.stdout).contains("queued:"));
    assert_eq!(
        fs::read_to_string(f.fake.join("cwds"))
            .unwrap()
            .lines()
            .count(),
        1
    );

    let predecessor = read_meta(
        "task",
        &fs::read_to_string(state.join("task.meta")).unwrap(),
    )
    .unwrap();
    let allocation = predecessor.allocation.as_ref().unwrap();
    let commit = Command::new("git")
        .arg("-C")
        .arg(&allocation.path)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let commit = String::from_utf8(commit.stdout).unwrap().trim().to_owned();
    let unproven_done = report_done(&f, &predecessor, &commit, 1);
    assert!(unproven_done.status.success());
    assert!(
        String::from_utf8_lossy(&unproven_done.stderr)
            .contains("implementation completion was not proven")
    );
    let still_running = read_meta(
        "task",
        &fs::read_to_string(state.join("task.meta")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        still_running.schedule.state,
        multplx_domain::lifecycle::subagent_model::WorkState::Running
    );
    let evidence = write_evidence(&f, &predecessor, &commit, "evidence-one");
    let recorded = run(Command::new(env!("CARGO_BIN_EXE_mx"))
        .env("MX_HOME", &f.home)
        .env("MX_ROOT_OVERRIDE", &f.home)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["task-model", "evidence", "task", "--request-file"])
        .arg(&evidence));
    assert!(
        recorded.status.success(),
        "{}",
        String::from_utf8_lossy(&recorded.stderr)
    );
    let completed = report_done(&f, &predecessor, &commit, 1);
    assert!(
        completed.status.success(),
        "{}",
        String::from_utf8_lossy(&completed.stderr)
    );
    let completed_record = read_meta(
        "task",
        &fs::read_to_string(state.join("task.meta")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        completed_record.schedule.state,
        multplx_domain::lifecycle::subagent_model::WorkState::Completed
    );
    let renewed = report_working(&f, &completed_record, 1);
    assert!(renewed.status.success());
    let reopened = read_meta(
        "task",
        &fs::read_to_string(state.join("task.meta")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        reopened.schedule.state,
        multplx_domain::lifecycle::subagent_model::WorkState::Running
    );

    let revised = run(Command::new(env!("CARGO_BIN_EXE_mx"))
        .env("MX_HOME", &f.home)
        .env("MX_ROOT_OVERRIDE", &f.home)
        .env("MX_STATE_OVERRIDE", &state)
        .args([
            "task-model",
            "revise",
            "task",
            "--expected-revision",
            "1",
            "--scope",
            "Updated predecessor",
            "--reason",
            "revision fence",
        ]));
    assert!(
        revised.status.success(),
        "{}",
        String::from_utf8_lossy(&revised.stderr)
    );
    let stale = report_done(&f, &completed_record, &commit, 1);
    assert!(
        !stale.status.success(),
        "stale completion unexpectedly succeeded"
    );
    let reopened = read_meta(
        "task",
        &fs::read_to_string(state.join("task.meta")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        reopened.schedule.state,
        multplx_domain::lifecycle::subagent_model::WorkState::Runnable
    );

    let runner = f.home.join("spawn-queued.sh");
    fs::write(
        &runner,
        format!(
            "#!/bin/sh\nexec '{}' spawn \"$@\" --backend cmux\n",
            env!("CARGO_BIN_EXE_mx")
        ),
    )
    .unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o700)).unwrap();
    let pending = run(Command::new(env!("CARGO_BIN_EXE_mx"))
        .env("MX_HOME", &f.home)
        .env("MX_ROOT_OVERRIDE", &f.home)
        .env("MX_STATE_OVERRIDE", &state)
        .env("MX_HEADROOM_CPU_COUNT", "8")
        .env("MX_HEADROOM_LOAD1", "0")
        .env("MX_HEADROOM_MEM_AVAILABLE_BYTES", "17179869184")
        .env("MX_HEADROOM_API_CAPACITY", "8")
        .env("MX_HEADROOM_IN_USE", "0")
        .env("MX_HEADROOM_SPAWN_BIN", &runner)
        .env("MX_CMUX_FIXTURE", &f.fake)
        .env("MX_CMUX_BIN", &cmux)
        .env(
            "PATH",
            format!("{}:{}", f.fake.display(), std::env::var("PATH").unwrap()),
        )
        .args(["headroom", "--queue-drain"]));
    assert!(
        pending.status.success(),
        "{}",
        String::from_utf8_lossy(&pending.stderr)
    );
    assert_eq!(
        fs::read_to_string(f.fake.join("cwds"))
            .unwrap()
            .lines()
            .count(),
        1
    );

    let reopened = read_meta(
        "task",
        &fs::read_to_string(state.join("task.meta")).unwrap(),
    )
    .unwrap();
    let refreshed = write_evidence(&f, &reopened, &commit, "evidence-two");
    let recorded = run(Command::new(env!("CARGO_BIN_EXE_mx"))
        .env("MX_HOME", &f.home)
        .env("MX_ROOT_OVERRIDE", &f.home)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["task-model", "evidence", "task", "--request-file"])
        .arg(&refreshed));
    assert!(
        recorded.status.success(),
        "{}",
        String::from_utf8_lossy(&recorded.stderr)
    );
    let current = read_meta(
        "task",
        &fs::read_to_string(state.join("task.meta")).unwrap(),
    )
    .unwrap();
    assert!(report_done(&f, &current, &commit, 2).status.success());

    let launched = run(Command::new(env!("CARGO_BIN_EXE_mx"))
        .env("MX_HOME", &f.home)
        .env("MX_ROOT_OVERRIDE", &f.home)
        .env(
            "MX_RUST_SOURCE_ROOT",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        )
        .env("MX_STATE_OVERRIDE", &state)
        .env("MX_HEADROOM_CPU_COUNT", "8")
        .env("MX_HEADROOM_LOAD1", "0")
        .env("MX_HEADROOM_MEM_AVAILABLE_BYTES", "17179869184")
        .env("MX_HEADROOM_API_CAPACITY", "8")
        .env("MX_HEADROOM_IN_USE", "0")
        .env("MX_HEADROOM_SPAWN_BIN", &runner)
        .env("MX_CMUX_FIXTURE", &f.fake)
        .env("MX_CMUX_BIN", &cmux)
        .env(
            "PATH",
            format!("{}:{}", f.fake.display(), std::env::var("PATH").unwrap()),
        )
        .args(["headroom", "--queue-drain"]));
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    assert_eq!(
        fs::read_to_string(f.fake.join("cwds"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    let dependent = read_meta(
        "dependent",
        &fs::read_to_string(state.join("dependent.meta")).unwrap(),
    )
    .unwrap();
    assert_eq!(dependent.schedule.dependencies, ["task"]);
    assert_eq!(dependent.project.as_ref().unwrap().starting_revision, base);
    assert_eq!(dependent.allocation.as_ref().unwrap().base_revision, base);
}

fn cmux_spawn(f: &Fixture, id: &str, request: Option<&str>, cmux: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("MX_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(&f.home)
        .env("MX_HOME", &f.home)
        .env("MX_ROOT_OVERRIDE", &f.home)
        .env(
            "MX_RUST_SOURCE_ROOT",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        )
        .env("MX_STATE_OVERRIDE", f.home.join("state"))
        .env("MX_DATA_OVERRIDE", f.home.join("data"))
        .env("MX_CONFIG_OVERRIDE", f.home.join("config"))
        .env("MX_HEADROOM_CPU_COUNT", "8")
        .env("MX_HEADROOM_LOAD1", "0")
        .env("MX_HEADROOM_MEM_AVAILABLE_BYTES", "17179869184")
        .env("MX_HEADROOM_API_CAPACITY", "8")
        .env("MX_HEADROOM_IN_USE", "0")
        .env("MX_CMUX_FIXTURE", &f.fake)
        .env("MX_CMUX_BIN", cmux)
        .env(
            "PATH",
            format!("{}:{}", f.fake.display(), std::env::var("PATH").unwrap()),
        )
        .args(["spawn", id])
        .arg(&f.project)
        .args(["--backend", "cmux"]);
    if let Some(request) = request {
        command.args(["--request-id", request]);
    }
    command
}

#[test]
fn single_checkout_refuses_dirty_detached_and_already_reserved_sources_before_allocation() {
    for kind in ["dirty", "detached", "reserved", "missing-grant", "scout"] {
        let f = Fixture::new();
        let before = f.git(&["rev-parse", "HEAD"]);
        let expected = match kind {
            "dirty" => {
                fs::write(f.project.join("human-work"), "retain me").unwrap();
                "requires a clean checkout"
            }
            "detached" => {
                f.git(&["checkout", "--detach", "-q"]);
                "requires an attached base branch"
            }
            "reserved" => {
                let digest =
                    multplx_domain::maintainer_override::sha256_text(f.project.to_str().unwrap());
                fs::write(
                    f.home.join(format!("state/.single-checkout-{digest}.json")),
                    "existing receipt",
                )
                .unwrap();
                "already has a single-checkout reservation"
            }
            "scout" => "supported only for one delivery task",
            _ => "request identity is duplicated, missing, or misplaced: missing-grant",
        };
        let mut options = vec!["--single-checkout", "missing-grant"];
        if kind == "scout" {
            options.push("--scout");
        }
        f.refused(f.spawn(&options).output().unwrap(), expected);
        assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
        assert!(!f.home.join("state/.spawn-task.intent").exists());
        if kind == "dirty" {
            assert_eq!(
                fs::read_to_string(f.project.join("human-work")).unwrap(),
                "retain me"
            );
        }
    }
}

#[test]
fn durable_launch_intent_retries_once_with_the_original_base_and_allocation() {
    let f = Fixture::new();
    let before = f.git(&["rev-parse", "HEAD"]);
    f.refused(
        f.spawn(&[])
            .env("MX_SPAWN_FAULT", "after-intent")
            .output()
            .unwrap(),
        "interruption after durable launch intent",
    );
    let intent_path = f.home.join("state/.spawn-task.intent");
    let intent = fs::read(&intent_path).unwrap();
    let binding: serde_json::Value = serde_json::from_slice(&intent).unwrap();
    assert!(String::from_utf8_lossy(&intent).contains(&before));
    assert!(binding["attempt"]["id"].is_string());
    f.git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--allow-empty",
        "-qm",
        "source moved",
    ]);
    let moved = f.git(&["rev-parse", "HEAD"]);
    let retry = f.spawn(&[]).output().unwrap();
    assert_eq!(retry.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&retry.stderr).contains("backend command failed"));
    let action_path = f.home.join("state/.spawn-actions/task.json");
    let action: serde_json::Value =
        serde_json::from_slice(&fs::read(&action_path).expect("action receipt")).unwrap();
    assert_eq!(action["binding"]["project"]["starting_revision"], before);
    assert_eq!(action["stage"], "failed");
    let allocation = action["binding"]["allocation"]["allocation_id"]
        .as_str()
        .expect("allocation")
        .to_owned();
    let worktrees = f
        .git(&["worktree", "list", "--porcelain"])
        .matches("worktree ")
        .count();
    let repeated = f.spawn(&[]).output().unwrap();
    assert_eq!(repeated.status.code(), Some(1));
    let repeated_action: serde_json::Value =
        serde_json::from_slice(&fs::read(action_path).expect("repeated action")).unwrap();
    assert_eq!(
        repeated_action["binding"]["allocation"]["allocation_id"],
        allocation
    );
    assert_eq!(
        f.git(&["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        worktrees
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), moved);
}

#[test]
fn invalid_launch_configuration_refuses_without_an_allocation_or_intent() {
    let f = Fixture::new();
    fs::write(f.home.join("config/subagent-dispatch.json"), "{}\n").unwrap();
    f.refused(f.spawn(&[]).output().unwrap(), "pass an explicit harness");
    assert!(!f.home.join("state/.spawn-task.intent").exists());
    f.refused(
        f.spawn(&["--replace-attempt"]).output().unwrap(),
        "requires current attempt id",
    );
    f.refused(
        f.spawn(&["--single-checkout"]).output().unwrap(),
        "requires",
    );
    f.refused(
        f.spawn(&["--resource", "session=2"]).output().unwrap(),
        "invalid or reserved",
    );
    f.refused(
        f.spawn(&["--resource", "gpu=0"]).output().unwrap(),
        "invalid or reserved",
    );
    f.refused(
        f.spawn(&["--resource"]).output().unwrap(),
        "requires NAME=UNITS",
    );
    assert!(!f.home.join("state/.spawn-task.intent").exists());
}

#[test]
fn invalid_filesystem_and_byte_inputs_cannot_publish_or_teardown_an_execution() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let f = Fixture::new();
    let blocked = f.home.join("blocked-state");
    fs::write(&blocked, "private state must survive").unwrap();
    f.refused(
        f.spawn(&[])
            .env("MX_STATE_OVERRIDE", &blocked)
            .output()
            .unwrap(),
        "cannot create state directory",
    );
    assert_eq!(
        fs::read_to_string(blocked).unwrap(),
        "private state must survive"
    );
    let invalid = OsString::from_vec(vec![0xff]);
    let output = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args(["spawn", "task=project"])
        .arg(&invalid)
        .env("MX_HOME", &f.home)
        .output()
        .unwrap();
    f.refused(output, "spawn argument is not valid UTF-8");
    for args in [
        vec![
            OsString::from("teardown"),
            invalid.clone(),
            "--override".into(),
            "request".into(),
        ],
        vec![
            OsString::from("teardown"),
            "task".into(),
            "--override".into(),
            invalid,
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mx"))
            .args(args)
            .env("MX_HOME", &f.home)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("not valid UTF-8"));
    }
    assert!(!f.home.join("state/.spawn-task.intent").exists());
    assert!(!f.home.join("state/task.meta").exists());
}

#[test]
fn native_cmux_launch_binds_exact_git_allocation_and_retains_it_after_endpoint_failure() {
    for failure in ["none", "after-endpoint", "after-metadata"] {
        let fail_after_endpoint = failure == "after-endpoint";
        let fail_after_metadata = failure == "after-metadata";
        let f = Fixture::new();
        let cmux = f.fake.join("cmux");
        fs::write(&cmux, r#"#!/bin/sh
set -eu
case "$1" in
  version) echo 'cmux 0.64.17 (97) [abcdef1]' ;;
  ping) echo PONG ;;
  workspace)
    if [ -f "$MX_CMUX_FIXTURE/title" ]; then
      printf '{"workspaces":[{"id":"11111111-1111-4111-8111-111111111111","title":"%s"}]}' "$(cat "$MX_CMUX_FIXTURE/title")"
    else echo '{"workspaces":[]}'; fi ;;
  new-workspace)
    shift
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --name) shift; printf '%s' "$1" > "$MX_CMUX_FIXTURE/title" ;;
        --cwd) shift; git -C "$1" rev-parse --git-common-dir >/dev/null; printf '%s' "$1" > "$MX_CMUX_FIXTURE/cwd" ;;
      esac
      shift
    done ;;
  list-panes) echo '{"panes":[{"selected_surface_id":"22222222-2222-4222-8222-222222222222","surface_ids":["22222222-2222-4222-8222-222222222222"]}]}' ;;
  send)
    printf '%s\n' "$*" >> "$MX_CMUX_FIXTURE/sent"
    for argument do literal=$argument; done
    printf '%s' "$literal" > "$MX_CMUX_FIXTURE/launch-input"
    script=${literal#\'}
    script=${script%\'}
    cat "$script" > "$MX_CMUX_FIXTURE/launch-script"
    [ "${MX_CMUX_FAIL_SUBMIT:-0}" = 0 ] || exit 99 ;;
  send-key)
    printf '%s\n' "$*" >> "$MX_CMUX_FIXTURE/sent"
    [ "${MX_CMUX_FAIL_SUBMIT:-0}" = 0 ] || exit 99
    bash "$MX_CMUX_FIXTURE/inert-terminal-start.sh" "$(cat "$MX_CMUX_FIXTURE/launch-input")" ;;
  close-workspace) rm "$MX_CMUX_FIXTURE/title" ;;
  *) echo "unexpected cmux request: $*" >&2; exit 99 ;;
esac
"#).unwrap();
        fs::set_permissions(&cmux, fs::Permissions::from_mode(0o755)).unwrap();
        let before = f.git(&["rev-parse", "HEAD"]);
        let mut command = f.spawn(&["--backend", "cmux"]);
        command
            .env("MX_CMUX_FIXTURE", &f.fake)
            .env("MX_CMUX_BIN", &cmux);
        if fail_after_metadata {
            command.env("MX_CMUX_FAIL_SUBMIT", "1");
        }
        if fail_after_endpoint {
            command.env("MX_SPAWN_FAULT", "after-endpoint");
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(i32::from(failure != "none")),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cwd = fs::read_to_string(f.fake.join("cwd")).unwrap();
        assert_ne!(PathBuf::from(&cwd), f.project);
        assert_eq!(
            Command::new("git")
                .arg("-C")
                .arg(&cwd)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
            format!("{before}\n").as_bytes()
        );
        assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
        if fail_after_endpoint {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("injected failure after endpoint")
            );
            assert!(f.home.join("state/.spawn-task.intent").exists());
            assert!(!f.home.join("state/task.meta").exists());
            assert!(
                !f.fake.join("title").exists(),
                "failed endpoint was not closed"
            );
            assert!(
                !f.fake.join("sent").exists(),
                "harness submitted after endpoint fault"
            );
        } else if fail_after_metadata {
            let raw = fs::read_to_string(f.home.join("state/task.meta")).unwrap();
            let record =
                multplx_domain::lifecycle::subagent_model::read_meta("task", &raw).unwrap();
            assert_eq!(
                record.schedule.state,
                multplx_domain::lifecycle::subagent_model::WorkState::WaitingExternal
            );
            assert!(
                record
                    .schedule
                    .waiting_condition
                    .unwrap()
                    .contains("launch failed; reconcile retained endpoint/worktree")
            );
            assert!(f.home.join("state/.spawn-task.intent").exists());
            assert!(
                f.fake.join("title").exists(),
                "uncertain submitted endpoint must remain available for reconciliation"
            );
            assert!(PathBuf::from(&cwd).is_dir());
            assert!(raw.contains(&format!("worktree={cwd}\n")));
        } else {
            let raw = fs::read_to_string(f.home.join("state/task.meta")).unwrap();
            let record =
                multplx_domain::lifecycle::subagent_model::read_meta("task", &raw).unwrap();
            assert!(raw.contains(&format!("worktree={cwd}\n")));
            assert_eq!(record.attempt.unwrap().generation, 1);
            assert!(
                fs::read_to_string(f.fake.join("launch-script"))
                    .unwrap()
                    .contains("MX_ATTEMPT_ID=")
            );
            assert!(!f.home.join("state/.spawn-task.intent").exists());
        }
        assert!(!f.home.join("sentinel.log").exists());
    }
}

#[test]
fn interrupted_endpoint_retry_requires_fresh_absence_and_preserves_allocation() {
    let f = Fixture::new();
    let cmux = f.fake.join("cmux");
    fs::write(
        &cmux,
        r#"#!/bin/sh
set -eu
case "$1" in
  version) echo 'cmux 0.64.17 (97) [abcdef1]' ;;
  ping) echo PONG ;;
  workspace)
    if [ -f "$MX_CMUX_FIXTURE/observe-fail" ]; then
      remaining=$(cat "$MX_CMUX_FIXTURE/observe-fail")
      if [ "$remaining" -gt 1 ]; then expr "$remaining" - 1 > "$MX_CMUX_FIXTURE/observe-fail"; else rm "$MX_CMUX_FIXTURE/observe-fail"; fi
      exit 99
    fi
    if [ -f "$MX_CMUX_FIXTURE/title" ]; then printf '{"workspaces":[{"id":"11111111-1111-4111-8111-111111111111","title":"%s"}]}' "$(cat "$MX_CMUX_FIXTURE/title")"; else echo '{"workspaces":[]}'; fi ;;
  new-workspace)
    shift
    while [ "$#" -gt 0 ]; do
      case "$1" in --name) shift; printf '%s' "$1" > "$MX_CMUX_FIXTURE/title";; --cwd) shift; printf '%s' "$1" > "$MX_CMUX_FIXTURE/cwd";; esac
      shift
    done ;;
  list-panes) echo '{"panes":[{"selected_surface_id":"22222222-2222-4222-8222-222222222222","surface_ids":["22222222-2222-4222-8222-222222222222"]}]}' ;;
  send)
    printf '%s\n' "$*" >> "$MX_CMUX_FIXTURE/sent"
    for argument do literal=$argument; done
    printf '%s' "$literal" > "$MX_CMUX_FIXTURE/launch-input"
    script=${literal#\'}
    script=${script%\'}
    cat "$script" > "$MX_CMUX_FIXTURE/launch-script"
    [ "${MX_CMUX_FAIL_SUBMIT:-0}" = 0 ] || exit 99 ;;
  send-key)
    printf '%s\n' "$*" >> "$MX_CMUX_FIXTURE/sent"
    [ "${MX_CMUX_FAIL_SUBMIT:-0}" = 0 ] || exit 99
    bash "$MX_CMUX_FIXTURE/inert-terminal-start.sh" "$(cat "$MX_CMUX_FIXTURE/launch-input")" ;;
  close-workspace) printf '2' > "$MX_CMUX_FIXTURE/observe-fail" ;;
  *) exit 99 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&cmux, fs::Permissions::from_mode(0o755)).unwrap();

    let failed = f
        .spawn(&["--backend", "cmux"])
        .env("MX_CMUX_FIXTURE", &f.fake)
        .env("MX_CMUX_BIN", &cmux)
        .env("MX_SPAWN_FAULT", "after-endpoint")
        .output()
        .unwrap();
    assert_eq!(
        failed.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("injected failure after endpoint"),
        "{}",
        String::from_utf8_lossy(&failed.stderr)
    );
    let action_path = f.home.join("state/.spawn-actions/task.json");
    let retained: serde_json::Value =
        serde_json::from_slice(&fs::read(&action_path).unwrap()).unwrap();
    assert_eq!(retained["stage"], "endpoint-created");
    let allocation = retained["binding"]["allocation"].clone();
    let attempt = retained["binding"]["attempt"].clone();

    let unknown = f
        .spawn(&["--backend", "cmux"])
        .env("MX_CMUX_FIXTURE", &f.fake)
        .env("MX_CMUX_BIN", &cmux)
        .output()
        .unwrap();
    assert_eq!(
        unknown.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("uncertain endpoint"),
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );
    let live = f
        .spawn(&["--backend", "cmux"])
        .env("MX_CMUX_FIXTURE", &f.fake)
        .env("MX_CMUX_BIN", &cmux)
        .output()
        .unwrap();
    assert_eq!(
        live.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&live.stderr)
    );
    assert!(
        String::from_utf8_lossy(&live.stderr).contains("uncertain endpoint"),
        "{}",
        String::from_utf8_lossy(&live.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&action_path).unwrap()).unwrap()["stage"],
        "endpoint-created"
    );

    fs::remove_file(f.fake.join("title")).unwrap();
    let recovered = f
        .spawn(&["--backend", "cmux"])
        .env("MX_CMUX_FIXTURE", &f.fake)
        .env("MX_CMUX_BIN", &cmux)
        .output()
        .unwrap();
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    let final_action: serde_json::Value =
        serde_json::from_slice(&fs::read(action_path).unwrap()).unwrap();
    assert_eq!(final_action["stage"], "running");
    assert_eq!(final_action["binding"]["allocation"], allocation);
    assert_eq!(final_action["binding"]["attempt"], attempt);
}

#[test]
fn display_model_labels_fail_before_allocations_or_endpoint_calls() {
    let f = Fixture::new();
    for harness in ["codex", "claude", "cursor", "pi"] {
        f.refused(
            run(&mut f.spawn(&["--harness", harness, "--model", "GPT-6 Luna"])),
            "model selection",
        );
        assert!(!f.home.join("state/launch-actions").exists());
    }
}

#[test]
fn primary_public_launcher_and_worker_package_fallback_share_exact_hook_arguments() {
    let f = Fixture::new();
    let runtime = f.home.join("runtime");
    fs::create_dir_all(runtime.join("bin")).unwrap();
    fs::create_dir_all(runtime.join("target/release")).unwrap();
    fs::write(runtime.join("AGENTS.md"), "fixture\n").unwrap();
    for script in [
        "mx-lock.sh",
        "mx-native-observe.sh",
        "mx-subagent-pretool-check.sh",
        "mx-codex-idle.sh",
        "mx-turnend-guard.sh",
    ] {
        let path = runtime.join("bin").join(script);
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let public = f.home.join("multplx");
    let packaged = runtime.join("target/release/mx");
    fs::hard_link(env!("CARGO_BIN_EXE_mx"), &public).unwrap();
    fs::hard_link(env!("CARGO_BIN_EXE_mx"), &packaged).unwrap();
    let fake_provider = f.fake.join("provider");
    fs::write(
        &fake_provider,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$MX_HOME/primary-args\"\nprintf '%s\\n' \"${CODEX_HOME:-unset}\" > \"$MX_HOME/provider-home\"\n",
    )
    .unwrap();
    fs::set_permissions(&fake_provider, fs::Permissions::from_mode(0o755)).unwrap();
    let provider_home = f.home.join("Codex's isolated home");
    fs::create_dir(&provider_home).unwrap();
    let mut primary = Command::new(&public);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("MX_") {
            primary.env_remove(key);
        }
    }
    success(
        primary
            .args(["launch-harness", "codex"])
            .env("MX_ROOT_OVERRIDE", &runtime)
            .env("MX_HOME", &f.home)
            .env("MX_LAUNCH_VALIDATED", "1")
            .env("MX_MULTICALL_EXPLICIT", "1")
            .env("MX_REAL_CODEX", &fake_provider)
            .env("CODEX_HOME", &provider_home),
    );
    let primary_args = fs::read_to_string(f.home.join("primary-args")).unwrap();
    let cmux = install_fake_cmux(&f);
    let template = f.spawn(&["--backend", "cmux", "--model", "custom-provider-model"]);
    let mut worker = Command::new(&packaged);
    worker
        .args(template.get_args())
        .current_dir(template.get_current_dir().unwrap());
    for (key, value) in template.get_envs() {
        if let Some(value) = value {
            worker.env(key, value);
        } else {
            worker.env_remove(key);
        }
    }
    success(
        worker
            .env("MX_RUST_SOURCE_ROOT", &runtime)
            .env("MX_CMUX_FIXTURE", &f.fake)
            .env("MX_CMUX_BIN", &cmux)
            .env("MX_REAL_CODEX", &fake_provider)
            .env("CODEX_HOME", &provider_home),
    );
    let launch = fs::read_to_string(f.fake.join("launch-script")).unwrap();
    let quoted_home = format!(
        "CODEX_HOME='{}'",
        provider_home.display().to_string().replace('\'', "'\\''")
    );
    assert!(launch.contains(&quoted_home));
    // A terminal server may carry a different home (or no caller environment).
    // Execute the captured real launch in that shell against an inert provider.
    success(
        Command::new("/bin/sh")
            .arg(f.fake.join("launch-script"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("CODEX_HOME", f.home.join("wrong-terminal-home")),
    );
    assert_eq!(
        fs::read_to_string(f.home.join("provider-home"))
            .unwrap()
            .trim(),
        provider_home.to_str().unwrap()
    );

    let definitions = primary_args
        .lines()
        .filter(|value| value.starts_with("hooks."))
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 6);
    assert!(
        definitions
            .iter()
            .any(|definition| definition.starts_with("hooks.Stop="))
    );
    assert!(
        definitions
            .iter()
            .any(|definition| definition.starts_with("hooks.SessionEnd="))
    );
    for definition in definitions {
        let quoted = format!("'{}'", definition.replace('\'', "'\\''"));
        assert!(
            launch.contains(&quoted),
            "worker does not reuse primary definition: {definition}"
        );
    }
    let metadata = fs::read_to_string(f.home.join("state/task.meta")).unwrap();
    assert!(metadata.contains("model=custom-provider-model\n"));
    assert!(launch.contains("--model 'custom-provider-model'"));
}

#[test]
fn coordinator_display_model_fails_before_home_provisioning() {
    let f = Fixture::new();
    let template = f.spawn(&[]);
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    command.current_dir(&f.home);
    for (key, value) in template.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    f.refused(
        run(command.args([
            "spawn",
            "task",
            "--sub-orchestrator",
            "--idea",
            "bounded-idea",
            "--scope",
            "bounded research",
            "--harness",
            "codex",
            "--model",
            "GPT-6 Luna",
        ])),
        "model selection",
    );
    assert!(!f.home.join("data/agents.md").exists());
    assert!(!f.home.join("state/.spawn-task.intent").exists());
}

#[test]
fn standing_report_worker_retains_explicit_codex_home_in_a_separate_terminal_shell() {
    let f = Fixture::new();
    for directory in ["state", "data", "config", "projects", "bin"] {
        fs::create_dir_all(f.project.join(directory)).unwrap();
    }
    fs::write(f.project.join(".mx-agent-home"), "task\n").unwrap();
    fs::write(f.project.join("AGENTS.md"), "# Fixture standing home\n").unwrap();
    fs::write(
        f.project.join("data/charter.md"),
        "Write a bounded fixture report.\n",
    )
    .unwrap();
    let provider_home = f.home.join("isolated codex home");
    fs::create_dir(&provider_home).unwrap();
    let provider = f.fake.join("provider");
    fs::write(&provider, "#!/bin/sh\nprintf '%s\\n' \"$CODEX_HOME\" \"$MX_TASK_ID\" \"$MX_ATTEMPT_ID\" > \"$MX_HOME/provider-context\"\n").unwrap();
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o755)).unwrap();
    let tmux = f.fake.join("tmux");
    fs::write(
        f.fake.join("inert-terminal-tmux.sh"),
        include_str!("../../../tests/inert-terminal-tmux.sh"),
    )
    .unwrap();
    fs::write(
        &tmux,
        r#"#!/bin/sh
set -eu
fixture=$(dirname "$0")
if [ "$1" = send-keys ]; then
    previous=
    for argument do
        if [ "$previous" = -l ]; then
            script=${argument#\'}; script=${script%\'}
            cat "$script" > "$fixture/launch-script"
        fi
        previous=$argument
    done
fi
bash "$fixture/inert-terminal-tmux.sh" "$@"
case "$1" in
    has-session|new-session) exit 0 ;;
    new-window) printf '%s\n' '@context' ;;
    display-message) printf '%s\n' '%1' ;;
    list-windows) [ -f "$fixture/launch-script" ] && printf '%s\n' 'mx-task'; exit 0 ;;
    *) exit 0 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755)).unwrap();
    success(
        f.spawn(&[
            "--persistent",
            "--role",
            "researcher",
            "--output",
            "report",
            "--backend",
            "tmux",
        ])
        .env("MX_TMUX_BIN", &tmux)
        .env("MX_REAL_CODEX", &provider)
        .env("CODEX_HOME", &provider_home),
    );
    let metadata = fs::read_to_string(f.home.join("state/task.meta")).unwrap();
    let task = read_meta("task", &metadata).unwrap();
    assert!(task.persistent && task.private_home);
    assert_eq!(
        task.role,
        multplx_domain::lifecycle::subagent_model::AssignmentRole::Researcher
    );
    assert!(
        task.project.is_none(),
        "report project references do not allocate a checkout"
    );
    success(
        Command::new("/bin/sh")
            .arg(f.fake.join("launch-script"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("CODEX_HOME", f.home.join("wrong-terminal-home")),
    );
    let actual = fs::read_to_string(f.project.join("provider-context")).unwrap();
    let lines = actual.lines().collect::<Vec<_>>();
    assert_eq!(
        lines,
        vec![
            provider_home.to_str().unwrap(),
            "task",
            &task.attempt.unwrap().id
        ]
    );
}

#[test]
fn relative_codex_home_refused_before_task_or_backend_mutation() {
    let f = Fixture::new();
    f.refused(
        run(f.spawn(&[]).env("CODEX_HOME", "relative")),
        "CODEX_HOME must be absolute",
    );
}

#[test]
fn codex_default_home_is_not_replaced_by_the_terminal_server_environment() {
    let f = Fixture::new();
    let provider = f.fake.join("provider");
    fs::write(
        &provider,
        "#!/bin/sh\nprintf '%s\\n' \"${CODEX_HOME:-unset}\" > \"$MX_HOME/provider-home\"\n",
    )
    .unwrap();
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o755)).unwrap();
    let cmux = install_fake_cmux(&f);
    success(
        f.spawn(&["--backend", "cmux"])
            .env("MX_CMUX_FIXTURE", &f.fake)
            .env("MX_CMUX_BIN", &cmux)
            .env("MX_REAL_CODEX", &provider)
            .env_remove("CODEX_HOME"),
    );
    success(
        Command::new("/bin/sh")
            .arg(f.fake.join("launch-script"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("CODEX_HOME", f.home.join("wrong-terminal-home")),
    );
    assert_eq!(
        fs::read_to_string(f.home.join("provider-home"))
            .unwrap()
            .trim(),
        "unset"
    );
}

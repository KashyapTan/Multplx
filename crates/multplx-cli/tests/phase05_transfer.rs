use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use multplx_domain::lifecycle::subagent_model::{
    ArtifactKind, AssignmentRole, TaskRecord, qualified_task_id, read_meta, write_meta,
};

fn write_record(path: &Path, record: &TaskRecord) {
    fs::write(
        path,
        write_meta("mode=deep-review\nyolo=off\n", record).unwrap(),
    )
    .unwrap();
}

#[test]
fn transfer_cli_retries_exactly_and_retains_one_authority_with_new_brief() {
    let temp = tempfile::tempdir().unwrap();
    let temp_root = fs::canonicalize(temp.path()).unwrap();
    let root = temp_root.join("root");
    let root_authority = temp_root.join("root-authority");
    let old = temp_root.join("old-owner");
    let successor = temp_root.join("successor-home");
    let project = temp_root.join("project");
    for home in [&root, &old, &successor] {
        fs::create_dir_all(home.join("state")).unwrap();
        fs::create_dir_all(home.join("data")).unwrap();
        fs::create_dir_all(home.join("config")).unwrap();
        fs::create_dir_all(home.join("projects")).unwrap();
    }
    fs::create_dir(&project).unwrap();
    let git = |arguments: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(&project)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--allow-empty",
        "-qm",
        "base",
    ]);
    let project_binding = multplx_domain::project_registry::bind_project(&old, &project).unwrap();
    fs::create_dir(&root_authority).unwrap();
    let root_id = format!("root-home:{}", root.display());
    let mut coordinator = TaskRecord::new(
        "coord".into(),
        AssignmentRole::SubOrchestrator,
        ArtifactKind::Coordination,
        true,
        root_id.clone(),
        root_id.clone(),
        root.to_string_lossy().into_owned(),
    );
    coordinator.private_home = true;
    coordinator.persistent_home = Some(successor.to_string_lossy().into_owned());
    coordinator.owner_state = Some(root_authority.to_string_lossy().into_owned());
    write_record(&root_authority.join("coord.meta"), &coordinator);

    let mut task = TaskRecord::new(
        "work".into(),
        AssignmentRole::Implementer,
        ArtifactKind::Implementation,
        false,
        root_id.clone(),
        root_id.clone(),
        old.to_string_lossy().into_owned(),
    );
    task.parent_id = Some(root_id.clone());
    task.parent_home = Some(root.to_string_lossy().into_owned());
    task.parent_state = Some(root_authority.to_string_lossy().into_owned());
    task.owning_coordinator = Some(root_id.clone());
    task.project = Some(project_binding);
    write_record(&old.join("state/work.meta"), &task);

    let destination_state = root_authority.clone();
    let arguments = [
        "task-transfer",
        "work",
        "--to",
        "coord",
        "--to-state",
        destination_state.to_str().unwrap(),
        "--request-id",
        "transfer-1",
        "--expected-generation",
        "1",
        "--reason",
        "route accepted work",
        "--scope",
        "new bounded scope",
        "--scope-reason",
        "owner changed the scope",
        "--accept",
        "successor can resume",
        "--source",
        "research.md",
    ];
    for expected_resumed in [false, true] {
        let output = Command::new(env!("CARGO_BIN_EXE_mx"))
            .args(arguments)
            .env("MX_HOME", &old)
            .env("MX_ROOT_OVERRIDE", &root)
            .env("MX_STATE_OVERRIDE", old.join("state"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(&format!("resumed={expected_resumed}"))
        );
    }

    let raw = fs::read_to_string(old.join("state/work.meta")).unwrap();
    let transferred = read_meta("work", &raw).unwrap();
    assert_eq!(transferred.attempt.as_ref().unwrap().generation, 2);
    let expected_owner = qualified_task_id(root.to_str().unwrap(), "coord");
    assert_eq!(
        transferred.owning_coordinator.as_deref(),
        Some(expected_owner.as_str())
    );
    assert!(!successor.join("state/work.meta").exists());
    let accepted_brief = fs::read_to_string(transferred.accepted_brief_path.unwrap()).unwrap();
    assert!(accepted_brief.contains("## Current scope\nnew bounded scope"));
    fs::create_dir_all(old.join("data/work")).unwrap();
    fs::write(old.join("data/work/brief.md"), &accepted_brief).unwrap();

    let routed_inspect = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args([
            "task-model",
            "inspect",
            "work",
            "--authority-state",
            old.join("state").to_str().unwrap(),
        ])
        .env("MX_HOME", &successor)
        .env("MX_ROOT_OVERRIDE", &root)
        .output()
        .unwrap();
    assert!(
        routed_inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&routed_inspect.stderr)
    );
    assert!(String::from_utf8_lossy(&routed_inspect.stdout).contains("new bounded scope"));

    let fake = temp_root.join("fake");
    fs::create_dir(&fake).unwrap();
    let tmux = fake.join("tmux");
    fs::write(
        &tmux,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  -V) printf 'tmux 3.6a\n' ;;
  has-session|new-session) exit 0 ;;
  new-window)
    rm -f "$MX_TMUX_STATE/dead"
    while [ "$#" -gt 0 ]; do
      if [ "$1" = -c ]; then shift; printf '%s\n' "$1" > "$MX_TMUX_STATE/cwd"; break; fi
      shift
    done
    printf '@1\n' ;;
  display-message)
    if [ -f "$MX_TMUX_STATE/dead" ]; then echo "can't find window:" >&2; exit 1; fi
    case "$*" in
      *pane_current_path*) cat "$MX_TMUX_STATE/cwd" ;;
      *pane_current_command*) printf 'codex\n' ;;
      *cursor_y*) printf '0\n' ;;
      *'#S'*) printf 'session\n' ;;
      *) printf '%%1\n' ;;
    esac ;;
  list-windows) [ -f "$MX_TMUX_STATE/dead" ] || printf 'work\n' ;;
  kill-window) : > "$MX_TMUX_STATE/dead" ;;
  capture-pane) printf '│ │\n' ;;
  send-keys) printf '%s\n' "$*" >> "$MX_TMUX_STATE/sent" ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755)).unwrap();
    let prior_meta = fs::read_to_string(old.join("state/work.meta")).unwrap();
    let mut live = read_meta("work", &prior_meta).unwrap();
    live.runtime.provider = "tmux".into();
    live.runtime.session_id = Some("session".into());
    live.runtime.endpoint = Some("session:work".into());
    let live_meta = write_meta(
        &format!("window=session:work\nbackend=tmux\nharness=codex\n{prior_meta}"),
        &live,
    )
    .unwrap();
    fs::write(old.join("state/work.meta"), live_meta).unwrap();
    let path = format!("{}:{}", fake.display(), std::env::var("PATH").unwrap());

    let routed_send = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args([
            "send",
            "work",
            "continue",
            "--authority-state",
            old.join("state").to_str().unwrap(),
        ])
        .env("MX_HOME", &successor)
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_TMUX_STATE", &fake)
        .env("MX_SEND_SLEEP", "0")
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(
        routed_send.status.success(),
        "{}",
        String::from_utf8_lossy(&routed_send.stderr)
    );
    assert!(
        fs::read_to_string(fake.join("sent"))
            .unwrap()
            .contains("continue")
    );

    let stopped = read_meta(
        "work",
        &fs::read_to_string(old.join("state/work.meta")).unwrap(),
    )
    .unwrap();
    assert!(stopped.runtime.endpoint.is_some());

    let retained_attempt = stopped.attempt.as_ref().unwrap().id.clone();
    let retained_brief = stopped.accepted_brief_path.clone();
    let retained_digest = stopped.accepted_brief_digest.clone();
    let retained_transfers = stopped.transfers.clone();
    let old_state = old.join("state");
    let spawn_args = [
        "spawn",
        "work",
        project.to_str().unwrap(),
        "--backend",
        "tmux",
        "--harness",
        "codex",
        "--replace-attempt",
        &retained_attempt,
        "--authority-state",
        old_state.to_str().unwrap(),
    ];
    let routed_spawn = |caller: &Path, authority: Option<&Path>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
        let mut routed = spawn_args.map(str::to_owned);
        if let Some(authority) = authority {
            routed[10] = authority.to_string_lossy().into_owned();
        }
        command
            .args(&routed)
            .env("MX_HOME", caller)
            .env("MX_ROOT_OVERRIDE", &root)
            .env_remove("MX_TASK_ID")
            .env("MX_TMUX_STATE", &fake)
            .env("MX_HEADROOM_SKIP_QUEUE", "1")
            .env("MX_HEADROOM_CPU_COUNT", "8")
            .env("MX_HEADROOM_LOAD1", "0")
            .env("MX_HEADROOM_MEM_AVAILABLE_BYTES", "17179869184")
            .env("MX_HEADROOM_API_CAPACITY", "8")
            .env("MX_HEADROOM_IN_USE", "0")
            .env("PATH", &path)
            .output()
            .unwrap()
    };
    let wrong_caller = routed_spawn(&root, None);
    assert!(!wrong_caller.status.success());
    assert!(String::from_utf8_lossy(&wrong_caller.stderr).contains("owning coordinator home"));
    let missing_authority = temp_root.join("missing-authority");
    let invalid_authority = routed_spawn(&successor, Some(&missing_authority));
    assert!(!invalid_authority.status.success());
    assert!(String::from_utf8_lossy(&invalid_authority.stderr).contains("routed authority state"));
    assert!(!old.join("state/.spawn-work.intent").exists());

    let spawned = routed_spawn(&successor, None);
    assert!(
        spawned.status.success(),
        "{}",
        String::from_utf8_lossy(&spawned.stderr)
    );
    assert!(!fake.join("dead").exists());
    let resumed = read_meta(
        "work",
        &fs::read_to_string(old.join("state/work.meta")).unwrap(),
    )
    .unwrap();
    assert_eq!(resumed.attempt.as_ref().unwrap().generation, 3);
    assert_eq!(resumed.prior_attempts.len(), 2);
    assert_eq!(resumed.accepted_brief_path, retained_brief);
    assert_eq!(resumed.accepted_brief_digest, retained_digest);
    assert_eq!(resumed.transfers, retained_transfers);
    assert_eq!(resumed.owner_home.as_deref(), Some(old.to_str().unwrap()));
    assert_eq!(
        resumed.owner_state.as_deref(),
        Some(old_state.to_str().unwrap())
    );
    let allocation = resumed.allocation.as_ref().expect("resumed allocation");
    assert_eq!(allocation.task_id, "work");
    assert_eq!(allocation.attempt_id, resumed.attempt.as_ref().unwrap().id);
    let store = multplx_domain::lifecycle::worktree::Store::new(
        resumed.project.as_ref().expect("project binding"),
    )
    .unwrap();
    assert_eq!(
        store.inspect(&allocation.allocation_id).unwrap().owner_home,
        fs::canonicalize(&old).unwrap()
    );
    assert!(!successor.join("state/work.meta").exists());
    assert!(!successor.join("state/.spawn-work.intent").exists());

    let routed_transfer = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args([
            "task-transfer",
            "work",
            "--to",
            "root",
            "--request-id",
            "transfer-2",
            "--expected-generation",
            "3",
            "--reason",
            "return completed route",
            "--to-state",
            root_authority.to_str().unwrap(),
            "--authority-state",
            old.join("state").to_str().unwrap(),
        ])
        .env("MX_HOME", &successor)
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_TMUX_STATE", &fake)
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(
        routed_transfer.status.success(),
        "{}",
        String::from_utf8_lossy(&routed_transfer.stderr)
    );
    let returned = read_meta(
        "work",
        &fs::read_to_string(old.join("state/work.meta")).unwrap(),
    )
    .unwrap();
    assert!(fake.join("dead").is_file());
    assert!(returned.runtime.endpoint.is_none());
    assert!(returned.runtime.session_id.is_none());
    assert_eq!(returned.attempt.as_ref().unwrap().generation, 4);
    assert_eq!(
        returned.owning_coordinator.as_deref(),
        Some(root_id.as_str())
    );
    assert!(!root_authority.join("work.meta").exists());
}

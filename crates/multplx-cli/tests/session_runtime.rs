use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(command: &mut Command) -> Output {
    command.output().expect("run mx")
}

fn mx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mx"))
}

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent");
    fs::write(path, body).expect("write script");
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("mode");
}

fn fake_tools(root: &Path) -> PathBuf {
    let path = root.join("tools");
    executable(&path.join("jq"), "#!/bin/sh\nexit 0\n");
    executable(&path.join("node"), "#!/bin/sh\nexit 0\n");
    path
}

fn snapshot_fixture(home: &Path) -> serde_json::Value {
    serde_json::json!({
        "schema": "mx-system-snapshot.v1",
        "generated": "2026-08-12T12:00:00Z",
        "mx_home": home,
        "roots": {
            "mx_root": home,
            "state": home.join("state"),
            "data": home.join("data"),
            "config": home.join("config"),
            "projects": home.join("projects")
        },
        "backlog": {
            "path": home.join("data/backlog.md"),
            "present": true,
            "records": [
                {"state":"queued","id":"next","title":"Next task","repo":"demo","kind":"delivery","blocked_by":"hold-1","blocked_reason":"approval","pr_url":null,"report_path":null,"local_note":null},
                {"state":"done","id":"landed","title":"Landed task","repo":"demo","kind":"delivery","blocked_by":null,"pr_url":"https://example.invalid/pr/1","report_path":null,"local_note":null}
            ]
        },
        "tasks": [
            {
                "id":"actor-1","kind":"delivery","project":"demo","backend":"tmux",
                "current_state":{"state":"working","source":"native"},
                "endpoint":{"target":"pane-1","exists":true,"agent_alive":"not_checked"},
                "pr":{"url":null},
                "paths":{"home":{"path":null,"present":false},"worktree":{"path":"/work/actor-1","present":true},"report":{"path":"/reports/actor-1.md","present":true}},
                "actions":{"watch":"bin/mx-peek actor-1","send":null},
                "backlog":{"state":"in_flight","id":"actor-1","title":"Actor","repo":"demo","kind":"delivery"}
            },
            {
                "id":"daemon-1","kind":"daemon","project":"demo","backend":"herdr",
                "current_state":{"state":"paused","source":"report"},
                "endpoint":{"target":"pane-2","exists":false,"agent_alive":"dead"},
                "pr":{"url":"https://example.invalid/pr/2"},
                "paths":{"home":{"path":"/homes/daemon-1","present":false},"worktree":{"path":null,"present":false},"report":{"path":null,"present":false}},
                "actions":{"watch":"bin/mx-peek daemon-1","send":"bin/mx-send daemon-1"},
                "backlog":null
            }
        ],
        "main_inventory":{"valid":true,"reason":null,"orphan_in_flight":[],"unstructured_current_count":0},
        "scout_reports":[],
        "watcher":{"lock_present":false,"pid":null,"identity_verified":false,"alive":false,"beacon_age_secs":null,"stale":false,"afk":false},
        "wake_queue":{"depth":0,"oldest_age_secs":null},
        "dispatch_queue":{"depth":0,"records":[],"available":true,"reason":null},
        "headroom":null,
        "headroom_reason":"not requested",
        "vplan_reviews":{"records":[]},
        "later_feeds":{
            "gate_runs":{"supported":true,"available":true,"records":[]},
            "workflow_runs":{"supported":true,"available":true,"records":[]},
            "deliveries":{"supported":true,"available":true,"records":[]},
            "upstream_drift":{},
            "doctor":{"available":true},
            "timeline":{"available":true}
        },
        "daemon_current":{"registry":{},"records":[],"total_registered":0,"total":0,"shown":0,"truncated":0},
        "daemon_landed":{"records":[],"truncated":[],"unreadable":[],"partial":[]},
        "daemon_guidance":{"note":"No registered daemons."},
        "future_additive_field":{"safe":"ignored"}
    })
}

#[test]
fn compatibility_dispatch_pins_the_complete_composition_to_legacy() {
    let temp = tempfile::tempdir().expect("tempdir");
    executable(
        &temp.path().join("bin/mx-session-start.sh"),
        "#!/bin/sh\nprintf '%s|%s\\n' \"${MX_SESSION_IMPLEMENTATION:-unset}\" \"${1:-}\"\n",
    );
    let output = run(mx()
        .env("MX_ROOT_OVERRIDE", temp.path())
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args(["session", "mx-session-start.sh", "probe"]));
    assert!(output.status.success());
    assert_eq!(output.stdout, b"legacy|probe\n");
}

#[test]
fn native_supervision_renderer_is_deterministic_and_path_aware() {
    let temp = tempfile::tempdir().expect("tempdir");
    let protocols = temp.path().join("docs/supervision-protocols");
    fs::create_dir_all(&protocols).expect("protocols");
    fs::write(
        protocols.join("pi.md"),
        "Mode: Pi. __MX_PI_TURNEND_EXT__ __MX_PI_EXT__\n",
    )
    .expect("pi protocol");
    fs::write(protocols.join("unknown.md"), "Mode: Unknown.\n").expect("unknown protocol");
    let output = run(mx()
        .env("MX_ROOT_OVERRIDE", "/logical/home")
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args([
            "session",
            "mx-supervision-instructions.sh",
            "--harness",
            "pi",
            "--read-only",
            "1",
        ]));
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("UTF-8");
    assert!(text.contains("primary harness: pi"));
    assert!(text.contains("- Lock: read-only"));
    assert!(text.contains("/logical/home/.pi/extensions/mx-primary-turnend-guard.ts"));
    assert!(text.contains("/logical/home/.pi/extensions/mx-primary-pi-watch.ts"));
}

#[test]
fn unknown_and_missing_session_entries_fail_before_execution() {
    let unknown = run(mx().args(["session", "not-an-entry"]));
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown session entry point"));

    let temp = tempfile::tempdir().expect("tempdir");
    fs::create_dir(temp.path().join("bin")).expect("bin");
    let missing = run(mx()
        .env("MX_ROOT_OVERRIDE", temp.path())
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args(["session", "mx-doctor.sh"]));
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("compatibility body is unavailable"));
}

#[test]
fn native_nudge_and_supervision_cover_scope_lock_and_usage_edges() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let state = root.join("state");
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir(&state).expect("state");
    fs::write(root.join("AGENTS.md"), "# contract\n").expect("contract");
    fs::write(root.join(".mx-daemon-home"), "daemon-1\n").expect("marker");

    let nudge = run(mx()
        .env_remove("DEEP_REVIEW_GATE")
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", &root)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["session", "mx-sessionstart-nudge.sh"]));
    assert!(nudge.status.success());
    assert!(String::from_utf8_lossy(&nudge.stdout).contains(
        "Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions."
    ));

    fs::write(state.join(".lock"), format!("{}\n", std::process::id())).expect("lock");
    let locked = run(mx()
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", &root)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["session", "mx-sessionstart-nudge.sh"]));
    assert!(locked.status.success());
    assert!(locked.stdout.is_empty());

    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let help = run(mx().env("MX_RUST_SOURCE_ROOT", &source).args([
        "session",
        "mx-supervision-instructions.sh",
        "--help",
    ]));
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
    let invalid = run(mx().env("MX_RUST_SOURCE_ROOT", &source).args([
        "session",
        "mx-supervision-instructions.sh",
        "--unexpected",
    ]));
    assert_eq!(invalid.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("unknown argument"));
    let repair = run(mx()
        .env("MX_RUST_SOURCE_ROOT", &source)
        .env("MX_CODEX_WATCH_CHECKPOINT", "45")
        .args([
            "session",
            "mx-supervision-instructions.sh",
            "--harness",
            "codex",
            "--repair-line",
            "--queue-pending",
            "1",
        ]));
    assert!(repair.status.success());
    assert_eq!(
        String::from_utf8_lossy(&repair.stdout),
        "After draining queued wakes, repair missing watcher supervision with a foreground checkpoint: bin/mx-watch-checkpoint.sh --seconds 45.\n"
    );
    for (harness, extra, expected) in [
        (
            "claude",
            vec!["--read-only", "1"],
            "Watcher repair belongs to the session holding the system lock",
        ),
        (
            "pi",
            vec!["--afk", "1"],
            "Away mode owns watcher supervision",
        ),
        (
            "unknown-harness",
            Vec::new(),
            "according to the session-start block",
        ),
    ] {
        let mut arguments = vec![
            "session",
            "mx-supervision-instructions.sh",
            "--harness",
            harness,
            "--repair-line",
        ];
        arguments.extend(extra);
        let output = run(mx().env("MX_RUST_SOURCE_ROOT", &source).args(arguments));
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
    }
}

#[test]
fn native_system_view_parses_one_snapshot_and_preserves_json_mode() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    fs::create_dir_all(root.join("bin")).expect("bin");
    let snapshot = root.join("snapshot.json");
    let bytes = serde_json::to_vec(&snapshot_fixture(&root)).expect("snapshot JSON");
    fs::write(&snapshot, &bytes).expect("snapshot");
    executable(
        &root.join("bin/mx-system-snapshot.sh"),
        "#!/bin/sh\nexec /bin/cat \"$MX_SNAPSHOT_FIXTURE\"\n",
    );
    let tools = fake_tools(&root);

    let view = run(mx()
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_RUST_SOURCE_ROOT", &root)
        .env("MX_SNAPSHOT_FIXTURE", &snapshot)
        .args(["session", "mx-system-view.sh"]));
    assert!(
        view.status.success(),
        "{}",
        String::from_utf8_lossy(&view.stderr)
    );
    let text = String::from_utf8(view.stdout).expect("UTF-8");
    assert!(text.contains("| actor-1 | working / native | delivery | demo | tmux | present |"));
    assert!(
        text.contains("| daemon-1 | paused / report | daemon | demo | herdr | absent / dead |")
    );
    assert!(text.contains("| next | Next task | demo | delivery | hold-1 - approval | - |"));
    assert!(
        text.contains(
            "| landed | Landed task | demo | delivery | - | https://example.invalid/pr/1 |"
        )
    );
    assert!(text.ends_with("## Daemons\nNo registered daemons.\n"));

    let json = run(mx()
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_RUST_SOURCE_ROOT", &root)
        .env("MX_SNAPSHOT_FIXTURE", &snapshot)
        .args(["session", "mx-system-view.sh", "--json"]));
    assert!(json.status.success());
    assert_eq!(json.stdout, bytes);

    let help = run(mx().args(["session", "mx-system-view.sh", "--help"]));
    assert!(help.status.success());
    let usage = run(mx().args(["session", "mx-system-view.sh", "--bad"]));
    assert_eq!(usage.status.code(), Some(2));

    fs::write(&snapshot, "{}\n").expect("invalid snapshot");
    let invalid = run(mx()
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_RUST_SOURCE_ROOT", &root)
        .env("MX_SNAPSHOT_FIXTURE", &snapshot)
        .args(["session", "mx-system-view.sh"]));
    assert_eq!(invalid.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&invalid.stderr),
        "mx-system-view: invalid canonical snapshot\n"
    );

    let empty_tools = root.join("empty-tools");
    fs::create_dir(&empty_tools).expect("empty tools");
    let no_jq = run(mx()
        .env("PATH", &empty_tools)
        .args(["session", "mx-system-view.sh"]));
    assert_eq!(no_jq.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&no_jq.stderr),
        "mx-system-view: jq not found\n"
    );

    let mut empty = snapshot_fixture(&root);
    empty["tasks"] = serde_json::json!([]);
    empty["backlog"]["records"] = serde_json::json!([]);
    fs::write(&snapshot, serde_json::to_vec(&empty).expect("empty JSON")).expect("empty snapshot");
    let empty_view = run(mx()
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_RUST_SOURCE_ROOT", &root)
        .env("MX_SNAPSHOT_FIXTURE", &snapshot)
        .args(["session", "mx-system-view.sh"]));
    assert!(empty_view.status.success());
    let empty_text = String::from_utf8_lossy(&empty_view.stdout);
    assert!(empty_text.contains("No live task metadata found."));
    assert!(empty_text.contains("No queued backlog records found."));
    assert!(empty_text.contains("No done backlog records found."));

    let mut edges = snapshot_fixture(&root);
    edges["tasks"][0]["endpoint"]["exists"] = serde_json::Value::Null;
    edges["tasks"][0]["paths"]["home"] =
        serde_json::json!({"path":"/homes/actor-1","present":true});
    let mut absent_worktree = edges["tasks"][0].clone();
    absent_worktree["id"] = serde_json::json!("actor-2");
    absent_worktree["paths"]["home"] = serde_json::json!({"path":null,"present":false});
    absent_worktree["paths"]["worktree"] =
        serde_json::json!({"path":"/work/actor-2","present":false});
    let mut no_path = absent_worktree.clone();
    no_path["id"] = serde_json::json!("actor-3");
    no_path["paths"]["worktree"] = serde_json::json!({"path":null,"present":false});
    edges["tasks"]
        .as_array_mut()
        .expect("tasks")
        .extend([absent_worktree, no_path]);
    edges["backlog"]["records"][0]["blocked_reason"] = serde_json::Value::Null;
    fs::write(&snapshot, serde_json::to_vec(&edges).expect("edge JSON")).expect("edge snapshot");
    let edge_view = run(mx()
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_RUST_SOURCE_ROOT", &root)
        .env("MX_SNAPSHOT_FIXTURE", &snapshot)
        .args(["session", "mx-system-view.sh"]));
    assert!(edge_view.status.success());
    let edge_text = String::from_utf8_lossy(&edge_view.stdout);
    assert!(
        edge_text.contains("| actor-1 | working / native | delivery | demo | tmux | unknown |")
    );
    assert!(edge_text.contains("/work/actor-2 (absent)"));
    assert!(edge_text.contains("| next | Next task | demo | delivery | hold-1 | - |"));
}

#[test]
fn native_timeline_covers_filters_malformed_rows_and_private_html() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let state = root.join("state");
    let data = root.join("data");
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir(&state).expect("state");
    fs::create_dir(&data).expect("data");
    let fixture = include_str!("../../../tests/fixtures/timeline.journal.jsonl");
    fs::write(state.join("timeline-fixture.journal"), fixture).expect("journal");
    executable(&root.join("bin/mx-vplan.sh"), "#!/bin/sh\nexit 0\n");
    let tools = fake_tools(&root);
    let command = || {
        let mut command = mx();
        command
            .env("PATH", &tools)
            .env("MX_ROOT_OVERRIDE", &root)
            .env("MX_HOME", &root)
            .env("MX_STATE_OVERRIDE", &state)
            .env("MX_DATA_OVERRIDE", &data)
            .env("MX_RUST_SOURCE_ROOT", &root)
            .args(["session", "mx-timeline.sh", "timeline-fixture"]);
        command
    };

    let text = run(&mut command());
    assert!(text.status.success());
    assert_eq!(
        String::from_utf8_lossy(&text.stdout),
        include_str!("../../../tests/fixtures/timeline.golden")
    );
    let filtered = run(command().args(["--event", "gate.*", "--json"]));
    assert!(filtered.status.success());
    assert_eq!(String::from_utf8_lossy(&filtered.stdout).lines().count(), 1);
    assert!(String::from_utf8_lossy(&filtered.stdout).contains("gate.step.finished"));
    let since = run(command().args(["--since", "2026-07-30T12:06:00Z"]));
    assert!(since.status.success());
    assert_eq!(String::from_utf8_lossy(&since.stdout).lines().count(), 3);
    let duration = run(command()
        .env("MX_TIMELINE_NOW_MS", "1785413700000")
        .args(["--since", "5m"]));
    assert!(duration.status.success());
    assert_eq!(String::from_utf8_lossy(&duration.stdout).lines().count(), 2);

    fs::write(
        state.join("timeline-fixture.journal"),
        format!("{fixture}{{broken\n"),
    )
    .expect("malformed journal");
    let malformed = run(&mut command());
    assert!(malformed.status.success());
    assert_eq!(
        String::from_utf8_lossy(&malformed.stderr),
        "mx-timeline: skipped 1 malformed journal line(s)\n"
    );

    let html = run(command().args(["--html"]));
    assert!(html.status.success());
    let artifact = PathBuf::from(String::from_utf8_lossy(&html.stdout).trim());
    assert!(
        fs::read_to_string(&artifact)
            .expect("artifact")
            .contains("<!DOCTYPE html>")
    );
    assert_eq!(
        fs::metadata(&artifact)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let invalid = run(command().args(["--since", "never"]));
    assert_eq!(invalid.status.code(), Some(1));
    let conflict = run(command().args(["--json", "--html"]));
    assert_eq!(conflict.status.code(), Some(1));
    let unknown = run(command().args(["--unknown"]));
    assert_eq!(unknown.status.code(), Some(1));
    let missing_since = run(command().args(["--since"]));
    assert_eq!(missing_since.status.code(), Some(1));
    let help = run(mx().args(["session", "mx-timeline.sh", "--help"]));
    assert!(help.status.success());
    let no_id = run(mx().args(["session", "mx-timeline.sh"]));
    assert_eq!(no_id.status.code(), Some(2));
    let missing = run(mx()
        .env("PATH", &tools)
        .env("MX_HOME", &root)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["session", "mx-timeline.sh", "absent"]));
    assert_eq!(missing.status.code(), Some(1));

    executable(&root.join("bin/mx-vplan.sh"), "#!/bin/sh\nexit 1\n");
    let invalid_vplan = run(command().args(["--html"]));
    assert_eq!(invalid_vplan.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid_vplan.stderr).contains("unavailable or invalid"));
    fs::remove_file(root.join("bin/mx-vplan.sh")).expect("remove vplan");
    let missing_vplan = run(command().args(["--html"]));
    assert_eq!(missing_vplan.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing_vplan.stderr).contains("vplan module is unavailable"));

    let jq_only = root.join("jq-only");
    fs::create_dir(&jq_only).expect("jq-only");
    executable(&jq_only.join("jq"), "#!/bin/sh\nexit 0\n");
    let no_node = run(mx()
        .env("PATH", &jq_only)
        .env("MX_HOME", &root)
        .env("MX_STATE_OVERRIDE", &state)
        .args(["session", "mx-timeline.sh", "timeline-fixture"]));
    assert_eq!(no_node.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&no_node.stderr).contains("node is required"));
}

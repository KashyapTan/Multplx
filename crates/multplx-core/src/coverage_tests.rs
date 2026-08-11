use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::classification::{
    Heuristic, NativeState, RunStep, is_maintainer_relevant, last_status_line, open_activities,
    render_open_statuses, resolve_signal, status_line_note, status_line_verb,
};
use crate::error::Result;
use crate::filesystem::{
    append_single_write, atomic_replace, cleanup_regular, mode, read_bounded_regular,
};
use crate::identifiers::{PathComponent, Sha256Digest, TaskId};
use crate::journal::{JournalEvent, JournalWriter};
use crate::locks::{DirectoryLock, epoch_seconds};
use crate::paths::ExistingRoot;
use crate::probe::{
    Backend, SystemToolProbe, ToolProbe, ToolRecord, bootstrap_tangle, install_command,
    manual_install_url, tangle_record, tool_records,
};
use crate::process::{AncestryRow, OwnedChild, ProcessIdentity, ProcessProbe, path_age};
use crate::supervision::inspect;
use crate::supervisor_target::{SupervisorEnvironment, backend, target};
use crate::transition::{TransitionAction, policy};
use crate::wake::{
    AnnotationLimits, WakeKind, WakeQueue, WakeRecord, latest_event, render_annotations,
    render_identity, status_key_map,
};

#[derive(Clone, Copy)]
struct CurrentProcess;

impl ProcessProbe for CurrentProcess {
    fn is_alive(&self, pid: u32) -> bool {
        pid == std::process::id()
    }

    fn identity(&self, pid: u32) -> Result<ProcessIdentity> {
        Ok(ProcessIdentity {
            pid,
            marker: format!("identity-{pid}"),
        })
    }

    fn ancestry_row(&self, pid: u32) -> Result<AncestryRow> {
        Ok(AncestryRow {
            parent_pid: 1,
            command: if pid == std::process::id() {
                "codex".to_owned()
            } else {
                "other".to_owned()
            },
            arguments: "codex fixture".to_owned(),
        })
    }
}

#[test]
fn identifier_path_and_filesystem_public_contracts_cover_success_and_refusal() {
    let task = TaskId::parse("task-1").expect("task");
    let component = PathComponent::parse("record").expect("component");
    let digest = Sha256Digest::parse("a".repeat(64)).expect("digest");
    assert_eq!(task.as_str(), "task-1");
    assert_eq!(component.as_str(), "record");
    assert_eq!(digest.as_str(), "a".repeat(64));

    let temp = tempfile::tempdir().expect("tempdir");
    let root = ExistingRoot::open(temp.path()).expect("root");
    assert_eq!(
        root.as_path(),
        temp.path().canonicalize().expect("canonical")
    );
    let file = root.join(&component);
    atomic_replace(&file, b"private\n", 0o640).expect("replace");
    assert_eq!(root.existing_descendant(&file).expect("descendant"), file);
    assert!(root.existing_descendant(root.as_path()).is_err());
    assert_eq!(read_bounded_regular(&file, 8).expect("read"), b"private\n");
    assert!(read_bounded_regular(&file, 7).is_err());
    assert_eq!(mode(&file).expect("mode"), 0o640);
    append_single_write(&file, b"next\n", 0o600).expect("append");
    assert_eq!(fs::read(&file).expect("bytes"), b"private\nnext\n");
    cleanup_regular(&file).expect("cleanup");
    cleanup_regular(&file).expect("missing cleanup");
    fs::create_dir(&file).expect("directory");
    assert!(cleanup_regular(&file).is_err());

    let outside = temp.path().join("outside");
    let link = temp.path().join("root-link");
    fs::create_dir(&outside).expect("outside");
    symlink(&outside, &link).expect("root link");
    assert!(ExistingRoot::open(&link).is_err());
    assert!(root.absent_descendant(Path::new("/absolute")).is_err());
}

#[test]
fn classification_public_contracts_cover_every_vocabulary_and_fold() {
    for (input, expected) in [
        ("idle", NativeState::Idle),
        ("working", NativeState::Working),
        ("blocked", NativeState::Blocked),
        ("done", NativeState::Done),
        ("other", NativeState::Unknown),
    ] {
        assert_eq!(NativeState::parse(input), expected);
    }
    for input in [
        "working", "parked", "done", "blocked", "paused", "failed", "other",
    ] {
        let parsed = RunStep::parse(input);
        assert_eq!(parsed == RunStep::Unknown, input == "other");
    }
    assert_eq!(Heuristic::parse("busy"), Heuristic::Busy);
    assert_eq!(Heuristic::parse("idle"), Heuristic::Idle);
    assert_eq!(Heuristic::parse("other"), Heuristic::Unknown);
    assert_eq!(
        resolve_signal(
            NativeState::Unknown,
            RunStep::Unknown,
            "",
            Heuristic::Unknown,
            "paused",
        ),
        "none"
    );
    assert_eq!(status_line_verb("blocked [key=x]: why"), "blocked");
    assert_eq!(status_line_note("blocked [key=x]: why"), "why");
    let open = open_activities(
        "working [key=a]: first\npaused [key=b]: wait\ndone [key=a]: yes\nblocked [key=b]: no\nworking [key=bad/key]: ignored\n",
        "paused",
        "resolved",
        "maintainer-held",
    );
    assert!(open.is_empty());
    assert_eq!(render_open_statuses(&open), "");
    assert!(!is_maintainer_relevant("", None, "paused").expect("empty"));
    assert!(!is_maintainer_relevant("working: yes", None, "paused").expect("working"));
    assert!(is_maintainer_relevant("DONE: ready", Some("done:"), "paused").expect("regex"));
    assert!(is_maintainer_relevant("x", Some("["), "paused").is_err());

    let temp = tempfile::tempdir().expect("tempdir");
    let status = temp.path().join("task.status");
    assert_eq!(last_status_line(&status, 100).expect("absent"), None);
    fs::write(&status, b"working: one\n\nblocked: two\n").expect("status");
    assert_eq!(
        last_status_line(&status, 100).expect("last").as_deref(),
        Some("blocked: two")
    );
    assert!(last_status_line(&status, 2).is_err());
    fs::write(&status, [0xff]).expect("invalid UTF-8");
    assert!(last_status_line(&status, 100).is_err());
}

#[test]
fn journal_vocabulary_validation_and_accessors_are_covered() {
    let tokens = [
        "task.spawned",
        "status.reported",
        "status.classified",
        "gate.step.started",
        "gate.step.finished",
        "hold.opened",
        "hold.resolved",
        "workflow.stage.entered",
        "workflow.stage.gated",
        "delivery.queued",
        "delivery.pushed",
        "delivery.pr_opened",
    ];
    for token in tokens {
        assert_eq!(JournalEvent::parse(token).expect("event").as_str(), token);
    }
    assert!(JournalEvent::parse("unknown").is_err());
    let temp = tempfile::tempdir().expect("tempdir");
    let writer = JournalWriter::new(temp.path());
    let task = TaskId::parse("journal").expect("task");
    assert_eq!(writer.state(), temp.path());
    assert_eq!(writer.path(&task), temp.path().join("journal.journal"));
    assert!(
        writer
            .emit(
                &task,
                JournalEvent::TaskSpawned,
                &json!([]),
                "source",
                "2026-08-10T12:00:00Z",
            )
            .is_err()
    );
    for (source, timestamp) in [
        ("bad source", "2026-08-10T12:00:00Z"),
        ("source", "2026-08-10 12:00:00Z"),
    ] {
        assert!(
            writer
                .emit(
                    &task,
                    JournalEvent::TaskSpawned,
                    &json!({}),
                    source,
                    timestamp,
                )
                .is_err()
        );
    }
}

struct MissingTools {
    available: HashSet<String>,
    lease: bool,
}

impl ToolProbe for MissingTools {
    fn available(&self, tool: &str) -> bool {
        self.available.contains(tool)
    }

    fn treehouse_supports_lease(&self) -> bool {
        self.lease
    }
}

#[test]
fn probe_vocabulary_order_and_rendering_are_covered() {
    assert_eq!(
        Backend::parse("tmux").expect("tmux").required_tools(),
        &["tmux"]
    );
    assert_eq!(
        Backend::parse("herdr").expect("herdr").required_tools(),
        &["herdr", "jq"]
    );
    assert_eq!(
        Backend::parse("cmux").expect("cmux").required_tools(),
        &["cmux", "jq"]
    );
    assert!(Backend::parse("bad").is_err());
    let probe = MissingTools {
        available: HashSet::from(["treehouse".to_owned()]),
        lease: false,
    };
    let records = tool_records("herdr", &probe);
    assert!(matches!(records[0], ToolRecord::MissingManual { .. }));
    assert!(
        records.iter().any(
            |record| matches!(record, ToolRecord::Missing { tool, .. } if tool == "treehouse")
        )
    );
    for record in records {
        assert!(record.render_record().ends_with('\n'));
        assert!(record.render_bootstrap().ends_with('\n'));
    }
    let invalid = tool_records("bad", &probe);
    assert!(matches!(invalid[0], ToolRecord::BackendInvalid { .. }));
    assert!(install_command("curl").is_some());
    assert!(install_command("unknown").is_none());
    assert_eq!(manual_install_url("herdr"), Some("https://herdr.dev"));
    assert_eq!(manual_install_url("tmux"), None);
    assert!(SystemToolProbe.available("/bin/sh"));
    assert!(!SystemToolProbe.available("/definitely/missing/mx-tool"));
}

fn git_fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let status = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(temp.path())
        .status()
        .expect("git init");
    assert!(status.success());
    fs::write(temp.path().join("fixture"), b"fixture\n").expect("fixture");
    let status = Command::new("git")
        .args(["-C", temp.path().to_str().expect("UTF-8"), "add", "."])
        .status()
        .expect("git add");
    assert!(status.success());
    let status = Command::new("git")
        .args([
            "-C",
            temp.path().to_str().expect("UTF-8"),
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "fixture",
        ])
        .status()
        .expect("git commit");
    assert!(status.success());
    temp
}

#[test]
fn tangle_supervisor_primary_and_supervision_public_paths_are_covered() {
    let repo = git_fixture();
    assert_eq!(tangle_record(repo.path()).expect("healthy"), None);
    assert_eq!(bootstrap_tangle(repo.path(), true).expect("healthy"), None);
    assert!(
        Command::new("git")
            .args([
                "-C",
                repo.path().to_str().expect("UTF-8"),
                "checkout",
                "-b",
                "feature"
            ])
            .status()
            .expect("checkout")
            .success()
    );
    assert_eq!(
        tangle_record(repo.path()).expect("tangle"),
        Some(("feature".to_owned(), "main".to_owned()))
    );
    let rendered = bootstrap_tangle(repo.path(), true)
        .expect("bootstrap")
        .expect("tangle text");
    assert!(rendered.contains("TANGLE:"));
    assert!(rendered.contains("read-only"));

    let explicit = SupervisorEnvironment {
        target: Some("target".to_owned()),
        backend: Some("cmux".to_owned()),
        ..SupervisorEnvironment::default()
    };
    assert_eq!(target(&explicit).value, "target");
    assert_eq!(backend(&explicit).value, "cmux");
    let herdr = SupervisorEnvironment {
        herdr_environment: true,
        herdr_pane_id: Some("pane".to_owned()),
        ..SupervisorEnvironment::default()
    };
    assert_eq!(target(&herdr).value, "default:pane");
    assert_eq!(backend(&herdr).value, "herdr");
    let fallback = SupervisorEnvironment::default();
    assert!(!target(&fallback).detected);
    assert!(!backend(&fallback).detected);

    let state = repo.path().join("state");
    fs::create_dir(&state).expect("state");
    fs::write(state.join("task.meta"), b"id=task\n").expect("meta");
    fs::write(state.join(".last-watcher-beat"), b"").expect("beat");
    fs::write(state.join(".wake-queue"), b"wake\n").expect("queue");
    let status = inspect(&state, Duration::from_secs(300), SystemTime::now());
    assert!(status.watcher_fresh);
    assert!(status.queue_pending);
    assert!(!status.unhealthy());
}

#[test]
fn lock_process_and_child_public_lifecycle_paths_are_covered() {
    let temp = tempfile::tempdir().expect("tempdir");
    let lock_path = temp.path().join("lock");
    let lock =
        DirectoryLock::acquire_wait(&lock_path, &CurrentProcess, Duration::ZERO).expect("lock");
    assert_eq!(lock.path(), lock_path);
    lock.publish_metadata("mx-home", b"/tmp/home\n")
        .expect("metadata");
    assert!(lock.publish_metadata("unknown", b"x").is_err());
    lock.release().expect("release");
    assert!(!lock_path.exists());
    assert_eq!(epoch_seconds(UNIX_EPOCH + Duration::from_secs(42)), 42);

    let file = temp.path().join("age");
    fs::write(&file, b"").expect("age file");
    assert!(path_age(&file, u64::MAX).expect("age") > 0);
    assert!(path_age(temp.path().join("missing"), 1).is_err());

    let mut command = Command::new("/bin/sh");
    command.args(["-c", "exit 7"]);
    let child = OwnedChild::spawn(&mut command).expect("spawn");
    assert!(child.id() > 1);
    assert_eq!(child.wait().expect("wait").code(), Some(7));
    let mut missing = Command::new("/definitely/missing/mx-child");
    assert!(OwnedChild::spawn(&mut missing).is_err());
}

#[test]
fn wake_parsing_annotation_and_successful_drain_paths_are_covered() {
    for kind in [
        WakeKind::Signal,
        WakeKind::Stale,
        WakeKind::Check,
        WakeKind::Heartbeat,
    ] {
        assert_eq!(WakeKind::parse(kind.as_str()).expect("kind"), kind);
    }
    assert!(WakeKind::parse("unknown").is_err());
    for malformed in ["", "a\tb", "x\t1\tsignal\tk\tp", "1\tx\tsignal\tk\tp"] {
        assert!(WakeRecord::parse(malformed).is_err());
    }
    assert!(WakeRecord::parse(&"x".repeat(70_000)).is_err());
    assert!(status_key_map("task.other").is_err());

    let temp = tempfile::tempdir().expect("tempdir");
    let status = temp.path().join("task.status");
    fs::write(&status, b"working: old\nblocked:\tlatest\r\n").expect("status");
    let event = latest_event(&status, 1024).expect("event");
    assert_eq!(event.line, "blocked: latest");
    assert!(!event.truncated);
    let truncated = latest_event(&status, 8).expect("truncated event");
    assert!(truncated.truncated);
    assert!(latest_event(&status, 0).is_err());
    assert_eq!(
        render_identity(&ProcessIdentity {
            pid: 1,
            marker: "marker".to_owned(),
        }),
        "marker\n"
    );

    let records = vec![
        WakeRecord::new(1, 1, WakeKind::Signal, "task.turn-ended", "one"),
        WakeRecord::new(2, 2, WakeKind::Signal, "task.status", "two"),
    ];
    let annotation = render_annotations(temp.path(), &records, AnnotationLimits::default());
    assert!(annotation.contains("task.status"));
    assert!(!annotation.contains("historical /"));
    let capped = render_annotations(
        temp.path(),
        &records,
        AnnotationLimits {
            tail_bytes: 1024,
            item_bytes: 40,
            global_bytes: 1,
            read_cap: 0,
        },
    );
    assert!(capped.contains("read cap"));

    let queue = WakeQueue::new(temp.path());
    queue
        .append(
            WakeKind::Signal,
            "task.status",
            "first",
            UNIX_EPOCH + Duration::from_secs(1),
            &CurrentProcess,
        )
        .expect("append");
    queue
        .append(
            WakeKind::Signal,
            "task.status",
            "second",
            UNIX_EPOCH + Duration::from_secs(2),
            &CurrentProcess,
        )
        .expect("append");
    let drained = queue
        .drain_with_publish(&CurrentProcess, |rows| {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].payload, "second");
            Ok(())
        })
        .expect("drain");
    assert_eq!(drained.len(), 1);
    assert!(
        queue
            .drain_with_publish(&CurrentProcess, |_| Ok(()))
            .expect("empty drain")
            .is_empty()
    );
}

#[test]
fn refusal_and_transition_accessors_are_covered() {
    assert!(crate::gate_refuse::is_gate_agent(true, false));
    assert!(!crate::gate_refuse::is_gate_agent(true, true));
    for (status, expected) in [
        ("blocked", TransitionAction::Actionable),
        ("working", TransitionAction::Absorb),
        ("idle", TransitionAction::Defer),
        ("done", TransitionAction::Defer),
        ("other", TransitionAction::Fallback),
    ] {
        assert_eq!(policy(status), expected);
        assert!(!expected.as_str().is_empty());
    }
    assert!(crate::transition::TransitionRecord::parse("too\tfew").is_err());
}

//! Real CLI and Git integration against owner-published synthetic task metadata.
//! These tests do not launch a model provider or claim live harness evidence.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use multplx_core::filesystem::atomic_replace;
use multplx_domain::lifecycle::spawn::{self, Context};
use multplx_domain::lifecycle::subagent_model::{
    ArtifactKind, AssignmentRole, TaskRecord, read_meta, require_writer_version, write_meta,
};
use sha2::{Digest, Sha256};

struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    record: TaskRecord,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp");
        let home = fs::canonicalize(temp.path()).unwrap().join("home");
        let project = home.join("project");
        fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "--quiet", "-b", "main"]);
        git(
            &project,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "base",
            ],
        );
        fs::create_dir_all(home.join("state")).unwrap();
        fs::create_dir_all(home.join("data/task")).unwrap();
        let body = "Inspect the local test fixture.\n";
        fs::write(home.join("data/task/brief.md"), body).unwrap();
        let root = format!("root-home:{}", home.display());
        let mut record = TaskRecord::new(
            "task".into(),
            AssignmentRole::Researcher,
            ArtifactKind::Report,
            false,
            root.clone(),
            root,
            home.to_string_lossy().into_owned(),
        );
        record.project =
            Some(multplx_domain::project_registry::bind_project(&home, &project).unwrap());
        record.accepted_brief_digest = Some(format!("{:x}", Sha256::digest(body)));
        record.accepted_brief_path = Some(
            home.join("data/task/brief.md")
                .to_string_lossy()
                .into_owned(),
        );
        record.briefs[0].scope = body.into();
        require_writer_version(&home.join("state")).unwrap();
        atomic_replace(
            home.join("state/task.meta"),
            write_meta("kind=scout\nmode=local-only\nyolo=off\n", &record)
                .unwrap()
                .as_bytes(),
            0o600,
        )
        .unwrap();
        Self {
            _temp: temp,
            home,
            project,
            record,
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("MX_") {
                command.env_remove(name);
            }
        }
        command
            .current_dir(&self.project)
            .env("MX_HOME", &self.home)
            .env("MX_ROOT_OVERRIDE", &self.home)
            .env("MX_REPORT_STATE_OVERRIDE", self.home.join("state"))
            .env("MX_TASK_ID", "task");
        let attempt = self.record.attempt.as_ref().unwrap();
        command
            .env("MX_ATTEMPT_ID", &attempt.id)
            .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
            .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string());
        command
    }
    fn report(&self, message_id: &str) -> Command {
        let mut command = self.command();
        command.args([
            "supervision",
            "mx-report",
            "--id",
            "task",
            "--state",
            "working",
            "--message",
            "fixture progress",
            "--message-id",
            message_id,
        ]);
        command
    }
    fn evidence(&self, id: &str) -> serde_json::Value {
        serde_json::from_slice(
            &fs::read(
                self.home
                    .join("state/evidence")
                    .join(format!("task-{id}.json")),
            )
            .unwrap(),
        )
        .unwrap()
    }
}
fn git(path: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn success(output: Output) {
    assert!(
        output.status.success(),
        "status={:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn reports_accept_current_identity_retry_once_and_retain_stale_evidence() {
    let fixture = Fixture::new();
    success(fixture.report("current").output().unwrap());
    success(fixture.report("current").output().unwrap());
    let status = fs::read_to_string(fixture.home.join("state/task.status")).unwrap();
    assert_eq!(status.lines().count(), 1);
    assert_eq!(fixture.evidence("current")["accepted"], true);
    let stale = fixture
        .report("old-attempt")
        .env("MX_ATTEMPT_ID", "obsolete-attempt")
        .output()
        .unwrap();
    assert_eq!(stale.status.code(), Some(3));
    assert_eq!(fixture.evidence("old-attempt")["accepted"], false);
    let stale = fixture
        .report("old-brief")
        .env("MX_BRIEF_REVISION", "2")
        .output()
        .unwrap();
    assert_eq!(stale.status.code(), Some(3));
    assert_eq!(fixture.evidence("old-brief")["accepted"], false);
    assert_eq!(
        fs::read_to_string(fixture.home.join("state/task.status")).unwrap(),
        status
    );
}

#[test]
fn copied_metadata_cannot_accept_a_report_in_another_home() {
    let fixture = Fixture::new();
    let other = fixture._temp.path().join("other");
    fs::create_dir_all(other.join("state")).unwrap();
    fs::copy(
        fixture.home.join("state/task.meta"),
        other.join("state/task.meta"),
    )
    .unwrap();
    let output = fixture
        .report("wrong-home")
        .env("MX_HOME", &other)
        .env("MX_REPORT_STATE_OVERRIDE", other.join("state"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(!other.join("state/task.status").exists());
    assert!(!fixture.home.join("state/task.status").exists());
}

#[test]
fn explicit_recorded_state_override_accepts_only_its_exact_owner_path() {
    let fixture = Fixture::new();
    let state = fs::canonicalize(fixture._temp.path())
        .unwrap()
        .join("configured-state");
    fs::create_dir_all(&state).unwrap();
    let mut record = fixture.record.clone();
    record.owner_state = Some(state.to_string_lossy().into_owned());
    require_writer_version(&state).unwrap();
    atomic_replace(
        state.join("task.meta"),
        write_meta("kind=scout\nmode=local-only\nyolo=off\n", &record)
            .unwrap()
            .as_bytes(),
        0o600,
    )
    .unwrap();
    success(
        fixture
            .report("custom-state")
            .env("MX_REPORT_STATE_OVERRIDE", &state)
            .output()
            .unwrap(),
    );
    let evidence: serde_json::Value =
        serde_json::from_slice(&fs::read(state.join("evidence/task-custom-state.json")).unwrap())
            .unwrap();
    assert_eq!(evidence["accepted"], true);
    assert!(!fixture.home.join("state/task.status").exists());
    let copied = fixture._temp.path().join("copied-state");
    fs::create_dir_all(&copied).unwrap();
    fs::copy(state.join("task.meta"), copied.join("task.meta")).unwrap();
    assert_eq!(
        fixture
            .report("copied-custom-state")
            .env("MX_REPORT_STATE_OVERRIDE", &copied)
            .output()
            .unwrap()
            .status
            .code(),
        Some(3)
    );
    assert!(!copied.join("task.status").exists());
}

#[test]
fn cli_revision_retains_prior_brief_and_prepare_launch_uses_the_accepted_revision() {
    let fixture = Fixture::new();
    success(fixture.report("before-revision").output().unwrap());
    let revised = fixture.home.join("revised.md");
    fs::write(&revised, "Inspect the corrected fixture scope.\n").unwrap();
    let args = [
        "task-model",
        "revise",
        "task",
        "--expected-revision",
        "1",
        "--scope",
        "corrected scope",
        "--reason",
        "human correction",
        "--role",
        "reviewer",
        "--artifact",
        "report",
        "--brief-file",
        revised.to_str().unwrap(),
    ];
    success(fixture.command().args(args).output().unwrap());
    success(fixture.command().args(args).output().unwrap());
    let record = read_meta(
        "task",
        &fs::read_to_string(fixture.home.join("state/task.meta")).unwrap(),
    )
    .unwrap();
    assert_eq!(record.accepted_brief_revision, Some(2));
    assert_eq!(record.briefs.len(), 2);
    let context = Context {
        root: fixture.home.clone(),
        home: fixture.home.clone(),
        data: fixture.home.join("data"),
        state: fixture.home.join("state"),
        projects: fixture.home.join("projects"),
    };
    let args = vec![
        "task".into(),
        fixture.project.to_string_lossy().into_owned(),
        "--role".into(),
        "reviewer".into(),
        "--output".into(),
        "report".into(),
    ];
    let args = args
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>();
    let mut request = spawn::parse(&args, &context, "codex").unwrap();
    spawn::prepare_binding(&context, &mut request).unwrap();
    assert_eq!(
        request.binding.as_ref().unwrap().accepted_brief_revision,
        Some(2)
    );
    assert_eq!(
        request.binding.as_ref().unwrap().accepted_brief_digest,
        record.accepted_brief_digest
    );
    assert_eq!(
        request.binding.as_ref().unwrap().project,
        fixture.record.project
    );
    // Retry the already accepted event after a revision; it remains one historical event.
    success(fixture.report("before-revision").output().unwrap());
    assert_eq!(
        fs::read_to_string(fixture.home.join("state/task.status"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(
        fixture
            .report("stale-after-revision")
            .output()
            .unwrap()
            .status
            .code(),
        Some(3)
    );
    assert_eq!(fixture.evidence("stale-after-revision")["accepted"], false);
    success(
        fixture
            .report("after-revision")
            .env("MX_BRIEF_REVISION", "2")
            .output()
            .unwrap(),
    );
    assert_eq!(fixture.evidence("after-revision")["accepted"], true);
}

#[test]
fn teardown_refuses_borrowed_source_targets_and_replaced_project_identity() {
    let fixture = Fixture::new();
    fs::write(
        fixture.home.join("data/task/report.md"),
        "Synthetic report satisfies retirement prerequisite; source must remain intact.\n",
    )
    .unwrap();
    let borrowed = fs::canonicalize(fixture._temp.path())
        .unwrap()
        .join("borrowed-project");
    fs::create_dir_all(&borrowed).unwrap();
    git(&borrowed, &["init", "--quiet", "-b", "main"]);
    git(
        &borrowed,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "base",
        ],
    );
    let mut record = fixture.record.clone();
    record.project =
        Some(multplx_domain::project_registry::bind_project(&fixture.home, &borrowed).unwrap());
    let legacy = format!(
        "kind=scout\nmode=local-only\nyolo=off\nproject={}\nworktree={}\n",
        borrowed.display(),
        borrowed.display()
    );
    atomic_replace(
        fixture.home.join("state/task.meta"),
        write_meta(&legacy, &record).unwrap().as_bytes(),
        0o600,
    )
    .unwrap();
    fs::write(
        fixture.home.join("data/backlog.md"),
        "## In flight\n\n## Queued\n\n## Done\n",
    )
    .unwrap();
    success(
        fixture
            .command()
            .args([
                "authority",
                "mx-decision-hold.sh",
                "complete",
                "task",
                "--none",
            ])
            .output()
            .unwrap(),
    );
    let before = fs::read(fixture.home.join("state/task.meta")).unwrap();
    let output = fixture
        .command()
        .args(["teardown", "task"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("user-owned checkout"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(borrowed.join(".git").exists());
    assert_eq!(
        fs::read(fixture.home.join("state/task.meta")).unwrap(),
        before
    );
    let retained = borrowed.with_file_name("retained-project");
    fs::rename(&borrowed, &retained).unwrap();
    fs::create_dir(&borrowed).unwrap();
    git(&borrowed, &["init", "--quiet", "-b", "main"]);
    git(
        &borrowed,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "replacement",
        ],
    );
    let output = fixture
        .command()
        .args(["teardown", "task"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("replaced"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(retained.join(".git").exists());
    assert!(borrowed.join(".git").exists());
}

#[test]
fn concurrent_three_repository_queue_writers_keep_task_routes_and_bases_immutable() {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Barrier};

    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap().join("home");
    fs::create_dir_all(home.join("state")).unwrap();
    fs::create_dir_all(home.join("config")).unwrap();
    let mut tasks = Vec::new();
    for index in 0..3 {
        let id = format!("repo-task-{index}");
        let project = home.join(format!("repo-{index}"));
        fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "--quiet", "-b", "main"]);
        fs::write(project.join("identity.txt"), &id).unwrap();
        git(&project, &["add", "identity.txt"]);
        git(
            &project,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "--quiet",
                "-m",
                "base",
            ],
        );
        fs::create_dir_all(home.join("data").join(&id)).unwrap();
        fs::write(
            home.join("data").join(&id).join("brief.md"),
            format!("Inspect only repository {index}.\n"),
        )
        .unwrap();
        tasks.push((id, project));
    }
    let command = |home: &Path| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("MX_") {
                command.env_remove(key);
            }
        }
        command
            .current_dir(home)
            .env("MX_HOME", home)
            .env("MX_ROOT_OVERRIDE", home)
            // Deterministic capacity observations; no provider/session is launched.
            .env("MX_HEADROOM_CPU_COUNT", "8")
            .env("MX_HEADROOM_LOAD1", "0")
            .env("MX_HEADROOM_MEM_AVAILABLE_BYTES", "8589934592")
            .env("MX_HEADROOM_IN_USE", "0")
            .env("MX_HEADROOM_IGNORE_DISPATCH_CONFIG", "1");
        command
    };
    let barrier = Arc::new(Barrier::new(tasks.len()));
    let writers = tasks
        .iter()
        .map(|(id, project)| {
            let barrier = Arc::clone(&barrier);
            let mut queued = command(&home);
            queued.args([
                "headroom",
                "--queue-add",
                id,
                project.to_str().unwrap(),
                "--harness",
                "codex",
                "--role",
                "researcher",
                "--output",
                "report",
            ]);
            std::thread::spawn(move || {
                barrier.wait();
                queued.output().unwrap()
            })
        })
        .collect::<Vec<_>>();
    for writer in writers {
        success(writer.join().unwrap());
    }

    let mut accepted = Vec::new();
    let mut queue_bytes = Vec::new();
    for (id, project) in &tasks {
        let bytes = fs::read_to_string(
            home.join("state/.dispatch-queue")
                .join(format!("{id}.request")),
        )
        .unwrap();
        assert!(bytes.starts_with("version=2\n"));
        let model = bytes
            .lines()
            .find_map(|line| line.strip_prefix("canonical_model="))
            .unwrap();
        let record =
            read_meta(id, &format!("schema_version=2\ncanonical_model={model}\n")).unwrap();
        assert_eq!(record.task_id, *id);
        assert_eq!(record.project.as_ref().unwrap().canonical_path, *project);
        assert_eq!(record.role, AssignmentRole::Researcher);
        assert_eq!(
            record.schedule.waiting_condition.as_deref(),
            Some("dispatch capacity")
        );
        accepted.push(record);
        queue_bytes.push(bytes);
    }
    assert_eq!(
        accepted
            .iter()
            .map(|r| &r.project.as_ref().unwrap().project_id)
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(
        accepted
            .iter()
            .map(|r| &r.project.as_ref().unwrap().starting_revision)
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(
        accepted
            .iter()
            .map(|r| &r.attempt.as_ref().unwrap().id)
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    assert!(
        accepted
            .iter()
            .all(|r| r.parent_id == accepted[0].parent_id && r.root_id == accepted[0].root_id)
    );
    assert_eq!(
        multplx_domain::project_registry::read_catalog(&home)
            .unwrap()
            .projects
            .len(),
        3
    );

    // Select each display context after advancing its checkout. Existing queued
    // tasks keep their captured starting commit, including an idempotent retry.
    for (index, (id, project)) in tasks.iter().enumerate().rev() {
        git(
            project,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "later HEAD",
            ],
        );
        let selected = command(&home)
            .args(["project", "resolve", project.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(selected.status.success());
        let context: multplx_domain::project_registry::ProjectBinding =
            serde_json::from_slice(&selected.stdout).unwrap();
        assert_eq!(
            context.project_id,
            accepted[index].project.as_ref().unwrap().project_id
        );
        assert_ne!(
            context.starting_revision,
            accepted[index].project.as_ref().unwrap().starting_revision
        );
        multplx_domain::project_registry::validate_binding(
            &home,
            accepted[index].project.as_ref().unwrap(),
        )
        .unwrap();
        success(
            command(&home)
                .args([
                    "headroom",
                    "--queue-add",
                    id,
                    project.to_str().unwrap(),
                    "--harness",
                    "codex",
                    "--role",
                    "researcher",
                    "--output",
                    "report",
                ])
                .output()
                .unwrap(),
        );
        assert_eq!(
            fs::read_to_string(
                home.join("state/.dispatch-queue")
                    .join(format!("{id}.request"))
            )
            .unwrap(),
            queue_bytes[index]
        );
    }
}

#[test]
fn single_checkout_retirement_verifies_reservation_and_never_removes_borrowed_files() {
    let fixture = Fixture::new();
    fs::write(
        fixture.project.join("retained-user-file"),
        "unfinished user work\n",
    )
    .unwrap();
    let reservation = fixture.home.join("state/single-checkout.json");
    let legacy = format!(
        "kind=scout\nmode=local-only\nyolo=off\nproject={}\nworktree={}\nsingle_checkout=yes\nsingle_checkout_record={}\n",
        fixture.project.display(),
        fixture.project.display(),
        reservation.display()
    );
    atomic_replace(
        fixture.home.join("state/task.meta"),
        write_meta(&legacy, &fixture.record).unwrap().as_bytes(),
        0o600,
    )
    .unwrap();
    let context = multplx_domain::lifecycle::teardown::Context {
        root: fixture.home.clone(),
        home: fixture.home.clone(),
        data: fixture.home.join("data"),
        state: fixture.home.join("state"),
    };
    let before = fs::read(fixture.home.join("state/task.meta")).unwrap();
    atomic_replace(
        &reservation,
        serde_json::to_vec(
            &serde_json::json!({"task_id":"other-task","target_identity":fixture.project}),
        )
        .unwrap()
        .as_slice(),
        0o600,
    )
    .unwrap();
    let mut stopped = false;
    let wrong = multplx_domain::lifecycle::teardown::run_override("task", &context, |_| {
        stopped = true;
        Ok(())
    });
    assert!(
        !stopped,
        "invalid retained-checkout ownership must not stop the task"
    );
    assert_eq!(wrong.status, 1);
    assert!(
        wrong.stderr.contains("reservation ownership mismatch"),
        "{}",
        wrong.stderr
    );
    assert_eq!(
        fs::read(fixture.home.join("state/task.meta")).unwrap(),
        before
    );
    assert!(reservation.exists());
    let ordinary = multplx_domain::lifecycle::teardown::run(&["task".into()], &context, |_| Ok(()));
    assert_eq!(ordinary.status, 1);
    assert!(ordinary.stderr.contains("user-owned checkout"));
    atomic_replace(
        &reservation,
        serde_json::to_vec(
            &serde_json::json!({"task_id":"task","target_identity":fixture.project}),
        )
        .unwrap()
        .as_slice(),
        0o600,
    )
    .unwrap();
    let retired = multplx_domain::lifecycle::teardown::run_override("task", &context, |_| Ok(()));
    assert_eq!(retired.status, 0, "{}", retired.stderr);
    assert!(!reservation.exists());
    assert!(!fixture.home.join("state/task.meta").exists());
    assert!(fixture.project.join(".git").exists());
    assert_eq!(
        fs::read_to_string(fixture.project.join("retained-user-file")).unwrap(),
        "unfinished user work\n"
    );
}

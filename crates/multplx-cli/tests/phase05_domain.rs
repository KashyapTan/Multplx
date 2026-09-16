use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use multplx_domain::lifecycle::subagent_model::{
    ArtifactKind, AssignmentRole, DomainBinding, TaskRecord, read_meta, write_meta,
};

fn mx(home: &Path, state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    command
        .env("MX_HOME", home)
        .env("MX_ROOT_OVERRIDE", home)
        .env("MX_STATE_OVERRIDE", state)
        .env("MX_DATA_OVERRIDE", home.join("data"))
        .env("MX_PROJECTS_OVERRIDE", home.join("projects"));
    command
}

fn run(command: &mut Command) -> Output {
    command.output().expect("run mx")
}

fn coordinator(home: &Path, state: &Path) -> TaskRecord {
    fs::create_dir_all(state).unwrap();
    let owner = home.to_string_lossy().into_owned();
    let root = format!("root-home:{owner}");
    let mut record = TaskRecord::new(
        "coord".into(),
        AssignmentRole::SubOrchestrator,
        ArtifactKind::Coordination,
        false,
        root.clone(),
        root,
        owner,
    );
    record.owner_state = Some(state.to_string_lossy().into_owned());
    record.parent_state = record.owner_state.clone();
    record.private_home = true;
    record.persistent_home = Some(
        home.join("coordinators/coord")
            .to_string_lossy()
            .into_owned(),
    );
    record.domain = Some(DomainBinding {
        domain_id: "domain-coord".into(),
        coordinator_id: "coord".into(),
        scope_revision: 1,
        assignment_generation: 1,
        projects: Vec::new(),
        idea_id: Some("runtime-idea".into()),
        scope: "research the runtime".into(),
    });
    record.owning_coordinator = Some("coord".into());
    fs::write(
        state.join("coord.meta"),
        write_meta("kind=daemon\n", &record).unwrap(),
    )
    .unwrap();
    record
}

fn git_project(base: &Path) -> PathBuf {
    let project = base.join("selected project");
    fs::create_dir(&project).unwrap();
    for arguments in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["config", "user.name", "Domain CLI Test"],
    ] {
        assert!(
            Command::new("git")
                .args(arguments)
                .current_dir(&project)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(project.join("README.md"), "selected\n").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&project)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["commit", "-q", "-m", "initial"])
            .current_dir(&project)
            .status()
            .unwrap()
            .success()
    );
    project
}

#[test]
fn domain_cli_inspects_revises_retries_and_binds_a_project() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("owner home");
    let state = home.join("authority state");
    coordinator(&home, &state);

    let inspected = run(mx(&home, &state).args(["domain", "inspect", "coord"]));
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let inspected: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected["domain"]["scope_revision"], 1);
    assert_eq!(inspected["domain"]["idea_id"], "runtime-idea");

    let revision = [
        "domain",
        "revise",
        "coord",
        "--expected-revision",
        "1",
        "--scope",
        "research only the deterministic runtime path",
        "--reason",
        "scope accepted",
    ];
    for _ in 0..2 {
        let revised = run(mx(&home, &state).args(revision));
        assert!(
            revised.status.success(),
            "{}",
            String::from_utf8_lossy(&revised.stderr)
        );
        let revised: serde_json::Value = serde_json::from_slice(&revised.stdout).unwrap();
        assert_eq!(revised["domain"]["scope_revision"], 2);
        assert_eq!(revised["domain"]["idea_id"], "runtime-idea");
    }

    let project = git_project(temp.path());
    let bound = run(mx(&home, &state).args([
        "domain",
        "bind-project",
        "coord",
        "--expected-revision",
        "2",
        "--project",
        project.to_str().unwrap(),
        "--reason",
        "implementation repository accepted",
    ]));
    assert!(
        bound.status.success(),
        "{}",
        String::from_utf8_lossy(&bound.stderr)
    );
    let bound: serde_json::Value = serde_json::from_slice(&bound.stdout).unwrap();
    assert_eq!(bound["domain"]["scope_revision"], 3);
    assert_eq!(bound["domain"]["projects"].as_array().unwrap().len(), 1);

    let raw = fs::read_to_string(state.join("coord.meta")).unwrap();
    let record = read_meta("coord", &raw).unwrap();
    assert_eq!(record.assignments.len(), 3);
    assert!(Path::new(record.accepted_brief_path.as_deref().unwrap()).is_file());
    assert!(
        Path::new(record.persistent_home.as_deref().unwrap())
            .join("data/charter.md")
            .is_file()
    );
}

#[test]
fn domain_cli_rejects_ambiguous_or_incomplete_mutations_before_revision() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let state = home.join("state");
    coordinator(&home, &state);

    for arguments in [
        vec!["domain", "unknown", "coord"],
        vec!["domain", "inspect", "coord", "extra"],
        vec!["domain", "revise", "coord", "--scope", "bounded"],
        vec![
            "domain",
            "revise",
            "coord",
            "--expected-revision",
            "many",
            "--scope",
            "bounded",
            "--reason",
            "reason",
        ],
        vec![
            "domain",
            "revise",
            "coord",
            "--expected-revision",
            "1",
            "--scope",
            "bounded",
            "--scope",
            "duplicate",
            "--reason",
            "reason",
        ],
        vec![
            "domain",
            "revise",
            "coord",
            "--expected-revision",
            "1",
            "--scope",
            "bounded",
            "--reason",
            "reason",
            "--idea",
            "idea",
            "--project",
            "project",
        ],
        vec![
            "domain",
            "bind-project",
            "coord",
            "--expected-revision",
            "1",
            "--scope",
            "forbidden",
            "--reason",
            "reason",
        ],
        vec![
            "domain",
            "revise",
            "coord",
            "--expected-revision",
            "1",
            "--scope",
            "bounded",
            "--reason",
            "reason",
            "--unknown",
            "value",
        ],
    ] {
        let rejected = run(mx(&home, &state).args(&arguments));
        assert!(!rejected.status.success(), "accepted {arguments:?}");
    }
    let unchanged = multplx_domain::lifecycle::domain::read_coordinator(&state, "coord").unwrap();
    assert_eq!(unchanged.domain.unwrap().scope_revision, 1);

    for arguments in [
        vec!["domain"],
        vec!["domain", "--help"],
        vec!["domain", "revise", "--help"],
    ] {
        let help = run(mx(&home, &state).args(arguments));
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains("mx domain revise"));
    }
}

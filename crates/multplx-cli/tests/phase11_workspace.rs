//! Phase 11 real local Git/filesystem/runtime-owner integration.
//!
//! No model provider is launched and no live-agent throughput is claimed.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use multplx_domain::lifecycle::subagent_model::{
    ArtifactKind, AssignmentRole, DomainBinding, TaskRecord, write_meta,
};

fn run(command: &mut Command) -> Output {
    command.output().expect("run command")
}

fn success(command: &mut Command) -> Output {
    let output = run(command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = success(Command::new("git").arg("-C").arg(repo).args(args));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn repo(path: &Path) -> String {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "--quiet", "-b", "main"]);
    fs::write(path.join("base.txt"), format!("{}\n", path.display())).unwrap();
    git(path, &["add", "base.txt"]);
    git(
        path,
        &[
            "-c",
            "user.name=Phase11",
            "-c",
            "user.email=phase11@example.test",
            "commit",
            "--quiet",
            "-m",
            "base",
        ],
    );
    git(path, &["rev-parse", "HEAD"])
}

fn mx(home: &Path, runtime: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("MX_") {
            command.env_remove(name);
        }
    }
    command
        .env("MX_HOME", home)
        .env("MX_ROOT_OVERRIDE", runtime)
        .env("MX_STATE_OVERRIDE", home.join("state"))
        .env("MX_DATA_OVERRIDE", home.join("data"))
        .env("MX_CONFIG_OVERRIDE", home.join("config"));
    command
}

fn task(
    home: &Path,
    runtime: &Path,
    project: &Path,
    request: &str,
    task: &str,
    text: &str,
    options: (Option<&str>, Option<&Path>),
) -> serde_json::Value {
    let mut command = mx(home, runtime);
    command.args([
        "task",
        "--project",
        project.to_str().unwrap(),
        "--request-id",
        request,
        "--batch-id",
        "three-repository-request",
        "--task-id",
        task,
    ]);
    if let Some(start) = options.0 {
        command.args(["--start", start]);
    }
    if let Some(artifact) = options.1 {
        command.args(["--artifact", artifact.to_str().unwrap()]);
    }
    command.arg(text);
    serde_json::from_slice(&success(&mut command).stdout).unwrap()
}

fn allocate(
    home: &Path,
    runtime: &Path,
    selector: &str,
    request: &str,
    task: &str,
    base: &str,
) -> serde_json::Value {
    serde_json::from_slice(
        &success(mx(home, runtime).args([
            "worktree",
            "acquire",
            selector,
            "--request",
            request,
            "--task",
            task,
            "--attempt",
            &format!("attempt-{task}"),
            "--base",
            base,
        ]))
        .stdout,
    )
    .unwrap()
}

fn commit_outcome(allocation: &serde_json::Value, filename: &str, content: &str) -> PathBuf {
    let path = PathBuf::from(allocation["binding"]["path"].as_str().unwrap());
    let artifact = path.join(filename);
    fs::write(&artifact, content).unwrap();
    git(&path, &["add", filename]);
    git(
        &path,
        &[
            "-c",
            "user.name=Phase11",
            "-c",
            "user.email=phase11@example.test",
            "commit",
            "--quiet",
            "-m",
            filename,
        ],
    );
    artifact
}

fn complete_requests(home: &Path, requests: &[(&str, &Path)]) {
    let script = home.join("complete-requests.sh");
    let mut text = String::from("#!/bin/sh\nset -eu\n");
    for (request, artifact) in requests {
        text.push_str(&format!(
            "'{}' request acknowledge '{}' >/dev/null\n'{}' request response '{}' --id 'response-{}' --summary accepted >/dev/null\n'{}' request complete '{}' --id 'completion-{}' --summary complete --artifact '{}' >/dev/null\n",
            env!("CARGO_BIN_EXE_mx"), request,
            env!("CARGO_BIN_EXE_mx"), request, request,
            env!("CARGO_BIN_EXE_mx"), request, request, artifact.display(),
        ));
    }
    fs::write(&script, text).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let owner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/codex-wake-owner.py");
    success(
        Command::new("python3")
            .arg(owner)
            .arg(home.join("state"))
            .arg("retain")
            .arg(script)
            .env("MX_HOME", home)
            .env("MX_STATE_OVERRIDE", home.join("state")),
    );
}

#[test]
fn three_repository_intake_allocation_outcomes_repair_and_domain_routing_stay_independent() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let home = root.join("home");
    let runtime = root.join("runtime");
    let repo_a = root.join("dev/team-a/app");
    let repo_b = root.join("dev/group/team-b/app");
    let repo_c = root.join("elsewhere/repo-c");
    fs::create_dir_all(home.join("state")).unwrap();
    fs::create_dir_all(home.join("data")).unwrap();
    fs::create_dir(&runtime).unwrap();
    let base_a = repo(&repo_a);
    let base_b = repo(&repo_b);
    let base_c = repo(&repo_c);
    let research = root.join("research.md");
    fs::write(&research, "accepted research context\n").unwrap();

    let mut first_a = mx(&home, &runtime);
    first_a.args([
        "task",
        "--project",
        repo_a.to_str().unwrap(),
        "--request-id",
        "request-a",
        "--batch-id",
        "three-repository-request",
        "--task-id",
        "task-a",
        "--artifact",
        research.to_str().unwrap(),
        "Research repository A",
    ]);
    let accepted_a: serde_json::Value =
        serde_json::from_slice(&success(&mut first_a).stdout).unwrap();
    assert_eq!(accepted_a["request"]["starting_revision"], base_a);
    let accepted_c = task(
        &home,
        &runtime,
        &repo_c,
        "request-c",
        "task-c",
        "Implement repository C",
        (None, None),
    );

    success(mx(&home, &runtime).args(["project", "register", repo_b.to_str().unwrap()]));
    let ambiguous = run(mx(&home, &runtime).args([
        "task",
        "--project",
        "app",
        "--request-id",
        "request-b-ambiguous",
        "Fix repository B",
    ]));
    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("2 candidates"));
    let root_store =
        multplx_domain::operational_input::RequestStore::new(home.join("state"), &home);
    assert_eq!(root_store.observe(16).unwrap().1, 2);

    git(&repo_b, &["checkout", "--detach", &base_b]);
    fs::write(repo_b.join("later.txt"), "later\n").unwrap();
    git(&repo_b, &["add", "later.txt"]);
    git(
        &repo_b,
        &[
            "-c",
            "user.name=Phase11",
            "-c",
            "user.email=phase11@example.test",
            "commit",
            "--quiet",
            "-m",
            "later",
        ],
    );
    let accepted_b = task(
        &home,
        &runtime,
        &repo_b,
        "request-b",
        "task-b",
        "Implement repository B",
        (Some(&base_b), None),
    );
    assert_eq!(accepted_b["request"]["starting_revision"], base_b);

    fs::write(repo_a.join("after.txt"), "after\n").unwrap();
    git(&repo_a, &["add", "after.txt"]);
    git(
        &repo_a,
        &[
            "-c",
            "user.name=Phase11",
            "-c",
            "user.email=phase11@example.test",
            "commit",
            "--quiet",
            "-m",
            "after intake",
        ],
    );
    let retried_a = task(
        &home,
        &runtime,
        &repo_a,
        "request-a",
        "task-a",
        "Research repository A",
        (None, Some(&research)),
    );
    assert_eq!(retried_a["newly_accepted"], false);
    assert_eq!(retried_a["request"]["starting_revision"], base_a);

    let moved_c = root.join("moved/repo-c");
    fs::create_dir(moved_c.parent().unwrap()).unwrap();
    fs::rename(&repo_c, &moved_c).unwrap();
    let checkout_c = accepted_c["request"]["checkout_id"].as_str().unwrap();
    success(mx(&home, &runtime).args(["project", "repair", checkout_c, moved_c.to_str().unwrap()]));

    let allocation_a = allocate(
        &home,
        &runtime,
        &repo_a.to_string_lossy(),
        "request-a",
        "task-a",
        &base_a,
    );
    let allocation_b = allocate(
        &home,
        &runtime,
        &repo_b.to_string_lossy(),
        "request-b",
        "task-b",
        &base_b,
    );
    let allocation_c = allocate(&home, &runtime, checkout_c, "request-c", "task-c", &base_c);
    let artifact_a = commit_outcome(&allocation_a, "research.md", "repository A findings\n");
    let artifact_b = commit_outcome(&allocation_b, "implementation.txt", "repository B result\n");
    let artifact_c = commit_outcome(&allocation_c, "implementation.txt", "repository C result\n");
    complete_requests(
        &home,
        &[
            ("request-a", &artifact_a),
            ("request-b", &artifact_b),
            ("request-c", &artifact_c),
        ],
    );
    for (request, artifact) in [
        ("request-a", &artifact_a),
        ("request-b", &artifact_b),
        ("request-c", &artifact_c),
    ] {
        let record = root_store.get(request).unwrap();
        assert_eq!(
            record.completion.unwrap().artifact.as_deref(),
            Some(artifact.to_str().unwrap())
        );
    }

    let private = root.join("private-coordinator");
    fs::create_dir_all(private.join("state")).unwrap();
    fs::create_dir(private.join("bin")).unwrap();
    fs::write(private.join("AGENTS.md"), "fixture\n").unwrap();
    fs::write(private.join(".mx-daemon-home"), "coord\n").unwrap();
    let mut coordinator = TaskRecord::new(
        "coord".into(),
        AssignmentRole::SubOrchestrator,
        ArtifactKind::Coordination,
        true,
        "root".into(),
        "root".into(),
        home.to_string_lossy().into_owned(),
    );
    coordinator.private_home = true;
    coordinator.persistent_home = Some(private.to_string_lossy().into_owned());
    coordinator.domain = Some(DomainBinding {
        domain_id: "domain-coord".into(),
        coordinator_id: "coord".into(),
        scope_revision: 1,
        assignment_generation: 1,
        projects: vec![accepted_a["request"]["project_id"].as_str().unwrap().into()],
        idea_id: None,
        scope: "Coordinate repository A follow-up".into(),
    });
    fs::write(
        home.join("state/coord.meta"),
        write_meta("kind=daemon\n", &coordinator).unwrap(),
    )
    .unwrap();
    let domain = serde_json::from_slice::<serde_json::Value>(
        &success(mx(&home, &runtime).args([
            "task",
            "--domain",
            "coord",
            "--project",
            repo_a.to_str().unwrap(),
            "--request-id",
            "domain-request",
            "--task-id",
            "domain-task",
            "Coordinate the repository A follow-up",
        ]))
        .stdout,
    )
    .unwrap();
    assert_eq!(domain["route"]["kind"], "scoped-coordinator");
    assert!(root_store.get("domain-request").is_err());
    let domain_store =
        multplx_domain::operational_input::RequestStore::new(private.join("state"), &private);
    assert_eq!(
        domain_store.get("domain-request").unwrap().task_id,
        "domain-task"
    );
    assert_eq!(root_store.observe(16).unwrap().1, 3);
}

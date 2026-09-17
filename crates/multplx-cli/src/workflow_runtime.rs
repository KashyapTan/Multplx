use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use multplx_core::filesystem::atomic_replace;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use multplx_domain::backlog::{AddRequest, BacklogStore};
use multplx_domain::maintainer_override::{Binding, OverrideStore};
use multplx_domain::workflow::{
    self, Assignment, Contract, Definition, Executor, Gate, RunState, Stage, StageStatus, StageType,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const USAGE: &str = "Usage:\n  mx-workflow.sh validate <definition.workflow.md>\n  mx-workflow.sh run <name|definition.workflow.md> [--input <text> | --request <request-id>] [--id <run-id>] [--project <project-id|checkout-id|alias|path> | --repo <legacy-project-root>] [--depends <run-id>]...\n  mx-workflow.sh status <run-id>\n  mx-workflow.sh resume <run-id>\n  mx-workflow.sh abort <run-id>\n  mx-workflow.sh skip <run-id> <stage-id> --override <request-id>\n  mx-workflow.sh reorder <run-id> <stage-id> --before <stage-id> --override <request-id>\n  mx-workflow.sh dry-run <name|definition.workflow.md> [--input <text>]\n\nSchema v2 placements are orchestrator-context and sub-agent-session. Version 1 broker/actor definitions remain readable as legacy placement aliases. Implementation and sub-orchestrator assignments require a sub-agent session. A routed --request supplies the exact task, batch, project, checkout, brief, scope, context and dependencies; conflicting overrides are rejected. Dependencies must name existing workflow runs and only completed dependencies release a run. Skip and reorder are explicit versioned plan changes; no stage is silently omitted. Orchestrator-context execution requires an explicit MX_WORKFLOW_AGENT_COMMAND file-backed adapter.\n";

pub(crate) fn run(args: &[OsString]) -> i32 {
    let Some(values) = args
        .iter()
        .map(|value| value.to_str().map(ToOwned::to_owned))
        .collect::<Option<Vec<_>>>()
    else {
        return fail("arguments must be UTF-8");
    };
    let result = match values.first().map(String::as_str) {
        Some("validate") => validate(&values[1..]),
        Some("dry-run") => dry_run(&values[1..]),
        Some("run") => launch(&values[1..]),
        Some("status") => status(&values[1..]),
        Some("resume") => resume(&values[1..]),
        Some("abort") => abort(&values[1..]),
        Some("skip") => skip(&values[1..]),
        Some("reorder") => reorder(&values[1..]),
        Some("-h" | "--help") => {
            print!("{USAGE}");
            return 0;
        }
        _ => {
            eprint!("{USAGE}");
            return 2;
        }
    };
    match result {
        Ok(()) => 0,
        Err(error) => fail(&error),
    }
}

fn fail(message: &str) -> i32 {
    eprintln!("mx-workflow: {message}");
    1
}

fn source_root() -> PathBuf {
    std::env::var_os("MX_ROOT_OVERRIDE")
        .or_else(|| std::env::var_os("MX_RUST_SOURCE_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn runtime_root() -> PathBuf {
    std::env::var_os("MX_RUST_SOURCE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn home_root() -> PathBuf {
    std::env::var_os("MX_HOME")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(source_root)
}

fn state_root() -> PathBuf {
    std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_root().join("state"))
}

fn data_root() -> PathBuf {
    std::env::var_os("MX_DATA_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_root().join("data"))
}

fn definition_path(requested: &str) -> Result<PathBuf, String> {
    let candidate = if requested.contains('/') || requested.ends_with(".workflow.md") {
        PathBuf::from(requested)
    } else {
        source_root()
            .join("workflows")
            .join(format!("{requested}.workflow.md"))
    };
    if !candidate.is_file() {
        return Err(format!("definition not found: {}", candidate.display()));
    }
    candidate
        .canonicalize()
        .map_err(|error| format!("definition is unavailable: {error}"))
}

fn run_directory(id: &str) -> Result<PathBuf, String> {
    if !safe_slug(id) {
        return Err(format!("invalid run id: {id}"));
    }
    let directory = state_root().join(format!("{id}.workflow"));
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|_| format!("workflow run not found or unsafe: {id}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!("workflow run not found or unsafe: {id}"));
    }
    Ok(directory)
}

fn safe_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn validate_dependencies(id: &str, dependencies: &[String]) -> Result<(), String> {
    let mut unique = std::collections::BTreeSet::new();
    for dependency in dependencies {
        if !safe_slug(dependency) || dependency == id || !unique.insert(dependency) {
            return Err(
                "workflow dependencies must be unique existing run ids and cannot include self"
                    .to_owned(),
            );
        }
        run_directory(dependency)?;
    }
    let mut stack = dependencies.to_vec();
    let mut visited = std::collections::BTreeSet::new();
    while let Some(dependency) = stack.pop() {
        if dependency == id {
            return Err("cyclic workflow dependencies are not allowed".to_owned());
        }
        if !visited.insert(dependency.clone()) {
            continue;
        }
        let record = read_json(&run_directory(&dependency)?.join("run.json"))?;
        if let Some(children) = record["dependencies"].as_array() {
            for child in children {
                let child = child
                    .as_str()
                    .ok_or("workflow dependency record is invalid")?;
                if !safe_slug(child) {
                    return Err("workflow dependency record is invalid".to_owned());
                }
                stack.push(child.to_owned());
            }
        }
    }
    Ok(())
}

fn dependencies_ready(directory: &Path) -> Result<bool, String> {
    let record = read_json(&directory.join("run.json"))?;
    let dependencies = record["dependencies"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let run = record["run"].as_str().unwrap_or_default();
    let mut waiting = Vec::new();
    for dependency in dependencies {
        let dependency = dependency
            .as_str()
            .ok_or("workflow dependency record is invalid")?;
        let dependency_record = read_json(&run_directory(dependency)?.join("run.json"))?;
        if dependency_record["status"].as_str() != Some("completed") {
            waiting.push(dependency.to_owned());
            continue;
        }
        let backlog = data_root().join("backlog.md");
        if backlog.is_file() {
            let store = BacklogStore::new(backlog);
            if store
                .snapshot(run)
                .is_ok_and(|item| item.blockers.iter().any(|item| item == dependency))
            {
                store
                    .unblock(run, dependency)
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    if waiting.is_empty() {
        Ok(true)
    } else {
        set_run_state(
            directory,
            "waiting",
            "",
            &format!("waiting on workflow dependencies: {}", waiting.join(", ")),
        )?;
        Ok(false)
    }
}

fn now() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    atomic_replace(path, &bytes, 0o600).map_err(|error| error.to_string())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("unsafe workflow record: {}", path.display()));
    }
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| format!("invalid workflow record {}: {error}", path.display()))
}

fn validate(values: &[String]) -> Result<(), String> {
    let [requested] = values else {
        return Err("validate requires one definition".to_owned());
    };
    let definition =
        workflow::parse(&definition_path(requested)?).map_err(|error| error.to_string())?;
    println!(
        "valid: {} ({} stages)",
        definition.name,
        definition.stages.len()
    );
    Ok(())
}

fn dry_run(values: &[String]) -> Result<(), String> {
    let Some(requested) = values.first() else {
        return Err("dry-run requires a definition".to_owned());
    };
    let mut input = "example input".to_owned();
    let mut index = 1;
    while index < values.len() {
        if values[index] != "--input" || index + 1 >= values.len() {
            return Err("invalid dry-run arguments".to_owned());
        }
        input = values[index + 1].clone();
        index += 2;
    }
    let definition =
        workflow::parse(&definition_path(requested)?).map_err(|error| error.to_string())?;
    println!("workflow: {}", definition.name);
    println!("input: {input}");
    for stage in definition.stages {
        let kind = match stage.kind {
            StageType::Interactive => "interactive",
            StageType::Agent => "agent",
            StageType::Command => "command",
        };
        let gate = match stage.gate {
            Gate::Approve => "approve",
            Gate::Auto => "auto",
        };
        let executor = match stage.executor {
            Some(Executor::OrchestratorContext) => "orchestrator-context",
            Some(Executor::SubAgentSession) => "sub-agent-session",
            None => "-",
        };
        let assignment = match stage.assignment {
            Some(Assignment::Researcher) => "researcher",
            Some(Assignment::Implementer) => "implementer",
            Some(Assignment::Reviewer) => "reviewer",
            Some(Assignment::SubOrchestrator) => "sub-orchestrator",
            None => "legacy-default",
        };
        let output = stage
            .output
            .as_deref()
            .map(|value| workflow::substitute(value, "dry-run", &input, ""))
            .unwrap_or_else(|| "-".to_owned());
        println!(
            "{} | type={kind} | gate={gate} | executor={executor} | assignment={assignment} | output={output}",
            stage.id
        );
    }
    Ok(())
}

fn launch(values: &[String]) -> Result<(), String> {
    let Some(requested) = values.first() else {
        return Err("run requires a definition".to_owned());
    };
    let mut input = None;
    let mut id = None;
    let mut project = None;
    let mut legacy_repo = None;
    let mut request_id = None;
    let mut dependencies = Vec::new();
    let mut index = 1;
    while index < values.len() {
        let value = values
            .get(index + 1)
            .ok_or_else(|| format!("{} requires a value", values[index]))?;
        match values[index].as_str() {
            "--input" => input = Some(value.clone()),
            "--id" => id = Some(value.clone()),
            "--project" => project = Some(value.clone()),
            "--repo" => legacy_repo = Some(PathBuf::from(value)),
            "--request" => request_id = Some(value.clone()),
            "--depends" => dependencies.push(value.clone()),
            _ => return Err(format!("unknown argument: {}", values[index])),
        }
        index += 2;
    }
    let path = definition_path(requested)?;
    let definition = workflow::parse(&path).map_err(|error| error.to_string())?;
    verify_tracked(&path)?;
    if project.is_some() && legacy_repo.is_some() {
        return Err("choose --project or --repo, not both".to_owned());
    }
    let routed_request = request_id
        .as_deref()
        .map(|request| {
            multplx_domain::operational_input::RequestStore::new(state_root(), home_root())
                .get(request)
                .map_err(|error| format!("routed workflow request is unavailable: {error}"))
        })
        .transpose()?;
    let selected = if let Some(selector) = project {
        multplx_domain::project_registry::resolve_checkout(&home_root(), &selector)?
    } else {
        match (legacy_repo, routed_request.as_ref()) {
            (Some(repo), _) => multplx_domain::project_registry::register_project(
                &home_root(),
                &repo,
                None,
                multplx_domain::project_registry::CheckoutOwnership::UserOwned,
            )?,
            (None, Some(request)) => multplx_domain::project_registry::resolve_checkout(
                &home_root(),
                &request.checkout_id,
            )?,
            (None, None) => multplx_domain::project_registry::register_project(
                &home_root(),
                &std::env::current_dir().map_err(|error| error.to_string())?,
                None,
                multplx_domain::project_registry::CheckoutOwnership::UserOwned,
            )?,
        }
    };
    if let Some(request) = &routed_request
        && (request.project_id != selected.project_id
            || request.checkout_id != selected.checkout_id
            || request.starting_revision != selected.starting_revision)
    {
        return Err(
            "routed workflow request does not match the selected project, checkout or starting revision"
                .to_owned(),
        );
    }
    let input = match (
        input.filter(|value| !value.is_empty()),
        routed_request.as_ref(),
    ) {
        (Some(input), Some(request)) if input != request.scope => {
            return Err("--input conflicts with the routed request scope".to_owned());
        }
        (Some(input), _) => input,
        (None, Some(request)) => request.scope.clone(),
        (None, None) => return Err("--input or --request is required".to_owned()),
    };
    let repo = selected.canonical_path.clone();
    let id = if let Some(request) = &routed_request {
        if id.as_deref().is_some_and(|id| id != request.task_id) {
            return Err("--id conflicts with the routed request task identity".to_owned());
        }
        request.task_id.clone()
    } else {
        id.unwrap_or_else(|| {
            std::env::var("MX_WORKFLOW_RUN_ID").unwrap_or_else(|_| {
                format!(
                    "{}-{}-{:04x}",
                    definition.name,
                    now().replace(['-', ':', 'T', 'Z'], ""),
                    std::process::id() % 65536
                )
            })
        })
    };
    if !safe_slug(&id) {
        return Err("run id must be a privacy-safe slug".to_owned());
    }
    if let Some(request) = &routed_request {
        if !dependencies.is_empty() && dependencies != request.dependencies {
            return Err("--depends conflicts with routed request dependencies".to_owned());
        }
        if dependencies.is_empty() {
            dependencies = request.dependencies.clone();
        }
    }
    validate_dependencies(&id, &dependencies)?;
    fs::create_dir_all(state_root()).map_err(|error| error.to_string())?;
    fs::create_dir_all(data_root()).map_err(|error| error.to_string())?;
    let directory = state_root().join(format!("{id}.workflow"));
    if directory.exists() || fs::symlink_metadata(&directory).is_ok() {
        return Err(format!("run id already exists: {id}"));
    }
    workflow::create_snapshot(&path, &directory, &definition, &input)
        .map_err(|error| error.to_string())?;
    let timestamp = now();
    let digest = format!(
        "{:x}",
        Sha256::digest(fs::read(&path).map_err(|error| error.to_string())?)
    );
    let backlog_project = selected.project_id.clone();
    let request_correlation = routed_request.as_ref().map(|request| {
        json!({
            "batch_id": request.batch_id,
            "request_id": request.request_id,
            "task_id": request.task_id,
            "client_id": request.client_id,
            "parent_task_id": request.parent_task_id,
            "brief_revision": request.brief_revision,
            "context_artifact": request.context_artifact,
        })
    });
    let accepted_brief_revision = routed_request
        .as_ref()
        .map_or(1, |request| request.brief_revision);
    let record = json!({
        "version": 1,
        "run": id,
        "workflow": definition.name,
        "definition_path": path,
        "definition_sha256": digest,
        "workflow_revision": digest,
        "plan_revision": 1,
        "accepted_brief_revision": accepted_brief_revision,
        "request_correlation": request_correlation,
        "dependencies": dependencies,
        "project": selected,
        "repo": repo,
        "home": home_root().canonicalize().map_err(|error| error.to_string())?,
        "status": "running",
        "current_stage": Value::Null,
        "message": "workflow launched",
        "created_at": timestamp,
        "updated_at": timestamp,
    });
    write_json(&directory.join("run.json"), &record)?;
    register_backlog(&id, &definition, &backlog_project, &dependencies)?;
    println!("launched: {id}");
    let reconcile_result = reconcile_locked(&directory);
    render_status(&directory)?;
    reconcile_result
}

fn verify_tracked(path: &Path) -> Result<(), String> {
    let root = source_root()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let workflows = root.join("workflows");
    if !path.starts_with(&workflows)
        || path.extension().and_then(|value| value.to_str()) != Some("md")
    {
        return Err(format!(
            "runnable definitions must live under {}/workflows",
            root.display()
        ));
    }
    let relative = path
        .strip_prefix(&root)
        .map_err(|error| error.to_string())?;
    let status = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "--error-unmatch"])
        .arg(relative)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!(
            "runnable definition is not repo-tracked: {}",
            relative.display()
        ));
    }
    Ok(())
}

fn register_backlog(
    id: &str,
    definition: &Definition,
    project: &str,
    dependencies: &[String],
) -> Result<(), String> {
    let path = data_root().join("backlog.md");
    if !path.is_file() {
        return Ok(());
    }
    let store = BacklogStore::new(path);
    if store.snapshot(id).is_ok() {
        return Err(format!("backlog identity already exists: {id}"));
    }
    store
        .add(&AddRequest {
            id,
            title: &format!("Workflow {}: {}", definition.name, definition.description),
            repo: project,
            kind: "delivery",
            body: "",
            start: true,
            blockers: dependencies,
        })
        .map_err(|error| error.to_string())
}

fn status(values: &[String]) -> Result<(), String> {
    let [id] = values else {
        return Err("status requires one run id".to_owned());
    };
    render_status(&run_directory(id)?)
}

fn resume(values: &[String]) -> Result<(), String> {
    let [id] = values else {
        return Err("resume requires one run id".to_owned());
    };
    let directory = run_directory(id)?;
    let result = reconcile_locked(&directory);
    render_status(&directory)?;
    result
}

fn reconcile_locked(directory: &Path) -> Result<(), String> {
    let _lock = DirectoryLock::try_acquire(
        directory.join(".reconcile.lock"),
        &SystemProcessProbe::default(),
    )
    .map_err(|_| "workflow run is already being reconciled".to_owned())?;
    reconcile(directory)
}

fn abort(values: &[String]) -> Result<(), String> {
    let [id] = values else {
        return Err("abort requires one run id".to_owned());
    };
    let directory = run_directory(id)?;
    let _lock = DirectoryLock::try_acquire(
        directory.join(".reconcile.lock"),
        &SystemProcessProbe::default(),
    )
    .map_err(|_| "workflow run is already being reconciled".to_owned())?;
    let record = read_json(&directory.join("run.json"))?;
    match record["status"].as_str().unwrap_or_default() {
        "completed" => return Err("completed run cannot be aborted".to_owned()),
        "aborted" => return Err("run is already aborted".to_owned()),
        _ => {}
    }
    set_run_state(
        &directory,
        "aborted",
        record["current_stage"].as_str().unwrap_or_default(),
        "workflow permanently aborted",
    )?;
    let backlog = data_root().join("backlog.md");
    if backlog.is_file() {
        BacklogStore::new(backlog)
            .hold(id, "workflow aborted", "workflow")
            .map_err(|_| "could not park aborted workflow in backlog".to_owned())?;
    }
    println!("aborted: {id}");
    Ok(())
}

fn definition(directory: &Path) -> Result<Definition, String> {
    serde_json::from_value(read_json(&directory.join("definition.json"))?)
        .map_err(|error| error.to_string())
}

fn stage_order(directory: &Path, definition: &Definition) -> Result<Vec<String>, String> {
    let path = directory.join("stage-order.json");
    let order: Vec<String> = serde_json::from_value(read_json(&path)?)
        .map_err(|_| "stage order does not match the immutable definition".to_owned())?;
    let statuses = stage_statuses(directory, definition)?;
    RunState::from_records(definition, order.clone(), statuses)
        .map_err(|error| error.to_string())?;
    Ok(order)
}

fn stage_statuses(
    directory: &Path,
    definition: &Definition,
) -> Result<BTreeMap<String, StageStatus>, String> {
    let mut statuses = BTreeMap::new();
    for stage in &definition.stages {
        let path = directory.join("stages").join(format!("{}.json", stage.id));
        if path.is_file() {
            statuses.insert(
                stage.id.clone(),
                parse_stage_status(read_json(&path)?["status"].as_str().unwrap_or("pending"))?,
            );
        }
    }
    Ok(statuses)
}

fn parse_stage_status(value: &str) -> Result<StageStatus, String> {
    match value {
        "pending" => Ok(StageStatus::Pending),
        "running" => Ok(StageStatus::Running),
        "ready" => Ok(StageStatus::Ready),
        "waiting-agent" => Ok(StageStatus::WaitingAgent),
        "waiting-external" => Ok(StageStatus::WaitingExternal),
        "waiting-failure" => Ok(StageStatus::WaitingFailure),
        "waiting-approval" => Ok(StageStatus::WaitingApproval),
        "done" => Ok(StageStatus::Done),
        "passed" => Ok(StageStatus::Passed),
        "failed" => Ok(StageStatus::Failed),
        "skipped" => Ok(StageStatus::Skipped),
        other => Err(format!("invalid stage status: {other}")),
    }
}

fn stage_status(directory: &Path, id: &str) -> Result<StageStatus, String> {
    let path = directory.join("stages").join(format!("{id}.json"));
    if !path.is_file() {
        return Ok(StageStatus::Pending);
    }
    parse_stage_status(read_json(&path)?["status"].as_str().unwrap_or("pending"))
}

fn set_run_state(directory: &Path, status: &str, stage: &str, message: &str) -> Result<(), String> {
    let mut record = read_json(&directory.join("run.json"))?;
    record["status"] = Value::String(status.to_owned());
    record["current_stage"] = if stage.is_empty() {
        Value::Null
    } else {
        Value::String(stage.to_owned())
    };
    record["message"] = Value::String(message.to_owned());
    record["updated_at"] = Value::String(now());
    write_json(&directory.join("run.json"), &record)
}

fn verify_snapshot(directory: &Path) -> Result<Definition, String> {
    let record = read_json(&directory.join("run.json"))?;
    let snapshot = directory.join("definition.workflow.md");
    let normalized = directory.join("definition.json");
    if !snapshot.is_file() || !normalized.is_file() {
        return Err("definition snapshot is missing or unsafe".to_owned());
    }
    let bytes = fs::read(&snapshot).map_err(|error| error.to_string())?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if record["definition_sha256"].as_str() != Some(&actual) {
        return Err("definition snapshot digest changed after launch".to_owned());
    }
    let parsed = workflow::parse(&snapshot).map_err(|error| error.to_string())?;
    let recorded: Definition =
        serde_json::from_value(read_json(&normalized)?).map_err(|error| error.to_string())?;
    if parsed != recorded {
        return Err("normalized definition changed after launch".to_owned());
    }
    Ok(recorded)
}

fn output_path(directory: &Path, stage: &Stage) -> Result<Option<PathBuf>, String> {
    let Some(declared) = &stage.output else {
        return Ok(None);
    };
    let run = read_json(&directory.join("run.json"))?;
    workflow::output_path(
        Path::new(run["home"].as_str().unwrap_or_default()),
        declared,
        run["run"].as_str().unwrap_or_default(),
    )
    .map(Some)
    .map_err(|error| error.to_string())
}

fn contract_check(directory: &Path, stage: &Stage) -> Result<(), String> {
    let record_path = directory.join("stages").join(format!("{}.json", stage.id));
    if stage.kind == StageType::Agent
        && stage.executor == Some(Executor::SubAgentSession)
        && record_path.is_file()
    {
        let run = read_json(&directory.join("run.json"))?;
        let record = read_json(&record_path)?;
        verify_stage_task_binding(&run, stage, &record)?;
    }
    if stage.kind == StageType::Command {
        let record = read_json(&record_path)?;
        let stdout = directory
            .join("commands")
            .join(format!("{}.stdout", stage.id));
        let stderr = directory
            .join("commands")
            .join(format!("{}.stderr", stage.id));
        if record["exit_code"].as_i64() != Some(0)
            || record["stdout"].as_str() != stdout.to_str()
            || record["stderr"].as_str() != stderr.to_str()
            || !stdout.is_file()
            || !stderr.is_file()
        {
            return Err("command execution record is not a captured zero exit".to_owned());
        }
    }
    if let Some(output) = output_path(directory, stage)?
        && (!output.is_file() || fs::metadata(&output).map(|meta| meta.len()).unwrap_or(0) == 0)
    {
        return Err("stage contract is unmet".to_owned());
    }
    if stage.contract == Some(Contract::LocalCommits) {
        let record = read_json(&record_path)?;
        let task = record["task_id"].as_str().unwrap_or_default();
        let worktree = Path::new(record["worktree"].as_str().unwrap_or_default());
        let fork = record["fork_sha"].as_str().unwrap_or_default();
        let head = git_line(worktree, &["rev-parse", "--verify", "HEAD"])
            .ok_or("sub-agent local-commit contract is unmet")?;
        let branch = git_line(worktree, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .ok_or("sub-agent local-commit contract is unmet")?;
        if branch != format!("mx/{task}")
            || head == fork
            || !git_success(worktree, &["merge-base", "--is-ancestor", fork, &head])
        {
            return Err("sub-agent local-commit contract is unmet".to_owned());
        }
    }
    Ok(())
}

fn prompt_file(
    directory: &Path,
    definition: &Definition,
    stage: &Stage,
) -> Result<PathBuf, String> {
    let run_record = read_json(&directory.join("run.json"))?;
    let run = run_record["run"].as_str().unwrap_or_default();
    let input =
        fs::read_to_string(directory.join("input.txt")).map_err(|error| error.to_string())?;
    let output = output_path(directory, stage)?
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut body = format!(
        "# Workflow stage: {}\n\nRun: `{run}`\n\n{}\n",
        stage.title,
        workflow::substitute(&stage.body, run, &input, &output)
    );
    if let Some(correlation) = run_record.get("request_correlation")
        && !correlation.is_null()
    {
        body.push_str(&format!(
            "\n## Accepted request\n\nBatch: `{}`\n\nRequest: `{}`\n\nTask: `{}`\n\nAccepted brief revision: `{}`\n",
            correlation["batch_id"].as_str().unwrap_or_default(),
            correlation["request_id"].as_str().unwrap_or_default(),
            correlation["task_id"].as_str().unwrap_or_default(),
            correlation["brief_revision"].as_u64().unwrap_or(1),
        ));
        if let Some(artifact) = correlation["context_artifact"].as_str() {
            body.push_str(&format!("\nOriginal context artifact: `{artifact}`\n"));
        }
    }
    for source in &stage.brief_from {
        let source_stage = definition
            .stages
            .iter()
            .find(|candidate| &candidate.id == source)
            .ok_or("inherited stage is absent")?;
        let path = output_path(directory, source_stage)?.ok_or("inherited stage has no output")?;
        body.push_str(&format!(
            "\n## Inherited artifact: {source}\n\nPath: `{}`\n\n",
            path.display()
        ));
        body.push_str(
            &fs::read_to_string(&path).unwrap_or_else(|_| "[artifact missing]\n".to_owned()),
        );
    }
    let path = directory.join("prompts").join(format!("{}.md", stage.id));
    atomic_replace(&path, body.as_bytes(), 0o600).map_err(|error| error.to_string())?;
    Ok(path)
}

fn reconcile(directory: &Path) -> Result<(), String> {
    let run_record = read_json(&directory.join("run.json"))?;
    match run_record["status"].as_str().unwrap_or_default() {
        "aborted" => {
            return Err(format!(
                "run {} is permanently aborted",
                run_record["run"].as_str().unwrap_or_default()
            ));
        }
        "completed" => return Ok(()),
        _ => {}
    }
    let definition = verify_snapshot(directory)?;
    if !dependencies_ready(directory)? {
        return Ok(());
    }
    let order = stage_order(directory, &definition)?;
    for id in order {
        let definition = verify_snapshot(directory)?;
        let stage = definition
            .stages
            .iter()
            .find(|stage| stage.id == id)
            .ok_or("workflow stage is absent")?;
        let status = stage_status(directory, &id)?;
        if status == StageStatus::Skipped {
            continue;
        }
        if status == StageStatus::Passed {
            contract_check(directory, stage).map_err(|_| {
                let _ = set_run_state(
                    directory,
                    "failed",
                    &id,
                    "passed stage contract no longer holds",
                );
                "passed stage contract no longer holds".to_owned()
            })?;
            continue;
        }
        set_run_state(
            directory,
            "running",
            &id,
            &format!("reconciling stage {id}"),
        )?;
        journal_stage(directory, &id, "entered", "");
        match stage.kind {
            StageType::Interactive => {
                if status == StageStatus::Pending {
                    let prompt = prompt_file(directory, &definition, stage)?;
                    write_json(
                        &directory.join("stages").join(format!("{id}.json")),
                        &json!({"id": id, "status": "ready", "prompt": prompt, "started_at": now()}),
                    )?;
                }
                match gate_stage(directory, stage)? {
                    true => mark_passed(directory, &id)?,
                    false => {
                        set_run_state(
                            directory,
                            "waiting",
                            &id,
                            &format!(
                                "maintainer approval required; resolve {id} through mx-decision-hold"
                            ),
                        )?;
                        journal_stage(directory, &id, "gated", "waiting");
                        return Ok(());
                    }
                }
            }
            StageType::Agent => {
                if stage.executor == Some(Executor::OrchestratorContext) {
                    if !matches!(status, StageStatus::Done | StageStatus::WaitingApproval) {
                        execute_orchestrator_context(directory, &definition, stage)?;
                    }
                } else {
                    match status {
                        StageStatus::Pending => {
                            execute_subagent(directory, &definition, stage)?;
                            set_run_state(
                                directory,
                                "waiting",
                                &id,
                                "sub-agent stage is still running",
                            )?;
                            return Ok(());
                        }
                        StageStatus::WaitingAgent
                        | StageStatus::Done
                        | StageStatus::WaitingApproval => {
                            if !reconcile_subagent(directory, stage)? {
                                set_run_state(
                                    directory,
                                    "waiting",
                                    &id,
                                    "sub-agent stage is still running",
                                )?;
                                return Ok(());
                            }
                        }
                        StageStatus::Failed => return Err("sub-agent reported failure".to_owned()),
                        _ => {
                            return Err(format!(
                                "stage {id} has an incomplete sub-agent launch record"
                            ));
                        }
                    }
                }
                contract_check(directory, stage).map_err(|_| {
                    let _ = set_run_state(directory, "failed", &id, "stage contract is unmet");
                    "stage contract is unmet".to_owned()
                })?;
                if stage.gate == Gate::Approve && !gate_stage(directory, stage)? {
                    set_run_state(directory, "waiting", &id, "maintainer approval required")?;
                    return Ok(());
                }
                mark_passed(directory, &id)?;
            }
            StageType::Command => {
                let execution = match status {
                    StageStatus::Done | StageStatus::WaitingApproval => Ok(()),
                    StageStatus::WaitingFailure => {
                        match hold_state(directory, &format!("{id}-failure"))? {
                            HoldState::Resolved => execute_command(directory, &definition, stage),
                            HoldState::Open => {
                                set_run_state(
                                    directory,
                                    "waiting",
                                    &id,
                                    "command failure awaits maintainer decision",
                                )?;
                                return Ok(());
                            }
                            HoldState::Absent => {
                                return Err("command failure hold is missing or invalid".to_owned());
                            }
                        }
                    }
                    StageStatus::WaitingExternal => execute_command(directory, &definition, stage),
                    StageStatus::Failed => return Err("command stage failed".to_owned()),
                    _ => execute_command(directory, &definition, stage),
                };
                match execution {
                    Ok(()) => {}
                    Err(CommandFailure::External) => {
                        set_run_state(
                            directory,
                            "waiting",
                            &id,
                            "command is waiting on its composed lifecycle",
                        )?;
                        return Ok(());
                    }
                    Err(CommandFailure::Exit) => {
                        if status == StageStatus::WaitingFailure {
                            set_run_state(
                                directory,
                                "failed",
                                &id,
                                "command failed again after the accepted retry",
                            )?;
                            return Err("command failed again after the accepted retry".to_owned());
                        }
                        create_hold(
                            directory,
                            &format!("{id}-failure"),
                            &format!("Workflow command {id} failed"),
                            &format!("workflow command {id} failed; inspect captured output"),
                        )?;
                        attach_command_failure(directory, stage)?;
                        set_run_state(
                            directory,
                            "waiting",
                            &id,
                            &format!("command failed; inspect commands/{id}.stderr"),
                        )?;
                        return Ok(());
                    }
                }
                if stage.gate == Gate::Approve && !gate_stage(directory, stage)? {
                    set_run_state(directory, "waiting", &id, "maintainer approval required")?;
                    return Ok(());
                }
                mark_passed(directory, &id)?;
            }
        }
        journal_stage(directory, &id, "gated", "passed");
    }
    complete_backlog(directory)?;
    set_run_state(directory, "completed", "", "workflow completed")
}

fn execute_orchestrator_context(
    directory: &Path,
    definition: &Definition,
    stage: &Stage,
) -> Result<(), String> {
    let prompt = prompt_file(directory, definition, stage)?;
    let schema = directory.join("schemas/agent-result.json");
    let output = directory.join("agents").join(format!("{}.json", stage.id));
    let session = directory
        .join("agents")
        .join(format!("{}.session", stage.id));
    atomic_replace(&schema, br#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"required":["status","message"],"properties":{"status":{"type":"string","enum":["done","failed"]},"message":{"type":"string","minLength":1,"maxLength":12000}}}"#, 0o600).map_err(|error| error.to_string())?;
    write_json(
        &directory.join("stages").join(format!("{}.json", stage.id)),
        &json!({"id": stage.id, "status": "running", "started_at": now(), "prompt": prompt}),
    )?;
    let command = std::env::var_os("MX_WORKFLOW_AGENT_COMMAND")
        .ok_or("headless workflow agent adapter is unavailable")?;
    let status =
        crate::agent_transport::structured_command(command, &schema, &prompt, &output, &session)
            .status()
            .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("orchestrator-context agent stage failed".to_owned());
    }
    let result = read_json(&output)?;
    let keys = result
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let message = result["message"].as_str().unwrap_or_default();
    if keys.len() != 2
        || !keys.contains(&"status".to_owned())
        || !keys.contains(&"message".to_owned())
        || message.is_empty()
        || message.len() > 12000
        || !matches!(result["status"].as_str(), Some("done" | "failed"))
    {
        return Err(format!(
            "stage {} returned invalid structured completion",
            stage.id
        ));
    }
    let state = result["status"].as_str().unwrap_or("failed");
    write_json(
        &directory.join("stages").join(format!("{}.json", stage.id)),
        &json!({
            "id": stage.id, "status": state, "finished_at": now(), "prompt": prompt,
            "result": output, "session_id": fs::read_to_string(session).unwrap_or_default().trim()
        }),
    )?;
    if state == "done" {
        Ok(())
    } else {
        Err("orchestrator-context agent stage failed".to_owned())
    }
}

fn execute_subagent(
    directory: &Path,
    definition: &Definition,
    stage: &Stage,
) -> Result<(), String> {
    let run = read_json(&directory.join("run.json"))?;
    let run_id = run["run"].as_str().unwrap_or_default();
    let used = definition
        .stages
        .iter()
        .filter_map(|candidate| {
            let path = directory
                .join("stages")
                .join(format!("{}.json", candidate.id));
            path.is_file()
                .then(|| {
                    read_json(&path)
                        .ok()?
                        .get("task_id")?
                        .as_str()
                        .map(ToOwned::to_owned)
                })
                .flatten()
        })
        .any(|task| task == run_id);
    let task = if used {
        format!("{run_id}-{}", stage.id)
    } else {
        run_id.to_owned()
    };
    let prompt = prompt_file(directory, definition, stage)?;
    let brief = Path::new(run["home"].as_str().unwrap_or_default())
        .join("data")
        .join(&task)
        .join("brief.md");
    fs::create_dir_all(brief.parent().expect("brief parent")).map_err(|error| error.to_string())?;
    let assignment = stage.assignment.unwrap_or(Assignment::Implementer);
    let assignment_name = match assignment {
        Assignment::Researcher => "researcher",
        Assignment::Implementer => "implementer",
        Assignment::Reviewer => "reviewer",
        Assignment::SubOrchestrator => "sub-orchestrator",
    };
    let workspace_rule = if assignment == Assignment::SubOrchestrator {
        "Delegate implementation and test-code changes to bounded children. Keep one stage owner, route human questions through the parent channel, and do not treat a child summary as stage completion."
    } else {
        "Verify that `pwd -P` and `git rev-parse --show-toplevel` identify the exact isolated allocation supplied by Multplx rather than its primary checkout. Stop and report `blocked` if isolation is not genuine. Work only in that allocation, except for the stage's explicitly declared output path and validated status command."
    };
    let text = format!(
        "You are the {assignment_name} assigned to one selected Multplx workflow stage.\n\n# Stage charter\n\n{}\n\n# Workflow execution rules\n\nRun: `{run_id}`.\nWorkflow revision: `{}`.\nPlan revision: `{}`.\nAccepted workflow brief revision: `{}`.\nProject: `{}`.\nCheckout: `{}`.\nStarting revision: `{}`.\n{workspace_rule}\nFollow only this stage's accepted scope, outputs and explicit interaction points. Later workflow stages remain out of scope.\nYou may delegate within this stage; the stage remains active until its declared contract is met.\nOnly humans merge PRs; never merge, enable auto-merge, enqueue a merge or push the PR result to the remote target branch.\nDeep-review and vplan run only when this stage explicitly declares them.\nReport completion through the validated status path:\n\n`{}/bin/mx-report --id {task} --state done --message \"workflow stage complete at {{full commit SHA}}\"`\n",
        fs::read_to_string(&prompt).unwrap_or_default(),
        run["workflow_revision"].as_str().unwrap_or_default(),
        run["plan_revision"].as_u64().unwrap_or(1),
        run["accepted_brief_revision"].as_u64().unwrap_or(1),
        run["project"]["project_id"].as_str().unwrap_or_default(),
        run["project"]["checkout_id"].as_str().unwrap_or_default(),
        run["project"]["starting_revision"]
            .as_str()
            .unwrap_or_default(),
        runtime_root().display()
    );
    atomic_replace(&brief, text.as_bytes(), 0o600).map_err(|error| error.to_string())?;
    write_json(
        &directory.join("stages").join(format!("{}.json", stage.id)),
        &json!({"id": stage.id, "status": "running", "started_at": now(), "task_id": task, "brief": brief}),
    )?;
    let injected_spawn = std::env::var_os("MX_WORKFLOW_SPAWN_COMMAND");
    let status = if let Some(command) = injected_spawn.as_ref() {
        let mut command = Command::new(command);
        command.args([&task, run["repo"].as_str().unwrap_or_default()]);
        if assignment == Assignment::SubOrchestrator {
            command.args([
                "--sub-orchestrator",
                "--scope",
                &format!("Workflow {run_id} stage {}", stage.id),
            ]);
        } else {
            command.args(["--role", assignment_name]);
        }
        command.status()
    } else {
        let mut command = Command::new(runtime_root().join("bin/mx-spawn.sh"));
        if assignment == Assignment::SubOrchestrator {
            command.args([
                &task,
                "--sub-orchestrator",
                "--project",
                run["repo"].as_str().unwrap_or_default(),
                "--scope",
                &format!(
                    "Workflow {run_id} stage {} at plan revision {}",
                    stage.id,
                    run["plan_revision"].as_u64().unwrap_or(1)
                ),
                "--request-id",
                &format!("workflow-{run_id}-{}", stage.id),
                "--json",
            ]);
        } else {
            command.args([&task, run["repo"].as_str().unwrap_or_default()]);
            command.args(["--role", assignment_name]);
            command.args([
                "--output",
                if assignment == Assignment::Implementer {
                    "implementation"
                } else {
                    "report"
                },
            ]);
        }
        if let Some(harness) = std::env::var_os("MX_WORKFLOW_SUBAGENT_HARNESS")
            .or_else(|| std::env::var_os("MX_WORKFLOW_ACTOR_HARNESS"))
        {
            command.arg("--harness").arg(harness);
        }
        command.status()
    }
    .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("sub-agent launch failed".to_owned());
    }
    let meta = Path::new(run["home"].as_str().unwrap_or_default())
        .join("state")
        .join(format!("{task}.meta"));
    let (task_binding, worktree, fork) = match read_canonical_task(&meta, &task) {
        Ok(record) if !record.legacy_unknown => canonical_stage_binding(&run, stage, &record)?,
        Ok(_) | Err(_) if injected_spawn.is_some() => {
            let worktree = metadata_value(&meta, "worktree");
            if assignment != Assignment::SubOrchestrator && !Path::new(&worktree).is_dir() {
                return Err(format!("sub-agent stage {} has no worktree", stage.id));
            }
            let fork = if worktree.is_empty() {
                String::new()
            } else {
                git_line(Path::new(&worktree), &["rev-parse", "--verify", "HEAD"])
                    .ok_or("sub-agent fork SHA is unavailable")?
            };
            (json!({"mocked": true, "task_id": task}), worktree, fork)
        }
        Ok(_) => return Err(
            "workflow stage task metadata is legacy and has no exact attempt or allocation binding"
                .to_owned(),
        ),
        Err(error) => return Err(error),
    };
    write_json(
        &directory.join("stages").join(format!("{}.json", stage.id)),
        &json!({
            "id": stage.id, "status": "waiting-agent", "started_at": now(), "task_id": task,
            "brief": brief, "worktree": worktree, "fork_sha": fork,
            "task_binding": task_binding, "fresh_session": stage.fresh_session.unwrap_or(false),
            "assignment": assignment_name
        }),
    )
}

fn read_canonical_task(
    meta: &Path,
    task: &str,
) -> Result<multplx_domain::lifecycle::subagent_model::TaskRecord, String> {
    let text = fs::read_to_string(meta)
        .map_err(|error| format!("canonical stage task metadata is unavailable: {error}"))?;
    multplx_domain::lifecycle::subagent_model::read_meta(task, &text)
        .map_err(|error| format!("canonical stage task metadata is invalid: {error}"))
}

fn canonical_stage_binding(
    run: &Value,
    stage: &Stage,
    record: &multplx_domain::lifecycle::subagent_model::TaskRecord,
) -> Result<(Value, String, String), String> {
    let attempt = record
        .attempt
        .as_ref()
        .ok_or("workflow stage task has no current attempt")?;
    let project = record
        .project
        .as_ref()
        .ok_or("workflow stage task has no project binding")?;
    if run.get("project").is_some() {
        if project.project_id != run["project"]["project_id"].as_str().unwrap_or_default()
            || project.checkout_id != run["project"]["checkout_id"].as_str().unwrap_or_default()
            || project.starting_revision
                != run["project"]["starting_revision"]
                    .as_str()
                    .unwrap_or_default()
        {
            return Err(
                "workflow stage task belongs to a different project, checkout or starting revision"
                    .to_owned(),
            );
        }
    } else if fs::canonicalize(run["repo"].as_str().unwrap_or_default()).ok()
        != fs::canonicalize(&project.canonical_path).ok()
    {
        return Err("legacy workflow stage task belongs to a different project path".to_owned());
    }
    let (allocation, worktree, fork) = if stage.assignment == Some(Assignment::SubOrchestrator) {
        if record.domain.is_none()
            || record.role
                != multplx_domain::lifecycle::subagent_model::AssignmentRole::SubOrchestrator
        {
            return Err("workflow coordinator stage has no bounded coordinator domain".to_owned());
        }
        (Value::Null, String::new(), String::new())
    } else {
        let allocation = record
            .allocation
            .as_ref()
            .ok_or("workflow stage task has no durable allocation")?;
        if allocation.task_id != record.task_id
            || allocation.attempt_id != attempt.id
            || allocation.project_id != project.project_id
            || allocation.checkout_id != project.checkout_id
            || allocation.base_revision != project.starting_revision
            || !Path::new(&allocation.path).is_dir()
        {
            return Err("workflow stage allocation is stale or foreign".to_owned());
        }
        (
            serde_json::to_value(allocation).map_err(|error| error.to_string())?,
            allocation.path.clone(),
            allocation.base_revision.clone(),
        )
    };
    Ok((
        json!({
            "task_id": record.task_id,
            "attempt": attempt,
            "accepted_brief_revision": record.accepted_brief_revision,
            "project": project,
            "allocation": allocation,
            "domain": record.domain,
            "owner_home": record.owner_home,
        }),
        worktree,
        fork,
    ))
}

fn reconcile_subagent(directory: &Path, stage: &Stage) -> Result<bool, String> {
    let record_path = directory.join("stages").join(format!("{}.json", stage.id));
    let mut record = read_json(&record_path)?;
    let task = record["task_id"].as_str().unwrap_or_default();
    let run = read_json(&directory.join("run.json"))?;
    verify_stage_task_binding(&run, stage, &record)?;
    let output = if let Some(command) = subagent_state_command() {
        Command::new(command).arg(task).output()
    } else {
        Command::new(runtime_root().join("bin/mx-actor-state.sh"))
            .arg(task)
            .env("MX_HOME", run["home"].as_str().unwrap_or_default())
            .env(
                "MX_STATE_OVERRIDE",
                Path::new(run["home"].as_str().unwrap_or_default()).join("state"),
            )
            .output()
    }
    .map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let state = text
        .strip_prefix("state: ")
        .and_then(|rest| rest.split([' ', '·']).next())
        .unwrap_or_default();
    match state {
        "done" => {
            contract_check(directory, stage).map_err(|_| {
                format!(
                    "sub-agent stage {} reported done before its contract was met",
                    stage.id
                )
            })?;
            record["status"] = Value::String("done".to_owned());
            record["finished_at"] = Value::String(now());
            write_json(&record_path, &record)?;
            Ok(true)
        }
        "failed" => {
            record["status"] = Value::String("failed".to_owned());
            record["finished_at"] = Value::String(now());
            write_json(&record_path, &record)?;
            Err("sub-agent reported failure".to_owned())
        }
        _ => Ok(false),
    }
}

fn verify_stage_task_binding(
    run: &Value,
    stage: &Stage,
    stage_record: &Value,
) -> Result<(), String> {
    if stage_record["task_binding"]["mocked"].as_bool() == Some(true) {
        if subagent_state_command().is_some() {
            return Ok(());
        }
        return Err(
            "mocked workflow stage binding is unavailable outside an injected test runtime"
                .to_owned(),
        );
    }
    let task = stage_record["task_id"]
        .as_str()
        .ok_or("workflow stage task identity is missing")?;
    let meta = Path::new(run["home"].as_str().unwrap_or_default())
        .join("state")
        .join(format!("{task}.meta"));
    let current = read_canonical_task(&meta, task)?;
    let (binding, worktree, fork) = canonical_stage_binding(run, stage, &current)?;
    let legacy_record = stage_record.get("task_binding").is_none();
    if (!legacy_record && binding != stage_record["task_binding"])
        || worktree != stage_record["worktree"].as_str().unwrap_or_default()
        || fork != stage_record["fork_sha"].as_str().unwrap_or_default()
    {
        return Err(
            "workflow stage task, attempt, brief, project or allocation binding changed".to_owned(),
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum CommandFailure {
    External,
    Exit,
}

fn workflow_git_value(directory: &Path, arguments: &[&str]) -> Option<String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(250);
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if !status.success() {
        return None;
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    let output = output.trim().to_owned();
    (!output.is_empty() && !output.contains(['\n', '\r'])).then_some(output)
}

fn workflow_merge_context(
    command: &str,
    worktree: &Path,
    state: &Path,
    task: &str,
) -> (Vec<String>, Option<String>) {
    let directory = multplx_core::command_policy::git_push_directory(command)
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                worktree.join(path)
            }
        })
        .unwrap_or_else(|| worktree.to_path_buf());
    let mut targets = vec!["main".to_owned(), "master".to_owned()];
    if let Some(remote) = workflow_git_value(
        &directory,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) && let Some(branch) = remote.strip_prefix("origin/")
        && !targets.iter().any(|target| target == branch)
    {
        targets.push(branch.to_owned());
    }
    if let Ok(bytes) = multplx_core::filesystem::read_bounded_regular(
        state.join(format!("{task}.ready-to-push")),
        64 * 1024,
    ) && let Ok(text) = String::from_utf8(bytes)
        && let Some(base) = text.lines().find_map(|line| line.strip_prefix("base="))
        && !base.is_empty()
        && !targets.iter().any(|target| target == base)
    {
        targets.push(base.to_owned());
    }
    let current = workflow_git_value(&directory, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    (targets, current)
}

fn execute_command(
    directory: &Path,
    definition: &Definition,
    stage: &Stage,
) -> Result<(), CommandFailure> {
    let run = read_json(&directory.join("run.json")).map_err(|_| CommandFailure::Exit)?;
    let run_id = run["run"].as_str().unwrap_or_default();
    let output = output_path(directory, stage).map_err(|_| CommandFailure::Exit)?;
    let output_shell = output
        .as_ref()
        .map(|path| shell_quote(&path.to_string_lossy()))
        .unwrap_or_default();
    let command = workflow::substitute(
        stage.run.as_deref().unwrap_or_default(),
        run_id,
        "",
        &output_shell,
    );
    let worktree = last_subagent_worktree(directory, definition)
        .unwrap_or_else(|| PathBuf::from(run["repo"].as_str().unwrap_or_default()));
    let stdout = directory
        .join("commands")
        .join(format!("{}.stdout", stage.id));
    let stderr = directory
        .join("commands")
        .join(format!("{}.stderr", stage.id));
    let record_path = directory.join("stages").join(format!("{}.json", stage.id));
    write_json(&record_path, &json!({"id": stage.id, "status": "running", "started_at": now(), "command": command, "cwd": worktree, "stdout": stdout, "stderr": stderr})).map_err(|_| CommandFailure::Exit)?;
    let task = last_subagent_task(directory, definition).unwrap_or_else(|| run_id.to_owned());
    let workflow_state = Path::new(run["home"].as_str().unwrap_or_default()).join("state");
    let (targets, current) = workflow_merge_context(&command, &worktree, &workflow_state, &task);
    if let Err(denial) =
        multplx_core::command_policy::remote_pr_merge(&command, &targets, current.as_deref())
    {
        fs::write(&stdout, b"").map_err(|_| CommandFailure::Exit)?;
        fs::write(
            &stderr,
            format!(
                "workflow remote merge guard: [{}] {}\n",
                denial.code, denial.reason
            ),
        )
        .map_err(|_| CommandFailure::Exit)?;
        let mut record = read_json(&record_path).map_err(|_| CommandFailure::Exit)?;
        record["finished_at"] = Value::String(now());
        record["exit_code"] = Value::from(3);
        write_json(&record_path, &record).map_err(|_| CommandFailure::Exit)?;
        return Err(CommandFailure::Exit);
    }
    let stdout_file = fs::File::create(&stdout).map_err(|_| CommandFailure::Exit)?;
    let stderr_file = fs::File::create(&stderr).map_err(|_| CommandFailure::Exit)?;
    let status = Command::new("bash")
        .args(["-lc", &command])
        .current_dir(&worktree)
        .env("MX_WORKFLOW_HOME", run["home"].as_str().unwrap_or_default())
        .env("MX_WORKFLOW_RUN", run_id)
        .env("MX_WORKFLOW_WORKTREE", &worktree)
        .env("MX_TASK_ID", &task)
        .stdout(stdout_file)
        .stderr(stderr_file)
        .status()
        .map_err(|_| CommandFailure::Exit)?;
    let mut record = read_json(&record_path).map_err(|_| CommandFailure::Exit)?;
    record["finished_at"] = Value::String(now());
    record["exit_code"] = Value::from(status.code().unwrap_or(1));
    write_json(&record_path, &record).map_err(|_| CommandFailure::Exit)?;
    if status.success() && contract_check(directory, stage).is_ok() {
        record["status"] = Value::String("done".to_owned());
        write_json(&record_path, &record).map_err(|_| CommandFailure::Exit)?;
        return Ok(());
    }
    if !task.is_empty() && subagent_waiting(&run, &task) {
        record["status"] = Value::String("waiting-external".to_owned());
        write_json(&record_path, &record).map_err(|_| CommandFailure::Exit)?;
        Err(CommandFailure::External)
    } else {
        record["status"] = Value::String("waiting-failure".to_owned());
        write_json(&record_path, &record).map_err(|_| CommandFailure::Exit)?;
        Err(CommandFailure::Exit)
    }
}

fn subagent_state_command() -> Option<OsString> {
    std::env::var_os("MX_WORKFLOW_SUBAGENT_STATE_COMMAND")
        .or_else(|| std::env::var_os("MX_WORKFLOW_ACTOR_STATE_COMMAND"))
}

fn subagent_waiting(run: &Value, task: &str) -> bool {
    let output = if let Some(command) = subagent_state_command() {
        Command::new(command).arg(task).output()
    } else {
        Command::new(runtime_root().join("bin/mx-actor-state.sh"))
            .arg(task)
            .env("MX_HOME", run["home"].as_str().unwrap_or_default())
            .output()
    };
    output
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
        .is_some_and(|text| {
            text.starts_with("state: parked")
                || text.starts_with("state: blocked")
                || text.starts_with("state: paused")
        })
}

fn last_subagent_worktree(directory: &Path, definition: &Definition) -> Option<PathBuf> {
    definition
        .stages
        .iter()
        .rev()
        .filter(|stage| {
            stage.kind == StageType::Agent
                && stage.executor == Some(Executor::SubAgentSession)
                && stage.assignment != Some(Assignment::SubOrchestrator)
        })
        .find_map(|stage| {
            let record =
                read_json(&directory.join("stages").join(format!("{}.json", stage.id))).ok()?;
            let path = PathBuf::from(record["worktree"].as_str()?);
            path.is_dir().then_some(path)
        })
}

fn last_subagent_task(directory: &Path, definition: &Definition) -> Option<String> {
    definition
        .stages
        .iter()
        .rev()
        .filter(|stage| {
            stage.kind == StageType::Agent
                && stage.executor == Some(Executor::SubAgentSession)
                && stage.assignment != Some(Assignment::SubOrchestrator)
        })
        .find_map(|stage| {
            read_json(&directory.join("stages").join(format!("{}.json", stage.id))).ok()?["task_id"]
                .as_str()
                .map(ToOwned::to_owned)
        })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HoldState {
    Absent,
    Open,
    Resolved,
}

fn hold_state(directory: &Path, key: &str) -> Result<HoldState, String> {
    let run = read_json(&directory.join("run.json"))?;
    let id = format!("{}-decision-{key}", run["run"].as_str().unwrap_or_default());
    let backlog = Path::new(run["home"].as_str().unwrap_or_default()).join("data/backlog.md");
    if !backlog.is_file() {
        return Ok(HoldState::Absent);
    }
    match BacklogStore::new(backlog).snapshot(&id) {
        Ok(item) if item.state == "done" => Ok(HoldState::Resolved),
        Ok(item) if item.state == "queued" => Ok(HoldState::Open),
        Ok(_) => Err("approval hold is invalid".to_owned()),
        Err(_) => Ok(HoldState::Absent),
    }
}

fn create_hold(directory: &Path, key: &str, title: &str, reason: &str) -> Result<(), String> {
    let run = read_json(&directory.join("run.json"))?;
    let run_id = run["run"].as_str().unwrap_or_default();
    let home = Path::new(run["home"].as_str().unwrap_or_default());
    let workflow_revision = format!(
        "{}:plan-{}",
        run["workflow_revision"]
            .as_str()
            .or_else(|| run["definition_sha256"].as_str())
            .unwrap_or_default(),
        run["plan_revision"].as_u64().unwrap_or(1)
    );
    let brief_revision = run["accepted_brief_revision"]
        .as_u64()
        .unwrap_or(1)
        .to_string();
    let output = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
        .args([
            "authority",
            "mx-decision-hold.sh",
            "hold",
            run_id,
            key,
            "--title",
            title,
            "--reason",
            reason,
            "--repo",
            run["project"]["project_id"].as_str().unwrap_or("workflow"),
            "--task",
            run_id,
            "--brief-revision",
            &brief_revision,
            "--workflow-revision",
            &workflow_revision,
            "--question",
            title,
        ])
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", home.join("state"))
        .env("MX_DATA_OVERRIDE", home.join("data"))
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let store = BacklogStore::new(home.join("data/backlog.md"));
    let hold = format!("{run_id}-decision-{key}");
    let item = store.snapshot(run_id).map_err(|error| error.to_string())?;
    if !item.blockers.iter().any(|blocker| blocker == &hold) {
        store
            .block(run_id, &hold)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn record_resolved_decision(directory: &Path, stage: &Stage) -> Result<(), String> {
    let run = read_json(&directory.join("run.json"))?;
    let run_id = run["run"].as_str().unwrap_or_default();
    let hold_id = format!("{run_id}-decision-{}", stage.id);
    let backlog = Path::new(run["home"].as_str().unwrap_or_default()).join("data/backlog.md");
    let item = BacklogStore::new(backlog)
        .snapshot(&hold_id)
        .map_err(|error| error.to_string())?;
    let field = |label: &str| {
        item.body
            .lines()
            .find_map(|line| line.strip_prefix(label))
            .map(str::to_owned)
    };
    let expected_workflow = format!(
        "{}:plan-{}",
        run["workflow_revision"]
            .as_str()
            .or_else(|| run["definition_sha256"].as_str())
            .unwrap_or_default(),
        run["plan_revision"].as_u64().unwrap_or(1)
    );
    if field("Target task: ").as_deref() != Some(run_id)
        || field("Target brief revision: ").and_then(|value| value.parse::<u64>().ok())
            != Some(run["accepted_brief_revision"].as_u64().unwrap_or(1))
        || field("Target workflow revision: ").as_deref() != Some(&expected_workflow)
        || field("Question: ").as_deref() != Some(stage.title.as_str())
    {
        return Err("resolved decision belongs to a stale workflow target".to_owned());
    }
    let answer = item
        .body
        .split_once("\n\nMaintainer decision:\n")
        .and_then(|(_, rest)| {
            rest.split_once("\n\nRouted work:")
                .map(|(answer, _)| answer)
        })
        .filter(|answer| !answer.trim().is_empty())
        .ok_or("resolved workflow decision has no durable answer")?;
    let record = json!({
        "decision_id": hold_id,
        "task_id": run_id,
        "brief_revision": run["accepted_brief_revision"],
        "workflow_revision": expected_workflow,
        "stage": stage.id,
        "question": stage.title,
        "answer": answer,
        "resolved_at": now(),
    });
    let decisions = directory.join("decisions");
    fs::create_dir_all(&decisions).map_err(|error| error.to_string())?;
    let path = decisions.join(format!("{}.json", stage.id));
    if path.is_file() {
        let existing = read_json(&path)?;
        for key in [
            "decision_id",
            "task_id",
            "brief_revision",
            "workflow_revision",
            "stage",
            "question",
            "answer",
        ] {
            if existing[key] != record[key] {
                return Err("workflow decision retry conflicts with its durable answer".to_owned());
            }
        }
        return Ok(());
    }
    write_json(&path, &record)
}

fn gate_stage(directory: &Path, stage: &Stage) -> Result<bool, String> {
    if stage.gate != Gate::Approve {
        return Ok(true);
    }
    match hold_state(directory, &stage.id)? {
        HoldState::Resolved => {
            record_resolved_decision(directory, stage)?;
            contract_check(directory, stage)?;
            Ok(true)
        }
        HoldState::Open => Ok(false),
        HoldState::Absent => {
            create_hold(
                directory,
                &stage.id,
                &stage.title,
                &format!("workflow stage {} awaits approval", stage.id),
            )?;
            let path = directory.join("stages").join(format!("{}.json", stage.id));
            let mut record = if path.is_file() {
                read_json(&path)?
            } else {
                json!({"id": stage.id})
            };
            record["status"] = Value::String("waiting-approval".to_owned());
            record["gate_opened_at"] = Value::String(now());
            write_json(&path, &record)?;
            Ok(false)
        }
    }
}

fn mark_passed(directory: &Path, id: &str) -> Result<(), String> {
    let path = directory.join("stages").join(format!("{id}.json"));
    let mut record = if path.is_file() {
        read_json(&path)?
    } else {
        json!({"id": id})
    };
    record["status"] = Value::String("passed".to_owned());
    record["passed_at"] = Value::String(now());
    write_json(&path, &record)
}

fn attach_command_failure(directory: &Path, stage: &Stage) -> Result<(), String> {
    let run = read_json(&directory.join("run.json"))?;
    let run_id = run["run"].as_str().unwrap_or_default();
    let record = read_json(&directory.join("stages").join(format!("{}.json", stage.id)))?;
    let stdout = record["stdout"].as_str().unwrap_or_default();
    let stderr = record["stderr"].as_str().unwrap_or_default();
    let workflow_revision = format!(
        "{}:plan-{}",
        run["workflow_revision"]
            .as_str()
            .or_else(|| run["definition_sha256"].as_str())
            .unwrap_or_default(),
        run["plan_revision"].as_u64().unwrap_or(1)
    );
    let tail = |path: &str| {
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .rev()
            .take(50)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    };
    let body = format!(
        "Origin: {run_id}\nDecision key: {}-failure\nState: awaiting maintainer decision.\nTarget task: {run_id}\nTarget brief revision: {}\nTarget workflow revision: {workflow_revision}\nQuestion: Workflow command {} failed\nCommand exit: {}\nCaptured stdout: {stdout}\nCaptured stderr: {stderr}\n\nStdout tail:\n{}\n\nStderr tail:\n{}",
        stage.id,
        run["accepted_brief_revision"].as_u64().unwrap_or(1),
        stage.id,
        record["exit_code"],
        if tail(stdout).is_empty() {
            "[empty]".to_owned()
        } else {
            tail(stdout)
        },
        if tail(stderr).is_empty() {
            "[empty]".to_owned()
        } else {
            tail(stderr)
        }
    );
    BacklogStore::new(Path::new(run["home"].as_str().unwrap_or_default()).join("data/backlog.md"))
        .update(
            &format!("{run_id}-decision-{}-failure", stage.id),
            &body,
            false,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn complete_backlog(directory: &Path) -> Result<(), String> {
    let run = read_json(&directory.join("run.json"))?;
    let path = Path::new(run["home"].as_str().unwrap_or_default()).join("data/backlog.md");
    if !path.is_file() {
        return Ok(());
    }
    let store = BacklogStore::new(path);
    let id = run["run"].as_str().unwrap_or_default();
    if store.snapshot(id).map_err(|error| error.to_string())?.state != "done" {
        store
            .done(id, None, 20)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn render_status(directory: &Path) -> Result<(), String> {
    let run = read_json(&directory.join("run.json"))?;
    let definition = definition(directory)?;
    println!("run: {}", run["run"].as_str().unwrap_or_default());
    println!("workflow: {}", run["workflow"].as_str().unwrap_or_default());
    println!("status: {}", run["status"].as_str().unwrap_or_default());
    println!(
        "plan_revision: {}",
        run["plan_revision"].as_u64().unwrap_or(1)
    );
    if let Some(project) = run.get("project") {
        println!(
            "project: {} checkout={} start={}",
            project["project_id"].as_str().unwrap_or_default(),
            project["checkout_id"].as_str().unwrap_or_default(),
            project["starting_revision"].as_str().unwrap_or_default()
        );
    }
    if let Some(dependencies) = run["dependencies"].as_array()
        && !dependencies.is_empty()
    {
        println!(
            "dependencies: {}",
            dependencies
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    if let Some(correlation) = run.get("request_correlation")
        && !correlation.is_null()
    {
        println!(
            "request: {} batch={} brief={}",
            correlation["request_id"].as_str().unwrap_or_default(),
            correlation["batch_id"].as_str().unwrap_or_default(),
            correlation["brief_revision"].as_u64().unwrap_or(1)
        );
    }
    println!(
        "current_stage: {}",
        run["current_stage"].as_str().unwrap_or("-")
    );
    println!("message: {}", run["message"].as_str().unwrap_or_default());
    println!(
        "definition_sha256: {}",
        run["definition_sha256"].as_str().unwrap_or_default()
    );
    println!("stages:");
    for id in stage_order(directory, &definition)? {
        println!(
            "  {id}: {}",
            stage_status_name(stage_status(directory, &id)?)
        );
        let stage = definition
            .stages
            .iter()
            .find(|stage| stage.id == id)
            .expect("stage");
        if let Some(output) = output_path(directory, stage)? {
            println!("    output: {}", output.display());
        }
        let record_path = directory.join("stages").join(format!("{id}.json"));
        if record_path.is_file() {
            let record = read_json(&record_path)?;
            for (key, label) in [("task_id", "task_id"), ("prompt", "prompt")] {
                if let Some(value) = record[key].as_str() {
                    println!("    {label}: {value}");
                }
            }
            if let Some(stdout) = record["stdout"].as_str() {
                println!(
                    "    stdout: {stdout}\n    stderr: {}",
                    record["stderr"].as_str().unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}

fn stage_status_name(status: StageStatus) -> &'static str {
    match status {
        StageStatus::Pending => "pending",
        StageStatus::Running => "running",
        StageStatus::Ready => "ready",
        StageStatus::WaitingAgent => "waiting-agent",
        StageStatus::WaitingExternal => "waiting-external",
        StageStatus::WaitingFailure => "waiting-failure",
        StageStatus::WaitingApproval => "waiting-approval",
        StageStatus::Done => "done",
        StageStatus::Passed => "passed",
        StageStatus::Failed => "failed",
        StageStatus::Skipped => "skipped",
    }
}

fn consume_override(
    mode: &str,
    run: &str,
    stage: &str,
    before: Option<&str>,
    request: &str,
) -> Result<(), String> {
    let mut args = vec![mode.to_owned(), run.to_owned(), stage.to_owned()];
    if let Some(before) = before {
        args.push(before.to_owned());
    }
    let value = crate::authority::override_bindings(&args)?;
    let text = |field: &str| value[field].as_str().unwrap_or_default();
    OverrideStore::new(&state_root())
        .consume(
            request,
            &Binding {
                boundary: text("boundary"),
                task: text("task"),
                project: text("project"),
                operation: text("operation"),
                target: text("target"),
                expected_state_digest: text("expected_state_digest"),
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn record_plan_change(
    directory: &Path,
    operation: &str,
    stage: &str,
    before: Option<&str>,
    request: &str,
) -> Result<(), String> {
    let mut run = read_json(&directory.join("run.json"))?;
    let previous = run["plan_revision"].as_u64().unwrap_or(1);
    let revision = previous
        .checked_add(1)
        .ok_or("workflow plan revision exhausted")?;
    let path = directory.join("plan-history.json");
    let mut history = if path.is_file() {
        read_json(&path)?
            .as_array()
            .cloned()
            .ok_or("workflow plan history is invalid")?
    } else {
        Vec::new()
    };
    history.push(json!({
        "revision": revision,
        "previous_revision": previous,
        "operation": operation,
        "stage": stage,
        "before_stage": before,
        "request_id": request,
        "recorded_at": now(),
    }));
    write_json(&path, &Value::Array(history))?;
    run["plan_revision"] = Value::from(revision);
    run["updated_at"] = Value::String(now());
    write_json(&directory.join("run.json"), &run)
}

fn ensure_plan_change_is_current(directory: &Path, definition: &Definition) -> Result<(), String> {
    if definition.stages.iter().any(|stage| {
        stage_status(directory, &stage.id).is_ok_and(|status| {
            matches!(
                status,
                StageStatus::WaitingApproval | StageStatus::WaitingFailure
            )
        })
    }) {
        return Err(
            "cannot revise a workflow plan while a human decision is open; resolve it or abort and start a new run"
                .to_owned(),
        );
    }
    Ok(())
}

fn skip(values: &[String]) -> Result<(), String> {
    let [run, stage, flag, request] = values else {
        return Err("invalid skip arguments".to_owned());
    };
    if flag != "--override" {
        return Err("invalid skip arguments".to_owned());
    }
    let directory = run_directory(run)?;
    let definition = definition(&directory)?;
    if !definition
        .stages
        .iter()
        .any(|candidate| &candidate.id == stage)
    {
        return Err(format!("workflow stage not found: {stage}"));
    }
    ensure_plan_change_is_current(&directory, &definition)?;
    let status = stage_status(&directory, stage)?;
    if status != StageStatus::Pending {
        return Err(format!(
            "only a pending stage can be skipped; {stage} is {}",
            stage_status_name(status)
        ));
    }
    consume_override("workflow-skip", run, stage, None, request)?;
    write_json(
        &directory.join("stages").join(format!("{stage}.json")),
        &json!({"id": stage, "status": "skipped", "exception": "maintainer-directed", "override_request": request, "skipped_at": now()}),
    )?;
    record_plan_change(&directory, "skip", stage, None, request)?;
    let _ = OverrideStore::new(&state_root()).result(
        request,
        true,
        &format!("workflow stage {stage} recorded as maintainer-directed skipped"),
    );
    let result = reconcile_locked(&directory);
    render_status(&directory)?;
    result
}

fn reorder(values: &[String]) -> Result<(), String> {
    let [run, stage, before_flag, before, override_flag, request] = values else {
        return Err("invalid reorder arguments".to_owned());
    };
    if before_flag != "--before" || override_flag != "--override" {
        return Err("invalid reorder arguments".to_owned());
    }
    let directory = run_directory(run)?;
    let definition = definition(&directory)?;
    ensure_plan_change_is_current(&directory, &definition)?;
    let order = stage_order(&directory, &definition)?;
    let statuses = stage_statuses(&directory, &definition)?;
    let mut state =
        RunState::from_records(&definition, order, statuses).map_err(|error| error.to_string())?;
    state
        .reorder(stage, before)
        .map_err(|error| error.to_string())?;
    consume_override("workflow-reorder", run, stage, Some(before), request)?;
    let mut order: Vec<String> =
        serde_json::from_value(read_json(&directory.join("stage-order.json"))?)
            .map_err(|error| error.to_string())?;
    let from = order
        .iter()
        .position(|value| value == stage)
        .ok_or("workflow reorder stage was not found")?;
    order.remove(from);
    let target = order
        .iter()
        .position(|value| value == before)
        .ok_or("workflow reorder stage was not found")?;
    order.insert(target, stage.clone());
    write_json(
        &directory.join("stage-order.json"),
        &serde_json::to_value(order).map_err(|error| error.to_string())?,
    )?;
    record_plan_change(&directory, "reorder", stage, Some(before), request)?;
    let _ = OverrideStore::new(&state_root()).result(
        request,
        true,
        &format!("workflow stage {stage} moved before {before} by maintainer direction"),
    );
    let result = reconcile_locked(&directory);
    render_status(&directory)?;
    result
}

fn metadata_value(path: &Path, key: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_default()
        .to_owned()
}

fn git_line(directory: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_success(directory: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

fn journal_stage(directory: &Path, stage: &str, kind: &str, outcome: &str) {
    let Ok(run) = read_json(&directory.join("run.json")) else {
        return;
    };
    let run_id = run["run"].as_str().unwrap_or_default();
    let Ok(task) = multplx_core::identifiers::TaskId::parse(run_id.to_owned()) else {
        return;
    };
    let (event, detail) = if kind == "entered" {
        (
            multplx_core::journal::JournalEvent::WorkflowStageEntered,
            json!({"run": run_id, "stage": stage}),
        )
    } else {
        (
            multplx_core::journal::JournalEvent::WorkflowStageGated,
            json!({"run": run_id, "stage": stage, "gate": "auto", "outcome": outcome}),
        )
    };
    if let Some(warning) = multplx_core::journal::JournalWriter::new(state_root()).try_emit(
        &task,
        event,
        &detail,
        "mx-workflow",
        &now(),
    ) {
        eprintln!("{warning}");
    }
}

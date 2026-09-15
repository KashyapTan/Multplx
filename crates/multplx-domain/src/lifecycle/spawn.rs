//! Common sub-agent launch preflight and metadata publication.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use multplx_core::filesystem::atomic_replace;
use multplx_core::identifiers::TaskId;

use super::home_seed::resolved;

#[derive(Clone, Debug)]
pub struct Context {
    pub root: PathBuf,
    pub home: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub projects: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub id: String,
    pub home: PathBuf,
    pub project: PathBuf,
    /// Bounded compatibility projection for legacy consumers.
    pub kind: String,
    pub role: String,
    pub output: String,
    pub persistent: bool,
    pub binding: Option<super::subagent_model::TaskRecord>,
    pub mode: String,
    pub yolo: bool,
    pub backend: String,
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub single_checkout_override: Option<String>,
    pub single_checkout_record: Option<PathBuf>,
    pub single_checkout_base_head: Option<String>,
    pub single_checkout_base_branch: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionContext {
    pub root_home: PathBuf,
    pub owner_home: PathBuf,
    pub owner_state: PathBuf,
    pub parent_task_id: String,
    pub parent_home: PathBuf,
    pub parent_state: PathBuf,
    pub attempt_id: String,
}

/// Project Phase 02 identity into the root-scoped admission boundary. The root
/// path comes only from the validated canonical root identity established by
/// `prepare_binding`; launch-directory and runtime-source overrides are ignored.
pub fn admission_context(
    record: &super::subagent_model::TaskRecord,
) -> Result<AdmissionContext, String> {
    record.validate()?;
    let root = record.root_id.as_deref().ok_or("root identity missing")?;
    let root_home = root
        .strip_prefix("root-home:")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("root identity does not name a canonical operational home")?;
    let owner_home = PathBuf::from(record.owner_home.as_deref().ok_or("owner home missing")?);
    let owner_state = PathBuf::from(record.owner_state.as_deref().ok_or("owner state missing")?);
    let parent_task_id = record.parent_id.clone().ok_or("parent identity missing")?;
    let parent_home = PathBuf::from(record.parent_home.as_deref().ok_or("parent home missing")?);
    let parent_state = PathBuf::from(
        record
            .parent_state
            .as_deref()
            .ok_or("parent state missing")?,
    );
    for (name, path) in [
        ("root home", &root_home),
        ("owner home", &owner_home),
        ("owner state", &owner_state),
        ("parent home", &parent_home),
        ("parent state", &parent_state),
    ] {
        if !path.is_absolute() {
            return Err(format!("{name} is not absolute"));
        }
    }
    let attempt_id = record
        .attempt
        .as_ref()
        .ok_or("attempt identity missing")?
        .id
        .clone();
    Ok(AdmissionContext {
        root_home: super::home_seed::resolved(&root_home),
        owner_home: super::home_seed::resolved(&owner_home),
        owner_state: super::home_seed::resolved(&owner_state),
        parent_task_id,
        parent_home: super::home_seed::resolved(&parent_home),
        parent_state: super::home_seed::resolved(&parent_state),
        attempt_id,
    })
}

/// Verify that a nested command is executing under the exact current parent
/// attempt, report-owner state and runtime home recorded at launch.
pub fn validate_initiating_parent(
    parent: &super::subagent_model::TaskRecord,
    runtime_home: &Path,
    reported_state: &Path,
    launch_attempt: &str,
    launch_generation: u64,
    launch_revision: u64,
) -> Result<(), String> {
    let recorded_parent_state = parent
        .owner_state
        .as_deref()
        .map(PathBuf::from)
        .ok_or("parent state missing")?;
    if resolved(reported_state) != resolved(&recorded_parent_state) {
        return Err("launching parent was read from the wrong owner state".into());
    }
    let runtime_parent_home = parent
        .persistent_home
        .as_deref()
        .or(parent.owner_home.as_deref())
        .ok_or("parent runtime home missing")?;
    if resolved(runtime_home) != resolved(Path::new(runtime_parent_home)) {
        return Err("launching parent runtime home does not match its task identity".into());
    }
    let attempt = parent.attempt.as_ref().ok_or("parent attempt missing")?;
    if launch_attempt != attempt.id
        || launch_generation != attempt.generation
        || launch_revision != attempt.brief_revision
    {
        return Err("launching process belongs to a stale parent attempt or brief revision".into());
    }
    Ok(())
}

fn descendant(parent: &Path, child: &Path) -> bool {
    parent != child && child.starts_with(parent)
}

fn validate_record_value(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("spawn {field} cannot be empty"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "spawn {field} contains a control character that cannot be recorded safely"
        ));
    }
    Ok(())
}

fn path_record_value<'a>(field: &str, path: &'a Path) -> Result<&'a str, String> {
    let value = path
        .to_str()
        .ok_or_else(|| format!("spawn {field} is not valid UTF-8"))?;
    validate_record_value(field, value)?;
    Ok(value)
}

/// Reject launch and metadata fields that cannot cross the line-oriented state boundary.
pub fn validate_for_launch(request: &Request) -> Result<(), String> {
    for (field, value) in [
        ("harness", request.harness.as_str()),
        ("kind", request.kind.as_str()),
        ("model", request.model.as_str()),
        ("effort", request.effort.as_str()),
        ("backend", request.backend.as_str()),
    ] {
        validate_record_value(field, value)?;
    }
    if !matches!(
        request.role.as_str(),
        "researcher" | "implementer" | "reviewer" | "sub-orchestrator"
    ) || !matches!(request.output.as_str(), "report" | "implementation")
    {
        return Err("invalid common sub-agent assignment".into());
    }
    if (request.kind == "daemon" && (request.mode != "daemon" || request.yolo))
        || (request.kind != "daemon"
            && crate::project_registry::DeliveryMode::parse(&request.mode).is_none())
    {
        return Err("invalid spawn delivery authority".to_owned());
    }
    path_record_value("home", &request.home)?;
    path_record_value("project", &request.project)?;
    Ok(())
}

fn registry_fields(path: &Path, id: &str) -> Result<BTreeMap<String, String>, String> {
    let text = fs::read_to_string(path).map_err(|error_value| error_value.to_string())?;
    let lines = text
        .lines()
        .filter(|line| {
            line.strip_prefix("- ")
                .is_some_and(|tail| tail.split_whitespace().next() == Some(id))
        })
        .collect::<Vec<_>>();
    let line = match lines.as_slice() {
        [line] => *line,
        [] => return Err(format!("no daemon registry entry for {id}")),
        _ => {
            return Err(format!(
                "duplicate persistent sub-agent registry identity: {id}"
            ));
        }
    };
    let mut fields = BTreeMap::new();
    let Some(start) = line.find("(home: ") else {
        return Err("malformed daemon registry entry".to_owned());
    };
    let details = &line[start + 1..];
    let details = details
        .strip_suffix(')')
        .ok_or("malformed daemon registry entry")?;
    for field in details.split("; ") {
        if let Some((key, value)) = field.split_once(": ")
            && fields.insert(key.to_owned(), value.to_owned()).is_some()
        {
            return Err(format!(
                "duplicate persistent sub-agent registry field: {key}"
            ));
        }
    }
    Ok(fields)
}

fn optional_registry_fields(path: &Path, id: &str) -> Result<BTreeMap<String, String>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    match registry_fields(path, id) {
        Err(error) if error.starts_with("no daemon registry entry for ") => Ok(BTreeMap::new()),
        result => result,
    }
}

pub fn task_authority(
    state: &Path,
    id: &str,
    resolution: &crate::project_registry::Resolution,
    selected_mode: Option<crate::project_registry::DeliveryMode>,
    selected_yolo: Option<bool>,
) -> Result<(String, bool), String> {
    let mut mode = selected_mode.unwrap_or(resolution.mode).as_str().to_owned();
    let mut yolo = false; // Legacy yolo is inert; no launch grants merge authority.
    let _ = selected_yolo;
    let meta_path = state.join(format!("{id}.meta"));
    if meta_path.exists() {
        let meta = fs::read_to_string(&meta_path).map_err(|error| error.to_string())?;
        let field = |key: &str| -> Result<&str, String> {
            let prefix = format!("{key}=");
            let values = meta
                .lines()
                .filter_map(|line| line.strip_prefix(&prefix))
                .collect::<Vec<_>>();
            match values.as_slice() {
                [value] => Ok(*value),
                _ => Err(format!(
                    "existing task has missing or duplicate {key}; reconcile authority before relaunch"
                )),
            }
        };
        let recorded_mode = field("mode")?;
        let recorded_yolo = match field("yolo")? {
            "on" => true,
            "off" => false,
            _ => return Err("existing task has invalid yolo".to_owned()),
        };
        if crate::project_registry::DeliveryMode::parse(recorded_mode).is_none()
            || selected_mode.is_some_and(|value| value.as_str() != recorded_mode)
        {
            return Err(
                "refusing to change existing task delivery authority during launch".to_owned(),
            );
        }
        mode = recorded_mode.to_owned();
        let _ = recorded_yolo;
        yolo = false;
    }
    Ok((mode, yolo))
}

pub fn parse(
    args: &[OsString],
    context: &Context,
    default_harness: &str,
) -> Result<Request, String> {
    validate_record_value("harness", default_harness)?;
    let mut positional = Vec::new();
    let mut selected_mode = None;
    let mut selected_yolo = None;
    let mut daemon = false;
    let mut legacy_daemon = false;
    let mut scout = false;
    let mut role: Option<String> = None;
    let mut output: Option<String> = None;
    let mut backend = "tmux".to_owned();
    let mut harness = None;
    let mut model = "default".to_owned();
    let mut effort = "default".to_owned();
    let mut index = 0;
    while index < args.len() {
        let value = args[index]
            .to_str()
            .ok_or("spawn argument is not valid UTF-8")?;
        match value {
            "--daemon" => {
                daemon = true;
                legacy_daemon = true;
            }
            "--persistent" => daemon = true,
            "--scout" => scout = true,
            "--review" => role = Some("reviewer".into()),
            "--harness" | "--model" | "--effort" | "--backend" | "--mode" | "--yolo" | "--role"
            | "--output" => {
                let next = args
                    .get(index + 1)
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| format!("{value} requires a value"))?
                    .to_owned();
                validate_record_value(value.trim_start_matches("--"), &next)?;
                match value {
                    "--role" => {
                        if !matches!(
                            next.as_str(),
                            "researcher" | "implementer" | "reviewer" | "sub-orchestrator"
                        ) {
                            return Err("invalid assignment role".into());
                        }
                        if role.replace(next).is_some() {
                            return Err("duplicate assignment role".into());
                        }
                    }
                    "--output" => {
                        if !matches!(next.as_str(), "report" | "implementation") {
                            return Err("invalid output; expected report or implementation".into());
                        }
                        if output.replace(next).is_some() {
                            return Err("duplicate output".into());
                        }
                    }
                    "--mode" => {
                        selected_mode = Some(
                            crate::project_registry::DeliveryMode::parse(&next)
                                .ok_or("invalid delivery mode")?,
                        )
                    }
                    "--yolo" => {
                        selected_yolo = Some(match next.as_str() {
                            "on" => true,
                            "off" => false,
                            _ => return Err("--yolo requires on or off".to_owned()),
                        })
                    }
                    "--harness" => harness = Some(next),
                    "--model" => model = next,
                    "--effort" => effort = next,
                    _ => backend = next,
                }
                index += 1;
            }
            value if value.starts_with("--") => {
                return Err(format!("unsupported native daemon spawn option: {value}"));
            }
            _ => positional.push(value.to_owned()),
        }
        index += 1;
    }
    let candidate_id = positional.first().ok_or("invalid spawn request")?;
    TaskId::parse(candidate_id).map_err(|_| "invalid spawn request")?;
    if role.is_none()
        && !scout
        && let Ok(text) = fs::read_to_string(context.state.join(format!("{candidate_id}.meta")))
    {
        let record = super::subagent_model::read_meta(candidate_id, &text)?;
        if !record.legacy_unknown {
            role = Some(
                match record.role {
                    super::subagent_model::AssignmentRole::Researcher => "researcher",
                    super::subagent_model::AssignmentRole::Reviewer => "reviewer",
                    super::subagent_model::AssignmentRole::Implementer => "implementer",
                    super::subagent_model::AssignmentRole::SubOrchestrator => "sub-orchestrator",
                }
                .into(),
            );
            if output.is_none() {
                output = Some(
                    if record.artifact == super::subagent_model::ArtifactKind::Implementation {
                        "implementation"
                    } else {
                        "report"
                    }
                    .into(),
                );
            }
        }
    }
    let scaffold = context.data.join(candidate_id).join("brief.md");
    if !context.state.join(format!("{candidate_id}.meta")).exists()
        && let Ok(brief) = fs::read_to_string(scaffold)
    {
        let markers = brief
            .lines()
            .filter_map(|line| line.strip_prefix("<!-- mx-assignment role="))
            .collect::<Vec<_>>();
        if markers.len() > 1 {
            return Err("duplicate brief assignment markers".into());
        }
        if let Some(marker) = markers.first() {
            let marked = marker.split_whitespace().next().unwrap_or_default();
            if !matches!(
                marked,
                "researcher" | "implementer" | "reviewer" | "sub-orchestrator"
            ) {
                return Err("invalid brief assignment role".into());
            }
            if role.as_deref().is_some_and(|value| value != marked)
                || (scout && marked != "researcher")
            {
                return Err("launch role conflicts with the accepted brief assignment".into());
            }
            role = Some(marked.into());
            if let Some(marked_output) = marker
                .split_whitespace()
                .find_map(|part| part.strip_prefix("output="))
            {
                let selected = match marked_output {
                    "report" | "implementation" => marked_output,
                    "coordination" if marked == "sub-orchestrator" => "report",
                    _ => return Err("invalid brief assignment output".into()),
                };
                if output.as_deref().is_some_and(|value| value != selected) {
                    return Err("launch output conflicts with the accepted brief assignment".into());
                }
                output = Some(selected.into());
            }
        }
    }
    if scout && role.as_deref().is_some_and(|value| value != "researcher") {
        return Err("--scout conflicts with --role".into());
    }
    let role = role.unwrap_or_else(|| {
        if scout {
            "researcher"
        } else if legacy_daemon {
            "sub-orchestrator"
        } else {
            "implementer"
        }
        .into()
    });
    if role == "sub-orchestrator" && !daemon {
        return Err("sub-orchestrator requires an existing persistent home; named coordinator spawn is owned by Phase 05".into());
    }
    let output = output.unwrap_or_else(|| {
        if role == "implementer" {
            "implementation"
        } else {
            "report"
        }
        .into()
    });
    if role == "sub-orchestrator" && output == "implementation" {
        return Err("sub-orchestrators delegate implementation".into());
    }
    let id = positional.first().ok_or("invalid spawn request")?.clone();
    TaskId::parse(&id).map_err(|_| "invalid spawn request")?;
    if !matches!(backend.as_str(), "tmux" | "herdr" | "cmux") {
        return Err(format!("unknown backend '{backend}'"));
    }
    if daemon && backend == "cmux" {
        return Err("backend=cmux does not support --daemon spawns yet".to_owned());
    }
    let fields = optional_registry_fields(&context.data.join("daemons.md"), &id)?;
    if !daemon {
        let project_arg = positional.get(1).ok_or("invalid spawn request")?;
        let project = if let Some(relative) = project_arg.strip_prefix("projects/") {
            context.projects.join(relative)
        } else {
            PathBuf::from(project_arg)
        };
        let project = if project.exists() {
            project
        } else {
            crate::project_registry::resolve_checkout(
                &context.home,
                project_arg.strip_prefix("projects/").unwrap_or(project_arg),
            )
            .map(|binding| binding.canonical_path)
            .unwrap_or(project)
        };
        let project = fs::canonicalize(&project).map_err(|_| {
            format!(
                "no brief at {}",
                context.data.join(&id).join("brief.md").display()
            )
        })?;
        let project = std::process::Command::new("git")
            .arg("-C")
            .arg(&project)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|path| fs::canonicalize(path.trim()).ok())
            .unwrap_or(project);
        if !context.data.join(&id).join("brief.md").is_file() {
            return Err(format!(
                "no brief at {}",
                context.data.join(&id).join("brief.md").display()
            ));
        }
        if positional.len() > 2 {
            harness = positional.get(2).cloned();
        }
        let resolution = crate::project_registry::resolve_path(
            &context.data.join("projects.md"),
            &context.projects,
            &context.root,
            &project,
        );
        if let Some(warning) = &resolution.warning {
            eprintln!("{warning}");
        }
        let selected_mode = if selected_mode.is_none()
            && !context.state.join(format!("{id}.meta")).exists()
            && project.join(".git").exists()
        {
            Some(
                match crate::project_registry::publication_for_path_at(
                    &context.home,
                    &context.data,
                    &context.projects,
                    &project,
                )? {
                    crate::project_registry::PublicationDestination::Local => {
                        crate::project_registry::DeliveryMode::LocalOnly
                    }
                    crate::project_registry::PublicationDestination::PullRequest => {
                        crate::project_registry::DeliveryMode::DirectPr
                    }
                },
            )
        } else {
            selected_mode
        };
        let (mode, yolo) = task_authority(
            &context.state,
            &id,
            &resolution,
            selected_mode,
            selected_yolo,
        )?;
        return Ok(Request {
            mode,
            yolo,
            id,
            home: context.home.clone(),
            project,
            kind: if output == "report" {
                "scout"
            } else {
                "delivery"
            }
            .to_owned(),
            role,
            output,
            persistent: false,
            binding: None,
            backend,
            harness: harness.unwrap_or_else(|| default_harness.to_owned()),
            model,
            effort,
            single_checkout_override: None,
            single_checkout_record: None,
            single_checkout_base_head: None,
            single_checkout_base_branch: None,
        });
    }
    if selected_mode.is_some() || selected_yolo.is_some() {
        return Err("daemon spawns do not accept task delivery mode or yolo overrides".to_owned());
    }
    let candidate = positional.get(1).map(PathBuf::from);
    let explicit_home = candidate.as_ref().filter(|path| path.is_dir()).cloned();
    if explicit_home.is_some() {
        if positional.len() > 2 {
            harness = positional.get(2).cloned();
        }
    } else if positional.len() > 1 {
        harness = positional.get(1).cloned();
    }
    let home = explicit_home
        .unwrap_or_else(|| PathBuf::from(fields.get("home").cloned().unwrap_or_default()));
    let home = resolved(&home);
    let active = resolved(&context.home);
    let root = resolved(&context.root);
    let reason = if home == Path::new("/") {
        Some("the filesystem root")
    } else if home == active {
        Some("the active Multplx home")
    } else if home == root {
        Some("the Multplx repo")
    } else if descendant(&active, &home) {
        Some("inside the active Multplx home")
    } else if descendant(&root, &home) {
        Some("inside the Multplx repo")
    } else if descendant(&home, &active) {
        Some("an ancestor of the active Multplx home")
    } else if descendant(&home, &root) {
        Some("an ancestor of the Multplx repo")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Err(format!(
            "daemon home cannot be {reason}: {}",
            home.display()
        ));
    }
    if !home.is_dir() {
        return Err(format!(
            "Multplx home does not exist or is not a directory: {}",
            home.display()
        ));
    }
    for name in ["data", "state", "config", "projects"] {
        let path = home.join(name);
        if path.exists() || fs::symlink_metadata(&path).is_ok() {
            let canonical = fs::canonicalize(&path).map_err(|_| {
                format!(
                    "daemon {name} directory must resolve inside the daemon home: {}",
                    path.display()
                )
            })?;
            if !descendant(&home, &canonical) {
                return Err(format!(
                    "daemon {name} directory must resolve inside the daemon home: {}",
                    path.display()
                ));
            }
        }
    }
    let marker = home.join(".mx-daemon-home");
    if !marker.is_file() {
        return Err(format!(
            "Multplx home {} is not a seeded daemon home",
            home.display()
        ));
    }
    let marker_id = fs::read_to_string(marker).unwrap_or_default();
    if marker_id.trim_end() != id {
        return Err(format!(
            "Multplx home {} is marked for daemon {}, expected {id}",
            home.display(),
            marker_id.trim_end()
        ));
    }
    if !home.join("AGENTS.md").is_file() {
        return Err(format!(
            "{} is not a Multplx home (missing AGENTS.md)",
            home.display()
        ));
    }
    if !home.join("bin").is_dir() {
        return Err(format!(
            "{} is not a Multplx home (missing bin/)",
            home.display()
        ));
    }
    if fields
        .get("home")
        .is_some_and(|value| resolved(Path::new(value)) != home)
    {
        return Err("daemon registry home does not match spawn target".to_owned());
    }
    Ok(Request {
        id,
        project: home.clone(),
        home,
        kind: "daemon".to_owned(),
        role,
        output,
        persistent: true,
        binding: None,
        mode: "daemon".to_owned(),
        yolo: false,
        backend,
        harness: harness.unwrap_or_else(|| default_harness.to_owned()),
        model,
        effort,
        single_checkout_override: None,
        single_checkout_record: None,
        single_checkout_base_head: None,
        single_checkout_base_branch: None,
    })
}

/// Freeze launch identity before any external endpoint is created. Project context
/// changes cannot alter this binding; an existing attempt needs explicit recovery.
pub fn prepare_binding(context: &Context, request: &mut Request) -> Result<(), String> {
    use super::subagent_model::{ArtifactKind, AssignmentRole, TaskRecord, read_meta};
    use sha2::{Digest, Sha256};
    super::subagent_model::require_writer_version(&context.state)?;
    let role = match request.role.as_str() {
        "researcher" => AssignmentRole::Researcher,
        "reviewer" => AssignmentRole::Reviewer,
        "sub-orchestrator" => AssignmentRole::SubOrchestrator,
        _ => AssignmentRole::Implementer,
    };
    let artifact = if role == AssignmentRole::SubOrchestrator {
        ArtifactKind::Coordination
    } else if request.output == "report" {
        ArtifactKind::Report
    } else {
        ArtifactKind::Implementation
    };
    let brief = if request.persistent {
        request.home.join("data/charter.md")
    } else {
        context.data.join(&request.id).join("brief.md")
    };
    let queued = std::env::var("MX_QUEUED_MODEL").ok();
    let existing = context.state.join(format!("{}.meta", request.id));
    let prior = if request.binding.is_some() {
        request.binding.take()
    } else if let Some(queued) = queued {
        let queued = read_meta(
            &request.id,
            &format!("schema_version=2\ncanonical_model={queued}\n"),
        )?;
        if existing.exists() {
            let current = read_meta(
                &request.id,
                &fs::read_to_string(&existing).map_err(|error| error.to_string())?,
            )?;
            if current.task_id != queued.task_id
                || current.parent_id != queued.parent_id
                || current.root_id != queued.root_id
                || current.owner_home != queued.owner_home
                || current.owner_state != queued.owner_state
                || current.parent_home != queued.parent_home
                || current.parent_state != queued.parent_state
                || current.attempt != queued.attempt
                || current.accepted_brief_revision != queued.accepted_brief_revision
                || current.accepted_brief_digest != queued.accepted_brief_digest
                || current.project != queued.project
                || current.role != queued.role
                || current.artifact != queued.artifact
            {
                return Err("queued request conflicts with current task identity".into());
            }
            Some(current)
        } else {
            Some(queued)
        }
    } else if existing.exists() {
        Some(read_meta(
            &request.id,
            &fs::read_to_string(existing).map_err(|error| error.to_string())?,
        )?)
    } else {
        None
    };
    let brief = prior
        .as_ref()
        .and_then(|record| record.accepted_brief_path.as_ref())
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .unwrap_or(brief);
    let body = fs::read(&brief)
        .map_err(|error| format!("cannot bind accepted brief {}: {error}", brief.display()))?;
    let digest = format!("{:x}", Sha256::digest(&body));
    let mut record = if let Some(record) = prior {
        if record.legacy_unknown {
            return Err(
                "legacy launch identity requires explicit home migration before replacement".into(),
            );
        }
        if record.owner_home.as_deref().map(Path::new) != Some(resolved(&context.home).as_path())
            || record.owner_state.as_deref().map(Path::new)
                != Some(resolved(&context.state).as_path())
        {
            return Err("launch owner home/state does not match recorded identity".into());
        }
        if record.accepted_brief_digest.as_deref() != Some(&digest) {
            return Err("accepted brief changed; record a new revision before launch".into());
        }
        if record.role != role
            || record.artifact != artifact
            || record.persistent != request.persistent
        {
            return Err(
                "launch conflicts with recorded assignment; record reassignment first".into(),
            );
        }
        if let Some(project) = &record.project {
            crate::project_registry::validate_binding(&context.home, project)?;
            if project.canonical_path != request.project {
                return Err("launch cannot retarget the recorded checkout".into());
            }
        }
        record
    } else {
        let owner_home = path_record_value("owner home", &resolved(&context.home))?.to_owned();
        let default_root = format!("root-home:{owner_home}");
        let (parent, parent_home, parent_state_path, root) = if let Ok(parent_id) =
            std::env::var("MX_TASK_ID")
        {
            TaskId::parse(&parent_id).map_err(|error| error.to_string())?;
            let parent_state = std::env::var_os("MX_REPORT_STATE_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| context.state.clone());
            let parent_path = parent_state.join(format!("{parent_id}.meta"));
            let parent = read_meta(
                &parent_id,
                &fs::read_to_string(&parent_path).map_err(|error| {
                    format!(
                        "cannot resolve launching parent {}: {error}",
                        parent_path.display()
                    )
                })?,
            )?;
            if parent.legacy_unknown {
                return Err("launching parent identity is legacy unknown; migrate before nesting canonical children".into());
            }
            let launch_attempt = std::env::var("MX_ATTEMPT_ID")
                .map_err(|_| "launching parent attempt identity missing")?;
            let launch_generation = std::env::var("MX_ATTEMPT_GENERATION")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or("launching parent attempt generation missing or invalid")?;
            let launch_revision = std::env::var("MX_BRIEF_REVISION")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or("launching parent brief revision missing or invalid")?;
            validate_initiating_parent(
                &parent,
                &context.home,
                &parent_state,
                &launch_attempt,
                launch_generation,
                launch_revision,
            )?;
            let parent_home = parent.owner_home.clone().ok_or("parent home missing")?;
            if parent_id == request.id && parent_home == owner_home {
                return Err("self-parent launch cycle".into());
            }
            (
                parent_id,
                parent_home,
                parent.owner_state.clone().ok_or("parent state missing")?,
                parent.root_id.ok_or("parent root missing")?,
            )
        } else {
            (
                default_root.clone(),
                owner_home.clone(),
                resolved(&context.state).to_string_lossy().into_owned(),
                default_root,
            )
        };
        let mut record = TaskRecord::new(
            request.id.clone(),
            role,
            artifact,
            request.persistent,
            parent,
            root,
            owner_home,
        );
        record.parent_home = Some(parent_home);
        record.owner_state = Some(resolved(&context.state).to_string_lossy().into_owned());
        record.parent_state = Some(parent_state_path);
        record.persistent_home = request
            .persistent
            .then(|| request.home.to_string_lossy().into_owned());
        record.accepted_brief_digest = Some(digest);
        record.accepted_brief_path = Some(
            context
                .state
                .join(format!("brief-revisions/{}-1.md", request.id))
                .to_string_lossy()
                .into_owned(),
        );
        record.briefs[0].scope =
            String::from_utf8(body.clone()).map_err(|_| "accepted brief must be UTF-8")?;
        record.briefs[0]
            .source_artifacts
            .push(brief.to_string_lossy().into_owned());
        if !request.persistent {
            record.project = Some(crate::project_registry::bind_project_at(
                &context.home,
                &context.data,
                &context.projects,
                &request.project,
            )?);
        }
        record
    };
    record.runtime.provider = request.backend.clone();
    record.validate()?;
    let root = record.root_id.clone().ok_or("root identity missing")?;
    let mut lineage = vec![record.clone()];
    let mut pending = vec![record.clone()];
    let mut loaded = std::collections::BTreeSet::new();
    loaded.insert(super::subagent_model::qualified_task_id(
        record.owner_home.as_deref().unwrap_or(""),
        &record.task_id,
    ));
    while let Some(current) = pending.pop() {
        let parent = current
            .parent_id
            .as_ref()
            .ok_or("parent identity missing")?;
        let mut edges = current
            .schedule
            .dependencies
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    current.owner_home.clone().unwrap_or_default(),
                    current.owner_state.clone(),
                )
            })
            .collect::<Vec<_>>();
        if parent != &root {
            edges.push((
                parent.clone(),
                current.parent_home.clone().ok_or("parent home missing")?,
                current.parent_state.clone(),
            ));
        }
        for (id, home, state) in edges {
            let key = super::subagent_model::qualified_task_id(&home, &id);
            if !loaded.insert(key) {
                continue;
            }
            TaskId::parse(&id).map_err(|error| error.to_string())?;
            let state = if let Some(state) = state {
                PathBuf::from(state)
            } else if home == context.home.to_string_lossy() {
                context.state.clone()
            } else {
                PathBuf::from(&home).join("state")
            };
            let path = state.join(format!("{id}.meta"));
            let ancestor = read_meta(
                &id,
                &fs::read_to_string(&path).map_err(|error| {
                    format!("cannot resolve lineage {}: {error}", path.display())
                })?,
            )?;
            if ancestor.owner_home.as_deref() != Some(home.as_str()) || ancestor.legacy_unknown {
                return Err("unresolved or wrong-home parent identity".into());
            }
            lineage.push(ancestor.clone());
            pending.push(ancestor);
        }
    }
    super::subagent_model::validate_lineage(&lineage, &[root])?;
    if request.persistent
        && let Some(allocation) =
            super::home_seed::read_home_allocation(&context.data, &request.id)?
    {
        if allocation.binding.path != resolved(&request.home)
            || allocation.binding.owner_home != resolved(&context.home)
            || allocation.state != "active"
        {
            return Err("persistent home allocation is not active at the selected path".into());
        }
        super::home_seed::verify_active_home(&allocation)?;
        record.home_allocation = Some(allocation.binding);
    }
    request.binding = Some(record);
    Ok(())
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchStage {
    Reserved,
    EndpointCreated,
    MetadataPublished,
    Submitted,
    Running,
    Failed,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LaunchAction {
    pub version: u32,
    pub request_id: String,
    pub task_id: String,
    pub binding: super::subagent_model::TaskRecord,
    pub backend: String,
    pub worktree: Option<PathBuf>,
    pub endpoint: Option<String>,
    pub stage: LaunchStage,
    pub detail: Option<String>,
}

fn action_path(context: &Context, request_id: &str) -> Result<PathBuf, String> {
    TaskId::parse(request_id).map_err(|_| "invalid spawn action request id")?;
    Ok(context
        .state
        .join(".spawn-actions")
        .join(format!("{request_id}.json")))
}

pub fn read_action(context: &Context, request_id: &str) -> Result<Option<LaunchAction>, String> {
    let path = action_path(context, request_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = multplx_core::filesystem::read_bounded_regular(&path, 1024 * 1024)
        .map_err(|error| format!("spawn action receipt is unreadable; retained: {error}"))?;
    let action: LaunchAction = serde_json::from_slice(&bytes)
        .map_err(|error| format!("spawn action receipt is corrupt; retained: {error}"))?;
    if action.version != 1
        || action.request_id != request_id
        || action.task_id != action.binding.task_id
    {
        return Err("spawn action receipt identity is invalid; retained".into());
    }
    action.binding.validate()?;
    Ok(Some(action))
}

pub fn reserve_action(
    context: &Context,
    request_id: &str,
    request: &Request,
) -> Result<LaunchAction, String> {
    let binding = request.binding.as_ref().ok_or("spawn binding missing")?;
    let expected = LaunchAction {
        version: 1,
        request_id: request_id.into(),
        task_id: request.id.clone(),
        binding: binding.clone(),
        backend: request.backend.clone(),
        worktree: None,
        endpoint: None,
        stage: LaunchStage::Reserved,
        detail: None,
    };
    if let Some(existing) = read_action(context, request_id)? {
        let same = existing.task_id == expected.task_id
            && existing.binding == expected.binding
            && existing.backend == expected.backend;
        if !same {
            return Err("spawn action request conflicts with its durable receipt".into());
        }
        return Ok(existing);
    }
    fs::create_dir_all(context.state.join(".spawn-actions")).map_err(|e| e.to_string())?;
    atomic_replace(
        action_path(context, request_id)?,
        &serde_json::to_vec(&expected).map_err(|e| e.to_string())?,
        0o600,
    )
    .map_err(|e| e.to_string())?;
    Ok(expected)
}

fn same_launch_identity(
    left: &super::subagent_model::TaskRecord,
    right: &super::subagent_model::TaskRecord,
) -> bool {
    let same_project = match (&left.project, &right.project) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.project_id == right.project_id
                && left.common_git_identity == right.common_git_identity
                && left.common_git_dir == right.common_git_dir
                && left.canonical_path == right.canonical_path
                && left.ownership == right.ownership
        }
        _ => false,
    };
    left.schema_version == right.schema_version
        && left.task_id == right.task_id
        && left.role == right.role
        && left.artifact == right.artifact
        && left.persistent == right.persistent
        && left.parent_id == right.parent_id
        && left.root_id == right.root_id
        && left.parent_home == right.parent_home
        && left.owner_home == right.owner_home
        && left.parent_state == right.parent_state
        && left.owner_state == right.owner_state
        && left.persistent_home == right.persistent_home
        && left.home_allocation == right.home_allocation
        && left.accepted_brief_digest == right.accepted_brief_digest
        && left.accepted_brief_path == right.accepted_brief_path
        && left.accepted_brief_revision == right.accepted_brief_revision
        && left.briefs == right.briefs
        && left.assignments == right.assignments
        && same_project
}

/// Restore the exact attempt and allocation frozen before an interrupted
/// launch. Only a receipt already marked failed after verified endpoint
/// absence may replace the newly prepared, otherwise equivalent binding.
pub fn recover_failed_action_binding(
    context: &Context,
    request_id: &str,
    request: &mut Request,
) -> Result<bool, String> {
    let Some(action) = read_action(context, request_id)? else {
        return Ok(false);
    };
    if action.task_id != request.id || action.backend != request.backend {
        return Err("spawn action request conflicts with the retry target".into());
    }
    if action.stage != LaunchStage::Failed {
        return Err(
            "spawn action endpoint is not proven absent; reconcile it before retrying".into(),
        );
    }
    let current = request.binding.as_ref().ok_or("spawn binding missing")?;
    if !same_launch_identity(current, &action.binding) {
        return Err("spawn action binding conflicts with the retry identity".into());
    }
    request.binding = Some(action.binding);
    Ok(true)
}

/// Recover a launch reservation that stopped before allocation and endpoint
/// creation. The absence of an action receipt proves that no later stage was
/// durably entered; any allocated binding is therefore refused.
pub fn recover_preallocation_intent(
    context: &Context,
    request_id: &str,
    request: &mut Request,
) -> Result<bool, String> {
    if read_action(context, request_id)?.is_some() {
        return Ok(false);
    }
    let path = context.state.join(format!(".spawn-{}.intent", request.id));
    if !path.exists() {
        return Ok(false);
    }
    let bytes = multplx_core::filesystem::read_bounded_regular(&path, 1024 * 1024)
        .map_err(|error| format!("spawn intent is unreadable; retained: {error}"))?;
    let binding: super::subagent_model::TaskRecord = serde_json::from_slice(&bytes)
        .map_err(|error| format!("spawn intent is corrupt; retained: {error}"))?;
    binding.validate()?;
    if binding.allocation.is_some() {
        return Err(
            "spawn intent contains an allocation without an action receipt; retained".into(),
        );
    }
    let current = request.binding.as_ref().ok_or("spawn binding missing")?;
    if !same_launch_identity(current, &binding) {
        return Err("spawn intent conflicts with the retry identity".into());
    }
    request.binding = Some(binding);
    Ok(true)
}

pub fn advance_action(
    context: &Context,
    request_id: &str,
    expected_binding: &super::subagent_model::TaskRecord,
    stage: LaunchStage,
    worktree: Option<&Path>,
    endpoint: Option<&str>,
    detail: Option<&str>,
) -> Result<LaunchAction, String> {
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        context
            .state
            .join(format!(".spawn-action-{request_id}.lock")),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    let mut action = read_action(context, request_id)?.ok_or("spawn action receipt missing")?;
    if &action.binding != expected_binding {
        return Err("spawn action binding changed; retained".into());
    }
    let rank = |value: LaunchStage| match value {
        LaunchStage::Reserved => 0,
        LaunchStage::EndpointCreated => 1,
        LaunchStage::MetadataPublished => 2,
        LaunchStage::Submitted => 3,
        LaunchStage::Running => 4,
        LaunchStage::Failed => 5,
    };
    if action.stage != LaunchStage::Failed
        && stage != LaunchStage::Failed
        && rank(stage) < rank(action.stage)
    {
        return Err("spawn action stage cannot move backward".into());
    }
    if let Some(path) = worktree {
        if action.worktree.as_deref().is_some_and(|old| old != path) {
            return Err("spawn action worktree changed; retained".into());
        }
        action.worktree = Some(path.to_path_buf());
    }
    if let Some(endpoint) = endpoint {
        validate_record_value("endpoint", endpoint)?;
        if action
            .endpoint
            .as_deref()
            .is_some_and(|old| old != endpoint)
            && action.stage != LaunchStage::Failed
        {
            return Err("spawn action endpoint changed before failure reconciliation".into());
        }
        action.endpoint = Some(endpoint.into());
    }
    action.stage = stage;
    action.detail = detail.map(str::to_owned);
    atomic_replace(
        action_path(context, request_id)?,
        &serde_json::to_vec(&action).map_err(|e| e.to_string())?,
        0o600,
    )
    .map_err(|e| e.to_string())?;
    Ok(action)
}

/// Begin a retry only after the backend owner has established that a recorded
/// endpoint is absent. The allocation binding remains unchanged.
pub fn retry_action_after_absence(
    context: &Context,
    request_id: &str,
    expected_binding: &super::subagent_model::TaskRecord,
    detail: &str,
) -> Result<LaunchAction, String> {
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        context
            .state
            .join(format!(".spawn-action-{request_id}.lock")),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    let mut action = read_action(context, request_id)?.ok_or("spawn action receipt missing")?;
    if &action.binding != expected_binding || action.stage != LaunchStage::Failed {
        return Err("spawn action is not eligible for absent-endpoint retry".into());
    }
    action.endpoint = None;
    action.stage = LaunchStage::Reserved;
    action.detail = Some(detail.into());
    atomic_replace(
        action_path(context, request_id)?,
        &serde_json::to_vec(&action).map_err(|e| e.to_string())?,
        0o600,
    )
    .map_err(|e| e.to_string())?;
    Ok(action)
}

pub fn publish_meta(context: &Context, request: &Request, endpoint: &str) -> Result<(), String> {
    publish_meta_for_worktree(context, request, endpoint, &request.project)
}

/// Return the private generated-config directory for one exact launch attempt.
pub fn task_temp_path(_context: &Context, request: &Request) -> Result<PathBuf, String> {
    let Some(record) = request.binding.as_ref() else {
        // Legacy metadata-only callers have no executable launch attempt.
        return Ok(std::env::temp_dir().join(format!("mx-{}", request.id)));
    };
    task_temp_path_for_record(record)
}

/// Return the only qualified generated-config directory accepted for an exact
/// canonical task attempt. Teardown uses this same derivation before removing
/// the path recorded in metadata.
pub fn task_temp_path_for_record(
    record: &super::subagent_model::TaskRecord,
) -> Result<PathBuf, String> {
    record.validate()?;
    let attempt = record
        .attempt
        .as_ref()
        .ok_or("launch attempt identity is missing")?;
    let root = record.root_id.as_deref().ok_or("root identity missing")?;
    let owner_state = record
        .owner_state
        .as_deref()
        .map(Path::new)
        .ok_or("launch owner state is missing")?;
    let key = crate::maintainer_override::sha256_text(&format!(
        "{root}\0{}\0{}\0{}\0{}\0{}",
        Path::new(owner_state).display(),
        record.task_id,
        attempt.id,
        attempt.generation,
        attempt.brief_revision
    ));
    Ok(std::env::temp_dir().join(format!("mx-{}-{}", record.task_id, &key[..16])))
}

pub fn publish_meta_for_worktree(
    context: &Context,
    request: &Request,
    endpoint: &str,
    actor_worktree: &Path,
) -> Result<(), String> {
    validate_for_launch(request)?;
    let _publication_lock = multplx_core::locks::DirectoryLock::acquire_wait(
        context.state.join(format!(".spawn-{}.lock", request.id)),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    if let Some(binding) = &request.binding {
        let bytes = fs::read(context.state.join(format!(".spawn-{}.intent", request.id)))
            .map_err(|error| format!("launch reservation missing: {error}"))?;
        let reserved: super::subagent_model::TaskRecord = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid launch reservation: {error}"))?;
        if &reserved != binding {
            return Err("launch reservation identity or accepted revision changed".into());
        }
        let prior_path = context.state.join(format!("{}.meta", request.id));
        if prior_path.exists() {
            let prior = super::subagent_model::read_meta(
                &request.id,
                &fs::read_to_string(prior_path).map_err(|error| error.to_string())?,
            )?;
            let expected_old = binding.prior_attempts.last().or(binding.attempt.as_ref());
            if prior.attempt.as_ref() != expected_old
                || prior.accepted_brief_revision != binding.accepted_brief_revision
            {
                return Err("task attempt or brief changed during external launch".into());
            }
        }
    }
    let fields = optional_registry_fields(&context.data.join("daemons.md"), &request.id)?;
    let projects = fields.get("projects").cloned().unwrap_or_default();
    let worktree = if request.kind == "daemon" {
        &request.home
    } else {
        actor_worktree
    };
    let mode = &request.mode;
    let yolo = if request.yolo { "on" } else { "off" };
    for (field, value) in [
        ("endpoint", endpoint),
        ("harness", request.harness.as_str()),
        ("kind", request.kind.as_str()),
        ("model", request.model.as_str()),
        ("effort", request.effort.as_str()),
        ("backend", request.backend.as_str()),
    ] {
        validate_record_value(field, value)?;
    }
    if projects.chars().any(char::is_control) {
        return Err(
            "spawn projects contains a control character that cannot be recorded safely".to_owned(),
        );
    }
    let worktree_value = path_record_value("worktree", worktree)?;
    let project_value = path_record_value("project", &request.project)?;
    let home_value = path_record_value("home", &request.home)?;
    let mut text = format!(
        "window={endpoint}\nworktree={}\nproject={}\nharness={}\nkind={}\nmode={mode}\nyolo={yolo}\nmodel={}\neffort={}\ntasktmp={}\n",
        worktree_value,
        project_value,
        request.harness,
        request.kind,
        request.model,
        request.effort,
        path_record_value("tasktmp", &task_temp_path(context, request)?)?
    );
    if request.backend != "tmux" {
        text.push_str(&format!("backend={}\n", request.backend));
    }
    if request.kind == "daemon" {
        text.push_str(&format!("home={home_value}\nprojects={projects}\n"));
    }
    if let (Some(request_id), Some(record), Some(head), Some(branch)) = (
        request.single_checkout_override.as_deref(),
        request.single_checkout_record.as_deref(),
        request.single_checkout_base_head.as_deref(),
        request.single_checkout_base_branch.as_deref(),
    ) {
        validate_record_value("single-checkout override", request_id)?;
        validate_record_value("single-checkout base head", head)?;
        validate_record_value("single-checkout base branch", branch)?;
        let record_value = path_record_value("single-checkout record", record)?;
        text.push_str(&format!(
            "single_checkout=yes\nsingle_checkout_override={request_id}\nsingle_checkout_record={}\nsingle_checkout_base_head={head}\nsingle_checkout_base_branch={branch}\n",
            record_value
        ));
    }
    if let Some(binding) = &request.binding {
        let mut record = binding.clone();
        record.runtime.provider = request.backend.clone();
        record.runtime.endpoint = Some(endpoint.into());
        // Endpoint labels are reusable; the adapters do not export a proven
        // immutable harness session identity here.
        record.runtime.session_id = None;
        text = super::subagent_model::write_meta(&text, &record)?;
    }
    if let Some(binding) = &request.binding {
        use multplx_core::filesystem::{TransitionWrite, recoverable_transition};
        let attempt = binding.attempt.as_ref().ok_or("launch attempt missing")?;
        let brief = if request.persistent {
            request.home.join("data/charter.md")
        } else {
            context.data.join(&request.id).join("brief.md")
        };
        let brief = binding
            .accepted_brief_path
            .as_ref()
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .unwrap_or(brief);
        let bytes = fs::read(&brief).map_err(|error| error.to_string())?;
        use sha2::{Digest, Sha256};
        if binding.accepted_brief_digest.as_deref()
            != Some(format!("{:x}", Sha256::digest(&bytes)).as_str())
        {
            return Err("accepted brief changed during launch".into());
        }
        let archive = PathBuf::from(format!(
            "brief-revisions/{}-{}.md",
            request.id, attempt.brief_revision
        ));
        fs::create_dir_all(context.state.join("brief-revisions"))
            .map_err(|error| error.to_string())?;
        let before_brief = fs::read(context.state.join(&archive)).ok();
        if before_brief.as_ref().is_some_and(|prior| prior != &bytes) {
            return Err("accepted brief archive conflicts".into());
        }
        let meta = PathBuf::from(format!("{}.meta", request.id));
        let before_meta = fs::read(context.state.join(&meta)).ok();
        if before_meta.as_deref() == Some(text.as_bytes()) {
            return Ok(());
        }
        let operation = format!("launch-{}", attempt.id);
        if context
            .state
            .join(".transitions")
            .join(format!("{operation}.json"))
            .exists()
        {
            let writes =
                multplx_core::filesystem::read_transition_writes(&context.state, &operation)
                    .map_err(|error| error.to_string())?;
            if writes
                .last()
                .is_none_or(|write| write.path != meta || write.after != text.as_bytes())
            {
                return Err("launch recovery conflicts with recorded intent".into());
            }
            return multplx_core::filesystem::recover_transition(&context.state, &operation)
                .map_err(|error| error.to_string());
        }
        return recoverable_transition(
            &context.state,
            &format!("launch-{}", attempt.id),
            &[
                TransitionWrite {
                    path: archive,
                    before: before_brief,
                    after: bytes,
                },
                TransitionWrite {
                    path: meta,
                    before: before_meta,
                    after: text.into_bytes(),
                },
            ],
            None,
        )
        .map_err(|error| error.to_string());
    }
    atomic_replace(
        context.state.join(format!("{}.meta", request.id)),
        text.as_bytes(),
        0o600,
    )
    .map_err(|error_value| error_value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(temp: &Path) -> Context {
        let base = fs::canonicalize(temp).expect("canonical tempdir");
        let value = Context {
            root: base.join("root"),
            home: base.join("home"),
            data: base.join("home/data"),
            state: base.join("home/state"),
            projects: base.join("home/projects"),
        };
        for path in [&value.root, &value.data, &value.state, &value.projects] {
            fs::create_dir_all(path).expect("directory");
        }
        value
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn bound_request(context: &Context) -> Request {
        let owner = fs::canonicalize(&context.home).expect("owner");
        let owner_text = owner.to_string_lossy().into_owned();
        let mut binding = crate::lifecycle::subagent_model::TaskRecord::new(
            "task".into(),
            crate::lifecycle::subagent_model::AssignmentRole::Implementer,
            crate::lifecycle::subagent_model::ArtifactKind::Implementation,
            false,
            format!("root-home:{owner_text}"),
            format!("root-home:{owner_text}"),
            owner_text,
        );
        binding.owner_state = Some(context.state.to_string_lossy().into_owned());
        binding.parent_state = binding.owner_state.clone();
        Request {
            id: "task".into(),
            project: context.root.clone(),
            home: context.home.clone(),
            kind: "delivery".into(),
            role: "implementer".into(),
            output: "implementation".into(),
            persistent: false,
            binding: Some(binding),
            mode: "deep-review".into(),
            yolo: false,
            backend: "tmux".into(),
            harness: "codex".into(),
            model: "default".into(),
            effort: "default".into(),
            single_checkout_override: None,
            single_checkout_record: None,
            single_checkout_base_head: None,
            single_checkout_base_branch: None,
        }
    }

    #[test]
    fn task_temp_path_is_owner_and_attempt_qualified() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        let request = bound_request(&context);
        let first = task_temp_path(&context, &request).expect("first temp path");
        assert!(
            first
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("mx-task-") && name.len() >= 24)
        );

        let mut next_attempt = request.clone();
        let attempt = next_attempt
            .binding
            .as_mut()
            .and_then(|record| record.attempt.as_mut())
            .expect("attempt");
        attempt.id.push_str("-next");
        attempt.generation += 1;
        assert_ne!(
            task_temp_path(&context, &next_attempt).expect("next attempt path"),
            first
        );

        let mut other_owner = request;
        other_owner.binding.as_mut().expect("binding").owner_state = Some(
            temp.path()
                .join("other-state")
                .to_string_lossy()
                .into_owned(),
        );
        assert_ne!(
            task_temp_path(&context, &other_owner).expect("other owner path"),
            first
        );
    }

    #[test]
    fn launch_actions_are_monotonic_and_retry_only_after_proven_absence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        let mut request = bound_request(&context);
        let binding = request.binding.clone().expect("binding");
        let reserved = reserve_action(&context, "request", &request).expect("reserve");
        assert_eq!(reserved.stage, LaunchStage::Reserved);
        advance_action(
            &context,
            "request",
            &binding,
            LaunchStage::EndpointCreated,
            Some(Path::new("/tmp/worktree")),
            Some("session:task"),
            None,
        )
        .expect("endpoint");
        assert!(
            advance_action(
                &context,
                "request",
                &binding,
                LaunchStage::Reserved,
                None,
                None,
                None,
            )
            .is_err()
        );
        assert!(retry_action_after_absence(&context, "request", &binding, "not proven").is_err());
        advance_action(
            &context,
            "request",
            &binding,
            LaunchStage::Failed,
            None,
            None,
            Some("verified absent"),
        )
        .expect("failed");
        let mut fresh = binding.clone();
        fresh.attempt.as_mut().expect("attempt").id = "new-attempt".into();
        request.binding = Some(fresh);
        assert!(recover_failed_action_binding(&context, "request", &mut request).expect("recover"));
        assert_eq!(request.binding.as_ref(), Some(&binding));
        let retry = retry_action_after_absence(
            &context,
            "request",
            request.binding.as_ref().expect("binding"),
            "absence checked",
        )
        .expect("retry");
        assert_eq!(retry.stage, LaunchStage::Reserved);
        assert_eq!(retry.endpoint, None);

        let preallocation_root = temp.path().join("preallocation");
        fs::create_dir(&preallocation_root).expect("preallocation root");
        let preallocation = self::context(&preallocation_root);
        let mut first = bound_request(&preallocation);
        let frozen = first.binding.clone().expect("frozen binding");
        atomic_replace(
            preallocation.state.join(".spawn-task.intent"),
            &serde_json::to_vec(&frozen).expect("intent"),
            0o600,
        )
        .expect("write intent");
        first
            .binding
            .as_mut()
            .expect("fresh")
            .attempt
            .as_mut()
            .expect("attempt")
            .id = "fresh-attempt".into();
        assert!(
            recover_preallocation_intent(&preallocation, "preallocation-request", &mut first)
                .expect("preallocation recovery")
        );
        assert_eq!(first.binding.as_ref(), Some(&frozen));
    }

    #[test]
    fn launch_action_faults_retain_the_frozen_attempt_and_allocation_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        let request = bound_request(&context);
        let binding = request.binding.clone().expect("binding");

        assert!(read_action(&context, "bad/id").is_err());
        fs::create_dir_all(context.state.join(".spawn-actions")).expect("actions");
        let path = context.state.join(".spawn-actions/request.json");
        fs::write(&path, b"{").expect("corrupt action");
        assert!(read_action(&context, "request").is_err());
        let invalid = LaunchAction {
            version: 1,
            request_id: "request".into(),
            task_id: "other".into(),
            binding: binding.clone(),
            backend: request.backend.clone(),
            worktree: None,
            endpoint: None,
            stage: LaunchStage::Reserved,
            detail: None,
        };
        atomic_replace(
            &path,
            &serde_json::to_vec(&invalid).expect("invalid action encoding"),
            0o600,
        )
        .expect("invalid action");
        assert!(read_action(&context, "request").is_err());
        fs::remove_file(&path).expect("remove invalid action");

        reserve_action(&context, "request", &request).expect("reserve");
        let mut conflicting_request = request.clone();
        conflicting_request.backend = "herdr".into();
        assert!(reserve_action(&context, "request", &conflicting_request).is_err());
        let mut retry = request.clone();
        assert!(recover_failed_action_binding(&context, "request", &mut retry).is_err());
        retry.id = "other".into();
        assert!(recover_failed_action_binding(&context, "request", &mut retry).is_err());

        let mut changed_binding = binding.clone();
        changed_binding.role = crate::lifecycle::subagent_model::AssignmentRole::Reviewer;
        assert!(
            advance_action(
                &context,
                "request",
                &changed_binding,
                LaunchStage::EndpointCreated,
                None,
                None,
                None,
            )
            .is_err()
        );
        advance_action(
            &context,
            "request",
            &binding,
            LaunchStage::EndpointCreated,
            Some(Path::new("/tmp/worktree-one")),
            Some("session:task"),
            None,
        )
        .expect("endpoint");
        assert!(
            advance_action(
                &context,
                "request",
                &binding,
                LaunchStage::MetadataPublished,
                Some(Path::new("/tmp/worktree-two")),
                None,
                None,
            )
            .is_err()
        );
        assert!(
            advance_action(
                &context,
                "request",
                &binding,
                LaunchStage::MetadataPublished,
                None,
                Some("session:other"),
                None,
            )
            .is_err()
        );
        assert!(
            advance_action(
                &context,
                "request",
                &binding,
                LaunchStage::MetadataPublished,
                None,
                Some("unsafe\nendpoint"),
                None,
            )
            .is_err()
        );

        let other_root = temp.path().join("other");
        fs::create_dir(&other_root).expect("other root");
        let other = self::context(&other_root);
        let mut absent = bound_request(&other);
        assert!(!recover_failed_action_binding(&other, "missing", &mut absent).expect("no action"));
        assert!(!recover_preallocation_intent(&other, "missing", &mut absent).expect("no intent"));
        fs::write(other.state.join(".spawn-task.intent"), b"{").expect("corrupt intent");
        assert!(recover_preallocation_intent(&other, "missing", &mut absent).is_err());
        let frozen = absent.binding.clone().expect("frozen");
        atomic_replace(
            other.state.join(".spawn-task.intent"),
            &serde_json::to_vec(&frozen).expect("intent encoding"),
            0o600,
        )
        .expect("intent");
        absent.binding.as_mut().expect("current").role =
            crate::lifecycle::subagent_model::AssignmentRole::Reviewer;
        assert!(recover_preallocation_intent(&other, "missing", &mut absent).is_err());
        reserve_action(&other, "missing", &bound_request(&other)).expect("action");
        assert!(
            !recover_preallocation_intent(&other, "missing", &mut absent).expect("action wins")
        );

        let mut legacy = bound_request(&context);
        legacy.binding = None;
        assert_eq!(
            task_temp_path(&context, &legacy).expect("legacy temp"),
            std::env::temp_dir().join("mx-task")
        );
    }

    #[test]
    fn nested_parent_validation_rejects_stale_attempt_and_wrong_home() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        let mut request = bound_request(&context);
        let parent = request.binding.take().expect("parent");
        let attempt = parent.attempt.as_ref().expect("attempt");
        assert!(
            validate_initiating_parent(
                &parent,
                &context.home,
                &context.state,
                &attempt.id,
                attempt.generation,
                attempt.brief_revision,
            )
            .is_ok()
        );
        assert!(
            validate_initiating_parent(
                &parent,
                &context.home,
                &context.state,
                "stale-attempt",
                attempt.generation,
                attempt.brief_revision,
            )
            .is_err()
        );
        assert!(
            validate_initiating_parent(
                &parent,
                &context.root,
                &context.state,
                &attempt.id,
                attempt.generation,
                attempt.brief_revision,
            )
            .is_err()
        );
        let mut persistent = parent.clone();
        persistent.persistent = true;
        persistent.persistent_home = Some(context.root.to_string_lossy().into_owned());
        assert!(
            validate_initiating_parent(
                &persistent,
                &context.root,
                &context.state,
                &attempt.id,
                attempt.generation,
                attempt.brief_revision,
            )
            .is_ok()
        );

        assert!(
            validate_initiating_parent(
                &parent,
                &context.home,
                &context.root,
                &attempt.id,
                attempt.generation,
                attempt.brief_revision,
            )
            .is_err()
        );
        assert!(
            validate_initiating_parent(
                &parent,
                &context.home,
                &context.state,
                &attempt.id,
                attempt.generation + 1,
                attempt.brief_revision,
            )
            .is_err()
        );
        assert!(
            validate_initiating_parent(
                &parent,
                &context.home,
                &context.state,
                &attempt.id,
                attempt.generation,
                attempt.brief_revision + 1,
            )
            .is_err()
        );
        let mut missing_state = parent.clone();
        missing_state.owner_state = None;
        assert!(
            validate_initiating_parent(
                &missing_state,
                &context.home,
                &context.state,
                &attempt.id,
                attempt.generation,
                attempt.brief_revision,
            )
            .is_err()
        );
        let mut missing_home = parent.clone();
        missing_home.owner_home = None;
        assert!(
            validate_initiating_parent(
                &missing_home,
                &context.home,
                &context.state,
                &attempt.id,
                attempt.generation,
                attempt.brief_revision,
            )
            .is_err()
        );
        let mut missing_attempt = parent.clone();
        missing_attempt.attempt = None;
        assert!(
            validate_initiating_parent(
                &missing_attempt,
                &context.home,
                &context.state,
                &attempt.id,
                attempt.generation,
                attempt.brief_revision,
            )
            .is_err()
        );
    }

    #[test]
    fn project_authority_is_resolved_once_and_self_repo_is_not_a_clone() {
        let temp = tempfile::tempdir().expect("temp");
        let context = context(temp.path());
        let project = context.projects.join("demo");
        fs::create_dir(&project).unwrap();
        fs::create_dir_all(context.data.join("task")).unwrap();
        fs::write(context.data.join("task/brief.md"), "brief").unwrap();
        fs::write(
            context.data.join("projects.md"),
            "- demo [local-only +yolo] - demo\n",
        )
        .unwrap();
        let request = parse(
            &args(&["task", project.to_str().unwrap()]),
            &context,
            "codex",
        )
        .unwrap();
        assert_eq!((&*request.mode, request.yolo), ("local-only", false));
        publish_meta_for_worktree(&context, &request, "broker:mx-task", &project).unwrap();
        fs::write(
            context.data.join("projects.md"),
            "- demo [direct-PR] - demo\n",
        )
        .unwrap();
        let recovered = parse(
            &args(&["task", project.to_str().unwrap()]),
            &context,
            "codex",
        )
        .unwrap();
        assert_eq!((&*recovered.mode, recovered.yolo), ("local-only", false));
        assert!(
            parse(
                &args(&["task", project.to_str().unwrap(), "--mode", "direct-PR"]),
                &context,
                "codex"
            )
            .is_err()
        );
        fs::remove_file(context.state.join("task.meta")).unwrap();
        fs::write(
            context.data.join("projects.md"),
            "- root [local-only +yolo] - unrelated clone\n",
        )
        .unwrap();
        let own = parse(
            &args(&["task", context.root.to_str().unwrap()]),
            &context,
            "codex",
        )
        .unwrap();
        assert_eq!((&*own.mode, own.yolo), ("deep-review", false));
        let own = parse(
            &args(&[
                "task",
                context.root.to_str().unwrap(),
                "--mode",
                "direct-PR",
                "--yolo",
                "on",
            ]),
            &context,
            "codex",
        )
        .unwrap();
        assert_eq!((&*own.mode, own.yolo), ("direct-PR", false));
    }

    #[test]
    fn scaffold_output_is_independent_and_conflicting_launch_cannot_reinterpret_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        fs::create_dir(context.projects.join("project")).unwrap();
        fs::create_dir_all(context.data.join("task")).unwrap();
        for (role, output) in [("implementer", "report"), ("researcher", "implementation")] {
            fs::write(
                context.data.join("task/brief.md"),
                format!("<!-- mx-assignment role={role} persistent=false output={output} -->\nAccepted scope.\n"),
            ).unwrap();
            let request = parse(&args(&["task", "projects/project"]), &context, "codex").unwrap();
            assert_eq!(request.role, role);
            assert_eq!(request.output, output);
            let other = if output == "report" {
                "implementation"
            } else {
                "report"
            };
            assert!(
                parse(
                    &args(&["task", "projects/project", "--output", other]),
                    &context,
                    "codex"
                )
                .unwrap_err()
                .contains("output conflicts")
            );
        }
    }

    #[test]
    fn delivery_and_daemon_parsing_cover_success_and_closed_refusals() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        let project = context.projects.join("project");
        fs::create_dir(&project).expect("project");
        fs::create_dir_all(context.data.join("task")).expect("brief directory");
        fs::write(context.data.join("task/brief.md"), "brief\n").expect("brief");
        let delivery = parse(
            &args(&[
                "task",
                "projects/project",
                "--scout",
                "--harness",
                "pi",
                "--model",
                "m",
                "--effort",
                "high",
                "--backend",
                "herdr",
            ]),
            &context,
            "codex",
        )
        .expect("delivery");
        assert_eq!(
            (
                delivery.kind.as_str(),
                delivery.harness.as_str(),
                delivery.backend.as_str()
            ),
            ("scout", "pi", "herdr")
        );
        assert_eq!(
            delivery.project,
            fs::canonicalize(project).expect("project")
        );
        assert!(
            parse(&args(&["task", "/missing"]), &context, "codex")
                .expect_err("brief")
                .contains("no brief")
        );
        assert!(
            parse(
                &args(&["task", "/tmp", "--backend", "bad"]),
                &context,
                "codex"
            )
            .expect_err("backend")
            .contains("unknown backend")
        );
        assert!(
            parse(&args(&["task", "/tmp", "--unknown"]), &context, "codex")
                .expect_err("option")
                .contains("unsupported")
        );

        let daemon = temp.path().join("daemon");
        for name in ["bin", "data", "state", "config", "projects"] {
            fs::create_dir_all(daemon.join(name)).expect("daemon directory");
        }
        fs::write(daemon.join(".mx-daemon-home"), "daemon\n").expect("marker");
        fs::write(daemon.join("AGENTS.md"), "agents\n").expect("agents");
        fs::write(
            context.data.join("daemons.md"),
            format!(
                "- daemon - live (home: {}; projects: one,two)\n",
                daemon.display()
            ),
        )
        .expect("registry");
        let request = parse(
            &args(&[
                "daemon",
                daemon.to_str().unwrap(),
                "--daemon",
                "--backend",
                "herdr",
            ]),
            &context,
            "claude",
        )
        .expect("daemon");
        assert_eq!(
            (request.kind.as_str(), request.harness.as_str()),
            ("daemon", "claude")
        );
        publish_meta(&context, &request, "session:workspace:pane").expect("publish");
        let meta = fs::read_to_string(context.state.join("daemon.meta")).expect("meta");
        assert!(meta.contains("backend=herdr\n"));
        assert!(meta.contains("projects=one,two\n"));
        assert!(
            parse(
                &args(&["daemon", "--daemon", "--backend", "cmux"]),
                &context,
                "codex"
            )
            .expect_err("cmux")
            .contains("does not support")
        );
        fs::write(daemon.join(".mx-daemon-home"), "other\n").expect("marker");
        assert!(
            parse(
                &args(&["daemon", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("marker")
            .contains("marked for daemon")
        );
    }

    #[test]
    fn daemon_home_safety_matrix_and_single_checkout_metadata_are_closed() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        for path in [
            Path::new("/"),
            context.home.as_path(),
            context.root.as_path(),
        ] {
            assert!(
                parse(
                    &args(&["daemon", path.to_str().unwrap(), "--daemon"]),
                    &context,
                    "codex"
                )
                .is_err()
            );
        }
        let inside = context.home.join("inside");
        fs::create_dir(&inside).expect("inside");
        assert!(
            parse(
                &args(&["daemon", inside.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("inside")
            .contains("inside the active")
        );
        let daemon = fs::canonicalize(temp.path())
            .expect("canonical")
            .join("daemon-two");
        fs::create_dir(&daemon).expect("daemon");
        assert!(
            parse(
                &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("seed")
            .contains("not a seeded")
        );
        fs::write(daemon.join(".mx-daemon-home"), "two\n").expect("marker");
        assert!(
            parse(
                &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("agents")
            .contains("missing AGENTS")
        );
        fs::write(daemon.join("AGENTS.md"), "agents\n").expect("agents");
        assert!(
            parse(
                &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("bin")
            .contains("missing bin")
        );
        fs::create_dir(daemon.join("bin")).expect("bin");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).expect("outside");
        symlink(&outside, daemon.join("data")).expect("symlink");
        assert!(
            parse(
                &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("containment")
            .contains("resolve inside")
        );
        fs::remove_file(daemon.join("data")).expect("remove");
        let mut request = parse(
            &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
            &context,
            "codex",
        )
        .expect("daemon");
        request.single_checkout_override = Some("request".into());
        request.single_checkout_record = Some(context.state.join("record.json"));
        request.single_checkout_base_head = Some("head".into());
        request.single_checkout_base_branch = Some("main".into());
        request.kind = "delivery".into();
        request.mode = "deep-review".into();
        request.backend = "tmux".into();
        publish_meta_for_worktree(
            &context,
            &request,
            "session:window",
            Path::new("/tmp/actor-worktree"),
        )
        .expect("publish");
        let meta = fs::read_to_string(context.state.join("two.meta")).expect("meta");
        assert!(meta.contains("single_checkout=yes\n"));
        assert!(meta.contains("worktree=/tmp/actor-worktree\n"));
        assert!(!meta.contains("backend=tmux"));
    }

    #[test]
    fn parser_and_publication_fault_matrix_rejects_malformed_inputs() {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        assert!(parse(&[], &context, "codex").is_err());
        assert!(parse(&[OsString::from_vec(vec![0xff])], &context, "codex").is_err());
        for option in ["--harness", "--model", "--effort", "--backend"] {
            assert!(parse(&args(&["task", option]), &context, "codex").is_err());
        }
        for option in ["--harness", "--model", "--effort"] {
            assert!(
                parse(
                    &args(&["task", "/tmp", option, "unsafe\nvalue"]),
                    &context,
                    "codex"
                )
                .expect_err("control character")
                .contains("control character")
            );
        }
        assert!(parse(&args(&["task"]), &context, "codex").is_err());

        let registry = context.data.join("daemons.md");
        fs::write(&registry, "- malformed - live without details\n").expect("registry");
        assert!(registry_fields(&registry, "malformed").is_err());
        fs::write(&registry, "- malformed - live (home: /tmp\n").expect("registry");
        assert!(registry_fields(&registry, "malformed").is_err());

        let inside_root = context.root.join("inside");
        fs::create_dir(&inside_root).expect("inside root");
        assert!(
            parse(
                &args(&["daemon", inside_root.to_str().unwrap(), "--daemon"]),
                &context,
                "codex",
            )
            .expect_err("inside root")
            .contains("inside the Multplx repo")
        );
        let ancestor = fs::canonicalize(temp.path()).expect("ancestor");
        assert!(
            parse(
                &args(&["daemon", ancestor.to_str().unwrap(), "--daemon"]),
                &context,
                "codex",
            )
            .expect_err("ancestor")
            .contains("ancestor of the active")
        );

        let daemon = temp.path().join("valid-daemon");
        fs::create_dir_all(daemon.join("bin")).expect("bin");
        fs::write(daemon.join(".mx-daemon-home"), "daemon\n").expect("marker");
        fs::write(daemon.join("AGENTS.md"), "agents\n").expect("agents");
        fs::write(
            &registry,
            "- daemon - live (home: /different; harness: codex)\n",
        )
        .expect("registry");
        assert!(
            parse(
                &args(&[
                    "daemon",
                    daemon.to_str().unwrap(),
                    "explicit-harness",
                    "--daemon",
                ]),
                &context,
                "codex",
            )
            .expect_err("registry mismatch")
            .contains("registry home does not match")
        );

        let request = Request {
            id: "task".into(),
            home: context.home.clone(),
            project: context.root.clone(),
            kind: "delivery".into(),
            role: "implementer".into(),
            output: "implementation".into(),
            persistent: false,
            binding: None,
            mode: "deep-review".into(),
            yolo: false,
            backend: "tmux".into(),
            harness: "codex".into(),
            model: "default".into(),
            effort: "default".into(),
            single_checkout_override: None,
            single_checkout_record: None,
            single_checkout_base_head: None,
            single_checkout_base_branch: None,
        };
        let mut invalid_context = context.clone();
        invalid_context.state = temp.path().join("state-file");
        fs::write(&invalid_context.state, "not a directory").expect("state file");
        assert!(publish_meta(&invalid_context, &request, "window").is_err());
    }

    #[test]
    fn launch_boundary_rejects_empty_control_and_non_utf8_record_values() {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let mut request = Request {
            id: "task".into(),
            home: temp.path().join("home"),
            project: temp.path().join("project"),
            kind: "delivery".into(),
            role: "implementer".into(),
            output: "implementation".into(),
            persistent: false,
            binding: None,
            mode: "deep-review".into(),
            yolo: false,
            backend: "tmux".into(),
            harness: "codex".into(),
            model: "default".into(),
            effort: "default".into(),
            single_checkout_override: None,
            single_checkout_record: None,
            single_checkout_base_head: None,
            single_checkout_base_branch: None,
        };
        validate_for_launch(&request).expect("safe launch request");

        request.harness.clear();
        assert!(
            validate_for_launch(&request)
                .expect_err("empty harness")
                .contains("cannot be empty")
        );
        request.harness = "codex\nunsafe".into();
        assert!(
            validate_for_launch(&request)
                .expect_err("control character")
                .contains("control character")
        );
        request.harness = "codex".into();
        request.project = PathBuf::from(OsString::from_vec(vec![b'/', 0xff]));
        assert!(
            validate_for_launch(&request)
                .expect_err("non-UTF-8 path")
                .contains("not valid UTF-8")
        );
    }

    #[test]
    fn assignment_and_authority_options_fail_closed_before_launch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        let project = context.projects.join("project");
        fs::create_dir(&project).expect("project");
        fs::create_dir_all(context.data.join("task")).expect("brief directory");
        let brief = context.data.join("task/brief.md");
        fs::write(&brief, "brief\n").expect("brief");

        for values in [
            vec!["task", "projects/project", "--role", "operator"],
            vec![
                "task",
                "projects/project",
                "--role",
                "implementer",
                "--role",
                "reviewer",
            ],
            vec!["task", "projects/project", "--output", "archive"],
            vec![
                "task",
                "projects/project",
                "--output",
                "report",
                "--output",
                "implementation",
            ],
            vec!["task", "projects/project", "--mode", "merge-anywhere"],
            vec!["task", "projects/project", "--yolo", "maybe"],
            vec!["task", "projects/project", "--scout", "--role", "reviewer"],
            vec!["task", "projects/project", "--role", "sub-orchestrator"],
            vec![
                "task",
                "projects/project",
                "--role",
                "sub-orchestrator",
                "--output",
                "implementation",
            ],
        ] {
            assert!(
                parse(&args(&values), &context, "codex").is_err(),
                "{values:?}"
            );
        }

        fs::write(
            &brief,
            "<!-- mx-assignment role=implementer output=implementation -->\n<!-- mx-assignment role=reviewer output=report -->\n",
        )
        .expect("duplicate markers");
        assert!(parse(&args(&["task", "projects/project"]), &context, "codex").is_err());
        fs::write(
            &brief,
            "<!-- mx-assignment role=operator output=report -->\n",
        )
        .expect("invalid marker role");
        assert!(parse(&args(&["task", "projects/project"]), &context, "codex").is_err());
        fs::write(
            &brief,
            "<!-- mx-assignment role=reviewer output=archive -->\n",
        )
        .expect("invalid marker output");
        assert!(parse(&args(&["task", "projects/project"]), &context, "codex").is_err());
        fs::write(
            &brief,
            "<!-- mx-assignment role=reviewer output=report -->\n",
        )
        .expect("role marker");
        assert!(
            parse(
                &args(&["task", "projects/project", "--role", "implementer",]),
                &context,
                "codex",
            )
            .is_err()
        );
        assert!(
            parse(
                &args(&["daemon", "--daemon", "--mode", "deep-review"]),
                &context,
                "codex",
            )
            .is_err()
        );

        let mut invalid = bound_request(&context);
        invalid.role = "operator".into();
        assert!(validate_for_launch(&invalid).is_err());
        invalid.role = "implementer".into();
        invalid.mode = "daemon".into();
        assert!(validate_for_launch(&invalid).is_err());

        let publication = bound_request(&context);
        let binding = publication.binding.as_ref().expect("binding");
        atomic_replace(
            context.state.join(".spawn-task.intent"),
            &serde_json::to_vec(binding).expect("intent encoding"),
            0o600,
        )
        .expect("intent");
        fs::write(&brief, "accepted bytes changed\n").expect("changed brief");
        assert!(
            publish_meta_for_worktree(
                &context,
                &publication,
                "session:task",
                &publication.project,
            )
            .expect_err("digest mismatch")
            .contains("accepted brief changed")
        );
    }
}

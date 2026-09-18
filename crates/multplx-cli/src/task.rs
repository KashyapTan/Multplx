//! Human-facing durable task intake into the existing orchestrator conversation.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use multplx_core::process::SystemProcessProbe;
use multplx_domain::operational_input::{RequestStore, RequestSubmission};

pub const HELP: &str = "Usage: mx task --project SELECTOR [OPTIONS] TEXT\n\nOptions:\n  --request-id ID       Stable retry identity (generated when omitted).\n  --batch-id ID         Correlate independent requests (defaults to request ID).\n  --task-id ID          Requested task identity (defaults to request ID).\n  --domain ID           Route to an explicit scoped coordinator whose domain contains the project.\n  --start REVISION      Use this named clean commit instead of current HEAD.\n  --depends TASK        Record a dependency; repeatable.\n  --artifact PATH       Reference an existing context artifact.\n  --capture-working-changes\n                        Refused: captured working changes are not supported by this release.\n\nThe selected Git checkout is remembered as user-owned and its project, checkout and\nfull starting commit are frozen before delivery. Uncommitted files are excluded and\nleft untouched; use --start for a different named clean commit. Submission returns a\ndurable receipt. Delivery does not claim that implementation has started. Reusing an\nexplicit request ID with identical input converges; changed input fails. Project\nselection alone always routes to the one main orchestrator.\n";

#[derive(Debug, Default)]
struct Arguments {
    project: Option<String>,
    request_id: Option<String>,
    batch_id: Option<String>,
    task_id: Option<String>,
    domain: Option<String>,
    start: Option<String>,
    dependencies: Vec<String>,
    artifact: Option<String>,
    scope: Option<String>,
}

fn value(args: &[OsString], index: &mut usize, option: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| format!("{option} requires a value"))
}

fn parse(args: &[OsString]) -> Result<Arguments, String> {
    let mut parsed = Arguments::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].to_str() {
            Some("--project") if parsed.project.is_none() => parsed.project = Some(value(args, &mut index, "--project")?),
            Some("--request-id") if parsed.request_id.is_none() => parsed.request_id = Some(value(args, &mut index, "--request-id")?),
            Some("--batch-id") if parsed.batch_id.is_none() => parsed.batch_id = Some(value(args, &mut index, "--batch-id")?),
            Some("--task-id") if parsed.task_id.is_none() => parsed.task_id = Some(value(args, &mut index, "--task-id")?),
            Some("--domain") if parsed.domain.is_none() => parsed.domain = Some(value(args, &mut index, "--domain")?),
            Some("--start") if parsed.start.is_none() => parsed.start = Some(value(args, &mut index, "--start")?),
            Some("--depends") => parsed.dependencies.push(value(args, &mut index, "--depends")?),
            Some("--artifact") if parsed.artifact.is_none() => parsed.artifact = Some(value(args, &mut index, "--artifact")?),
            Some("--capture-working-changes") => return Err("captured working changes are not supported; commit them or select a named clean revision with --start".into()),
            Some("-h" | "--help") => return Err(HELP.into()),
            Some(option) if option.starts_with('-') => return Err(format!("unknown task option {option}")),
            Some(_) => {
                let words = args[index..]
                    .iter()
                    .map(|value| value.to_str().ok_or("task text must be UTF-8"))
                    .collect::<Result<Vec<_>, _>>()?;
                parsed.scope = Some(words.join(" "));
                break;
            }
            None => return Err("task arguments must be UTF-8".into()),
        }
        index += 1;
    }
    if parsed.project.is_none()
        || parsed
            .scope
            .as_ref()
            .is_none_or(|scope| scope.trim().is_empty())
    {
        return Err(HELP.into());
    }
    Ok(parsed)
}

fn target(
    root_home: &Path,
    root_state: &Path,
    runtime_root: &Path,
    domain: Option<&str>,
    project_id: &str,
) -> Result<(PathBuf, PathBuf, Option<String>), String> {
    let Some(domain_id) = domain else {
        return Ok((root_home.to_owned(), root_state.to_owned(), None));
    };
    let record = multplx_domain::lifecycle::domain::read_coordinator(root_state, domain_id)?;
    let binding = record
        .domain
        .as_ref()
        .ok_or("coordinator domain is unavailable")?;
    if !binding.projects.iter().any(|project| project == project_id) {
        return Err(format!(
            "project {project_id} is outside coordinator {domain_id}'s recorded domain"
        ));
    }
    let home = record
        .persistent_home
        .as_deref()
        .map(PathBuf::from)
        .ok_or("coordinator has no private routing home")?;
    let validated = multplx_domain::inheritance::validate_daemon_home(
        domain_id,
        &home,
        root_home,
        runtime_root,
    )?;
    let home = validated.path;
    Ok((home.clone(), home.join("state"), Some(domain_id.to_owned())))
}

fn execute(args: &[OsString]) -> Result<serde_json::Value, String> {
    let parsed = parse(args)?;
    let (runtime_root, root_home, _) = crate::active_paths();
    std::fs::create_dir_all(root_home.join("state")).map_err(|error| error.to_string())?;
    let selector = parsed.project.as_deref().expect("validated project");
    let mut binding = if Path::new(selector).exists() {
        multplx_domain::project_registry::register_project(
            &root_home,
            Path::new(selector),
            None,
            multplx_domain::project_registry::CheckoutOwnership::UserOwned,
        )?
    } else {
        multplx_domain::project_registry::resolve_checkout(&root_home, selector)?
    };
    if let Some(revision) = parsed.start.as_deref() {
        binding.starting_revision = multplx_domain::project_registry::resolve_starting_revision(
            &binding.canonical_path,
            revision,
        )?;
    }
    let dirty = multplx_domain::project_registry::has_working_changes(&binding.canonical_path)?;
    let root_state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root_home.join("state"));
    let (recipient_home, recipient_state, domain) = target(
        &root_home,
        &root_state,
        &runtime_root,
        parsed.domain.as_deref(),
        &binding.project_id,
    )?;
    std::fs::create_dir_all(&recipient_state).map_err(|error| error.to_string())?;

    let generated = multplx_domain::lifecycle::subagent_model::new_identity("request");
    let request_id = parsed.request_id.as_deref().unwrap_or(&generated);
    let batch_id = parsed.batch_id.as_deref().unwrap_or(request_id);
    let task_id = parsed.task_id.as_deref().unwrap_or(request_id);
    let processes = SystemProcessProbe::default();
    let owner = crate::request_connected_owner(&recipient_state, &processes)?;
    let store = RequestStore::new(&recipient_state, &recipient_home);
    if parsed.start.is_none()
        && let Ok(existing) = store.get(request_id)
        && existing.project_id == binding.project_id
        && existing.checkout_id == binding.checkout_id
    {
        binding.starting_revision = existing.starting_revision;
    }
    multplx_domain::project_registry::validate_binding(&root_home, &binding)?;
    let artifact = parsed
        .artifact
        .as_deref()
        .map(|artifact| {
            let metadata = std::fs::symlink_metadata(artifact).map_err(|error| {
                format!("context artifact {artifact:?} is unavailable: {error}")
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("context artifact must be an existing regular file".into());
            }
            std::fs::canonicalize(artifact)
                .map(|path| path.to_string_lossy().into_owned())
                .map_err(|error| error.to_string())
        })
        .transpose()?;
    let request = RequestSubmission {
        batch_id,
        request_id,
        task_id,
        client_id: "workspace-cli",
        recipient_owner: owner.as_ref(),
        parent_task_id: None,
        parent_home: None,
        attempt_id: None,
        attempt_generation: None,
        project_id: &binding.project_id,
        checkout_id: &binding.checkout_id,
        starting_revision: &binding.starting_revision,
        brief_revision: 1,
        scope: parsed.scope.as_deref().expect("validated scope"),
        dependencies: &parsed.dependencies,
        context_artifact: artifact.as_deref(),
    };
    let accepted = store.submit(&request, SystemTime::now(), None)?;
    let key = format!("request-{}", accepted.request.request_id);
    let payload = format!(
        "request: batch={} request={} task={} project={} brief={}",
        accepted.request.batch_id,
        accepted.request.request_id,
        accepted.request.task_id,
        accepted.request.project_id,
        accepted.request.brief_revision
    );
    let (wake, _) = multplx_core::wake::WakeQueue::new(&recipient_state)
        .append_once(
            multplx_core::wake::WakeKind::Signal,
            &key,
            &payload,
            SystemTime::now(),
            &processes,
        )
        .map_err(|error| error.to_string())?;
    let recorded = store.record_notification(
        &accepted.request.request_id,
        &format!("wake-{:020}", wake.sequence),
    )?;
    Ok(serde_json::json!({
        "schema": "mx-task-intake-receipt.v1",
        "newly_accepted": accepted.newly_accepted,
        "implementation_started": false,
        "working_changes": if dirty { "present-and-excluded" } else { "none" },
        "route": {
            "kind": if domain.is_some() { "scoped-coordinator" } else { "main-orchestrator" },
            "coordinator": domain,
            "home": recipient_home,
        },
        "request": recorded,
    }))
}

pub fn run(args: &[OsString]) -> i32 {
    if args.is_empty()
        || args
            .iter()
            .any(|argument| matches!(argument.to_str(), Some("-h" | "--help")))
    {
        print!("{HELP}");
        return 0;
    }
    match execute(args) {
        Ok(receipt) => match serde_json::to_string_pretty(&receipt) {
            Ok(receipt) => {
                println!("{receipt}");
                0
            }
            Err(error) => {
                eprintln!("mx task: {error}");
                1
            }
        },
        Err(error) if error == HELP => {
            eprint!("{HELP}");
            2
        }
        Err(error) => {
            eprintln!("mx task: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn grammar_rejects_non_utf8_options_and_task_text() {
        use std::os::unix::ffi::OsStringExt;

        let invalid = OsString::from_vec(vec![0xff]);
        assert_eq!(
            parse(std::slice::from_ref(&invalid)).unwrap_err(),
            "task arguments must be UTF-8"
        );
        assert_eq!(
            parse(&["--project".into(), invalid.clone(), "work".into()]).unwrap_err(),
            "--project requires a value"
        );
        assert_eq!(
            parse(&["--project".into(), "app".into(), "work".into(), invalid,]).unwrap_err(),
            "task text must be UTF-8"
        );
    }

    #[test]
    fn grammar_requires_project_and_text_and_refuses_capture() {
        assert!(parse(&["--project".into(), "app".into(), "Fix login".into()]).is_ok());
        assert!(parse(&["Fix login".into()]).is_err());
        assert!(parse(&["--project".into(), "app".into()]).is_err());
        assert!(
            parse(&[
                "--project".into(),
                "app".into(),
                "--capture-working-changes".into(),
                "Fix".into(),
            ])
            .unwrap_err()
            .contains("not supported")
        );
    }

    #[test]
    fn grammar_preserves_retry_batch_domain_and_dependencies() {
        let parsed = parse(&[
            "--project".into(),
            "app".into(),
            "--request-id".into(),
            "req-1".into(),
            "--batch-id".into(),
            "batch-1".into(),
            "--domain".into(),
            "coord".into(),
            "--depends".into(),
            "api".into(),
            "--depends".into(),
            "db".into(),
            "Fix".into(),
            "login".into(),
        ])
        .unwrap();
        assert_eq!(parsed.request_id.as_deref(), Some("req-1"));
        assert_eq!(parsed.batch_id.as_deref(), Some("batch-1"));
        assert_eq!(parsed.domain.as_deref(), Some("coord"));
        assert_eq!(parsed.dependencies, ["api", "db"]);
        assert_eq!(parsed.scope.as_deref(), Some("Fix login"));
    }

    #[test]
    fn explicit_domain_routes_only_to_its_private_home_and_bound_project() {
        use multplx_domain::lifecycle::subagent_model::{
            ArtifactKind, AssignmentRole, DomainBinding, TaskRecord, write_meta,
        };

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let runtime = temp.path().join("runtime");
        let state = root.join("state");
        let private = temp.path().join("private-coord");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir(&runtime).unwrap();
        std::fs::create_dir_all(private.join("state")).unwrap();
        std::fs::create_dir(private.join("bin")).unwrap();
        std::fs::write(private.join(".mx-daemon-home"), "coord\n").unwrap();
        std::fs::write(private.join("AGENTS.md"), "fixture\n").unwrap();
        let mut record = TaskRecord::new(
            "coord".into(),
            AssignmentRole::SubOrchestrator,
            ArtifactKind::Coordination,
            true,
            "root".into(),
            "root".into(),
            root.to_string_lossy().into_owned(),
        );
        record.private_home = true;
        record.persistent_home = Some(private.to_string_lossy().into_owned());
        record.domain = Some(DomainBinding {
            domain_id: "domain-coord".into(),
            coordinator_id: "coord".into(),
            scope_revision: 1,
            assignment_generation: 1,
            projects: vec!["project-1".into()],
            idea_id: None,
            scope: "coordinate project 1".into(),
        });
        std::fs::write(
            state.join("coord.meta"),
            write_meta("kind=daemon\n", &record).unwrap(),
        )
        .unwrap();

        let (home, routed_state, coordinator) =
            target(&root, &state, &runtime, Some("coord"), "project-1").unwrap();
        assert_eq!(home, std::fs::canonicalize(&private).unwrap());
        assert_eq!(routed_state, home.join("state"));
        assert_eq!(coordinator.as_deref(), Some("coord"));
        assert!(
            target(&root, &state, &runtime, Some("coord"), "project-2")
                .unwrap_err()
                .contains("outside coordinator")
        );
        assert_eq!(
            target(&root, &state, &runtime, None, "project-2").unwrap(),
            (root.clone(), state, None)
        );
    }
}

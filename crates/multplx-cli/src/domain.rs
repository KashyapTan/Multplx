use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const HELP: &str = "Usage: mx domain inspect <coordinator-id>\n       mx domain revise <coordinator-id> --expected-revision N --scope TEXT --reason TEXT [(--project PROJECT)...|--idea IDEA]\n       mx domain bind-project <coordinator-id> --expected-revision N --project PROJECT --reason TEXT\n\nScope and project changes require a stopped/reconciled coordinator. Revisions fence the prior assignment generation, update the accepted charter and preserve task identity and history. Project selectors resolve through the canonical project registry.\n";

fn active_home() -> PathBuf {
    std::env::var_os("MX_HOME")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .or_else(|| std::env::var_os("MX_RUST_SOURCE_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn active_runtime_root() -> PathBuf {
    std::env::var_os("MX_ROOT_OVERRIDE")
        .or_else(|| std::env::var_os("MX_RUST_SOURCE_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn resolve_project(
    home: &Path,
    selector: &str,
) -> Result<multplx_domain::project_registry::ProjectBinding, String> {
    if Path::new(selector).exists() {
        let data = std::env::var_os("MX_DATA_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("data"));
        let projects = std::env::var_os("MX_PROJECTS_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("projects"));
        multplx_domain::project_registry::bind_project_at(
            home,
            &data,
            &projects,
            Path::new(selector),
        )
    } else {
        multplx_domain::project_registry::resolve_checkout(home, selector)
    }
}

fn value(args: &[OsString], index: &mut usize, option: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| format!("{option} requires a value"))
}

pub fn run(args: &[OsString]) -> i32 {
    match command(args) {
        Ok(output) => {
            print!("{output}");
            0
        }
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

fn command(args: &[OsString]) -> Result<String, String> {
    if args.is_empty()
        || args
            .iter()
            .any(|arg| matches!(arg.to_str(), Some("-h" | "--help")))
    {
        return Ok(HELP.into());
    }
    let operation = args[0].to_str().ok_or("domain operation is not UTF-8")?;
    let id = args.get(1).and_then(|value| value.to_str()).ok_or(HELP)?;
    multplx_core::identifiers::TaskId::parse(id).map_err(|e| e.to_string())?;
    let home = active_home();
    let runtime_root = active_runtime_root();
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if operation == "inspect" && args.len() == 2 {
        return serde_json::to_string_pretty(&multplx_domain::lifecycle::domain::read_coordinator(
            &state, id,
        )?)
        .map(|text| format!("{text}\n"))
        .map_err(|e| e.to_string());
    }
    if !matches!(operation, "revise" | "bind-project") {
        return Err(HELP.into());
    }
    let mut expected = None;
    let mut scope = None;
    let mut reason = None;
    let mut idea = None;
    let mut selectors = Vec::new();
    let mut seen = BTreeSet::new();
    let mut index = 2;
    while index < args.len() {
        let option = args[index].to_str().ok_or("domain option is not UTF-8")?;
        if option != "--project" && !seen.insert(option) {
            return Err(format!("duplicate domain option {option}"));
        }
        match option {
            "--expected-revision" => {
                expected = Some(
                    value(args, &mut index, option)?
                        .parse::<u64>()
                        .map_err(|_| "invalid expected revision")?,
                )
            }
            "--scope" => scope = Some(value(args, &mut index, option)?),
            "--reason" => reason = Some(value(args, &mut index, option)?),
            "--idea" => idea = Some(value(args, &mut index, option)?),
            "--project" => selectors.push(value(args, &mut index, option)?),
            _ => return Err(format!("unknown domain option {option}")),
        }
        index += 1;
    }
    let expected = expected.ok_or("--expected-revision is required")?;
    let reason = reason.ok_or("--reason is required")?;
    if operation == "bind-project" {
        if selectors.len() != 1 || scope.is_some() || idea.is_some() {
            return Err(
                "bind-project requires exactly one --project and accepts no --scope or --idea"
                    .into(),
            );
        }
        let project = resolve_project(&home, &selectors[0])?;
        let record = multplx_domain::lifecycle::domain::bind_project(
            &state,
            &home,
            &runtime_root,
            id,
            expected,
            &project,
            reason,
        )?;
        return serde_json::to_string_pretty(&record)
            .map(|text| format!("{text}\n"))
            .map_err(|e| e.to_string());
    }
    let scope = scope.ok_or("revise requires --scope")?;
    let current = multplx_domain::lifecycle::domain::read_coordinator(&state, id)?;
    let current_domain = current.domain.as_ref().expect("validated domain");
    if selectors.is_empty() && idea.is_none() {
        if current_domain.projects.is_empty() {
            idea = current_domain.idea_id.clone();
        } else {
            selectors = current_domain.projects.clone();
        }
    } else if !selectors.is_empty() && idea.is_some() {
        return Err("choose project bindings or an idea identity for the revised domain".into());
    }
    let projects = selectors
        .iter()
        .map(|selector| resolve_project(&home, selector))
        .collect::<Result<Vec<_>, _>>()?;
    let record = multplx_domain::lifecycle::domain::update_scope(
        &state,
        &home,
        &runtime_root,
        id,
        expected,
        scope,
        &projects,
        idea,
        reason,
    )?;
    serde_json::to_string_pretty(&record)
        .map(|text| format!("{text}\n"))
        .map_err(|e| e.to_string())
}

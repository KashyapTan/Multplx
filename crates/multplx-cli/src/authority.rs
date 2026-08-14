use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use multplx_domain::maintainer_override::{
    self, Binding, BoundaryClass, OverrideStore, RecordState, Request,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const OVERRIDE_USAGE: &str = "Request, decide, consume, inspect, and audit exact maintainer exceptions.\n\nUsage:\n  mx-maintainer-override.sh registry [--json]\n  mx-maintainer-override.sh request --boundary <id> --task <id> --project <slug>\n    --operation <literal operation> --target <identity>\n    --expected-state <sha256> --consequence <one line> [--ttl <seconds>]\n  mx-maintainer-override.sh grant <request-id> --maintainer-words <literal words>\n  mx-maintainer-override.sh deny <request-id> --maintainer-words <literal words>\n  mx-maintainer-override.sh consume <request-id> --boundary <id> --task <id>\n    --project <slug> --operation <literal operation> --target <identity>\n    --expected-state <sha256>\n  mx-maintainer-override.sh result <request-id> --outcome succeeded|failed --detail <text>\n  mx-maintainer-override.sh inspect <request-id>\n  mx-maintainer-override.sh audit [--json]\n  mx-maintainer-override.sh digest <literal text>\n  mx-maintainer-override.sh argv [literal argv...]\n  mx-maintainer-override.sh handoff <request-id>\n";

pub fn run(entry: &str, args: &[OsString]) -> i32 {
    const ENTRIES: &[&str] = &[
        "mx-decision-hold.sh",
        "mx-maintainer-override.sh",
        "mx-override-bindings.sh",
        "mx-override-run.sh",
        "mx-workflow.sh",
    ];
    if !ENTRIES.contains(&entry) {
        eprintln!("error: unknown authority entry point: {entry}");
        return 2;
    }
    if entry == "mx-maintainer-override.sh" {
        return run_override(args);
    }
    if entry == "mx-override-run.sh" {
        return override_run(args);
    }
    if entry == "mx-override-bindings.sh" {
        return validation_bindings_command(args);
    }
    if entry == "mx-decision-hold.sh" {
        return decision_hold_command(args);
    }
    if entry == "mx-workflow.sh"
        && matches!(
            args.first().and_then(|value| value.to_str()),
            Some("parse-json" | "output-path" | "substitute")
        )
    {
        return workflow_adapter(args);
    }
    if entry == "mx-workflow.sh" {
        return crate::workflow_runtime::run(args);
    }
    unreachable!("closed authority entry list")
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ValidationState {
    gate_run_digest: String,
    requested_sha: String,
    worktree_head: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ValidationBindings {
    pub boundary: String,
    pub task: String,
    pub project: String,
    pub operation: String,
    pub target: String,
    pub expected_state_digest: String,
    pub consequence: String,
    state: ValidationState,
}

fn file_digest(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .unwrap_or_else(|_| "absent".to_owned())
}

fn project_slug(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if value.is_empty() {
        "unknown".to_owned()
    } else {
        value
    }
}

pub(crate) fn validation_bindings(id: &str, sha: &str) -> Result<ValidationBindings, String> {
    if !safe_slug(id)
        || !(sha.len() >= 2
            && sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    {
        return Err("invalid task id or commit SHA".to_owned());
    }
    let state = state_root();
    let gate = state.join(format!("{id}.gate"));
    let run = gate.join("run.json");
    let run_meta = fs::symlink_metadata(&run).map_err(|_| "gate run is unavailable")?;
    if !run_meta.is_file() || run_meta.file_type().is_symlink() {
        return Err("gate run is unavailable".to_owned());
    }
    let run_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&run).map_err(|_| "gate run is unavailable")?)
            .map_err(|_| "gate run is unavailable")?;
    let mut repo = run_json
        .get("worktree")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if repo.is_empty() {
        let meta = fs::read_to_string(state.join(format!("{id}.meta"))).unwrap_or_default();
        repo = meta
            .lines()
            .find_map(|line| line.strip_prefix("worktree="))
            .unwrap_or_default()
            .to_owned();
    }
    let actual = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unreadable".to_owned());
    let validation_state = ValidationState {
        gate_run_digest: file_digest(&run),
        requested_sha: sha.to_owned(),
        worktree_head: actual,
    };
    let state_json = serde_json::to_string(&validation_state).map_err(|error| error.to_string())?;
    let repo_name = Path::new(&repo)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    Ok(ValidationBindings {
        boundary: "validation.waive-gate".to_owned(),
        task: id.to_owned(),
        project: project_slug(repo_name),
        operation: format!("waive validation gate for {id} at {sha}"),
        target: format!("{}@{sha}", gate.display()),
        expected_state_digest: maintainer_override::sha256_text(&state_json),
        consequence: "Create a maintainer-waived delivery handoff for this exact SHA without recording validation as passed.".to_owned(),
        state: validation_state,
    })
}

fn validation_bindings_command(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("mx-override-bindings: arguments must be UTF-8");
        return 1;
    };
    match override_bindings(&values) {
        Ok(binding) => {
            println!("{}", serde_json::to_string(&binding).expect("binding JSON"));
            0
        }
        Err(error) => {
            eprintln!("mx-override-bindings: {error}");
            1
        }
    }
}

fn home_root() -> PathBuf {
    std::env::var_os("MX_HOME")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn metadata_value(path: &Path, key: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_default()
        .to_owned()
}

fn git_output(directory: &Path, args: &[&str], fallback: &str) -> String {
    git_line(directory, args).unwrap_or_else(|| fallback.to_owned())
}

fn directory_inventory(directory: &Path, suffix: &str) -> String {
    let mut entries = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.ends_with(suffix) && regular_file(&path))
                .then(|| format!("{name}:{}", file_digest(&path)))
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.join("\n")
}

fn emit_binding(
    boundary: &str,
    task: &str,
    project: &str,
    operation: String,
    target: String,
    consequence: &str,
    state: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let state_text = serde_json::to_string(&state).map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "boundary": boundary,
        "task": task,
        "project": project,
        "operation": operation,
        "target": target,
        "expected_state_digest": maintainer_override::sha256_text(&state_text),
        "consequence": consequence,
        "state": state,
    }))
}

pub(crate) fn override_bindings(values: &[String]) -> Result<serde_json::Value, String> {
    let Some(mode) = values.first().map(String::as_str) else {
        return Err("unknown binding mode: ".to_owned());
    };
    let state_root = state_root();
    match mode {
        "validation" => {
            let [_, id, sha] = values else {
                return Err("validation requires task id and commit SHA".to_owned());
            };
            let binding = validation_bindings(id, sha)?;
            serde_json::to_value(binding).map_err(|error| error.to_string())
        }
        "cleanup" => {
            let [_, id] = values else {
                return Err("cleanup requires one task id".to_owned());
            };
            if !safe_slug(id) {
                return Err("invalid task id".to_owned());
            }
            let metadata = state_root.join(format!("{id}.meta"));
            if !regular_file(&metadata) {
                return Err("task metadata is unavailable".to_owned());
            }
            let worktree = metadata_value(&metadata, "worktree");
            let project_path = metadata_value(&metadata, "project");
            let home = metadata_value(&metadata, "home");
            let kind = match metadata_value(&metadata, "kind") {
                value if value.is_empty() => "delivery".to_owned(),
                value => value,
            };
            let target = if kind == "daemon" && !home.is_empty() {
                home
            } else {
                worktree.clone()
            };
            if target.is_empty() {
                return Err("task cleanup target is unavailable".to_owned());
            }
            let (head, status_digest) = if Path::new(&worktree).is_dir() {
                let status = git_output(
                    Path::new(&worktree),
                    &["status", "--porcelain=v1", "--untracked-files=all"],
                    "unreadable",
                );
                (
                    git_output(
                        Path::new(&worktree),
                        &["rev-parse", "--verify", "HEAD"],
                        "unreadable",
                    ),
                    maintainer_override::sha256_text(&status),
                )
            } else {
                ("absent".to_owned(), "absent".to_owned())
            };
            let children = if kind == "daemon" && Path::new(&target).join("state").is_dir() {
                directory_inventory(&Path::new(&target).join("state"), ".meta")
            } else {
                "absent".to_owned()
            };
            emit_binding(
                "cleanup.discard-unlanded",
                id,
                &project_slug(
                    Path::new(&project_path)
                        .file_name()
                        .and_then(|v| v.to_str())
                        .unwrap_or_default(),
                ),
                format!("discard task resources for {id}"),
                target,
                "Discard exactly the inventoried unlanded task material and retire only this task's resources.",
                serde_json::json!({
                    "meta_digest": file_digest(&metadata),
                    "head": head,
                    "status_digest": status_digest,
                    "ready_to_push_digest": file_digest(&state_root.join(format!("{id}.ready-to-push"))),
                    "report_digest": file_digest(&home_root().join("data").join(id).join("report.md")),
                    "child_inventory": children,
                }),
            )
        }
        "workflow-skip" | "workflow-reorder" => {
            let needed = if mode == "workflow-skip" { 3 } else { 4 };
            if values.len() != needed {
                return Err(if mode == "workflow-skip" {
                    "workflow-skip requires run and stage"
                } else {
                    "workflow-reorder requires run, stage, and before-stage"
                }
                .to_owned());
            }
            let run_id = &values[1];
            let stage = &values[2];
            let before = values.get(3).map(String::as_str).unwrap_or_default();
            if !safe_slug(run_id) || !safe_slug(stage) {
                return Err("invalid workflow identity".to_owned());
            }
            let directory = state_root.join(format!("{run_id}.workflow"));
            let run = directory.join("run.json");
            let definition = directory.join("definition.json");
            if !regular_file(&run) || !regular_file(&definition) {
                return Err("workflow run is unavailable".to_owned());
            }
            let repo = fs::read(&run)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|value| {
                    value
                        .get("repo")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_default();
            let state = serde_json::json!({
                "run_digest": file_digest(&run),
                "definition_digest": file_digest(&definition),
                "order_digest": file_digest(&directory.join("stage-order.json")),
                "stage_record_digest": file_digest(&directory.join("stages").join(format!("{stage}.json"))),
                "before_record_digest": file_digest(&directory.join("stages").join(format!("{before}.json"))),
                "stage": stage,
                "before_stage": before,
            });
            let project = project_slug(
                Path::new(&repo)
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default(),
            );
            if mode == "workflow-skip" {
                emit_binding(
                    "workflow.skip-stage",
                    run_id,
                    &project,
                    format!("skip workflow stage {stage} in run {run_id}"),
                    format!("{}#{stage}", directory.display()),
                    "Skip only the named stage and preserve every other snapshotted stage.",
                    state,
                )
            } else {
                emit_binding(
                    "workflow.reorder-stage",
                    run_id,
                    &project,
                    format!("move workflow stage {stage} before {before} in run {run_id}"),
                    format!("{}#{stage}-before-{before}", directory.display()),
                    "Move only the named stage before the named target and preserve every other snapshotted stage.",
                    state,
                )
            }
        }
        "single-checkout" => {
            let [_, id, project] = values else {
                return Err("single-checkout requires task id and project directory".to_owned());
            };
            if !safe_slug(id) {
                return Err("invalid task id".to_owned());
            }
            let project = PathBuf::from(project)
                .canonicalize()
                .map_err(|_| "project directory is unavailable")?;
            let top = git_line(&project, &["rev-parse", "--show-toplevel"])
                .and_then(|value| PathBuf::from(value).canonicalize().ok())
                .filter(|value| value == &project)
                .ok_or("project is not a git checkout root")?;
            let status = git_output(
                &project,
                &["status", "--porcelain=v1", "--untracked-files=all"],
                "unreadable",
            );
            let mut active = fs::read_dir(&state_root)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    (name.ends_with(".meta")
                        && regular_file(&path)
                        && metadata_value(&path, "project") == project.to_string_lossy())
                    .then(|| format!("{name}:{}", file_digest(&path)))
                })
                .collect::<Vec<_>>();
            active.sort();
            let project_text = project.to_string_lossy();
            let reservation = state_root.join(format!(
                ".single-checkout-{}.json",
                maintainer_override::sha256_text(&project_text)
            ));
            emit_binding(
                "isolation.single-checkout",
                id,
                &project_slug(
                    project
                        .file_name()
                        .and_then(|v| v.to_str())
                        .unwrap_or_default(),
                ),
                format!("launch task {id} in serialized single-checkout mode"),
                top.to_string_lossy().into_owned(),
                "Run only this task in the named checkout, record the loss of isolation, and exclude another single-checkout task until teardown.",
                serde_json::json!({
                    "head": git_output(&project, &["rev-parse", "--verify", "HEAD"], "unreadable"),
                    "branch": git_output(&project, &["symbolic-ref", "--quiet", "--short", "HEAD"], "detached"),
                    "status_digest": maintainer_override::sha256_text(&status),
                    "active_task_inventory": active.join("\n"),
                    "reservation_digest": file_digest(&reservation),
                }),
            )
        }
        "terminate-owner" => {
            let [_, pid] = values else {
                return Err("terminate-owner requires one harness pid".to_owned());
            };
            if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("invalid harness pid".to_owned());
            }
            let lock = state_root.join(".lock");
            if !regular_file(&lock) {
                return Err("session lock is unavailable".to_owned());
            }
            if fs::read_to_string(&lock).unwrap_or_default().trim() != pid {
                return Err("session lock owner changed".to_owned());
            }
            let output = Command::new("ps")
                .args(["-o", "args=", "-p", pid])
                .output()
                .ok();
            let command = output
                .as_ref()
                .filter(|result| result.status.success())
                .map(|result| String::from_utf8_lossy(&result.stdout).trim().to_owned())
                .unwrap_or_default();
            let basename = command
                .split_whitespace()
                .next()
                .and_then(|value| Path::new(value).file_name())
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let verified = ["claude", "codex", "cursor-agent", "pi"]
                .iter()
                .any(|name| basename.contains(name) || command.contains(name));
            let alive = Command::new("kill")
                .args(["-0", pid])
                .status()
                .is_ok_and(|status| status.success());
            if !alive || !verified {
                return Err("session lock owner is not a live verified harness".to_owned());
            }
            emit_binding(
                "session.terminate-owner",
                "broker-session",
                "multplx",
                format!("terminate live broker harness pid {pid} and reacquire session lock"),
                format!("harness-pid:{pid}"),
                "Send TERM only to the verified competing harness, prove it exited, then acquire the ordinary lock without bypassing it.",
                serde_json::json!({"lock_digest": file_digest(&lock), "pid": pid, "verified_harness_command": command}),
            )
        }
        _ => Err(format!("unknown binding mode: {mode}")),
    }
}

pub(crate) fn cleanup_binding(id: &str) -> Result<serde_json::Value, String> {
    override_bindings(&["cleanup".to_owned(), id.to_owned()])
}

pub(crate) fn single_checkout_binding(
    id: &str,
    project: &Path,
) -> Result<serde_json::Value, String> {
    override_bindings(&[
        "single-checkout".to_owned(),
        id.to_owned(),
        project
            .to_str()
            .ok_or("project path is not valid UTF-8")?
            .to_owned(),
    ])
}

fn executable_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return path.is_file().then_some(path);
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

#[derive(Serialize)]
struct DirectWriteState {
    checkout_head: String,
    checkout_branch: String,
    checkout_status_digest: String,
}

#[derive(Serialize)]
struct InstallState {
    required_command: String,
    current_path: String,
    host: String,
}

#[derive(Serialize)]
struct ElevationState {
    cwd: String,
    uid: String,
    executable: String,
}

fn git_line(directory: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_owned()
    })
}

fn override_action_state(
    boundary: &str,
    target: &mut String,
    verify_command: &str,
    executable: &str,
) -> Result<String, String> {
    match boundary {
        "project.direct-write" => {
            let canonical = PathBuf::from(&*target).canonicalize().map_err(
                |_| "target or capability binding is not valid for project.direct-write",
            )?;
            let top = git_line(&canonical, &["rev-parse", "--show-toplevel"])
                .and_then(|path| PathBuf::from(path).canonicalize().ok())
                .filter(|path| path == &canonical)
                .ok_or("target or capability binding is not valid for project.direct-write")?;
            *target = top.to_string_lossy().into_owned();
            let head = git_line(&top, &["rev-parse", "--verify", "HEAD"])
                .ok_or("target or capability binding is not valid for project.direct-write")?;
            let branch = git_line(&top, &["symbolic-ref", "--quiet", "--short", "HEAD"])
                .unwrap_or_else(|| "detached".to_owned());
            let status = git_line(&top, &["status", "--porcelain=v1", "--untracked-files=all"])
                .ok_or("target or capability binding is not valid for project.direct-write")?;
            serde_json::to_string(&DirectWriteState {
                checkout_head: head,
                checkout_branch: branch,
                checkout_status_digest: maintainer_override::sha256_text(&status),
            })
            .map_err(|error| error.to_string())
        }
        "dependency.install" => {
            if !safe_slug(verify_command) || *target != format!("command:{verify_command}") {
                return Err(
                    "target or capability binding is not valid for dependency.install".to_owned(),
                );
            }
            let host = Command::new("uname")
                .args(["-s", "-m"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| {
                    String::from_utf8_lossy(&output.stdout)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join("-")
                })
                .unwrap_or_default();
            serde_json::to_string(&InstallState {
                required_command: verify_command.to_owned(),
                current_path: executable_path(verify_command)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "absent".to_owned()),
                host,
            })
            .map_err(|error| error.to_string())
        }
        "security.one-action-elevation" => {
            let uid = Command::new("id")
                .arg("-u")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                .unwrap_or_default();
            serde_json::to_string(&ElevationState {
                cwd: std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                uid,
                executable: executable_path(executable)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unavailable".to_owned()),
            })
            .map_err(|error| error.to_string())
        }
        _ => Err(format!(
            "boundary does not use the exact-command runner: {boundary}"
        )),
    }
}

fn override_run(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("mx-override-run: arguments must be UTF-8");
        return 2;
    };
    let Some(first) = values.first() else {
        eprintln!("mx-override-run: request id or --print-bindings is required");
        return 2;
    };
    let print_only = first == "--print-bindings";
    let request = (!print_only).then_some(first.as_str());
    let mut index = 1;
    let mut boundary = String::new();
    let mut task = String::new();
    let mut project = String::new();
    let mut target = String::new();
    let mut verify_command = String::new();
    while index < values.len() && values[index] != "--" {
        let field = values[index].as_str();
        let Some(value) = values.get(index + 1) else {
            eprintln!("mx-override-run: {field} requires a value");
            return 2;
        };
        match field {
            "--boundary" => boundary = value.clone(),
            "--task" => task = value.clone(),
            "--project" => project = value.clone(),
            "--target" => target = value.clone(),
            "--verify-command" => verify_command = value.clone(),
            _ => {
                eprintln!("mx-override-run: unknown argument: {field}");
                return 2;
            }
        }
        index += 2;
    }
    if values.get(index).is_some_and(|value| value == "--") {
        index += 1;
    }
    let command = &values[index..];
    if boundary.is_empty()
        || task.is_empty()
        || project.is_empty()
        || target.is_empty()
        || command.is_empty()
        || !safe_slug(&task)
        || !safe_slug(&project)
    {
        eprintln!("mx-override-run: every binding and one command are required");
        return 2;
    }
    let operation = serde_json::to_string(command).expect("operation JSON");
    let executable = &command[0];
    let state = match override_action_state(&boundary, &mut target, &verify_command, executable) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("mx-override-run: {error}");
            return 2;
        }
    };
    let digest = maintainer_override::sha256_text(&state);
    let consequence = match boundary.as_str() {
        "project.direct-write" => "Run only the exact argv from the named checkout and report its resulting git-state digest for ordinary validation and delivery.".to_owned(),
        "dependency.install" => format!("Run only the exact installer argv and report success only if command {verify_command} is discoverable afterward."),
        "security.one-action-elevation" => "Run only the exact elevated argv once while leaving every other sandbox and command guard unchanged.".to_owned(),
        _ => unreachable!(),
    };
    if print_only {
        let value = serde_json::json!({
            "boundary": boundary,
            "task": task,
            "project": project,
            "operation": operation,
            "target": target,
            "expected_state_digest": digest,
            "consequence": consequence,
            "state": serde_json::from_str::<serde_json::Value>(&state).expect("state JSON")
        });
        println!("{}", serde_json::to_string(&value).expect("binding JSON"));
        return 0;
    }
    let binding = Binding {
        boundary: &boundary,
        task: &task,
        project: &project,
        operation: &operation,
        target: &target,
        expected_state_digest: &digest,
    };
    let store = OverrideStore::new(&state_root());
    let request = request.expect("not print-only");
    if store.consume(request, &binding).is_err() {
        return 1;
    }
    let status = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(if boundary == "project.direct-write" {
            Path::new(&target)
        } else {
            Path::new(".")
        })
        .status()
        .ok()
        .and_then(|status| status.code())
        .unwrap_or(1);
    let succeeded = status == 0
        && (boundary != "dependency.install" || executable_path(&verify_command).is_some());
    let resulting = override_action_state(&boundary, &mut target, &verify_command, executable)
        .unwrap_or_else(|_| "unavailable".to_owned());
    let detail = if succeeded {
        format!(
            "exact {boundary} action completed; resulting state digest {}",
            maintainer_override::sha256_text(&resulting)
        )
    } else {
        format!(
            "exact {boundary} action failed or capability verification failed with status {status}"
        )
    };
    let _ = store.result(request, succeeded, &detail);
    if succeeded { 0 } else { status.max(1) }
}

fn workflow_adapter(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("mx-workflow: arguments must be UTF-8");
        return 1;
    };
    match values.as_slice() {
        [command, path] if command == "parse-json" => {
            match multplx_domain::workflow::parse(&PathBuf::from(path)) {
                Ok(definition) => match serde_json::to_string_pretty(&definition) {
                    Ok(json) => {
                        println!("{json}");
                        0
                    }
                    Err(error) => {
                        eprintln!("mx-workflow: {error}");
                        1
                    }
                },
                Err(error) => {
                    eprintln!("mx-workflow: {error}");
                    1
                }
            }
        }
        [command, home, declared, run] if command == "output-path" => {
            match multplx_domain::workflow::output_path(
                PathBuf::from(home).as_path(),
                declared,
                run,
            ) {
                Ok(path) => {
                    println!("{}", path.display());
                    0
                }
                Err(error) => {
                    eprintln!("mx-workflow: {error}");
                    1
                }
            }
        }
        [command, value, run, input, output] if command == "substitute" => {
            println!(
                "{}",
                multplx_domain::workflow::substitute(value, run, input, output)
            );
            0
        }
        _ => {
            eprintln!("mx-workflow: invalid internal workflow adapter invocation");
            2
        }
    }
}

fn state_root() -> PathBuf {
    if let Some(state) = std::env::var_os("MX_STATE_OVERRIDE") {
        return PathBuf::from(state);
    }
    let home = std::env::var_os("MX_HOME")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("state")
}

fn safe_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn decision_hold_command(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("mx-decision-hold: arguments must be UTF-8");
        return 1;
    };
    let result = match values.first().map(String::as_str) {
        Some("id") => decision_id(&values[1..]).map(|output| println!("{output}")),
        Some("hold") => decision_hold(&values[1..]).map(|output| println!("{output}")),
        Some("complete") => decision_complete(&values[1..]).map(|output| println!("{output}")),
        Some("verify") => decision_verify(&values[1..]).map(|output| println!("{output}")),
        Some("resolve") => decision_resolve(&values[1..]).map(|output| println!("{output}")),
        Some("-h" | "--help") => {
            print!("{DECISION_HOLD_USAGE}");
            return 0;
        }
        _ => {
            eprint!("{DECISION_HOLD_USAGE}");
            return 2;
        }
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("mx-decision-hold: {error}");
            1
        }
    }
}

const DECISION_HOLD_USAGE: &str = "Usage:\n  mx-decision-hold.sh id <origin-id> <decision-key>\n  mx-decision-hold.sh hold <origin-id> <decision-key> --title <title> --reason <reason> [--repo <repo>]\n  mx-decision-hold.sh complete <origin-id> (--none | <decision-key>...)\n  mx-decision-hold.sh verify <origin-id>\n  mx-decision-hold.sh resolve <origin-id> <decision-key> --decision-file <path> --routed-to <task-id> [--routed-to <task-id>...]\n";

fn data_root() -> PathBuf {
    std::env::var_os("MX_DATA_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_root().join("data"))
}

fn decision_identity(origin: &str, key: &str) -> Result<String, String> {
    use multplx_domain::decision_hold::HoldIdentity;
    if !safe_slug(origin) {
        return Err(format!(
            "origin-id must be a non-empty privacy-safe slug: {origin}"
        ));
    }
    if !safe_slug(key) {
        return Err(format!(
            "decision-key must be a non-empty privacy-safe slug: {key}"
        ));
    }
    HoldIdentity::parse(origin, key)
        .map(|identity| identity.id())
        .map_err(|error| error.to_string())
}

fn decision_id(values: &[String]) -> Result<String, String> {
    let [origin, key] = values else {
        return Err("id requires origin-id and decision-key".to_owned());
    };
    decision_identity(origin, key)
}

fn one_line(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if value.contains(['\r', '\n']) {
        return Err(format!("{label} must be one line"));
    }
    Ok(())
}

fn backlog_store() -> Result<multplx_domain::backlog::BacklogStore, String> {
    let path = data_root().join("backlog.md");
    if !path.is_file() {
        return Err(format!("backlog is absent: {}", path.display()));
    }
    let store = multplx_domain::backlog::BacklogStore::new(path);
    store
        .validate()
        .map_err(|_| "backlog format is invalid".to_owned())?;
    Ok(store)
}

fn origin_exists(store: &multplx_domain::backlog::BacklogStore, origin: &str) -> bool {
    state_root().join(format!("{origin}.meta")).is_file()
        || data_root().join(origin).join("report.md").is_file()
        || store.snapshot(origin).is_ok()
}

fn durable_hold(store: &multplx_domain::backlog::BacklogStore, id: &str) -> Result<(), String> {
    let item = store.snapshot(id).map_err(|_| {
        format!(
            "maintainer decision {id} is absent from {}/backlog.md",
            data_root().display()
        )
    })?;
    let active = item.state == "queued"
        && item.held
        && item.kind == "maintainer"
        && item.hold_kind == "maintainer";
    let resolved = item.state == "done"
        && item.kind == "maintainer"
        && item
            .body
            .contains("Resolution recorded by mx-decision-hold.")
        && item.body.contains("Routed work:");
    if active || resolved {
        Ok(())
    } else {
        Err(format!(
            "maintainer decision {id} is neither actively held nor durably resolved"
        ))
    }
}

fn active_hold(
    store: &multplx_domain::backlog::BacklogStore,
    id: &str,
) -> Result<multplx_domain::backlog::ItemSnapshot, String> {
    let item = store.snapshot(id).map_err(|_| {
        format!(
            "maintainer hold {id} is absent from {}/backlog.md",
            data_root().display()
        )
    })?;
    if item.state != "queued" {
        return Err(format!(
            "maintainer hold {id} is not queued (state={})",
            item.state
        ));
    }
    if !item.held {
        return Err(format!("maintainer hold {id} is not active"));
    }
    if item.kind != "maintainer" {
        return Err(format!("backlog item {id} is not kind maintainer"));
    }
    if item.hold_kind != "maintainer" {
        return Err(format!("backlog item {id} is not held for the maintainer"));
    }
    Ok(item)
}

fn decision_hold(values: &[String]) -> Result<String, String> {
    if values.len() < 2 {
        return Err("hold requires origin-id and decision-key".to_owned());
    }
    let origin = &values[0];
    let key = &values[1];
    let id = decision_identity(origin, key)?;
    let mut title = None;
    let mut reason = None;
    let mut repo = None;
    let mut index = 2;
    while index < values.len() {
        let value = values
            .get(index + 1)
            .ok_or("option requires a value")?
            .clone();
        match values[index].as_str() {
            "--title" => title = Some(value),
            "--reason" => reason = Some(value),
            "--repo" => repo = Some(value),
            _ => return Err(format!("unknown argument: {}", values[index])),
        }
        index += 2;
    }
    let title = title.ok_or("title must not be empty")?;
    let reason = reason.ok_or("reason must not be empty")?;
    one_line("title", &title)?;
    one_line("reason", &reason)?;
    if reason.contains(['(', ')']) {
        return Err("reason must not contain parentheses (backlog hold contract)".to_owned());
    }
    let store = backlog_store()?;
    if !origin_exists(&store, origin) {
        return Err(format!(
            "origin {origin} is not owned by the active home {}",
            home_root().display()
        ));
    }
    match store.snapshot(&id) {
        Ok(item) => {
            if item.state == "done" {
                return Err(format!(
                    "maintainer decision {id} is already durably resolved; use a new decision key for a new decision"
                ));
            }
            if item.kind != "maintainer" {
                return Err(format!(
                    "existing backlog identity {id} is not kind maintainer"
                ));
            }
            if item.title != title {
                return Err(format!(
                    "existing maintainer hold {id} has a different title"
                ));
            }
        }
        Err(_) => {
            let repo = repo.unwrap_or_else(|| {
                let project =
                    metadata_value(&state_root().join(format!("{origin}.meta")), "project");
                Path::new(&project)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("broker")
                    .to_owned()
            });
            one_line("repo", &repo)?;
            store
                .add(&multplx_domain::backlog::AddRequest {
                    id: &id,
                    title: &title,
                    repo: &repo,
                    kind: "maintainer",
                    body: &format!("Origin: {origin}\nDecision key: {key}\nState: awaiting maintainer decision."),
                    start: false,
                    blockers: &[],
                })
                .map_err(|_| format!("could not create maintainer decision item {id}"))?;
        }
    }
    store
        .hold(&id, &reason, "maintainer")
        .map_err(|_| format!("could not activate maintainer hold {id}"))?;
    active_hold(&store, &id)?;
    decision_journal(
        origin,
        multplx_core::journal::JournalEvent::HoldOpened,
        &serde_json::json!({"decision_key": key, "hold_id": id, "title": title}),
    );
    Ok(id)
}

fn decision_journal(
    origin: &str,
    event: multplx_core::journal::JournalEvent,
    detail: &serde_json::Value,
) {
    let Ok(task) = multplx_core::identifiers::TaskId::parse(origin.to_owned()) else {
        return;
    };
    let now = time::OffsetDateTime::now_utc();
    let timestamp = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    if let Some(warning) = multplx_core::journal::JournalWriter::new(state_root()).try_emit(
        &task,
        event,
        detail,
        "mx-decision-hold",
        &timestamp,
    ) {
        eprintln!("{warning}");
    }
}

fn open_origin_decisions(origin: &str) -> Vec<multplx_core::classification::OpenStatus> {
    let status =
        fs::read_to_string(state_root().join(format!("{origin}.status"))).unwrap_or_default();
    let open = multplx_core::classification::open_decisions(&status, "resolved", "maintainer-held");
    if open.is_empty() {
        return open;
    }
    let meta = state_root().join(format!("{origin}.meta"));
    if !meta.is_file() {
        return open;
    }
    let kind = metadata_value(&meta, "kind");
    let last_verb = status
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(multplx_core::classification::status_line_verb)
        .unwrap_or_default();
    if kind != "daemon" && matches!(last_verb, "done" | "failed") {
        Vec::new()
    } else {
        open
    }
}

fn append_metadata(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|error| format!("could not update origin metadata: {error}"))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("could not update origin metadata: {error}"))
}

fn decision_complete(values: &[String]) -> Result<String, String> {
    if values.len() < 2 {
        return Err("complete requires origin-id and an inventory".to_owned());
    }
    let origin = &values[0];
    if !safe_slug(origin) {
        return Err(format!(
            "origin-id must be a non-empty privacy-safe slug: {origin}"
        ));
    }
    let supplied = &values[1..];
    let supplied = if supplied == ["--none"] {
        &[][..]
    } else {
        if supplied.iter().any(|value| value == "--none") {
            return Err("--none cannot be combined with decision keys".to_owned());
        }
        for key in supplied {
            decision_identity(origin, key)?;
        }
        supplied
    };
    let store = backlog_store()?;
    if !origin_exists(&store, origin) {
        return Err(format!(
            "origin {origin} is not owned by the active home {}",
            home_root().display()
        ));
    }
    let meta = state_root().join(format!("{origin}.meta"));
    let previous = metadata_value(&meta, "decision_keys");
    let mut inventory = multplx_domain::decision_hold::DecisionInventory::parse_csv(&previous)
        .map_err(|error| error.to_string())?;
    inventory
        .union(supplied.iter().map(String::as_str))
        .map_err(|error| error.to_string())?;
    let keys = inventory.render_csv();
    for key in keys.split(',').filter(|value| !value.is_empty()) {
        durable_hold(&store, &decision_identity(origin, key)?)?;
    }
    let open = open_origin_decisions(origin);
    inventory
        .verify_open(open.iter().map(|item| item.key.as_str()))
        .map_err(|error| {
            let message = error.to_string();
            let key = message.split_whitespace().nth(3).unwrap_or("unknown");
            format!(
                "open structured decision {origin}/{key} has no maintainer-held inventory entry"
            )
        })?;
    if meta.is_file() {
        if metadata_value(&meta, "decisions_reviewed") != "1" || previous != keys {
            append_metadata(
                &meta,
                &format!("decisions_reviewed=1\ndecision_keys={keys}\n"),
            )?;
        }
        let status = state_root().join(format!("{origin}.status"));
        let raw = fs::read_to_string(&status).unwrap_or_default();
        let raw_open =
            multplx_core::classification::open_decisions(&raw, "resolved", "maintainer-held");
        if !raw_open.is_empty() {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&status)
                .map_err(|error| format!("could not transfer decision status: {error}"))?;
            for item in raw_open
                .iter()
                .filter(|item| keys.split(',').any(|key| key == item.key))
            {
                writeln!(
                    file,
                    "maintainer-held [key={}]: tracked by {}",
                    item.key,
                    decision_identity(origin, &item.key)?
                )
                .map_err(|error| format!("could not transfer decision status: {error}"))?;
            }
        }
    }
    Ok(format!(
        "complete: {origin} decision inventory reviewed{}",
        if keys.is_empty() {
            String::new()
        } else {
            format!(" ({keys})")
        }
    ))
}

fn decision_verify(values: &[String]) -> Result<String, String> {
    let [origin] = values else {
        return Err("verify requires one origin-id".to_owned());
    };
    if !safe_slug(origin) {
        return Err(format!(
            "origin-id must be a non-empty privacy-safe slug: {origin}"
        ));
    }
    let meta = state_root().join(format!("{origin}.meta"));
    if !meta.is_file() {
        return Err(format!("origin metadata is absent: {}", meta.display()));
    }
    let store = backlog_store()?;
    if metadata_value(&meta, "decisions_reviewed") != "1" {
        return Err(format!(
            "origin {origin} has no completed unresolved-decision inventory"
        ));
    }
    let inventory = multplx_domain::decision_hold::DecisionInventory::parse_csv(&metadata_value(
        &meta,
        "decision_keys",
    ))
    .map_err(|error| error.to_string())?;
    for key in inventory
        .render_csv()
        .split(',')
        .filter(|value| !value.is_empty())
    {
        durable_hold(&store, &decision_identity(origin, key)?)?;
    }
    let open = open_origin_decisions(origin);
    inventory
        .verify_open(open.iter().map(|item| item.key.as_str()))
        .map_err(|_| {
            let key = open
                .first()
                .map(|item| item.key.as_str())
                .unwrap_or("unknown");
            format!("open structured decision {origin}/{key} is outside the reviewed inventory")
        })?;
    for item in open {
        durable_hold(&store, &decision_identity(origin, &item.key)?)?;
    }
    Ok(format!("verified: {origin} unresolved-decision inventory"))
}

pub(crate) fn verify_decision_completion(origin: &str) -> Result<(), String> {
    decision_verify(&[origin.to_owned()]).map(|_| ())
}

fn resolution_identity(
    body: &str,
) -> Result<multplx_domain::decision_hold::ResolutionIdentity, String> {
    let prefix = "Resolution recorded by mx-decision-hold.\nDecision digest: ";
    let rest = body
        .strip_prefix(prefix)
        .ok_or("maintainer hold has no retry identity record")?;
    let (digest, rest) = rest
        .split_once("\nRouted identities: ")
        .ok_or("maintainer hold has an invalid retry identity record")?;
    let (routes, _) = rest
        .split_once("\n\nMaintainer decision:")
        .ok_or("maintainer hold has an invalid retry identity record")?;
    Ok(multplx_domain::decision_hold::ResolutionIdentity {
        decision_digest: digest.to_owned(),
        routed_to: routes
            .split(',')
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    })
}

fn decision_resolve(values: &[String]) -> Result<String, String> {
    if values.len() < 2 {
        return Err("resolve requires origin-id and decision-key".to_owned());
    }
    let origin = &values[0];
    let key = &values[1];
    let id = decision_identity(origin, key)?;
    let mut decision_file = None;
    let mut routed = Vec::new();
    let mut index = 2;
    while index < values.len() {
        let value = values
            .get(index + 1)
            .ok_or("option requires a value")?
            .clone();
        match values[index].as_str() {
            "--decision-file" => decision_file = Some(value),
            "--routed-to" => {
                if !safe_slug(&value) {
                    return Err(format!(
                        "routed-task must be a non-empty privacy-safe slug: {value}"
                    ));
                }
                routed.push(value);
            }
            _ => return Err(format!("unknown argument: {}", values[index])),
        }
        index += 2;
    }
    let path = decision_file.ok_or("--decision-file is required")?;
    let decision = fs::read(&path).map_err(|_| format!("decision file does not exist: {path}"))?;
    let candidate = multplx_domain::decision_hold::ResolutionIdentity::new(&decision, routed)
        .map_err(|error| error.to_string())?;
    let store = backlog_store()?;
    if let Ok(item) = store.snapshot(&id)
        && item.state == "done"
        && item.kind == "maintainer"
        && item
            .body
            .contains("Resolution recorded by mx-decision-hold.")
        && item.body.contains("Routed work:")
    {
        resolution_identity(&item.body)?
            .accepts_retry(&candidate)
            .map_err(|_| {
                format!(
                    "maintainer hold {id} records a different maintainer decision or routed work"
                )
            })?;
        decision_journal(
            origin,
            multplx_core::journal::JournalEvent::HoldResolved,
            &serde_json::json!({"decision_key": key, "hold_id": id, "routed_to": candidate.routed_to}),
        );
        return Ok(format!("resolved: {id}"));
    }
    let hold = active_hold(&store, &id)?;
    let previously_recorded = hold
        .body
        .contains("Resolution recorded by mx-decision-hold.");
    if previously_recorded {
        resolution_identity(&hold.body)?
            .accepts_retry(&candidate)
            .map_err(|_| {
                format!(
                    "maintainer hold {id} records a different maintainer decision or routed work"
                )
            })?;
    }
    for task in &candidate.routed_to {
        let item = store
            .snapshot(task)
            .map_err(|_| format!("routed task {task} does not exist in the active home"))?;
        if item.state == "done" && !previously_recorded {
            return Err(format!("routed task {task} is already done"));
        }
        let recorded_route = hold
            .body
            .contains("Resolution recorded by mx-decision-hold.")
            && hold.body.contains(&format!("- {task}"));
        if !item.blockers.iter().any(|blocker| blocker == &id) && !recorded_route {
            return Err(format!("routed task {task} is not durably blocked by {id}"));
        }
    }
    let decision_text = String::from_utf8_lossy(&decision);
    let mut body = format!(
        "Resolution recorded by mx-decision-hold.\nDecision digest: {}\nRouted identities: {}\n\nMaintainer decision:\n{}\n\nRouted work:\n",
        candidate.decision_digest,
        candidate.routed_to.join(","),
        decision_text.trim_end_matches(['\r', '\n'])
    );
    for task in &candidate.routed_to {
        body.push_str(&format!("- {task}\n"));
    }
    store
        .update(&id, &body, false)
        .map_err(|_| format!("could not record the maintainer decision on {id}"))?;
    for task in &candidate.routed_to {
        let item = store
            .snapshot(task)
            .map_err(|_| format!("routed task {task} disappeared before routing"))?;
        if item.blockers.iter().any(|blocker| blocker == &id) {
            store
                .unblock(task, &id)
                .map_err(|_| format!("could not route the recorded decision to {task}"))?;
        }
    }
    store
        .done(&id, None, 20)
        .map_err(|_| format!("could not close resolved maintainer hold {id}"))?;
    let resolved = store.snapshot(&id).map_err(|_| {
        format!("maintainer hold {id} did not retain its durable resolution record")
    })?;
    if resolved.state != "done"
        || !resolved
            .body
            .contains("Resolution recorded by mx-decision-hold.")
    {
        return Err(format!(
            "maintainer hold {id} did not retain its durable resolution record"
        ));
    }
    decision_journal(
        origin,
        multplx_core::journal::JournalEvent::HoldResolved,
        &serde_json::json!({"decision_key": key, "hold_id": id, "routed_to": candidate.routed_to}),
    );
    Ok(format!(
        "resolved: {id} -> {}",
        candidate.routed_to.join(" ")
    ))
}

fn text_args(args: &[OsString]) -> Option<Vec<String>> {
    args.iter()
        .map(|value| value.to_str().map(ToOwned::to_owned))
        .collect()
}

fn override_error(message: impl AsRef<str>) -> i32 {
    eprintln!("mx-maintainer-override: {}", message.as_ref());
    1
}

fn usage_error(message: impl AsRef<str>) -> i32 {
    eprintln!("mx-maintainer-override: {}", message.as_ref());
    eprint!("{OVERRIDE_USAGE}");
    2
}

fn option_map(values: &[String]) -> std::result::Result<BTreeMap<String, String>, String> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < values.len() {
        let name = &values[index];
        if !name.starts_with("--") {
            return Err(format!("unknown argument: {name}"));
        }
        let Some(value) = values.get(index + 1) else {
            return Err(format!("{name} requires a non-empty value"));
        };
        if value.is_empty() {
            return Err(format!("{name} requires a non-empty value"));
        }
        options.insert(name.trim_start_matches("--").to_owned(), value.clone());
        index += 2;
    }
    Ok(options)
}

fn required<'a>(options: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    options
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

fn run_override(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        return override_error("arguments must be UTF-8");
    };
    let Some(command) = values.first().map(String::as_str) else {
        eprint!("{OVERRIDE_USAGE}");
        return 2;
    };
    let rest = &values[1..];
    let store = OverrideStore::new(&state_root());
    match command {
        "-h" | "--help" => {
            print!("{OVERRIDE_USAGE}");
            0
        }
        "registry" => command_registry(rest),
        "request" => command_request(&store, rest),
        "grant" | "deny" => command_decide(&store, command == "grant", rest),
        "consume" => command_consume(&store, rest),
        "result" => command_result(&store, rest),
        "inspect" => command_inspect(&store, rest),
        "audit" => command_audit(&store, rest),
        "digest" if rest.len() == 1 => {
            println!("{}", maintainer_override::sha256_text(&rest[0]));
            0
        }
        "digest" => usage_error("digest requires one literal argument"),
        "argv" => {
            println!("{}", serde_json::to_string(rest).expect("serialize argv"));
            0
        }
        "handoff" => command_handoff(&store, rest),
        _ => {
            eprint!("{OVERRIDE_USAGE}");
            2
        }
    }
}

fn command_registry(args: &[String]) -> i32 {
    match args {
        [] => {
            print!("{}", maintainer_override::registry_text());
            0
        }
        [flag] if flag == "--json" => {
            let rows = maintainer_override::REGISTRY
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "boundary_id": entry.id,
                        "class": entry.class.as_str(),
                        "alternate": entry.alternate,
                    })
                })
                .collect::<Vec<_>>();
            println!(
                "{}",
                serde_json::to_string_pretty(&rows).expect("registry JSON")
            );
            0
        }
        _ => usage_error("registry accepts only --json"),
    }
}

fn command_request(store: &OverrideStore, args: &[String]) -> i32 {
    let options = match option_map(args) {
        Ok(value) => value,
        Err(error) => {
            return usage_error(error.replace("unknown argument", "unknown request argument"));
        }
    };
    let fields = [
        "boundary",
        "task",
        "project",
        "operation",
        "target",
        "expected-state",
        "consequence",
    ];
    if fields.iter().any(|name| required(&options, name).is_none()) {
        return usage_error("request requires every binding field");
    }
    if options
        .keys()
        .any(|name| !fields.contains(&name.as_str()) && name != "ttl")
    {
        return usage_error("unknown request argument");
    }
    let ttl = options.get("ttl").map_or_else(
        || {
            std::env::var("MX_OVERRIDE_DEFAULT_TTL")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(maintainer_override::DEFAULT_TTL)
        },
        |value| value.parse::<u64>().unwrap_or(0),
    );
    let request = Request {
        boundary: required(&options, "boundary").expect("checked"),
        task: required(&options, "task").expect("checked"),
        project: required(&options, "project").expect("checked"),
        operation: required(&options, "operation").expect("checked"),
        target: required(&options, "target").expect("checked"),
        expected_state_digest: required(&options, "expected-state").expect("checked"),
        consequence: required(&options, "consequence").expect("checked"),
        ttl,
    };
    match store.request(&request) {
        Ok(id) => {
            println!("{id}");
            0
        }
        Err(error) => override_error(error.to_string()),
    }
}

fn command_decide(store: &OverrideStore, grant: bool, args: &[String]) -> i32 {
    let label = if grant { "grant" } else { "deny" };
    let Some(request) = args.first() else {
        return usage_error(format!("{label} requires a request id"));
    };
    let options = match option_map(&args[1..]) {
        Ok(value) => value,
        Err(error) => return usage_error(error),
    };
    let Some(words) = required(&options, "maintainer-words") else {
        return usage_error(format!("{label} requires --maintainer-words"));
    };
    if options.len() != 1 {
        return usage_error(format!("unknown {label} argument"));
    }
    if let Err(error) = maintainer_override::require_primary_lock(&state_root()) {
        return override_error(error.to_string());
    }
    match store.decide(request, words, grant) {
        Ok(()) => 0,
        Err(error) => override_error(error.to_string()),
    }
}

fn binding_from(options: &BTreeMap<String, String>) -> Option<Binding<'_>> {
    Some(Binding {
        boundary: required(options, "boundary")?,
        task: required(options, "task")?,
        project: required(options, "project")?,
        operation: required(options, "operation")?,
        target: required(options, "target")?,
        expected_state_digest: required(options, "expected-state")?,
    })
}

fn command_consume(store: &OverrideStore, args: &[String]) -> i32 {
    let Some(request) = args.first() else {
        return usage_error("consume requires a request id");
    };
    let options = match option_map(&args[1..]) {
        Ok(value) => value,
        Err(error) => return usage_error(error),
    };
    let Some(binding) = binding_from(&options) else {
        return usage_error("consume requires every binding field");
    };
    if options.len() != 6 {
        return usage_error("unknown consume argument");
    }
    match store.consume(request, &binding) {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(error) => override_error(error.to_string()),
    }
}

fn command_result(store: &OverrideStore, args: &[String]) -> i32 {
    let Some(request) = args.first() else {
        return usage_error("result requires a request id");
    };
    let options = match option_map(&args[1..]) {
        Ok(value) => value,
        Err(error) => return usage_error(error),
    };
    let (Some(outcome), Some(detail)) =
        (required(&options, "outcome"), required(&options, "detail"))
    else {
        return usage_error("result requires --outcome and --detail");
    };
    let succeeded = match outcome {
        "succeeded" => true,
        "failed" => false,
        _ => return override_error("result must be succeeded or failed"),
    };
    match store.result(request, succeeded, detail) {
        Ok(()) => 0,
        Err(error) => override_error(error.to_string()),
    }
}

fn sorted_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries = object.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted_json(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(sorted_json).collect())
        }
        value => value,
    }
}

fn command_inspect(store: &OverrideStore, args: &[String]) -> i32 {
    let [request] = args else {
        return usage_error("inspect requires one request id");
    };
    match store.find(request) {
        Ok((_, _, record)) => {
            let value = serde_json::to_value(record).expect("record JSON");
            println!(
                "{}",
                serde_json::to_string_pretty(&sorted_json(value)).expect("pretty JSON")
            );
            0
        }
        Err(_) => override_error(format!("request not found or invalid: {request}")),
    }
}

fn command_audit(store: &OverrideStore, args: &[String]) -> i32 {
    let json = match args {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => return usage_error("audit accepts only --json"),
    };
    let (records, invalid) = store.audit();
    if json {
        let values = records
            .into_iter()
            .map(|(state, record)| {
                let mut value = serde_json::to_value(record).expect("record JSON");
                value.as_object_mut().expect("record object").insert(
                    "record_state".to_owned(),
                    serde_json::Value::String(state.as_str().to_owned()),
                );
                value
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&values).expect("audit JSON")
        );
    } else {
        for path in &invalid {
            println!("invalid\t{}", path.display());
        }
        for (state, record) in records {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                record.request_id,
                state.as_str(),
                record.boundary_id,
                record.task_id,
                record.project,
                record.target_identity,
                serde_json::to_value(record.outcome)
                    .expect("outcome")
                    .as_str()
                    .expect("outcome string")
            );
        }
    }
    if invalid.is_empty() { 0 } else { 1 }
}

fn command_handoff(store: &OverrideStore, args: &[String]) -> i32 {
    let [request] = args else {
        return usage_error("handoff requires one request id");
    };
    let Ok((state, _, record)) = store.find(request) else {
        return override_error(format!("request not found or invalid: {request}"));
    };
    if state != RecordState::Consumed || record.outcome != maintainer_override::Outcome::NotRun {
        return override_error("handoff requires an atomically consumed request with no outcome");
    }
    let permitted = maintainer_override::boundary(&record.boundary_id).is_some_and(|entry| {
        entry.class == BoundaryClass::Policy
            && matches!(
                entry.id,
                "authentication.login" | "delivery.credentialed-action"
            )
    });
    if !permitted {
        return override_error(format!(
            "boundary does not use operator handoff: {}",
            record.boundary_id
        ));
    }
    println!(
        "request={}\nboundary={}\ntarget={}\noperation={}\nconsequence={}",
        record.request_id,
        record.boundary_id,
        record.target_identity,
        record.action_argv_or_operation,
        record.consequence
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_entry_fails_before_execution() {
        assert_eq!(run("not-an-entry", &[]), 2);
    }
}

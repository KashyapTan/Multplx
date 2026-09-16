//! Transactional retirement of persistent daemon homes.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use multplx_core::filesystem::atomic_replace;
use multplx_core::identifiers::TaskId;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use rustix::fs::OFlags;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::home_seed::resolved;
use super::worktree::{command_output, command_output_with};

pub const USAGE: &str = "usage: mx teardown <task-id> [--stop-coordinator|--checkpoint|--stop-subtree|--retire-home] [--authority-state <absolute-path>]\n       mx teardown <task-id> --override <request-id> [--authority-state <absolute-path>]\n\n--authority-state routes a transferred task through its retained canonical record after validating the current owning coordinator.\n--stop-coordinator and --checkpoint stop only the verified coordinator endpoint and retain its children and home.\n--stop-subtree stops verified endpoints in the owned descendant tree and retains every task, worktree, outcome and home.\n--retire-home removes an idle home only after child ownership and parent-channel outcomes are settled.\nThe legacy one-argument form is retained as an alias for --retire-home.\n";
const JOURNAL_PREFIX: &str = ".teardown.transaction.";
const MARKER: &str = ".mx-daemon-home";
const CONTROL_PREFIX: &str = ".coordinator-control.";

#[derive(Clone, Debug)]
pub struct Context {
    pub root: PathBuf,
    pub home: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Output {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Journal {
    id: String,
    home: String,
    stage: String,
    #[serde(default)]
    home_allocation: Option<super::home_seed::HomeBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ControlReceipt {
    version: u32,
    id: String,
    operation: String,
    stage: String,
    coordinator_attempt: String,
    coordinator_endpoint: Option<String>,
    targets: Vec<ControlTarget>,
    stopped: Vec<String>,
    reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ControlTarget {
    path: String,
    task_id: String,
    attempt_id: Option<String>,
    generation: Option<u64>,
    endpoint: Option<String>,
}

fn control_target(path: &Path) -> Result<ControlTarget, String> {
    let task_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or("coordinator control target has an invalid task id")?
        .to_owned();
    let raw = String::from_utf8(read_regular(path, "coordinator control target")?)
        .map_err(|_| "coordinator control target is not UTF-8")?;
    let record = super::subagent_model::read_meta(&task_id, &raw)?;
    Ok(ControlTarget {
        path: path.to_string_lossy().into_owned(),
        task_id,
        attempt_id: record.attempt.as_ref().map(|attempt| attempt.id.clone()),
        generation: record.attempt.as_ref().map(|attempt| attempt.generation),
        endpoint: record.runtime.endpoint.clone(),
    })
}

fn error(status: i32, message: impl Into<String>) -> Output {
    Output {
        status,
        stdout: String::new(),
        stderr: format!("error: {}\n", message.into()),
    }
}

fn path_text(path: &Path, label: &str) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{label} is not valid UTF-8: {}", path.display()))
}

fn read_regular(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(OFlags::NOFOLLOW.bits() as i32)
        .open(path)
        .map_err(|error_value| format!("cannot open {label} {}: {error_value}", path.display()))?;
    let metadata = file.metadata().map_err(|error_value| {
        format!("cannot inspect {label} {}: {error_value}", path.display())
    })?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(format!(
            "{label} must be a regular single-link file: {}",
            path.display()
        ));
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(format!(
            "{label} is not owned by the current user: {}",
            path.display()
        ));
    }
    if metadata.len() > 16 * 1024 * 1024 {
        return Err(format!("{label} is unexpectedly large: {}", path.display()));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error_value| format!("cannot read {label} {}: {error_value}", path.display()))?;
    Ok(bytes)
}

fn require_owned_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error_value| {
        format!("cannot inspect {label} {}: {error_value}", path.display())
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{label} is not a real directory: {}",
            path.display()
        ));
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(format!(
            "{label} is not owned by the current user: {}",
            path.display()
        ));
    }
    fs::canonicalize(path)
        .map_err(|error_value| format!("cannot resolve {label} {}: {error_value}", path.display()))
}

fn metadata(path: &Path) -> Result<BTreeMap<String, String>, String> {
    metadata_for_retirement(path, false)
}

fn metadata_for_retirement(
    path: &Path,
    allow_retained_checkout: bool,
) -> Result<BTreeMap<String, String>, String> {
    let bytes = read_regular(path, "task metadata")?;
    let text =
        String::from_utf8(bytes).map_err(|_| "task metadata is not valid UTF-8".to_owned())?;
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if values.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(format!("REFUSED: duplicate task metadata field {key}"));
        }
    }
    if values.contains_key("canonical_model")
        || values.get("schema_version").is_some_and(|v| v != "1")
    {
        let id = path
            .file_stem()
            .and_then(|v| v.to_str())
            .ok_or("invalid canonical task metadata filename")?;
        let record = super::subagent_model::read_meta(id, &text)?;
        let owner = record
            .owner_home
            .as_deref()
            .ok_or("canonical cleanup owner home missing")?;
        let expected_state = record
            .owner_state
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(owner).join("state"));
        if fs::canonicalize(path.parent().ok_or("task metadata parent missing")?).ok()
            != fs::canonicalize(expected_state).ok()
        {
            return Err("REFUSED: canonical task cleanup owner state does not match".into());
        }
        if let Some(project) = &record.project {
            crate::project_registry::validate_binding(Path::new(owner), project)?;
        }
        // Only the explicit override path may inspect a retained primary
        // checkout. Its reservation is verified below and no checkout deletion
        // occurs there; ordinary and recursive cleanup retain the guard.
        if let Some(worktree) = values.get("worktree")
            && !(allow_retained_checkout
                && values.get("single_checkout").map(String::as_str) == Some("yes"))
        {
            crate::project_registry::protect_borrowed_checkouts(
                Path::new(owner),
                Path::new(worktree),
            )?;
        }
    }
    Ok(values)
}

fn is_strict_descendant(parent: &Path, child: &Path) -> bool {
    child != parent && child.starts_with(parent)
}

fn validate_removal_target(
    context: &Context,
    target: &Path,
    label: &str,
) -> Result<PathBuf, String> {
    let target = resolved(target);
    crate::project_registry::protect_borrowed_checkouts(&context.home, &target)?;
    let active = resolved(&context.home);
    let root = resolved(&context.root);
    let reason = if target == Path::new("/") {
        Some("is the filesystem root")
    } else if target == active {
        Some("is the active Multplx home")
    } else if target == root {
        Some("is the Multplx repo")
    } else if is_strict_descendant(&target, &active) {
        Some("is an ancestor of the active Multplx home")
    } else if is_strict_descendant(&target, &root) {
        Some("is an ancestor of the Multplx repo")
    } else if is_strict_descendant(&active, &target) {
        Some("is inside the active Multplx home")
    } else if is_strict_descendant(&root, &target) {
        Some("is inside the Multplx repo")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Err(format!(
            "REFUSED: unsafe {label} removal target {} {reason}",
            target.display()
        ));
    }
    Ok(target)
}

fn registry_home(line: &str) -> Option<&str> {
    let start = line.find("(home: ")? + "(home: ".len();
    let tail = &line[start..];
    let end = tail.find(';')?;
    Some(&tail[..end])
}

fn validate_task_target(
    context: &Context,
    id: &str,
    values: &BTreeMap<String, String>,
    path: &Path,
) -> Result<PathBuf, String> {
    let raw = values
        .iter()
        .map(|(key, value)| format!("{key}={value}\n"))
        .collect::<String>();
    let task = super::subagent_model::read_meta(id, &raw)?;
    if let Some(token) = task.allocation.as_ref() {
        let target = require_owned_directory(path, "task worktree")?;
        let store = super::worktree::Store::new(
            task.project.as_ref().ok_or("allocation project missing")?,
        )?;
        let allocation = store.inspect(&token.allocation_id)?;
        if allocation.binding != *token
            || Path::new(&token.path) != target
            || allocation.owner_home
                != fs::canonicalize(&context.home).map_err(|e| e.to_string())?
        {
            return Err("unsafe task target: allocation lease or path mismatch".into());
        }
        return Ok(target);
    }
    validate_removal_target(context, path, "task worktree")
}

fn validate_registry_descendants(registry: &Path, home: &Path) -> Result<(), String> {
    let bytes = match read_regular(registry, "daemon registry") {
        Ok(bytes) => bytes,
        Err(_) if !registry.exists() => return Ok(()),
        Err(error_value) => return Err(error_value),
    };
    let text =
        String::from_utf8(bytes).map_err(|_| "daemon registry is not valid UTF-8".to_owned())?;
    for line in text.lines().filter(|line| line.starts_with("- ")) {
        let Some(value) = registry_home(line) else {
            continue;
        };
        let registered = resolved(Path::new(value));
        if is_strict_descendant(home, &registered) {
            let id = line[2..].split_whitespace().next().unwrap_or("unknown");
            return Err(format!(
                "REFUSED: unsafe daemon home removal target {} contains registered daemon home {} for {id}",
                home.display(),
                registered.display()
            ));
        }
    }
    Ok(())
}

fn validate_pr_artifacts(state: &Path, id: &str) -> Result<(), String> {
    let state_meta = fs::symlink_metadata(state).map_err(|error_value| error_value.to_string())?;
    if state_meta.file_type().is_symlink() || !state_meta.is_dir() {
        return Err(format!(
            "REFUSED: unsafe task state directory {}; preserving task state.",
            state.display()
        ));
    }
    for suffix in [
        "check.sh",
        "pr-poll",
        "pr-poll-registration",
        "pr-poll-retirement",
        "check-trust",
    ] {
        let artifact = state.join(format!("{id}.{suffix}"));
        let Ok(metadata) = fs::symlink_metadata(&artifact) else {
            continue;
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.dev() != state_meta.dev()
        {
            return Err(
                "REFUSED: unsafe task PR-check artifact; preserving task state.".to_owned(),
            );
        }
    }
    let quarantine = state.join(".pr-check-quarantine");
    let Ok(metadata) = fs::symlink_metadata(&quarantine) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.dev() != state_meta.dev()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(format!(
            "REFUSED: unsafe PR-check quarantine path {}; preserving task state.",
            quarantine.display()
        ));
    }
    for entry in fs::read_dir(&quarantine).map_err(|error_value| error_value.to_string())? {
        let entry = entry.map_err(|error_value| error_value.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err("REFUSED: unsafe task quarantine entry; preserving task state.".to_owned());
        };
        if !name.starts_with(&format!("{id}.")) {
            continue;
        }
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|error_value| error_value.to_string())?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.dev() != state_meta.dev()
            || metadata.mode() & 0o777 != 0o600
        {
            return Err("REFUSED: unsafe task quarantine entry; preserving task state.".to_owned());
        }
        if id == "_noncanonical" {
            return Err(
                "REFUSED: unresolved legacy PR-check namespace collision; migrate quarantine artifacts before teardown."
                    .to_owned(),
            );
        }
    }
    Ok(())
}

fn listed_worktree(project: &Path, target: &Path) -> Result<bool, String> {
    let output = command_output(Command::new("git").args([
        "-C".as_ref(),
        project.as_os_str(),
        "-c".as_ref(),
        "core.quotePath=false".as_ref(),
        "worktree".as_ref(),
        "list".as_ref(),
        "--porcelain".as_ref(),
    ]))
    .map_err(|error_value| error_value.to_string())?;
    if !output.status.success() {
        return Ok(false);
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| "git worktree inventory is not valid UTF-8".to_owned())?;
    if text
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .any(|line| resolved(Path::new(line)) == resolved(target))
    {
        return Ok(true);
    }
    let origin = |directory: &Path| {
        git_text(directory, &["remote", "get-url", "origin"]).map(|value| {
            let path = Path::new(&value);
            if path.is_absolute() || value.starts_with('.') {
                resolved(&directory.join(path))
                    .to_string_lossy()
                    .into_owned()
            } else {
                value
            }
        })
    };
    Ok(origin(project).is_some_and(|project_origin| origin(target) == Some(project_origin)))
}

fn git_text(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output =
        command_output(Command::new("git").arg("-C").arg(directory).args(arguments)).ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_success(directory: &Path, arguments: &[&str]) -> bool {
    git_text(directory, arguments).is_some()
}

fn default_branch(project: &Path) -> Option<String> {
    git_text(
        project,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .map(|value| value.trim_start_matches("origin/").to_owned())
    .or_else(|| {
        ["main", "master"].into_iter().find_map(|branch| {
            git_success(
                project,
                &[
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{branch}"),
                ],
            )
            .then(|| branch.to_owned())
        })
    })
}

fn pr_number(target: &str) -> Option<String> {
    let raw = target
        .split_once("/pull/")
        .map_or(target, |(_, value)| value);
    let number = raw
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!number.is_empty()).then_some(number)
}

/// A known open PR always retains the resource, even when its patch is also
/// present on the default branch. Unavailable evidence for a recorded PR or a
/// GitHub remote is unknown, not permission to discard the branch.
pub(super) fn publication_blocked(worktree: &Path, recorded: Option<&str>) -> bool {
    let origin = git_text(worktree, &["remote", "get-url", "origin"]);
    if recorded.is_none() && origin.is_none() {
        return false;
    }
    let github = origin
        .as_deref()
        .is_some_and(|url| url.contains("github.com"));
    let target = if let Some(target) = recorded.filter(|value| !value.is_empty()) {
        target.to_owned()
    } else {
        let Some(branch) = git_text(worktree, &["rev-parse", "--abbrev-ref", "HEAD"]) else {
            return true;
        };
        let output = command_output(
            Command::new("gh")
                .args([
                    "pr",
                    "list",
                    "--state",
                    "all",
                    "--head",
                    &branch,
                    "--limit",
                    "1",
                    "--json",
                    "number",
                    "--jq",
                    ".[0].number",
                ])
                .current_dir(worktree),
        );
        let Ok(output) = output else {
            return github;
        };
        if !output.status.success() {
            return github;
        }
        let target = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if target.is_empty() || target == "null" {
            return false;
        }
        target
    };
    let output = command_output(
        Command::new("gh")
            .args([
                "pr",
                "view",
                &target,
                "--json",
                "state,headRefOid",
                "-q",
                ".state + \"\\t\" + .headRefOid",
            ])
            .current_dir(worktree),
    );
    match output {
        Ok(output) if output.status.success() => {
            pr_state_blocks_cleanup(&String::from_utf8_lossy(&output.stdout))
        }
        _ => true,
    }
}

fn pr_state_blocks_cleanup(row: &str) -> bool {
    let Some((state, head)) = row.trim().split_once('\t') else {
        return true;
    };
    head.is_empty() || !matches!(state.to_ascii_lowercase().as_str(), "merged" | "closed")
}

fn pr_landed(worktree: &Path, recorded: Option<&str>) -> bool {
    let branch = git_text(worktree, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|| "HEAD".to_owned());
    let target = recorded
        .map(str::to_owned)
        .or_else(|| {
            let output = command_output(
                Command::new("gh")
                    .args([
                        "pr",
                        "list",
                        "--state",
                        "all",
                        "--head",
                        &branch,
                        "--limit",
                        "1",
                        "--json",
                        "number",
                        "--jq",
                        ".[0].number",
                    ])
                    .current_dir(worktree),
            )
            .ok()?;
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        })
        .filter(|value| !value.is_empty());
    let Some(target) = target else { return false };
    let output = command_output(
        Command::new("gh")
            .args([
                "pr",
                "view",
                &target,
                "--json",
                "state,headRefOid",
                "-q",
                ".state + \"\\t\" + .headRefOid",
            ])
            .current_dir(worktree),
    );
    let Ok(output) = output else { return false };
    if !output.status.success() {
        return false;
    }
    let row = String::from_utf8_lossy(&output.stdout);
    let Some((state, head)) = row.trim().split_once('\t') else {
        return false;
    };
    if !state.eq_ignore_ascii_case("merged") || head.is_empty() {
        return false;
    }
    if !git_success(worktree, &["cat-file", "-e", &format!("{head}^{{commit}}")]) {
        let Some(number) = pr_number(&target) else {
            return false;
        };
        if !git_success(
            worktree,
            &[
                "fetch",
                "--quiet",
                "origin",
                &format!("refs/pull/{number}/head"),
            ],
        ) {
            return false;
        }
    }
    if git_success(worktree, &["merge-base", "--is-ancestor", "HEAD", head]) {
        return true;
    }
    let Some(base) = git_text(worktree, &["merge-base", "HEAD", head]) else {
        return false;
    };
    let Some(pr_commits) = git_text(
        worktree,
        &["log", "--format=%H", &format!("{base}..{head}")],
    ) else {
        return false;
    };
    let mut pr_patches = std::collections::BTreeSet::new();
    for commit in pr_commits.lines().filter(|line| !line.is_empty()) {
        let Some(patch) = patch_id(worktree, commit) else {
            return false;
        };
        pr_patches.insert(patch);
    }
    let Some(unpushed) = git_text(
        worktree,
        &["log", "--format=%H", "HEAD", "--not", "--remotes"],
    ) else {
        return false;
    };
    !unpushed.is_empty()
        && unpushed
            .lines()
            .filter(|line| !line.is_empty())
            .all(|commit| {
                patch_id(worktree, commit).is_some_and(|patch| pr_patches.contains(&patch))
            })
}

fn patch_id(worktree: &Path, commit: &str) -> Option<String> {
    let shown = command_output(Command::new("git").arg("-C").arg(worktree).args([
        "show",
        "--pretty=medium",
        "--no-ext-diff",
        commit,
    ]))
    .ok()?;
    if !shown.status.success() {
        return None;
    }
    let output = command_output_with(
        Command::new("git").args(["patch-id", "--stable"]),
        Some(&shown.stdout),
        Duration::from_secs(30),
    )
    .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned()
    })
}

fn content_in_default(worktree: &Path, project: &Path) -> bool {
    let Some(branch) = default_branch(project) else {
        return false;
    };
    let reference = if git_success(project, &["remote", "get-url", "origin"]) {
        if !git_success(
            worktree,
            &[
                "fetch",
                "--quiet",
                "origin",
                &format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"),
            ],
        ) {
            return false;
        }
        format!("refs/remotes/origin/{branch}")
    } else {
        format!("refs/heads/{branch}")
    };
    let Some(default_tree) = git_text(worktree, &["rev-parse", &format!("{reference}^{{tree}}")])
    else {
        return false;
    };
    let Some(merged_tree) = git_text(
        worktree,
        &["merge-tree", "--write-tree", &reference, "HEAD"],
    ) else {
        return false;
    };
    merged_tree.lines().next() == Some(default_tree.as_str())
}

/// Existing Git/forge landing evidence shared with the allocation owner.
pub(super) fn allocation_landed(worktree: &Path, project: &Path) -> bool {
    !publication_blocked(worktree, None)
        && (pr_landed(worktree, None) || content_in_default(worktree, project))
}

fn persistent_metadata(id: &str, values: &BTreeMap<String, String>) -> Result<bool, String> {
    let text = values
        .iter()
        .map(|(key, value)| format!("{key}={value}\n"))
        .collect::<String>();
    super::subagent_model::read_meta(id, &text).map(|record| record.persistent)
}

fn validate_worktree_safety(
    context: &Context,
    id: &str,
    values: &BTreeMap<String, String>,
    worktree: &Path,
    project: &Path,
) -> Result<(), String> {
    // Legacy scout scratch semantics are a bounded reader only. Canonical
    // report assignments retain any code or publication work like every sub-agent.
    if !values.contains_key("canonical_model")
        && values.get("kind").map(String::as_str) == Some("scout")
    {
        return Ok(());
    }
    let ready = context.state.join(format!("{id}.ready-to-push"));
    if ready.exists() || fs::symlink_metadata(&ready).is_ok() {
        return Err(format!(
            "REFUSED: worktree {} is queued for credentialed delivery.",
            worktree.display()
        ));
    }
    let dirty = git_status_after_stale_lock_cleanup(worktree)
        .map_err(|cause| {
            format!(
                "REFUSED: cannot inspect worktree {} for uncommitted changes: {cause}.",
                worktree.display()
            )
        })?
        .lines()
        .any(|line| !line.starts_with("?? .claude/"));
    let unpushed = git_text(
        worktree,
        &["log", "--oneline", "HEAD", "--not", "--remotes"],
    )
    .ok_or_else(|| {
        format!(
            "REFUSED: cannot inspect worktree {} for commits not on a remote.",
            worktree.display()
        )
    })?;
    if dirty {
        return Err(format!(
            "REFUSED: worktree {} has uncommitted changes present.",
            worktree.display()
        ));
    }
    if publication_blocked(worktree, values.get("pr").map(String::as_str)) {
        return Err(
            "REFUSED: open pull request or uncertain publication evidence; retained".into(),
        );
    }
    if unpushed.is_empty() {
        return Ok(());
    }
    if values.get("mode").map(String::as_str) == Some("local-only") {
        let branch = default_branch(project).ok_or_else(|| {
            format!(
                "REFUSED: cannot determine default branch for {}.",
                project.display()
            )
        })?;
        let unmerged = git_text(worktree, &["log", "--oneline", "HEAD", "--not", &branch])
            .ok_or_else(|| format!("REFUSED: cannot inspect commits not on {branch}."))?;
        if unmerged.is_empty() {
            return Ok(());
        }
        return Err(format!(
            "REFUSED: local-only worktree {} has work not yet merged into {branch} and not on any remote.",
            worktree.display()
        ));
    }
    if pr_landed(worktree, values.get("pr").map(String::as_str))
        || content_in_default(worktree, project)
    {
        return Ok(());
    }
    Err(format!(
        "REFUSED: worktree {} has work not on any remote and not landed.",
        worktree.display()
    ))
}

fn environment_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn environment_seconds(name: &str, fallback: Option<&str>, default: f64) -> f64 {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            fallback
                .and_then(|name| env::var(name).ok())
                .filter(|value| !value.is_empty())
        })
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

struct BoundedHolderProbe;
impl multplx_core::locks::HolderProbe for BoundedHolderProbe {
    fn holder_status(&self, path: &Path) -> multplx_core::locks::HolderStatus {
        use multplx_core::locks::HolderStatus;
        match command_output(Command::new("lsof").arg("--").arg(path)) {
            Ok(output) if output.status.success() => HolderStatus::Held,
            Ok(output)
                if output.status.code() == Some(1)
                    && output.stdout.is_empty()
                    && output.stderr.is_empty() =>
            {
                HolderStatus::Clear
            }
            _ => HolderStatus::Unknown,
        }
    }
}

fn stale_lock_proof(
    lock: &Path,
    minimum_age: f64,
    probe: &impl multplx_core::locks::HolderProbe,
) -> Result<bool, multplx_core::error::CoreError> {
    if env::var("MX_TEARDOWN_TEST_LOCK_MTIME_ERROR").as_deref() == Ok("1") {
        return Err(multplx_core::error::CoreError::Io {
            operation: "metadata",
            path: lock.to_owned(),
            source: std::io::Error::other("injected mtime read failure"),
        });
    }
    use multplx_core::locks::HolderStatus;
    for path in std::iter::once(lock).chain(lock.parent()) {
        match probe.holder_status(path) {
            HolderStatus::Held => return Ok(false),
            HolderStatus::Unknown => {
                return Err(multplx_core::error::CoreError::Command {
                    command: "lsof".into(),
                    reason: format!("lsof check failed for {}", path.display()),
                });
            }
            HolderStatus::Clear => {}
        }
    }
    multplx_core::locks::git_lock_is_provably_stale(
        lock,
        lock.parent(),
        Duration::from_secs_f64(minimum_age.max(0.0)),
        std::time::SystemTime::now(),
        probe,
    )
}

fn index_lock(worktree: &Path) -> Option<PathBuf> {
    let value = git_text(worktree, &["rev-parse", "--git-path", "index.lock"])?;
    let path = PathBuf::from(value);
    Some(if path.is_absolute() {
        path
    } else {
        worktree.join(path)
    })
}

pub(super) fn git_status_after_stale_lock_cleanup(worktree: &Path) -> Result<String, String> {
    let lock = index_lock(worktree).ok_or("cannot resolve Git index lock")?;
    let status = || {
        git_text(worktree, &["status", "--porcelain"])
            .ok_or_else(|| "Git status failed or timed out".to_owned())
    };
    if !lock.exists() {
        return status();
    }
    let retries = environment_usize("MX_WORKTREE_LOCK_RETRIES", 3).min(30);
    let wait = environment_seconds(
        "MX_WORKTREE_LOCK_RETRY_WAIT_SECS",
        Some("MX_STALE_WORKTREE_LOCK_RETRY_WAIT_SECS"),
        1.0,
    )
    .clamp(0.0, 2.0);
    eprintln!(
        "teardown: Git lock {}; waiting {wait}s and retrying ({retries} attempts, waiting {wait}s each)",
        lock.display()
    );
    for _ in 0..retries {
        std::thread::sleep(Duration::from_secs_f64(wait));
        if !lock.exists() {
            eprintln!("teardown: Git lock observation succeeded on retry");
            return status();
        }
    }
    eprintln!("teardown: Git lock persisted across {retries} retries");
    let minimum_age = environment_seconds("MX_STALE_WORKTREE_LOCK_AGE_SECS", None, 30.0);
    let proof = stale_lock_proof(&lock, minimum_age, &BoundedHolderProbe).map_err(|e| match e {
        multplx_core::error::CoreError::Io { .. } => format!(
            "cannot read mtime for git lock {}: {e}; retained",
            lock.display()
        ),
        _ => format!(
            "Git lock {} not provably stale: {e}; retained",
            lock.display()
        ),
    })?;
    if !proof {
        return Err(format!(
            "Git lock {} not provably stale: live/unknown holders or insufficient age; retained",
            lock.display()
        ));
    }
    fs::remove_file(&lock).map_err(|e| {
        format!(
            "cannot remove proven stale Git lock {}: {e}",
            lock.display()
        )
    })?;
    eprintln!(
        "teardown: removed provably-stale git lock {}",
        lock.display()
    );
    status()
}

fn return_allocation(
    id: &str,
    values: &BTreeMap<String, String>,
    worktree: &Path,
) -> Result<(), String> {
    let meta = values
        .iter()
        .map(|(key, value)| format!("{key}={value}\n"))
        .collect::<String>();
    let task = super::subagent_model::read_meta(id, &meta)?;
    let token = task
        .allocation
        .as_ref()
        .ok_or("legacy or unknown worktree ownership; retain for explicit migration")?;
    if Path::new(&token.path) != worktree {
        return Err("worktree path differs from allocation token".into());
    }
    let store =
        super::worktree::Store::new(task.project.as_ref().ok_or("allocation project missing")?)?;
    store.release(token)?;
    store.prune(token, true, None)?;
    Ok(())
}

fn validate_children(context: &Context, home: &Path) -> Result<Vec<PathBuf>, String> {
    let child_context = Context {
        root: context.root.clone(),
        home: home.to_owned(),
        data: home.join("data"),
        state: home.join("state"),
    };
    let state = home.join("state");
    if !state.exists() {
        return Ok(Vec::new());
    }
    let state = fs::canonicalize(&state).map_err(|error_value| error_value.to_string())?;
    if !is_strict_descendant(home, &state) {
        return Err(format!(
            "REFUSED: unsafe daemon home state directory {} resolves outside the daemon home",
            home.join("state").display()
        ));
    }
    let mut children = Vec::new();
    for entry in fs::read_dir(&state).map_err(|error_value| error_value.to_string())? {
        let entry = entry.map_err(|error_value| error_value.to_string())?;
        if entry.path().extension().is_none_or(|value| value != "meta") {
            continue;
        }
        let child_id = entry
            .path()
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("child task metadata name is not valid UTF-8")?
            .to_owned();
        TaskId::parse(&child_id).map_err(|_| "child task metadata has an invalid id")?;
        validate_pr_artifacts(&state, &child_id)?;
        let values = metadata(&entry.path())?;
        if persistent_metadata(&child_id, &values)? {
            let child_home = values
                .get("home")
                .filter(|value| !value.is_empty())
                .or_else(|| values.get("worktree"))
                .ok_or("child daemon metadata has no home")?;
            validate_removal_target(context, Path::new(child_home), "child daemon home")?;
            let child_home = validate_home(&child_context, &child_id, Path::new(child_home))?;
            let _ = validate_children(context, &child_home)?;
        } else if let Some(worktree) = values.get("worktree").filter(|value| !value.is_empty())
            && Path::new(worktree).exists()
        {
            let raw = values
                .iter()
                .map(|(key, value)| format!("{key}={value}\n"))
                .collect::<String>();
            if super::subagent_model::read_meta(&child_id, &raw)?
                .allocation
                .is_none()
            {
                // Legacy paths have no resource-owner proof to narrow these
                // exclusions: preserve the outer active-home boundary as well.
                validate_removal_target(context, Path::new(worktree), "child worktree")?;
            }
            let target =
                validate_task_target(&child_context, &child_id, &values, Path::new(worktree))?;
            let project = values
                .get("project")
                .ok_or("child metadata has no project")?;
            if !listed_worktree(Path::new(project), &target)? {
                return Err(format!(
                    "REFUSED: unsafe child worktree removal target {worktree} is not a git worktree for {project}"
                ));
            }
            let lock_output = command_output(Command::new("git").args([
                "-C".as_ref(),
                target.as_os_str(),
                "rev-parse".as_ref(),
                "--git-path".as_ref(),
                "index.lock".as_ref(),
            ]))
            .map_err(|error_value| error_value.to_string())?;
            if lock_output.status.success() {
                let lock_text = String::from_utf8(lock_output.stdout)
                    .map_err(|_| "git lock path is not valid UTF-8".to_owned())?;
                let lock = PathBuf::from(lock_text.trim());
                let lock = if lock.is_absolute() {
                    lock
                } else {
                    target.join(lock)
                };
                if lock.exists() {
                    return Err(format!(
                        "REFUSED: child git lock {} is not provably stale; leaving it in place",
                        lock.display()
                    ));
                }
            }
        }
        children.push(entry.path());
    }
    Ok(children)
}

fn remove_pr_artifacts(state: &Path, id: &str) -> Result<(), String> {
    validate_pr_artifacts(state, id)?;
    for suffix in [
        "check.sh",
        "pr-poll",
        "pr-poll-registration",
        "pr-poll-retirement",
        "check-trust",
    ] {
        let _ = fs::remove_file(state.join(format!("{id}.{suffix}")));
    }
    let quarantine = state.join(".pr-check-quarantine");
    if quarantine.is_dir() {
        for entry in fs::read_dir(&quarantine).map_err(|error_value| error_value.to_string())? {
            let entry = entry.map_err(|error_value| error_value.to_string())?;
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{id}."))
            {
                fs::remove_file(entry.path()).map_err(|error_value| error_value.to_string())?;
            }
        }
        let _ = fs::remove_dir(quarantine);
    }
    Ok(())
}

fn cleanup_children<F>(context: &Context, home: &Path, kill: &mut F) -> Result<(), String>
where
    F: FnMut(&Path) -> Result<(), String>,
{
    let child_context = Context {
        root: context.root.clone(),
        home: home.to_owned(),
        data: home.join("data"),
        state: home.join("state"),
    };
    let state = home.join("state");
    if !state.is_dir() {
        return Ok(());
    }
    let children = validate_children(context, home)?;
    for meta_path in children {
        let child_id = meta_path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("child id is not valid UTF-8")?
            .to_owned();
        let _child_lock = DirectoryLock::acquire_wait(
            state.join(format!(".teardown.{child_id}.lock")),
            &SystemProcessProbe::default(),
            Duration::from_secs(5),
        )
        .map_err(|e| format!("cannot acquire child teardown lock: {e}"))?;
        // Discovery above is read-only. Re-read the authoritative generation
        // after taking the same task guard as spawn, then hold it through the
        // endpoint stop and exact-allocation cleanup.
        let values = metadata(&meta_path)?;
        kill(&meta_path)?;
        if persistent_metadata(&child_id, &values)? {
            let child_home = values
                .get("home")
                .filter(|value| !value.is_empty())
                .or_else(|| values.get("worktree"))
                .ok_or("child daemon has no home")?;
            cleanup_children(&child_context, Path::new(child_home), kill)?;
            remove_home(&child_context, Path::new(child_home))?;
            remove_registry_entry(&child_context, &child_id)?;
        } else if let Some(worktree) = values.get("worktree").filter(|value| !value.is_empty())
            && Path::new(worktree).exists()
        {
            let project = values
                .get("project")
                .ok_or("child metadata has no project")?;
            validate_worktree_safety(
                &child_context,
                &child_id,
                &values,
                Path::new(worktree),
                Path::new(project),
            )?;
            return_allocation(&child_id, &values, Path::new(worktree))?;
        }
        remove_pr_artifacts(&state, &child_id)?;
        for suffix in ["status", "turn-ended", "meta", "pi-ext.ts", "journal"] {
            let _ = fs::remove_file(state.join(format!("{child_id}.{suffix}")));
        }
    }
    Ok(())
}

fn validate_home(context: &Context, id: &str, requested: &Path) -> Result<PathBuf, String> {
    let home = validate_removal_target(context, requested, "daemon home")?;
    require_owned_directory(&home, "daemon home")?;
    let marker = home.join(MARKER);
    if !marker.exists() {
        return Err(format!(
            "REFUSED: unsafe daemon home removal target {} is not a seeded daemon home",
            home.display()
        ));
    }
    let marker_text = String::from_utf8(read_regular(&marker, "daemon home marker")?)
        .map_err(|_| "daemon home marker is not valid UTF-8".to_owned())?;
    if marker_text.trim_end() != id {
        return Err(format!(
            "REFUSED: unsafe daemon home removal target {} is marked for daemon {}, expected {id}",
            home.display(),
            marker_text.trim_end()
        ));
    }
    for name in ["data", "state", "config", "projects"] {
        let path = home.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error_value) => return Err(error_value.to_string()),
        };
        if !metadata.is_dir() && !metadata.file_type().is_symlink() {
            return Err(format!(
                "REFUSED: unsafe daemon home {name} path {} is not a directory",
                path.display()
            ));
        }
        let canonical = fs::canonicalize(&path).map_err(|_| {
            format!(
                "REFUSED: unsafe daemon home {name} directory {} resolves outside the daemon home",
                path.display()
            )
        })?;
        if !is_strict_descendant(&home, &canonical) {
            return Err(format!(
                "REFUSED: unsafe daemon home {name} directory {} resolves outside the daemon home",
                path.display()
            ));
        }
    }
    validate_registry_descendants(&context.data.join("daemons.md"), &home)?;
    validate_registry_descendants(&home.join("data/daemons.md"), &home)?;
    Ok(home)
}

fn has_children(home: &Path) -> Result<Option<PathBuf>, String> {
    let state = home.join("state");
    if !state.exists() {
        return Ok(None);
    }
    let state = fs::canonicalize(&state).map_err(|error_value| error_value.to_string())?;
    if !is_strict_descendant(home, &state) {
        return Err(format!(
            "REFUSED: unsafe daemon home state directory {} resolves outside the daemon home",
            home.join("state").display()
        ));
    }
    for entry in fs::read_dir(state).map_err(|error_value| error_value.to_string())? {
        let entry = entry.map_err(|error_value| error_value.to_string())?;
        if entry
            .path()
            .extension()
            .is_some_and(|value| value == "meta")
        {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

fn linked_runtime_worktree(root: &Path, home: &Path) -> Result<bool, String> {
    let output = command_output(Command::new("git").args([
        "-C",
        path_text(root, "Multplx root")?.as_str(),
        "-c",
        "core.quotePath=false",
        "worktree",
        "list",
        "--porcelain",
    ]))
    .map_err(|error_value| error_value.to_string())?;
    if !output.status.success() {
        return Ok(false);
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| "git worktree inventory is not valid UTF-8".to_owned())?;
    Ok(text
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .any(|value| resolved(Path::new(value)) == home))
}

fn remove_home(context: &Context, home: &Path) -> Result<(), String> {
    if !home.exists() {
        return Ok(());
    }
    crate::project_registry::protect_borrowed_checkouts(&context.home, home)?;
    crate::project_registry::protect_borrowed_checkouts(home, home)?;
    let id = fs::read_to_string(home.join(MARKER)).map_err(|e| e.to_string())?;
    if let Some(allocation) = super::home_seed::read_home_allocation(&context.data, id.trim())? {
        let meta = fs::read_to_string(context.state.join(format!("{}.meta", id.trim())))
            .map_err(|e| e.to_string())?;
        let task = super::subagent_model::read_meta(id.trim(), &meta)?;
        let token = task
            .home_allocation
            .as_ref()
            .ok_or("home lease not bound to task metadata; retained")?;
        if token != &allocation.binding || token.path != home {
            return Err("stale or foreign home lease; retained".into());
        }
        let archive = super::home_seed::retire_home(&context.data, token)?;
        eprintln!("retained private home at {}", archive.display());
        return Ok(());
    }
    if linked_runtime_worktree(&context.root, home)? {
        return Err(
            "Git-backed home retained: explicit allocation retirement or legacy migration required"
                .into(),
        );
    }
    Err("home has no exact owned allocation; retained for explicit legacy migration".into())
}

fn remove_registry_entry(context: &Context, id: &str) -> Result<(), String> {
    // Shared with home-seed publication; never held across Git or process work.
    let _registry_lock = DirectoryLock::acquire_wait(
        context.data.join(".home-seed.lock"),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|e| format!("cannot acquire home registry publication lock: {e}"))?;
    let registry = context.data.join("daemons.md");
    let bytes = match read_regular(&registry, "daemon registry") {
        Ok(bytes) => bytes,
        Err(_) if !registry.exists() => return Ok(()),
        Err(error_value) => return Err(error_value),
    };
    let text =
        String::from_utf8(bytes).map_err(|_| "daemon registry is not valid UTF-8".to_owned())?;
    let mut retained = text
        .lines()
        .filter(|line| {
            !line
                .strip_prefix("- ")
                .is_some_and(|tail| tail.split_whitespace().next() == Some(id))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !retained.is_empty() {
        retained.push('\n');
    }
    atomic_replace(&registry, retained.as_bytes(), 0o600)
        .map_err(|error_value| error_value.to_string())
}

fn remove_state(context: &Context, id: &str) -> Result<(), String> {
    for suffix in ["status", "turn-ended", "meta", "pi-ext.ts", "journal"] {
        let path = context.state.join(format!("{id}.{suffix}"));
        match fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    || !metadata.is_file()
                    || metadata.nlink() != 1 =>
            {
                return Err(format!(
                    "REFUSED: unsafe task state artifact {}",
                    path.display()
                ));
            }
            Ok(_) => fs::remove_file(&path).map_err(|error_value| error_value.to_string())?,
            Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {}
            Err(error_value) => return Err(error_value.to_string()),
        }
    }
    Ok(())
}

fn remove_task_tmp(values: &BTreeMap<String, String>, id: &str) -> Result<(), String> {
    let Some(value) = values.get("tasktmp").filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let path = PathBuf::from(value);
    if fs::symlink_metadata(&path)
        .is_err_and(|error_value| error_value.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(());
    }
    let legacy = std::env::temp_dir().join(format!("mx-{id}"));
    let qualified = values
        .get("canonical_model")
        .and_then(|model| serde_json::from_str::<super::subagent_model::TaskRecord>(model).ok())
        .filter(|record| record.task_id == id)
        .and_then(|record| super::spawn::task_temp_path_for_record(&record).ok());
    if path != legacy && qualified.as_ref() != Some(&path) {
        return Err(format!(
            "REFUSED: unsafe task temporary directory {}",
            path.display()
        ));
    }
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(format!(
            "REFUSED: unsafe task temporary directory {}",
            path.display()
        )),
        Ok(_) => fs::remove_dir_all(path).map_err(|error_value| error_value.to_string()),
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error_value) => Err(error_value.to_string()),
    }
}

fn backlog_reminder(context: &Context, id: &str, values: &BTreeMap<String, String>) -> String {
    let kind = values.get("kind").map(String::as_str).unwrap_or("delivery");
    if kind == "daemon" {
        return String::new();
    }
    let config = env::var_os("MX_CONFIG_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| context.home.join("config"));
    if crate::backlog::backend_value(&config) == "manual" {
        return format!(
            "Backlog: {id} just finished. Update data/backlog.md - move {id} to Done, keep Done to the 10 most recent, then re-scan Queued and dispatch only work whose blockers are gone and date is due.\n"
        );
    }
    let done = if kind == "scout" {
        format!("bin/mx-backlog.sh done {id} --report data/{id}/report.md")
    } else if values.get("mode").map(String::as_str) == Some("local-only") {
        format!("bin/mx-backlog.sh done {id} --note \"local main\"")
    } else {
        format!(
            "bin/mx-backlog.sh done {id} --pr {}",
            values
                .get("pr")
                .filter(|value| !value.is_empty())
                .map(String::as_str)
                .unwrap_or("PR_URL")
        )
    };
    format!(
        "Backlog: {id} just finished. Run {done}, then run bin/mx-backlog.sh ready for dependency-cleared candidates, check date gates, and dispatch only work whose blockers are gone and date is due.\n"
    )
}

fn publish(path: &Path, journal: &Journal) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(journal).map_err(|error_value| error_value.to_string())?;
    bytes.push(b'\n');
    atomic_replace(path, &bytes, 0o600).map_err(|error_value| error_value.to_string())
}

fn publish_control(path: &Path, receipt: &ControlReceipt) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(receipt).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    atomic_replace(path, &bytes, 0o600).map_err(|error| error.to_string())
}

fn collect_stop_targets(
    context: &Context,
    home: &Path,
    targets: &mut Vec<PathBuf>,
    visited: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<(), String> {
    if targets.len() >= 1024 {
        return Err("REFUSED: coordinator subtree exceeds the bounded lifecycle limit".into());
    }
    let state = require_owned_directory(&home.join("state"), "coordinator child state")?;
    if !visited.insert(state.clone()) {
        return Err("REFUSED: coordinator subtree contains a home cycle".into());
    }
    let mut entries = fs::read_dir(&state)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("meta") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("child task metadata name is not valid UTF-8")?;
        TaskId::parse(id).map_err(|_| "child task metadata has an invalid id")?;
        let raw = String::from_utf8(read_regular(&path, "child task metadata")?)
            .map_err(|_| "child task metadata is not UTF-8")?;
        let record = super::subagent_model::read_meta(id, &raw)?;
        if !record.legacy_unknown
            && record.owner_state.as_deref().map(Path::new) != Some(state.as_path())
        {
            return Err("REFUSED: child task metadata is owned by another state directory".into());
        }
        if let Some(child_home) = record.persistent_home.as_deref() {
            let child_home = validate_home(context, id, Path::new(child_home))?;
            collect_stop_targets(context, &child_home, targets, visited)?;
        }
        targets.push(path);
    }
    Ok(())
}

fn coordinator_control<F>(
    context: &Context,
    id: &str,
    meta: &Path,
    record: &super::subagent_model::TaskRecord,
    operation: &str,
    kill: &mut F,
) -> Result<String, String>
where
    F: FnMut(&Path) -> Result<(), String>,
{
    use super::subagent_model::AssignmentRole;

    if record.role != AssignmentRole::SubOrchestrator {
        return Err(
            "REFUSED: lifecycle coordinator controls require a sub-orchestrator assignment".into(),
        );
    }
    let coordinator_attempt = record
        .attempt
        .as_ref()
        .ok_or("coordinator attempt identity is unavailable")?;
    let coordinator_endpoint = record.runtime.endpoint.clone();
    let execution_key = format!(
        "{:x}",
        Sha256::digest(
            coordinator_endpoint
                .as_deref()
                .unwrap_or("absent")
                .as_bytes()
        )
    );
    let receipt_path = context.state.join(format!(
        "{CONTROL_PREFIX}{id}.{operation}.{}.{}.json",
        coordinator_attempt.id,
        &execution_key[..16]
    ));
    let mut receipt = if receipt_path.exists() {
        let bytes = read_regular(&receipt_path, "coordinator control receipt")?;
        let receipt: ControlReceipt = serde_json::from_slice(&bytes)
            .map_err(|_| "malformed coordinator control receipt; retained".to_owned())?;
        if receipt.version != 1
            || receipt.id != id
            || receipt.operation != operation
            || receipt.coordinator_attempt != coordinator_attempt.id
            || receipt.coordinator_endpoint != coordinator_endpoint
        {
            return Err("coordinator control receipt identity mismatch; retained".into());
        }
        receipt
    } else {
        let mut targets = Vec::new();
        if operation == "stop-subtree" {
            let home = record
                .persistent_home
                .as_deref()
                .ok_or("REFUSED: coordinator has no private home for subtree control")?;
            let home = validate_home(context, id, Path::new(home))?;
            collect_stop_targets(
                context,
                &home,
                &mut targets,
                &mut std::collections::BTreeSet::new(),
            )?;
        }
        targets.push(meta.to_path_buf());
        let targets = targets
            .iter()
            .map(|target| control_target(target))
            .collect::<Result<Vec<_>, _>>()?;
        let receipt = ControlReceipt {
            version: 1,
            id: id.to_owned(),
            operation: operation.to_owned(),
            stage: "prepared".into(),
            coordinator_attempt: coordinator_attempt.id.clone(),
            coordinator_endpoint,
            targets,
            stopped: Vec::new(),
            reason: None,
        };
        publish_control(&receipt_path, &receipt)?;
        receipt
    };
    if receipt.stage == "committed" {
        return Ok(format!("coordinator {id} {operation} already complete\n"));
    }
    for target in receipt.targets.clone() {
        if receipt.stopped.contains(&target.path) {
            continue;
        }
        let path = PathBuf::from(&target.path);
        let current = control_target(&path)?;
        if current.task_id != target.task_id
            || current.attempt_id != target.attempt_id
            || current.generation != target.generation
            || current.endpoint != target.endpoint
        {
            receipt.stage = "uncertain".into();
            receipt.reason = Some("target execution changed after control intent".into());
            publish_control(&receipt_path, &receipt)?;
            return Err(
                "coordinator control target changed after intent; replacement was not stopped"
                    .into(),
            );
        }
        if let Err(error) = kill(&path) {
            receipt.stage = "uncertain".into();
            receipt.reason = Some(error.clone());
            publish_control(&receipt_path, &receipt)?;
            return Err(format!(
                "coordinator {operation} could not verify endpoint stop; state and outcomes retained: {error}"
            ));
        }
        let raw = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let mut stopped = super::subagent_model::read_meta(&target.task_id, &raw)?;
        stopped.runtime.endpoint = None;
        stopped.runtime.session_id = None;
        stopped.schedule.state = super::subagent_model::WorkState::WaitingExternal;
        stopped.schedule.waiting_condition = Some(format!(
            "endpoint stopped by coordinator lifecycle operation {operation}; explicit restart required"
        ));
        let raw_without_stopped_endpoint = raw
            .lines()
            .filter(|line| !line.starts_with("window=") && !line.starts_with("session_id="))
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        let updated = super::subagent_model::write_meta(&raw_without_stopped_endpoint, &stopped)?;
        atomic_replace(&path, updated.as_bytes(), 0o600).map_err(|error| error.to_string())?;
        receipt.stopped.push(target.path);
        receipt.stage = "stopping".into();
        receipt.reason = None;
        publish_control(&receipt_path, &receipt)?;
    }
    receipt.stage = "committed".into();
    publish_control(&receipt_path, &receipt)?;
    Ok(format!(
        "coordinator {id} {operation} complete; retained {} task record(s)\n",
        receipt.targets.len()
    ))
}

fn injected(point: &str) -> Result<(), String> {
    if env::var("MX_TEARDOWN_CRASH_AFTER").as_deref() == Ok(point) {
        std::process::exit(96);
    }
    if env::var("MX_TEARDOWN_FAIL_AFTER").as_deref() == Ok(point) {
        return Err(format!("injected teardown failure after {point}"));
    }
    Ok(())
}

fn finish_transaction<F>(
    context: &Context,
    path: &Path,
    journal: &mut Journal,
    kill: &mut F,
) -> Result<(), String>
where
    F: FnMut(&Path) -> Result<(), String>,
{
    let home = validate_removal_target(context, Path::new(&journal.home), "daemon home")?;
    if journal.stage == "prepared" {
        if let Ok(raw) = fs::read_to_string(context.state.join(format!("{}.meta", journal.id))) {
            let task = super::subagent_model::read_meta(&journal.id, &raw)?;
            if task.home_allocation != journal.home_allocation {
                return Err("stale home retirement journal; retained".into());
            }
        }
        if home.exists() {
            validate_home(context, &journal.id, &home)?;
        }
        kill(&context.state.join(format!("{}.meta", journal.id)))?;
        remove_home(context, &home)?;
        journal.stage = "home-removed".to_owned();
        publish(path, journal)?;
        injected("home")?;
    }
    if journal.stage == "home-removed" {
        remove_registry_entry(context, &journal.id)?;
        journal.stage = "registry-removed".to_owned();
        publish(path, journal)?;
        injected("registry")?;
    }
    if journal.stage == "registry-removed" {
        remove_state(context, &journal.id)?;
        journal.stage = "committed".to_owned();
        publish(path, journal)?;
        injected("state")?;
    }
    if journal.stage != "committed" {
        return Err("malformed daemon teardown journal stage".to_owned());
    }
    fs::remove_file(path).map_err(|error_value| error_value.to_string())
}

fn recover<F>(context: &Context, selected: Option<&str>, kill: &mut F) -> Result<(), String>
where
    F: FnMut(&Path) -> Result<(), String>,
{
    for entry in fs::read_dir(&context.state).map_err(|error_value| error_value.to_string())? {
        let entry = entry.map_err(|error_value| error_value.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "teardown transaction name is not valid UTF-8".to_owned())?;
        let Some(id) = name.strip_prefix(JOURNAL_PREFIX) else {
            continue;
        };
        if selected.is_some_and(|selected| selected != id) {
            continue;
        }
        crate::review_delivery::OperationalTaskId::parse(id.to_owned())
            .map_err(|_| "malformed teardown transaction id".to_owned())?;
        let path = entry.path();
        let bytes = read_regular(&path, "teardown recovery journal")?;
        let mut journal: Journal = serde_json::from_slice(&bytes)
            .map_err(|error_value| format!("malformed teardown recovery journal: {error_value}"))?;
        if journal.id != id {
            return Err("teardown recovery journal identity mismatch".to_owned());
        }
        finish_transaction(context, &path, &mut journal, kill)?;
    }
    Ok(())
}

/// Reconcile other idle task journals under their own reservations. A busy or
/// corrupt unrelated task never monopolizes the home's teardown path; its
/// journal remains durable and the requested task still receives full checks.
fn recover_other_tasks<F>(context: &Context, selected: &str, kill: &mut F) -> Result<(), String>
where
    F: FnMut(&Path) -> Result<(), String>,
{
    for entry in fs::read_dir(&context.state).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|name| name.strip_prefix(JOURNAL_PREFIX))
        else {
            continue;
        };
        if id == selected
            || crate::review_delivery::OperationalTaskId::parse(id.to_owned()).is_err()
        {
            continue;
        }
        let Ok(_lock) = DirectoryLock::acquire_wait(
            context.state.join(format!(".teardown.{id}.lock")),
            &SystemProcessProbe::default(),
            Duration::ZERO,
        ) else {
            continue;
        };
        if let Err(error) = recover(context, Some(id), kill) {
            eprintln!("teardown: retained recovery journal for {id}: {error}");
        }
    }
    Ok(())
}

fn execute<F>(args: &[OsString], context: &Context, mut kill: F) -> Result<String, String>
where
    F: FnMut(&Path) -> Result<(), String>,
{
    let (raw_id, operation) = match args {
        [raw_id] => (raw_id, "retire-home"),
        [raw_id, flag]
            if matches!(
                flag.to_str(),
                Some("--stop-coordinator" | "--checkpoint" | "--stop-subtree" | "--retire-home")
            ) =>
        {
            (
                raw_id,
                flag.to_str()
                    .expect("matched UTF-8")
                    .trim_start_matches("--"),
            )
        }
        _ => return Err(USAGE.trim_end().to_owned()),
    };
    let id = raw_id.to_str().ok_or("task id is not valid UTF-8")?;
    crate::review_delivery::OperationalTaskId::parse(id.to_owned())
        .map_err(|_| "invalid teardown request".to_owned())?;
    path_text(&context.root, "Multplx root")?;
    path_text(&context.home, "active Multplx home")?;
    path_text(&context.data, "active data")?;
    path_text(&context.state, "active state")?;
    let state = require_owned_directory(&context.state, "active state")?;
    recover_other_tasks(context, id, &mut kill)?;
    let _lock = DirectoryLock::acquire_wait(
        state.join(format!(".teardown.{id}.lock")),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error_value| format!("cannot acquire teardown lock: {error_value}"))?;
    recover(context, Some(id), &mut kill)?;
    let meta = context.state.join(format!("{id}.meta"));
    let values = metadata(&meta).map_err(|error_value| {
        if !meta.exists() {
            format!("no meta for task {id} at {}", meta.display())
        } else {
            error_value
        }
    })?;
    if matches!(
        operation,
        "stop-coordinator" | "checkpoint" | "stop-subtree"
    ) {
        let record = super::subagent_model::read_meta(
            id,
            &fs::read_to_string(&meta).map_err(|error| error.to_string())?,
        )?;
        return coordinator_control(context, id, &meta, &record, operation, &mut kill);
    }
    if values.get("kind").map(String::as_str).unwrap_or("delivery") != "daemon" {
        let mut endpoint_stopped = false;
        validate_pr_artifacts(&context.state, id)?;
        if let (Some(raw_worktree), Some(raw_project)) = (
            values.get("worktree").filter(|value| !value.is_empty()),
            values.get("project").filter(|value| !value.is_empty()),
        ) && Path::new(raw_worktree).exists()
        {
            let worktree = validate_task_target(context, id, &values, Path::new(raw_worktree))?;
            let worktree_argument = Path::new(raw_worktree);
            let project = require_owned_directory(Path::new(raw_project), "task project")?;
            validate_worktree_safety(context, id, &values, worktree_argument, &project)?;
            if !listed_worktree(&project, worktree_argument)? {
                return Err(format!(
                    "REFUSED: unsafe task worktree removal target {} is not a git worktree for {}",
                    worktree.display(),
                    project.display()
                ));
            }
            kill(&meta)?;
            endpoint_stopped = true;
            return_allocation(id, &values, worktree_argument)?;
        }
        if !endpoint_stopped {
            kill(&meta)?;
        }
        remove_pr_artifacts(&context.state, id)?;
        remove_task_tmp(&values, id)?;
        remove_state(context, id)?;
        return Ok(format!(
            "teardown {id} complete (window {}, worktree {})\n{}",
            values.get("window").map(String::as_str).unwrap_or_default(),
            values
                .get("worktree")
                .map(String::as_str)
                .unwrap_or_default(),
            backlog_reminder(context, id, &values)
        ));
    }
    require_owned_directory(&context.data, "active data")?;
    let raw_home = values
        .get("home")
        .filter(|value| !value.is_empty())
        .or_else(|| values.get("worktree"))
        .ok_or("daemon metadata has no home or worktree")?;
    let home = validate_home(context, id, Path::new(raw_home))?;
    let pending_outcomes = super::parent_channel::pending_outcomes(&home.join("state"))?;
    if pending_outcomes > 0 {
        return Err(format!(
            "REFUSED: coordinator {id} has {pending_outcomes} undelivered parent outcome(s); deliver or durably transfer them before retirement"
        ));
    }
    if let Some(child) = has_children(&home)? {
        return Err(format!(
            "REFUSED: daemon {id} still has in-flight work in {}. Found {}.",
            home.join("state").display(),
            child.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    let journal_path = context.state.join(format!("{JOURNAL_PREFIX}{id}"));
    let mut journal = Journal {
        id: id.to_owned(),
        home: path_text(&home, "daemon home")?,
        stage: "prepared".to_owned(),
        home_allocation: super::subagent_model::read_meta(
            id,
            &fs::read_to_string(&meta).map_err(|e| e.to_string())?,
        )?
        .home_allocation,
    };
    publish(&journal_path, &journal)?;
    finish_transaction(context, &journal_path, &mut journal, &mut kill)?;
    Ok(format!(
        "teardown {id} complete (window {}, worktree {})\n",
        values.get("window").map(String::as_str).unwrap_or_default(),
        values
            .get("worktree")
            .map(String::as_str)
            .unwrap_or_default()
    ))
}

pub fn run<F>(args: &[OsString], context: &Context, kill: F) -> Output
where
    F: FnMut(&Path) -> Result<(), String>,
{
    match execute(args, context, kill) {
        Ok(stdout) => Output {
            status: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) if message.starts_with("usage:") => Output {
            status: 2,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
        Err(message) if message.starts_with("REFUSED:") => Output {
            status: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
        Err(message) => error(1, message),
    }
}

pub fn run_override<F>(id: &str, context: &Context, mut kill: F) -> Output
where
    F: FnMut(&Path) -> Result<(), String>,
{
    let operation = (|| -> Result<String, String> {
        crate::review_delivery::OperationalTaskId::parse(id.to_owned())
            .map_err(|_| "invalid teardown request".to_owned())?;
        let state = require_owned_directory(&context.state, "active state")?;
        require_owned_directory(&context.data, "active data")?;
        recover_other_tasks(context, id, &mut kill)?;
        let _lock = DirectoryLock::acquire_wait(
            state.join(format!(".teardown.{id}.lock")),
            &SystemProcessProbe::default(),
            Duration::from_secs(5),
        )
        .map_err(|error_value| format!("cannot acquire teardown lock: {error_value}"))?;
        recover(context, Some(id), &mut kill)?;
        let meta = context.state.join(format!("{id}.meta"));
        let values = metadata_for_retirement(&meta, true)?;
        if values.get("kind").map(String::as_str).unwrap_or("delivery") != "daemon" {
            validate_pr_artifacts(&context.state, id)?;
            let raw_worktree = values
                .get("worktree")
                .filter(|value| !value.is_empty())
                .ok_or("task metadata has no worktree")?;
            let raw_project = values
                .get("project")
                .filter(|value| !value.is_empty())
                .ok_or("task metadata has no project")?;
            let retained_checkout =
                values.get("single_checkout").map(String::as_str) == Some("yes");
            if !retained_checkout {
                kill(&meta)?;
            }
            if retained_checkout {
                let worktree = require_owned_directory(Path::new(raw_worktree), "task worktree")?;
                let project = require_owned_directory(Path::new(raw_project), "task project")?;
                if worktree != project {
                    return Err(
                        "REFUSED: single-checkout metadata does not name the primary project"
                            .to_owned(),
                    );
                }
                let record = values
                    .get("single_checkout_record")
                    .filter(|value| !value.is_empty())
                    .ok_or("single-checkout metadata has no reservation")?;
                let record = PathBuf::from(record);
                if record.parent().and_then(|path| fs::canonicalize(path).ok())
                    != fs::canonicalize(&context.state).ok()
                {
                    return Err(
                        "REFUSED: single-checkout reservation is outside active state".to_owned(),
                    );
                }
                let bytes = read_regular(&record, "single-checkout reservation")?;
                let value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|_| "single-checkout reservation is invalid".to_owned())?;
                if value.get("task_id").and_then(serde_json::Value::as_str) != Some(id)
                    || value
                        .get("target_identity")
                        .and_then(serde_json::Value::as_str)
                        != Some(raw_project.as_str())
                {
                    return Err(
                        "REFUSED: single-checkout reservation ownership mismatch".to_owned()
                    );
                }
                kill(&meta)?;
                fs::remove_file(&record).map_err(|error_value| error_value.to_string())?;
            } else if Path::new(raw_worktree).exists() {
                let worktree = require_owned_directory(Path::new(raw_worktree), "task worktree")?;
                let project = require_owned_directory(Path::new(raw_project), "task project")?;
                let target = validate_task_target(context, id, &values, &worktree)?;
                if !listed_worktree(&project, &target)? {
                    return Err(format!(
                        "REFUSED: unsafe task worktree removal target {} is not a git worktree for {}",
                        target.display(),
                        project.display()
                    ));
                }
                kill(&meta)?;
                return_allocation(id, &values, &target)?;
            }
            remove_pr_artifacts(&context.state, id)?;
            remove_task_tmp(&values, id)?;
            remove_state(context, id)?;
            return Ok(format!(
                "teardown {id} complete (window {}, worktree {})\n",
                values.get("window").map(String::as_str).unwrap_or_default(),
                raw_worktree
            ));
        }
        let raw_home = values
            .get("home")
            .filter(|value| !value.is_empty())
            .or_else(|| values.get("worktree"))
            .ok_or("daemon metadata has no home")?;
        let home = validate_home(context, id, Path::new(raw_home))?;
        let pending_outcomes = super::parent_channel::pending_outcomes(&home.join("state"))?;
        if pending_outcomes > 0 {
            return Err(format!(
                "REFUSED: coordinator {id} has {pending_outcomes} undelivered parent outcome(s); override cannot discard them"
            ));
        }
        validate_pr_artifacts(&context.state, id)?;
        let _ = validate_children(context, &home)?;
        cleanup_children(context, &home, &mut kill)?;
        let journal_path = context.state.join(format!("{JOURNAL_PREFIX}{id}"));
        let mut journal = Journal {
            id: id.to_owned(),
            home: path_text(&home, "daemon home")?,
            stage: "prepared".to_owned(),
            home_allocation: super::subagent_model::read_meta(
                id,
                &fs::read_to_string(&meta).map_err(|e| e.to_string())?,
            )?
            .home_allocation,
        };
        publish(&journal_path, &journal)?;
        finish_transaction(context, &journal_path, &mut journal, &mut kill)?;
        Ok(format!(
            "teardown {id} complete (window {}, worktree {})\n",
            values.get("window").map(String::as_str).unwrap_or_default(),
            values
                .get("worktree")
                .map(String::as_str)
                .unwrap_or_default()
        ))
    })();
    match operation {
        Ok(stdout) => Output {
            status: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) if message.starts_with("REFUSED:") => Output {
            status: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
        Err(message) => error(1, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn context(temp: &Path) -> Context {
        Context {
            root: temp.join("root"),
            home: temp.join("active"),
            data: temp.join("active/data"),
            state: temp.join("active/state"),
        }
    }

    fn prepare_context(temp: &Path) -> Context {
        let canonical = fs::canonicalize(temp).expect("canonical tempdir");
        let context = context(&canonical);
        fs::create_dir_all(&context.root).expect("root");
        fs::create_dir_all(&context.data).expect("data");
        fs::create_dir_all(&context.state).expect("state");
        context
    }

    fn seed_home(path: &Path, id: &str) {
        fs::create_dir_all(path).expect("home");
        fs::write(path.join(MARKER), format!("{id}\n")).expect("marker");
    }

    fn bind_private_home(context: &Context, id: &str, home: &Path) {
        let token = super::super::home_seed::HomeBinding {
            id: id.into(),
            owner_home: fs::canonicalize(&context.home).unwrap(),
            path: home.to_owned(),
            lease_id: format!("home-test-{id}"),
            generation: 1,
        };
        fs::write(home.join("private-preserved"), "keep private state").unwrap();
        let metadata = fs::metadata(home).unwrap();
        let allocation = super::super::home_seed::HomeAllocation {
            version: 1,
            binding: token.clone(),
            runtime_root: context.root.clone(),
            state: "active".into(),
            retained_path: None,
            directory_identity: Some((metadata.dev(), metadata.ino())),
            git_allocation: None,
        };
        fs::write(
            context.data.join(format!(".home-allocation-{id}.json")),
            serde_json::to_vec(&allocation).unwrap(),
        )
        .unwrap();
        let path = context.state.join(format!("{id}.meta"));
        let raw = fs::read_to_string(&path).unwrap();
        let mut task = super::super::subagent_model::read_meta(id, &raw).unwrap();
        task.owner_home = Some(token.owner_home.to_string_lossy().into_owned());
        task.persistent_home = Some(token.path.to_string_lossy().into_owned());
        task.persistent = true;
        task.home_allocation = Some(token);
        fs::write(
            path,
            super::super::subagent_model::write_meta(&raw, &task).unwrap(),
        )
        .unwrap();
    }

    fn coordinator_record(
        context: &Context,
        id: &str,
        home: &Path,
        endpoint: &str,
    ) -> super::super::subagent_model::TaskRecord {
        use super::super::subagent_model::{ArtifactKind, AssignmentRole, TaskRecord, WorkState};

        seed_home(home, id);
        for name in ["data", "state", "config", "projects"] {
            fs::create_dir_all(home.join(name)).unwrap();
        }
        let owner = fs::canonicalize(&context.home).unwrap();
        let mut record = TaskRecord::new(
            id.into(),
            AssignmentRole::SubOrchestrator,
            ArtifactKind::Report,
            true,
            "root".into(),
            format!("root-home:{}", context.root.display()),
            owner.to_string_lossy().into_owned(),
        );
        record.private_home = true;
        record.persistent_home = Some(home.to_string_lossy().into_owned());
        record.runtime.provider = "fixture".into();
        record.runtime.endpoint = Some(endpoint.into());
        record.schedule.state = WorkState::Running;
        let raw = format!(
            "kind=daemon\nhome={}\nwindow={endpoint}\nbackend=fixture\n",
            home.display()
        );
        fs::write(
            context.state.join(format!("{id}.meta")),
            super::super::subagent_model::write_meta(&raw, &record).unwrap(),
        )
        .unwrap();
        bind_private_home(context, id, home);
        super::super::subagent_model::read_meta(
            id,
            &fs::read_to_string(context.state.join(format!("{id}.meta"))).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn independent_teardowns_do_not_hold_a_home_wide_external_operation_lock() {
        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        fs::write(context.state.join("a.meta"), "kind=delivery\n").unwrap();
        fs::write(context.state.join("b.meta"), "kind=delivery\n").unwrap();
        fs::write(
            context.state.join(format!("{JOURNAL_PREFIX}unrelated")),
            "broken",
        )
        .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let gate = barrier.clone();
        let other = context.clone();
        let worker = std::thread::spawn(move || {
            run(&["a".into()], &other, |_| {
                gate.wait();
                gate.wait();
                Ok(())
            })
        });
        barrier.wait();
        let result = run(&["b".into()], &context, |_| Ok(()));
        barrier.wait();
        assert_eq!(result.status, 0, "{}", result.stderr);
        assert_eq!(worker.join().unwrap().status, 0);
        assert!(
            context
                .state
                .join(format!("{JOURNAL_PREFIX}unrelated"))
                .exists()
        );
    }

    #[test]
    fn open_or_unknown_pr_state_blocks_content_landing_fallback() {
        for row in [
            "OPEN\tabc",
            "open\tabc",
            "UNKNOWN\tabc",
            "MERGED",
            "MERGED\t",
        ] {
            assert!(pr_state_blocks_cleanup(row), "{row}");
        }
        assert!(!pr_state_blocks_cleanup("MERGED\tabc"));
        assert!(!pr_state_blocks_cleanup("CLOSED\tabc"));
    }

    #[test]
    fn unowned_plain_homes_retain_private_content() {
        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = temp.path().join("legacy");
        seed_home(&home, "legacy");
        fs::write(home.join("important"), "private content").unwrap();
        assert!(
            remove_home(&context, &home)
                .unwrap_err()
                .contains("no exact owned allocation")
        );
        assert_eq!(
            fs::read_to_string(home.join("important")).unwrap(),
            "private content"
        );
    }

    #[test]
    fn path_and_regular_file_validation_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let regular = temp.path().join("regular");
        fs::write(&regular, b"key=value\nignored\n").expect("regular");
        assert_eq!(
            read_regular(&regular, "fixture").expect("read"),
            b"key=value\nignored\n"
        );
        assert_eq!(
            metadata(&regular)
                .expect("metadata")
                .get("key")
                .map(String::as_str),
            Some("value")
        );

        let linked = temp.path().join("linked");
        symlink(&regular, &linked).expect("symlink");
        assert!(
            read_regular(&linked, "fixture")
                .expect_err("symlink")
                .contains("cannot open")
        );

        let hardlink = temp.path().join("hardlink");
        fs::hard_link(&regular, &hardlink).expect("hardlink");
        assert!(
            read_regular(&regular, "fixture")
                .expect_err("multiple links")
                .contains("single-link")
        );

        let invalid = PathBuf::from(OsString::from_vec(vec![b'x', 0xff]));
        assert!(
            path_text(&invalid, "fixture")
                .expect_err("utf8")
                .contains("not valid UTF-8")
        );
    }

    #[test]
    fn owned_directory_and_removal_boundaries_reject_aliases_and_ancestors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        assert_eq!(
            require_owned_directory(&context.state, "state").expect("directory"),
            fs::canonicalize(&context.state).expect("canonical")
        );
        let file = temp.path().join("file");
        fs::write(&file, "x").expect("file");
        assert!(
            require_owned_directory(&file, "state")
                .expect_err("file")
                .contains("not a real directory")
        );
        let linked = temp.path().join("linked-state");
        symlink(&context.state, &linked).expect("symlink");
        assert!(
            require_owned_directory(&linked, "state")
                .expect_err("symlink")
                .contains("not a real directory")
        );

        for unsafe_target in [
            Path::new("/"),
            context.root.as_path(),
            context.home.as_path(),
            temp.path(),
            context.root.join("inside").as_path(),
            context.home.join("inside").as_path(),
        ] {
            assert!(
                validate_removal_target(&context, unsafe_target, "fixture").is_err(),
                "{}",
                unsafe_target.display()
            );
        }
        let safe = temp.path().join("separate-home");
        assert_eq!(
            validate_removal_target(&context, &safe, "fixture").expect("safe"),
            resolved(&safe)
        );
        assert!(is_strict_descendant(&context.home, &context.state));
        assert!(!is_strict_descendant(&context.home, &context.home));
    }

    #[test]
    fn registry_descendant_validation_parses_only_registered_homes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(temp.path()).expect("canonical tempdir");
        let home = base.join("daemon");
        let child = home.join("nested");
        let registry = temp.path().join("daemons.md");
        fs::write(
            &registry,
            format!(
                "heading\n- child - live (home: {}; harness: codex)\n",
                child.display()
            ),
        )
        .expect("registry");
        assert_eq!(
            registry_home("- child - live (home: /tmp/child; harness: codex)"),
            Some("/tmp/child")
        );
        assert_eq!(registry_home("missing"), None);
        assert!(
            validate_registry_descendants(&registry, &home)
                .expect_err("descendant")
                .contains("contains registered daemon home")
        );
        fs::write(
            &registry,
            "- other - live (home: /tmp/other; harness: pi)\n",
        )
        .expect("registry");
        assert!(validate_registry_descendants(&registry, &home).is_ok());
        assert!(validate_registry_descendants(&temp.path().join("absent"), &home).is_ok());
    }

    #[test]
    fn pr_artifacts_require_regular_files_and_secure_quarantine() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        for suffix in [
            "check.sh",
            "pr-poll",
            "pr-poll-registration",
            "pr-poll-retirement",
            "check-trust",
        ] {
            fs::write(state.join(format!("task.{suffix}")), suffix).expect("artifact");
        }
        let quarantine = state.join(".pr-check-quarantine");
        fs::create_dir(&quarantine).expect("quarantine");
        fs::set_permissions(&quarantine, fs::Permissions::from_mode(0o700)).expect("mode");
        let quarantined = quarantine.join("task.check.sh.legacy");
        fs::write(&quarantined, "legacy").expect("quarantined");
        fs::set_permissions(&quarantined, fs::Permissions::from_mode(0o600)).expect("mode");
        assert!(validate_pr_artifacts(&state, "task").is_ok());
        remove_pr_artifacts(&state, "task").expect("remove");
        assert!(!state.join("task.check.sh").exists());
        assert!(!quarantined.exists());

        fs::create_dir_all(&quarantine).expect("quarantine");
        fs::set_permissions(&quarantine, fs::Permissions::from_mode(0o755)).expect("mode");
        assert!(
            validate_pr_artifacts(&state, "task")
                .expect_err("mode")
                .contains("unsafe PR-check quarantine")
        );
        fs::remove_dir(&quarantine).expect("remove quarantine");
        symlink("missing", state.join("task.check.sh")).expect("symlink");
        assert!(
            validate_pr_artifacts(&state, "task")
                .expect_err("artifact")
                .contains("unsafe task PR-check artifact")
        );
    }

    #[test]
    fn noncanonical_quarantine_collision_is_a_refusal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        let quarantine = state.join(".pr-check-quarantine");
        fs::create_dir_all(&quarantine).expect("quarantine");
        fs::set_permissions(&quarantine, fs::Permissions::from_mode(0o700)).expect("mode");
        let entry = quarantine.join("_noncanonical.check.sh.legacy");
        fs::write(&entry, "legacy").expect("entry");
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o600)).expect("mode");
        assert!(
            validate_pr_artifacts(&state, "_noncanonical")
                .expect_err("collision")
                .contains("namespace collision")
        );
    }

    #[test]
    fn seeded_home_validation_checks_marker_identity_and_containment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let home = temp.path().join("daemon-home");
        seed_home(&home, "daemon");
        fs::create_dir(home.join("state")).expect("state");
        assert_eq!(
            validate_home(&context, "daemon", &home).expect("valid"),
            resolved(&home)
        );
        assert!(
            validate_home(&context, "other", &home)
                .expect_err("marker")
                .contains("marked for daemon daemon")
        );
        fs::write(home.join(MARKER), "daemon\n").expect("marker");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).expect("outside");
        fs::remove_dir(home.join("state")).expect("state");
        symlink(&outside, home.join("state")).expect("symlink");
        assert!(
            validate_home(&context, "daemon", &home)
                .expect_err("outside")
                .contains("resolves outside")
        );

        let unseeded = temp.path().join("unseeded");
        fs::create_dir(&unseeded).expect("unseeded");
        assert!(
            validate_home(&context, "daemon", &unseeded)
                .expect_err("unseeded")
                .contains("not a seeded daemon home")
        );
    }

    #[test]
    fn child_and_state_cleanup_are_bounded_to_expected_artifacts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let home = fs::canonicalize(temp.path())
            .expect("canonical tempdir")
            .join("daemon-home");
        seed_home(&home, "daemon");
        assert!(has_children(&home).expect("no state").is_none());
        fs::create_dir(home.join("state")).expect("state");
        fs::write(home.join("state/notes.txt"), "keep").expect("notes");
        assert!(has_children(&home).expect("no children").is_none());
        fs::write(home.join("state/child.meta"), "kind=delivery\n").expect("child");
        assert!(has_children(&home).expect("child").is_some());

        for suffix in ["status", "turn-ended", "meta", "pi-ext.ts", "journal"] {
            fs::write(context.state.join(format!("task.{suffix}")), suffix)
                .expect("state artifact");
        }
        fs::write(context.state.join("unrelated"), "keep").expect("unrelated");
        remove_state(&context, "task").expect("remove state");
        assert!(context.state.join("unrelated").exists());
        assert!(!context.state.join("task.meta").exists());
        symlink("unrelated", context.state.join("task.meta")).expect("symlink");
        assert!(
            remove_state(&context, "task")
                .expect_err("unsafe")
                .contains("unsafe task state artifact")
        );
    }

    #[test]
    fn cleanup_children_recurses_through_daemons_and_removes_only_owned_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let base = fs::canonicalize(temp.path()).expect("canonical tempdir");
        let parent = base.join("parent-home");
        let child = base.join("child-home");
        seed_home(&parent, "parent");
        seed_home(&child, "child-daemon");
        fs::create_dir(parent.join("state")).expect("parent state");
        fs::create_dir(child.join("state")).expect("child state");
        fs::write(
            parent.join("state/child-daemon.meta"),
            format!("kind=daemon\nhome={}\n", child.display()),
        )
        .expect("daemon meta");
        fs::write(
            parent.join("state/child-task.meta"),
            "kind=delivery\nworktree=/does/not/exist\n",
        )
        .expect("task meta");
        fs::write(parent.join("state/child-task.status"), "done\n").expect("status");
        fs::write(parent.join("state/unrelated.txt"), "keep\n").expect("unrelated");
        fs::create_dir(parent.join("data")).unwrap();
        bind_private_home(
            &Context {
                root: context.root.clone(),
                home: parent.clone(),
                data: parent.join("data"),
                state: parent.join("state"),
            },
            "child-daemon",
            &child,
        );
        let mut killed = Vec::new();
        cleanup_children(&context, &parent, &mut |path| {
            killed.push(path.file_stem().unwrap().to_string_lossy().into_owned());
            Ok(())
        })
        .expect("cleanup");
        killed.sort();
        assert_eq!(killed, vec!["child-daemon", "child-task"]);
        assert!(!child.exists());
        assert!(!parent.join("state/child-daemon.meta").exists());
        assert!(!parent.join("state/child-task.status").exists());
        assert!(parent.join("state/unrelated.txt").exists());
    }

    #[test]
    fn recursive_cleanup_retains_dirty_current_allocation() {
        use super::super::subagent_model::{ArtifactKind, AssignmentRole, TaskRecord, write_meta};
        use super::super::worktree::{Acquire, Store};
        use crate::project_registry::{CheckoutOwnership, register_project};

        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let parent = fs::canonicalize(temp.path()).unwrap().join("parent-home");
        seed_home(&parent, "parent");
        for name in ["data", "state", "projects"] {
            fs::create_dir_all(parent.join(name)).unwrap();
        }
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&source)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main", "--quiet"]);
        fs::write(source.join("file"), "base").unwrap();
        git(&["add", "file"]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "base",
            "--quiet",
        ]);
        let project =
            register_project(&parent, &source, None, CheckoutOwnership::UserOwned).unwrap();
        let mut task = TaskRecord::new(
            "child".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            format!("root-home:{}", context.home.display()),
            format!("root-home:{}", context.home.display()),
            parent.to_string_lossy().into_owned(),
        );
        task.owner_state = Some(parent.join("state").to_string_lossy().into_owned());
        task.parent_state = Some(context.state.to_string_lossy().into_owned());
        task.parent_home = Some(context.home.to_string_lossy().into_owned());
        task.project = Some(project.clone());
        let allocation = Store::new(&project)
            .unwrap()
            .acquire(
                &Acquire {
                    request_id: "child-request",
                    owner_home: &parent,
                    project: &project,
                    task_id: "child",
                    attempt_id: &task.attempt.as_ref().unwrap().id,
                    persistent: false,
                },
                None,
            )
            .unwrap();
        task.allocation = Some(allocation.binding.clone());
        let worktree = PathBuf::from(&allocation.binding.path);
        fs::write(worktree.join("dirty"), "uncommitted").unwrap();
        let raw = format!(
            "kind=delivery\nworktree={}\nproject={}\n",
            worktree.display(),
            project.canonical_path.display()
        );
        fs::write(
            parent.join("state/child.meta"),
            write_meta(&raw, &task).unwrap(),
        )
        .unwrap();

        let error = cleanup_children(&context, &parent, &mut |_| Ok(())).unwrap_err();
        assert!(error.contains("uncommitted changes"), "{error}");
        assert!(worktree.is_dir());
        assert!(parent.join("state/child.meta").is_file());
        assert_eq!(
            Store::new(&project)
                .unwrap()
                .inspect(&allocation.binding.allocation_id)
                .unwrap()
                .state,
            super::super::worktree::State::Active
        );
    }

    #[test]
    fn legacy_nested_child_retirement_preserves_outer_active_home_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let child_home = fs::canonicalize(temp.path()).unwrap().join("child-home");
        fs::create_dir_all(child_home.join("state")).unwrap();
        fs::write(
            child_home.join("state/child.meta"),
            format!(
                "kind=delivery\nworktree={}\nproject={}\n",
                context.data.display(),
                context.root.display()
            ),
        )
        .unwrap();
        assert!(
            validate_children(&context, &child_home)
                .unwrap_err()
                .contains("inside the active Multplx home")
        );
        assert!(context.data.is_dir());
    }

    #[test]
    fn child_validation_rejects_missing_daemon_home_and_invalid_task_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let home = fs::canonicalize(temp.path())
            .expect("canonical tempdir")
            .join("parent-home");
        seed_home(&home, "parent");
        fs::create_dir(home.join("state")).expect("state");
        fs::write(home.join("state/child.meta"), "kind=daemon\n").expect("meta");
        assert!(
            validate_children(&context, &home)
                .expect_err("home")
                .contains("has no home")
        );
        fs::remove_file(home.join("state/child.meta")).expect("remove");
        fs::write(home.join("state/.bad.meta"), "kind=delivery\n").expect("meta");
        assert!(
            validate_children(&context, &home)
                .expect_err("id")
                .contains("invalid id")
        );
    }

    #[test]
    fn task_tmp_cleanup_requires_exact_task_directory_name() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut values = BTreeMap::new();
        assert!(remove_task_tmp(&values, "task").is_ok());
        let unique = format!(
            "task-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let safe = std::env::temp_dir().join(format!("mx-{unique}"));
        fs::create_dir(&safe).expect("safe");
        values.insert("tasktmp".into(), safe.display().to_string());
        remove_task_tmp(&values, &unique).expect("remove");
        assert!(!safe.exists());

        let owner = fs::canonicalize(temp.path()).expect("owner");
        let record = super::super::subagent_model::TaskRecord::new(
            "task".into(),
            super::super::subagent_model::AssignmentRole::Implementer,
            super::super::subagent_model::ArtifactKind::Implementation,
            false,
            "parent".into(),
            format!("root-home:{}", owner.display()),
            owner.to_string_lossy().into_owned(),
        );
        let qualified =
            super::super::spawn::task_temp_path_for_record(&record).expect("qualified task temp");
        fs::create_dir(&qualified).expect("qualified temp");
        values.insert(
            "canonical_model".into(),
            serde_json::to_string(&record).unwrap(),
        );
        values.insert("tasktmp".into(), qualified.display().to_string());
        remove_task_tmp(&values, "task").expect("remove qualified");
        assert!(!qualified.exists());

        let unsafe_path = temp.path().join("wrong");
        fs::create_dir(&unsafe_path).expect("unsafe");
        values.insert("tasktmp".into(), unsafe_path.display().to_string());
        assert!(
            remove_task_tmp(&values, "task")
                .expect_err("wrong name")
                .contains("unsafe task temporary")
        );
        values.insert(
            "tasktmp".into(),
            temp.path().join("missing/mx-task").display().to_string(),
        );
        assert!(remove_task_tmp(&values, "task").is_ok());
    }

    #[test]
    fn backlog_reminders_cover_daemon_scout_local_and_pull_request_modes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        fs::create_dir_all(context.home.join("config")).expect("config");
        let mut values = BTreeMap::new();
        values.insert("kind".into(), "daemon".into());
        assert!(backlog_reminder(&context, "task", &values).is_empty());
        values.insert("kind".into(), "scout".into());
        assert!(
            backlog_reminder(&context, "task", &values).contains("--report data/task/report.md")
        );
        values.insert("kind".into(), "delivery".into());
        values.insert("mode".into(), "local-only".into());
        assert!(backlog_reminder(&context, "task", &values).contains("--note \"local main\""));
        values.remove("mode");
        values.insert("pr".into(), "https://example.test/pull/4".into());
        assert!(
            backlog_reminder(&context, "task", &values)
                .contains("--pr https://example.test/pull/4")
        );
        values.remove("pr");
        assert!(backlog_reminder(&context, "task", &values).contains("--pr PR_URL"));
    }

    #[test]
    fn journal_publish_finish_and_recovery_preserve_transaction_stages() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let journal_path = context.state.join(format!("{JOURNAL_PREFIX}daemon"));
        fs::write(context.state.join("daemon.meta"), "kind=daemon\n").expect("meta");
        let mut journal = Journal {
            id: "daemon".into(),
            home: temp.path().join("removed-home").display().to_string(),
            stage: "prepared".into(),
            home_allocation: None,
        };
        publish(&journal_path, &journal).expect("publish");
        let mut killed = Vec::new();
        finish_transaction(&context, &journal_path, &mut journal, &mut |path| {
            killed.push(path.to_owned());
            Ok(())
        })
        .expect("finish");
        assert_eq!(journal.stage, "committed");
        assert!(!journal_path.exists());
        assert_eq!(killed, vec![context.state.join("daemon.meta")]);

        let recovery = context.state.join(format!("{JOURNAL_PREFIX}recovery"));
        fs::write(context.state.join("recovery.meta"), "kind=daemon\n").expect("meta");
        publish(
            &recovery,
            &Journal {
                id: "recovery".into(),
                home: temp.path().join("gone").display().to_string(),
                stage: "home-removed".into(),
                home_allocation: None,
            },
        )
        .expect("publish");
        recover(&context, None, &mut |_| Ok(())).expect("recover");
        assert!(!recovery.exists());

        let malformed = context.state.join(format!("{JOURNAL_PREFIX}bad"));
        publish(
            &malformed,
            &Journal {
                id: "bad".into(),
                home: temp.path().join("gone").display().to_string(),
                stage: "unexpected".into(),
                home_allocation: None,
            },
        )
        .expect("publish");
        assert!(
            recover(&context, None, &mut |_| Ok(()))
                .expect_err("stage")
                .contains("malformed daemon teardown journal stage")
        );
    }

    #[test]
    fn public_run_maps_usage_invalid_and_missing_metadata_errors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let usage = run(&[], &context, |_| Ok(()));
        assert_eq!(usage.status, 2);
        assert_eq!(usage.stderr, USAGE);
        let invalid = run(&[OsString::from("../bad")], &context, |_| Ok(()));
        assert_eq!(invalid.status, 1);
        assert!(invalid.stderr.contains("invalid teardown request"));
        let missing = run(&[OsString::from("missing")], &context, |_| Ok(()));
        assert_eq!(missing.status, 1);
        assert!(missing.stderr.contains("no meta for task missing"));

        fs::write(context.state.join("ordinary.meta"), "kind=delivery\n").expect("meta");
        let kill_failed = run(&[OsString::from("ordinary")], &context, |_| {
            Err("cannot stop endpoint".to_owned())
        });
        assert_eq!(kill_failed.status, 1);
        assert!(kill_failed.stderr.contains("cannot stop endpoint"));

        fs::write(context.state.join("daemon.meta"), "kind=daemon\n").expect("meta");
        let missing_home = run(&[OsString::from("daemon")], &context, |_| Ok(()));
        assert_eq!(missing_home.status, 1);
        assert!(missing_home.stderr.contains("no home or worktree"));
    }

    #[test]
    fn ordinary_task_without_worktree_tears_down_state_and_reports_backlog() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let meta = context.state.join("task.meta");
        fs::write(&meta, "kind=delivery\nwindow=mx-task\nworktree=/does/not/exist\npr=https://example.test/pull/7\n").expect("meta");
        fs::write(context.state.join("task.status"), "done\n").expect("status");
        let mut killed = false;
        let result = run(&[OsString::from("task")], &context, |path| {
            assert_eq!(path, meta);
            killed = true;
            Ok(())
        });
        assert_eq!(result.status, 0, "{}", result.stderr);
        assert!(result.stdout.contains("teardown task complete"));
        assert!(result.stdout.contains("--pr https://example.test/pull/7"));
        assert!(killed);
        assert!(!meta.exists());
        assert!(!context.state.join("task.status").exists());
    }

    #[test]
    fn override_teardown_removes_an_ordinary_task_without_a_live_worktree() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let meta = context.state.join("task.meta");
        fs::write(
            &meta,
            "kind=delivery\nwindow=mx-task\nworktree=/does/not/exist\nproject=/does/not/exist\n",
        )
        .expect("meta");
        fs::write(context.state.join("task.check.sh"), "check").expect("check");
        let mut killed = false;
        let result = run_override("task", &context, |path| {
            assert_eq!(path, meta);
            killed = true;
            Ok(())
        });
        assert_eq!(result.status, 0, "{}", result.stderr);
        assert!(result.stdout.contains("teardown task complete"));
        assert!(killed);
        assert!(!meta.exists());
        assert!(!context.state.join("task.check.sh").exists());
    }

    #[test]
    fn override_single_checkout_requires_exact_reservation_ownership() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let project = temp.path().join("project");
        fs::create_dir(&project).expect("project");
        let record = context.state.join("reservation.json");
        fs::write(
            &record,
            serde_json::json!({
                "task_id": "task",
                "target_identity": project.display().to_string()
            })
            .to_string(),
        )
        .expect("reservation");
        fs::write(
            context.state.join("task.meta"),
            format!(
                "kind=delivery\nworktree={}\nproject={}\nsingle_checkout=yes\nsingle_checkout_record={}\n",
                project.display(),
                project.display(),
                record.display()
            ),
        )
        .expect("meta");
        let result = run_override("task", &context, |_| Ok(()));
        assert_eq!(result.status, 0, "{}", result.stderr);
        assert!(!record.exists());

        let outside = temp.path().join("outside.json");
        fs::write(&outside, "{}").expect("outside");
        fs::write(
            context.state.join("bad.meta"),
            format!(
                "kind=delivery\nworktree={}\nproject={}\nsingle_checkout=yes\nsingle_checkout_record={}\n",
                project.display(),
                project.display(),
                outside.display()
            ),
        )
        .expect("meta");
        let result = run_override("bad", &context, |_| Ok(()));
        assert_eq!(result.status, 1);
        assert!(
            result
                .stderr
                .contains("reservation is outside active state")
        );
    }

    #[test]
    fn daemon_override_executes_the_full_journaled_transaction() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let home = fs::canonicalize(temp.path())
            .expect("canonical tempdir")
            .join("daemon-home");
        seed_home(&home, "daemon");
        for directory in ["data", "state", "config", "projects"] {
            fs::create_dir(home.join(directory)).expect("daemon directory");
        }
        fs::write(
            context.state.join("daemon.meta"),
            format!("kind=daemon\nhome={}\nwindow=mx-daemon\n", home.display()),
        )
        .expect("meta");
        fs::write(
            context.data.join("daemons.md"),
            format!(
                "# Daemons\n- daemon - live (home: {}; harness: codex)\n",
                home.display()
            ),
        )
        .expect("registry");
        bind_private_home(&context, "daemon", &home);
        let mut killed = false;
        let result = run_override("daemon", &context, |_| {
            killed = true;
            Ok(())
        });
        assert_eq!(result.status, 0, "{}", result.stderr);
        assert!(killed);
        assert!(!home.exists());
        let receipt = super::super::home_seed::read_home_allocation(&context.data, "daemon")
            .unwrap()
            .unwrap();
        assert_eq!(receipt.state, "retired");
        assert_eq!(
            fs::read_to_string(receipt.retained_path.unwrap().join("private-preserved")).unwrap(),
            "keep private state"
        );
        assert!(!context.state.join("daemon.meta").exists());
        assert!(
            !fs::read_to_string(context.data.join("daemons.md"))
                .expect("registry")
                .contains("- daemon ")
        );
    }

    #[test]
    fn normal_daemon_teardown_commits_and_child_presence_refuses() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let base = fs::canonicalize(temp.path()).expect("canonical tempdir");
        let home = base.join("daemon-home");
        seed_home(&home, "daemon");
        fs::create_dir(home.join("state")).expect("state");
        fs::write(
            context.state.join("daemon.meta"),
            format!("kind=daemon\nhome={}\nwindow=mx-daemon\n", home.display()),
        )
        .expect("meta");
        bind_private_home(&context, "daemon", &home);
        let result = run(&[OsString::from("daemon")], &context, |_| Ok(()));
        assert_eq!(result.status, 0, "{}", result.stderr);
        assert!(!home.exists());

        let busy = base.join("busy-home");
        seed_home(&busy, "busy");
        fs::create_dir(busy.join("state")).expect("state");
        fs::write(busy.join("state/child.meta"), "kind=delivery\n").expect("child");
        fs::write(
            context.state.join("busy.meta"),
            format!("kind=daemon\nhome={}\n", busy.display()),
        )
        .expect("meta");
        let result = run(&[OsString::from("busy")], &context, |_| Ok(()));
        assert_eq!(result.status, 1);
        assert!(result.stderr.contains("still has in-flight work"));
        assert!(busy.exists());
    }

    #[test]
    fn override_error_matrix_maps_invalid_missing_and_single_checkout_mismatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        assert_eq!(run_override("../bad", &context, |_| Ok(())).status, 1);
        assert_eq!(run_override("missing", &context, |_| Ok(())).status, 1);

        let project = temp.path().join("project");
        let other = temp.path().join("other");
        fs::create_dir(&project).expect("project");
        fs::create_dir(&other).expect("other");
        fs::write(
            context.state.join("mismatch.meta"),
            format!(
                "kind=delivery\nworktree={}\nproject={}\nsingle_checkout=yes\nsingle_checkout_record={}\n",
                project.display(),
                other.display(),
                context.state.join("reservation.json").display()
            ),
        )
        .expect("meta");
        let result = run_override("mismatch", &context, |_| Ok(()));
        assert_eq!(result.status, 1);
        assert!(result.stderr.contains("does not name the primary project"));
    }

    #[test]
    fn recovery_rejects_malformed_payloads_and_identity_mismatches() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let malformed = context.state.join(format!("{JOURNAL_PREFIX}bad"));
        fs::write(&malformed, "not-json\n").expect("malformed");
        assert!(
            recover(&context, None, &mut |_| Ok(()))
                .expect_err("json")
                .contains("malformed teardown recovery journal")
        );
        fs::remove_file(&malformed).expect("remove");
        publish(
            &malformed,
            &Journal {
                id: "other".into(),
                home: temp.path().join("gone").display().to_string(),
                stage: "committed".into(),
                home_allocation: None,
            },
        )
        .expect("publish");
        assert!(
            recover(&context, None, &mut |_| Ok(()))
                .expect_err("identity")
                .contains("identity mismatch")
        );
    }

    #[test]
    fn removal_helpers_are_idempotent_for_absent_state_and_home() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        assert!(remove_home(&context, &temp.path().join("absent-home")).is_ok());
        assert!(remove_registry_entry(&context, "missing").is_ok());
        assert!(remove_state(&context, "missing").is_ok());
        fs::write(
            context.data.join("daemons.md"),
            "# Daemons\n- keep - live\n- remove - live\n",
        )
        .expect("registry");
        remove_registry_entry(&context, "remove").expect("remove registry row");
        let registry = fs::read_to_string(context.data.join("daemons.md")).expect("registry");
        assert!(registry.contains("- keep - live"));
        assert!(!registry.contains("- remove - live"));
    }

    #[test]
    fn git_and_worktree_safety_helpers_cover_local_contracts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).expect("repo");
        let git = |arguments: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(arguments)
                    .status()
                    .expect("git")
                    .success()
            );
        };
        git(&["init", "-q", "-b", "main"]);
        fs::write(repo.join("tracked"), "base\n").expect("tracked");
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "-m",
            "base",
        ]);
        assert!(listed_worktree(&repo, &repo).expect("listed worktree"));
        assert_eq!(default_branch(&repo).as_deref(), Some("main"));
        let head = git_text(&repo, &["rev-parse", "HEAD"]).expect("head");
        assert!(!patch_id(&repo, &head).expect("patch id").is_empty());
        assert!(patch_id(&repo, "missing-object").is_none());
        assert!(git_text(&repo, &["not-a-git-command"]).is_none());
        assert!(!git_success(&repo, &["not-a-git-command"]));
        assert!(index_lock(&repo).is_some());
        assert_eq!(
            git_status_after_stale_lock_cleanup(&repo).as_deref(),
            Ok("")
        );
        assert_eq!(
            pr_number("https://example.test/o/r/pull/123/files").as_deref(),
            Some("123")
        );
        assert_eq!(pr_number("456").as_deref(), Some("456"));
        assert!(pr_number("not-a-pr").is_none());
        assert!(content_in_default(&repo, &repo));

        let mut values = BTreeMap::new();
        values.insert("kind".into(), "scout".into());
        assert!(validate_worktree_safety(&context, "task", &values, &repo, &repo).is_ok());
        values.insert("kind".into(), "delivery".into());
        fs::write(context.state.join("task.ready-to-push"), "queued\n").expect("ready");
        assert!(
            validate_worktree_safety(&context, "task", &values, &repo, &repo)
                .expect_err("queued")
                .contains("queued for credentialed delivery")
        );

        fs::remove_file(context.state.join("task.ready-to-push")).expect("remove ready");
        values.insert("mode".into(), "local-only".into());
        assert!(validate_worktree_safety(&context, "task", &values, &repo, &repo).is_ok());
        fs::write(repo.join("tracked"), "dirty\n").expect("dirty");
        assert!(
            validate_worktree_safety(&context, "task", &values, &repo, &repo)
                .expect_err("dirty")
                .contains("uncommitted changes")
        );
        git(&["checkout", "--", "tracked"]);
        git(&["checkout", "-q", "-b", "feature"]);
        fs::write(repo.join("tracked"), "feature\n").expect("feature");
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "-m",
            "feature",
        ]);
        assert!(
            validate_worktree_safety(&context, "task", &values, &repo, &repo)
                .expect_err("unmerged")
                .contains("not yet merged into main")
        );

        let not_repo = temp.path().join("not-repo");
        fs::create_dir(&not_repo).expect("not repo");
        assert!(!listed_worktree(&not_repo, &repo).expect("not listed"));
        assert!(default_branch(&not_repo).is_none());
        assert!(!content_in_default(&not_repo, &not_repo));
        assert!(index_lock(&not_repo).is_none());
        assert!(git_status_after_stale_lock_cleanup(&not_repo).is_err());
    }

    #[test]
    fn bounded_file_and_state_validation_rejects_oversize_and_aliases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let large = temp.path().join("large");
        let file = fs::File::create(&large).expect("large");
        file.set_len(16 * 1024 * 1024 + 1).expect("length");
        assert!(
            read_regular(&large, "fixture")
                .expect_err("large")
                .contains("unexpectedly large")
        );
        let state_file = temp.path().join("state-file");
        fs::write(&state_file, "x").expect("state");
        assert!(
            validate_pr_artifacts(&state_file, "task")
                .expect_err("state")
                .contains("unsafe task state directory")
        );
        let real_state = temp.path().join("real-state");
        fs::create_dir(&real_state).expect("state");
        let state_link = temp.path().join("state-link");
        symlink(&real_state, &state_link).expect("symlink");
        assert!(
            validate_pr_artifacts(&state_link, "task")
                .expect_err("state")
                .contains("unsafe task state directory")
        );
        fs::write(real_state.join("task.check.sh"), "check").expect("check");
        fs::hard_link(
            real_state.join("task.check.sh"),
            real_state.join("task.check-copy"),
        )
        .expect("hardlink");
        assert!(
            validate_pr_artifacts(&real_state, "task")
                .expect_err("hardlink")
                .contains("unsafe task PR-check artifact")
        );
    }

    #[test]
    fn metadata_home_and_quarantine_content_faults_are_explicit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let invalid_utf8 = temp.path().join("invalid.meta");
        fs::write(&invalid_utf8, [0xff]).expect("invalid");
        assert!(
            metadata(&invalid_utf8)
                .expect_err("utf8")
                .contains("not valid UTF-8")
        );

        let home = fs::canonicalize(temp.path())
            .expect("canonical tempdir")
            .join("daemon-home");
        seed_home(&home, "daemon");
        fs::write(home.join("data"), "not a directory").expect("data");
        assert!(
            validate_home(&context, "daemon", &home)
                .expect_err("shape")
                .contains("is not a directory")
        );
        fs::remove_file(home.join("data")).expect("remove");
        fs::write(home.join(MARKER), [0xff]).expect("marker");
        assert!(
            validate_home(&context, "daemon", &home)
                .expect_err("marker utf8")
                .contains("marker is not valid UTF-8")
        );

        let state = temp.path().join("quarantine-state");
        let quarantine = state.join(".pr-check-quarantine");
        fs::create_dir_all(&quarantine).expect("quarantine");
        fs::set_permissions(&quarantine, fs::Permissions::from_mode(0o700)).expect("mode");
        let entry = quarantine.join("task.legacy");
        fs::write(&entry, "legacy").expect("entry");
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o644)).expect("mode");
        assert!(
            validate_pr_artifacts(&state, "task")
                .expect_err("entry mode")
                .contains("unsafe task quarantine entry")
        );
        fs::remove_file(&entry).expect("remove");
        symlink("missing", &entry).expect("symlink");
        assert!(
            validate_pr_artifacts(&state, "task")
                .expect_err("entry symlink")
                .contains("unsafe task quarantine entry")
        );
    }

    #[test]
    fn recovery_rejects_a_journal_target_inside_the_active_home() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        fs::create_dir_all(&context.state).expect("state");
        let journal = context.state.join(format!("{JOURNAL_PREFIX}unsafe"));
        publish(
            &journal,
            &Journal {
                id: "unsafe".to_owned(),
                home: context.home.join("data/victim").display().to_string(),
                stage: "home-removed".to_owned(),
                home_allocation: None,
            },
        )
        .expect("journal");

        let result = recover(&context, None, &mut |_| Ok(()));

        assert!(result.is_err());
        assert!(journal.exists());
    }

    #[test]
    fn live_child_worktree_inventory_and_unlisted_task_paths_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let project = temp.path().join("project");
        fs::create_dir(&project).expect("project");
        let git = |directory: &Path, arguments: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(directory)
                    .args(arguments)
                    .status()
                    .expect("git")
                    .success()
            );
        };
        git(&project, &["init", "-q", "-b", "main"]);
        fs::write(project.join("tracked"), "base\n").expect("tracked");
        git(&project, &["add", "."]);
        git(
            &project,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-q",
                "-m",
                "base",
            ],
        );
        let child_worktree = temp.path().join("child-worktree");
        git(
            &project,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "child-branch",
                child_worktree.to_str().unwrap(),
            ],
        );
        assert!(linked_runtime_worktree(&project, &resolved(&child_worktree)).expect("slot"));

        let mut local_only = BTreeMap::new();
        local_only.insert("kind".into(), "delivery".into());
        local_only.insert("mode".into(), "local-only".into());
        let non_project = temp.path().join("non-project");
        fs::create_dir(&non_project).expect("non-project");
        assert!(
            validate_worktree_safety(
                &context,
                "local",
                &local_only,
                &child_worktree,
                &non_project,
            )
            .expect_err("default branch")
            .contains("cannot determine default branch")
        );

        let parent = fs::canonicalize(temp.path())
            .expect("canonical tempdir")
            .join("parent-home");
        seed_home(&parent, "parent");
        fs::create_dir(parent.join("state")).expect("state");
        fs::write(
            parent.join("state/child.meta"),
            format!(
                "kind=delivery\nworktree={}\nproject={}\n",
                child_worktree.display(),
                project.display()
            ),
        )
        .expect("meta");
        assert_eq!(
            validate_children(&context, &parent)
                .expect("children")
                .len(),
            1
        );
        let lock = PathBuf::from(
            git_text(&child_worktree, &["rev-parse", "--git-path", "index.lock"])
                .expect("lock path"),
        );
        let lock = if lock.is_absolute() {
            lock
        } else {
            child_worktree.join(lock)
        };
        fs::write(&lock, "held").expect("lock");
        assert!(
            validate_children(&context, &parent)
                .expect_err("lock refusal")
                .contains("not provably stale")
        );
        fs::remove_file(&lock).expect("remove lock");
        let mut killed = Vec::new();
        cleanup_children(&context, &parent, &mut |path| {
            killed.push(path.to_owned());
            Ok(())
        })
        .expect_err("legacy child ownership remains retained for migration");
        assert_eq!(killed.len(), 1);
        assert!(child_worktree.exists());

        let unrelated = temp.path().join("unrelated");
        fs::create_dir(&unrelated).expect("unrelated");
        let delivery = BTreeMap::from([("kind".to_owned(), "delivery".to_owned())]);
        assert!(
            validate_worktree_safety(&context, "untracked", &delivery, &unrelated, &project)
                .expect_err("inspect")
                .contains("cannot inspect worktree")
        );
        fs::write(
            parent.join("state/missing-project.meta"),
            format!("kind=delivery\nworktree={}\n", unrelated.display()),
        )
        .expect("meta");
        assert!(
            validate_children(&context, &parent)
                .expect_err("missing project")
                .contains("has no project")
        );
        fs::write(
            parent.join("state/missing-project.meta"),
            format!(
                "kind=delivery\nworktree={}\nproject={}\n",
                unrelated.display(),
                project.display()
            ),
        )
        .expect("meta");
        assert!(
            validate_children(&context, &parent)
                .expect_err("unlisted child")
                .contains("is not a git worktree")
        );
        fs::remove_file(parent.join("state/missing-project.meta")).expect("remove meta");
        fs::write(
            context.state.join("scout.meta"),
            format!(
                "kind=scout\nworktree={}\nproject={}\n",
                unrelated.display(),
                project.display()
            ),
        )
        .expect("meta");
        let result = run(&[OsString::from("scout")], &context, |_| Ok(()));
        assert_eq!(result.status, 1);
        assert!(result.stderr.contains("is not a git worktree"));

        fs::write(
            context.state.join("delivery.meta"),
            format!(
                "kind=delivery\nworktree={}\nproject={}\n",
                unrelated.display(),
                project.display()
            ),
        )
        .expect("meta");
        let result = run_override("delivery", &context, |_| Ok(()));
        assert_eq!(result.status, 1);
        assert!(result.stderr.contains("is not a git worktree"));

        let removable = temp.path().join("removable-worktree");
        git(
            &project,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "remove-branch",
                removable.to_str().unwrap(),
            ],
        );
        let mut return_context = context.clone();
        return_context.root = project;
        assert!(remove_home(&return_context, &removable).is_err());
        assert!(removable.exists());
    }

    #[test]
    fn broken_home_surfaces_and_registry_aliases_are_refused() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = prepare_context(temp.path());
        let home = temp.path().join("daemon-home");
        seed_home(&home, "daemon");
        symlink(temp.path().join("missing"), home.join("state")).expect("broken state");
        assert!(
            validate_home(&context, "daemon", &home)
                .expect_err("broken state")
                .contains("resolves outside")
        );
        fs::remove_file(home.join("state")).expect("remove state");
        symlink(temp.path().join("outside"), home.join("state")).expect("state link");
        fs::create_dir(temp.path().join("outside")).expect("outside");
        assert!(
            has_children(&home)
                .expect_err("outside state")
                .contains("resolves outside")
        );

        let registry_target = temp.path().join("registry-target");
        fs::write(&registry_target, "# registry\n").expect("registry target");
        symlink(&registry_target, context.data.join("daemons.md")).expect("registry link");
        assert!(remove_registry_entry(&context, "daemon").is_err());
    }

    #[test]
    fn stale_home_journal_and_lease_never_kill_or_retire_a_new_generation() {
        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = context.home.with_file_name("owned-home");
        seed_home(&home, "owned");
        fs::write(
            context.state.join("owned.meta"),
            format!("kind=daemon\nhome={}\n", home.display()),
        )
        .unwrap();
        bind_private_home(&context, "owned", &home);
        let meta = context.state.join("owned.meta");
        let raw = fs::read_to_string(&meta).unwrap();
        let task = super::super::subagent_model::read_meta("owned", &raw).unwrap();
        let mut old = task.home_allocation.clone().unwrap();
        old.generation += 1;
        let journal_path = context.state.join(format!("{JOURNAL_PREFIX}owned"));
        let mut journal = Journal {
            id: "owned".into(),
            home: home.display().to_string(),
            stage: "prepared".into(),
            home_allocation: Some(old.clone()),
        };
        publish(&journal_path, &journal).unwrap();
        assert!(
            finish_transaction(&context, &journal_path, &mut journal, &mut |_| panic!(
                "stale journal killed endpoint"
            ))
            .unwrap_err()
            .contains("stale home retirement journal")
        );
        let mut stale_task = task;
        stale_task.home_allocation = Some(old);
        fs::write(
            &meta,
            format!(
                "canonical_model={}\n",
                serde_json::to_string(&stale_task).unwrap()
            ),
        )
        .unwrap();
        assert!(
            remove_home(&context, &home)
                .unwrap_err()
                .contains("stale or foreign home lease")
        );
        assert_eq!(
            fs::read_to_string(home.join("private-preserved")).unwrap(),
            "keep private state"
        );
        assert!(journal_path.exists());
    }

    #[test]
    fn unrelated_busy_recovery_and_escaped_child_state_preserve_their_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = context.home.with_file_name("child-home");
        fs::create_dir(&home).unwrap();
        assert!(validate_children(&context, &home).unwrap().is_empty());
        cleanup_children(&context, &home, &mut |_| panic!("no children")).unwrap();
        symlink(&context.state, home.join("state")).unwrap();
        assert!(
            validate_children(&context, &home)
                .unwrap_err()
                .contains("outside the daemon home")
        );
        let journal = context.state.join(format!("{JOURNAL_PREFIX}busy"));
        fs::write(&journal, "unread while busy").unwrap();
        let selected = context.state.join(format!("{JOURNAL_PREFIX}selected"));
        fs::write(&selected, "unread selected").unwrap();
        let invalid = context.state.join(format!("{JOURNAL_PREFIX}bad id"));
        fs::write(&invalid, "invalid id retained").unwrap();
        let _guard = DirectoryLock::acquire_wait(
            context.state.join(".teardown.busy.lock"),
            &SystemProcessProbe::default(),
            Duration::ZERO,
        )
        .unwrap();
        recover_other_tasks(&context, "selected", &mut |_| panic!("busy task killed")).unwrap();
        assert_eq!(fs::read_to_string(&journal).unwrap(), "unread while busy");
        assert!(selected.exists() && invalid.exists());
    }

    #[test]
    fn copied_canonical_metadata_and_temporary_path_aliases_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = context.home.with_file_name("daemon-home");
        seed_home(&home, "daemon");
        let meta = context.state.join("daemon.meta");
        fs::write(&meta, format!("kind=daemon\nhome={}\n", home.display())).unwrap();
        bind_private_home(&context, "daemon", &home);
        let copied_state = context.home.with_file_name("copied-state");
        fs::create_dir(&copied_state).unwrap();
        let copy = copied_state.join("daemon.meta");
        fs::copy(&meta, &copy).unwrap();
        assert!(
            metadata(&copy)
                .unwrap_err()
                .contains("owner state does not match")
        );
        fs::write(&copy, "kind=daemon\nkind=delivery\n").unwrap();
        assert!(
            metadata(&copy)
                .unwrap_err()
                .contains("duplicate task metadata field")
        );
        let temporary = copied_state.join("mx-task");
        symlink(&home, &temporary).unwrap();
        let values = BTreeMap::from([("tasktmp".into(), temporary.display().to_string())]);
        assert!(
            remove_task_tmp(&values, "task")
                .unwrap_err()
                .contains("unsafe task temporary")
        );
        fs::remove_file(&temporary).unwrap();
        fs::write(&temporary, "private").unwrap();
        assert!(remove_task_tmp(&values, "task").is_err());
        assert_eq!(fs::read_to_string(&temporary).unwrap(), "private");
        assert!(home.join("private-preserved").exists());
    }

    #[test]
    fn git_landing_conflicts_and_unreachable_remote_retain_content() {
        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let repo = &context.root;
        let git = |args: &[&str]| git_text(repo, args).unwrap();
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.invalid"]);
        fs::write(repo.join("file"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        git(&["checkout", "-b", "feature"]);
        fs::write(repo.join("file"), "feature\n").unwrap();
        git(&["commit", "-am", "feature"]);
        git(&["checkout", "main"]);
        fs::write(repo.join("file"), "main conflict\n").unwrap();
        git(&["commit", "-am", "main"]);
        git(&["checkout", "feature"]);
        assert!(!content_in_default(repo, repo));
        git(&[
            "remote",
            "add",
            "origin",
            repo.join("nonexistent.git").to_str().unwrap(),
        ]);
        assert!(!content_in_default(repo, repo));
        let legacy = context.home.with_file_name("legacy-linked");
        git(&["worktree", "add", "--detach", legacy.to_str().unwrap()]);
        seed_home(&legacy, "legacy");
        fs::write(legacy.join("private"), "keep").unwrap();
        assert!(
            remove_home(&context, &legacy)
                .unwrap_err()
                .contains("Git-backed home retained")
        );
        assert_eq!(fs::read_to_string(legacy.join("private")).unwrap(), "keep");
        assert!(listed_worktree(repo, &legacy).unwrap());
    }

    #[test]
    fn incomplete_forge_landing_evidence_is_never_permission_to_discard() {
        const CASE: &str = "MX_TEST_TEARDOWN_FORGE_CASE";
        if let Ok(scenario) = env::var(CASE) {
            let worktree = env::current_dir().unwrap();
            let target = if scenario == "missing-number" {
                "unidentified-pr"
            } else {
                "123"
            };
            assert!(
                !pr_landed(&worktree, Some(target)),
                "unsafe landing proof: {scenario}"
            );
            let trace = fs::read_to_string(env::var_os("MX_TEST_FORGE_TRACE").unwrap()).unwrap();
            assert!(
                trace.contains(&env::var("MX_TEST_FORGE_EXPECT").unwrap()),
                "wrong refusal branch: {trace}"
            );
            return;
        }
        // Each fake forge/Git observation runs in its own test process so the
        // hermetic PATH cannot affect concurrently executing real Git tests.
        let temp = tempfile::tempdir().unwrap();
        let shim = temp.path().join("bin");
        fs::create_dir(&shim).unwrap();
        let git = shim.join("git");
        fs::write(
            &git,
            r#"#!/bin/sh
[ "$1" = -C ] && shift 2
printf 'git %s\n' "$*" >> "$MX_TEST_FORGE_TRACE"
case "$1" in
rev-parse) printf 'feature\n';;
cat-file) case "$MX_TEST_TEARDOWN_FORGE_CASE" in missing-number|fetch-fail) exit 1;; esac;;
fetch) exit 1;;
merge-base) [ "$2" = --is-ancestor ] && exit 1
    [ "$MX_TEST_TEARDOWN_FORGE_CASE" = no-base ] && exit 1
    printf 'base\n';;
log) [ "$MX_TEST_TEARDOWN_FORGE_CASE" = log-fail ] && exit 1
    [ "$3" = HEAD ] && exit 1
    printf 'pr-commit\n';;
show) [ "$MX_TEST_TEARDOWN_FORGE_CASE" = patch-fail ] && exit 1
    printf 'patch-body\n';;
patch-id) printf 'patch-id commit\n';;
*) exit 1;;
esac
"#,
        )
        .unwrap();
        let gh = shim.join("gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
printf 'gh %s\n' "$*" >> "$MX_TEST_FORGE_TRACE"
case "$MX_TEST_TEARDOWN_FORGE_CASE" in
view-fail) exit 1;;
malformed) printf 'unknown\n';;
open) printf 'OPEN\thead\n';;
*) printf 'MERGED\thead\n';;
esac
"#,
        )
        .unwrap();
        for path in [&git, &gh] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        for (scenario, expected) in [
            ("view-fail", "gh pr view 123"),
            ("malformed", "gh pr view 123"),
            ("open", "gh pr view 123"),
            ("missing-number", "git cat-file -e head^{commit}"),
            ("fetch-fail", "git fetch --quiet origin refs/pull/123/head"),
            ("no-base", "git merge-base HEAD head"),
            ("log-fail", "git log --format=%H base..head"),
            (
                "patch-fail",
                "git show --pretty=medium --no-ext-diff pr-commit",
            ),
            ("unpushed-fail", "git log --format=%H HEAD --not --remotes"),
        ] {
            let trace = temp.path().join(format!("{scenario}.trace"));
            let result = Command::new(env::current_exe().unwrap())
                .args(["--exact", "lifecycle::teardown::tests::incomplete_forge_landing_evidence_is_never_permission_to_discard", "--nocapture"])
                .current_dir(temp.path()).env("PATH", &shim).env(CASE, scenario)
                .env("MX_TEST_FORGE_TRACE", trace).env("MX_TEST_FORGE_EXPECT", expected)
                .output().unwrap();
            assert!(
                result.status.success(),
                "{scenario}: {}",
                String::from_utf8_lossy(&result.stdout)
            );
        }
    }

    #[test]
    fn matching_origin_clone_observation_does_not_authorize_unknown_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let clone = context.home.with_file_name("independent-clone");
        fs::create_dir(&clone).unwrap();
        for repo in [&context.root, &clone] {
            assert!(git_success(repo, &["init", "-b", "main"]));
            assert!(git_success(
                repo,
                &["remote", "add", "origin", "../remote.git"]
            ));
        }
        assert!(listed_worktree(&context.root, &clone).unwrap());
        let values = BTreeMap::from([
            ("worktree".into(), clone.display().to_string()),
            ("project".into(), context.root.display().to_string()),
        ]);
        fs::write(clone.join("private"), "keep").unwrap();
        assert!(
            return_allocation("legacy", &values, &clone)
                .unwrap_err()
                .contains("unknown worktree ownership")
        );
        assert_eq!(fs::read_to_string(clone.join("private")).unwrap(), "keep");
        assert!(git_success(
            &clone,
            &[
                "remote",
                "set-url",
                "origin",
                "ssh://different.invalid/repo.git"
            ]
        ));
        assert!(!listed_worktree(&context.root, &clone).unwrap());
    }

    #[test]
    fn coordinator_stop_restart_stop_binds_each_execution_endpoint() {
        use super::super::subagent_model::{WorkState, read_meta, write_meta};

        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = temp.path().join("coordinator-home");
        coordinator_record(&context, "coord", &home, "endpoint-a");
        fs::write(home.join("state/independent-child.meta"), "active-child\n").unwrap();
        let meta = context.state.join("coord.meta");
        let stopped = std::sync::Mutex::new(Vec::new());
        let first = run(
            &["coord".into(), "--stop-coordinator".into()],
            &context,
            |path| {
                let record = read_meta("coord", &fs::read_to_string(path).unwrap()).unwrap();
                stopped
                    .lock()
                    .unwrap()
                    .push(record.runtime.endpoint.unwrap());
                Ok(())
            },
        );
        assert_eq!(first.status, 0, "{}", first.stderr);
        let raw = fs::read_to_string(&meta).unwrap();
        let mut record = read_meta("coord", &raw).unwrap();
        assert!(record.runtime.endpoint.is_none());
        assert_eq!(record.schedule.state, WorkState::WaitingExternal);

        record.runtime.endpoint = Some("endpoint-b".into());
        record.runtime.session_id = Some("session-b".into());
        record.schedule.state = WorkState::Running;
        record.schedule.waiting_condition = None;
        let raw = raw
            .lines()
            .filter(|line| !line.starts_with("window="))
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        fs::write(&meta, write_meta(&raw, &record).unwrap()).unwrap();
        let second = run(
            &["coord".into(), "--stop-coordinator".into()],
            &context,
            |path| {
                let record = read_meta("coord", &fs::read_to_string(path).unwrap()).unwrap();
                stopped
                    .lock()
                    .unwrap()
                    .push(record.runtime.endpoint.unwrap());
                Ok(())
            },
        );
        assert_eq!(second.status, 0, "{}", second.stderr);
        assert_eq!(&*stopped.lock().unwrap(), &["endpoint-a", "endpoint-b"]);
        assert_eq!(
            fs::read_to_string(home.join("state/independent-child.meta")).unwrap(),
            "active-child\n"
        );
        assert_eq!(
            fs::read_dir(&context.state)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".coordinator-control.coord.stop-coordinator"))
                .count(),
            2
        );
    }

    #[test]
    fn subtree_control_never_kills_a_replacement_after_intent() {
        use super::super::subagent_model::{
            ArtifactKind, AssignmentRole, TaskRecord, read_meta, write_meta,
        };

        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = temp.path().join("coordinator-home");
        coordinator_record(&context, "coord", &home, "coordinator-old");
        let owner = fs::canonicalize(&home).unwrap();
        let mut child = TaskRecord::new(
            "child".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            "coord".into(),
            format!("root-home:{}", context.root.display()),
            owner.to_string_lossy().into_owned(),
        );
        child.runtime.provider = "fixture".into();
        child.runtime.endpoint = Some("child-old".into());
        let child_meta = home.join("state/child.meta");
        fs::write(&child_meta, write_meta("kind=delivery\n", &child).unwrap()).unwrap();
        let coordinator_meta = context.state.join("coord.meta");
        let killed = std::sync::Mutex::new(Vec::new());
        let result = run(
            &["coord".into(), "--stop-subtree".into()],
            &context,
            |path| {
                let id = path.file_stem().unwrap().to_str().unwrap();
                let record = read_meta(id, &fs::read_to_string(path).unwrap()).unwrap();
                killed
                    .lock()
                    .unwrap()
                    .push(record.runtime.endpoint.clone().unwrap());
                if id == "child" {
                    let raw = fs::read_to_string(&coordinator_meta).unwrap();
                    let mut replacement = read_meta("coord", &raw).unwrap();
                    replacement.runtime.endpoint = Some("coordinator-new".into());
                    let raw = raw
                        .lines()
                        .filter(|line| !line.starts_with("window="))
                        .map(|line| format!("{line}\n"))
                        .collect::<String>();
                    fs::write(&coordinator_meta, write_meta(&raw, &replacement).unwrap()).unwrap();
                }
                Ok(())
            },
        );
        assert_eq!(result.status, 1);
        assert!(
            result.stderr.contains("replacement was not stopped"),
            "{}",
            result.stderr
        );
        assert_eq!(&*killed.lock().unwrap(), &["child-old"]);
        assert_eq!(
            read_meta("coord", &fs::read_to_string(coordinator_meta).unwrap())
                .unwrap()
                .runtime
                .endpoint
                .as_deref(),
            Some("coordinator-new")
        );
    }

    #[test]
    fn retirement_and_override_retain_home_when_parent_outcomes_are_unsettled() {
        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = temp.path().join("coordinator-home");
        coordinator_record(&context, "coord", &home, "coordinator");
        fs::create_dir_all(home.join("state/parent-outbox")).unwrap();
        let pending = super::super::parent_channel::ParentOutcome {
            schema_version: super::super::parent_channel::SCHEMA_VERSION,
            event: super::super::subagent_model::MessageEnvelope {
                schema_version: super::super::subagent_model::SCHEMA_VERSION,
                message_id: "unsettled".into(),
                task_id: "coord".into(),
                task_home: Some(home.to_string_lossy().into_owned()),
                parent_home: Some(context.home.to_string_lossy().into_owned()),
                attempt: None,
                parent_id: Some("root".into()),
                sender: "coord".into(),
                recipient: "root".into(),
                brief_revision: Some(1),
                kind: "done".into(),
                correlation_id: "outcome-1".into(),
                created_at: "2026-09-15T00:00:00Z".into(),
                summary: "unfinished delivery".into(),
                artifact: None,
                acknowledgement: super::super::subagent_model::Acknowledgement::Pending,
            },
            route: super::super::parent_channel::RouteBinding {
                sender_id: "coord".into(),
                sender_home: home.to_string_lossy().into_owned(),
                sender_state: home.join("state").to_string_lossy().into_owned(),
                recipient_id: "root".into(),
                recipient_home: context.home.to_string_lossy().into_owned(),
                recipient_state: context.state.to_string_lossy().into_owned(),
                root_id: format!("root-home:{}", context.root.display()),
                attempt_id: "attempt-1".into(),
                attempt_generation: 1,
                brief_revision: 1,
                scope_revision: Some(1),
                assignment_generation: 1,
                recipient_attempt_id: None,
                recipient_attempt_generation: None,
                recipient_scope_revision: None,
                recipient_assignment_generation: None,
                recipient_domain_id: None,
            },
            hops: Vec::new(),
            route_repairs: Vec::new(),
            created_epoch: 1,
            delivery: super::super::parent_channel::DeliveryState::Pending,
            delivered_epoch: None,
        };
        fs::write(
            home.join("state/parent-outbox/unsettled.json"),
            serde_json::to_vec(&pending).unwrap(),
        )
        .unwrap();

        let ordinary = run(&["coord".into(), "--retire-home".into()], &context, |_| {
            panic!("unsettled outcome must block endpoint teardown")
        });
        assert_eq!(ordinary.status, 1);
        assert!(home.exists());
        assert!(context.state.join("coord.meta").exists());

        let override_result = run_override("coord", &context, |_| {
            panic!("override must not discard unsettled parent outcome")
        });
        assert_eq!(override_result.status, 1);
        assert!(home.join("state/parent-outbox/unsettled.json").exists());
    }

    #[test]
    fn coordinator_control_receipt_recovers_exact_execution_and_rejects_tampering() {
        use super::super::subagent_model::read_meta;

        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = temp.path().join("coordinator-home");
        coordinator_record(&context, "coord", &home, "endpoint-a");
        let meta = context.state.join("coord.meta");
        let record = read_meta("coord", &fs::read_to_string(&meta).unwrap()).unwrap();
        let first =
            coordinator_control(&context, "coord", &meta, &record, "checkpoint", &mut |_| {
                Err("provider observation unavailable".into())
            })
            .unwrap_err();
        assert!(first.contains("could not verify endpoint stop"));
        assert_eq!(
            read_meta("coord", &fs::read_to_string(&meta).unwrap())
                .unwrap()
                .runtime
                .endpoint
                .as_deref(),
            Some("endpoint-a")
        );
        let receipt = fs::read_dir(&context.state)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".coordinator-control.coord.checkpoint")
            })
            .unwrap();
        let uncertain: ControlReceipt =
            serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(uncertain.stage, "uncertain");
        assert_eq!(
            uncertain.reason.as_deref(),
            Some("provider observation unavailable")
        );

        let recovered =
            coordinator_control(&context, "coord", &meta, &record, "checkpoint", &mut |_| {
                Ok(())
            })
            .unwrap();
        assert!(recovered.contains("checkpoint complete"));
        let replay =
            coordinator_control(&context, "coord", &meta, &record, "checkpoint", &mut |_| {
                panic!("committed receipt must not stop twice")
            })
            .unwrap();
        assert!(replay.contains("already complete"));

        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = temp.path().join("coordinator-home");
        coordinator_record(&context, "coord", &home, "endpoint-b");
        let meta = context.state.join("coord.meta");
        let record = read_meta("coord", &fs::read_to_string(&meta).unwrap()).unwrap();
        coordinator_control(
            &context,
            "coord",
            &meta,
            &record,
            "stop-coordinator",
            &mut |_| Err("retain receipt".into()),
        )
        .unwrap_err();
        let receipt = fs::read_dir(&context.state)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".coordinator-control.coord.stop-coordinator")
            })
            .unwrap();
        fs::write(&receipt, "not-json\n").unwrap();
        assert!(
            coordinator_control(
                &context,
                "coord",
                &meta,
                &record,
                "stop-coordinator",
                &mut |_| panic!("malformed receipt must fail before stop"),
            )
            .unwrap_err()
            .contains("malformed coordinator control receipt")
        );
    }

    #[test]
    fn subtree_control_rejects_unreadable_and_foreign_child_authority_before_stopping() {
        use super::super::subagent_model::{ArtifactKind, AssignmentRole, TaskRecord, write_meta};

        let temp = tempfile::tempdir().unwrap();
        let context = prepare_context(temp.path());
        let home = temp.path().join("coordinator-home");
        coordinator_record(&context, "coord", &home, "endpoint");
        fs::write(home.join("state/child.meta"), [0xff]).unwrap();
        let unreadable = run(&["coord".into(), "--stop-subtree".into()], &context, |_| {
            panic!("unreadable child must fail before stop")
        });
        assert_eq!(unreadable.status, 1);
        assert!(unreadable.stderr.contains("not UTF-8"));

        fs::remove_file(home.join("state/child.meta")).unwrap();
        let mut child = TaskRecord::new(
            "child".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            "coord".into(),
            format!("root-home:{}", context.root.display()),
            home.to_string_lossy().into_owned(),
        );
        child.owner_state = Some(context.state.to_string_lossy().into_owned());
        fs::write(
            home.join("state/child.meta"),
            write_meta("kind=delivery\n", &child).unwrap(),
        )
        .unwrap();
        let foreign = run(&["coord".into(), "--stop-subtree".into()], &context, |_| {
            panic!("foreign child must fail before stop")
        });
        assert_eq!(foreign.status, 1);
        assert!(foreign.stderr.contains("owned by another state directory"));
    }
}

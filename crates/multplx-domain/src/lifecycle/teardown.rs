//! Transactional retirement of persistent daemon homes.

use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
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

use super::home_seed::resolved;

pub const USAGE: &str = "usage: mx-teardown.sh <task-id> [--override <request-id>]\n";
const JOURNAL_PREFIX: &str = ".teardown.transaction.";
const MARKER: &str = ".mx-daemon-home";

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
    let bytes = read_regular(path, "task metadata")?;
    let text =
        String::from_utf8(bytes).map_err(|_| "task metadata is not valid UTF-8".to_owned())?;
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        values.insert(key.to_owned(), value.to_owned());
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
    let output = Command::new("git")
        .args([
            "-C".as_ref(),
            project.as_os_str(),
            "-c".as_ref(),
            "core.quotePath=false".as_ref(),
            "worktree".as_ref(),
            "list".as_ref(),
            "--porcelain".as_ref(),
        ])
        .output()
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
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_success(directory: &Path, arguments: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .status()
        .is_ok_and(|status| status.success())
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

fn pr_landed(worktree: &Path, recorded: Option<&str>) -> bool {
    let branch = git_text(worktree, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|| "HEAD".to_owned());
    let target = recorded
        .map(str::to_owned)
        .or_else(|| {
            let output = Command::new("gh")
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
                .current_dir(worktree)
                .output()
                .ok()?;
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        })
        .filter(|value| !value.is_empty());
    let Some(target) = target else { return false };
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &target,
            "--json",
            "state,headRefOid",
            "-q",
            ".state + \"\\t\" + .headRefOid",
        ])
        .current_dir(worktree)
        .output();
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
    let shown = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["show", "--pretty=medium", "--no-ext-diff", commit])
        .output()
        .ok()?;
    if !shown.status.success() {
        return None;
    }
    let mut child = Command::new("git")
        .args(["patch-id", "--stable"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    std::io::Write::write_all(child.stdin.as_mut()?, &shown.stdout).ok()?;
    drop(child.stdin.take());
    let output = child.wait_with_output().ok()?;
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

fn validate_worktree_safety(
    context: &Context,
    id: &str,
    values: &BTreeMap<String, String>,
    worktree: &Path,
    project: &Path,
) -> Result<(), String> {
    if values.get("kind").map(String::as_str) == Some("scout") {
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
        .ok_or_else(|| {
            format!(
                "REFUSED: cannot inspect worktree {} for uncommitted changes.",
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
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn stale_lock_proof(
    lock: &Path,
    minimum_age: f64,
    probe: &multplx_core::locks::LsofProbe,
) -> Result<bool, multplx_core::error::CoreError> {
    if env::var("MX_TEARDOWN_TEST_LOCK_MTIME_ERROR").as_deref() == Ok("1") {
        return Err(multplx_core::error::CoreError::Io {
            operation: "metadata",
            path: lock.to_owned(),
            source: std::io::Error::other("injected mtime read failure"),
        });
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

fn git_status_after_stale_lock_cleanup(worktree: &Path) -> Option<String> {
    if let Some(status) = git_text(worktree, &["status", "--porcelain"]) {
        return Some(status);
    }
    let lock = index_lock(worktree)?;
    if !lock.exists() {
        return None;
    }
    let retries = environment_usize("MX_TREEHOUSE_RETURN_LOCK_RETRIES", 3);
    let wait = environment_seconds(
        "MX_TREEHOUSE_RETURN_LOCK_RETRY_WAIT_SECS",
        Some("MX_STALE_WORKTREE_LOCK_RETRY_WAIT_SECS"),
        1.0,
    );
    for _ in 0..retries {
        std::thread::sleep(Duration::from_secs_f64(wait.max(0.0)));
        if let Some(status) = git_text(worktree, &["status", "--porcelain"]) {
            return Some(status);
        }
    }
    let minimum_age = environment_seconds("MX_STALE_WORKTREE_LOCK_AGE_SECS", None, 30.0);
    let probe = multplx_core::locks::LsofProbe;
    if stale_lock_proof(&lock, minimum_age, &probe).ok()? {
        fs::remove_file(&lock).ok()?;
        eprintln!(
            "teardown: removed provably-stale git lock {}",
            lock.display()
        );
        return git_text(worktree, &["status", "--porcelain"]);
    }
    None
}

fn return_worktree(project: &Path, worktree: &Path) -> Result<(), String> {
    let retries = environment_usize("MX_TREEHOUSE_RETURN_LOCK_RETRIES", 3);
    let wait = environment_seconds(
        "MX_TREEHOUSE_RETURN_LOCK_RETRY_WAIT_SECS",
        Some("MX_STALE_WORKTREE_LOCK_RETRY_WAIT_SECS"),
        1.0,
    );
    let minimum_age = environment_seconds("MX_STALE_WORKTREE_LOCK_AGE_SECS", None, 30.0);
    for attempt in 0..=retries {
        let output = Command::new("treehouse")
            .args(["return".as_ref(), "--force".as_ref(), worktree.as_os_str()])
            .current_dir(project)
            .output()
            .map_err(|error_value| format!("treehouse command unavailable: {error_value}"))?;
        if output.status.success() {
            if attempt > 0 {
                eprintln!("teardown: worktree return succeeded on retry");
            }
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr);
        if !detail.contains("index.lock") {
            return Err(format!(
                "treehouse return failed for task worktree {}: {}",
                worktree.display(),
                detail.trim()
            ));
        }
        let Some(lock) = index_lock(worktree) else {
            return Err("treehouse return failed: cannot resolve git index.lock".to_owned());
        };
        if attempt < retries {
            eprintln!(
                "teardown: git index.lock blocked worktree return; waiting {wait}s and retrying"
            );
            std::thread::sleep(Duration::from_secs_f64(wait.max(0.0)));
            continue;
        }
        let probe = multplx_core::locks::LsofProbe;
        let stale = stale_lock_proof(&lock, minimum_age, &probe);
        match stale {
            Ok(true) => {
                fs::remove_file(&lock).map_err(|error_value| error_value.to_string())?;
                eprintln!(
                    "teardown: removed provably-stale git lock {}",
                    lock.display()
                );
                let retry = Command::new("treehouse")
                    .args(["return".as_ref(), "--force".as_ref(), worktree.as_os_str()])
                    .current_dir(project)
                    .output()
                    .map_err(|error_value| error_value.to_string())?;
                if retry.status.success() {
                    return Ok(());
                }
                return Err("treehouse return still failing after stale-lock cleanup".to_owned());
            }
            Ok(false) => {
                let holder = multplx_core::locks::HolderProbe::holder_status(&probe, &lock);
                let qualifier = if holder == multplx_core::locks::HolderStatus::Unknown {
                    " (lsof check failed)"
                } else {
                    ""
                };
                return Err(format!(
                    "git lock {} persisted across {} retries (waiting {wait}s each) and is not provably stale{qualifier}",
                    lock.display(),
                    retries
                ));
            }
            Err(error_value) => {
                return Err(format!(
                    "cannot read mtime for git lock {}; not provably stale: {error_value}",
                    lock.display()
                ));
            }
        }
    }
    unreachable!()
}

fn validate_children(context: &Context, home: &Path) -> Result<Vec<PathBuf>, String> {
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
        let kind = values.get("kind").map(String::as_str).unwrap_or("delivery");
        if kind == "daemon" {
            let child_home = values
                .get("home")
                .filter(|value| !value.is_empty())
                .or_else(|| values.get("worktree"))
                .ok_or("child daemon metadata has no home")?;
            let child_home = validate_home(context, &child_id, Path::new(child_home))?;
            let _ = validate_children(context, &child_home)?;
        } else if let Some(worktree) = values.get("worktree").filter(|value| !value.is_empty())
            && Path::new(worktree).exists()
        {
            let target = validate_removal_target(context, Path::new(worktree), "child worktree")?;
            let project = values
                .get("project")
                .ok_or("child metadata has no project")?;
            if !listed_worktree(Path::new(project), &target)? {
                return Err(format!(
                    "REFUSED: unsafe child worktree removal target {worktree} is not a git worktree for {project}"
                ));
            }
            let lock_output = Command::new("git")
                .args([
                    "-C".as_ref(),
                    target.as_os_str(),
                    "rev-parse".as_ref(),
                    "--git-path".as_ref(),
                    "index.lock".as_ref(),
                ])
                .output()
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
        let values = metadata(&meta_path)?;
        kill(&meta_path)?;
        if values.get("kind").map(String::as_str) == Some("daemon") {
            let child_home = values
                .get("home")
                .filter(|value| !value.is_empty())
                .or_else(|| values.get("worktree"))
                .ok_or("child daemon has no home")?;
            cleanup_children(context, Path::new(child_home), kill)?;
            remove_home(context, Path::new(child_home))?;
        } else if let Some(worktree) = values.get("worktree").filter(|value| !value.is_empty())
            && Path::new(worktree).exists()
        {
            let project = values.get("project").ok_or("child task has no project")?;
            let output = Command::new("treehouse")
                .args([
                    "return".as_ref(),
                    "--force".as_ref(),
                    Path::new(worktree).as_os_str(),
                ])
                .current_dir(project)
                .output();
            match output {
                Ok(output) if output.status.success() => {}
                Ok(output) if String::from_utf8_lossy(&output.stderr).contains("index.lock") => {
                    return Err(format!(
                        "child treehouse return is not provably stale: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
                _ => fs::remove_dir_all(worktree).map_err(|error_value| error_value.to_string())?,
            }
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

fn treehouse_slot(root: &Path, home: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .args([
            "-C",
            path_text(root, "Multplx root")?.as_str(),
            "-c",
            "core.quotePath=false",
            "worktree",
            "list",
            "--porcelain",
        ])
        .output()
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
    remove_home_with(context, home, OsStr::new("treehouse"))
}

fn remove_home_with(context: &Context, home: &Path, treehouse: &OsStr) -> Result<(), String> {
    if !home.exists() {
        return Ok(());
    }
    if treehouse_slot(&context.root, home)? {
        let output = Command::new(treehouse)
            .args(["return".as_ref(), "--force".as_ref(), home.as_os_str()])
            .current_dir(&context.root)
            .output()
            .map_err(|error_value| format!("treehouse command unavailable: {error_value}"))?;
        if !output.status.success() {
            return Err(format!(
                "treehouse return failed for daemon home {}; lease may still be held{}",
                home.display(),
                if output.stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {}", String::from_utf8_lossy(&output.stderr).trim())
                }
            ));
        }
    } else {
        fs::remove_dir_all(home).map_err(|error_value| error_value.to_string())?;
    }
    Ok(())
}

fn remove_registry_entry(context: &Context, id: &str) -> Result<(), String> {
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
    if path.file_name().and_then(|value| value.to_str()) != Some(&format!("mx-{id}")) {
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
        if home.exists() {
            validate_home(context, &journal.id, &home)?;
        }
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
        kill(&context.state.join(format!("{}.meta", journal.id)))?;
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

fn recover<F>(context: &Context, kill: &mut F) -> Result<(), String>
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

fn execute<F>(args: &[OsString], context: &Context, mut kill: F) -> Result<String, String>
where
    F: FnMut(&Path) -> Result<(), String>,
{
    let [raw_id] = args else {
        return Err(USAGE.trim_end().to_owned());
    };
    let id = raw_id.to_str().ok_or("task id is not valid UTF-8")?;
    crate::review_delivery::OperationalTaskId::parse(id.to_owned())
        .map_err(|_| "invalid teardown request".to_owned())?;
    path_text(&context.root, "Multplx root")?;
    path_text(&context.home, "active Multplx home")?;
    path_text(&context.data, "active data")?;
    path_text(&context.state, "active state")?;
    let state = require_owned_directory(&context.state, "active state")?;
    let _lock = DirectoryLock::acquire_wait(
        state.join(".teardown.lock"),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error_value| format!("cannot acquire teardown lock: {error_value}"))?;
    recover(context, &mut kill)?;
    let meta = context.state.join(format!("{id}.meta"));
    let values = metadata(&meta).map_err(|error_value| {
        if !meta.exists() {
            format!("no meta for task {id} at {}", meta.display())
        } else {
            error_value
        }
    })?;
    if values.get("kind").map(String::as_str).unwrap_or("delivery") != "daemon" {
        validate_pr_artifacts(&context.state, id)?;
        if let (Some(raw_worktree), Some(raw_project)) = (
            values.get("worktree").filter(|value| !value.is_empty()),
            values.get("project").filter(|value| !value.is_empty()),
        ) && Path::new(raw_worktree).exists()
        {
            let worktree = validate_removal_target(
                context,
                &require_owned_directory(Path::new(raw_worktree), "task worktree")?,
                "task worktree",
            )?;
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
            return_worktree(&project, worktree_argument)?;
        }
        kill(&meta)?;
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
        let _lock = DirectoryLock::acquire_wait(
            state.join(".teardown.lock"),
            &SystemProcessProbe::default(),
            Duration::from_secs(5),
        )
        .map_err(|error_value| format!("cannot acquire teardown lock: {error_value}"))?;
        recover(context, &mut kill)?;
        let meta = context.state.join(format!("{id}.meta"));
        let values = metadata(&meta)?;
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
            kill(&meta)?;
            if values.get("single_checkout").map(String::as_str) == Some("yes") {
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
                fs::remove_file(&record).map_err(|error_value| error_value.to_string())?;
            } else if Path::new(raw_worktree).exists() {
                let worktree = require_owned_directory(Path::new(raw_worktree), "task worktree")?;
                let project = require_owned_directory(Path::new(raw_project), "task project")?;
                let target = validate_removal_target(context, &worktree, "task worktree")?;
                if !listed_worktree(&project, &target)? {
                    return Err(format!(
                        "REFUSED: unsafe task worktree removal target {} is not a git worktree for {}",
                        target.display(),
                        project.display()
                    ));
                }
                let output = Command::new("treehouse")
                    .args(["return".as_ref(), "--force".as_ref(), target.as_os_str()])
                    .current_dir(&project)
                    .output()
                    .map_err(|error_value| {
                        format!("treehouse command unavailable: {error_value}")
                    })?;
                if !output.status.success() {
                    return Err(format!(
                        "treehouse return failed for task worktree {}: {}",
                        target.display(),
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
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
        validate_pr_artifacts(&context.state, id)?;
        let _ = validate_children(context, &home)?;
        cleanup_children(context, &home, &mut kill)?;
        let journal_path = context.state.join(format!("{JOURNAL_PREFIX}{id}"));
        let mut journal = Journal {
            id: id.to_owned(),
            home: path_text(&home, "daemon home")?,
            stage: "prepared".to_owned(),
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
        let safe = temp.path().join("mx-task");
        fs::create_dir(&safe).expect("safe");
        values.insert("tasktmp".into(), safe.display().to_string());
        remove_task_tmp(&values, "task").expect("remove");
        assert!(!safe.exists());
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
            },
        )
        .expect("publish");
        recover(&context, &mut |_| Ok(())).expect("recover");
        assert!(!recovery.exists());

        let malformed = context.state.join(format!("{JOURNAL_PREFIX}bad"));
        publish(
            &malformed,
            &Journal {
                id: "bad".into(),
                home: temp.path().join("gone").display().to_string(),
                stage: "unexpected".into(),
            },
        )
        .expect("publish");
        assert!(
            recover(&context, &mut |_| Ok(()))
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
        let mut killed = false;
        let result = run_override("daemon", &context, |_| {
            killed = true;
            Ok(())
        });
        assert_eq!(result.status, 0, "{}", result.stderr);
        assert!(killed);
        assert!(!home.exists());
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
            recover(&context, &mut |_| Ok(()))
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
            },
        )
        .expect("publish");
        assert!(
            recover(&context, &mut |_| Ok(()))
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
            Some("")
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
        assert!(git_status_after_stale_lock_cleanup(&not_repo).is_none());
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
            },
        )
        .expect("journal");

        let result = recover(&context, &mut |_| Ok(()));

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
        assert!(treehouse_slot(&project, &resolved(&child_worktree)).expect("slot"));

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
        .expect("return child worktree");
        assert_eq!(killed.len(), 1);
        assert!(!child_worktree.exists());

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
        let treehouse = temp.path().join("treehouse");
        fs::write(
            &treehouse,
            "#!/bin/sh\n[ \"$1\" = return ] && [ \"$2\" = --force ] || exit 2\nexec git worktree remove --force \"$3\"\n",
        )
        .expect("treehouse fixture");
        let mut permissions = fs::metadata(&treehouse)
            .expect("treehouse metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&treehouse, permissions).expect("treehouse mode");
        remove_home_with(&return_context, &removable, treehouse.as_os_str())
            .expect("return worktree home");
        assert!(!removable.exists());
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
}

//! Non-destructive project-clone refresh.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, SystemTime};

use multplx_core::locks::{LsofProbe, git_lock_is_provably_stale};

use crate::project_registry::{DeliveryMode, resolve as resolve_project_mode};

#[derive(Clone, Debug)]
pub struct SyncContext {
    pub home: PathBuf,
    pub projects: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct SyncOutput {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
}

fn git(project: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
}

fn git_ok(project: &Path, args: &[&str]) -> bool {
    git(project, args).is_ok_and(|output| output.status.success())
}

fn git_text(project: &Path, args: &[&str]) -> Option<String> {
    let output = git(project, args).ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim_end_matches(['\n', '\r'])
            .to_owned()
    })
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_branch(project: &Path) -> Option<String> {
    if let Some(reference) = git_text(
        project,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) && !reference.is_empty()
    {
        return Some(
            reference
                .strip_prefix("origin/")
                .unwrap_or(&reference)
                .to_owned(),
        );
    }
    ["main", "master"]
        .into_iter()
        .find(|branch| {
            git_ok(
                project,
                &[
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{branch}"),
                ],
            )
        })
        .map(str::to_owned)
}

fn packed_refs_signature(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.contains("Unable to create '") && text.contains("packed-refs.lock': File exists")
        || text.contains("Unable to create \"") && text.contains("packed-refs.lock\": File exists")
}

fn packed_refs_lock(project: &Path) -> Option<PathBuf> {
    let path = git_text(project, &["rev-parse", "--git-path", "packed-refs.lock"])?;
    let path = PathBuf::from(path);
    Some(if path.is_absolute() {
        path
    } else {
        fs::canonicalize(project).ok()?.join(path)
    })
}

fn env_u64(name: &str, fallback: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn fetch(project: &Path, label: &str, result: &mut SyncOutput) -> Result<(), Vec<u8>> {
    let retries = env_u64("MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRIES", 3);
    let age = env_u64("MX_SYSTEM_SYNC_PACKED_REFS_LOCK_AGE_SECS", 30);
    let wait_raw = env::var("MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRY_WAIT_SECS")
        .unwrap_or_else(|_| "1".to_owned());
    let wait = wait_raw
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0);
    let wait = match wait {
        Some(value) => value,
        None => {
            result.stderr.push(format!(
                "system-sync: invalid packed-refs lock retry wait '{wait_raw}'; using 1s"
            ));
            1.0
        }
    };
    let run = || git(project, &["fetch", "origin", "--prune", "--quiet"]);
    let mut output = run().map_err(|error| error.to_string().into_bytes())?;
    if output.status.success() {
        return Ok(());
    }
    if !packed_refs_signature(&output.stderr) {
        return Err(output.stderr);
    }
    let lock = packed_refs_lock(project);
    let lock_desc = lock.as_ref().map_or_else(
        || "packed-refs.lock".to_owned(),
        |path| path.display().to_string(),
    );
    for attempt in 1..=retries {
        result.stderr.push(format!(
            "{label}: fetch blocked by packed-refs lock ({lock_desc}); waiting {wait_raw}s and retrying ({attempt}/{retries}) (owning process may be exiting)"
        ));
        thread::sleep(Duration::from_secs_f64(wait));
        output = run().map_err(|error| error.to_string().into_bytes())?;
        if output.status.success() {
            result.stderr.push(format!(
                "{label}: fetch succeeded on retry; packed-refs lock cleared on its own"
            ));
            result.stdout.push(format!(
                "{label}: recovered: packed-refs lock cleared on its own during retry"
            ));
            return Ok(());
        }
        if !packed_refs_signature(&output.stderr) {
            return Err(output.stderr);
        }
    }
    if let Some(lock) = lock.filter(|path| path.exists()) {
        let stale = git_lock_is_provably_stale(
            &lock,
            Some(project),
            Duration::from_secs(age),
            SystemTime::now(),
            &LsofProbe,
        )
        .unwrap_or(false);
        if stale {
            if let Err(error) = fs::remove_file(&lock) {
                result.stderr.push(format!(
                    "{label}: failed to remove provably-stale packed-refs lock {}; leaving it in place",
                    lock.display()
                ));
                let _ = error;
                return Err(output.stderr);
            }
            result.stderr.push(format!(
                "{label}: removed provably-stale packed-refs lock {} (age >= {age}s, no live holder) and retrying fetch",
                lock.display()
            ));
            output = run().map_err(|error| error.to_string().into_bytes())?;
            if output.status.success() {
                result.stderr.push(format!(
                    "{label}: fetch succeeded after stale packed-refs lock cleanup"
                ));
                result.stdout.push(format!(
                    "{label}: recovered: removed a stale packed-refs lock (no live holder)"
                ));
                return Ok(());
            }
            return Err(output.stderr);
        }
        result.stderr.push(format!(
            "{label}: fetch blocked by packed-refs lock {} that persisted across {retries} retries and is not provably stale (may belong to a live process); leaving it in place",
            lock.display()
        ));
        return Err(output.stderr);
    }
    result.stderr.push(format!(
        "{label}: fetch packed-refs lock signature persisted across {retries} retries even after the lock file disappeared"
    ));
    Err(output.stderr)
}

fn worktree_branches(project: &Path) -> Vec<String> {
    git_text(project, &["worktree", "list", "--porcelain"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix("branch refs/heads/").map(str::to_owned))
        .collect()
}

fn prune(project: &Path, label: &str, result: &mut SyncOutput) {
    if env::var("MX_SYSTEM_PRUNE").as_deref() == Ok("0") {
        return;
    }
    let occupied = worktree_branches(project);
    let current =
        git_text(project, &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_default();
    let refs = git_text(
        project,
        &[
            "for-each-ref",
            "--format=%(refname:short) %(upstream:track)",
            "refs/heads",
        ],
    )
    .unwrap_or_default();
    for line in refs.lines() {
        let Some((branch, track)) = line.split_once(' ') else {
            continue;
        };
        if track != "[gone]" || branch == current || occupied.iter().any(|value| value == branch) {
            continue;
        }
        if git_ok(project, &["branch", "-D", "--", branch]) {
            result.stdout.push(format!("{label}: pruned {branch}"));
        }
    }
}

fn is_ancestor(project: &Path, old: &str, new: &str) -> bool {
    git_ok(project, &["merge-base", "--is-ancestor", old, new])
}

fn branch_elsewhere(project: &Path, branch: &str) -> bool {
    worktree_branches(project)
        .iter()
        .any(|value| value == branch)
}

fn behind(project: &Path, base: &str) -> String {
    git_text(project, &["rev-list", "--count", &format!("HEAD..{base}")])
        .unwrap_or_else(|| "?".to_owned())
}

fn stuck(project: &Path, label: &str, state: &str, base: &str, result: &mut SyncOutput) {
    result.stdout.push(format!(
        "{label}: STUCK: on {state}, {} commits behind {base} - needs attention",
        behind(project, base)
    ));
}

fn project_label(project: &Path, projects: &Path) -> String {
    if project.parent() == Some(projects) || project.starts_with("projects") {
        project
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        project.display().to_string()
    }
}

fn sync_one(context: &SyncContext, project: &Path, result: &mut SyncOutput) {
    let label = project_label(project, &context.projects);
    if !project.is_dir() {
        result
            .stdout
            .push(format!("{label}: skipped: not a directory"));
        return;
    }
    if !git_ok(project, &["rev-parse", "--is-inside-work-tree"]) {
        result
            .stdout
            .push(format!("{label}: skipped: not a git repo"));
        return;
    }
    let registry = context.home.join("data/projects.md");
    if resolve_project_mode(&registry, &label).mode == DeliveryMode::LocalOnly {
        result
            .stdout
            .push(format!("{label}: skipped: local-only project"));
        return;
    }
    if !git_ok(project, &["remote", "get-url", "origin"]) {
        result
            .stdout
            .push(format!("{label}: skipped: no origin remote"));
        return;
    }
    if let Err(error) = fetch(project, &label, result) {
        let detail = first_line(&error);
        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        };
        result
            .stdout
            .push(format!("{label}: skipped: fetch failed{suffix}"));
        return;
    }
    prune(project, &label, result);
    let Some(default) = default_branch(project) else {
        result
            .stdout
            .push(format!("{label}: skipped: cannot determine default branch"));
        return;
    };
    let base = format!("origin/{default}");
    if !git_ok(
        project,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ],
    ) {
        result
            .stdout
            .push(format!("{label}: skipped: {base} does not exist"));
        return;
    }
    let current = git_text(project, &["symbolic-ref", "--short", "HEAD"]).unwrap_or_default();
    let dirty = git_text(project, &["status", "--porcelain"]).is_some_and(|text| !text.is_empty());
    let mut recovered = false;
    if current != default {
        let local_default_safe = !git_ok(
            project,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{default}^{{commit}}"),
            ],
        ) || is_ancestor(project, &default, &base);
        if current.is_empty()
            && !dirty
            && is_ancestor(project, "HEAD", &base)
            && !branch_elsewhere(project, &default)
            && local_default_safe
            && git_ok(project, &["checkout", "--quiet", &default])
        {
            recovered = true;
        } else {
            let mut state = if !current.is_empty() {
                format!("branch {current}")
            } else if !dirty && !is_ancestor(project, "HEAD", &base) {
                "detached HEAD with unique commits".to_owned()
            } else if branch_elsewhere(project, &default) {
                format!("detached HEAD ({default} checked out in another worktree)")
            } else if !local_default_safe {
                format!("detached HEAD (local {default} diverged from {base})")
            } else {
                "detached HEAD".to_owned()
            };
            if dirty {
                state.push_str(" with uncommitted changes");
            }
            stuck(project, &label, &state, &base, result);
            return;
        }
    } else if dirty {
        stuck(
            project,
            &label,
            &format!("branch {current} with uncommitted changes"),
            &base,
            result,
        );
        return;
    }
    if !git_ok(
        project,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{default}^{{commit}}"),
        ],
    ) {
        result
            .stdout
            .push(format!("{label}: skipped: local {default} does not exist"));
        return;
    }
    let Some(local) = git_text(project, &["rev-parse", &default]) else {
        result
            .stdout
            .push(format!("{label}: skipped: cannot read local {default}"));
        return;
    };
    let Some(remote) = git_text(project, &["rev-parse", &base]) else {
        result
            .stdout
            .push(format!("{label}: skipped: cannot read {base}"));
        return;
    };
    if local == remote {
        result.stdout.push(if recovered {
            format!("{label}: recovered: re-attached {default} (already current)")
        } else {
            format!("{label}: already current")
        });
        return;
    }
    if !is_ancestor(project, &default, &base) {
        stuck(
            project,
            &label,
            &format!("diverged {default}"),
            &base,
            result,
        );
        return;
    }
    let before = git_text(project, &["rev-parse", "--short", &default]).unwrap_or_default();
    let merge = git(project, &["merge", "--ff-only", &base]);
    if !merge.as_ref().is_ok_and(|output| output.status.success()) {
        let detail = merge
            .ok()
            .map(|output| first_line(&output.stderr))
            .unwrap_or_default();
        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        };
        result
            .stdout
            .push(format!("{label}: skipped: fast-forward failed{suffix}"));
        return;
    }
    let after = git_text(project, &["rev-parse", "--short", &default]).unwrap_or_default();
    result.stdout.push(if recovered {
        format!("{label}: recovered: re-attached {default}, synced {before}..{after}")
    } else {
        format!("{label}: synced {before}..{after}")
    });
}

fn resolve_arg(arg: &Path, projects: &Path) -> PathBuf {
    let text = arg.to_string_lossy();
    if let Some(name) = text.strip_prefix("projects/") {
        let candidate = projects.join(name);
        if candidate.is_dir() {
            return candidate;
        }
    } else if !text.contains('/') {
        let candidate = projects.join(arg);
        if candidate.is_dir() {
            return candidate;
        }
        if arg.is_dir() {
            return arg.to_path_buf();
        }
    } else if arg.is_dir() {
        return arg.to_path_buf();
    }
    arg.to_path_buf()
}

/// Refresh one project or every direct project clone.
pub fn run(context: &SyncContext, requested: Option<&Path>) -> SyncOutput {
    let mut result = SyncOutput::default();
    if let Some(requested) = requested {
        sync_one(
            context,
            &resolve_arg(requested, &context.projects),
            &mut result,
        );
        return result;
    }
    let Ok(entries) = fs::read_dir(&context.projects) else {
        return result;
    };
    let mut projects: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    projects.sort();
    for project in projects {
        sync_one(context, &project, &mut result);
    }
    result
}

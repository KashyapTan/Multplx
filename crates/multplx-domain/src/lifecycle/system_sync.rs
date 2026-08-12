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

fn retry_wait(raw: &str, result: &mut SyncOutput) -> f64 {
    raw.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or_else(|| {
            result.stderr.push(format!(
                "system-sync: invalid packed-refs lock retry wait '{raw}'; using 1s"
            ));
            1.0
        })
}

fn provably_stale(project: &Path, lock: &Path, age: u64) -> bool {
    git_lock_is_provably_stale(
        lock,
        Some(project),
        Duration::from_secs(age),
        SystemTime::now(),
        &LsofProbe,
    )
    .unwrap_or(false)
}

fn fetch(project: &Path, label: &str, result: &mut SyncOutput) -> Result<(), Vec<u8>> {
    let retries = env_u64("MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRIES", 3);
    let age = env_u64("MX_SYSTEM_SYNC_PACKED_REFS_LOCK_AGE_SECS", 30);
    let wait_raw = env::var("MX_SYSTEM_SYNC_PACKED_REFS_LOCK_RETRY_WAIT_SECS")
        .unwrap_or_else(|_| "1".to_owned());
    let wait = retry_wait(&wait_raw, result);
    let lock = packed_refs_lock(project);
    fetch_attempts(
        label,
        result,
        retries,
        age,
        &wait_raw,
        wait,
        lock,
        || git(project, &["fetch", "origin", "--prune", "--quiet"]),
        |lock| provably_stale(project, lock, age),
    )
}

#[allow(clippy::too_many_arguments)]
fn fetch_attempts<F, S>(
    label: &str,
    result: &mut SyncOutput,
    retries: u64,
    age: u64,
    wait_raw: &str,
    wait: f64,
    lock: Option<PathBuf>,
    mut run: F,
    is_stale: S,
) -> Result<(), Vec<u8>>
where
    F: FnMut() -> std::io::Result<Output>,
    S: Fn(&Path) -> bool,
{
    let mut output = run().map_err(|error| error.to_string().into_bytes())?;
    if output.status.success() {
        return Ok(());
    }
    if !packed_refs_signature(&output.stderr) {
        return Err(output.stderr);
    }
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
        let stale = is_stale(&lock);
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

fn merge_failure(label: &str, merge: std::io::Result<Output>) -> Option<String> {
    if merge.as_ref().is_ok_and(|output| output.status.success()) {
        return None;
    }
    let detail = merge
        .ok()
        .map(|output| first_line(&output.stderr))
        .unwrap_or_default();
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    };
    Some(format!("{label}: skipped: fast-forward failed{suffix}"))
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
    if let Some(line) = merge_failure(label.as_str(), git(project, &["merge", "--ff-only", &base]))
    {
        result.stdout.push(line);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::os::unix::process::ExitStatusExt;

    fn run_git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn commit(dir: &Path, message: &str) -> String {
        run_git(dir, &["add", "."]);
        run_git(
            dir,
            &[
                "-c",
                "user.name=Multplx Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                message,
                "--quiet",
            ],
        );
        run_git(dir, &["rev-parse", "HEAD"])
    }

    fn fixture(temp: &Path) -> (SyncContext, PathBuf, PathBuf) {
        let home = temp.join("home");
        let projects = home.join("projects");
        let seed = temp.join("seed");
        let remote = temp.join("remote.git");
        fs::create_dir_all(home.join("data")).expect("data");
        fs::create_dir_all(&projects).expect("projects");
        fs::create_dir(&seed).expect("seed");
        run_git(&seed, &["init", "-b", "main", "--quiet"]);
        fs::write(seed.join("README.md"), "one\n").expect("readme");
        commit(&seed, "base");
        let output = Command::new("git")
            .args(["clone", "--bare", "--quiet"])
            .arg(&seed)
            .arg(&remote)
            .output()
            .expect("bare clone");
        assert!(output.status.success());
        run_git(
            &seed,
            &["remote", "add", "origin", remote.to_str().expect("remote")],
        );
        let app = projects.join("app");
        let output = Command::new("git")
            .args(["clone", "--quiet"])
            .arg(&remote)
            .arg(&app)
            .output()
            .expect("app clone");
        assert!(output.status.success());
        (SyncContext { home, projects }, seed, app)
    }

    fn advance(seed: &Path, value: &str) -> String {
        fs::write(seed.join("README.md"), format!("{value}\n")).expect("advance");
        let revision = commit(seed, value);
        run_git(seed, &["push", "origin", "main", "--quiet"]);
        revision
    }

    #[test]
    fn syncs_current_and_recovers_safe_detached_projects() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (context, seed, app) = fixture(temp.path());
        let revision = advance(&seed, "two");
        let synced = run(&context, Some(Path::new("app")));
        assert!(
            synced
                .stdout
                .iter()
                .any(|line| line.contains("app: synced"))
        );
        assert_eq!(run_git(&app, &["rev-parse", "HEAD"]), revision);
        assert_eq!(
            run(&context, Some(Path::new("projects/app"))).stdout,
            vec!["app: already current"]
        );

        let old = run_git(&app, &["rev-parse", "HEAD"]);
        advance(&seed, "three");
        run_git(&app, &["checkout", "--detach", &old, "--quiet"]);
        let recovered = run(&context, Some(&app));
        assert!(
            recovered
                .stdout
                .iter()
                .any(|line| line.contains("recovered: re-attached main, synced"))
        );
        assert_eq!(run_git(&app, &["symbolic-ref", "--short", "HEAD"]), "main");
    }

    #[test]
    fn reports_non_destructive_skip_and_stuck_states() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (context, seed, app) = fixture(temp.path());
        assert_eq!(
            run(&context, Some(&temp.path().join("missing"))).stdout,
            vec![format!(
                "{}: skipped: not a directory",
                temp.path().join("missing").display()
            )]
        );

        let plain = context.projects.join("plain");
        fs::create_dir(&plain).expect("plain");
        assert_eq!(
            run(&context, Some(Path::new("plain"))).stdout,
            vec!["plain: skipped: not a git repo"]
        );

        fs::write(
            context.home.join("data/projects.md"),
            "- app [local-only] - app\n",
        )
        .expect("registry");
        assert_eq!(
            run(&context, Some(Path::new("app"))).stdout,
            vec!["app: skipped: local-only project"]
        );
        fs::write(
            context.home.join("data/projects.md"),
            "- app [deep-review] - app\n",
        )
        .expect("registry");

        advance(&seed, "two");
        fs::write(app.join("dirty"), "x").expect("dirty");
        assert!(
            run(&context, Some(Path::new("app"))).stdout[0].contains("with uncommitted changes")
        );
        fs::remove_file(app.join("dirty")).expect("clean");
        run_git(&app, &["checkout", "-b", "topic", "--quiet"]);
        assert!(run(&context, Some(Path::new("app"))).stdout[0].contains("STUCK: on branch topic"));

        let all = run(&context, None);
        assert!(
            all.stdout
                .iter()
                .any(|line| line.starts_with("app: STUCK:"))
        );
        assert!(
            all.stdout
                .iter()
                .any(|line| line == "plain: skipped: not a git repo")
        );
    }

    #[test]
    fn helper_and_git_edge_paths_remain_bounded_and_non_destructive() {
        assert_eq!(first_line(b"  first   line \nsecond"), "first line");
        assert_eq!(first_line(b""), "");
        assert!(packed_refs_signature(
            b"Unable to create '.git/packed-refs.lock': File exists"
        ));
        assert!(packed_refs_signature(
            b"Unable to create \".git/packed-refs.lock\": File exists"
        ));
        assert!(!packed_refs_signature(b"ordinary fetch failure"));
        assert_eq!(env_u64("MX_TEST_VARIABLE_THAT_IS_NOT_SET", 17), 17);

        let temp = tempfile::tempdir().expect("tempdir");
        let (context, seed, app) = fixture(temp.path());
        assert!(
            packed_refs_lock(&app)
                .expect("lock path")
                .ends_with("packed-refs.lock")
        );
        assert_eq!(default_branch(&app).as_deref(), Some("main"));

        let no_origin = context.projects.join("no-origin");
        fs::create_dir(&no_origin).expect("no origin");
        run_git(&no_origin, &["init", "-b", "main", "--quiet"]);
        fs::write(no_origin.join("file"), "x").expect("file");
        commit(&no_origin, "base");
        assert_eq!(default_branch(&no_origin).as_deref(), Some("main"));
        let mut fetch_result = SyncOutput::default();
        assert!(fetch(&no_origin, "no-origin", &mut fetch_result).is_err());
        assert_eq!(
            run(&context, Some(Path::new("no-origin"))).stdout,
            vec!["no-origin: skipped: no origin remote"]
        );

        run_git(&app, &["checkout", "-b", "stale", "--quiet"]);
        fs::write(app.join("stale"), "x").expect("stale");
        commit(&app, "stale");
        run_git(&app, &["push", "-u", "origin", "stale", "--quiet"]);
        run_git(&app, &["checkout", "main", "--quiet"]);
        run_git(&app, &["push", "origin", "--delete", "stale", "--quiet"]);
        let pruned = run(&context, Some(Path::new("app")));
        assert!(pruned.stdout.iter().any(|line| line == "app: pruned stale"));
        assert!(!git_ok(
            &app,
            &["show-ref", "--verify", "--quiet", "refs/heads/stale"]
        ));

        advance(&seed, "remote-change");
        fs::write(app.join("local"), "unique").expect("local");
        commit(&app, "local-change");
        let diverged = run(&context, Some(Path::new("app")));
        assert!(diverged.stdout[0].contains("STUCK: on diverged main"));

        let detached = context.projects.join("detached");
        let output = Command::new("git")
            .args(["clone", "--quiet"])
            .arg(temp.path().join("remote.git"))
            .arg(&detached)
            .output()
            .expect("detached clone");
        assert!(output.status.success());
        fs::write(detached.join("unique"), "x").expect("unique");
        let unique = commit(&detached, "unique detached");
        run_git(&detached, &["checkout", "--detach", &unique, "--quiet"]);
        assert!(
            run(&context, Some(Path::new("detached"))).stdout[0]
                .contains("detached HEAD with unique commits")
        );

        let bad_fetch = context.projects.join("bad-fetch");
        fs::create_dir(&bad_fetch).expect("bad fetch");
        run_git(&bad_fetch, &["init", "-b", "main", "--quiet"]);
        fs::write(bad_fetch.join("file"), "x").expect("bad file");
        commit(&bad_fetch, "base");
        run_git(
            &bad_fetch,
            &[
                "remote",
                "add",
                "origin",
                temp.path().join("missing-remote").to_str().expect("remote"),
            ],
        );
        assert!(
            run(&context, Some(Path::new("bad-fetch"))).stdout[0]
                .contains("skipped: fetch failed:")
        );

        let empty_remote = temp.path().join("empty.git");
        let output = Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(["init", "--bare", "--quiet"])
            .arg(&empty_remote)
            .output()
            .expect("empty remote");
        assert!(output.status.success());
        let missing_base = context.projects.join("missing-base");
        fs::create_dir(&missing_base).expect("missing base");
        run_git(&missing_base, &["init", "-b", "main", "--quiet"]);
        fs::write(missing_base.join("file"), "x").expect("file");
        commit(&missing_base, "base");
        run_git(
            &missing_base,
            &[
                "remote",
                "add",
                "origin",
                empty_remote.to_str().expect("empty remote"),
            ],
        );
        assert_eq!(
            run(&context, Some(Path::new("missing-base"))).stdout,
            vec!["missing-base: skipped: origin/main does not exist"]
        );
    }

    fn command_output(success: bool, stderr: &str) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(if success { 0 } else { 256 }),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    fn scripted(outputs: Vec<Output>) -> impl FnMut() -> std::io::Result<Output> {
        let mut outputs = VecDeque::from(outputs);
        move || Ok(outputs.pop_front().expect("scripted output"))
    }

    #[test]
    fn packed_refs_retry_recovery_covers_every_safe_outcome() {
        const LOCKED: &str = "Unable to create '.git/packed-refs.lock': File exists";
        let temp = tempfile::tempdir().expect("tempdir");

        let lock = temp.path().join("retry.lock");
        fs::write(&lock, "lock").expect("lock");
        let mut result = SyncOutput::default();
        fetch_attempts(
            "app",
            &mut result,
            2,
            30,
            "0",
            0.0,
            Some(lock),
            scripted(vec![
                command_output(false, LOCKED),
                command_output(true, ""),
            ]),
            |_| false,
        )
        .expect("retry success");
        assert!(result.stdout[0].contains("cleared on its own"));
        assert_eq!(result.stderr.len(), 2);

        let lock = temp.path().join("changed.lock");
        fs::write(&lock, "lock").expect("lock");
        let mut result = SyncOutput::default();
        let error = fetch_attempts(
            "app",
            &mut result,
            1,
            30,
            "0",
            0.0,
            Some(lock),
            scripted(vec![
                command_output(false, LOCKED),
                command_output(false, "network unavailable"),
            ]),
            |_| false,
        )
        .expect_err("changed failure");
        assert_eq!(first_line(&error), "network unavailable");

        let stale = temp.path().join("stale.lock");
        fs::write(&stale, "lock").expect("stale lock");
        let mut result = SyncOutput::default();
        fetch_attempts(
            "app",
            &mut result,
            0,
            30,
            "0",
            0.0,
            Some(stale.clone()),
            scripted(vec![
                command_output(false, LOCKED),
                command_output(true, ""),
            ]),
            |_| true,
        )
        .expect("stale recovery");
        assert!(!stale.exists());
        assert!(result.stdout[0].contains("removed a stale"));

        let live = temp.path().join("live.lock");
        fs::write(&live, "lock").expect("live lock");
        let mut result = SyncOutput::default();
        assert!(
            fetch_attempts(
                "app",
                &mut result,
                0,
                30,
                "0",
                0.0,
                Some(live),
                scripted(vec![command_output(false, LOCKED)]),
                |_| false,
            )
            .is_err()
        );
        assert!(result.stderr[0].contains("not provably stale"));

        let mut result = SyncOutput::default();
        assert!(
            fetch_attempts(
                "app",
                &mut result,
                0,
                30,
                "0",
                0.0,
                None,
                scripted(vec![command_output(false, LOCKED)]),
                |_| false,
            )
            .is_err()
        );
        assert!(result.stderr[0].contains("lock file disappeared"));

        let directory = temp.path().join("directory.lock");
        fs::create_dir(&directory).expect("lock directory");
        let mut result = SyncOutput::default();
        assert!(
            fetch_attempts(
                "app",
                &mut result,
                0,
                30,
                "0",
                0.0,
                Some(directory),
                scripted(vec![command_output(false, LOCKED)]),
                |_| true,
            )
            .is_err()
        );
        assert!(result.stderr[0].contains("failed to remove"));

        let stale_failure = temp.path().join("stale-failure.lock");
        fs::write(&stale_failure, "lock").expect("stale failure lock");
        let mut result = SyncOutput::default();
        assert!(
            fetch_attempts(
                "app",
                &mut result,
                0,
                30,
                "0",
                0.0,
                Some(stale_failure),
                scripted(vec![
                    command_output(false, LOCKED),
                    command_output(false, "still failed"),
                ]),
                |_| true,
            )
            .is_err()
        );

        let mut waits = SyncOutput::default();
        assert_eq!(retry_wait("0.25", &mut waits), 0.25);
        assert_eq!(retry_wait("invalid", &mut waits), 1.0);
        assert_eq!(retry_wait("NaN", &mut waits), 1.0);
        assert_eq!(waits.stderr.len(), 2);
        assert!(!provably_stale(
            temp.path(),
            &temp.path().join("missing.lock"),
            0
        ));

        assert_eq!(
            merge_failure("app", Ok(command_output(false, "merge refused\nmore"))),
            Some("app: skipped: fast-forward failed: merge refused".to_owned())
        );
        assert_eq!(
            merge_failure(
                "app",
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
            ),
            Some("app: skipped: fast-forward failed".to_owned())
        );
        assert_eq!(merge_failure("app", Ok(command_output(true, ""))), None);
    }

    #[test]
    fn detached_worktree_and_default_branch_edges_are_classified() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (context, seed, app) = fixture(temp.path());
        let head = run_git(&app, &["rev-parse", "HEAD"]);
        run_git(&app, &["checkout", "--detach", &head, "--quiet"]);
        assert_eq!(
            run(&context, Some(Path::new("app"))).stdout,
            vec!["app: recovered: re-attached main (already current)"]
        );

        run_git(&app, &["checkout", "--detach", &head, "--quiet"]);
        let worktree = temp.path().join("main-worktree");
        run_git(
            &app,
            &[
                "worktree",
                "add",
                "--quiet",
                worktree.to_str().expect("worktree"),
                "main",
            ],
        );
        assert!(
            run(&context, Some(Path::new("app"))).stdout[0]
                .contains("main checked out in another worktree")
        );
        run_git(
            &app,
            &[
                "worktree",
                "remove",
                "--force",
                worktree.to_str().expect("worktree"),
            ],
        );

        run_git(&app, &["checkout", "main", "--quiet"]);
        fs::write(app.join("local-only"), "x").expect("local");
        commit(&app, "local unique");
        advance(&seed, "remote unique");
        run_git(&app, &["checkout", "--detach", &head, "--quiet"]);
        assert!(
            run(&context, Some(Path::new("app"))).stdout[0]
                .contains("local main diverged from origin/main")
        );

        let dirty = context.projects.join("dirty-detached");
        let output = Command::new("git")
            .args(["clone", "--quiet"])
            .arg(temp.path().join("remote.git"))
            .arg(&dirty)
            .output()
            .expect("dirty clone");
        assert!(output.status.success());
        let dirty_head = run_git(&dirty, &["rev-parse", "HEAD"]);
        run_git(&dirty, &["checkout", "--detach", &dirty_head, "--quiet"]);
        fs::write(dirty.join("dirty"), "x").expect("dirty file");
        assert!(
            run(&context, Some(Path::new("dirty-detached"))).stdout[0]
                .contains("detached HEAD with uncommitted changes")
        );

        let odd = context.projects.join("odd");
        let output = Command::new("git")
            .args(["clone", "--quiet"])
            .arg(temp.path().join("remote.git"))
            .arg(&odd)
            .output()
            .expect("odd clone");
        assert!(output.status.success());
        run_git(
            &temp.path().join("remote.git"),
            &["symbolic-ref", "HEAD", "refs/heads/odd"],
        );
        run_git(&odd, &["remote", "set-head", "origin", "--delete"]);
        run_git(&odd, &["branch", "-m", "odd"]);
        let odd_result = run(&context, Some(Path::new("odd")));
        assert!(
            odd_result.stdout[0].contains("cannot determine default branch"),
            "{:?}",
            odd_result.stdout
        );

        assert_eq!(
            resolve_arg(Path::new("missing"), &context.projects),
            PathBuf::from("missing")
        );
        assert_eq!(
            resolve_arg(Path::new("projects/missing"), &context.projects),
            PathBuf::from("projects/missing")
        );
        assert!(
            run(
                &SyncContext {
                    home: context.home,
                    projects: temp.path().join("absent-projects")
                },
                None
            )
            .stdout
            .is_empty()
        );
    }
}

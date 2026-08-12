//! Guarded fast-forward mechanics shared by lifecycle commands.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Updated,
    Current,
    Skipped,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub status: Status,
    pub instructions: Vec<&'static str>,
    pub line: String,
}

#[derive(Clone, Debug)]
pub enum Base {
    Origin,
    Commit(String),
}

#[derive(Clone, Debug)]
pub struct Context {
    pub root: PathBuf,
    pub home: PathBuf,
    pub marker: String,
}

fn git(dir: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new("git").arg("-C").arg(dir).args(args).output()
}

fn ok(dir: &Path, args: &[&str]) -> bool {
    git(dir, args).is_ok_and(|output| output.status.success())
}

fn text(dir: &Path, args: &[&str]) -> Option<String> {
    let output = git(dir, args).ok()?;
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

pub fn default_branch(dir: &Path) -> Option<String> {
    if let Some(reference) = text(
        dir,
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
            ok(
                dir,
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

pub fn primary_head_commit(root: &Path) -> Option<String> {
    let branch = default_branch(root)?;
    text(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
    )
}

fn skipped(label: &str, reason: impl std::fmt::Display) -> Outcome {
    Outcome {
        status: Status::Skipped,
        instructions: Vec::new(),
        line: format!("{label}: skipped: {reason}"),
    }
}

pub fn fast_forward(
    dir: &Path,
    label: &str,
    base_mode: &Base,
    allow_detached: bool,
    ignore_seed_marker: bool,
) -> Outcome {
    if !dir.is_dir() {
        return skipped(label, "not a directory");
    }
    if !ok(dir, &["rev-parse", "--is-inside-work-tree"]) {
        return skipped(label, "not a git repo");
    }
    let Some(default) = default_branch(dir) else {
        return skipped(label, "cannot determine default branch");
    };
    let base = match base_mode {
        Base::Origin => {
            if !ok(dir, &["remote", "get-url", "origin"]) {
                return skipped(label, "no origin remote");
            }
            if !ok(dir, &["fetch", "origin", "--prune", "--quiet"]) {
                return skipped(label, "fetch failed");
            }
            format!("origin/{default}")
        }
        Base::Commit(commit) => commit.clone(),
    };
    if !ok(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ],
    ) {
        return skipped(label, format!("{base} does not exist"));
    }
    let current = text(dir, &["symbolic-ref", "--short", "HEAD"]).unwrap_or_default();
    if current.is_empty() && !allow_detached {
        return skipped(label, format!("detached HEAD, expected {default}"));
    }
    if !current.is_empty() && current != default {
        return skipped(label, format!("on {current}, expected {default}"));
    }
    let dirty = text(dir, &["status", "--porcelain"]).unwrap_or_default();
    let dirty = dirty
        .lines()
        .any(|line| !(ignore_seed_marker && line == "?? .mx-daemon-home"));
    if dirty {
        return skipped(label, "dirty working tree");
    }
    let Some(local) = text(dir, &["rev-parse", "HEAD"]) else {
        return skipped(label, "cannot read HEAD");
    };
    let Some(base_revision) = text(dir, &["rev-parse", &base]) else {
        return skipped(label, format!("cannot read {base}"));
    };
    if local == base_revision {
        return Outcome {
            status: Status::Current,
            instructions: Vec::new(),
            line: format!("{label}: already current"),
        };
    }
    if !ok(dir, &["merge-base", "--is-ancestor", "HEAD", &base]) {
        return skipped(label, format!("diverged from {base}"));
    }
    let instructions: Vec<&'static str> = ["AGENTS.md", "bin", ".agents/skills"]
        .into_iter()
        .filter(|path| !ok(dir, &["diff", "--quiet", "HEAD", &base, "--", path]))
        .collect();
    let before = text(dir, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let output = match git(dir, &["merge", "--ff-only", &base]) {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            return skipped(
                label,
                format!("fast-forward failed: {}", first_line(&output.stderr)),
            );
        }
        Err(error) => return skipped(label, format!("fast-forward failed: {error}")),
    };
    let _ = output;
    let after = text(dir, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let suffix = if instructions.is_empty() {
        String::new()
    } else {
        format!(" (instructions changed: {})", instructions.join(", "))
    };
    Outcome {
        status: Status::Updated,
        instructions,
        line: format!("{label}: updated {before}..{after}{suffix}"),
    }
}

fn is_strict_child(parent: &Path, child: &Path) -> bool {
    parent != child && child.starts_with(parent)
}

pub fn validate_daemon_home(context: &Context, id: &str, home: &Path) -> Result<PathBuf, String> {
    let home = fs::canonicalize(home).map_err(|_| "not a directory".to_owned())?;
    let active = fs::canonicalize(&context.home)
        .map_err(|_| "active Multplx home is not a directory".to_owned())?;
    let root = fs::canonicalize(&context.root)
        .map_err(|_| "Multplx repo is not a directory".to_owned())?;
    if home == Path::new("/") {
        return Err("daemon home cannot be the filesystem root".to_owned());
    }
    if home == active {
        return Err("daemon home cannot be the active Multplx home".to_owned());
    }
    if home == root {
        return Err("daemon home cannot be the Multplx repo".to_owned());
    }
    if is_strict_child(&active, &home) {
        return Err("daemon home cannot be inside the active Multplx home".to_owned());
    }
    if is_strict_child(&root, &home) {
        return Err("daemon home cannot be inside the Multplx repo".to_owned());
    }
    if is_strict_child(&home, &active) {
        return Err("daemon home cannot be an ancestor of the active Multplx home".to_owned());
    }
    if is_strict_child(&home, &root) {
        return Err("daemon home cannot be an ancestor of the Multplx repo".to_owned());
    }
    for name in ["data", "state", "config", "projects"] {
        let path = home.join(name);
        let resolved = if path.exists() {
            if !path.is_dir() {
                return Err(format!("daemon {name} path is not a directory"));
            }
            fs::canonicalize(&path)
                .map_err(|_| format!("daemon {name} directory cannot be resolved"))?
        } else if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(format!(
                "daemon {name} directory must resolve inside the daemon home"
            ));
        } else {
            path
        };
        if !is_strict_child(&home, &resolved) {
            return Err(format!(
                "daemon {name} directory must resolve inside the daemon home"
            ));
        }
        if resolved == active || is_strict_child(&active, &resolved) {
            return Err(format!(
                "daemon {name} directory cannot be inside the active Multplx home"
            ));
        }
        if resolved == root || is_strict_child(&root, &resolved) {
            return Err(format!(
                "daemon {name} directory cannot be inside the Multplx repo"
            ));
        }
    }
    let marker = home.join(&context.marker);
    let marker_meta =
        fs::symlink_metadata(&marker).map_err(|_| "not a seeded daemon home".to_owned())?;
    if marker_meta.file_type().is_symlink() {
        return Err("daemon marker must not be a symlink".to_owned());
    }
    if !marker_meta.is_file() {
        return Err("not a seeded daemon home".to_owned());
    }
    let marker_id = fs::read_to_string(&marker).unwrap_or_default();
    if marker_id.trim_end_matches(['\n', '\r']) != id {
        let value = marker_id.trim();
        return Err(format!(
            "marked for daemon {}, expected {id}",
            if value.is_empty() { "unknown" } else { value }
        ));
    }
    if !home.join("AGENTS.md").is_file() {
        return Err("not a Multplx home (missing AGENTS.md)".to_owned());
    }
    if !home.join("bin").is_dir() {
        return Err("not a Multplx home (missing bin/)".to_owned());
    }
    Ok(home)
}

fn meta_value(path: &Path, key: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix(&format!("{key}=")))
        .next_back()
        .unwrap_or_default()
        .to_owned()
}

fn registry_fields(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("- ")?;
    let (id, _) = rest.split_once(' ')?;
    let home_start = line.find("(home:")? + "(home:".len();
    let after = &line[home_start..];
    let end = after.find(';')?;
    Some((id.to_owned(), after[..end].trim().to_owned()))
}

/// Update the primary checkout and all registered daemon homes from origin.
pub fn update(context: &Context, state: &Path, registry: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    let broker = fast_forward(&context.root, "broker", &Base::Origin, false, false);
    let reread = broker.status == Status::Updated && !broker.instructions.is_empty();
    lines.push(broker.line);
    let mut seen = HashSet::new();
    let mut nudges = Vec::new();
    let mut records = Vec::<(String, String, String)>::new();
    if let Ok(entries) = fs::read_dir(state) {
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("meta")
                || meta_value(&path, "kind") != "daemon"
            {
                continue;
            }
            let id = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let mut home = meta_value(&path, "home");
            if home.is_empty()
                && let Ok(text) = fs::read_to_string(registry)
            {
                home = text
                    .lines()
                    .filter_map(registry_fields)
                    .find_map(|(candidate, home)| (candidate == id).then_some(home))
                    .unwrap_or_default();
            }
            records.push((id, home, meta_value(&path, "window")));
        }
    }
    if let Ok(text) = fs::read_to_string(registry) {
        records.extend(
            text.lines()
                .filter_map(registry_fields)
                .map(|(id, home)| (id, home, String::new())),
        );
    }
    for (id, home, window) in records {
        if id.is_empty() || home.is_empty() {
            continue;
        }
        let raw = PathBuf::from(&home);
        let resolved = fs::canonicalize(&raw).unwrap_or(raw.clone());
        let root = fs::canonicalize(&context.root).unwrap_or_else(|_| context.root.clone());
        if resolved == root || !seen.insert(resolved.clone()) {
            continue;
        }
        let resolved = match validate_daemon_home(context, &id, &raw) {
            Ok(home) => home,
            Err(error) => {
                lines.push(format!("daemon {id}: skipped: unsafe home: {error}"));
                continue;
            }
        };
        let outcome = fast_forward(
            &resolved,
            &format!("daemon {id}"),
            &Base::Origin,
            true,
            true,
        );
        if outcome.status == Status::Updated && !window.is_empty() {
            nudges.push(format!("mx-{id}"));
        }
        lines.push(outcome.line);
    }
    lines.push(format!(
        "reread-broker: {}",
        if reread { "yes" } else { "no" }
    ));
    lines.push(format!(
        "nudge-daemons: {}",
        if nudges.is_empty() {
            "none".to_owned()
        } else {
            nudges.join(" ")
        }
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn fast_forward_covers_updates_current_and_refusals() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).expect("repo");
        run_git(&repo, &["init", "-b", "main", "--quiet"]);
        fs::write(repo.join("README.md"), "one\n").expect("readme");
        let base = commit(&repo, "base");
        fs::write(repo.join("AGENTS.md"), "instructions\n").expect("agents");
        let next = commit(&repo, "next");
        run_git(&repo, &["reset", "--hard", &base, "--quiet"]);

        assert_eq!(default_branch(&repo).as_deref(), Some("main"));
        assert_eq!(primary_head_commit(&repo).as_deref(), Some(base.as_str()));
        let updated = fast_forward(&repo, "repo", &Base::Commit(next.clone()), false, false);
        assert_eq!(updated.status, Status::Updated);
        assert_eq!(updated.instructions, vec!["AGENTS.md"]);
        assert!(updated.line.contains("instructions changed"));
        assert_eq!(
            fast_forward(&repo, "repo", &Base::Commit(next.clone()), false, false).status,
            Status::Current
        );

        fs::write(repo.join("dirty"), "x").expect("dirty");
        assert!(
            fast_forward(&repo, "repo", &Base::Commit(next.clone()), false, false)
                .line
                .contains("dirty working tree")
        );
        fs::remove_file(repo.join("dirty")).expect("clean");
        run_git(&repo, &["checkout", "-b", "topic", "--quiet"]);
        assert!(
            fast_forward(&repo, "repo", &Base::Commit(next.clone()), false, false)
                .line
                .contains("on topic")
        );
        run_git(&repo, &["checkout", "main", "--quiet"]);
        run_git(&repo, &["checkout", "--detach", &base, "--quiet"]);
        assert!(
            fast_forward(&repo, "repo", &Base::Commit(next.clone()), false, false)
                .line
                .contains("detached HEAD")
        );
        assert_eq!(
            fast_forward(&repo, "repo", &Base::Commit(next.clone()), true, false).status,
            Status::Updated
        );

        run_git(&repo, &["checkout", "main", "--quiet"]);
        fs::write(repo.join("README.md"), "three\n").expect("third");
        let third = commit(&repo, "third");
        run_git(&repo, &["reset", "--hard", &next, "--quiet"]);
        let plain = fast_forward(&repo, "repo", &Base::Commit(third), false, false);
        assert_eq!(plain.status, Status::Updated);
        assert!(plain.instructions.is_empty());
        fs::write(repo.join("local"), "unique").expect("local");
        commit(&repo, "local unique");
        assert!(
            fast_forward(&repo, "repo", &Base::Commit(base), false, false)
                .line
                .contains("diverged")
        );

        assert!(
            fast_forward(
                &temp.path().join("missing"),
                "x",
                &Base::Origin,
                false,
                false
            )
            .line
            .contains("not a directory")
        );
        assert!(
            fast_forward(temp.path(), "x", &Base::Origin, false, false)
                .line
                .contains("not a git repo")
        );
        assert!(
            fast_forward(&repo, "x", &Base::Commit("missing".to_owned()), true, false)
                .line
                .contains("does not exist")
        );
    }

    #[test]
    fn validates_seeded_daemon_homes_and_rejects_unsafe_ones() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let active = temp.path().join("active");
        let daemon = temp.path().join("daemon");
        for dir in [&root, &active, &daemon] {
            fs::create_dir(dir).expect("dir");
        }
        fs::create_dir(daemon.join("bin")).expect("bin");
        fs::write(daemon.join("AGENTS.md"), "x").expect("agents");
        fs::write(daemon.join(".mx-daemon-home"), "helper\n").expect("marker");
        let context = Context {
            root: root.clone(),
            home: active.clone(),
            marker: ".mx-daemon-home".to_owned(),
        };
        assert_eq!(
            validate_daemon_home(&context, "helper", &daemon).expect("valid"),
            fs::canonicalize(&daemon).expect("canonical")
        );
        assert!(
            validate_daemon_home(&context, "helper", &active)
                .unwrap_err()
                .contains("active")
        );
        assert!(
            validate_daemon_home(&context, "helper", &root)
                .unwrap_err()
                .contains("repo")
        );
        assert!(
            validate_daemon_home(&context, "other", &daemon)
                .unwrap_err()
                .contains("marked for daemon helper")
        );
        fs::remove_file(daemon.join(".mx-daemon-home")).expect("remove marker");
        assert!(
            validate_daemon_home(&context, "helper", &daemon)
                .unwrap_err()
                .contains("not a seeded")
        );

        let nested = active.join("nested");
        fs::create_dir(&nested).expect("nested");
        assert!(
            validate_daemon_home(&context, "helper", &nested)
                .unwrap_err()
                .contains("inside the active")
        );
    }

    #[test]
    fn update_reports_broker_and_unsafe_registry_records() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir(&root).expect("root");
        fs::create_dir_all(&state).expect("state");
        fs::write(
            state.join("helper.meta"),
            format!("kind=daemon\nhome={}\nwindow=1\n", root.display()),
        )
        .expect("meta");
        fs::write(state.join("fallback.meta"), "kind=daemon\nhome=\n").expect("fallback meta");
        fs::write(state.join("empty.meta"), "kind=daemon\nhome=\n").expect("empty meta");
        fs::write(state.join("actor.meta"), "kind=actor\n").expect("actor meta");
        let context = Context {
            root,
            home,
            marker: ".mx-daemon-home".to_owned(),
        };
        let unsafe_home = temp.path().join("unsafe-home");
        fs::create_dir(&unsafe_home).expect("unsafe home");
        let registry = temp.path().join("daemons.md");
        fs::write(
            &registry,
            format!(
                "- fallback (home: {}; state: idle)\n- registry (home: {}; state: idle)\n- ignored malformed\n",
                unsafe_home.display(),
                unsafe_home.display(),
            ),
        )
        .expect("registry");
        let lines = update(&context, &state, &registry);
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("broker: skipped:"))
        );
        assert!(lines.iter().any(|line| line == "reread-broker: no"));
        assert!(lines.iter().any(|line| line == "nudge-daemons: none"));
        assert!(lines.iter().any(|line| {
            line.starts_with("daemon fallback: skipped: unsafe home: not a seeded daemon home")
        }));
    }

    #[test]
    fn origin_updates_and_registered_daemon_refreshes_are_reported() {
        assert_eq!(first_line(b"  fatal:   failed \nmore"), "fatal: failed");
        let temp = tempfile::tempdir().expect("tempdir");
        let seed = temp.path().join("seed");
        let remote = temp.path().join("remote.git");
        fs::create_dir(&seed).expect("seed");
        run_git(&seed, &["init", "-b", "main", "--quiet"]);
        fs::create_dir(seed.join("bin")).expect("bin");
        fs::write(seed.join("AGENTS.md"), "one\n").expect("agents");
        fs::write(seed.join("bin/tool"), "one\n").expect("tool");
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

        let broker = temp.path().join("broker");
        let daemon = temp.path().join("daemon");
        for destination in [&broker, &daemon] {
            let output = Command::new("git")
                .args(["clone", "--quiet"])
                .arg(&remote)
                .arg(destination)
                .output()
                .expect("clone");
            assert!(output.status.success());
        }
        fs::write(daemon.join(".mx-daemon-home"), "helper\n").expect("marker");
        let active = temp.path().join("active");
        let state = active.join("state");
        fs::create_dir_all(&state).expect("state");
        fs::write(
            state.join("helper.meta"),
            format!("kind=daemon\nhome={}\nwindow=7\n", daemon.display()),
        )
        .expect("meta");
        let context = Context {
            root: broker.clone(),
            home: active,
            marker: ".mx-daemon-home".to_owned(),
        };

        assert_eq!(
            fast_forward(&broker, "broker", &Base::Origin, false, false).status,
            Status::Current
        );
        fs::write(seed.join("AGENTS.md"), "two\n").expect("agents two");
        fs::write(seed.join("bin/tool"), "two\n").expect("tool two");
        commit(&seed, "advance");
        run_git(&seed, &["push", "origin", "main", "--quiet"]);
        let lines = update(&context, &state, &temp.path().join("missing-registry"));
        assert!(lines.iter().any(|line| line.starts_with("broker: updated")));
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("daemon helper: updated"))
        );
        assert!(lines.iter().any(|line| line == "reread-broker: yes"));
        assert!(lines.iter().any(|line| line == "nudge-daemons: mx-helper"));

        let no_origin = temp.path().join("no-origin");
        fs::create_dir(&no_origin).expect("no origin");
        run_git(&no_origin, &["init", "-b", "main", "--quiet"]);
        fs::write(no_origin.join("file"), "x").expect("file");
        commit(&no_origin, "base");
        assert!(
            fast_forward(&no_origin, "plain", &Base::Origin, false, false)
                .line
                .contains("no origin remote")
        );
        run_git(&no_origin, &["branch", "-m", "odd"]);
        assert!(
            fast_forward(&no_origin, "plain", &Base::Origin, false, false)
                .line
                .contains("cannot determine default branch")
        );
        run_git(&no_origin, &["branch", "-m", "main"]);
        run_git(
            &no_origin,
            &[
                "remote",
                "add",
                "origin",
                temp.path().join("missing-origin").to_str().expect("origin"),
            ],
        );
        assert!(
            fast_forward(&no_origin, "plain", &Base::Origin, false, false)
                .line
                .contains("fetch failed")
        );
    }

    #[test]
    fn daemon_home_validation_covers_containment_and_shape_failures() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("scope/root");
        let active = temp.path().join("scope/active");
        let daemon = temp.path().join("daemon");
        for dir in [&root, &active, &daemon] {
            fs::create_dir_all(dir).expect("dir");
        }
        let context = Context {
            root: root.clone(),
            home: active.clone(),
            marker: ".mx-daemon-home".to_owned(),
        };
        assert!(
            validate_daemon_home(&context, "id", temp.path().join("scope").as_path())
                .unwrap_err()
                .contains("ancestor")
        );
        assert!(
            validate_daemon_home(&context, "id", Path::new("/"))
                .unwrap_err()
                .contains("filesystem root")
        );
        let outside_active = temp.path().join("outside-active");
        fs::create_dir(&outside_active).expect("outside active");
        let root_ancestor_context = Context {
            root: root.clone(),
            home: outside_active,
            marker: ".mx-daemon-home".to_owned(),
        };
        assert!(
            validate_daemon_home(
                &root_ancestor_context,
                "id",
                temp.path().join("scope").as_path()
            )
            .unwrap_err()
            .contains("ancestor of the Multplx repo")
        );
        let inside_root = root.join("daemon");
        fs::create_dir(&inside_root).expect("inside root");
        assert!(
            validate_daemon_home(&context, "id", &inside_root)
                .unwrap_err()
                .contains("inside the Multplx repo")
        );

        fs::write(daemon.join(".mx-daemon-home"), "id\n").expect("marker");
        assert!(
            validate_daemon_home(&context, "id", &daemon)
                .unwrap_err()
                .contains("missing AGENTS")
        );
        fs::write(daemon.join("AGENTS.md"), "x").expect("agents");
        assert!(
            validate_daemon_home(&context, "id", &daemon)
                .unwrap_err()
                .contains("missing bin")
        );
        fs::create_dir(daemon.join("bin")).expect("bin");

        fs::write(daemon.join("data"), "not a directory").expect("data file");
        assert!(
            validate_daemon_home(&context, "id", &daemon)
                .unwrap_err()
                .contains("data path is not a directory")
        );
        fs::remove_file(daemon.join("data")).expect("remove data");
        #[cfg(unix)]
        std::os::unix::fs::symlink("missing", daemon.join("data")).expect("dangling data link");
        assert!(
            validate_daemon_home(&context, "id", &daemon)
                .unwrap_err()
                .contains("resolve inside the daemon home")
        );
        fs::remove_file(daemon.join("data")).expect("remove dangling link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&active, daemon.join("data")).expect("data link");
        assert!(
            validate_daemon_home(&context, "id", &daemon)
                .unwrap_err()
                .contains("resolve inside the daemon home")
        );
        fs::remove_file(daemon.join("data")).expect("remove link");

        fs::remove_file(daemon.join(".mx-daemon-home")).expect("remove marker");
        #[cfg(unix)]
        std::os::unix::fs::symlink("AGENTS.md", daemon.join(".mx-daemon-home"))
            .expect("marker link");
        assert!(
            validate_daemon_home(&context, "id", &daemon)
                .unwrap_err()
                .contains("marker must not be a symlink")
        );
        fs::remove_file(daemon.join(".mx-daemon-home")).expect("remove marker link");
        fs::create_dir(daemon.join(".mx-daemon-home")).expect("marker dir");
        assert!(
            validate_daemon_home(&context, "id", &daemon)
                .unwrap_err()
                .contains("not a seeded")
        );
    }
}

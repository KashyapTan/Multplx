//! Read-only upstream review reports and guarded cursor advancement.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use multplx_core::filesystem::atomic_replace;
use regex::Regex;
use tempfile::TempDir;
use time::OffsetDateTime;

pub const HELP: &str = "Fetch, classify, and report upstream changes without modifying the Multplx tree.\n\nUsage:\n  mx-upstream-diff.sh --out <dir> [--since <sha>]\n  mx-upstream-diff.sh --record-reviewed <sha-or-head-sha-file>\n  mx-upstream-diff.sh --status\n";

#[derive(Clone)]
struct Record {
    repo: String,
    fork: String,
    reviewed: String,
    status: String,
    retired_reason: String,
    mappings: Vec<(String, Class)>,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Relevant,
    Irrelevant,
    Deleted,
    Flag,
}

pub struct Output {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

fn fail(message: impl Into<String>) -> Output {
    Output {
        status: 1,
        stdout: String::new(),
        stderr: format!("mx-upstream-diff: {}\n", message.into()),
    }
}

fn value(text: &str, key: &str) -> Option<String> {
    let mut header = false;
    for (index, line) in text.lines().enumerate() {
        if index == 0 && line == "---" {
            header = true;
            continue;
        }
        if header && line == "---" {
            break;
        }
        if header && line.starts_with(&format!("{key}:")) {
            return Some(line[key.len() + 1..].trim().to_owned());
        }
    }
    None
}

fn load_record(path: &Path) -> Result<Record, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| format!("record is missing or unsafe: {}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("record is missing or unsafe: {}", path.display()));
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let required = |key: &str| value(&text, key).ok_or_else(|| format!("record is missing {key}"));
    let repo = required("upstream_repo")?;
    let fork = required("fork_point")?;
    let reviewed = required("last_reviewed")?;
    let status = required("status")?;
    let retired_reason = required("retired_reason")?;
    let sha = Regex::new(r"^[0-9a-f]{40}$").expect("sha regex");
    if !sha.is_match(&fork) {
        return Err("record contains an invalid fork_point".to_owned());
    }
    if !sha.is_match(&reviewed) {
        return Err("record contains an invalid last_reviewed".to_owned());
    }
    match status.as_str() {
        "active" => {}
        "retired" if retired_reason.is_empty() => {
            return Err("retired record requires retired_reason".to_owned());
        }
        "retired" => {}
        _ => return Err("record status must be active or retired".to_owned()),
    }
    let mut mappings = Vec::new();
    let mut in_map = false;
    let mut ended = false;
    for line in text.lines() {
        if line == "<!-- mx-upstream-map:start -->" {
            in_map = true;
            continue;
        }
        if line == "<!-- mx-upstream-map:end -->" {
            ended = true;
            in_map = false;
            continue;
        }
        if !in_map || !line.starts_with('|') {
            continue;
        }
        let cells = line.split('|').map(str::trim).collect::<Vec<_>>();
        if cells.len() < 4 {
            continue;
        }
        let pattern = cells[1].trim_matches('`');
        let raw_class = cells[2];
        if pattern == "Upstream path glob" || pattern.chars().all(|ch| ch == '-') {
            continue;
        }
        if pattern.is_empty() || pattern.chars().any(char::is_whitespace) {
            return Err(format!("invalid relevance path glob: {pattern}"));
        }
        let class = match raw_class {
            "relevant" => Class::Relevant,
            "irrelevant" => Class::Irrelevant,
            "deleted" => Class::Deleted,
            "flag" => Class::Flag,
            _ => {
                return Err(format!(
                    "invalid relevance class for {pattern}: {raw_class}"
                ));
            }
        };
        mappings.push((pattern.to_owned(), class));
    }
    if !ended || mappings.is_empty() {
        return Err("relevance map markers or rows are missing".to_owned());
    }
    Ok(Record {
        repo,
        fork,
        reviewed,
        status,
        retired_reason,
        mappings,
        text,
    })
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn prepare_clone(record: &Record, clone: &Path) -> Result<(), String> {
    if fs::symlink_metadata(clone).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(format!(
            "scratch clone must not be a symlink: {}",
            clone.display()
        ));
    }
    if clone.join(".git").is_dir() {
        if git(clone, &["remote", "get-url", "upstream"])?.trim() != record.repo {
            return Err("scratch clone remote does not match upstream_repo".to_owned());
        }
    } else if clone.exists() {
        return Err(format!(
            "scratch clone path exists but is not a git clone: {}",
            clone.display()
        ));
    } else {
        let parent = clone.parent().ok_or("scratch clone has no parent")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let name = clone.file_name().ok_or("scratch clone has no name")?;
        let output = Command::new("git")
            .current_dir(parent)
            .args(["clone", "--quiet", "--no-checkout", "--origin", "upstream"])
            .arg(&record.repo)
            .arg(name)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err("cannot clone upstream repository".to_owned());
        }
    }
    git(
        clone,
        &["remote", "set-url", "--push", "upstream", "/dev/null"],
    )
    .map_err(|_| "cannot disable the upstream push URL".to_owned())?;
    git(
        clone,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "upstream",
            "+refs/heads/*:refs/remotes/upstream/*",
        ],
    )
    .map_err(|_| "cannot fetch upstream repository".to_owned())?;
    Ok(())
}

fn upstream_head(clone: &Path) -> Result<String, String> {
    if let Ok(symbolic) = git(clone, &["symbolic-ref", "-q", "refs/remotes/upstream/HEAD"]) {
        let symbolic = symbolic.trim();
        if !symbolic.is_empty() {
            return Ok(
                git(clone, &["rev-parse", &format!("{symbolic}^{{commit}}")])?
                    .trim()
                    .to_owned(),
            );
        }
    }
    for candidate in ["refs/remotes/upstream/main", "refs/remotes/upstream/master"] {
        if git(clone, &["show-ref", "--verify", candidate]).is_ok() {
            return Ok(
                git(clone, &["rev-parse", &format!("{candidate}^{{commit}}")])?
                    .trim()
                    .to_owned(),
            );
        }
    }
    Err("cannot resolve upstream default branch".to_owned())
}

fn commit(clone: &Path, sha: &str, label: &str) -> Result<(), String> {
    git(clone, &["cat-file", "-e", &format!("{sha}^{{commit}}")])
        .map(|_| ())
        .map_err(|_| format!("{label} is not a fetched upstream commit: {sha}"))
}

fn ancestor(clone: &Path, older: &str, newer: &str, message: &str) -> Result<(), String> {
    git(clone, &["merge-base", "--is-ancestor", older, newer])
        .map(|_| ())
        .map_err(|_| message.to_owned())
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }
    let mut offset = 0;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(found) = value[offset..].find(part) else {
            return false;
        };
        if index == 0 && !pattern.starts_with('*') && found != 0 {
            return false;
        }
        offset += found + part.len();
    }
    pattern.ends_with('*') || parts.last().is_some_and(|last| value.ends_with(last))
}

fn classify(record: &Record, path: &str) -> Class {
    record
        .mappings
        .iter()
        .find(|(pattern, _)| glob_matches(pattern, path))
        .map_or(Class::Flag, |(_, class)| *class)
}

fn class_name(class: Class) -> &'static str {
    match class {
        Class::Relevant => "relevant",
        Class::Irrelevant => "irrelevant",
        Class::Deleted => "deleted",
        Class::Flag => "flag",
    }
}

fn report(record: &Record, output: &Path, since: Option<&str>) -> Result<String, String> {
    if fs::symlink_metadata(output).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(format!(
            "output directory must not be a symlink: {}",
            output.display()
        ));
    }
    fs::create_dir_all(output).map_err(|error| error.to_string())?;
    let output = fs::canonicalize(output).map_err(|error| error.to_string())?;
    let clone = output.join(".upstream");
    prepare_clone(record, &clone)?;
    let head = upstream_head(&clone)?;
    let from = since.unwrap_or(&record.reviewed);
    let from = if from.is_empty() { &record.fork } else { from };
    commit(&clone, &record.fork, "fork_point")?;
    commit(&clone, from, "since")?;
    ancestor(
        &clone,
        &record.fork,
        from,
        "since commit is older than or unrelated to the fork point",
    )?;
    ancestor(
        &clone,
        from,
        &head,
        "since commit is not an ancestor of upstream HEAD",
    )?;
    let commits = git(
        &clone,
        &["rev-list", "--reverse", &format!("{from}..{head}")],
    )?;
    let mut relevant = String::new();
    let mut flagged = String::new();
    let mut skipped = String::new();
    let mut needs = BTreeSet::new();
    let mut relevant_count = 0;
    let mut flagged_count = 0;
    let mut skipped_count = 0;
    let mut commit_count = 0;
    for sha in commits.lines().filter(|line| !line.is_empty()) {
        commit_count += 1;
        let paths_raw = git(
            &clone,
            &[
                "diff-tree",
                "--root",
                "--no-commit-id",
                "--name-only",
                "-r",
                sha,
            ],
        )?;
        let mut category = Class::Irrelevant;
        let mut paths = Vec::new();
        for path in paths_raw.lines().filter(|line| !line.is_empty()) {
            let class = classify(record, path);
            paths.push(format!("{path} ({})", class_name(class)));
            if class == Class::Relevant {
                category = Class::Relevant;
            } else if class == Class::Flag && category != Class::Relevant {
                category = Class::Flag;
                needs.insert(path.to_owned());
            }
        }
        let short = git(&clone, &["rev-parse", "--short=12", sha])?
            .trim()
            .to_owned();
        let subject = git(&clone, &["log", "-1", "--format=%s", sha])?
            .trim()
            .to_owned();
        let heading = format!(
            "### `{short}` {subject}\n\n- Paths: {}\n\n",
            paths.join(", ")
        );
        if category == Class::Relevant || category == Class::Flag {
            let show = git(
                &clone,
                &[
                    "--no-pager",
                    "show",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    "--format=fuller",
                    "--stat",
                    "--patch",
                    sha,
                ],
            )?;
            let section =
                format!("{heading}#### Change metadata and diff\n\n```diff\n{show}```\n\n");
            if category == Class::Relevant {
                relevant_count += 1;
                relevant.push_str(&section);
            } else {
                flagged_count += 1;
                flagged.push_str(&section);
            }
        } else {
            skipped_count += 1;
            skipped.push_str(&format!("- `{short}` {subject} - {}\n", paths.join(", ")));
        }
    }
    let render = |body: &str| {
        if body.is_empty() {
            "None.\n\n".to_owned()
        } else {
            body.to_owned()
        }
    };
    let needs_text = if needs.is_empty() {
        "None.\n\n".to_owned()
    } else {
        format!(
            "{}\n",
            needs
                .iter()
                .map(|path| format!("- `{path}`\n"))
                .collect::<String>()
        )
    };
    let report = format!(
        "# Upstream review input\n\n- Upstream repository: {}\n- Diff range: `{}..{}`\n- Upstream HEAD: `{}`\n- Commits: {}\n- Relevant commits: {}\n- Flagged commits: {}\n- Mechanically skipped commits: {}\n- Paths needing mapping: {}\n\n## Relevant changes\n\n{}## Flagged changes\n\n{}## Paths needing mapping\n\n{}## Mechanically skipped\n\n{}",
        record.repo,
        from,
        head,
        head,
        commit_count,
        relevant_count,
        flagged_count,
        skipped_count,
        needs.len(),
        render(&relevant),
        render(&flagged),
        needs_text,
        if skipped.is_empty() {
            "None.\n"
        } else {
            &skipped
        }
    );
    fs::write(output.join("report-input.md"), report).map_err(|error| error.to_string())?;
    fs::write(output.join("head-sha"), format!("{head}\n")).map_err(|error| error.to_string())?;
    Ok(format!(
        "report={}\nhead={}\nrange={}..{}\n",
        output.join("report-input.md").display(),
        head,
        from,
        head
    ))
}

struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("pid"));
        let _ = fs::remove_dir(&self.0);
    }
}

fn acquire_lock(record_path: &Path) -> Result<Lock, String> {
    let lock = record_path
        .parent()
        .ok_or("record has no parent")?
        .join(".upstream-record.lock");
    for attempt in 0..3 {
        if fs::create_dir(&lock).is_ok() {
            fs::write(lock.join("pid"), format!("{}\n", std::process::id()))
                .map_err(|_| "cannot claim record lock")?;
            return Ok(Lock(lock));
        }
        if !lock.is_dir()
            || fs::symlink_metadata(&lock).is_ok_and(|meta| meta.file_type().is_symlink())
        {
            return Err("record lock path is unsafe".to_owned());
        }
        let pid = fs::read_to_string(lock.join("pid"))
            .ok()
            .map(|text| text.trim().to_owned());
        if let Some(pid) = pid.as_deref().filter(|pid| pid.parse::<u32>().is_ok()) {
            if Command::new("/bin/kill")
                .args(["-0", pid])
                .status()
                .is_ok_and(|status| status.success())
            {
                return Err(format!("record update is already running as pid {pid}"));
            }
        } else {
            let young = fs::metadata(&lock)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age.as_secs() < 2);
            if young {
                return Err("record update lock is still being claimed".to_owned());
            }
        }
        let stale = lock.with_file_name(format!(
            ".upstream-record.lock.stale.{}.{}",
            std::process::id(),
            attempt
        ));
        if fs::rename(&lock, &stale).is_ok() {
            let _ = fs::remove_file(stale.join("pid"));
            fs::remove_dir(&stale)
                .map_err(|_| "stale record lock contains unexpected files".to_owned())?;
        }
    }
    Err("cannot acquire record update lock".to_owned())
}

fn record_reviewed(record_path: &Path, record: &Record, requested: &str) -> Result<String, String> {
    let _lock = acquire_lock(record_path)?;
    let (target, evidence) = if Path::new(requested).is_file()
        && !fs::symlink_metadata(requested).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        let text = fs::read_to_string(requested).map_err(|error| error.to_string())?;
        if text.lines().count() != 1 {
            return Err("reviewed SHA file must contain exactly one line".to_owned());
        }
        (
            text.trim().to_owned(),
            Path::new(requested).parent().map(Path::to_path_buf),
        )
    } else {
        (requested.to_owned(), None)
    };
    if !Regex::new(r"^[0-9a-f]{40}$")
        .expect("sha regex")
        .is_match(&target)
    {
        return Err(format!("invalid reviewed commit id: {target}"));
    }
    let temporary;
    let clone = if let Some(candidate) = evidence
        .as_ref()
        .map(|path| path.join(".upstream"))
        .filter(|path| path.join(".git").is_dir())
    {
        candidate
    } else {
        temporary = TempDir::new().map_err(|error| error.to_string())?;
        temporary.path().join(".upstream")
    };
    prepare_clone(record, &clone)?;
    let head = upstream_head(&clone)?;
    commit(&clone, &record.fork, "fork_point")?;
    commit(&clone, &record.reviewed, "last_reviewed")?;
    commit(&clone, &target, "reviewed")?;
    ancestor(
        &clone,
        &record.fork,
        &record.reviewed,
        "last_reviewed is older than or unrelated to the fork point",
    )?;
    ancestor(
        &clone,
        &record.reviewed,
        &target,
        "refusing to move last_reviewed backwards or outside the reviewed range",
    )?;
    ancestor(
        &clone,
        &target,
        &head,
        "reviewed commit is not reachable from upstream HEAD",
    )?;
    if target == record.reviewed {
        return Ok(format!("last_reviewed={target}\nunchanged=true\n"));
    }
    let date = std::env::var("MX_UPSTREAM_REVIEW_DATE").unwrap_or_else(|_| {
        let now = OffsetDateTime::now_utc();
        format!(
            "{:04}-{:02}-{:02}",
            now.year(),
            u8::from(now.month()),
            now.day()
        )
    });
    if !Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$")
        .expect("date regex")
        .is_match(&date)
    {
        return Err(format!("invalid review date: {date}"));
    }
    let mut changed = 0;
    let mut starts = 0;
    let mut ends = 0;
    let mut in_log = false;
    let mut output = String::new();
    for line in record.text.lines() {
        if line.starts_with("last_reviewed:") {
            output.push_str(&format!("last_reviewed: {target}\n"));
            changed += 1;
        } else if line == "<!-- mx-upstream-log:start -->" {
            starts += 1;
            in_log = true;
            output.push_str(line);
            output.push('\n');
        } else if line == "<!-- mx-upstream-log:end -->" {
            ends += 1;
            output.push_str(&format!(
                "- {date}: reviewed through `{target}` via the upstream-sync workflow.\n{line}\n"
            ));
            in_log = false;
        } else if !(in_log && line == "_No completed upstream review has been recorded._") {
            output.push_str(line);
            output.push('\n');
        }
    }
    if changed != 1 || starts != 1 || ends != 1 || in_log {
        return Err(
            "record must contain one last_reviewed field and one completed-review log".to_owned(),
        );
    }
    let mode = fs::metadata(record_path)
        .map(|meta| std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o777)
        .unwrap_or(0o644);
    atomic_replace(record_path, output.as_bytes(), mode)
        .map_err(|_| "cannot update record".to_owned())?;
    Ok(format!("last_reviewed={target}\n"))
}

pub fn run(args: &[String], record_path: &Path) -> Output {
    if matches!(args, [arg] if arg == "-h" || arg == "--help") {
        return Output {
            status: 0,
            stdout: HELP.to_owned(),
            stderr: String::new(),
        };
    }
    let record = match load_record(record_path) {
        Ok(record) => record,
        Err(error) => return fail(error),
    };
    if args == ["--status"] {
        return Output {
            status: if record.status == "active" { 0 } else { 3 },
            stdout: format!(
                "upstream_repo={}\nfork_point={}\nlast_reviewed={}\nstatus={}\nretired_reason={}\n",
                record.repo, record.fork, record.reviewed, record.status, record.retired_reason
            ),
            stderr: String::new(),
        };
    }
    if record.status == "retired" {
        return Output {
            status: 3,
            stdout: String::new(),
            stderr: format!("upstream sync retired: {}\n", record.retired_reason),
        };
    }
    let result = if matches!(args, [flag, reviewed] if flag == "--record-reviewed") {
        record_reviewed(record_path, &record, &args[1])
    } else {
        let mut output = None;
        let mut since = None;
        let mut index = 0;
        let mut valid = true;
        while index < args.len() {
            match args[index].as_str() {
                "--out" if index + 1 < args.len() && output.is_none() => {
                    output = Some(args[index + 1].as_str());
                    index += 2;
                }
                "--since" if index + 1 < args.len() && since.is_none() => {
                    since = Some(args[index + 1].as_str());
                    index += 2;
                }
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if let Some(output) = output.filter(|_| valid) {
            report(&record, Path::new(output), since)
        } else {
            return Output {
                status: 2,
                stdout: String::new(),
                stderr: HELP.to_owned(),
            };
        }
    };
    match result {
        Ok(stdout) => Output {
            status: 0,
            stdout,
            stderr: String::new(),
        },
        Err(error) => fail(error),
    }
}

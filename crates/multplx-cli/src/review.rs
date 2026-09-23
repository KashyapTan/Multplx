use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use multplx_core::checks;
use multplx_core::filesystem::atomic_replace;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::{
    ProcessProbe, ProcessTerminator, SystemProcessProbe, SystemProcessTerminator,
};
use multplx_domain::maintainer_override::{Binding, OverrideStore};
use multplx_domain::review_delivery::{
    DeliveryRecord, FileIdentity, OperationalTaskId, PollRegistration, PrIdentity,
    PublicationAuthority, PublicationOperation, PublicationStage, Validation, agent_ambience,
    head_valid, publish_private, read_private, ref_valid, title_valid,
};
use sha2::{Digest, Sha256};

const ENTRIES: &[&str] = &[
    "mx-check-register.sh",
    "mx-deep-review.sh",
    "mx-deliver.sh",
    "mx-merge-local.sh",
    "mx-pr-check-migrate.sh",
    "mx-pr-check.sh",
    "mx-pr-merge.sh",
    "mx-pr-poll.sh",
    "mx-promote.sh",
    "mx-review-diff.sh",
    "mx-validation-waive.sh",
];

pub fn run(entry: &str, args: &[OsString]) -> i32 {
    if !ENTRIES.contains(&entry) {
        eprintln!("error: unknown review or delivery entry point: {entry}");
        return 2;
    }
    match entry {
        "mx-check-register.sh" => check_register(args),
        "mx-deep-review.sh" => crate::deep_review::run(args),
        "mx-deliver.sh" => deliver(args),
        "mx-merge-local.sh" => merge_local(args),
        "mx-pr-check-migrate.sh" => pr_check_migrate(args),
        "mx-pr-check.sh" => pr_check(args),
        "mx-pr-merge.sh" => pr_merge(args),
        "mx-pr-poll.sh" => pr_poll(args),
        "mx-promote.sh" => promote(args),
        "mx-review-diff.sh" => review_diff(args),
        "mx-validation-waive.sh" => validation_waive(args),
        _ => {
            eprintln!("error: unhandled review or delivery entry point: {entry}");
            2
        }
    }
}

fn source_root() -> PathBuf {
    std::env::var_os("MX_RUST_SOURCE_ROOT")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn state_root() -> PathBuf {
    std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("MX_HOME")
                .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("state")
        })
}

fn text_args(args: &[OsString]) -> Option<Vec<&str>> {
    args.iter().map(|value| value.to_str()).collect()
}

fn command_output(program: &str, directory: &Path, args: &[&str]) -> Option<Output> {
    Command::new(program)
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .ok()
}

fn command_success(program: &str, directory: &Path, args: &[&str]) -> bool {
    command_output(program, directory, args).is_some_and(|output| output.status.success())
}

fn command_line(program: &str, directory: &Path, args: &[&str]) -> Option<String> {
    let output = command_output(program, directory, args)?;
    if !output.status.success() {
        return None;
    }
    let line = std::str::from_utf8(&output.stdout).ok()?.trim().to_owned();
    (!line.is_empty()).then_some(line)
}

fn meta_value(text: &str, key: &str, last: bool) -> Option<String> {
    let prefix = format!("{key}=");
    let mut values = text
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix))
        .map(str::to_owned);
    if last {
        values.next_back()
    } else {
        values.next()
    }
}

fn default_branch(project: &Path) -> Option<String> {
    if let Some(remote) = command_line(
        "git",
        project,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        return Some(remote.strip_prefix("origin/").unwrap_or(&remote).to_owned());
    }
    ["main", "master"].into_iter().find_map(|branch| {
        command_success(
            "git",
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
}

fn merge_local(args: &[OsString]) -> i32 {
    let _ = Command::new(source_root().join("bin/mx-guard.sh")).status();
    let Some(values) = text_args(args) else {
        eprintln!("usage: mx-merge-local.sh <task-id>");
        return 1;
    };
    if values.len() != 1 || OperationalTaskId::parse(values[0]).is_err() {
        eprintln!("usage: mx-merge-local.sh <task-id>");
        return 1;
    }
    let id = values[0];
    let meta = state_root().join(format!("{id}.meta"));
    let Ok(text) = fs::read_to_string(&meta) else {
        eprintln!("error: no meta for task {id} at {}", meta.display());
        return 1;
    };
    let project = PathBuf::from(meta_value(&text, "project", false).unwrap_or_default());
    let mode = meta_value(&text, "mode", false).unwrap_or_default();
    if mode != "local-only" {
        eprintln!(
            "error: task {id} is mode={mode}, not local-only; PR merges are human-executed actions"
        );
        return 1;
    }
    let worktree = PathBuf::from(meta_value(&text, "worktree", false).unwrap_or_default());
    let task = match multplx_domain::lifecycle::subagent_model::read_meta(id, &text) {
        Ok(task) => task,
        Err(error) => {
            eprintln!("error: invalid task binding: {error}");
            return 1;
        }
    };
    if !task.legacy_unknown {
        let Some(binding) = task.project.as_ref() else {
            eprintln!("error: canonical local task has no immutable project binding");
            return 1;
        };
        if binding.ownership == multplx_domain::project_registry::CheckoutOwnership::UserOwned {
            eprintln!(
                "error: task-bound source checkout is user-owned; retained {id} on mx/{id} for human local integration"
            );
            return 1;
        }
        let Some(allocation) = task.allocation.as_ref() else {
            eprintln!("error: managed local task has no exact allocation binding");
            return 1;
        };
        let common_dir = command_line("git", &worktree, &["rev-parse", "--git-common-dir"])
            .map(PathBuf::from)
            .and_then(|path| {
                if path.is_absolute() {
                    fs::canonicalize(path).ok()
                } else {
                    fs::canonicalize(worktree.join(path)).ok()
                }
            });
        if fs::canonicalize(&project).ok() != fs::canonicalize(&binding.canonical_path).ok()
            || fs::canonicalize(&worktree).ok()
                != fs::canonicalize(Path::new(&allocation.path)).ok()
            || common_dir != fs::canonicalize(&binding.common_git_dir).ok()
        {
            eprintln!("error: local landing task project or allocation binding changed");
            return 1;
        }
    }
    let branch = format!("mx/{id}");
    if !command_success(
        "git",
        &project,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    ) {
        eprintln!(
            "error: branch {branch} does not exist in {}",
            project.display()
        );
        return 1;
    }
    let recorded_root = fs::canonicalize(&worktree).ok();
    let git_root = command_line("git", &worktree, &["rev-parse", "--show-toplevel"])
        .and_then(|root| fs::canonicalize(root).ok());
    if recorded_root.is_none()
        || recorded_root != git_root
        || command_line(
            "git",
            &worktree,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
        )
        .as_deref()
            != Some(&branch)
        || command_output("git", &worktree, &["status", "--porcelain"])
            .is_none_or(|output| !output.status.success() || !output.stdout.is_empty())
        || command_line("git", &worktree, &["rev-parse", "HEAD"])
            != command_line("git", &project, &["rev-parse", &branch])
    {
        eprintln!("error: local landing requires the recorded clean worktree on {branch}");
        return 1;
    }
    let Some(default) = default_branch(&project) else {
        eprintln!(
            "error: cannot determine default branch for {}; expected origin/HEAD, main, or master",
            project.display()
        );
        return 1;
    };
    let current =
        command_line("git", &project, &["symbolic-ref", "--short", "HEAD"]).unwrap_or_default();
    if current != default {
        eprintln!(
            "error: {} is on '{current}', expected default branch '{default}'; cannot merge safely",
            project.display()
        );
        return 1;
    }
    let dirty = command_output("git", &project, &["status", "--porcelain"])
        .is_none_or(|output| !output.status.success() || !output.stdout.is_empty());
    if dirty {
        eprintln!(
            "error: {} has a dirty working tree; refusing to merge into it",
            project.display()
        );
        return 1;
    }
    if !command_success(
        "git",
        &project,
        &["merge-base", "--is-ancestor", &default, &branch],
    ) {
        eprintln!("REFUSED: {branch} is not a fast-forward of {default} (it has diverged).");
        eprintln!("Have the sub-agent rebase {branch} onto {default}, then retry.");
        return 1;
    }
    let Some(before) = command_line("git", &project, &["rev-parse", "--short", &default]) else {
        return 1;
    };
    if !Command::new("git")
        .arg("-C")
        .arg(&project)
        .args(["merge", "--ff-only", &branch])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return 1;
    }
    let Some(after) = command_line("git", &project, &["rev-parse", "--short", &default]) else {
        return 1;
    };
    println!(
        "merged {branch} into local {default} ({before} -> {after}) in {}",
        project.display()
    );
    0
}

fn pr_number(target: &str) -> Option<String> {
    if let Some((_, suffix)) = target.rsplit_once("/pull/") {
        let number = suffix
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        return (!number.is_empty()).then_some(number);
    }
    let number = target
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!number.is_empty()).then_some(number)
}

fn resolve_pr_head(worktree: &Path, url: &str, recorded: &str) -> Option<String> {
    if let Some(number) = pr_number(url)
        && command_success("git", worktree, &["remote", "get-url", "origin"])
    {
        let destination = format!("+refs/pull/{number}/head:refs/mx-review/pull/{number}/head");
        if command_success(
            "git",
            worktree,
            &["fetch", "--quiet", "origin", &destination],
        ) && let Some(head) = command_line(
            "git",
            worktree,
            &[
                "rev-parse",
                "--verify",
                &format!("refs/mx-review/pull/{number}/head^{{commit}}"),
            ],
        ) {
            return Some(head);
        }
    }
    (!recorded.is_empty()
        && command_success(
            "git",
            worktree,
            &["cat-file", "-e", &format!("{recorded}^{{commit}}")],
        ))
    .then(|| recorded.to_owned())
}

fn review_diff(args: &[OsString]) -> i32 {
    let _ = Command::new(source_root().join("bin/mx-guard.sh")).status();
    let usage = "usage: mx-review-diff.sh <task-id> [--stat]";
    let Some(values) = text_args(args) else {
        eprintln!("{usage}");
        return 1;
    };
    if matches!(values.as_slice(), ["-h" | "--help"]) {
        eprintln!("{usage}");
        return 0;
    }
    let (id, stat_only) = match values.as_slice() {
        [id] => (*id, false),
        [id, "--stat"] => (*id, true),
        _ => {
            eprintln!("{usage}");
            return 1;
        }
    };
    let meta = state_root().join(format!("{id}.meta"));
    let Ok(text) = fs::read_to_string(&meta) else {
        eprintln!("error: no meta for task {id} at {}", meta.display());
        return 1;
    };
    let worktree = PathBuf::from(meta_value(&text, "worktree", false).unwrap_or_default());
    let project = PathBuf::from(meta_value(&text, "project", false).unwrap_or_default());
    if worktree.as_os_str().is_empty() {
        eprintln!("error: meta for task {id} is missing worktree=");
        return 1;
    }
    if project.as_os_str().is_empty() {
        eprintln!("error: meta for task {id} is missing project=");
        return 1;
    }
    if !worktree.is_dir() {
        eprintln!(
            "error: worktree for task {id} is missing: {}",
            worktree.display()
        );
        return 1;
    }
    if !project.is_dir() {
        eprintln!(
            "error: project for task {id} is missing: {}",
            project.display()
        );
        return 1;
    }
    let Some(default) = default_branch(&project) else {
        eprintln!(
            "error: cannot determine default branch for {}; expected origin/HEAD, main, or master",
            project.display()
        );
        return 1;
    };
    let preferred = format!("mx/{id}");
    let branch = if command_success(
        "git",
        &worktree,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{preferred}"),
        ],
    ) {
        preferred
    } else {
        let Some(current) = command_line(
            "git",
            &worktree,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
        ) else {
            eprintln!(
                "error: branch mx/{id} does not exist and worktree {} is detached",
                worktree.display()
            );
            return 1;
        };
        if !command_success(
            "git",
            &worktree,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{current}"),
            ],
        ) {
            eprintln!(
                "error: branch {current} does not exist in {}",
                worktree.display()
            );
            return 1;
        }
        current
    };
    let url = meta_value(&text, "pr", true).unwrap_or_default();
    let recorded = meta_value(&text, "pr_head", true).unwrap_or_default();
    let compare = if url.is_empty() {
        branch.clone()
    } else if let Some(head) = resolve_pr_head(&worktree, &url, &recorded) {
        head
    } else {
        eprintln!(
            "warning: PR head unavailable; diff may lag the open PR (using local branch {branch})"
        );
        branch.clone()
    };
    let base = if command_success("git", &project, &["remote", "get-url", "origin"]) {
        let spec = format!("+refs/heads/{default}:refs/remotes/origin/{default}");
        if !command_success("git", &worktree, &["fetch", "origin", &spec, "--quiet"]) {
            return 1;
        }
        format!("origin/{default}")
    } else {
        default
    };
    for (label, value) in [("base", &base), ("compare ref", &compare)] {
        if !command_success(
            "git",
            &worktree,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{value}^{{commit}}"),
            ],
        ) {
            eprintln!(
                "error: {label} {value} does not resolve in {}",
                worktree.display()
            );
            return 1;
        }
    }
    println!("diff base: {base}");
    let range = format!("{base}...{compare}");
    if command_success("git", &worktree, &["diff", "--quiet", &range, "--"]) {
        println!("no changes vs {base}");
        return 0;
    }
    if !Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["diff", "--stat", &range, "--"])
        .status()
        .is_ok_and(|status| status.success())
    {
        return 1;
    }
    if !stat_only {
        println!();
        if !Command::new("git")
            .arg("-C")
            .arg(&worktree)
            .args(["diff", &range, "--"])
            .status()
            .is_ok_and(|status| status.success())
        {
            return 1;
        }
    }
    0
}

fn validation_waive(args: &[OsString]) -> i32 {
    let usage =
        "usage: mx-validation-waive.sh <task-id> <sha> <override-request-id> [--title <title>]";
    let Some(values) = text_args(args) else {
        eprintln!("{usage}");
        return 2;
    };
    let (id, sha, request, explicit_title) = match values.as_slice() {
        [id, sha, request] => (*id, *sha, *request, None),
        [id, sha, request, flag, title] if *flag == "--title" => {
            (*id, *sha, *request, Some(*title))
        }
        _ => {
            eprintln!("{usage}");
            return 2;
        }
    };
    if OperationalTaskId::parse(id).is_err() || !head_valid(sha) {
        eprintln!("validation-waive: invalid task or SHA");
        return 2;
    }
    let state = state_root();
    let meta = state.join(format!("{id}.meta"));
    let gate = state.join(format!("{id}.gate"));
    let run = gate.join("run.json");
    for path in [&meta, &run] {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            eprintln!("validation-waive: task or gate state is unavailable");
            return 1;
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            eprintln!("validation-waive: task or gate state is unavailable");
            return 1;
        }
    }
    let Ok(run_value): Result<serde_json::Value, _> =
        serde_json::from_slice(&fs::read(&run).unwrap_or_default())
    else {
        eprintln!("validation-waive: task or gate state is unavailable");
        return 1;
    };
    if run_value.get("status").and_then(serde_json::Value::as_str) == Some("passed") {
        eprintln!("validation-waive: gate already passed; use the ordinary handoff");
        return 1;
    }
    let text = fs::read_to_string(&meta).unwrap_or_default();
    let worktree = PathBuf::from(meta_value(&text, "worktree", false).unwrap_or_default());
    let Some(branch) = command_line(
        "git",
        &worktree,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    ) else {
        return 1;
    };
    let Some(head) = command_line("git", &worktree, &["rev-parse", "--verify", "HEAD"]) else {
        return 1;
    };
    if branch != format!("mx/{id}") || head != sha {
        eprintln!("validation-waive: worktree no longer matches the exact SHA");
        return 1;
    }
    let mut base = run_value
        .get("default_branch")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if base.is_empty() {
        base = command_line(
            "git",
            &worktree,
            &[
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ],
        )
        .unwrap_or_default();
        base = base.strip_prefix("origin/").unwrap_or(&base).to_owned();
    }
    if base.is_empty() {
        base = "main".to_owned();
    }
    if !ref_valid(&base) {
        eprintln!("validation-waive: invalid base branch");
        return 1;
    }
    let title = explicit_title
        .map(str::to_owned)
        .or_else(|| command_line("git", &worktree, &["log", "-1", "--format=%s"]));
    let Some(title) = title.filter(|value| title_valid(value)) else {
        eprintln!("validation-waive: invalid delivery title");
        return 1;
    };
    let bindings = match crate::authority::validation_bindings(id, sha) {
        Ok(binding) => binding,
        Err(_) => return 1,
    };
    let store = OverrideStore::new(&state);
    let binding = Binding {
        boundary: &bindings.boundary,
        task: &bindings.task,
        project: &bindings.project,
        operation: &bindings.operation,
        target: &bindings.target,
        expected_state_digest: &bindings.expected_state_digest,
    };
    if store.consume(request, &binding).is_err() {
        return 1;
    }
    let record = format!(
        "version=2\ntask={id}\nworktree={}\nbranch={branch}\napproved_sha={sha}\nbase={base}\ngate_run={}\napproval=pending\ntitle={title}\nvalidation=waived\noverride_request={request}\n",
        worktree.display(),
        gate.display()
    );
    let destination = state.join(format!("{id}.ready-to-push"));
    if publish_private(&destination, record.as_bytes()).is_err() {
        return 1;
    }
    let _ = store.result(
        request,
        true,
        &format!("maintainer-waived delivery handoff created for exact SHA {sha}"),
    );
    println!(
        "validation-waive: waived, not passed, for {id} at {sha}; run mx-deliver.sh prepare explicitly to publish this revision"
    );
    0
}

fn pr_check(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("error: invalid PR check request");
        return 2;
    };
    let [id, raw_url] = values.as_slice() else {
        eprintln!("error: invalid PR check request");
        return 2;
    };
    let Ok(task) = OperationalTaskId::parse(*id) else {
        eprintln!("error: invalid PR check request");
        return 2;
    };
    let Ok(identity) = PrIdentity::parse(raw_url) else {
        eprintln!("error: invalid PR check request");
        return 2;
    };
    let state = state_root();
    let Ok(state_meta) = fs::symlink_metadata(&state) else {
        eprintln!("error: task metadata is unavailable");
        return 1;
    };
    if !state_meta.is_dir() || state_meta.file_type().is_symlink() {
        eprintln!("error: task metadata is unavailable");
        return 1;
    }
    let meta = state.join(format!("{task}.meta"));
    let Ok(meta_metadata) = fs::symlink_metadata(&meta) else {
        eprintln!("error: task metadata is unavailable");
        return 1;
    };
    if !meta_metadata.is_file()
        || meta_metadata.file_type().is_symlink()
        || meta_metadata.nlink() != 1
        || meta_metadata.dev() != state_meta.dev()
        || meta_metadata.len() > 4 * 1024 * 1024
    {
        eprintln!("error: task metadata is unavailable");
        return 1;
    }
    let Ok(meta_bytes) = fs::read(&meta) else {
        eprintln!("error: task metadata is unavailable");
        return 1;
    };

    let retirement = state.join(format!("{task}.pr-poll-retirement"));
    if fs::symlink_metadata(&retirement).is_ok() {
        eprintln!("error: pending PR poll retirement could not be validated");
        return 1;
    }

    // The non-executing migration owns recovery of any pre-Portion-11 poll.
    // This process boundary disappears with the migration handler itself.
    if pr_check_migrate(&[OsString::from("--checks-safe")]) != 0 {
        return 1;
    }
    let _ = Command::new(source_root().join("bin/mx-guard.sh")).status();

    let meta_text = String::from_utf8_lossy(&meta_bytes);
    let worktree = meta_value(&meta_text, "worktree", true).unwrap_or_default();
    let canonical_project =
        match task_publication_project_from_text(&task, Path::new(&worktree), &meta_text) {
            Ok(Some(project)) if project != identity.project_path() => {
                eprintln!("error: PR repository differs from the task-bound publication project");
                return 1;
            }
            Ok(project) => project,
            Err(error) => {
                eprintln!("error: {error}");
                return 1;
            }
        };
    let pr_head = if canonical_project.is_some() {
        let Some(local_head) = command_line("git", Path::new(&worktree), &["rev-parse", "HEAD"])
        else {
            eprintln!("error: task publication head is unavailable");
            return 1;
        };
        let Some(base) = default_branch(Path::new(&worktree)) else {
            eprintln!("error: task publication base is unavailable");
            return 1;
        };
        let credentials = match delivery_credentials() {
            Ok(credentials) => credentials,
            Err(error) => {
                eprintln!("error: {error}");
                return 1;
            }
        };
        let mut command = delivery_command("gh", &credentials);
        command.current_dir(&worktree).args([
            "pr",
            "view",
            &identity.url,
            "--json",
            "url,headRefName,baseRefName,state,isCrossRepository,headRefOid",
        ]);
        let value = delivery_output(command)
            .ok()
            .filter(|output| output.success)
            .and_then(|output| serde_json::from_slice::<serde_json::Value>(&output.stdout).ok());
        let valid = value.as_ref().is_some_and(|value| {
            value["url"].as_str() == Some(identity.url.as_str())
                && value["headRefName"].as_str() == Some(format!("mx/{task}").as_str())
                && value["baseRefName"].as_str() == Some(base.as_str())
                && value["state"].as_str() == Some("OPEN")
                && value["isCrossRepository"].as_bool() == Some(false)
                && value["headRefOid"].as_str() == Some(local_head.as_str())
        });
        if !valid {
            eprintln!("error: PR branch, base, repository, or head differs from the task binding");
            return 1;
        }
        Some(local_head)
    } else if !worktree.is_empty() && Path::new(&worktree).is_dir() {
        Command::new("gh")
            .current_dir(&worktree)
            .args([
                "pr",
                "view",
                &identity.url,
                "--json",
                "headRefOid",
                "-q",
                ".headRefOid",
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|value| value.trim().to_owned())
            .filter(|value| head_valid(value))
    } else {
        None
    };

    let data = state.join(format!("{task}.pr-poll"));
    let check = state.join(format!("{task}.check.sh"));
    let registration = state.join(format!("{task}.pr-poll-registration"));
    let (Ok(mut data_temp), Ok(mut check_temp), Ok(mut registration_temp)) = (
        tempfile::Builder::new()
            .prefix(".mx-pr-poll-data.")
            .tempfile_in(&state),
        tempfile::Builder::new()
            .prefix(".mx-pr-poll-check.")
            .tempfile_in(&state),
        tempfile::Builder::new()
            .prefix(".mx-pr-poll-registration.")
            .tempfile_in(&state),
    ) else {
        eprintln!("error: could not prepare PR poll");
        return 1;
    };
    for temporary in [&data_temp, &check_temp, &registration_temp] {
        if fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o600)).is_err() {
            eprintln!("error: could not prepare PR poll");
            return 1;
        }
    }
    if data_temp
        .write_all(identity.render_sidecar().as_bytes())
        .is_err()
    {
        eprintln!("error: could not prepare PR poll");
        return 1;
    }
    let template = source_root().join("bin/mx-pr-poll.sh");
    let Ok(template_bytes) = fs::read(&template) else {
        eprintln!("error: could not prepare PR poll");
        return 1;
    };
    if check_temp.write_all(&template_bytes).is_err() {
        eprintln!("error: could not prepare PR poll");
        return 1;
    }
    let (Ok(data_file), Ok(check_file)) = (
        read_private(data_temp.path(), 0o600, state_meta.dev()),
        read_private(check_temp.path(), 0o600, state_meta.dev()),
    ) else {
        eprintln!("error: could not prepare PR poll");
        return 1;
    };
    let record = PollRegistration {
        task: task.clone(),
        identity: identity.clone(),
        data_hash: data_file.digest.clone(),
        template_hash: check_file.digest.clone(),
        data_identity: FileIdentity {
            device: data_file.identity.device,
            inode: data_file.identity.inode,
        },
        check_identity: FileIdentity {
            device: check_file.identity.device,
            inode: check_file.identity.inode,
        },
    };
    if registration_temp
        .write_all(record.render().as_bytes())
        .is_err()
    {
        eprintln!("error: could not prepare PR poll");
        return 1;
    }
    if std::env::var_os("MX_PR_CHECK_FAULT_AFTER_STAGE").is_some() {
        return 1;
    }
    if let Ok(delay) = std::env::var("MX_PR_CHECK_TEST_DELAY_AFTER_STAGE")
        && let Ok(delay) = delay.parse::<f64>()
        && delay.is_finite()
        && delay > 0.0
    {
        std::thread::sleep(std::time::Duration::from_secs_f64(delay));
    }

    let mut updated = meta_text
        .lines()
        .filter(|line| !line.starts_with("pr=") && !line.starts_with("pr_head="))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    updated.push(format!("pr={}", identity.url));
    if let Some(ref head) = pr_head {
        updated.push(format!("pr_head={head}"));
    }
    let bytes = format!("{}\n", updated.join("\n"));
    if publish_private(&meta, bytes.as_bytes()).is_err() {
        eprintln!("error: PR metadata recording failed");
        return 1;
    }
    let destination_safe = |path: &Path| match fs::symlink_metadata(path) {
        Ok(metadata) => {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.nlink() == 1
                && metadata.dev() == state_meta.dev()
        }
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    };
    if [&data, &registration, &check]
        .into_iter()
        .any(|path| !destination_safe(path))
    {
        eprintln!("error: could not publish PR poll");
        return 1;
    }
    for (temporary, destination) in [
        (&data_temp, &data),
        (&registration_temp, &registration),
        (&check_temp, &check),
    ] {
        if fs::rename(temporary.path(), destination).is_err() {
            for path in [&check, &registration, &data] {
                let _ = fs::remove_file(path);
            }
            eprintln!("error: could not publish PR poll");
            return 1;
        }
    }
    let valid = read_private(&data, 0o600, state_meta.dev())
        .is_ok_and(|file| file.identity == data_file.identity && file.digest == data_file.digest)
        && read_private(&check, 0o600, state_meta.dev()).is_ok_and(|file| {
            file.identity == check_file.identity
                && file.digest == check_file.digest
                && file.bytes == template_bytes
        })
        && read_private(&registration, 0o600, state_meta.dev())
            .is_ok_and(|file| PollRegistration::parse(&file.bytes).as_ref() == Ok(&record));
    if !valid {
        for path in [&check, &registration, &data] {
            let _ = fs::remove_file(path);
        }
        eprintln!("error: could not publish PR poll");
        return 1;
    }
    if let Some(commit) = pr_head.as_deref()
        && let Err(error) = record_direct_pr_publication_evidence(
            &state,
            task.as_str(),
            commit,
            identity.url.as_str(),
        )
    {
        eprintln!("error: canonical PR publication evidence could not be recorded: {error}");
        return 1;
    }
    println!("armed: state/{task}.check.sh");
    0
}

const MIGRATION_MARKER: &str = "mx-pr-check-migration-v1\n";
const MIGRATION_SCAN_MARKER: &str = "mx-pr-check-migration-scan-v1\n";

fn migration_publish_private(path: &Path, bytes: &[u8], temporary_class: &str) -> Result<(), ()> {
    if std::env::var("MX_TEST_MV_MATCH")
        .ok()
        .is_some_and(|needle| temporary_class.contains(&needle))
        && matches!(
            std::env::var("MX_TEST_MV_ACTION").as_deref(),
            Ok("fail" | "signal")
        )
    {
        return Err(());
    }
    publish_private(path, bytes).map_err(|_| ())?;
    if std::env::var_os("MX_TEST_FINAL_PATH").as_deref() == Some(path.as_os_str()) {
        match std::env::var("MX_TEST_FINAL_ACTION").as_deref() {
            Ok("type") => {
                let target = std::env::var_os("MX_TEST_FAULT_LINK_TARGET").ok_or(())?;
                let _ = fs::remove_file(path);
                std::os::unix::fs::symlink(target, path).map_err(|_| ())?;
            }
            Ok("mode") => {
                fs::set_permissions(path, fs::Permissions::from_mode(0o644)).map_err(|_| ())?;
            }
            Ok("content") => fs::write(path, b"faulted final bytes\n").map_err(|_| ())?,
            Ok("device") => {
                if let Some(gate) = std::env::var_os("MX_TEST_FAULT_GATE") {
                    let _ = fs::write(gate, b"");
                }
                let _ = fs::remove_file(path);
                return Err(());
            }
            _ => {}
        }
    }
    let parent = path.parent().ok_or(())?;
    let device = fs::symlink_metadata(parent).map_err(|_| ())?.dev();
    if read_private(path, 0o600, device).is_ok_and(|file| file.bytes == bytes) {
        Ok(())
    } else {
        let _ = fs::remove_file(path);
        Err(())
    }
}

fn poll_valid(state: &Path, task: &OperationalTaskId, template: &[u8]) -> bool {
    let Ok(state_meta) = fs::symlink_metadata(state) else {
        return false;
    };
    let data_path = state.join(format!("{task}.pr-poll"));
    let check_path = state.join(format!("{task}.check.sh"));
    let registration_path = state.join(format!("{task}.pr-poll-registration"));
    let (Ok(data), Ok(check), Ok(registration)) = (
        read_private(&data_path, 0o600, state_meta.dev()),
        read_private(&check_path, 0o600, state_meta.dev()),
        read_private(&registration_path, 0o600, state_meta.dev()),
    ) else {
        return false;
    };
    let Ok(registration) = PollRegistration::parse(&registration.bytes) else {
        return false;
    };
    let Ok(identity) = PrIdentity::parse_sidecar(&data.bytes) else {
        return false;
    };
    registration.task == *task
        && registration.identity == identity
        && registration.data_hash == data.digest
        && registration.template_hash == check.digest
        && registration.data_identity == data.identity
        && registration.check_identity == check.identity
        && check.bytes == template
}

fn live_poll_paths_safe(state: &Path, task: &OperationalTaskId) -> bool {
    [
        state.join(format!("{task}.check.sh")),
        state.join(format!("{task}.pr-poll")),
        state.join(format!("{task}.pr-poll-registration")),
    ]
    .iter()
    .all(|path| {
        fs::symlink_metadata(path).map_or(true, |metadata| {
            !metadata.file_type().is_symlink()
                && (!metadata.is_file() || metadata.nlink() == 1)
                && (path.extension().and_then(|value| value.to_str()) != Some("sh")
                    || metadata.is_file())
        })
    })
}

fn read_lock_field(lock: &Path, name: &str) -> Option<String> {
    let path = lock.join(name);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 {
        return None;
    }
    Some(
        fs::read_to_string(path)
            .ok()?
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn acquire_migration_exclusion(state: &Path) -> Result<(DirectoryLock, bool), &'static str> {
    let lock = state.join(".watch.lock");
    let probe = SystemProcessProbe::default();
    let mut stopped = false;
    if let Some(pid) = read_lock_field(&lock, "pid").and_then(|value| value.parse::<u32>().ok())
        && probe.is_alive(pid)
    {
        let identity = probe
            .identity(pid)
            .map_err(|_| "watcher ownership is ambiguous")?;
        let home = std::env::var_os("MX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                state
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf()
            });
        let watcher = source_root().join("bin/mx-watch.sh");
        if read_lock_field(&lock, "mx-home").as_deref() != Some(&home.to_string_lossy())
            || read_lock_field(&lock, "watcher-path").as_deref() != Some(&watcher.to_string_lossy())
            || read_lock_field(&lock, "pid-identity").as_deref() != Some(identity.marker.as_str())
        {
            return Err("watcher ownership is ambiguous");
        }
        let mut terminator = SystemProcessTerminator::default();
        terminator
            .terminate(&identity)
            .map_err(|_| "watcher could not be paused")?;
        if !terminator.wait_gone(&identity, Duration::from_secs(5)) {
            return Err("watcher did not pause");
        }
        stopped = true;
    }
    DirectoryLock::acquire_wait(&lock, &probe, Duration::from_secs(5))
        .map(|lock| (lock, stopped))
        .map_err(|_| "watcher exclusion could not be acquired")
}

fn check_task_basename(check: &Path) -> Option<String> {
    let output = Command::new("basename")
        .arg(check)
        .arg(".check.sh")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    Some(value.trim_end_matches('\n').to_owned())
}

fn ensure_private_directory(path: &Path, device: u64) -> Result<(), ()> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.dev() != device {
            return Err(());
        }
    } else {
        fs::create_dir(path).map_err(|_| ())?;
    }
    if !chmod_fault_matches(path) {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| ())?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    (metadata.mode() & 0o777 == 0o700).then_some(()).ok_or(())
}

fn chmod_fault_matches(path: &Path) -> bool {
    let Some(pattern) = std::env::var_os("MX_TEST_CHMOD_MATCH") else {
        return false;
    };
    let pattern = pattern.to_string_lossy();
    let path = path.to_string_lossy();
    pattern
        .strip_suffix('*')
        .map_or(path == pattern, |prefix| path.starts_with(prefix))
}

fn validate_quarantine_tree(state: &Path) -> Result<(), ()> {
    let quarantine = state.join(".pr-check-quarantine");
    if fs::symlink_metadata(&quarantine).is_err() {
        return Ok(());
    }
    let state_meta = fs::symlink_metadata(state).map_err(|_| ())?;
    ensure_private_directory(&quarantine, state_meta.dev())?;
    for entry in fs::read_dir(&quarantine).map_err(|_| ())? {
        let path = entry.map_err(|_| ())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.dev() != state_meta.dev()
            || metadata.nlink() != 1
        {
            return Err(());
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str())
            && name.contains(".diagnostic.")
            && ![".check.", ".data.", ".registration.", ".replacement."]
                .iter()
                .any(|marker| {
                    name.rfind(marker).is_some_and(|index| {
                        index > name.find(".diagnostic.").unwrap_or(usize::MAX)
                    })
                })
        {
            let Some((prefix, kind)) = name.split_once(".diagnostic.") else {
                return Err(());
            };
            let message = fs::read_to_string(&path).map_err(|_| ())?;
            if diagnostic_message(prefix, kind)
                .is_none_or(|expected| message != format!("{expected}\n"))
            {
                return Err(());
            }
        }
        if !chmod_fault_matches(&path) {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|_| ())?;
        }
        if fs::symlink_metadata(&path).map_err(|_| ())?.mode() & 0o777 != 0o600 {
            return Err(());
        }
    }
    Ok(())
}

fn migrate_legacy_noncanonical_namespace(state: &Path) -> Result<(), ()> {
    let quarantine = state.join(".pr-check-quarantine");
    if !quarantine.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(&quarantine)
        .map_err(|_| ())?
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    for source in entries {
        let Some(name) = source.file_name().and_then(|name| name.to_str()) else {
            return Err(());
        };
        if !name.starts_with("_noncanonical.") {
            continue;
        }
        if name.ends_with(".diagnostic.pending-noncanonical") {
            let terminal = quarantine.join("!noncanonical.diagnostic.noncanonical");
            if !terminal.exists() {
                migration_publish_private(
                    &terminal,
                    b"noncanonical task artifact quarantined and unarmed\n",
                    ".mx-pr-check-obligation.",
                )?;
            }
            let _ = fs::remove_file(source);
            continue;
        }
        let destination = quarantine.join(name.replacen("_noncanonical.", "!noncanonical.", 1));
        if destination.exists() {
            if fs::read(&source).map_err(|_| ())? != fs::read(&destination).map_err(|_| ())? {
                return Err(());
            }
            fs::remove_file(source).map_err(|_| ())?;
        } else {
            fs::rename(source, destination).map_err(|_| ())?;
        }
    }
    if quarantine
        .join("!noncanonical.diagnostic.noncanonical")
        .exists()
    {
        let _ = fs::remove_file(quarantine.join("!noncanonical.diagnostic.pending-noncanonical"));
    }
    Ok(())
}

fn quarantine_one(state: &Path, source: &Path, prefix: &str, kind: &str) -> Result<bool, ()> {
    if fs::symlink_metadata(source).is_err() {
        return Ok(false);
    }
    let state_meta = fs::symlink_metadata(state).map_err(|_| ())?;
    let source_meta = fs::symlink_metadata(source).map_err(|_| ())?;
    if !source_meta.is_file()
        || source_meta.file_type().is_symlink()
        || source_meta.nlink() != 1
        || source_meta.dev() != state_meta.dev()
    {
        return Err(());
    }
    let quarantine = state.join(".pr-check-quarantine");
    ensure_private_directory(&quarantine, state_meta.dev())?;
    let temporary = tempfile::Builder::new()
        .prefix(&format!("{prefix}.{kind}."))
        .tempfile_in(&quarantine)
        .map_err(|_| ())?;
    let destination = temporary.into_temp_path().keep().map_err(|_| ())?;
    fs::remove_file(&destination).map_err(|_| ())?;
    fs::rename(source, &destination).map_err(|_| ())?;
    if kind == "check"
        && let Some(target) = std::env::var_os("MX_TEST_FAULT_LINK_TARGET")
        && std::env::var_os("MX_TEST_REAL_MV").is_some()
        && std::env::var_os("MX_TEST_FINAL_PATH").is_none()
    {
        fs::remove_file(&destination).map_err(|_| ())?;
        std::os::unix::fs::symlink(target, &destination).map_err(|_| ())?;
    }
    if kind == "check" && std::env::var_os("MX_TEST_REAL_CP").is_some() {
        fs::copy(&destination, source).map_err(|_| ())?;
    }
    if kind == "check"
        && prefix == "task-a"
        && let Some(gate) = std::env::var_os("MX_TEST_FAULT_GATE")
        && std::env::var_os("MX_TEST_REAL_STAT").is_some()
    {
        let _ = fs::write(gate, b"");
        return Err(());
    }
    let moved_meta = fs::symlink_metadata(&destination).map_err(|_| ())?;
    if !moved_meta.is_file() || moved_meta.file_type().is_symlink() {
        return Err(());
    }
    if !chmod_fault_matches(&destination) {
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o600)).map_err(|_| ())?;
    }
    let final_meta = fs::symlink_metadata(&destination).map_err(|_| ())?;
    if !final_meta.is_file()
        || final_meta.file_type().is_symlink()
        || final_meta.nlink() != 1
        || final_meta.dev() != state_meta.dev()
        || final_meta.mode() & 0o777 != 0o600
        || fs::symlink_metadata(source).is_ok()
    {
        return Err(());
    }
    Ok(true)
}

pub(crate) fn quarantine_rejected_check(state: &Path, check: &Path) -> Result<(), ()> {
    let name = check
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".check.sh"))
        .ok_or(())?;
    quarantine_one(state, check, name, "check").map(|_| ())
}

fn diagnostic_message(prefix: &str, kind: &str) -> Option<String> {
    Some(match kind {
        "pending-canonical" | "pending-ambiguous" => {
            format!("task {prefix}: migration outcome tracking started before legacy poll handling")
        }
        "pending-noncanonical" => {
            "noncanonical task artifact: migration outcome tracking started before legacy poll handling".into()
        }
        "canonical" => format!("task {prefix}: canonical legacy poll rebuilt and armed"),
        "failure-canonical" => format!(
            "task {prefix}: canonical poll migration is incomplete; poll remains unarmed; repair its private artifacts, then rerun bootstrap"
        ),
        "failure-ambiguous" => format!(
            "task {prefix}: ambiguous poll migration is incomplete; poll remains unarmed; repair its private artifacts, then rerun bootstrap"
        ),
        "failure-replacement" => format!(
            "task {prefix}: replacement poll lacks canonical provenance or metadata binding; poll remains unarmed; republish it through mx-pr-check.sh"
        ),
        "ambiguous" => {
            format!("task {prefix}: ambiguous or invalid legacy poll quarantined and unarmed")
        }
        "validated" => {
            format!("task {prefix}: validated replacement poll armed after legacy quarantine")
        }
        "noncanonical" => "noncanonical task artifact quarantined and unarmed".into(),
        _ => return None,
    })
}

fn record_migration_outcome(state: &Path, prefix: &str, kind: &str) -> Result<(), ()> {
    let message = diagnostic_message(prefix, kind).ok_or(())?;
    let state_meta = fs::symlink_metadata(state).map_err(|_| ())?;
    let quarantine = state.join(".pr-check-quarantine");
    ensure_private_directory(&quarantine, state_meta.dev())?;
    let obligation = quarantine.join(format!("{prefix}.diagnostic.{kind}"));
    migration_publish_private(
        &obligation,
        format!("{message}\n").as_bytes(),
        ".mx-pr-check-obligation.",
    )?;
    let log = state.join(".pr-check-migration.log");
    let mut contents = if log.exists() {
        read_private(&log, 0o600, state_meta.dev())
            .map_err(|_| ())?
            .bytes
    } else {
        Vec::new()
    };
    let needle = format!("{message}\n");
    if !String::from_utf8_lossy(&contents)
        .lines()
        .any(|line| line == message)
    {
        contents.extend_from_slice(needle.as_bytes());
        migration_publish_private(&log, &contents, ".mx-pr-check-log.")?;
    }
    let pending_kind = match kind {
        "canonical" => Some("pending-canonical"),
        "ambiguous" | "validated" => Some("pending-ambiguous"),
        "noncanonical" => Some("pending-noncanonical"),
        _ => None,
    };
    if let Some(pending_kind) = pending_kind {
        let _ = fs::remove_file(quarantine.join(format!("{prefix}.diagnostic.{pending_kind}")));
    }
    Ok(())
}

fn recover_migration_obligations(state: &Path, template: &[u8]) -> Result<(bool, bool), ()> {
    let quarantine = state.join(".pr-check-quarantine");
    if !quarantine.is_dir() {
        return Ok((false, false));
    }
    let pending = fs::read_dir(&quarantine)
        .map_err(|_| ())?
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            let (prefix, kind) = name
                .strip_suffix(".diagnostic.pending-canonical")
                .map(|prefix| (prefix.to_owned(), "canonical"))
                .or_else(|| {
                    name.strip_suffix(".diagnostic.pending-ambiguous")
                        .map(|prefix| (prefix.to_owned(), "ambiguous"))
                })?;
            Some((path, prefix, kind))
        })
        .collect::<Vec<_>>();
    let mut rebuilt = false;
    let mut ambiguous = false;
    for (_path, raw, kind) in pending {
        let task = OperationalTaskId::parse(&raw).map_err(|_| ())?;
        if kind == "canonical" {
            let data = state.join(format!("{task}.pr-poll"));
            if fs::symlink_metadata(&data).is_ok_and(|metadata| !metadata.is_file()) {
                record_migration_outcome(state, &raw, "failure-canonical")?;
                return Err(());
            }
            if state.join(format!("{task}.check.sh")).is_file()
                && !quarantine
                    .join(format!("{raw}.diagnostic.failure-canonical"))
                    .exists()
            {
                continue;
            }
            let metadata = fs::read(state.join(format!("{task}.meta"))).map_err(|_| ())?;
            let identity =
                multplx_domain::review_delivery::metadata_pr(&metadata).map_err(|_| ())?;
            publish_poll(state, &task, &identity, template).map_err(|_| ())?;
            record_migration_outcome(state, &raw, "canonical")?;
            let _ = fs::remove_file(quarantine.join(format!("{raw}.diagnostic.failure-canonical")));
            rebuilt = true;
        } else if poll_valid(state, &task, template) {
            let metadata = fs::read(state.join(format!("{task}.meta"))).map_err(|_| ())?;
            let metadata_identity = multplx_domain::review_delivery::metadata_pr(&metadata).ok();
            let data_identity = fs::read(state.join(format!("{task}.pr-poll")))
                .ok()
                .and_then(|bytes| PrIdentity::parse_sidecar(&bytes).ok());
            if metadata_identity != data_identity {
                for path in [
                    state.join(format!("{task}.check.sh")),
                    state.join(format!("{task}.pr-poll")),
                    state.join(format!("{task}.pr-poll-registration")),
                ] {
                    let _ = quarantine_one(state, &path, &raw, "replacement");
                }
                record_migration_outcome(state, &raw, "failure-replacement")?;
                return Err(());
            }
            record_migration_outcome(state, &raw, "validated")?;
            for kind in ["failure-ambiguous", "failure-replacement"] {
                let _ = fs::remove_file(quarantine.join(format!("{raw}.diagnostic.{kind}")));
            }
            ambiguous = true;
        } else {
            let live = [
                state.join(format!("{task}.check.sh")),
                state.join(format!("{task}.pr-poll")),
                state.join(format!("{task}.pr-poll-registration")),
            ];
            if live.iter().any(|path| fs::symlink_metadata(path).is_ok()) {
                let unsafe_live = live.iter().any(|path| {
                    fs::symlink_metadata(path).is_ok_and(|metadata| {
                        !metadata.is_file() || metadata.file_type().is_symlink()
                    })
                });
                if live[0].is_file()
                    && live[1..].iter().all(|path| !path.exists())
                    && fs::read_dir(&quarantine).is_ok_and(|entries| {
                        entries.flatten().any(|entry| {
                            let path = entry.path();
                            entry
                                .file_name()
                                .to_string_lossy()
                                .starts_with(&format!("{raw}.check."))
                                && fs::read(path).ok() == fs::read(&live[0]).ok()
                        })
                    })
                {
                    continue;
                }
                if !unsafe_live {
                    if !quarantine
                        .join(format!("{raw}.diagnostic.failure-ambiguous"))
                        .exists()
                        || quarantine
                            .join(format!("{raw}.diagnostic.ambiguous"))
                            .exists()
                    {
                        continue;
                    }
                    for path in &live {
                        let _ = quarantine_one(state, path, &raw, "replacement");
                    }
                }
                record_migration_outcome(state, &raw, "failure-replacement")?;
                return Err(());
            }
            record_migration_outcome(state, &raw, "ambiguous")?;
            let _ = fs::remove_file(quarantine.join(format!("{raw}.diagnostic.failure-ambiguous")));
            ambiguous = true;
        }
    }
    Ok((rebuilt, ambiguous))
}

fn publish_poll(
    state: &Path,
    task: &OperationalTaskId,
    identity: &PrIdentity,
    template: &[u8],
) -> Result<(), ()> {
    let state_meta = fs::symlink_metadata(state).map_err(|_| ())?;
    let data_path = state.join(format!("{task}.pr-poll"));
    let check_path = state.join(format!("{task}.check.sh"));
    let registration_path = state.join(format!("{task}.pr-poll-registration"));
    publish_private(&data_path, identity.render_sidecar().as_bytes()).map_err(|_| ())?;
    publish_private(&check_path, template).map_err(|_| ())?;
    let data = read_private(&data_path, 0o600, state_meta.dev()).map_err(|_| ())?;
    let check = read_private(&check_path, 0o600, state_meta.dev()).map_err(|_| ())?;
    let registration = PollRegistration {
        task: task.clone(),
        identity: identity.clone(),
        data_hash: data.digest.clone(),
        template_hash: check.digest.clone(),
        data_identity: data.identity,
        check_identity: check.identity,
    };
    publish_private(&registration_path, registration.render().as_bytes()).map_err(|_| ())?;
    poll_valid(state, task, template).then_some(()).ok_or(())
}

fn pr_check_migrate(args: &[OsString]) -> i32 {
    let checks_safe = matches!(args, [value] if value == "--checks-safe");
    if !args.is_empty() && !checks_safe {
        eprintln!("error: invalid PR check migration request");
        return 2;
    }
    let state = state_root();
    let migration_failed = || {
        eprintln!(
            "PR_CHECK_MIGRATION: migration did not complete safely; inspect private state before rearming polls"
        );
        if checks_safe { 0 } else { 1 }
    };
    if !state.exists()
        && (fs::create_dir_all(&state).is_err()
            || fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).is_err())
    {
        eprintln!(
            "PR_CHECK_MIGRATION: state directory could not be created; migration did not complete safely"
        );
        return 1;
    }
    let Ok(state_meta) = fs::symlink_metadata(&state) else {
        return 1;
    };
    if !state_meta.is_dir() || state_meta.file_type().is_symlink() {
        eprintln!(
            "PR_CHECK_MIGRATION: state directory is not a private ordinary directory; migration did not complete safely"
        );
        return 1;
    }
    let marker = state.join(".pr-check-migration-v1");
    let scan_marker = state.join(".pr-check-migration-scan-v1");
    let marker_valid = |path: &Path, expected: &[u8]| {
        read_private(path, 0o600, state_meta.dev()).is_ok_and(|file| file.bytes == expected)
    };
    let marker_paths_safe = fs::read_dir(&state).is_ok_and(|entries| {
        entries.flatten().all(|entry| {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            if !name.ends_with(".check.sh") {
                return true;
            }
            let raw = name.trim_end_matches(".check.sh");
            let Ok(task) = OperationalTaskId::parse(raw) else {
                return true;
            };
            let core_task = multplx_core::identifiers::TaskId::parse(raw).ok();
            core_task
                .as_ref()
                .is_some_and(|task| checks::registered(&state, task).unwrap_or(false))
                || poll_valid(
                    &state,
                    &task,
                    &fs::read(source_root().join("bin/mx-pr-poll.sh")).unwrap_or_default(),
                )
                || live_poll_paths_safe(&state, &task)
        })
    });
    let private_boundaries_safe = [
        state.join(".pr-check-migration.log"),
        state.join(".pr-check-migration-v1"),
        state.join(".pr-check-migration-scan-v1"),
    ]
    .iter()
    .all(|path| {
        fs::symlink_metadata(path).map_or(true, |metadata| {
            metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1
        })
    });
    let quarantine_content_safe = validate_quarantine_tree(&state).is_ok();
    let quarantine = state.join(".pr-check-quarantine");
    let pending_or_failure = fs::read_dir(&quarantine).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.contains(".diagnostic.pending-") || name.contains(".diagnostic.failure-")
            })
        })
    });
    if marker_valid(&marker, MIGRATION_MARKER.as_bytes())
        && marker_valid(&scan_marker, MIGRATION_SCAN_MARKER.as_bytes())
        && !pending_or_failure
        && marker_paths_safe
        && private_boundaries_safe
        && quarantine_content_safe
    {
        let legacy_namespace = state.join(".pr-check-quarantine").is_dir()
            && fs::read_dir(state.join(".pr-check-quarantine")).is_ok_and(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("_noncanonical.")
                })
            });
        if !legacy_namespace {
            return 0;
        }
    }
    if !marker_paths_safe || !private_boundaries_safe || !quarantine_content_safe {
        for path in [&marker, &scan_marker] {
            if fs::symlink_metadata(path).is_ok_and(|metadata| {
                metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1
            }) {
                let _ = fs::remove_file(path);
            }
        }
        return migration_failed();
    }
    let legacy_namespace = quarantine.is_dir()
        && fs::read_dir(&quarantine).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("_noncanonical.")
            })
        });
    if checks_safe
        && marker_valid(&scan_marker, MIGRATION_SCAN_MARKER.as_bytes())
        && !legacy_namespace
    {
        return 0;
    }
    if pending_or_failure {
        let _ = fs::remove_file(&marker);
        let _ = fs::remove_file(&scan_marker);
    }
    let template_path = source_root().join("bin/mx-pr-poll.sh");
    let Ok(template) = fs::read(&template_path) else {
        return 1;
    };
    let (_exclusion, stopped_watcher) = match acquire_migration_exclusion(&state) {
        Ok(exclusion) => exclusion,
        Err(reason) => {
            eprintln!(
                "PR_CHECK_MIGRATION: {reason}; review state/.watch.lock before rearming polls"
            );
            return 1;
        }
    };
    if validate_quarantine_tree(&state).is_err() {
        return migration_failed();
    }
    if migrate_legacy_noncanonical_namespace(&state).is_err() {
        return migration_failed();
    }
    let (mut rebuilt, mut ambiguous) = match recover_migration_obligations(&state, &template) {
        Ok(result) => result,
        Err(()) => return migration_failed(),
    };
    let validated_rearmed = quarantine.is_dir()
        && fs::read_dir(&quarantine).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(".diagnostic.validated"))
            })
        });
    let checks = fs::read_dir(&state)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".check.sh"))
        })
        .collect::<Vec<_>>();
    for check in checks {
        let Some(raw) = check_task_basename(&check) else {
            return 1;
        };
        let Ok(task) = OperationalTaskId::parse(&raw) else {
            if record_migration_outcome(&state, "!noncanonical", "pending-noncanonical").is_err()
                || quarantine_one(&state, &check, "!noncanonical", "check").is_err()
                || record_migration_outcome(&state, "!noncanonical", "noncanonical").is_err()
            {
                return migration_failed();
            }
            ambiguous = true;
            continue;
        };
        let core_task = multplx_core::identifiers::TaskId::parse(&raw).ok();
        if core_task
            .as_ref()
            .is_some_and(|task| checks::registered(&state, task).unwrap_or(false))
            || poll_valid(&state, &task, &template)
        {
            continue;
        }
        if !live_poll_paths_safe(&state, &task) {
            return migration_failed();
        }
        let metadata = fs::read(state.join(format!("{task}.meta"))).ok();
        let identity = metadata
            .as_deref()
            .and_then(|bytes| multplx_domain::review_delivery::metadata_pr(bytes).ok());
        let pending_kind = if identity.is_some() {
            "pending-canonical"
        } else {
            "pending-ambiguous"
        };
        if record_migration_outcome(&state, &raw, pending_kind).is_err() {
            return migration_failed();
        }
        let quarantine_failed = quarantine_one(&state, &check, &raw, "check").is_err()
            || quarantine_one(&state, &state.join(format!("{task}.pr-poll")), &raw, "data")
                .is_err()
            || quarantine_one(
                &state,
                &state.join(format!("{task}.pr-poll-registration")),
                &raw,
                "registration",
            )
            .is_err();
        if let Some(identity) = identity {
            if quarantine_failed || publish_poll(&state, &task, &identity, &template).is_err() {
                let _ = record_migration_outcome(&state, &raw, "failure-canonical");
                if !check.exists() {
                    let _ = migration_publish_private(
                        &scan_marker,
                        MIGRATION_SCAN_MARKER.as_bytes(),
                        ".mx-pr-check-scan.",
                    );
                }
                eprintln!(
                    "PR_CHECK_MIGRATION: migration did not complete safely; inspect private state before rearming polls"
                );
                return if checks_safe { 0 } else { 1 };
            }
            if record_migration_outcome(&state, &raw, "canonical").is_err() {
                return migration_failed();
            }
            rebuilt = true;
        } else {
            if quarantine_failed {
                let _ = record_migration_outcome(&state, &raw, "failure-ambiguous");
                if !check.exists() {
                    let _ = migration_publish_private(
                        &scan_marker,
                        MIGRATION_SCAN_MARKER.as_bytes(),
                        ".mx-pr-check-scan.",
                    );
                }
                return migration_failed();
            }
            if record_migration_outcome(&state, &raw, "ambiguous").is_err() {
                return migration_failed();
            }
            ambiguous = true;
        }
    }
    if migration_publish_private(
        &scan_marker,
        MIGRATION_SCAN_MARKER.as_bytes(),
        ".mx-pr-check-scan.",
    )
    .is_err()
        || migration_publish_private(
            &marker,
            MIGRATION_MARKER.as_bytes(),
            ".mx-pr-check-migration.",
        )
        .is_err()
    {
        let _ = fs::remove_file(&marker);
        eprintln!(
            "PR_CHECK_MIGRATION: migration did not complete safely; inspect private state before rearming polls"
        );
        return 1;
    }
    if rebuilt {
        println!(
            "PR_CHECK_MIGRATION: canonical polls rebuilt and armed; resume supervision for this home"
        );
    } else if validated_rearmed {
        println!(
            "PR_CHECK_MIGRATION: validated replacement polls armed; resume supervision for this home"
        );
    } else if ambiguous {
        println!(
            "PR_CHECK_MIGRATION: quarantined polls remain unarmed; review state/.pr-check-migration.log before rearming"
        );
    } else if stopped_watcher {
        println!(
            "PR_CHECK_MIGRATION: migration completed safely; resume supervision for this home"
        );
    }
    0
}

#[derive(Clone, Debug)]
enum DeliveryCredentials {
    Default,
    Token(String),
    Config(PathBuf),
}

fn delivery_credentials() -> Result<DeliveryCredentials, String> {
    let token = std::env::var("MX_DELIVERY_GH_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let config = std::env::var_os("MX_DELIVERY_GH_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    match (token, config) {
        (Some(_), Some(_)) => Err("choose one delivery credential source, not both".to_owned()),
        (Some(token), None) => Ok(DeliveryCredentials::Token(token)),
        (None, Some(path)) if !path.is_absolute() => {
            Err("MX_DELIVERY_GH_CONFIG_DIR must be absolute".to_owned())
        }
        (None, Some(path)) if !path.is_dir() => {
            Err("MX_DELIVERY_GH_CONFIG_DIR is unavailable".to_owned())
        }
        (None, Some(path)) => Ok(DeliveryCredentials::Config(path)),
        (None, None) => Ok(DeliveryCredentials::Default),
    }
}

fn delivery_command(
    program: impl AsRef<std::ffi::OsStr>,
    credentials: &DeliveryCredentials,
) -> Command {
    let mut command = Command::new(program);
    match credentials {
        DeliveryCredentials::Default => {}
        DeliveryCredentials::Token(token) => {
            for key in [
                "GH_TOKEN",
                "GITHUB_TOKEN",
                "GH_ENTERPRISE_TOKEN",
                "GITHUB_ENTERPRISE_TOKEN",
                "GH_CONFIG_DIR",
            ] {
                command.env_remove(key);
            }
            command.env("GH_TOKEN", token);
        }
        DeliveryCredentials::Config(path) => {
            for key in [
                "GH_TOKEN",
                "GITHUB_TOKEN",
                "GH_ENTERPRISE_TOKEN",
                "GITHUB_ENTERPRISE_TOKEN",
                "GH_CONFIG_DIR",
            ] {
                command.env_remove(key);
            }
            command.env("GH_CONFIG_DIR", path);
        }
    }
    command.env("GH_PROMPT_DISABLED", "1");
    command
}

#[derive(Clone, Debug)]
struct DeliveryOutput {
    success: bool,
    stdout: Vec<u8>,
}

fn delivery_output(mut command: Command) -> Result<DeliveryOutput, String> {
    let timeout = std::env::var("MX_DELIVERY_COMMAND_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(120);
    match crate::supervision::run_bounded_command_result(
        &mut command,
        Duration::from_secs(timeout),
        None,
        None,
        false,
    ) {
        crate::supervision::BoundedCommandResult::Completed { success, output } => {
            Ok(DeliveryOutput {
                success,
                stdout: output.into_bytes(),
            })
        }
        crate::supervision::BoundedCommandResult::SpawnFailed(error) => {
            Err(format!("could not start publication command: {error}"))
        }
        crate::supervision::BoundedCommandResult::Interrupted => {
            Err("publication command was interrupted".to_owned())
        }
        crate::supervision::BoundedCommandResult::TimedOut => Err(format!(
            "publication command exceeded its {timeout}-second bound"
        )),
    }
}

fn delivery_status(command: Command) -> Result<bool, String> {
    delivery_output(command).map(|output| output.success)
}

fn private_metadata_text(state: &Path, path: &Path) -> Option<String> {
    let state_meta = fs::symlink_metadata(state).ok()?;
    let file = read_private(path, 0o600, state_meta.dev()).ok()?;
    String::from_utf8(file.bytes).ok()
}

fn task_publication_project(
    state: &Path,
    task: &OperationalTaskId,
    worktree: &Path,
) -> Result<Option<String>, String> {
    let text = private_metadata_text(state, &state.join(format!("{task}.meta")))
        .ok_or("private task metadata unavailable")?;
    task_publication_project_from_text(task, worktree, &text)
}

fn task_publication_project_from_text(
    task: &OperationalTaskId,
    worktree: &Path,
    text: &str,
) -> Result<Option<String>, String> {
    if !text
        .lines()
        .any(|line| line.starts_with("canonical_model="))
    {
        return Ok(None);
    }
    let record = multplx_domain::lifecycle::subagent_model::read_meta(task.as_str(), text)?;
    if record.legacy_unknown {
        return Ok(None);
    }
    let project = record
        .project
        .as_ref()
        .ok_or("canonical publication task has no immutable project binding")?;
    let allocation = record
        .allocation
        .as_ref()
        .ok_or("canonical publication task has no current allocation binding")?;
    if fs::canonicalize(worktree).ok() != fs::canonicalize(Path::new(&allocation.path)).ok() {
        return Err("publication worktree differs from the current task allocation".to_owned());
    }
    let mut ancestry = Command::new("git");
    ancestry.current_dir(worktree).args([
        "merge-base",
        "--is-ancestor",
        &project.starting_revision,
        "HEAD",
    ]);
    if !delivery_status(ancestry).is_ok_and(|success| success) {
        return Err("publication head is unrelated to the task starting revision".to_owned());
    }
    let home = record
        .owner_home
        .as_deref()
        .map(Path::new)
        .ok_or("canonical publication task has no owner home")?;
    multplx_domain::project_registry::validate_publication_location(home, project, worktree)
        .map(Some)
}

fn delivery_gate(
    state: &Path,
    record: &DeliveryRecord,
) -> Result<(String, String, String, String), String> {
    if let Validation::DirectPr { summary } = &record.validation {
        return Ok((
            summary.clone(),
            "unassessed".to_owned(),
            "Full validation gate not run".to_owned(),
            format!(
                "## Summary\n\n{summary}\n\n## Validation\n\ndirect-PR: full validation gate not run. Explicit approval binds exact SHA {}.\n",
                record.approved_sha
            ),
        ));
    }
    if let Validation::Reported {
        summary,
        checks,
        limitations,
    } = &record.validation
    {
        return Ok((
            summary.clone(),
            "reported".to_owned(),
            limitations.clone(),
            format!(
                "## Summary\n\n{summary}\n\n## Checks\n\n{checks}\n\n## Limitations\n\n{limitations}\n\nPublished commit: `{}`.\n",
                record.approved_sha
            ),
        ));
    }
    let gate_meta = fs::symlink_metadata(&record.gate_run).map_err(|_| "gate unavailable")?;
    if !gate_meta.is_dir() || gate_meta.file_type().is_symlink() {
        return Err("gate unavailable".to_owned());
    }
    let run_path = record.gate_run.join("run.json");
    let text = private_metadata_text(state, &run_path).ok_or("gate unavailable")?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| "gate invalid")?;
    let summary = value
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.chars().count() <= 20_000)
        .ok_or("gate summary invalid")?;
    let risk = value
        .get("risk_level")
        .and_then(serde_json::Value::as_str)
        .filter(|value| matches!(*value, "low" | "medium" | "high"))
        .ok_or("gate risk invalid")?;
    let rationale = value
        .get("risk_rationale")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.chars().count() <= 4_000)
        .ok_or("gate rationale invalid")?;
    if value
        .get("approved_head")
        .and_then(serde_json::Value::as_str)
        != Some(&record.approved_sha)
    {
        return Err("gate head changed".to_owned());
    }
    let body = match &record.validation {
        Validation::DirectPr { .. } => unreachable!("handled before gate read"),
        Validation::Reported { .. } => unreachable!("handled before gate read"),
        Validation::Passed => {
            if value.get("status").and_then(serde_json::Value::as_str) != Some("passed") {
                return Err("gate did not pass".to_owned());
            }
            format!("## Summary\n\n{summary}\n\n## Risk\n\n{risk} - {rationale}")
        }
        Validation::Waived { override_request } => {
            if value.get("status").and_then(serde_json::Value::as_str) == Some("passed") {
                return Err("waived gate unexpectedly passed".to_owned());
            }
            let (record_state, _, grant) = OverrideStore::new(state)
                .find(override_request)
                .map_err(|_| "waiver unavailable")?;
            if record_state != multplx_domain::maintainer_override::RecordState::Consumed
                || grant.boundary_id != "validation.waive-gate"
                || grant.task_id != record.task.as_str()
                || grant.target_identity
                    != format!("{}@{}", record.gate_run.display(), record.approved_sha)
                || grant.decision != multplx_domain::maintainer_override::Decision::Consumed
                || grant.outcome != multplx_domain::maintainer_override::Outcome::Succeeded
                || !grant
                    .action_argv_or_operation
                    .contains(&record.approved_sha)
            {
                return Err("waiver binding changed".to_owned());
            }
            format!(
                "## Summary\n\n{summary}\n\n## Validation\n\nMaintainer-waived for exact SHA {}; validation did not pass.\n\n## Risk\n\n{risk} - {rationale}",
                record.approved_sha
            )
        }
    };
    Ok((
        summary.to_owned(),
        risk.to_owned(),
        rationale.to_owned(),
        body,
    ))
}

fn delivery_record_unchanged(
    path: &Path,
    original: &multplx_domain::review_delivery::SecureFile,
) -> bool {
    path.parent()
        .and_then(|parent| fs::symlink_metadata(parent).ok())
        .and_then(|metadata| read_private(path, 0o600, metadata.dev()).ok())
        .is_some_and(|current| {
            current.identity == original.identity && current.digest == original.digest
        })
}

fn mark_delivery_stale(
    path: &Path,
    original: &multplx_domain::review_delivery::SecureFile,
) -> bool {
    let destination = PathBuf::from(format!("{}.stale", path.display()));
    delivery_record_unchanged(path, original)
        && fs::symlink_metadata(&destination).is_err()
        && fs::rename(path, destination).is_ok()
}

fn delivery_eligibility(state: &Path, record: &DeliveryRecord) -> Result<String, (bool, String)> {
    if record.publication == PublicationAuthority::LegacyPending {
        return Err((
            false,
            "a legacy pending handoff; run prepare explicitly to create an ordinary publication request"
                .to_owned(),
        ));
    }
    let meta = state.join(format!("{}.meta", record.task));
    let matches_meta = private_metadata_text(state, &meta).is_some_and(|text| {
        let values = text
            .lines()
            .filter_map(|line| line.strip_prefix("worktree="))
            .collect::<Vec<_>>();
        values == [record.worktree.to_string_lossy().as_ref()]
            && (!matches!(record.validation, Validation::DirectPr { .. })
                || (text
                    .lines()
                    .filter_map(|line| line.strip_prefix("mode="))
                    .collect::<Vec<_>>()
                    == ["direct-PR"]
                    && text
                        .lines()
                        .filter_map(|line| line.strip_prefix("kind="))
                        .collect::<Vec<_>>()
                        == ["delivery"]))
    });
    if !matches_meta {
        return Err((
            true,
            "task metadata no longer binds the recorded worktree".to_owned(),
        ));
    }
    if let (Some(attempt_id), Some(generation), Some(brief_revision)) = (
        record.attempt_id.as_deref(),
        record.attempt_generation,
        record.brief_revision,
    ) {
        let Some(text) = private_metadata_text(state, &meta) else {
            return Err((true, "task metadata became unavailable".to_owned()));
        };
        let current =
            multplx_domain::lifecycle::subagent_model::read_meta(record.task.as_str(), &text).ok();
        if current
            .as_ref()
            .and_then(|task| task.attempt.as_ref())
            .is_none_or(|attempt| {
                attempt.id != attempt_id
                    || attempt.generation != generation
                    || attempt.brief_revision != brief_revision
            })
            || current
                .as_ref()
                .and_then(|task| task.accepted_brief_revision)
                != Some(brief_revision)
        {
            return Err((
                true,
                "publication evidence belongs to a stale attempt or accepted brief".to_owned(),
            ));
        }
    }
    let stale = |message: &str| Err((true, message.to_owned()));
    if !record.worktree.is_dir() {
        return stale("recorded worktree is missing");
    }
    if command_line("git", &record.worktree, &["rev-parse", "--show-toplevel"]).as_deref()
        != record.worktree.to_str()
    {
        return stale("recorded worktree is not its git top level");
    }
    if command_line(
        "git",
        &record.worktree,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )
    .as_deref()
        != Some(&record.branch)
    {
        return stale("worktree branch moved from the approved branch");
    }
    if command_line("git", &record.worktree, &["rev-parse", "--verify", "HEAD"]).as_deref()
        != Some(&record.approved_sha)
    {
        return stale("worktree HEAD moved past the approved SHA");
    }
    let Some(status) = command_output("git", &record.worktree, &["status", "--porcelain"])
        .filter(|output| output.status.success())
    else {
        return stale("worktree cleanliness could not be verified");
    };
    if !status.stdout.is_empty() {
        return stale("worktree changed after validation");
    }
    if !command_success("git", &record.worktree, &["remote", "get-url", "origin"]) {
        return stale("worktree has no origin remote");
    }
    delivery_gate(state, record)
        .map(|(_, _, _, body)| body)
        .map_err(|_| {
            (
                true,
                "gate run no longer proves this approved SHA".to_owned(),
            )
        })
}

fn preserve_delivery_receipt(
    state: &Path,
    record: &DeliveryRecord,
) -> Result<Option<multplx_domain::review_delivery::SecureFile>, String> {
    let path = state.join(format!("{}.delivered", record.task));
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(_) => (),
    }
    let device = fs::symlink_metadata(state)
        .map_err(|error| error.to_string())?
        .dev();
    let prior = read_private(&path, 0o600, device)?;
    let previous = DeliveryRecord::parse(&prior.bytes, &record.task, state)?;
    if previous.publication == PublicationAuthority::LegacyPending
        || previous.worktree != record.worktree
        || previous.branch != record.branch
        || previous.base != record.base
    {
        return Err("prior delivery receipt binding changed".to_owned());
    }
    let archive = if previous.publication == PublicationAuthority::Ordinary {
        state.join(format!(
            "{}.delivered-{}-{}",
            record.task,
            previous.approved_sha,
            previous.publication_fingerprint()
        ))
    } else {
        state.join(format!(
            "{}.delivered-{}",
            record.task, previous.approved_sha
        ))
    };
    match fs::symlink_metadata(&archive) {
        Ok(_) => {
            if read_private(&archive, 0o600, device)?.bytes != prior.bytes {
                return Err("prior delivery history conflicts".to_owned());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut temporary =
                tempfile::NamedTempFile::new_in(state).map_err(|error| error.to_string())?;
            temporary
                .write_all(&prior.bytes)
                .map_err(|error| error.to_string())?;
            temporary
                .as_file()
                .sync_all()
                .map_err(|error| error.to_string())?;
            temporary
                .persist_noclobber(&archive)
                .map_err(|error| error.to_string())?;
            if read_private(&archive, 0o600, device)?.bytes != prior.bytes {
                return Err("prior delivery history changed".to_owned());
            }
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(Some(prior))
}

fn existing_delivery_pr(
    state: &Path,
    record: &DeliveryRecord,
    credentials: &DeliveryCredentials,
    prior: bool,
    expected_project: Option<&str>,
    authoritative: Option<&PrIdentity>,
) -> Result<Option<PrIdentity>, String> {
    let meta = private_metadata_text(state, &state.join(format!("{}.meta", record.task)))
        .ok_or("private task metadata unavailable")?;
    let recorded = if let Some(identity) = authoritative {
        Some(identity.clone())
    } else if meta.lines().any(|line| line.starts_with("pr=")) {
        Some(multplx_domain::review_delivery::metadata_pr(
            meta.as_bytes(),
        )?)
    } else {
        None
    };
    // Query before create, including when no local PR record exists. This is
    // the reconciliation point for a PR created before its local receipt.
    let mut command = delivery_command("gh", credentials);
    command.current_dir(&record.worktree).args([
        "pr",
        "list",
        "--head",
        &record.branch,
        "--state",
        "all",
        "--limit",
        "2",
        "--json",
        "url,headRefName,baseRefName,state,isCrossRepository",
    ]);
    let output = delivery_output(command)?;
    if !output.success {
        return Err("PR absence could not be verified".to_owned());
    }
    let values: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    if values.is_empty() {
        return if recorded.is_some() || prior {
            Err("recorded PR is absent from the task repository".to_owned())
        } else {
            Ok(None)
        };
    }
    let mut open = values
        .iter()
        .filter(|value| value["state"].as_str() == Some("OPEN"));
    let Some(value) = open.next() else {
        return Err(
            "the branch already has a non-open PR; refusing to create a duplicate".to_owned(),
        );
    };
    if open.next().is_some() {
        return Err("the branch has multiple open PR identities".to_owned());
    }
    let identity = PrIdentity::parse(value["url"].as_str().unwrap_or_default())?;
    if expected_project.is_some_and(|project| project != identity.project_path()) {
        return Err("PR repository differs from the task-bound publication project".to_owned());
    }
    if recorded
        .as_ref()
        .is_some_and(|recorded| recorded != &identity)
        || value["headRefName"].as_str() != Some(&record.branch)
        || value["baseRefName"].as_str() != Some(&record.base)
        || value["isCrossRepository"].as_bool() != Some(false)
    {
        return Err("existing PR identity, branch, base, or repository changed".to_owned());
    }
    Ok(Some(identity))
}

fn remote_branch_at_commit(record: &DeliveryRecord, credentials: &DeliveryCredentials) -> bool {
    let reference = format!("refs/heads/{}", record.branch);
    let mut command = delivery_command("git", credentials);
    command
        .current_dir(&record.worktree)
        .args(["ls-remote", "--heads", "origin", &reference]);
    delivery_output(command)
        .ok()
        .filter(|output| output.success)
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| {
            text.lines().find_map(|line| {
                let (commit, found) = line.split_once(char::is_whitespace)?;
                (found.trim() == reference).then_some(commit.to_owned())
            })
        })
        .as_deref()
        == Some(&record.approved_sha)
}

fn publication_operation_path(state: &Path, record: &DeliveryRecord) -> PathBuf {
    state.join(format!(
        "{}.publication-g{}-r{}-{}",
        record.task,
        record.attempt_generation.unwrap_or_default(),
        record.brief_revision.unwrap_or_default(),
        record.publication_fingerprint()
    ))
}

fn publication_operation_lock(state: &Path, record: &DeliveryRecord) -> PathBuf {
    state.join(format!(
        ".{}.publication-{}.lock",
        record.task,
        record.publication_fingerprint()
    ))
}

fn publication_operation(
    state: &Path,
    record: &DeliveryRecord,
) -> Result<PublicationOperation, String> {
    let _lock = DirectoryLock::acquire_wait(
        publication_operation_lock(state, record),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    let path = publication_operation_path(state, record);
    let state_meta = fs::symlink_metadata(state).map_err(|error| error.to_string())?;
    match read_private(&path, 0o600, state_meta.dev()) {
        Ok(file) => PublicationOperation::parse(&file.bytes, record),
        Err(_) if fs::symlink_metadata(&path).is_err() => {
            let operation = PublicationOperation::new(record);
            publish_private(&path, &operation.render()?)?;
            Ok(operation)
        }
        Err(error) => Err(error),
    }
}

fn advance_publication_operation(
    state: &Path,
    record: &DeliveryRecord,
    expected: &PublicationOperation,
    stage: PublicationStage,
    pr_url: Option<&str>,
) -> Result<PublicationOperation, String> {
    let _lock = DirectoryLock::acquire_wait(
        publication_operation_lock(state, record),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    let path = publication_operation_path(state, record);
    let state_meta = fs::symlink_metadata(state).map_err(|error| error.to_string())?;
    let current = read_private(&path, 0o600, state_meta.dev())?;
    let mut operation = PublicationOperation::parse(&current.bytes, record)?;
    if operation.stage >= stage {
        if pr_url.is_some_and(|url| {
            operation
                .pr_url
                .as_deref()
                .is_some_and(|saved| saved != url)
        }) {
            return Err("publication operation PR identity changed".to_owned());
        }
        return Ok(operation);
    }
    if operation != *expected {
        return Err("publication operation changed during reconciliation".to_owned());
    }
    operation.stage = stage;
    if let Some(url) = pr_url {
        let identity = PrIdentity::parse(url)?;
        if operation
            .pr_url
            .as_ref()
            .is_some_and(|existing| existing != &identity.url)
        {
            return Err("publication operation PR identity changed".to_owned());
        }
        operation.pr_url = Some(identity.url);
    }
    publish_private(&path, &operation.render()?)?;
    Ok(operation)
}

fn evidence_check(value: &str) -> multplx_domain::lifecycle::delivery_evidence::DeliveryCheck {
    use multplx_domain::lifecycle::delivery_evidence::{CheckOutcome, DeliveryCheck};
    let (outcome, summary) = [
        ("passed:", CheckOutcome::Passed),
        ("failed:", CheckOutcome::Failed),
        ("not-run:", CheckOutcome::NotRun),
        ("unknown:", CheckOutcome::Unknown),
    ]
    .into_iter()
    .find_map(|(prefix, outcome)| {
        value
            .strip_prefix(prefix)
            .map(|summary| (outcome, summary.trim()))
    })
    .unwrap_or((CheckOutcome::Unknown, value));
    DeliveryCheck {
        name: "reported validation".to_owned(),
        outcome,
        summary: if summary.is_empty() {
            "no details reported".to_owned()
        } else {
            summary.to_owned()
        },
        artifact: None,
    }
}

fn record_publication_evidence(
    state: &Path,
    record: &DeliveryRecord,
    outcome: multplx_domain::lifecycle::delivery_evidence::DeliveryOutcome,
    pr_url: Option<&str>,
) -> Result<(), String> {
    use multplx_domain::lifecycle::delivery_evidence::{DeliveryOutcome, EvidenceRequest};
    let (Some(attempt_id), Some(attempt_generation), Some(brief_revision)) = (
        record.attempt_id.clone(),
        record.attempt_generation,
        record.brief_revision,
    ) else {
        // Historical approved receipts remain deliverable for migration, but
        // cannot manufacture canonical attempt-bound evidence.
        return Ok(());
    };
    let meta_path = state.join(format!("{}.meta", record.task));
    let text = fs::read_to_string(&meta_path).map_err(|error| error.to_string())?;
    let task = multplx_domain::lifecycle::subagent_model::read_meta(record.task.as_str(), &text)?;
    if outcome == DeliveryOutcome::Published {
        let pr_url = pr_url.ok_or("published evidence requires a canonical PR URL")?;
        return record_published_evidence_for_task(
            state,
            record.task.as_str(),
            &task,
            &record.approved_sha,
            pr_url,
        );
    }
    let kind = match outcome {
        DeliveryOutcome::EvidenceUpdated => "evidence",
        DeliveryOutcome::Published => unreachable!("published handled above"),
        DeliveryOutcome::PublicationFailed => "failed",
        DeliveryOutcome::HumanMerged => "merged",
    };
    let mut hasher = Sha256::new();
    hasher.update(record.task.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(attempt_id.as_bytes());
    hasher.update(attempt_generation.to_le_bytes());
    hasher.update(brief_revision.to_le_bytes());
    hasher.update(kind.as_bytes());
    hasher.update(record.approved_sha.as_bytes());
    hasher.update(record.publication_fingerprint().as_bytes());
    if let Some(url) = pr_url {
        hasher.update(url.as_bytes());
    }
    let fingerprint = format!("{:x}", hasher.finalize());
    let evidence_id = format!("{kind}-{}", &fingerprint[..32]);
    let existing = task
        .delivery
        .history
        .iter()
        .find(|evidence| evidence.evidence_id == evidence_id);
    let (checks, limitations) = match &record.validation {
        Validation::Reported {
            checks,
            limitations,
            ..
        } => (
            vec![evidence_check(checks)],
            if limitations == "none reported" {
                Vec::new()
            } else {
                vec![limitations.clone()]
            },
        ),
        _ => (Vec::new(), Vec::new()),
    };
    let observed_at = existing
        .map(|evidence| evidence.observed_at.clone())
        .unwrap_or_else(|| {
            time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .expect("RFC3339 formatting")
        });
    let request = EvidenceRequest {
        evidence_id,
        attempt_id,
        attempt_generation,
        brief_revision,
        commit: record.approved_sha.clone(),
        checks,
        review: None,
        limitations,
        pr_url: pr_url.map(str::to_owned),
        outcome,
        observed_at,
        mark_current: true,
        expected_current_commit: task.delivery.current_commit.clone(),
    };
    let (_task, _evidence) = multplx_domain::lifecycle::delivery_evidence::record(
        state,
        record.task.as_str(),
        &request,
    )?;
    Ok(())
}

fn record_direct_pr_publication_evidence(
    state: &Path,
    task_id: &str,
    commit: &str,
    pr_url: &str,
) -> Result<(), String> {
    let text = fs::read_to_string(state.join(format!("{task_id}.meta")))
        .map_err(|error| error.to_string())?;
    if !text
        .lines()
        .any(|line| line.starts_with("canonical_model="))
    {
        return Ok(());
    }
    let task = multplx_domain::lifecycle::subagent_model::read_meta(task_id, &text)?;
    if task.legacy_unknown {
        return Ok(());
    }
    record_published_evidence_for_task(state, task_id, &task, commit, pr_url)
}

fn record_published_evidence_for_task(
    state: &Path,
    task_id: &str,
    task: &multplx_domain::lifecycle::subagent_model::TaskRecord,
    commit: &str,
    pr_url: &str,
) -> Result<(), String> {
    use multplx_domain::lifecycle::delivery_evidence::{DeliveryOutcome, EvidenceRequest};
    let attempt = task.attempt.as_ref().ok_or("task attempt missing")?;
    let brief_revision = task
        .accepted_brief_revision
        .ok_or("task accepted brief revision missing")?;
    if task
        .delivery
        .current_commit
        .as_deref()
        .is_some_and(|current| current != commit)
    {
        return Err("PR commit differs from the current delivery revision".to_owned());
    }
    let current = task.current_delivery_evidence().filter(|evidence| {
        evidence.attempt_id == attempt.id
            && evidence.attempt_generation == attempt.generation
            && evidence.brief_revision == brief_revision
            && evidence.commit == commit
    });
    let mut hasher = Sha256::new();
    for value in [task_id, attempt.id.as_str(), commit, pr_url] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    hasher.update(attempt.generation.to_le_bytes());
    hasher.update(brief_revision.to_le_bytes());
    let fingerprint = format!("{:x}", hasher.finalize());
    let evidence_id = format!("published-{}", &fingerprint[..32]);
    let existing = task
        .delivery
        .history
        .iter()
        .find(|evidence| evidence.evidence_id == evidence_id);
    let observed_at = existing
        .map(|evidence| evidence.observed_at.clone())
        .unwrap_or_else(|| {
            time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .expect("RFC3339 formatting")
        });
    let request = EvidenceRequest {
        evidence_id,
        attempt_id: attempt.id.clone(),
        attempt_generation: attempt.generation,
        brief_revision,
        commit: commit.to_owned(),
        checks: current
            .as_ref()
            .map_or_else(Vec::new, |evidence| evidence.checks.clone()),
        review: current
            .as_ref()
            .and_then(|evidence| evidence.review.clone()),
        limitations: current.map_or_else(Vec::new, |evidence| evidence.limitations),
        pr_url: Some(pr_url.to_owned()),
        outcome: DeliveryOutcome::Published,
        observed_at,
        mark_current: true,
        expected_current_commit: task.delivery.current_commit.clone(),
    };
    multplx_domain::lifecycle::delivery_evidence::record(state, task_id, &request)?;
    Ok(())
}

fn reconcile_completed_publication(
    state: &Path,
    task: &OperationalTaskId,
    record: &DeliveryRecord,
    receipt: &multplx_domain::review_delivery::SecureFile,
    operation: &PublicationOperation,
    credentials: &DeliveryCredentials,
) -> Result<(), String> {
    if operation.stage != PublicationStage::Complete {
        return Err("retained publication operation is incomplete".to_owned());
    }
    let recorded_url = operation
        .pr_url
        .as_deref()
        .ok_or("completed publication operation has no PR identity")?;
    delivery_eligibility(state, record).map_err(|(_, reason)| reason)?;
    let expected_project = task_publication_project(state, task, &record.worktree)?;
    if !remote_branch_at_commit(record, credentials) {
        return Err("published branch no longer identifies the recorded commit".to_owned());
    }
    let recorded_identity = PrIdentity::parse(recorded_url)?;
    let observed = existing_delivery_pr(
        state,
        record,
        credentials,
        true,
        expected_project.as_deref(),
        Some(&recorded_identity),
    )?
    .ok_or("recorded PR is no longer observable")?;
    if observed.url != recorded_url {
        return Err("completed publication PR identity changed".to_owned());
    }
    let _lock = DirectoryLock::acquire_wait(
        state.join(format!(".{task}.delivery-prepare.lock")),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    if state.join(format!("{task}.ready-to-push")).exists()
        || !delivery_record_unchanged(&state.join(format!("{task}.delivered")), receipt)
    {
        return Err("publication state changed during completed retry reconciliation".to_owned());
    }
    println!("delivery: {task} already published at {recorded_url}");
    Ok(())
}

fn deliver_one(id: &str, state: &Path, credentials: &DeliveryCredentials) -> i32 {
    let Ok(task) = OperationalTaskId::parse(id) else {
        eprintln!("error: invalid delivery request");
        return 2;
    };
    let Ok(lock) = DirectoryLock::acquire_wait(
        state.join(format!(".{task}.delivery-prepare.lock")),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    ) else {
        eprintln!("delivery: task delivery is busy");
        return 1;
    };
    let path = state.join(format!("{task}.ready-to-push"));
    if fs::symlink_metadata(&path).is_err() {
        let delivered = state.join(format!("{task}.delivered"));
        let result = fs::symlink_metadata(state)
            .map_err(|error| error.to_string())
            .and_then(|state_meta| {
                let receipt = read_private(&delivered, 0o600, state_meta.dev())?;
                let record = DeliveryRecord::parse(&receipt.bytes, &task, state)?;
                let operation = publication_operation(state, &record)?;
                drop(lock);
                reconcile_completed_publication(
                    state,
                    &task,
                    &record,
                    &receipt,
                    &operation,
                    credentials,
                )
            });
        if let Err(error) = result {
            eprintln!("delivery: no ready record for {task}; completed retry unavailable: {error}");
            return 1;
        }
        return 0;
    }
    let Ok(state_meta) = fs::symlink_metadata(state) else {
        eprintln!("delivery: refused malformed or unsafe record for {task}");
        return 1;
    };
    let Ok(file) = read_private(&path, 0o600, state_meta.dev()) else {
        eprintln!("delivery: refused malformed or unsafe record for {task}");
        return 1;
    };
    let Ok(record) = DeliveryRecord::parse(&file.bytes, &task, state) else {
        eprintln!("delivery: refused malformed or unsafe record for {task}");
        return 1;
    };
    let body = match delivery_eligibility(state, &record) {
        Ok(body) => body,
        Err((false, reason)) => {
            eprintln!("delivery: {task} is {reason}");
            return 1;
        }
        Err((true, reason)) => {
            if mark_delivery_stale(&path, &file) {
                eprintln!(
                    "delivery: stale {task} - {reason}; archived as {task}.ready-to-push.stale"
                );
            } else {
                eprintln!("delivery: stale {task} - {reason}; record changed while marking stale");
            }
            return 1;
        }
    };
    let prior = match preserve_delivery_receipt(state, &record) {
        Ok(prior) => prior,
        Err(error) => {
            eprintln!("delivery: refused prior receipt: {error}");
            return 1;
        }
    };
    let mut operation = match publication_operation(state, &record) {
        Ok(operation) => operation,
        Err(error) => {
            eprintln!("delivery: publication intent could not be reserved: {error}");
            return 1;
        }
    };
    drop(lock);

    let publication_failed = |detail: &str, pr_url: Option<&str>| {
        use multplx_domain::lifecycle::delivery_evidence::DeliveryOutcome;
        if let Err(error) =
            record_publication_evidence(state, &record, DeliveryOutcome::PublicationFailed, pr_url)
        {
            eprintln!("delivery: {detail}; failure evidence needs repair: {error}");
        } else {
            eprintln!("delivery: {detail}");
        }
    };
    let expected_project = match task_publication_project(state, &task, &record.worktree) {
        Ok(project) => project,
        Err(error) => {
            publication_failed(&error, operation.pr_url.as_deref());
            return 1;
        }
    };
    let mut existing_pr = match existing_delivery_pr(
        state,
        &record,
        credentials,
        prior.is_some(),
        expected_project.as_deref(),
        None,
    ) {
        Ok(identity) => identity,
        Err(error) => {
            publication_failed(&error, operation.pr_url.as_deref());
            return 1;
        }
    };
    if let Some(saved) = operation.pr_url.as_deref() {
        match existing_pr.as_ref() {
            Some(identity) if identity.url == saved => {}
            Some(_) => {
                publication_failed(
                    "forge PR identity differs from the durable publication receipt",
                    Some(saved),
                );
                return 1;
            }
            None => {
                publication_failed(
                    "durably observed PR is absent from the task repository",
                    Some(saved),
                );
                return 1;
            }
        }
    }
    if !delivery_record_unchanged(&path, &file)
        || prior.as_ref().is_some_and(|previous| {
            !delivery_record_unchanged(&state.join(format!("{task}.delivered")), previous)
        })
    {
        eprintln!(
            "delivery: refused {task} because its delivery record changed during verification"
        );
        return 1;
    }
    let mut push = delivery_command("git", credentials);
    push.arg("-C").arg(&record.worktree).args([
        "push",
        "origin",
        &format!("{}:refs/heads/{}", record.approved_sha, record.branch),
    ]);
    if !remote_branch_at_commit(&record, credentials)
        && !delivery_status(push).is_ok_and(|success| success)
    {
        publication_failed(
            &format!("push failed for {task}"),
            operation.pr_url.as_deref(),
        );
        return 1;
    }
    operation = match advance_publication_operation(
        state,
        &record,
        &operation,
        PublicationStage::BranchPublished,
        None,
    ) {
        Ok(operation) => operation,
        Err(error) => {
            publication_failed(
                &format!("branch published but receipt update failed: {error}"),
                operation.pr_url.as_deref(),
            );
            return 1;
        }
    };

    // Reconcile once more after the branch is visible. Another retry may have
    // created the PR before its local registration was durable.
    if existing_pr.is_none() {
        existing_pr = match existing_delivery_pr(
            state,
            &record,
            credentials,
            false,
            expected_project.as_deref(),
            None,
        ) {
            Ok(identity) => identity,
            Err(error) => {
                publication_failed(&error, operation.pr_url.as_deref());
                return 1;
            }
        };
    }
    if existing_pr.is_none() {
        let mut create_command = delivery_command("gh", credentials);
        create_command.current_dir(&record.worktree).args([
            "pr",
            "create",
            "--base",
            &record.base,
            "--head",
            &record.branch,
            "--title",
            &record.title,
            "--body",
            &body,
        ]);
        let _ = delivery_output(create_command);
        existing_pr = match existing_delivery_pr(
            state,
            &record,
            credentials,
            false,
            expected_project.as_deref(),
            None,
        ) {
            Ok(identity) => identity,
            Err(error) => {
                publication_failed(
                    &format!("PR creation could not be reconciled: {error}"),
                    operation.pr_url.as_deref(),
                );
                return 1;
            }
        };
    }
    let Some(identity) = existing_pr else {
        publication_failed(
            &format!("PR creation failed for {task}"),
            operation.pr_url.as_deref(),
        );
        return 1;
    };
    let mut edit = delivery_command("gh", credentials);
    edit.current_dir(&record.worktree).args([
        "pr",
        "edit",
        &identity.url,
        "--title",
        &record.title,
        "--body",
        &body,
    ]);
    if !delivery_status(edit).is_ok_and(|success| success) {
        publication_failed(
            &format!("existing PR content update failed for {task}"),
            operation.pr_url.as_deref(),
        );
        return 1;
    }
    let url = identity.url;
    operation = match advance_publication_operation(
        state,
        &record,
        &operation,
        PublicationStage::PrObserved,
        Some(&url),
    ) {
        Ok(operation) => operation,
        Err(error) => {
            publication_failed(
                &format!("PR observed but receipt update failed: {error}"),
                Some(&url),
            );
            return 1;
        }
    };
    let Ok(binary) = std::env::current_exe() else {
        return 1;
    };
    let mut check = delivery_command(binary, credentials);
    check
        .args(["review", "mx-pr-check.sh", task.as_str(), &url])
        .env("MX_MULTICALL_EXPLICIT", "1")
        .env("MX_STATE_OVERRIDE", state)
        .env("MX_RUST_SOURCE_ROOT", source_root());
    if !delivery_status(check).is_ok_and(|success| success) {
        publication_failed(&format!("PR state recording failed for {task}"), Some(&url));
        return 1;
    }
    operation = match advance_publication_operation(
        state,
        &record,
        &operation,
        PublicationStage::Registered,
        Some(&url),
    ) {
        Ok(operation) => operation,
        Err(error) => {
            publication_failed(
                &format!("PR registered but receipt update failed: {error}"),
                Some(&url),
            );
            return 1;
        }
    };
    if let Err(error) = record_publication_evidence(
        state,
        &record,
        multplx_domain::lifecycle::delivery_evidence::DeliveryOutcome::Published,
        Some(&url),
    ) {
        eprintln!("delivery: PR registered but canonical outcome needs repair: {error}");
        return 1;
    }
    let Ok(_commit_lock) = DirectoryLock::acquire_wait(
        state.join(format!(".{task}.delivery-prepare.lock")),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    ) else {
        eprintln!("delivery: PR registered but local completion is busy");
        return 1;
    };
    let destination = state.join(format!("{task}.delivered"));
    if let Err(error) = advance_publication_operation(
        state,
        &record,
        &operation,
        PublicationStage::Complete,
        Some(&url),
    ) {
        eprintln!("delivery: completed publication needs receipt repair: {error}");
        return 1;
    }
    if !delivery_record_unchanged(&path, &file)
        || match &prior {
            Some(previous) => !delivery_record_unchanged(&destination, previous),
            None => fs::symlink_metadata(&destination).is_ok(),
        }
        || fs::rename(&path, &destination).is_err()
    {
        eprintln!(
            "delivery: PR was recorded but the ready record could not be archived for {task}"
        );
        return 1;
    }
    println!("delivered: {task} {url}");
    0
}

fn prepare_or_approve(values: &[&str]) -> Result<(), String> {
    if values.first() == Some(&"approve") {
        return Err(
            "approve is retired; ordinary publication has no Multplx approval step".to_owned(),
        );
    }
    if values.first() != Some(&"prepare") || values.len() < 6 {
        return Err("use mx-deliver.sh --help for prepare syntax".to_owned());
    }
    let task = OperationalTaskId::parse(values[1])?;
    let mut sha = None;
    let mut summary = None;
    let mut checks = None;
    let mut limitations = None;
    let mut index = 2;
    while index < values.len() {
        let target = match values[index] {
            "--sha" => &mut sha,
            "--summary" => &mut summary,
            "--checks" => &mut checks,
            "--limitations" => &mut limitations,
            _ => return Err("use mx-deliver.sh --help for prepare syntax".to_owned()),
        };
        let Some(value) = values.get(index + 1).copied() else {
            return Err("publication option is missing its value".to_owned());
        };
        if target.replace(value).is_some() {
            return Err("publication option was repeated".to_owned());
        }
        index += 2;
    }
    let sha = sha.ok_or("--sha is required")?;
    let summary = summary.ok_or("--summary is required")?;
    let checks = checks.unwrap_or("not reported");
    let limitations = limitations.unwrap_or("none reported");
    if !head_valid(sha) {
        return Err("a full exact commit SHA is required".to_owned());
    }
    if std::env::var("MX_TASK_ID").is_ok_and(|id| id != task.as_str()) {
        return Err("prepare belongs to the initiating task".to_owned());
    }
    let state = state_root();
    let state_meta = fs::symlink_metadata(&state).map_err(|error| error.to_string())?;
    if !state_meta.is_dir() || state_meta.file_type().is_symlink() {
        return Err("delivery state directory is unavailable".to_owned());
    }
    let path = state.join(format!("{task}.ready-to-push"));
    let _lock = DirectoryLock::acquire_wait(
        state.join(format!(".{task}.delivery-prepare.lock")),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    let meta = private_metadata_text(&state, &state.join(format!("{task}.meta")))
        .ok_or("private task metadata unavailable")?;
    let worktree = PathBuf::from(meta_value(&meta, "worktree", false).ok_or("missing worktree")?);
    let base = default_branch(&worktree).ok_or("cannot resolve base branch")?;
    let title = command_line("git", &worktree, &["log", "-1", "--format=%s"])
        .ok_or("cannot read commit title")?;
    let model = multplx_domain::lifecycle::subagent_model::read_meta(task.as_str(), &meta).ok();
    let (attempt_id, attempt_generation, brief_revision) = model
        .as_ref()
        .filter(|record| !record.legacy_unknown)
        .and_then(|record| {
            record.attempt.as_ref().map(|attempt| {
                (
                    attempt.id.as_str(),
                    attempt.generation,
                    attempt.brief_revision,
                )
            })
        })
        .unwrap_or(("legacy-unknown", 0, 0));
    let text = format!(
        "version=4\ntask={task}\nworktree={}\nbranch=mx/{task}\ncommit={sha}\nbase={base}\ntitle={title}\nvalidation=reported\nsummary={summary}\nchecks={checks}\nlimitations={limitations}\nattempt_id={attempt_id}\nattempt_generation={attempt_generation}\nbrief_revision={brief_revision}\n",
        worktree.display()
    );
    let record = DeliveryRecord::parse(text.as_bytes(), &task, &state)?;
    delivery_eligibility(&state, &record).map_err(|(_, error)| error)?;
    if let Ok(metadata) = fs::symlink_metadata(&state)
        && let Ok(existing) = read_private(&path, 0o600, metadata.dev())
    {
        if existing.bytes == text.as_bytes() {
            record_publication_evidence(
                &state,
                &record,
                multplx_domain::lifecycle::delivery_evidence::DeliveryOutcome::EvidenceUpdated,
                None,
            )?;
            println!("publication: {task} already prepared at {sha}");
            return Ok(());
        }
        let old = DeliveryRecord::parse(&existing.bytes, &task, &state)?;
        if !matches!(
            old.publication,
            PublicationAuthority::LegacyPending | PublicationAuthority::Ordinary
        ) || old.worktree != record.worktree
            || old.branch != record.branch
            || old.base != record.base
            || old.approved_sha != record.approved_sha
        {
            return Err(
                "a different publication request already exists; reconcile it first".to_owned(),
            );
        }
    }
    publish_private(&path, text.as_bytes())?;
    record_publication_evidence(
        &state,
        &record,
        multplx_domain::lifecycle::delivery_evidence::DeliveryOutcome::EvidenceUpdated,
        None,
    )?;
    println!("publication: {task} ready at {sha}; no approval step is required");
    Ok(())
}

fn deliver(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("error: invalid delivery request");
        return 2;
    };
    if values.first() == Some(&"prepare") || values.first() == Some(&"approve") {
        return match prepare_or_approve(&values) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("delivery: {error}");
                1
            }
        };
    }
    if matches!(values.as_slice(), ["-h" | "--help"]) {
        print!(
            "Publish one or all prepared local branches with the caller's ordinary Git and forge authentication.\n\nUsage: mx-deliver.sh [<task-id>]\n       mx-deliver.sh prepare <task-id> --sha <full-SHA> --summary <one-line-summary> [--checks <passed|failed|not-run|unknown>:<one-line-results>] [--limitations <one-line-limitations>]\n\nprepare records an exact revision-bound publication request and is safe for the assigned agent to run. No Multplx approval step is required. Running the command with a task id reconciles the remote branch and canonical PR, then registers the PR through the existing poll owner. An exact task-id rerun after completion revalidates the current revision, remote branch and canonical PR without publishing again. A no-argument scan never promotes legacy pending handoffs; rerun prepare explicitly to replace a matching legacy handoff.\nExisting open PR revisions verify the canonical PR, branch, and base before updating its title and body. Ordinary historical receipts stay byte-for-byte available at state/<id>.delivered-<SHA>-<fingerprint>; legacy receipts use state/<id>.delivered-<SHA>. Pull-request merge, auto-merge, merge queue submission, and target-branch bypass remain human actions.\nMX_DELIVERY_GH_TOKEN or MX_DELIVERY_GH_CONFIG_DIR may explicitly override ambient forge authentication; otherwise ordinary caller credentials are inherited. Publication commands are bounded by MX_DELIVERY_COMMAND_TIMEOUT_SECONDS (default 120).\n"
        );
        return 0;
    }
    if values.len() > 1
        || values
            .first()
            .is_some_and(|id| OperationalTaskId::parse(*id).is_err())
    {
        eprintln!("error: invalid delivery request");
        return 2;
    }
    let state = state_root();
    let Ok(metadata) = fs::symlink_metadata(&state) else {
        eprintln!("error: delivery state directory is unavailable");
        return 1;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        eprintln!("error: delivery state directory is unavailable");
        return 1;
    }
    let credentials = match delivery_credentials() {
        Ok(credentials) => credentials,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let ids = if let Some(id) = values.first() {
        vec![(*id).to_owned()]
    } else {
        let mut ids = fs::read_dir(&state)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_suffix(".ready-to-push"))
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        ids.sort();
        ids
    };
    ids.into_iter()
        .map(|id| deliver_one(&id, &state, &credentials))
        .max()
        .unwrap_or(0)
}

fn normalized_merge_args(values: &[&str]) -> Result<Vec<String>, String> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < values.len() {
        let value = values[index];
        if matches!(value, "--repo" | "-R")
            || value.starts_with("--repo=")
            || (value.starts_with("-R") && value.len() > 2)
        {
            return Err("extra merge arguments must not override the repository".to_owned());
        }
        if value == "--method" {
            let Some(method) = values.get(index + 1) else {
                return Err("--method requires squash, merge, or rebase".to_owned());
            };
            if !matches!(*method, "squash" | "merge" | "rebase") {
                return Err(format!("unsupported merge method: {method}"));
            }
            output.push(format!("--{method}"));
            index += 2;
            continue;
        }
        if let Some(method) = value.strip_prefix("--method=") {
            if !matches!(method, "squash" | "merge" | "rebase") {
                return Err(format!("unsupported merge method: {method}"));
            }
            output.push(format!("--{method}"));
        } else {
            output.push(value.to_owned());
        }
        index += 1;
    }
    Ok(output)
}

fn pr_merge(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("error: invalid PR merge request");
        return 2;
    };
    if matches!(values.as_slice(), ["-h" | "--help"]) {
        print!(
            "Merge a registered pull request from a human-controlled shell.\n\nUsage: mx-pr-merge.sh <task-id> <canonical-PR-url> [--] [gh-pr-merge-options]\n\nThis helper refuses agent, gate, and automation sessions. Merge overrides, auto-merge, merge queue submission, and repository overrides are unsupported. Ordinary agents may publish branches and open or update PRs with mx-deliver.sh.\n"
        );
        return 0;
    }
    if values.len() < 2 {
        eprintln!("error: invalid PR merge request");
        return 2;
    }
    let id = values[0];
    let Ok(task) = OperationalTaskId::parse(id) else {
        eprintln!("error: invalid PR merge request");
        return 2;
    };
    let Ok(identity) = PrIdentity::parse(values[1]) else {
        eprintln!("error: invalid PR merge request");
        return 2;
    };
    if agent_ambience() {
        eprintln!(
            "error: PR merge commands are human-only and refuse agent, gate, and automation sessions"
        );
        return 3;
    }
    let mut index = 2;
    if matches!(
        values.get(index),
        Some(&"--override" | &"--print-override-bindings")
    ) {
        eprintln!("error: merge override routes are retired; PR merges are human-executed actions");
        return 1;
    }
    if values.get(index) == Some(&"--") {
        index += 1;
    }
    let normalized = match normalized_merge_args(&values[index..]) {
        Ok(values) => values,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let mut merge_args = vec![
        "pr".to_owned(),
        "merge".to_owned(),
        identity.number.clone(),
        "--repo".to_owned(),
        identity.project_path(),
    ];
    if !normalized
        .iter()
        .any(|value| matches!(value.as_str(), "--squash" | "--merge" | "--rebase"))
    {
        merge_args.push("--squash".to_owned());
    }
    merge_args.extend(normalized);

    let meta = state_root().join(format!("{task}.meta"));
    if !fs::symlink_metadata(&meta)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    {
        eprintln!("error: task metadata is unavailable");
        return 1;
    }
    if pr_check(&[OsString::from(task.as_str()), OsString::from(&identity.url)]) != 0 {
        eprintln!(
            "error: PR metadata and poll could not be established; no merge authority can change that capability result"
        );
        return 1;
    }
    Command::new("gh")
        .args(&merge_args)
        .status()
        .ok()
        .and_then(|status| status.code())
        .unwrap_or(1)
}

fn check_register(args: &[OsString]) -> i32 {
    let Some(raw) = args.first().and_then(|value| value.to_str()) else {
        eprintln!("error: invalid custom check registration");
        return 2;
    };
    if args.len() != 1 {
        eprintln!("error: invalid custom check registration");
        return 2;
    }
    let Ok(task) = OperationalTaskId::parse(raw) else {
        eprintln!("error: invalid custom check registration");
        return 2;
    };
    let state = state_root();
    let Ok(state_meta) = fs::symlink_metadata(&state) else {
        eprintln!("error: state directory is unavailable");
        return 1;
    };
    if !state_meta.is_dir() || state_meta.file_type().is_symlink() {
        eprintln!("error: state directory is unavailable");
        return 1;
    }
    let check = state.join(format!("{task}.check.sh"));
    let Ok(check_meta) = fs::symlink_metadata(&check) else {
        eprintln!("error: custom check is unavailable");
        return 1;
    };
    if !check_meta.is_file()
        || check_meta.file_type().is_symlink()
        || check_meta.permissions().mode() & 0o7777 != 0o700
        || check_meta.nlink() != 1
        || check_meta.dev() != state_meta.dev()
    {
        eprintln!("error: custom check is unavailable");
        return 1;
    }
    let Ok(check_file) = read_private(&check, 0o700, state_meta.dev()) else {
        eprintln!("error: custom check hash is unavailable");
        return 1;
    };
    let digest = check_file.digest;
    let trust = state.join(format!("{task}.check-trust"));
    if fs::symlink_metadata(&trust).is_ok_and(|metadata| {
        !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.nlink() != 1
            || metadata.dev() != state_meta.dev()
    }) {
        eprintln!("error: custom check trust path is unavailable");
        return 1;
    }
    let trust_bytes = checks::render_trust(&digest);
    let published = atomic_replace(&trust, trust_bytes.as_bytes(), 0o600).is_ok()
        && read_private(&trust, 0o600, state_meta.dev())
            .is_ok_and(|file| file.bytes == trust_bytes.as_bytes());
    if !published {
        let _ = fs::remove_file(&trust);
        return 1;
    }
    println!("registered: state/{task}.check.sh");
    0
}

fn pr_poll(args: &[OsString]) -> i32 {
    let Some(values) = args
        .iter()
        .map(|value| value.to_str())
        .collect::<Option<Vec<_>>>()
    else {
        return 0;
    };
    let identity = if values.len() == 6 && values[0] == "--validated" {
        let Ok(identity) = PrIdentity::parse(values[2]) else {
            return 0;
        };
        if values[1] != identity.provider
            || values[3] != identity.host
            || values[4] != identity.project_path()
            || values[5] != identity.number
        {
            return 0;
        }
        identity
    } else if values.is_empty() {
        let Some(path) = std::env::var_os("MX_PR_POLL_CHECK_PATH").map(PathBuf::from) else {
            return 0;
        };
        let Some(raw) = path
            .to_str()
            .and_then(|value| value.strip_suffix(".check.sh"))
        else {
            return 0;
        };
        let sidecar = PathBuf::from(format!("{raw}.pr-poll"));
        let Some(parent) = sidecar.parent() else {
            return 0;
        };
        let Ok(parent_meta) = fs::symlink_metadata(parent) else {
            return 0;
        };
        if !parent_meta.is_dir() || parent_meta.file_type().is_symlink() {
            return 0;
        }
        let Ok(file) = read_private(&sidecar, 0o600, parent_meta.dev()) else {
            return 0;
        };
        let Ok(identity) = PrIdentity::parse_sidecar(&file.bytes) else {
            return 0;
        };
        identity
    } else {
        return 0;
    };
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &identity.url,
            "--json",
            "state",
            "-q",
            ".state",
        ])
        .output();
    if output.is_ok_and(|output| output.status.success() && output.stdout == b"MERGED\n") {
        println!("merged");
    }
    0
}

fn promote(args: &[OsString]) -> i32 {
    let Some(raw) = args.first().and_then(|value| value.to_str()) else {
        eprintln!("usage: mx-promote.sh <task-id>");
        return 1;
    };
    if args.len() != 1 || OperationalTaskId::parse(raw).is_err() {
        eprintln!("usage: mx-promote.sh <task-id>");
        return 1;
    }
    let state = state_root();
    let Ok(state_meta) = fs::symlink_metadata(&state) else {
        eprintln!(
            "error: no meta for task {raw} at {}/{}.meta",
            state.display(),
            raw
        );
        return 1;
    };
    if !state_meta.is_dir() || state_meta.file_type().is_symlink() {
        eprintln!(
            "error: no meta for task {raw} at {}/{}.meta",
            state.display(),
            raw
        );
        return 1;
    }
    let meta = state.join(format!("{raw}.meta"));
    let Ok(file) = read_private(&meta, 0o600, state_meta.dev()) else {
        eprintln!("error: no meta for task {raw} at {}", meta.display());
        return 1;
    };
    let Ok(text) = std::str::from_utf8(&file.bytes) else {
        eprintln!("error: no meta for task {raw} at {}", meta.display());
        return 1;
    };
    if text
        .lines()
        .any(|line| line.starts_with("canonical_model="))
    {
        eprintln!(
            "mx-promote is a compatibility name, not a permission gate. Use mx task-model revise with the current revision, accepted brief and assignment role to record a changed outcome before execution."
        );
    } else {
        eprintln!(
            "legacy assignment identity is unknown; migrate this home before recording a role/outcome revision. Sub-agents do not need promotion to delegate or perform scoped work."
        );
    }
    1
}

fn _credential_boundary_is_visible_to_rust() -> bool {
    agent_ambience()
}

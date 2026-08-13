use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use multplx_core::checks;
use multplx_core::filesystem::atomic_replace;
use multplx_domain::maintainer_override::{Binding, OverrideStore};
use multplx_domain::review_delivery::{
    DeliveryRecord, FileIdentity, OperationalTaskId, PollRegistration, PrIdentity, Validation,
    agent_ambience, head_valid, publish_private, read_private, ref_valid, title_valid,
};

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
    match std::env::var("MX_REVIEW_DELIVERY_IMPLEMENTATION").as_deref() {
        Ok("rust") | Err(std::env::VarError::NotPresent) => {}
        Ok("legacy")
            if matches!(
                entry,
                "mx-check-register.sh"
                    | "mx-merge-local.sh"
                    | "mx-deliver.sh"
                    | "mx-pr-check.sh"
                    | "mx-pr-merge.sh"
                    | "mx-pr-poll.sh"
                    | "mx-promote.sh"
                    | "mx-review-diff.sh"
                    | "mx-validation-waive.sh"
            ) => {}
        Ok("legacy") => return run_compat(entry, args),
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => {
            eprintln!("error: MX_REVIEW_DELIVERY_IMPLEMENTATION must be rust or legacy");
            return 2;
        }
    }
    match entry {
        "mx-check-register.sh" => check_register(args),
        "mx-deep-review.sh" => crate::deep_review::run(args),
        "mx-deliver.sh" => deliver(args),
        "mx-merge-local.sh" => merge_local(args),
        "mx-pr-check.sh" => pr_check(args),
        "mx-pr-merge.sh" => pr_merge(args),
        "mx-pr-poll.sh" => pr_poll(args),
        "mx-promote.sh" => promote(args),
        "mx-review-diff.sh" => review_diff(args),
        "mx-validation-waive.sh" => validation_waive(args),
        _ => run_compat(entry, args),
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
            "error: task {id} is mode={mode}, not local-only; merge PR tasks with bin/mx-pr-merge.sh <id> <PR url> after approval"
        );
        return 1;
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
        eprintln!("Have the actor rebase {branch} onto {default}, then retry.");
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
        "validation-waive: waived, not passed, for {id} at {sha}; delivery approval is pending"
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
        || meta_metadata.len() > 64 * 1024
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
    if !Command::new("bash")
        .arg(source_root().join("bin/mx-pr-check-migrate.sh"))
        .arg("--checks-safe")
        .env("MX_REVIEW_DELIVERY_IMPLEMENTATION", "legacy")
        .env("MX_RUST_SOURCE_ROOT", source_root())
        .status()
        .is_ok_and(|status| status.success())
    {
        return 1;
    }
    let _ = Command::new(source_root().join("bin/mx-guard.sh")).status();

    let meta_text = String::from_utf8_lossy(&meta_bytes);
    let worktree = meta_value(&meta_text, "worktree", true).unwrap_or_default();
    let pr_head = if !worktree.is_empty() && Path::new(&worktree).is_dir() {
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
    if let Some(head) = pr_head {
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
    println!("armed: state/{task}.check.sh");
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
    for key in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GH_CONFIG_DIR",
        "MX_AGENT_GH_TOKEN",
        "MX_DELIVERY_GH_TOKEN",
        "MX_DELIVERY_GH_CONFIG_DIR",
        "CLAUDECODE",
        "CODEX_THREAD_ID",
        "PI_CODING_AGENT",
        "DEEP_REVIEW_GATE",
    ] {
        command.env_remove(key);
    }
    match credentials {
        DeliveryCredentials::Default => {}
        DeliveryCredentials::Token(token) => {
            command.env("GH_TOKEN", token);
        }
        DeliveryCredentials::Config(path) => {
            command.env("GH_CONFIG_DIR", path);
        }
    }
    command
}

fn private_metadata_text(state: &Path, path: &Path) -> Option<String> {
    let state_meta = fs::symlink_metadata(state).ok()?;
    let file = read_private(path, 0o600, state_meta.dev()).ok()?;
    String::from_utf8(file.bytes).ok()
}

fn delivery_gate(
    state: &Path,
    record: &DeliveryRecord,
) -> Result<(String, String, String, String), String> {
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
    if record.approval != "approved" {
        return Err((false, "pending explicit approval".to_owned()));
    }
    let meta = state.join(format!("{}.meta", record.task));
    let matches_meta = private_metadata_text(state, &meta).is_some_and(|text| {
        let values = text
            .lines()
            .filter_map(|line| line.strip_prefix("worktree="))
            .collect::<Vec<_>>();
        values == [record.worktree.to_string_lossy().as_ref()]
    });
    if !matches_meta {
        return Err((
            true,
            "task metadata no longer binds the recorded worktree".to_owned(),
        ));
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

fn deliver_one(id: &str, state: &Path, credentials: &DeliveryCredentials) -> i32 {
    let Ok(task) = OperationalTaskId::parse(id) else {
        eprintln!("error: invalid delivery request");
        return 2;
    };
    let path = state.join(format!("{task}.ready-to-push"));
    if fs::symlink_metadata(&path).is_err() {
        eprintln!("delivery: no ready record for {task}");
        return 1;
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
    if !delivery_record_unchanged(&path, &file) {
        eprintln!("delivery: refused {task} because its ready record changed during verification");
        return 1;
    }
    if !delivery_command("git", credentials)
        .arg("-C")
        .arg(&record.worktree)
        .args([
            "push",
            "origin",
            &format!("{}:refs/heads/{}", record.approved_sha, record.branch),
        ])
        .status()
        .is_ok_and(|status| status.success())
    {
        eprintln!("delivery: push failed for {task}");
        return 1;
    }
    let create = delivery_command("gh", credentials)
        .current_dir(&record.worktree)
        .args([
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
        ])
        .output();
    let output = match create {
        Ok(output) if output.status.success() => Some(output.stdout),
        _ => delivery_command("gh", credentials)
            .current_dir(&record.worktree)
            .args(["pr", "view", &record.branch, "--json", "url", "-q", ".url"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| output.stdout),
    };
    let Some(url) = output
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.lines().next_back().map(str::to_owned))
        .filter(|url| PrIdentity::parse(url).is_ok())
    else {
        eprintln!("delivery: PR creation failed for {task}");
        return 1;
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
    if !check.status().is_ok_and(|status| status.success()) {
        eprintln!("delivery: PR state recording failed for {task}");
        return 1;
    }
    let destination = state.join(format!("{task}.delivered"));
    if !delivery_record_unchanged(&path, &file)
        || fs::symlink_metadata(&destination).is_ok()
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

fn deliver(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("error: invalid delivery request");
        return 2;
    };
    if matches!(values.as_slice(), ["-h" | "--help"]) {
        print!(
            "Deliver one or all approved local branches from outside every agent session.\n\nUsage: mx-deliver.sh [<task-id>]\n"
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
    if agent_ambience() {
        eprintln!(
            "error: credentialed delivery must run outside every broker, actor, daemon, and gate session"
        );
        return 3;
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

fn merge_state(identity: &PrIdentity) -> Result<(String, String), String> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &identity.number,
            "--repo",
            &identity.project_path(),
            "--json",
            "headRefOid,statusCheckRollup",
        ])
        .output()
        .map_err(|_| "could not inspect exact PR head and check set")?;
    if !output.status.success() {
        return Err("could not inspect exact PR head and check set".to_owned());
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "could not inspect exact PR head and check set")?;
    let sha = value
        .get("headRefOid")
        .and_then(serde_json::Value::as_str)
        .filter(|value| head_valid(value))
        .ok_or("could not inspect exact PR head and check set")?;
    let mut failed = value
        .get("statusCheckRollup")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|check| {
            let state = ["conclusion", "state", "status"]
                .into_iter()
                .find_map(|key| check.get(key).and_then(serde_json::Value::as_str))
                .unwrap_or_default();
            let normalized = state.to_ascii_uppercase();
            matches!(
                normalized.as_str(),
                "FAILURE" | "FAILED" | "ERROR" | "CANCELLED" | "TIMED_OUT"
            )
            .then(|| {
                let name = ["name", "context", "workflowName"]
                    .into_iter()
                    .find_map(|key| check.get(key).and_then(serde_json::Value::as_str))
                    .unwrap_or("unknown");
                (name.to_owned(), state.to_owned())
            })
        })
        .collect::<Vec<_>>();
    failed.sort();
    let state = serde_json::json!({
        "sha": sha,
        "failed_checks": failed.into_iter().map(|(name, state)| serde_json::json!({"name": name, "state": state})).collect::<Vec<_>>()
    });
    Ok((
        serde_json::to_string(&state).map_err(|error| error.to_string())?,
        sha.to_owned(),
    ))
}

fn pr_merge(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("error: invalid PR merge request");
        return 2;
    };
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
            "error: credentialed delivery must run outside every broker, actor, daemon, and gate session"
        );
        return 3;
    }
    let mut index = 2;
    let mut override_id = None;
    let mut print_bindings = false;
    match values.get(index).copied() {
        Some("--override") => {
            let Some(value) = values
                .get(index + 1)
                .copied()
                .filter(|value| !value.is_empty())
            else {
                eprintln!("error: --override requires a request id");
                return 2;
            };
            override_id = Some(value);
            index += 2;
        }
        Some("--print-override-bindings") => {
            print_bindings = true;
            index += 1;
        }
        _ => {}
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

    let binding_document = if print_bindings || override_id.is_some() {
        let state = match merge_state(&identity) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("error: {error}");
                return 1;
            }
        };
        if serde_json::from_str::<serde_json::Value>(&state.0)
            .ok()
            .and_then(|value| {
                value
                    .get("failed_checks")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
            })
            .is_none_or(|checks| checks.is_empty())
        {
            eprintln!(
                "error: PR check failure is not a red-check set; use the concrete capability or integrity recovery path"
            );
            return 1;
        }
        let mut operation_args = vec!["gh".to_owned()];
        operation_args.extend(merge_args.clone());
        if !operation_args.iter().any(|value| value == "--admin") {
            operation_args.push("--admin".to_owned());
        }
        let operation = serde_json::to_string(&operation_args).expect("operation JSON");
        let target = format!("{}@{}", identity.url, state.1);
        let project = identity
            .repository
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        Some((
            serde_json::json!({
                "boundary": "delivery.merge-red",
                "task": task.as_str(),
                "project": project,
                "operation": operation,
                "target": target,
                "expected_state_digest": multplx_domain::maintainer_override::sha256_text(&state.0),
                "consequence": "Merge the exact PR head despite the recorded failed check set; record the merge as maintainer-directed.",
                "state": serde_json::from_str::<serde_json::Value>(&state.0).expect("state JSON")
            }),
            operation_args,
        ))
    } else {
        None
    };
    if print_bindings {
        println!(
            "{}",
            serde_json::to_string(binding_document.as_ref().map(|value| &value.0).unwrap())
                .expect("binding JSON")
        );
        return 0;
    }
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
    if let Some(request) = override_id {
        let Some((document, operation_args)) = binding_document else {
            return 1;
        };
        let field = |name: &str| {
            document
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap()
        };
        let binding = Binding {
            boundary: field("boundary"),
            task: field("task"),
            project: field("project"),
            operation: field("operation"),
            target: field("target"),
            expected_state_digest: field("expected_state_digest"),
        };
        let store = OverrideStore::new(&state_root());
        if store.consume(request, &binding).is_err() {
            return 1;
        }
        let status = Command::new(&operation_args[0])
            .args(&operation_args[1..])
            .status()
            .ok()
            .and_then(|status| status.code())
            .unwrap_or(1);
        let succeeded = status == 0;
        let detail = if succeeded {
            format!(
                "maintainer-directed merge completed for {} with recorded failed checks",
                field("target")
            )
        } else {
            format!("maintainer-directed merge command failed with status {status}")
        };
        let _ = store.result(request, succeeded, &detail);
        return status;
    }
    Command::new("gh")
        .args(&merge_args)
        .status()
        .ok()
        .and_then(|status| status.code())
        .unwrap_or(1)
}

fn run_compat(entry: &str, args: &[OsString]) -> i32 {
    let root = source_root();
    let path = root.join("bin").join(entry);
    if !path.is_file() {
        eprintln!(
            "error: review compatibility body is unavailable at {}",
            path.display()
        );
        return 1;
    }
    let error = Command::new("bash")
        .arg(path)
        .args(args)
        .env("MX_REVIEW_DELIVERY_IMPLEMENTATION", "legacy")
        .env("MX_RUST_SOURCE_ROOT", &root)
        .exec();
    eprintln!("error: could not start {entry}: {error}");
    1
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
    if !text.lines().any(|line| line == "kind=scout") {
        eprintln!("error: task {raw} is not a scout task (kind=scout not in meta)");
        return 1;
    }
    let mut output = text
        .lines()
        .filter(|line| !line.starts_with("kind="))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    output.push("kind=delivery".to_owned());
    if atomic_replace(&meta, format!("{}\n", output.join("\n")).as_bytes(), 0o600).is_err() {
        return 1;
    }
    let home = std::env::var_os("MX_HOME")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .unwrap_or_else(|| OsString::from("."));
    let quoted = shell_quote(&home.to_string_lossy());
    println!("promoted {raw} to delivery (teardown protection restored)");
    println!(
        "next: MX_HOME={quoted} bin/mx-send.sh mx-{raw} '<delivery instructions: review scratch state with git status and git log; reset to a clean default-branch base; carry over only intended fix changes; create branch mx/{raw}; implement; report done>'"
    );
    0
}

fn shell_quote(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[allow(dead_code)]
fn _credential_boundary_is_visible_to_rust() -> bool {
    agent_ambience()
}

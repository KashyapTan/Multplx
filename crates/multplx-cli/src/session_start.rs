//! Native owner of the ordered session-start digest.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

pub(crate) struct Paths {
    pub(crate) root: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) source_root: PathBuf,
}

const RULE: &str =
    "================================================================================";
const SUBRULE: &str =
    "--------------------------------------------------------------------------------";

fn subsection(output: &mut String, title: &str) {
    output.push_str(&format!("\n{title}\n{SUBRULE}\n"));
}

fn section(output: &mut String, title: &str) {
    output.push_str(&format!("\n{RULE}\n{title}\n{RULE}\n"));
}

fn command_output(path: &Path, environment: &[(&str, &str)]) -> (i32, String) {
    let Ok(mut combined) = tempfile::tempfile() else {
        return (
            1,
            format!("error: could not capture {} output\n", path.display()),
        );
    };
    let Ok(stderr) = combined.try_clone() else {
        return (
            1,
            format!("error: could not capture {} output\n", path.display()),
        );
    };
    let mut command = Command::new(path);
    command.stdout(Stdio::from(
        combined.try_clone().expect("clone temporary output"),
    ));
    command.stderr(Stdio::from(stderr));
    for (name, value) in environment {
        command.env(name, value);
    }
    let status = command.status();
    let _ = combined.seek(SeekFrom::Start(0));
    let mut text = String::new();
    let _ = combined.read_to_string(&mut text);
    (
        status.ok().and_then(|status| status.code()).unwrap_or(1),
        text,
    )
}

fn print_file(output: &mut String, path: &Path, label: &str) {
    subsection(output, label);
    match fs::read(path) {
        Ok(bytes) if bytes.is_empty() => output.push_str("(present, empty)\n"),
        Ok(bytes) => output.push_str(&String::from_utf8_lossy(&bytes)),
        Err(_) => output.push_str("ABSENT\n"),
    }
}

fn manual_backlog(path: &Path, reason: &str, limit: usize) -> String {
    let mut output = format!(
        "compact backlog listing ({reason}; max {limit} item(s); indented task bodies omitted)\n"
    );
    let raw = fs::read_to_string(path).unwrap_or_default();
    let mut in_section = false;
    let mut total = 0usize;
    let mut shown = 0usize;
    for line in raw.lines() {
        if line.starts_with("## ") {
            in_section = matches!(line.trim(), "## In flight" | "## Queued" | "## Done");
            if in_section {
                output.push_str(line);
                output.push('\n');
            }
        } else if in_section && (line.starts_with("- ") || line.starts_with("* ")) {
            total += 1;
            if shown < limit {
                output.push_str(line);
                output.push('\n');
                shown += 1;
            }
        }
    }
    if total == 0 {
        output.push_str("(no backlog item title lines found)\n");
    } else {
        output.push_str(&format!(
            "(shown {shown} of {total} backlog item title line(s))\n"
        ));
        if total > shown {
            output.push_str(&format!("(truncated {} item(s); increase MX_SESSION_START_BACKLOG_LIMIT for a larger startup listing)\n", total - shown));
        }
    }
    output
}

fn backlog(output: &mut String, paths: &Paths, limit: usize) {
    subsection(output, "data/backlog.md");
    let path = paths.data.join("backlog.md");
    let Ok(metadata) = fs::metadata(&path) else {
        output.push_str("ABSENT\n");
        return;
    };
    if metadata.len() == 0 {
        output.push_str("(present, empty)\n");
        return;
    }
    let mode =
        fs::read_to_string(paths.config.join("backlog-backend")).unwrap_or_else(|_| "owned".into());
    if mode.trim() == "owned" || mode.trim().is_empty() {
        output.push_str(&format!(
            "compact backlog listing (owned backend; max {limit} item(s); task bodies omitted)\n"
        ));
        match multplx_domain::backlog::BacklogStore::new(&path).list(limit) {
            Ok(listing) => output.push_str(&listing),
            Err(error) => {
                output.push_str(
                    "owned backlog compact listing failed; falling back to title-line rendering.\n",
                );
                output.push_str(&format!("{error}\n"));
                output.push_str(&manual_backlog(&path, "fallback", limit));
            }
        }
    } else {
        let reason = if mode.trim() == "manual" {
            "manual backend"
        } else {
            "unknown backend"
        };
        output.push_str(&manual_backlog(&path, reason, limit));
    }
    output.push_str("Full task bodies remain available on demand: bin/mx-backlog.sh show <id>, or data/backlog.md in manual mode.\n");
}

fn meta_value(raw: &str, key: &str) -> String {
    raw.lines()
        .rev()
        .filter_map(|line| line.split_once('='))
        .find_map(|(name, value)| (name == key).then(|| value.to_owned()))
        .unwrap_or_default()
}

fn endpoint_alive(backend: &str, target: &str, id: &str) -> bool {
    match backend {
        "tmux" => Command::new("tmux")
            .args(["display-message", "-p", "-t", target, "#{pane_id}"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success()),
        "herdr" => target.split_once(':').is_some_and(|(session, pane)| {
            !session.is_empty()
                && !pane.is_empty()
                && Command::new("herdr")
                    .args(["pane", "get", pane, "--session", session])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
        }),
        "cmux" => Command::new("cmux")
            .args(["pane", "read", "--pane", target, "--lines", "1"])
            .env("MX_EXPECTED_LABEL", format!("mx-{id}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success()),
        _ => false,
    }
}

fn status_tail(output: &mut String, path: &Path, limit: usize) {
    output.push_str(&format!("status tail (last {limit} line(s), wake-EVENT history, not current state; full log: {}):\n", path.display()));
    let raw = fs::read_to_string(path).unwrap_or_default();
    let lines = raw.lines().collect::<Vec<_>>();
    for line in lines.iter().skip(lines.len().saturating_sub(limit)) {
        output.push_str(line);
        output.push('\n');
    }
}

fn state_digest(output: &mut String, paths: &Paths, status_limit: usize) {
    subsection(output, "Work under way (state/*.meta)");
    let mut metas = fs::read_dir(&paths.state)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "meta"))
        .collect::<Vec<_>>();
    metas.sort();
    if metas.is_empty() {
        output.push_str("(none)\n");
    }
    for meta in &metas {
        let id = meta.file_stem().unwrap_or_default().to_string_lossy();
        output.push_str(&format!("\n--- {id} ---\n"));
        let raw = fs::read_to_string(meta).unwrap_or_default();
        output.push_str(&raw);
        let window = meta_value(&raw, "window");
        if window.is_empty() {
            output.push_str("endpoint: unknown (no window recorded)\n");
        } else {
            let backend = {
                let value = meta_value(&raw, "backend");
                if value.is_empty() {
                    "tmux".into()
                } else {
                    value
                }
            };
            let target = window.clone();
            let state = if endpoint_alive(&backend, &target, &id) {
                "alive"
            } else {
                "dead"
            };
            output.push_str(&format!(
                "endpoint: {state} (backend={backend} window={window})\n"
            ));
        }
        let status = paths.state.join(format!("{id}.status"));
        if status.is_file() {
            status_tail(output, &status, status_limit);
        } else {
            output.push_str(&format!(
                "status tail: (no status file yet: {})\n",
                status.display()
            ));
        }
    }
    subsection(
        output,
        "Orphan status logs (state/*.status without matching .meta)",
    );
    let mut statuses = fs::read_dir(&paths.state)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "status"))
        .collect::<Vec<_>>();
    statuses.sort();
    let mut found = false;
    for status in statuses {
        let id = status.file_stem().unwrap_or_default().to_string_lossy();
        if paths.state.join(format!("{id}.meta")).is_file() {
            continue;
        }
        found = true;
        output.push_str(&format!("\n--- {id} ---\n"));
        status_tail(output, &status, status_limit);
    }
    if !found {
        output.push_str("(none)\n");
    }
}

fn sha256(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn pi_loaded(marker: &Path, extension: &Path, lock: &Path) -> bool {
    let marker = fs::read_to_string(marker).ok();
    let lock = fs::read_to_string(lock).ok();
    let Some((marker, lock, version)) = marker
        .zip(lock)
        .zip(sha256(extension))
        .map(|((a, b), c)| (a, b, c))
    else {
        return false;
    };
    let mut lines = marker.lines();
    lines.next() == Some(version.as_str())
        && lines
            .next()
            .is_some_and(|pid| Some(pid) == lock.lines().next())
}

pub(crate) fn run(paths: &Paths, harness: &str) -> String {
    let status_limit = std::env::var("MX_SESSION_START_STATUS_TAIL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let backlog_limit = std::env::var("MX_SESSION_START_BACKLOG_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v: &usize| *v > 0)
        .unwrap_or(80);
    let bin = paths.source_root.join("bin");
    let mut output = String::new();
    section(
        &mut output,
        &format!("SESSION START - {}", paths.home.display()),
    );
    subsection(&mut output, "LOCK");
    let (lock_status, lock_output) = command_output(&bin.join("mx-lock.sh"), &[]);
    output.push_str(&lock_output);
    if !lock_output.ends_with('\n') {
        output.push('\n');
    }
    let read_only = lock_status != 0;
    if read_only {
        let bar = "●━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";
        output.push_str(&format!("{bar}\n●  READ-ONLY SESSION - SYSTEM LOCK OWNERSHIP WAS NOT VERIFIED\n●  {}\n●  Skipping every mutating step: PR-check migration, stale Herdr child cleanup,\n●  daemon sync, system sync, and wake-queue drain. Detect-only bootstrap\n●  diagnostics and the rest of this read-only-safe digest still ran below.\n●  Operate read-only until this resolves - do not spawn, steer, merge, or\n●  otherwise mutate system state from this session.\n{bar}\n", lock_output.trim_end()));
    }
    subsection(&mut output, "BOOTSTRAP");
    let mut boot = String::new();
    if read_only {
        boot = command_output(
            &bin.join("mx-bootstrap.sh"),
            &[("MX_BOOTSTRAP_DETECT_ONLY", "1")],
        )
        .1;
    } else {
        boot.push_str(&command_output(&bin.join("mx-herdr-session-cleanup.sh"), &[]).1);
        boot.push_str(&command_output(&bin.join("mx-bootstrap.sh"), &[]).1);
    }
    if boot.is_empty() {
        output.push_str("(silent - all good)\n");
    } else {
        output.push_str(&boot);
        if !boot.ends_with('\n') {
            output.push('\n');
        }
    }
    subsection(&mut output, "WAKE QUEUE");
    if read_only {
        let queued = fs::read_to_string(paths.state.join(".wake-queue"))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.is_empty())
            .count();
        output.push_str(&format!("skipped (read-only session) - {queued} record(s) remain queued because this session lacks verified system-lock ownership.\n"));
        output
            .push_str(&command_output(&bin.join("mx-guard.sh"), &[("MX_GUARD_READ_ONLY", "1")]).1);
    } else {
        let drained = command_output(&bin.join("mx-wake-drain.sh"), &[]).1;
        if drained.is_empty() {
            output.push_str("(no queued wakes)\n");
        } else {
            output.push_str(&drained);
            if !drained.ends_with('\n') {
                output.push('\n');
            }
        }
    }
    let afk = paths.state.join(".afk").exists();
    if harness == "pi" {
        let watch = paths.root.join(".pi/extensions/mx-primary-pi-watch.ts");
        let turn = paths
            .root
            .join(".pi/extensions/mx-primary-turnend-guard.ts");
        if !pi_loaded(
            &paths.state.join(".pi-watch-extension-loaded"),
            &watch,
            &paths.state.join(".lock"),
        ) || !pi_loaded(
            &paths.state.join(".pi-turnend-extension-loaded"),
            &turn,
            &paths.state.join(".lock"),
        ) {
            output.push_str(&format!("PI_WATCH_EXTENSION: not loaded - approve Pi project trust once per clone, then restart plain pi so {} and {} auto-load for turn-end guard and background wake coverage; use -e {} -e {} only if project hooks are not trusted\n", turn.display(), watch.display(), turn.display(), watch.display()));
        }
    }
    let supervision = multplx_domain::session::supervision_instructions(
        &[
            "--harness".into(),
            harness.into(),
            "--read-only".into(),
            usize::from(read_only).to_string(),
            "--afk".into(),
            usize::from(afk).to_string(),
        ],
        harness,
        &paths.source_root,
        &paths.root,
    );
    output.push_str(&supervision.stdout);
    section(&mut output, "CONTEXT");
    print_file(
        &mut output,
        &paths.data.join("projects.md"),
        "data/projects.md",
    );
    print_file(
        &mut output,
        &paths.data.join("daemons.md"),
        "data/daemons.md",
    );
    print_file(
        &mut output,
        &paths.data.join("maintainer.md"),
        "data/maintainer.md",
    );
    print_file(
        &mut output,
        &paths.data.join("maintainer-shared.md"),
        "data/maintainer-shared.md (shared, main-authoritative, read-only in daemon homes)",
    );
    print_file(
        &mut output,
        &paths.data.join("learnings.md"),
        "data/learnings.md",
    );
    section(&mut output, "SYSTEM STATE");
    backlog(&mut output, paths, backlog_limit);
    state_digest(&mut output, paths, status_limit);
    subsection(&mut output, "AFK");
    output.push_str(if afk {
        "present - away-mode supervision is active; the daemon owns the watcher.\n"
    } else {
        "absent\n"
    });
    section(&mut output, "NEXT STEP");
    if read_only {
        output.push_str("This session did not acquire the system lock. Stay read-only: do not arm,\ndrain, spawn, steer, merge, or repair system state from here. Only a session\nwith verified system-lock ownership may perform mutable follow-up.\n\n");
    } else if afk {
        output.push_str("Away mode is active. Follow the supervision operating instructions block above:\nload /afk and ensure the daemon is running, because the daemon owns watcher\nsupervision.\n\n");
    } else {
        output.push_str(&format!("Follow the supervision operating instructions block above for harness '{harness}'.\nThis script never starts supervision itself.\n\n"));
    }
    output.push_str("The digest above is complete for this session start. Do NOT re-read\ndata/projects.md, data/daemons.md, data/maintainer.md,\ndata/maintainer-shared.md, data/learnings.md,\nor state/*.meta now - they were just printed in full.\nDo NOT bulk-read data/backlog.md now either: the compact identity/metadata\nlisting was just printed with a pointer for targeted full-body follow-up.\nDo NOT bulk-read state/*.status now either: their bounded tails were just\nprinted with full log paths for targeted follow-up when older wake-event\nhistory is actually needed. Re-reading everything defeats the entire point\nof this command. Re-read a file only if this digest flagged it ABSENT (then\nrebuild or create it per AGENTS.md), its contents looked unparseable/corrupt,\nor an individual full status log is needed for older wake-event history.\n");
    output
}

//! Session-start transport and deterministic supervision rendering.
//!
//! The composed bootstrap, doctor, and snapshot commands enter Rust through
//! the CLI before their compatibility bodies run.  This module owns the two
//! self-contained Portion 09 surfaces that no longer need a shell policy body.

use std::fs;
use std::path::Path;

use multplx_core::process::{ProcessProbe, SystemProcessProbe};

use crate::operational_input::{self, Kind};
use crate::supervision::CommandResult;

const NUDGE_BODY: &str =
    "Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.";

/// Render the native session-start nudge, preserving its fail-open contract.
#[must_use]
pub fn sessionstart_nudge(root: &Path, state: &Path) -> CommandResult {
    if multplx_core::gate_refuse::is_gate_agent(
        std::env::var_os("DEEP_REVIEW_GATE").is_some(),
        std::env::var("MX_GATE_REFUSE_BYPASS").as_deref() == Ok("1"),
    ) || !multplx_core::primary_scope::matches(root, state)
        || lock_is_in_ancestry(state, &SystemProcessProbe::default())
    {
        return CommandResult {
            status: 0,
            stdout: String::new(),
            stderr: String::new(),
        };
    }
    let nudge = operational_input::construct(Kind::SessionStart, NUDGE_BODY)
        .expect("the constant session-start nudge body is non-empty");
    CommandResult {
        status: 0,
        stdout: format!("{nudge}\n"),
        stderr: String::new(),
    }
}

fn lock_is_in_ancestry(state: &Path, processes: &impl ProcessProbe) -> bool {
    let Ok(raw) = fs::read_to_string(state.join(".lock")) else {
        return false;
    };
    let Ok(lock_pid) = raw.lines().next().unwrap_or_default().parse::<u32>() else {
        return false;
    };
    if lock_pid <= 1 || !processes.is_alive(lock_pid) {
        return false;
    }
    let mut pid = std::process::id();
    for _ in 0..8 {
        if pid == lock_pid {
            return true;
        }
        let Ok(row) = processes.ancestry_row(pid) else {
            return false;
        };
        pid = row.parent_pid;
        if pid <= 1 {
            return false;
        }
    }
    false
}

const SUPERVISION_USAGE: &str = "Usage: mx-supervision-instructions.sh [--harness <name>] [--read-only 0|1] [--afk 0|1] [--repair-line] [--queue-pending 0|1]\n\nPrint the current primary harness's supervision operating instructions.\nWith --repair-line, print one concise repair instruction for guard and hook messages.\n";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SupervisionOptions {
    harness: Option<String>,
    read_only: bool,
    afk: bool,
    repair_line: bool,
    queue_pending: bool,
}

fn bool_value(value: &str) -> bool {
    matches!(value, "1" | "true" | "TRUE" | "yes" | "YES")
}

fn supervision_error(message: &str, include_usage: bool) -> CommandResult {
    CommandResult {
        status: 2,
        stdout: String::new(),
        stderr: if include_usage {
            format!("error: {message}\n{SUPERVISION_USAGE}")
        } else {
            format!("error: {message}\n")
        },
    }
}

fn parse_supervision(args: &[String]) -> Result<SupervisionOptions, CommandResult> {
    let mut options = SupervisionOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--harness" | "--read-only" | "--afk" | "--queue-pending" => {
                let name = args[index].clone();
                let Some(value) = args.get(index + 1) else {
                    let requirement = if name == "--harness" {
                        "a value"
                    } else {
                        "0 or 1"
                    };
                    return Err(supervision_error(
                        &format!("{name} requires {requirement}"),
                        false,
                    ));
                };
                match name.as_str() {
                    "--harness" => options.harness = Some(value.clone()),
                    "--read-only" => options.read_only = bool_value(value),
                    "--afk" => options.afk = bool_value(value),
                    "--queue-pending" => options.queue_pending = bool_value(value),
                    _ => unreachable!(),
                }
                index += 2;
            }
            "--repair-line" => {
                options.repair_line = true;
                index += 1;
            }
            "-h" | "--help" => {
                return Err(CommandResult {
                    status: 0,
                    stdout: SUPERVISION_USAGE.to_owned(),
                    stderr: String::new(),
                });
            }
            unknown => {
                return Err(supervision_error(
                    &format!("unknown argument: {unknown}"),
                    true,
                ));
            }
        }
    }
    Ok(options)
}

fn ordinary_wake_line(harness: &str) -> &'static str {
    match harness {
        "claude" => {
            "- Ordinary wake: the Stop-owned auto-arm (bin/mx-claude-stop-autoarm.sh) already owns watcher continuity; drain and handle the wake, and do not arm another cycle yourself."
        }
        "codex" | "cursor" => {
            "- Ordinary wake: take the next foreground bin/mx-watch-checkpoint.sh checkpoint as directed below."
        }
        "pi" => {
            "- Ordinary wake: the Pi extension already owns watcher continuity; do not arm another cycle."
        }
        _ => {
            "- Ordinary wake: follow the continuation in the harness protocol below; do not use shell &."
        }
    }
}

fn repair_line(options: &SupervisionOptions, harness: &str, root: &Path) -> String {
    if options.read_only {
        return "Watcher repair belongs to the session holding the system lock; do not drain, arm, or repair from this read-only session.\n".to_owned();
    }
    if options.afk {
        return "Away mode owns watcher supervision; load /afk and ensure the daemon is running instead of starting normal supervision directly.\n".to_owned();
    }
    let prefix = if options.queue_pending {
        "After draining queued wakes, "
    } else {
        ""
    };
    match harness {
        "claude" => format!(
            "{prefix}repair missing watcher supervision with bin/mx-watch-arm.sh as its own Claude Code background task, never shell &.\n"
        ),
        "codex" | "cursor" => {
            let seconds =
                std::env::var("MX_CODEX_WATCH_CHECKPOINT").unwrap_or_else(|_| "180".to_owned());
            format!(
                "{prefix}repair missing watcher supervision with a foreground checkpoint: bin/mx-watch-checkpoint.sh --seconds {seconds}.\n"
            )
        }
        "pi" => format!(
            "{prefix}repair a missing or failed watcher cycle with the Pi tool mx_watch_arm_pi, or restart Pi with -e {} -e {} if the extensions are not loaded.\n",
            root.join(".pi/extensions/mx-primary-turnend-guard.ts")
                .display(),
            root.join(".pi/extensions/mx-primary-pi-watch.ts").display()
        ),
        _ => format!(
            "{prefix}repair missing watcher supervision according to the session-start block for this harness; do not use shell &.\n"
        ),
    }
}

/// Parse and render the harness-specific supervision block.
#[must_use]
pub fn supervision_instructions(
    args: &[String],
    detected_harness: &str,
    source_root: &Path,
    logical_root: &Path,
) -> CommandResult {
    let options = match parse_supervision(args) {
        Ok(options) => options,
        Err(result) => return result,
    };
    let requested = options.harness.as_deref().unwrap_or(detected_harness);
    let harness = match requested {
        "claude" | "codex" | "cursor" | "pi" => requested,
        _ => "unknown",
    };
    if options.repair_line {
        return CommandResult {
            status: 0,
            stdout: repair_line(&options, harness, logical_root),
            stderr: String::new(),
        };
    }
    let fallback = source_root.join("docs/supervision-protocols/unknown.md");
    let selected = source_root
        .join("docs/supervision-protocols")
        .join(format!("{harness}.md"));
    let snippet_path = if selected.is_file() {
        selected
    } else {
        fallback
    };
    let mut snippet = fs::read_to_string(&snippet_path).unwrap_or_default();
    snippet = snippet.replace(
        "__MX_PI_EXT__",
        &logical_root
            .join(".pi/extensions/mx-primary-pi-watch.ts")
            .to_string_lossy(),
    );
    snippet = snippet.replace(
        "__MX_PI_TURNEND_EXT__",
        &logical_root
            .join(".pi/extensions/mx-primary-turnend-guard.ts")
            .to_string_lossy(),
    );
    if !snippet.ends_with('\n') {
        snippet.push('\n');
    }
    let rule = "================================================================================";
    let lock = if options.read_only {
        "- Lock: read-only; do not drain, arm, spawn, steer, merge, or repair system state here."
    } else {
        "- Lock: held by this session; this session owns normal supervision unless away mode says otherwise."
    };
    let afk = if options.afk {
        "- Away mode: active; load /afk and keep normal harness supervision paused while the daemon owns the watcher."
    } else {
        "- Away mode: inactive."
    };
    let stdout = format!(
        "{rule}\nSUPERVISION OPERATING INSTRUCTIONS - primary harness: {harness}\n{rule}\nCurrent state:\n{lock}\n{afk}\n{}\n\n{snippet}\n",
        ordinary_wake_line(harness)
    );
    CommandResult {
        status: 0,
        stdout,
        stderr: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_preserves_boolean_and_failure_grammar() {
        let parsed = parse_supervision(&[
            "--harness".into(),
            "codex".into(),
            "--read-only".into(),
            "yes".into(),
            "--queue-pending".into(),
            "no".into(),
        ])
        .expect("parse");
        assert_eq!(parsed.harness.as_deref(), Some("codex"));
        assert!(parsed.read_only);
        assert!(!parsed.queue_pending);
        assert_eq!(
            parse_supervision(&["--afk".into()])
                .expect_err("missing")
                .stderr,
            "error: --afk requires 0 or 1\n"
        );
    }

    #[test]
    fn explicit_repair_lines_remain_harness_specific() {
        let temp = tempfile::tempdir().expect("tempdir");
        let result = supervision_instructions(
            &["--harness".into(), "pi".into(), "--repair-line".into()],
            "unknown",
            temp.path(),
            temp.path(),
        );
        assert_eq!(result.status, 0);
        assert!(result.stdout.contains("mx_watch_arm_pi"));
        assert!(result.stdout.contains("mx-primary-turnend-guard.ts"));
    }

    #[test]
    fn ordinary_rendering_and_missing_values_cover_fail_closed_edges() {
        assert_eq!(
            parse_supervision(&["--harness".into()])
                .expect_err("missing harness")
                .stderr,
            "error: --harness requires a value\n"
        );
        let temp = tempfile::tempdir().expect("tempdir");
        let protocols = temp.path().join("docs/supervision-protocols");
        fs::create_dir_all(&protocols).expect("protocols");
        fs::write(
            protocols.join("unknown.md"),
            "Unknown protocol without newline",
        )
        .expect("protocol");
        let result = supervision_instructions(
            &[
                "--harness".into(),
                "future".into(),
                "--read-only".into(),
                "1".into(),
                "--afk".into(),
                "TRUE".into(),
            ],
            "unknown",
            temp.path(),
            temp.path(),
        );
        assert_eq!(result.status, 0);
        assert!(result.stdout.contains("primary harness: unknown"));
        assert!(result.stdout.contains("- Lock: read-only"));
        assert!(result.stdout.contains("- Away mode: active"));
        assert!(
            result
                .stdout
                .ends_with("Unknown protocol without newline\n\n")
        );
    }
}

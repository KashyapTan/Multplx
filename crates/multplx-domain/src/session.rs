//! Session-start transport and deterministic supervision rendering.
//!
//! The CLI owns the native composed session-start transaction.
//! This module owns its reusable supervision surfaces.

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

/// Read-only proof that this owner's native SessionStart hook registered.
/// The receipt does not acquire the lock or authorize another thread's bridge.
#[must_use]
pub fn codex_hook_ready(state: &Path) -> bool {
    let Ok(current_thread) = std::env::var("CODEX_THREAD_ID") else {
        return false;
    };
    codex_hook_ready_for_thread(state, &current_thread)
}

/// Verify readiness for an exact thread supplied by a trusted native hook.
/// Renderers use `codex_hook_ready` to require their own current thread ID.
#[must_use]
pub fn codex_hook_ready_for_thread(state: &Path, current_thread: &str) -> bool {
    let Ok(bytes) = multplx_core::filesystem::read_bounded_regular(
        state.join(".codex-idle-hook-ready.json"),
        16 * 1024,
    ) else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    let Some(thread) = receipt.get("thread").and_then(serde_json::Value::as_str) else {
        return false;
    };
    if thread.len() != 36
        || !thread.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return false;
    }
    if current_thread != thread {
        return false;
    }
    let Some(home) = receipt
        .get("codex_home")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    let actual_home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".codex"))
        });
    if actual_home
        .and_then(|path| fs::canonicalize(path).ok())
        .as_deref()
        != Some(Path::new(home))
    {
        return false;
    }
    if !receipt
        .get("executable")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|path| Path::new(path).is_absolute())
    {
        return false;
    }
    let Some(owner) = receipt.get("owner").cloned().and_then(|value| {
        serde_json::from_value::<multplx_core::process::ProcessIdentity>(value).ok()
    }) else {
        return false;
    };
    let probe = SystemProcessProbe::default();
    if !fs::read_to_string(state.join(".lock"))
        .is_ok_and(|text| text.trim() == owner.pid.to_string())
        || !probe
            .identity(owner.pid)
            .is_ok_and(|current| current == owner)
    {
        return false;
    }
    let mut pid = std::process::id();
    for _ in 0..12 {
        if pid == owner.pid {
            return true;
        }
        let Ok(row) = probe.ancestry_row(pid) else {
            return false;
        };
        if row.parent_pid <= 1 {
            return false;
        }
        pid = row.parent_pid;
    }
    false
}

fn codex_idle_activated(root: &Path) -> bool {
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("MX_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| root.to_owned())
                .join("state")
        });
    std::env::var("MX_CODEX_IDLE_CLI").as_deref() == Ok("1") && codex_hook_ready(&state)
}

fn ordinary_wake_line(harness: &str, codex_active: bool) -> &'static str {
    match harness {
        "claude" => {
            "- Ordinary wake: the Stop-owned auto-arm (bin/mx-claude-stop-autoarm.sh) already owns watcher continuity; claim the wake, record its disposition, then acknowledge it. Do not arm another cycle yourself. See `mx wake --help`."
        }
        "codex" if codex_active => {
            "- Ordinary wake: the Stop-owned Codex exact-thread queue bridge owns watcher continuity; claim the wake, record its durable disposition, acknowledge it, then end the handling turn. Foreground checkpoints are only the explicit fallback when queue support is unavailable. See `mx wake --help`."
        }
        "codex" => {
            "- Ordinary wake: the Codex queue bridge is inactive or native hooks are not ready here; claim the wake, record its durable disposition, acknowledge it, then take the next bounded foreground bin/mx-watch-checkpoint.sh checkpoint. Complete the provider's native hook trust review when required. Desktop event delivery is unverified. See `mx wake --help`."
        }
        "pi" => {
            "- Ordinary wake: the Pi extension already owns watcher continuity; claim the wake, record its disposition, then acknowledge it. Do not arm another cycle. See `mx wake --help`."
        }
        "cursor" => {
            "- Ordinary wake: the Cursor stop hook owns watcher continuity; claim the wake, record its durable disposition, then acknowledge it. End the handling turn; the hook parks automatically without a foreground checkpoint. See `mx wake --help`."
        }
        _ => {
            "- Ordinary wake: claim the wake, record its disposition, acknowledge it, then follow the continuation in the harness protocol below; do not use shell &. See `mx wake --help`."
        }
    }
}

fn repair_line(
    options: &SupervisionOptions,
    harness: &str,
    root: &Path,
    codex_active: bool,
) -> String {
    if options.read_only {
        return "Watcher repair belongs to the session holding the system lock; do not drain, arm, or repair from this read-only session.\n".to_owned();
    }
    if options.afk {
        return "Away mode owns watcher supervision; load /afk and ensure the daemon is running instead of starting normal supervision directly.\n".to_owned();
    }
    let prefix = if options.queue_pending {
        "After claiming queued wakes and durably recording disposition plus acknowledgement, "
    } else {
        ""
    };
    match harness {
        "claude" => format!(
            "{prefix}repair missing watcher supervision with bin/mx-watch-arm.sh as its own Claude Code background task, never shell &.\n"
        ),
        "codex" => {
            let seconds =
                std::env::var("MX_CODEX_WATCH_CHECKPOINT").unwrap_or_else(|_| "180".to_owned());
            if codex_active {
                format!(
                    "{prefix}inspect state/.codex-idle-failure and reconcile uncertain queue submission before explicitly running bin/mx-codex-idle.sh --retry from the same lock-owning Codex session with its exact CODEX_THREAD_ID. If queue support is unavailable, use the explicit foreground fallback: bin/mx-watch-checkpoint.sh --seconds {seconds}.\n"
                )
            } else {
                format!(
                    "{prefix}the Codex queue bridge is inactive or native hooks are not ready here; restore bounded foreground supervision with bin/mx-watch-checkpoint.sh --seconds {seconds}. Managed Multplx CLI launches set activation automatically; direct Codex CLI requires explicit MX_CODEX_IDLE_CLI=1 opt-in. Complete the provider's native hook trust review and ensure hooks are enabled; a matched SessionStart readiness receipt is required before queue delivery. Desktop event delivery is unverified.\n"
                )
            }
        }
        "pi" => format!(
            "{prefix}repair a missing or failed watcher cycle with the Pi tool mx_watch_arm_pi, or restart Pi with -e {} -e {} if the extensions are not loaded.\n",
            root.join(".pi/extensions/mx-primary-turnend-guard.ts")
                .display(),
            root.join(".pi/extensions/mx-primary-pi-watch.ts").display()
        ),
        "cursor" => format!(
            "{prefix}inspect the Cursor stop-hook watcher failure and repair its cause; hook-owned supervision resumes at turn end. Use interactive agent --trust so project hooks load; do not enter a foreground checkpoint loop.\n"
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
    let codex_active = harness == "codex" && codex_idle_activated(logical_root);
    if options.repair_line {
        return CommandResult {
            status: 0,
            stdout: repair_line(&options, harness, logical_root, codex_active),
            stderr: String::new(),
        };
    }
    let fallback = source_root.join("docs/supervision-protocols/unknown.md");
    let selected = source_root.join("docs/supervision-protocols").join(
        if harness == "codex" && !codex_active {
            "codex-inactive.md".to_owned()
        } else {
            format!("{harness}.md")
        },
    );
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
        ordinary_wake_line(harness, codex_active)
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

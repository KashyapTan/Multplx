//! Portion 08 reporting and hook-domain state transitions.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use multplx_core::filesystem::append_single_write;
use multplx_core::identifiers::TaskId;
use multplx_core::journal::{JournalEvent, JournalWriter};
use multplx_core::process::{ProcessProbe, SystemProcessProbe};
use rustix::process::{Pid, Signal, kill_process};
use serde_json::json;
use time::OffsetDateTime;

/// Closed actor-writable status vocabulary.
pub const REPORT_STATES: &[&str] = &[
    "working",
    "paused",
    "blocked",
    "needs-decision",
    "done",
    "failed",
    "resolved",
];

/// One command result with separately rendered streams.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandResult {
    fn success(stdout: impl Into<String>) -> Self {
        Self {
            status: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    fn error(status: i32, stderr: impl Into<String>) -> Self {
        Self {
            status,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
}

const REPORT_USAGE: &str = "Append one validated, task-bound status event.\n\nUsage:\n  mx-report --id <task-id> --state <state> --message <one-line-message> [--key <slug>]\n  mx-report --list-states\n\nThe closed actor-writable state vocabulary lives in the Rust report command.\nA write is accepted only when the caller is bound to the same task.\nState directory precedence is MX_REPORT_STATE_OVERRIDE, MX_STATE_OVERRIDE, MX_HOME/state, then repo/state.\n";

#[derive(Default)]
struct ReportOptions {
    id: Option<String>,
    state: Option<String>,
    message: Option<String>,
    key: Option<String>,
    list: bool,
}

fn usage_error(message: &str) -> CommandResult {
    CommandResult::error(2, format!("mx-report: {message}\n{REPORT_USAGE}"))
}

fn binding_error(message: &str) -> CommandResult {
    CommandResult::error(3, format!("mx-report: {message}\n"))
}

fn parse_report(args: &[String]) -> Result<ReportOptions, CommandResult> {
    let mut parsed = ReportOptions::default();
    let mut index = 0;
    while index < args.len() {
        let option = &args[index];
        let value = |name: &str, index: &mut usize| -> Result<String, CommandResult> {
            let Some(value) = args.get(*index + 1) else {
                return Err(usage_error(&format!("{name} requires a value")));
            };
            *index += 2;
            Ok(value.clone())
        };
        match option.as_str() {
            "--id" => parsed.id = Some(value("--id", &mut index)?),
            "--state" => parsed.state = Some(value("--state", &mut index)?),
            "--message" => parsed.message = Some(value("--message", &mut index)?),
            "--key" => parsed.key = Some(value("--key", &mut index)?),
            "--list-states" => {
                parsed.list = true;
                index += 1;
            }
            "-h" | "--help" => return Err(CommandResult::success(REPORT_USAGE)),
            _ => return Err(usage_error(&format!("unknown argument '{option}'"))),
        }
    }
    Ok(parsed)
}

fn state_directory(root: &Path) -> PathBuf {
    env::var_os("MX_REPORT_STATE_OVERRIDE")
        .or_else(|| env::var_os("MX_STATE_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("MX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.to_path_buf())
                .join("state")
        })
}

fn bound_task(state: &Path) -> Result<TaskId, CommandResult> {
    if let Some(raw) = env::var_os("MX_TASK_ID").filter(|value| !value.is_empty()) {
        return TaskId::parse(raw.to_string_lossy().into_owned())
            .map_err(|_| binding_error("calling session has an invalid task binding"));
    }
    let cwd = fs::canonicalize(".")
        .map_err(|_| binding_error("could not resolve the calling session cwd"))?;
    let mut matches = Vec::new();
    let entries = match fs::read_dir(state) {
        Ok(entries) => entries,
        Err(_) => {
            return Err(binding_error(
                "no task binding found; MX_TASK_ID is unset and cwd matches no recorded worktree",
            ));
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("meta") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let Ok(task) = TaskId::parse(stem) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(worktree) = text.lines().find_map(|line| line.strip_prefix("worktree=")) else {
            continue;
        };
        let Ok(worktree) = fs::canonicalize(worktree) else {
            continue;
        };
        if cwd == worktree || cwd.starts_with(&worktree) {
            matches.push(task);
        }
    }
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(binding_error(
            "no task binding found; MX_TASK_ID is unset and cwd matches no recorded worktree",
        )),
        _ => Err(binding_error(
            "task binding is ambiguous; cwd matches more than one recorded worktree",
        )),
    }
}

fn timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn nudge_watcher(state: &Path) -> Option<String> {
    let debug = env::var("MX_NUDGE_DEBUG").as_deref() == Ok("1");
    let result = (|| {
        if env::var("MX_NUDGE").as_deref() == Ok("0") {
            return Err("disabled by MX_NUDGE=0");
        }
        let lock = state.join(".watch.lock");
        let pid_text =
            fs::read_to_string(lock.join("pid")).map_err(|_| "no valid watcher pid advertised")?;
        let pid = pid_text
            .trim()
            .parse::<u32>()
            .map_err(|_| "no valid watcher pid advertised")?;
        let stored = fs::read_to_string(lock.join("pid-identity"))
            .map_err(|_| "no watcher pid identity advertised")?;
        let probe = SystemProcessProbe::default();
        if !probe.is_alive(pid) {
            return Err("advertised watcher pid is not alive");
        }
        let current = probe
            .identity(pid)
            .map_err(|_| "could not identify advertised watcher pid")?;
        if current.marker != stored.trim_end_matches('\n') {
            return Err("advertised watcher pid identity does not match");
        }
        let raw = i32::try_from(pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or("no valid watcher pid advertised")?;
        kill_process(raw, Signal::USR1).map_err(|_| "USR1 delivery failed")?;
        Ok(format!("sent USR1 to watcher pid {pid}"))
    })();
    debug.then(|| match result {
        Ok(message) => format!("mx-report: watcher nudge: {message}\n"),
        Err(message) => format!("mx-report: watcher nudge: {message}\n"),
    })
}

/// Run the Rust status reporter with exact public grammar and binding rules.
#[must_use]
pub fn report(args: &[String], root: &Path) -> CommandResult {
    let parsed = match parse_report(args) {
        Ok(parsed) => parsed,
        Err(result) => return result,
    };
    if parsed.list {
        if parsed.id.is_some()
            || parsed.state.is_some()
            || parsed.message.is_some()
            || parsed.key.is_some()
        {
            return usage_error("--list-states cannot be combined with write arguments");
        }
        return CommandResult::success(format!("{}\n", REPORT_STATES.join("\n")));
    }
    let Some(raw_id) = parsed.id else {
        return usage_error("--id is required");
    };
    let task = match TaskId::parse(&raw_id) {
        Ok(task) => task,
        Err(_) => return usage_error(&format!("invalid task id '{raw_id}'")),
    };
    let Some(state_name) = parsed.state else {
        return usage_error("--state is required");
    };
    let Some(message) = parsed.message else {
        return usage_error("--message is required");
    };
    if !REPORT_STATES.contains(&state_name.as_str()) {
        return CommandResult::error(
            2,
            format!(
                "mx-report: invalid state '{state_name}'. Valid states: {}\n",
                REPORT_STATES.join(", ")
            ),
        );
    }
    if message.contains(['\n', '\r']) {
        return CommandResult::error(2, "mx-report: message must be exactly one line\n");
    }
    if parsed.key.as_deref().is_some_and(|key| {
        key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    }) {
        return CommandResult::error(
            2,
            format!(
                "mx-report: invalid key '{}'. Keys may contain only A-Z, a-z, 0-9, dot, underscore, and dash\n",
                parsed.key.as_deref().unwrap_or_default()
            ),
        );
    }
    let state = state_directory(root);
    let bound = match bound_task(&state) {
        Ok(bound) => bound,
        Err(result) => return result,
    };
    if bound != task {
        return binding_error(&format!(
            "task binding mismatch: calling session is '{bound}', requested '{task}'"
        ));
    }
    if !state.is_dir() {
        return binding_error(&format!(
            "status state directory does not exist: {}",
            state.display()
        ));
    }
    let line = parsed.key.as_ref().map_or_else(
        || format!("{state_name}: {message}"),
        |key| format!("{state_name} [key={key}]: {message}"),
    );
    if let Err(error) = append_single_write(
        state.join(format!("{}.status", task.as_str())),
        format!("{line}\n").as_bytes(),
        0o600,
    ) {
        return CommandResult::error(1, format!("mx-report: {error}\n"));
    }
    let detail = json!({"raw":line,"state":state_name,"validated":true});
    let writer = JournalWriter::new(&state);
    let mut stderr = writer
        .try_emit(
            &task,
            JournalEvent::StatusReported,
            &detail,
            "mx-report",
            &timestamp(),
        )
        .map(|warning| format!("{warning}\n"))
        .unwrap_or_default();
    if let Some(debug) = nudge_watcher(&state) {
        stderr.push_str(&debug);
    }
    CommandResult {
        status: 0,
        stdout: String::new(),
        stderr,
    }
}

const SUBAGENT_USAGE: &str = "Usage: mx-subagent-pretool-check.sh [--tool <tool-name>] [--claude]\n\nWith no --tool, reads a PreToolUse-style JSON payload on stdin (Claude/Codex tool_name).\nDenies a delegation-SHAPED tool name in a genuine primary home.\n";

/// Run the primary-session delegation-shape guard without shell parsing or jq.
#[must_use]
pub fn subagent_guard(args: &[String], payload: &str, root: &Path) -> CommandResult {
    let mut tool = None;
    let mut claude = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--tool" => {
                let Some(value) = args.get(index + 1) else {
                    return CommandResult::error(2, "error: --tool requires a value\n");
                };
                tool = Some(value.clone());
                index += 2;
            }
            value if value.starts_with("--tool=") => {
                tool = Some(value[7..].to_owned());
                index += 1;
            }
            "--claude" => {
                claude = true;
                index += 1;
            }
            "-h" | "--help" => return CommandResult::success(SUBAGENT_USAGE),
            unknown => {
                return CommandResult::error(
                    2,
                    format!("error: unknown argument: {unknown}\n{SUBAGENT_USAGE}"),
                );
            }
        }
    }
    let stdin_mode = tool.is_none();
    if stdin_mode && !program_available("jq") {
        return CommandResult::success("");
    }
    let tool = tool.or_else(|| {
        serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|value| {
                value
                    .get("tool_name")
                    .or_else(|| value.get("toolName"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
    });
    let Some(tool) = tool.filter(|tool| !tool.is_empty()) else {
        return CommandResult::success("");
    };
    if tool.starts_with("mcp__") {
        return CommandResult::success("");
    }
    let normalized = tool
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    const OBSERVE: &[&str] = &[
        "taskoutput",
        "taskstop",
        "taskget",
        "tasklist",
        "cronlist",
        "bashoutput",
        "killshell",
    ];
    if OBSERVE.contains(&normalized.as_str()) {
        return CommandResult::success("");
    }
    const STEMS: &[&str] = &[
        "agent",
        "subagent",
        "task",
        "workflow",
        "cron",
        "schedul",
        "worktree",
        "delegate",
        "spawn",
        "dispatch",
        "handoff",
        "remote",
        "sendmessage",
        "monitor",
    ];
    let Some(stem) = STEMS.iter().find(|stem| normalized.contains(**stem)) else {
        return CommandResult::success("");
    };
    if env::var("MX_ALLOW_SUBAGENT").as_deref() == Ok("1") {
        return CommandResult::success("");
    }
    let home = env::var_os("MX_HOME")
        .or_else(|| env::var_os("MX_ROOT_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_path_buf());
    let state = env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if !multplx_core::primary_scope::matches(root, &state) {
        return CommandResult::success("");
    }
    let route = if root.join("bin/mx-scout.sh").is_file() {
        "first classify the work under the AGENTS.md intake contract: work already classified as a scout goes to bin/mx-scout.sh \"<question>\" [project], while authorized delivery work and its bounded research go to bin/mx-brief.sh then bin/mx-spawn.sh"
    } else {
        "first classify the work under the AGENTS.md intake contract, then use bin/mx-brief.sh followed by bin/mx-spawn.sh for dispatched work"
    };
    let reason = format!(
        "[subagent-dispatch] the broker primary dispatches through the system, not the harness's own delegation tools: work started that way has no durable system record, leaves every broker guard inert, and dies with this session. Instead, {route} (blocked tool: {tool}, delegation-shaped on \"{stem}\"). Launch the session with MX_ALLOW_SUBAGENT=1 for a deliberate exception."
    );
    let stderr = format!(
        "{}\n",
        serde_json::to_string(&json!({
            "hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny"},
            "systemMessage":reason
        }))
        .unwrap_or_default()
    );
    let stdout = if claude {
        String::new()
    } else {
        format!(
            "{}\n",
            serde_json::to_string(&json!({"decision":"deny","reason":reason})).unwrap_or_default()
        )
    };
    CommandResult {
        status: 2,
        stdout,
        stderr,
    }
}

fn program_available(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|directory| directory.join(name).is_file())
    })
}

/// Which shell-command policy a pre-tool transport applies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PretoolPolicy {
    WatcherArm,
    PersistentCd,
}

fn pretool_usage(policy: PretoolPolicy) -> &'static str {
    match policy {
        PretoolPolicy::WatcherArm => {
            "Usage: mx-arm-pretool-check.sh [--command <cmd>] [--background true|false] [--claude]\n"
        }
        PretoolPolicy::PersistentCd => {
            "Usage: mx-cd-pretool-check.sh [--command <cmd>] [--claude]\n"
        }
    }
}

fn cd_primary_scope(root: &Path) -> bool {
    if !root.join("AGENTS.md").is_file() || !root.join("bin").is_dir() {
        return false;
    }
    let value = |argument: &str| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", argument])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|text| text.trim().to_owned())
    };
    value("--git-dir")
        .zip(value("--git-common-dir"))
        .is_some_and(|(git, common)| git == common)
}

/// Run either shell-command policy transport with the established hook shapes.
#[must_use]
pub fn pretool_guard(
    policy: PretoolPolicy,
    args: &[String],
    payload: &str,
    root: &Path,
) -> CommandResult {
    let mut command = None;
    let mut claude = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--command" => {
                let Some(value) = args.get(index + 1) else {
                    return CommandResult::error(2, "error: --command requires a value\n");
                };
                command = Some(value.clone());
                index += 2;
            }
            value if value.starts_with("--command=") => {
                command = Some(value[10..].to_owned());
                index += 1;
            }
            "--background" => {
                if args.get(index + 1).is_none() {
                    return CommandResult::error(2, "error: --background requires a value\n");
                }
                index += 2;
            }
            value if value.starts_with("--background=") => index += 1,
            "--claude" => {
                claude = true;
                index += 1;
            }
            "-h" | "--help" => return CommandResult::success(pretool_usage(policy)),
            unknown => {
                return CommandResult::error(
                    2,
                    format!(
                        "error: unknown argument: {unknown}\n{}",
                        pretool_usage(policy)
                    ),
                );
            }
        }
    }
    let stdin_mode = command.is_none();
    if stdin_mode && (!program_available("jq") || payload.is_empty()) {
        return CommandResult::success("");
    }
    let command = command.or_else(|| {
        serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|value| {
                value
                    .pointer("/toolInput/command")
                    .or_else(|| value.pointer("/tool_input/command"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
    });
    let Some(command) = command.filter(|command| !command.is_empty()) else {
        return CommandResult::success("");
    };
    let denial = match policy {
        PretoolPolicy::WatcherArm => multplx_core::command_policy::watcher_arm(&command).err(),
        PretoolPolicy::PersistentCd => {
            if !cd_primary_scope(root) || !multplx_core::command_policy::persistent_cd(&command) {
                None
            } else {
                Some(multplx_core::command_policy::Denial {
                    code: "persistent-cd",
                    reason: "a persistent top-level directory change in the primary Multplx checkout is blocked; it would move the shell out of the home so a later broker-owned command runs inside a project clone. Reach the target without moving the shell - use git -C <dir> or an absolute path on the command itself - or scope the cd to a subshell like (cd <dir> && ...).",
                })
            }
        }
    };
    let Some(denial) = denial else {
        return CommandResult::success("");
    };
    let detail = format!("[{}] {}", denial.code, denial.reason);
    let stderr = format!(
        "{}\n",
        serde_json::to_string(&json!({
            "hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny"},
            "systemMessage":detail
        }))
        .unwrap_or_default()
    );
    let stdout = if claude {
        String::new()
    } else {
        format!(
            "{}\n",
            serde_json::to_string(&json!({"decision":"deny","reason":detail})).unwrap_or_default()
        )
    };
    CommandResult {
        status: 2,
        stdout,
        stderr,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use super::{PretoolPolicy, REPORT_STATES, pretool_guard, report, subagent_guard};

    fn primary_fixture() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("state")).expect("state");
        fs::create_dir(temp.path().join("bin")).expect("bin");
        fs::write(temp.path().join("AGENTS.md"), "# fixture\n").expect("agents");
        let output = Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(["init", "--quiet"])
            .output()
            .expect("git init");
        assert!(output.status.success());
        temp
    }

    #[test]
    fn list_states_is_the_exact_closed_vocabulary() {
        let result = report(&["--list-states".to_owned()], Path::new("/unused"));
        assert_eq!(result.status, 0);
        assert_eq!(result.stdout, format!("{}\n", REPORT_STATES.join("\n")));
    }

    #[test]
    fn report_usage_and_validation_refuse_before_binding() {
        let cases = [
            vec![],
            vec!["--unknown"],
            vec!["--id"],
            vec!["--id", "bad/id", "--state", "done", "--message", "ok"],
            vec!["--id", "task", "--state", "other", "--message", "ok"],
            vec!["--id", "task", "--state", "done", "--message", "two\nlines"],
            vec![
                "--id",
                "task",
                "--state",
                "done",
                "--message",
                "ok",
                "--key",
                "bad/key",
            ],
            vec!["--list-states", "--id", "task"],
            vec!["--id", "task"],
            vec!["--id", "task", "--state", "done"],
        ];
        for args in cases {
            let values = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert_ne!(report(&values, Path::new("/unused")).status, 0);
        }
        let help = report(&["--help".to_owned()], Path::new("/unused"));
        assert_eq!(help.status, 0);
        assert!(help.stdout.contains("Usage:"));
    }

    #[test]
    fn subagent_guard_covers_cli_transport_scope_and_output_modes() {
        let root = primary_fixture();
        let deny = subagent_guard(
            &["--tool".into(), "FutureAgentDispatch".into()],
            "",
            root.path(),
        );
        assert_eq!(deny.status, 2);
        assert!(deny.stdout.contains("subagent-dispatch"));
        assert!(deny.stderr.contains("FutureAgentDispatch"));

        let claude = subagent_guard(
            &["--tool=TaskCreate".into(), "--claude".into()],
            "",
            root.path(),
        );
        assert_eq!(claude.status, 2);
        assert!(claude.stdout.is_empty());
        for tool in ["mcp__server__spawn_agent", "TaskOutput", "Bash"] {
            assert_eq!(
                subagent_guard(&["--tool".into(), tool.into()], "", root.path()).status,
                0
            );
        }
        assert_eq!(
            subagent_guard(&["--tool".into()], "", root.path()).status,
            2
        );
        assert_eq!(subagent_guard(&["--bad".into()], "", root.path()).status, 2);
        assert_eq!(
            subagent_guard(&["--help".into()], "", root.path()).status,
            0
        );
        assert_eq!(subagent_guard(&[], "not-json", root.path()).status, 0);
        assert_eq!(
            subagent_guard(&[], r#"{"toolName":"Agent"}"#, root.path()).status,
            2
        );

        let outside = tempfile::tempdir().expect("outside");
        assert_eq!(
            subagent_guard(&["--tool".into(), "Agent".into()], "", outside.path(),).status,
            0
        );
        fs::write(root.path().join("bin/mx-scout.sh"), "#!/bin/sh\n").expect("scout");
        let scout_route = subagent_guard(&["--tool".into(), "Agent".into()], "", root.path());
        assert!(scout_route.stderr.contains("mx-scout.sh"));
    }

    #[test]
    fn pretool_guard_covers_both_policies_and_transport_grammar() {
        let root = primary_fixture();
        let arm = pretool_guard(
            PretoolPolicy::WatcherArm,
            &["--command".into(), "bin/mx-watch-arm.sh &".into()],
            "",
            root.path(),
        );
        assert_eq!(arm.status, 2);
        assert!(arm.stderr.contains("watcher-background"));
        let cd = pretool_guard(
            PretoolPolicy::PersistentCd,
            &["--command=cd projects/app".into(), "--claude".into()],
            "",
            root.path(),
        );
        assert_eq!(cd.status, 2);
        assert!(cd.stdout.is_empty());
        assert_eq!(
            pretool_guard(
                PretoolPolicy::PersistentCd,
                &["--command".into(), "git -C projects/app status".into()],
                "",
                root.path(),
            )
            .status,
            0
        );
        for args in [vec!["--command"], vec!["--background"], vec!["--unknown"]] {
            let values = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(
                pretool_guard(PretoolPolicy::WatcherArm, &values, "", root.path()).status,
                2
            );
        }
        assert_eq!(
            pretool_guard(
                PretoolPolicy::WatcherArm,
                &["--help".into()],
                "",
                root.path(),
            )
            .status,
            0
        );
        assert_eq!(
            pretool_guard(PretoolPolicy::WatcherArm, &[], "", root.path()).status,
            0
        );
        assert_eq!(
            pretool_guard(
                PretoolPolicy::PersistentCd,
                &["--help".into()],
                "",
                root.path(),
            )
            .status,
            0
        );
        assert_eq!(
            pretool_guard(
                PretoolPolicy::WatcherArm,
                &["--background=true".into(), "--command=".into()],
                "",
                root.path(),
            )
            .status,
            0
        );
        let outside = tempfile::tempdir().expect("outside");
        assert_eq!(
            pretool_guard(
                PretoolPolicy::PersistentCd,
                &["--command=cd projects/app".into()],
                "",
                outside.path(),
            )
            .status,
            0
        );
    }
}

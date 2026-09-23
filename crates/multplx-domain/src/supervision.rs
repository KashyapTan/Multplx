//! Portion 08 reporting and hook-domain state transitions.

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use multplx_core::filesystem::append_single_write;
use multplx_core::identifiers::TaskId;
use multplx_core::journal::{JournalEvent, JournalWriter};
use multplx_core::process::{ProcessProbe, SystemProcessProbe};
use rustix::process::{Pid, Signal, kill_process};
use serde_json::json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

// A hook can briefly queue behind another hook that is itself waiting for the
// shared transition lock. Keep that bounded wait tolerant of two-hop
// contention without turning a persistently held lock into a long stall.
const SUPERVISION_LOCK_TIMEOUT: Duration = Duration::from_secs(2);

fn recoverable_transition_wait(
    state: &Path,
    operation: &str,
    writes: &[multplx_core::filesystem::TransitionWrite],
) -> Result<(), String> {
    let started = Instant::now();
    loop {
        match multplx_core::filesystem::recoverable_transition(state, operation, writes, None) {
            Ok(()) => return Ok(()),
            Err(multplx_core::error::CoreError::LockHeld { .. })
                if started.elapsed() < SUPERVISION_LOCK_TIMEOUT =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn recover_transition_wait(state: &Path, operation: &str) -> Result<(), String> {
    let started = Instant::now();
    loop {
        match multplx_core::filesystem::recover_transition(state, operation) {
            Ok(()) => return Ok(()),
            Err(multplx_core::error::CoreError::LockHeld { .. })
                if started.elapsed() < SUPERVISION_LOCK_TIMEOUT =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Closed sub-agent-writable status vocabulary.
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

const REPORT_USAGE: &str = "Append one validated, task-bound status event.\n\nUsage:\n  mx-report --id <task-id> --state <state> --message <one-line-message> [--key <slug>] [--workflow-revision <id>]\n  mx-report --list-states\n\nThe closed sub-agent-writable state vocabulary lives in the Rust report command. Canonical needs-decision reports require --key and record the message as a revision-bound human question.\nA write is accepted only when the caller is bound to the same task. Canonical tasks require --attempt-id, --generation and --brief-revision (or MX_ATTEMPT_ID, MX_ATTEMPT_GENERATION and MX_BRIEF_REVISION). Optional --message-id preserves retry identity; --correlation-id binds a request/reply chain; --artifact binds result evidence. Stale evidence is retained and rejected. A done status is implementation completion only when current typed delivery evidence matches the bound checkout HEAD, or a report/coordination assignment supplies an existing regular-file --artifact. Status prose alone does not release dependencies; checks, review, PR readiness and human merge remain separate.\nState directory precedence is MX_REPORT_STATE_OVERRIDE, MX_STATE_OVERRIDE, MX_HOME/state, then repo/state.\n";

#[derive(Default)]
struct ReportOptions {
    id: Option<String>,
    state: Option<String>,
    message: Option<String>,
    key: Option<String>,
    workflow_revision: Option<String>,
    attempt_id: Option<String>,
    generation: Option<String>,
    brief_revision: Option<String>,
    message_id: Option<String>,
    correlation_id: Option<String>,
    artifact: Option<String>,
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
            "--attempt-id" => parsed.attempt_id = Some(value("--attempt-id", &mut index)?),
            "--generation" => parsed.generation = Some(value("--generation", &mut index)?),
            "--brief-revision" => {
                parsed.brief_revision = Some(value("--brief-revision", &mut index)?)
            }
            "--correlation-id" => {
                parsed.correlation_id = Some(value("--correlation-id", &mut index)?)
            }
            "--message-id" => parsed.message_id = Some(value("--message-id", &mut index)?),
            "--artifact" => parsed.artifact = Some(value("--artifact", &mut index)?),
            "--key" => parsed.key = Some(value("--key", &mut index)?),
            "--workflow-revision" => {
                parsed.workflow_revision = Some(value("--workflow-revision", &mut index)?)
            }
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

fn validate_report_home(
    record: &crate::lifecycle::subagent_model::TaskRecord,
    state: &Path,
) -> Result<(), String> {
    let home = record
        .owner_home
        .as_deref()
        .ok_or("canonical task has no owning home")?;
    let owned_state = record
        .owner_state
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(home).join("state"));
    let expected =
        fs::canonicalize(owned_state).map_err(|_| "canonical task owning state is unavailable")?;
    let actual = fs::canonicalize(state).map_err(|_| "report state is unavailable")?;
    if expected != actual {
        return Err("report recipient home does not own this task".into());
    }
    Ok(())
}

fn report_wake_payload(envelope: &crate::lifecycle::subagent_model::MessageEnvelope) -> String {
    format!(
        "report: message={} task={} state={} brief={} summary={}",
        envelope.message_id,
        envelope.task_id,
        envelope.kind,
        envelope.brief_revision.unwrap_or_default(),
        envelope.summary
    )
}

fn publish_report_wake(
    state: &Path,
    envelope: &crate::lifecycle::subagent_model::MessageEnvelope,
) -> Result<(), String> {
    crate::operational_input::publish_message_wake(
        state,
        envelope,
        &report_wake_payload(envelope),
        SystemTime::now(),
        &SystemProcessProbe::default(),
    )
    .map(|_| ())
}

/// Repair the report-commit to wake-publication window from retained accepted
/// evidence. Watcher startup invokes this before it begins signal ingestion.
pub fn reconcile_report_wakes(state: &Path) -> Result<usize, String> {
    let mut paths = fs::read_dir(state.join("evidence"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut reconciled = 0;
    let mut failures = 0usize;
    let mut first_error = None;
    for path in paths {
        let Ok(bytes) = multplx_core::filesystem::read_bounded_regular(&path, 1024 * 1024) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if value["accepted"] != true || value.get("envelope").is_none() {
            continue;
        }
        let envelope: crate::lifecycle::subagent_model::MessageEnvelope =
            match serde_json::from_value(value["envelope"].clone()) {
                Ok(envelope) => envelope,
                Err(error) => {
                    failures += 1;
                    first_error.get_or_insert_with(|| {
                        format!("{}: malformed retained envelope: {error}", path.display())
                    });
                    continue;
                }
            };
        let mut repaired = true;
        if let Some(frozen) = value.get("parent_outcome") {
            match serde_json::from_value::<crate::lifecycle::parent_channel::ParentOutcome>(
                frozen.clone(),
            ) {
                Ok(frozen) => {
                    if let Err(error) =
                        crate::lifecycle::parent_channel::persist_prepared(state, &frozen)
                    {
                        repaired = false;
                        failures += 1;
                        first_error.get_or_insert_with(|| {
                            format!("{}: cannot repair parent outcome: {error}", path.display())
                        });
                    }
                }
                Err(error) => {
                    repaired = false;
                    failures += 1;
                    first_error.get_or_insert_with(|| {
                        format!(
                            "{}: malformed retained parent outcome: {error}",
                            path.display()
                        )
                    });
                }
            }
        } else {
            // Pre-Phase-05 accepted evidence has no frozen route.  Repair it
            // when it is still current, but never reinterpret a historical
            // event using a replacement generation.
            let _ = crate::lifecycle::parent_channel::record_report(state, &envelope);
        }
        if let Err(error) = publish_report_wake(state, &envelope) {
            repaired = false;
            failures += 1;
            first_error.get_or_insert_with(|| {
                format!("{}: cannot repair report wake: {error}", path.display())
            });
        }
        if repaired {
            reconciled += 1;
        }
    }
    if failures > 0 {
        return Err(format!(
            "{failures} accepted report repair operation(s) remain pending; first: {}",
            first_error.unwrap_or_else(|| "unknown repair failure".to_owned())
        ));
    }
    Ok(reconciled)
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
            || parsed.workflow_revision.is_some()
            || parsed.attempt_id.is_some()
            || parsed.generation.is_some()
            || parsed.brief_revision.is_some()
            || parsed.message_id.is_some()
            || parsed.correlation_id.is_some()
            || parsed.artifact.is_some()
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
    if parsed.workflow_revision.as_deref().is_some_and(|revision| {
        revision.is_empty() || revision.len() > 256 || revision.contains(['\n', '\r'])
    }) {
        return usage_error("invalid --workflow-revision");
    }
    if state_name != "needs-decision" && parsed.workflow_revision.is_some() {
        return usage_error("--workflow-revision is valid only for needs-decision");
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
    let mut identity_detail = serde_json::Value::Null;
    let mut implementation_completed = false;
    let meta_path = state.join(format!("{}.meta", task.as_str()));
    // One scoped lock serializes report acceptance with explicit brief changes.
    let _binding_lock = match multplx_core::locks::DirectoryLock::acquire_wait(
        state.join(format!(".{}.identity.lock", task.as_str())),
        &SystemProcessProbe::default(),
        SUPERVISION_LOCK_TIMEOUT,
    ) {
        Ok(lock) => lock,
        Err(error) => return binding_error(&error.to_string()),
    };
    let meta_text =
        match multplx_core::filesystem::read_bounded_regular(&meta_path, 4 * 1024 * 1024) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => return binding_error("task metadata is not valid UTF-8"),
            },
            Err(multplx_core::error::CoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                String::new()
            }
            Err(error) => return binding_error(&error.to_string()),
        };
    let canonical = if meta_text.is_empty() {
        None
    } else {
        match crate::lifecycle::subagent_model::read_meta(task.as_str(), &meta_text) {
            Ok(record) => Some(record),
            Err(error) => return binding_error(&error),
        }
    };
    if let Some(mut record) = canonical.filter(|record| !record.legacy_unknown) {
        use crate::lifecycle::subagent_model::{
            Acknowledgement, Attempt, MessageEnvelope, SCHEMA_VERSION, new_identity,
        };
        let attempt_id = parsed.attempt_id.or_else(|| env::var("MX_ATTEMPT_ID").ok());
        let generation = parsed
            .generation
            .or_else(|| env::var("MX_ATTEMPT_GENERATION").ok())
            .and_then(|v| v.parse().ok());
        let brief_revision = parsed
            .brief_revision
            .or_else(|| env::var("MX_BRIEF_REVISION").ok())
            .and_then(|v| v.parse().ok());
        let attempt = attempt_id.zip(generation).zip(brief_revision).map(
            |((id, generation), brief_revision)| Attempt {
                id,
                generation,
                brief_revision,
            },
        );
        let message_id = parsed.message_id.unwrap_or_else(|| new_identity("report"));
        if TaskId::parse(&message_id).is_err() {
            return usage_error("invalid --message-id");
        }
        let recipient = record.parent_id.clone().unwrap_or_default();
        let mut envelope = MessageEnvelope {
            schema_version: SCHEMA_VERSION,
            message_id: message_id.clone(),
            task_id: task.to_string(),
            task_home: record.owner_home.clone(),
            parent_home: record.parent_home.clone(),
            attempt,
            parent_id: record.parent_id.clone(),
            sender: bound.to_string(),
            recipient: recipient.clone(),
            brief_revision,
            kind: state_name.clone(),
            correlation_id: parsed.correlation_id.unwrap_or_else(|| message_id.clone()),
            created_at: timestamp(),
            summary: message.clone(),
            artifact: parsed.artifact.clone(),
            acknowledgement: Acknowledgement::Pending,
        };
        let validation = validate_report_home(&record, &state)
            .and_then(|()| envelope.validate_current(&record, bound.as_str(), &recipient));
        // A completion report can close the implementation dependency gate
        // only when a separately typed, current implementation or report
        // artifact proves the result. The status message alone remains status
        // evidence for compatibility.
        if validation.is_ok() && state_name == "done" {
            let completion_evidence = match record.artifact {
                crate::lifecycle::subagent_model::ArtifactKind::Implementation => {
                    record.current_delivery_evidence().is_some_and(|evidence| {
                        let checkout = record
                            .allocation
                            .as_ref()
                            .map(|allocation| Path::new(&allocation.path))
                            .or_else(|| {
                                record
                                    .project
                                    .as_ref()
                                    .map(|project| project.canonical_path.as_path())
                            });
                        checkout
                            .and_then(|path| git_line(path, &["rev-parse", "--verify", "HEAD"]))
                            .is_some_and(|head| head == evidence.commit)
                    })
                }
                crate::lifecycle::subagent_model::ArtifactKind::Report
                | crate::lifecycle::subagent_model::ArtifactKind::Coordination => {
                    parsed.artifact.as_deref().is_some_and(|path| {
                        fs::symlink_metadata(path).is_ok_and(|metadata| {
                            metadata.is_file() && !metadata.file_type().is_symlink()
                        })
                    })
                }
            };
            if completion_evidence {
                record.schedule.state = crate::lifecycle::subagent_model::WorkState::Completed;
                record.schedule.waiting_condition = None;
            }
        }
        if validation.is_ok()
            && state_name == "working"
            && record.schedule.state == crate::lifecycle::subagent_model::WorkState::Completed
        {
            record.schedule.state = crate::lifecycle::subagent_model::WorkState::Running;
            record.schedule.waiting_condition = None;
        }
        implementation_completed =
            record.schedule.state == crate::lifecycle::subagent_model::WorkState::Completed;
        let mut prepared_outcome = if validation.is_ok() {
            match crate::lifecycle::parent_channel::prepare_report(&state, &envelope) {
                Ok(outcome) => outcome,
                Err(error) => return binding_error(&error),
            }
        } else {
            None
        };
        let decision = if validation.is_ok() && state_name == "needs-decision" {
            let Some(question_id) = parsed.key.as_deref() else {
                return usage_error("canonical needs-decision reports require --key");
            };
            match crate::lifecycle::parent_channel::bind_human_question(
                &mut record,
                question_id,
                brief_revision.expect("validated canonical report has brief revision"),
                parsed.workflow_revision.as_deref(),
                &message,
            ) {
                Ok(decision) => Some(decision),
                Err(error) => return binding_error(&error),
            }
        } else {
            None
        };
        let mut evidence = json!({"envelope":envelope,"accepted":validation.is_ok(),"rejection":validation.as_ref().err(),"decision":decision});
        if let Some(outcome) = &prepared_outcome {
            evidence["parent_outcome"] =
                serde_json::to_value(outcome).expect("parent outcome JSON");
        }
        let evidence_dir = state.join("evidence");
        if let Err(error) = fs::create_dir_all(&evidence_dir) {
            return binding_error(&error.to_string());
        }
        let evidence_path = evidence_dir.join(format!("{}-{}.json", task.as_str(), message_id));
        let mut evidence_bytes = serde_json::to_vec(&evidence).expect("evidence JSON");
        if evidence_path.exists() {
            let old_bytes =
                match multplx_core::filesystem::read_bounded_regular(&evidence_path, 1024 * 1024) {
                    Ok(bytes) => bytes,
                    Err(error) => return binding_error(&error.to_string()),
                };
            let old: serde_json::Value = match serde_json::from_slice(&old_bytes) {
                Ok(value) => value,
                Err(_) => return binding_error("corrupt retained evidence"),
            };
            let mut comparable = evidence.clone();
            comparable["envelope"]["created_at"] = old["envelope"]["created_at"].clone();
            if old.get("parent_outcome").is_none() {
                comparable
                    .as_object_mut()
                    .expect("evidence object")
                    .remove("parent_outcome");
            }
            if comparable["envelope"] != old["envelope"]
                || comparable["decision"] != old["decision"]
            {
                return binding_error("message identity reused with different evidence");
            }
            envelope = match serde_json::from_value(old["envelope"].clone()) {
                Ok(envelope) => envelope,
                Err(_) => return binding_error("corrupt retained evidence envelope"),
            };
            prepared_outcome = match old.get("parent_outcome") {
                Some(value) => match serde_json::from_value(value.clone()) {
                    Ok(outcome) => Some(outcome),
                    Err(_) => return binding_error("corrupt retained parent outcome"),
                },
                None => None,
            };
            let prior_receipt = state
                .join(".transitions")
                .join(format!("report-{}-{message_id}.json", task.as_str()));
            if old["accepted"] == true && prior_receipt.is_file() {
                let complete = multplx_core::filesystem::read_bounded_regular(
                    &prior_receipt,
                    16 * 1024 * 1024,
                )
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .is_some_and(|receipt| receipt["committed"] == true);
                if complete {
                    if let Some(outcome) = &prepared_outcome {
                        if let Err(error) =
                            crate::lifecycle::parent_channel::persist_prepared(&state, outcome)
                        {
                            return binding_error(&error);
                        }
                    } else {
                        let _ = crate::lifecycle::parent_channel::record_report(&state, &envelope);
                    }
                    if let Err(error) = publish_report_wake(&state, &envelope) {
                        return binding_error(&error);
                    }
                    return CommandResult::success(String::new());
                }
            }
            if old["accepted"] == false {
                return binding_error(
                    "historical evidence was rejected; message identity retained",
                );
            }
            evidence_bytes = old_bytes;
        }
        if let Err(error) = validation {
            if let Err(write_error) =
                multplx_core::filesystem::atomic_replace(&evidence_path, &evidence_bytes, 0o600)
            {
                return binding_error(&write_error.to_string());
            }
            return binding_error(&format!("{error}; historical evidence retained"));
        }
        let status_path = state.join(format!("{}.status", task.as_str()));
        let updated_meta = match crate::lifecycle::subagent_model::write_meta(&meta_text, &record) {
            Ok(text) => text,
            Err(error) => return binding_error(&error),
        };
        let before =
            match multplx_core::filesystem::read_bounded_regular(&status_path, 16 * 1024 * 1024) {
                Ok(bytes) => Some(bytes),
                Err(multplx_core::error::CoreError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::NotFound =>
                {
                    None
                }
                Err(error) => return binding_error(&error.to_string()),
            };
        let mut after = before.clone().unwrap_or_default();
        after.extend_from_slice(format!("{line}\n").as_bytes());
        let operation = format!("report-{}-{message_id}", task.as_str());
        // Retry the recorded exact intent, rather than appending to its own result.
        let receipt_path = state.join(".transitions").join(format!("{operation}.json"));
        let writes = if receipt_path.exists() {
            let receipt: serde_json::Value = match fs::read(&receipt_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            {
                Some(value) => value,
                None => return binding_error("corrupt report operation"),
            };
            match serde_json::from_value(receipt["writes"].clone()) {
                Ok(writes) => writes,
                Err(_) => return binding_error("corrupt report write intent"),
            }
        } else {
            vec![
                // This unchanged authority record reserves the accepted revision
                // until evidence/status publication is fully reconciled.
                multplx_core::filesystem::TransitionWrite {
                    path: format!("{}.meta", task.as_str()).into(),
                    before: Some(meta_text.as_bytes().to_vec()),
                    after: updated_meta.into_bytes(),
                },
                multplx_core::filesystem::TransitionWrite {
                    path: evidence_path
                        .strip_prefix(&state)
                        .expect("state evidence")
                        .into(),
                    before: fs::read(&evidence_path).ok(),
                    after: evidence_bytes,
                },
                multplx_core::filesystem::TransitionWrite {
                    path: status_path
                        .strip_prefix(&state)
                        .expect("state status")
                        .into(),
                    before,
                    after,
                },
            ]
        };
        if let Err(error) = recoverable_transition_wait(&state, &operation, &writes) {
            return binding_error(&error.to_string());
        }
        if let Some(outcome) = &prepared_outcome
            && let Err(error) = crate::lifecycle::parent_channel::persist_prepared(&state, outcome)
        {
            return binding_error(&format!(
                "accepted report retained but parent outcome publication needs repair: {error}"
            ));
        }
        if let Err(error) = publish_report_wake(&state, &envelope) {
            return binding_error(&error);
        }
        identity_detail = serde_json::to_value(&envelope).expect("envelope JSON");
    } else if let Err(error) = append_single_write(
        state.join(format!("{}.status", task.as_str())),
        format!("{line}\n").as_bytes(),
        0o600,
    ) {
        return CommandResult::error(1, format!("mx-report: {error}\n"));
    }
    drop(_binding_lock);
    let detail = json!({"raw":line,"state":state_name,"validated":true,"identity":identity_detail});
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
    if state_name == "done" && !implementation_completed {
        stderr.push_str("done status recorded; implementation completion was not proven, so dependency gates remain closed. Record current revision-bound task-model evidence whose commit matches the bound checkout HEAD, or attach a result artifact for a report assignment.\n");
    }
    CommandResult {
        status: 0,
        stdout: String::new(),
        stderr,
    }
}

const SUBAGENT_USAGE: &str = "Usage: mx-subagent-pretool-check.sh [--tool <tool-name>] [--command <cmd>] [--claude]\n\nNative delegation is allowed. Bash commands are checked only for supported remote PR merge, auto-merge, merge-queue, and target-branch push forms; local git merge and rebase remain available.\n";

fn git_line(directory: &Path, arguments: &[&str]) -> Option<String> {
    let mut child = std::process::Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(250);
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if !status.success() {
        return None;
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    let output = output.trim().to_owned();
    (!output.is_empty() && !output.contains(['\n', '\r'])).then_some(output)
}

fn remote_merge_git_context(command: &str) -> (Vec<String>, Option<String>) {
    let mut targets = vec!["main".to_owned(), "master".to_owned()];
    let directory = multplx_core::command_policy::git_push_directory(command)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if let Some(remote) = git_line(
        &directory,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) && let Some(branch) = remote.strip_prefix("origin/")
        && !targets.iter().any(|target| target == branch)
    {
        targets.push(branch.to_owned());
    }
    if let Ok(branch) = env::var("MX_PR_TARGET_BRANCH")
        && !branch.is_empty()
        && !targets.iter().any(|target| target == &branch)
    {
        targets.push(branch);
    }
    let state = env::var_os("MX_REPORT_STATE_OVERRIDE")
        .or_else(|| env::var_os("MX_STATE_OVERRIDE"))
        .map(PathBuf::from)
        .or_else(|| env::var_os("MX_HOME").map(|home| PathBuf::from(home).join("state")));
    if let (Some(state), Ok(task)) = (state, env::var("MX_TASK_ID"))
        && let Ok(bytes) = multplx_core::filesystem::read_bounded_regular(
            state.join(format!("{task}.ready-to-push")),
            64 * 1024,
        )
        && let Ok(text) = String::from_utf8(bytes)
        && let Some(base) = text.lines().find_map(|line| line.strip_prefix("base="))
        && !base.is_empty()
        && !base.contains(['\n', '\r'])
        && !targets.iter().any(|target| target == base)
    {
        targets.push(base.to_owned());
    }
    let current = git_line(&directory, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    (targets, current)
}

fn pretool_denial(denial: &multplx_core::command_policy::Denial, claude: bool) -> CommandResult {
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

/// Preserve the retired delegation-hook grammar while applying the one
/// retained product permission boundary to shell commands.
#[must_use]
pub fn subagent_guard(args: &[String], payload: &str, _root: &Path) -> CommandResult {
    let mut command = None;
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
    if command.is_none()
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(payload)
    {
        tool = tool.or_else(|| {
            value
                .get("toolName")
                .or_else(|| value.get("tool_name"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
        command = value
            .pointer("/toolInput/command")
            .or_else(|| value.pointer("/tool_input/command"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
    }
    if tool.as_deref().is_some_and(|name| {
        !matches!(
            name.to_ascii_lowercase().as_str(),
            "bash" | "shell" | "terminal" | "exec_command" | "shell_command" | "run_terminal_cmd"
        )
    }) {
        return CommandResult::success("");
    }
    let Some(command) = command.filter(|value| !value.is_empty()) else {
        return CommandResult::success("");
    };
    let (targets, current) = remote_merge_git_context(&command);
    if let Err(denial) =
        multplx_core::command_policy::remote_pr_merge(&command, &targets, current.as_deref())
    {
        return pretool_denial(&denial, claude);
    }
    CommandResult::success("")
}

const NATIVE_OBSERVE_USAGE: &str = "Usage: mx-native-observe.sh --provider <provider> --event start|result|interrupted|reconcile\n\nReads one provider lifecycle hook payload on stdin. Records provider child identity when exposed, or an explicit session-bound fallback when it is not. Reconcile marks prior session-bound starts interrupted after parent restart. A result event records lifecycle evidence and never marks the owning task complete.\n";

fn observation_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn observation_field(value: &serde_json::Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(name).and_then(serde_json::Value::as_str))
        .filter(|value| !value.is_empty() && value.len() <= 4096 && !value.contains(['\n', '\r']))
        .map(str::to_owned)
}

/// Record a provider-native delegation lifecycle event without turning hook
/// prose or process exit into a task completion transition.
#[must_use]
pub fn native_observe(args: &[String], payload: &str, root: &Path) -> CommandResult {
    use crate::lifecycle::subagent_model::{
        Attempt, NativeDelegationObservation, NativeObservationState, read_meta, write_meta,
    };

    let mut provider = None;
    let mut event = None;
    let mut index = 0;
    while index < args.len() {
        let value = |name: &str, index: &mut usize| -> Result<String, CommandResult> {
            let Some(value) = args.get(*index + 1) else {
                return Err(usage_error(&format!("{name} requires a value")));
            };
            *index += 2;
            Ok(value.clone())
        };
        match args[index].as_str() {
            "--provider" => match value("--provider", &mut index) {
                Ok(value) => provider = Some(value),
                Err(_) => return CommandResult::error(2, NATIVE_OBSERVE_USAGE),
            },
            "--event" => match value("--event", &mut index) {
                Ok(value) => event = Some(value),
                Err(_) => return CommandResult::error(2, NATIVE_OBSERVE_USAGE),
            },
            "-h" | "--help" => return CommandResult::success(NATIVE_OBSERVE_USAGE),
            _ => return CommandResult::error(2, NATIVE_OBSERVE_USAGE),
        }
    }
    let Some(provider) = provider.filter(|value| observation_slug(value)) else {
        return CommandResult::error(2, NATIVE_OBSERVE_USAGE);
    };
    let state_name = match event.as_deref() {
        Some("start") => NativeObservationState::Started,
        Some("result") => NativeObservationState::Result,
        Some("interrupted" | "reconcile") => NativeObservationState::Interrupted,
        _ => return CommandResult::error(2, NATIVE_OBSERVE_USAGE),
    };
    let value: serde_json::Value = match serde_json::from_str(payload) {
        Ok(value @ serde_json::Value::Object(_)) => value,
        _ => return CommandResult::error(2, "mx-native-observe: invalid hook payload\n"),
    };
    let child_id = observation_field(&value, &["agent_id", "agentId", "subagent_id"])
        .filter(|value| observation_slug(value));
    let parent_session_id = observation_field(&value, &["session_id", "sessionId"]);
    let turn_id = observation_field(&value, &["turn_id", "turnId"]);
    let artifact = observation_field(
        &value,
        &["agent_transcript_path", "transcript_path", "result_path"],
    );
    // A provider-issued identifier correlates start/result events, but it does
    // not prove that the child can survive or resume after its parent restarts.
    let recovery = "session-bound";
    let state = state_directory(root);
    if let Err(error) = fs::create_dir_all(&state) {
        return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
    }
    if let Err(error) = recover_native_receipts(&state) {
        return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
    }

    let env_attempt = env::var("MX_ATTEMPT_ID")
        .ok()
        .zip(
            env::var("MX_ATTEMPT_GENERATION")
                .ok()
                .and_then(|v| v.parse().ok()),
        )
        .zip(
            env::var("MX_BRIEF_REVISION")
                .ok()
                .and_then(|v| v.parse().ok()),
        )
        .map(|((id, generation), brief_revision)| Attempt {
            id,
            generation,
            brief_revision,
        });
    let task = env::var("MX_TASK_ID")
        .ok()
        .filter(|value| !value.is_empty())
        .and_then(|value| TaskId::parse(value).ok());
    if event.as_deref() == Some("reconcile") {
        return reconcile_native_observations(&state, task.as_ref(), env_attempt.as_ref());
    }
    let mut hasher = Sha256::new();
    for component in [
        Some(provider.as_str()),
        event.as_deref(),
        task.as_ref().map(TaskId::as_str),
        env_attempt.as_ref().map(|attempt| attempt.id.as_str()),
        child_id.as_deref(),
        parent_session_id.as_deref(),
        turn_id.as_deref(),
        artifact.as_deref(),
    ] {
        hasher.update(component.unwrap_or("-").as_bytes());
        hasher.update([0]);
    }
    if let Some(attempt) = &env_attempt {
        hasher.update(attempt.generation.to_le_bytes());
        hasher.update(attempt.brief_revision.to_le_bytes());
    }
    let observation_id = format!("native-{:x}", hasher.finalize());
    let mut observation = NativeDelegationObservation {
        observation_id: observation_id.clone(),
        provider,
        child_id,
        parent_session_id,
        turn_id,
        parent_attempt: env_attempt.clone(),
        state: state_name,
        observed_at: timestamp(),
        artifact,
        recovery: recovery.to_owned(),
    };
    let evidence_dir = state.join("native-delegations");
    if let Err(error) = fs::create_dir_all(&evidence_dir) {
        return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
    }
    let evidence_path = evidence_dir.join(format!("{observation_id}.json"));

    if let Some(task) = task {
        let meta_path = state.join(format!("{}.meta", task.as_str()));
        let _lock = match multplx_core::locks::DirectoryLock::acquire_wait(
            state.join(format!(".{}.identity.lock", task.as_str())),
            &SystemProcessProbe::default(),
            SUPERVISION_LOCK_TIMEOUT,
        ) {
            Ok(lock) => lock,
            Err(error) => return CommandResult::error(1, format!("mx-native-observe: {error}\n")),
        };
        let before =
            match multplx_core::filesystem::read_bounded_regular(&meta_path, 4 * 1024 * 1024) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
                }
            };
        let text = match String::from_utf8(before.clone()) {
            Ok(text) => text,
            Err(_) => {
                return CommandResult::error(1, "mx-native-observe: task metadata is not UTF-8\n");
            }
        };
        let mut record = match read_meta(task.as_str(), &text) {
            Ok(record) => record,
            Err(error) => return CommandResult::error(1, format!("mx-native-observe: {error}\n")),
        };
        let home_error = validate_report_home(&record, &state).err();
        let operation = format!("native-observation-{observation_id}");
        let receipt_path = state.join(".transitions").join(format!("{operation}.json"));
        if let Some(existing) = record
            .native_observations
            .iter()
            .find(|existing| existing.observation_id == observation_id)
        {
            observation.observed_at.clone_from(&existing.observed_at);
            if existing == &observation {
                if receipt_path.is_file() {
                    let writes = match multplx_core::filesystem::read_transition_writes(
                        &state, &operation,
                    ) {
                        Ok(writes) => writes,
                        Err(error) => {
                            return CommandResult::error(
                                1,
                                format!("mx-native-observe: {error}\n"),
                            );
                        }
                    };
                    if let Err(error) = recoverable_transition_wait(&state, &operation, &writes) {
                        return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
                    }
                }
                return CommandResult::success("");
            }
            return CommandResult::error(1, "mx-native-observe: observation identity conflict\n");
        }
        let accepted = home_error.is_none()
            && !record.legacy_unknown
            && env_attempt.is_some()
            && env_attempt == record.attempt;
        let rejection = if let Some(error) = home_error.as_deref() {
            Some(error)
        } else if !accepted {
            Some("stale or missing parent attempt identity")
        } else {
            None
        };
        let evidence = json!({
            "schema":"mx-native-delegation-evidence.v1",
            "task_id":task.as_str(),
            "observation":observation,
            "accepted":accepted,
            "rejection":rejection,
        });
        let evidence_bytes = serde_json::to_vec(&evidence).expect("native evidence JSON");
        if !accepted {
            if let Err(error) =
                multplx_core::filesystem::atomic_replace(&evidence_path, &evidence_bytes, 0o600)
            {
                return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
            }
            return CommandResult::error(
                3,
                "mx-native-observe: stale observation retained without changing current task\n",
            );
        }
        record.native_observations.push(observation);
        let after = match write_meta(&text, &record) {
            Ok(text) => text.into_bytes(),
            Err(error) => return CommandResult::error(1, format!("mx-native-observe: {error}\n")),
        };
        let writes = vec![
            multplx_core::filesystem::TransitionWrite {
                path: evidence_path
                    .strip_prefix(&state)
                    .expect("state evidence")
                    .into(),
                before: fs::read(&evidence_path).ok(),
                after: evidence_bytes,
            },
            // Canonical task metadata is the authoritative final write.
            multplx_core::filesystem::TransitionWrite {
                path: meta_path.strip_prefix(&state).expect("state meta").into(),
                before: Some(before),
                after,
            },
        ];
        if let Err(error) = recoverable_transition_wait(&state, &operation, &writes) {
            return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
        }
    } else {
        let evidence = json!({
            "schema":"mx-native-delegation-evidence.v1",
            "task_id":serde_json::Value::Null,
            "observation":observation,
            "accepted":true,
            "rejection":serde_json::Value::Null,
        });
        let bytes = serde_json::to_vec(&evidence).expect("native evidence JSON");
        if !evidence_path.exists()
            && let Err(error) =
                multplx_core::filesystem::atomic_replace(&evidence_path, &bytes, 0o600)
        {
            return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
        }
    }
    CommandResult::success("")
}

fn recover_native_receipts(state: &Path) -> Result<(), String> {
    let receipts = state.join(".transitions");
    let mut operations = fs::read_dir(receipts)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let operation = path.file_stem()?.to_str()?;
            (path.extension().and_then(|value| value.to_str()) == Some("json")
                && (operation.starts_with("native-observation-")
                    || operation.starts_with("native-reconcile-")))
            .then(|| operation.to_owned())
        })
        .collect::<Vec<_>>();
    operations.sort();
    for operation in operations {
        recover_transition_wait(state, &operation)?;
    }
    Ok(())
}

fn native_observation_key(
    observation: &crate::lifecycle::subagent_model::NativeDelegationObservation,
) -> String {
    let correlation = observation
        .child_id
        .as_ref()
        .or(observation.turn_id.as_ref())
        .map(String::as_str)
        .unwrap_or(observation.observation_id.as_str());
    let attempt = observation.parent_attempt.as_ref();
    format!(
        "{}\0{}\0{}\0{}\0{}\0{}",
        observation.provider,
        attempt.map(|value| value.id.as_str()).unwrap_or("-"),
        attempt.map(|value| value.generation).unwrap_or_default(),
        attempt
            .map(|value| value.brief_revision)
            .unwrap_or_default(),
        observation.parent_session_id.as_deref().unwrap_or("-"),
        correlation,
    )
}

fn interrupted_observation(
    started: &crate::lifecycle::subagent_model::NativeDelegationObservation,
) -> crate::lifecycle::subagent_model::NativeDelegationObservation {
    use crate::lifecycle::subagent_model::NativeObservationState;
    let mut hasher = Sha256::new();
    hasher.update(started.observation_id.as_bytes());
    hasher.update(b"\0parent-restart");
    let mut observation = started.clone();
    observation.observation_id = format!("native-{:x}", hasher.finalize());
    observation.state = NativeObservationState::Interrupted;
    observation.observed_at = timestamp();
    observation
}

fn reconcile_native_observations(
    state: &Path,
    task: Option<&TaskId>,
    env_attempt: Option<&crate::lifecycle::subagent_model::Attempt>,
) -> CommandResult {
    use crate::lifecycle::subagent_model::{NativeObservationState, read_meta, write_meta};
    if let Some(task) = task {
        let meta_path = state.join(format!("{}.meta", task.as_str()));
        let _lock = match multplx_core::locks::DirectoryLock::acquire_wait(
            state.join(format!(".{}.identity.lock", task.as_str())),
            &SystemProcessProbe::default(),
            SUPERVISION_LOCK_TIMEOUT,
        ) {
            Ok(lock) => lock,
            Err(error) => return CommandResult::error(1, format!("mx-native-observe: {error}\n")),
        };
        let before =
            match multplx_core::filesystem::read_bounded_regular(&meta_path, 4 * 1024 * 1024) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
                }
            };
        let text = match String::from_utf8(before.clone()) {
            Ok(text) => text,
            Err(_) => {
                return CommandResult::error(1, "mx-native-observe: task metadata is not UTF-8\n");
            }
        };
        let mut record = match read_meta(task.as_str(), &text) {
            Ok(record) => record,
            Err(error) => return CommandResult::error(1, format!("mx-native-observe: {error}\n")),
        };
        if let Err(error) = validate_report_home(&record, state) {
            return CommandResult::error(3, format!("mx-native-observe: {error}\n"));
        }
        if record.legacy_unknown || env_attempt.is_none() || env_attempt != record.attempt.as_ref()
        {
            return CommandResult::error(
                3,
                "mx-native-observe: stale reconciliation ignored for the current task attempt\n",
            );
        }
        let terminal = record
            .native_observations
            .iter()
            .filter(|observation| {
                matches!(
                    observation.state,
                    NativeObservationState::Result | NativeObservationState::Interrupted
                )
            })
            .map(native_observation_key)
            .collect::<std::collections::BTreeSet<_>>();
        let interrupted = record
            .native_observations
            .iter()
            .filter(|observation| {
                observation.state == NativeObservationState::Started
                    && observation.recovery == "session-bound"
                    && !terminal.contains(&native_observation_key(observation))
            })
            .map(interrupted_observation)
            .collect::<Vec<_>>();
        if interrupted.is_empty() {
            return CommandResult::success("");
        }
        let mut operation_hasher = Sha256::new();
        let evidence_dir = state.join("native-delegations");
        if let Err(error) = fs::create_dir_all(&evidence_dir) {
            return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
        }
        let mut writes = Vec::with_capacity(interrupted.len() + 1);
        for observation in &interrupted {
            operation_hasher.update(observation.observation_id.as_bytes());
            let path = evidence_dir.join(format!("{}.json", observation.observation_id));
            let evidence = json!({
                "schema":"mx-native-delegation-evidence.v1",
                "task_id":task.as_str(),
                "observation":observation,
                "accepted":true,
                "rejection":serde_json::Value::Null,
            });
            writes.push(multplx_core::filesystem::TransitionWrite {
                path: path.strip_prefix(state).expect("state evidence").into(),
                before: fs::read(&path).ok(),
                after: serde_json::to_vec(&evidence).expect("native evidence JSON"),
            });
        }
        record.native_observations.extend(interrupted);
        let after = match write_meta(&text, &record) {
            Ok(text) => text.into_bytes(),
            Err(error) => return CommandResult::error(1, format!("mx-native-observe: {error}\n")),
        };
        writes.push(multplx_core::filesystem::TransitionWrite {
            path: meta_path.strip_prefix(state).expect("state meta").into(),
            before: Some(before),
            after,
        });
        let operation = format!("native-reconcile-{:x}", operation_hasher.finalize());
        if let Err(error) = recoverable_transition_wait(state, &operation, &writes) {
            return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
        }
        return CommandResult::success("");
    }

    let directory = state.join("native-delegations");
    let mut observations = fs::read_dir(&directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            multplx_core::filesystem::read_bounded_regular(entry.path(), 1024 * 1024).ok()
        })
        .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .filter(|value| value["task_id"].is_null() && value["accepted"] == true)
        .filter_map(|value| {
            serde_json::from_value::<crate::lifecycle::subagent_model::NativeDelegationObservation>(
                value["observation"].clone(),
            )
            .ok()
        })
        .collect::<Vec<_>>();
    let terminal = observations
        .iter()
        .filter(|observation| {
            matches!(
                observation.state,
                NativeObservationState::Result | NativeObservationState::Interrupted
            )
        })
        .map(native_observation_key)
        .collect::<std::collections::BTreeSet<_>>();
    observations.retain(|observation| {
        observation.state == NativeObservationState::Started
            && observation.recovery == "session-bound"
            && !terminal.contains(&native_observation_key(observation))
    });
    for started in observations {
        let observation = interrupted_observation(&started);
        let path = directory.join(format!("{}.json", observation.observation_id));
        if path.exists() {
            continue;
        }
        let evidence = json!({
            "schema":"mx-native-delegation-evidence.v1",
            "task_id":serde_json::Value::Null,
            "observation":observation,
            "accepted":true,
            "rejection":serde_json::Value::Null,
        });
        if let Err(error) = multplx_core::filesystem::atomic_replace(
            &path,
            &serde_json::to_vec(&evidence).expect("native evidence JSON"),
            0o600,
        ) {
            return CommandResult::error(1, format!("mx-native-observe: {error}\n"));
        }
    }
    CommandResult::success("")
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
                    reason: "a persistent top-level directory change in the primary Multplx checkout is blocked; it would move the shell out of the home so a later orchestrator-owned command runs inside a project clone. Reach the target without moving the shell - use git -C <dir> or an absolute path on the command itself - or scope the cd to a subshell like (cd <dir> && ...).",
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

    use multplx_core::filesystem::atomic_replace;

    use super::{
        PretoolPolicy, REPORT_STATES, pretool_guard, reconcile_report_wakes, report, subagent_guard,
    };
    use crate::lifecycle::parent_channel::prepare_outcome;
    use crate::lifecycle::subagent_model::{
        Acknowledgement, ArtifactKind, AssignmentRole, MessageEnvelope, TaskRecord, write_meta,
    };

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
    fn report_repair_isolates_bad_evidence_and_recovers_after_queue_capacity_frees() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(state.join("evidence")).unwrap();
        fs::create_dir_all(state.join("parent-outbox")).unwrap();
        let root_id = format!("root-home:{}", home.display());
        let mut task = TaskRecord::new(
            "repair-task".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            root_id.clone(),
            root_id.clone(),
            home.to_string_lossy().into_owned(),
        );
        task.owner_state = Some(state.to_string_lossy().into_owned());
        task.parent_home = Some(home.to_string_lossy().into_owned());
        task.parent_state = Some(state.to_string_lossy().into_owned());
        atomic_replace(
            state.join("repair-task.meta"),
            write_meta("", &task).unwrap().as_bytes(),
            0o600,
        )
        .unwrap();
        let event = MessageEnvelope {
            schema_version: crate::lifecycle::subagent_model::SCHEMA_VERSION,
            message_id: "repair-after-capacity".into(),
            task_id: task.task_id.clone(),
            task_home: task.owner_home.clone(),
            parent_home: task.parent_home.clone(),
            attempt: task.attempt.clone(),
            parent_id: task.parent_id.clone(),
            sender: task.task_id.clone(),
            recipient: root_id,
            brief_revision: task.attempt.as_ref().map(|attempt| attempt.brief_revision),
            kind: "failed".into(),
            correlation_id: "repair-correlation".into(),
            created_at: "2026-09-15T00:00:00Z".into(),
            summary: "retained accepted outcome".into(),
            artifact: Some("failure.txt".into()),
            acknowledgement: Acknowledgement::Pending,
        };
        let outcome = prepare_outcome(&state, &event).unwrap();
        atomic_replace(
            state.join("evidence/000-malformed.json"),
            br#"{"accepted":true,"envelope":{"bad":true}}"#,
            0o600,
        )
        .unwrap();
        atomic_replace(
            state.join("evidence/001-repair.json"),
            &serde_json::to_vec(&serde_json::json!({
                "accepted": true,
                "envelope": event,
                "parent_outcome": outcome,
            }))
            .unwrap(),
            0o600,
        )
        .unwrap();
        for index in 0..4096 {
            fs::write(
                state.join(format!("parent-outbox/filler-{index:04}.json")),
                b"{}",
            )
            .unwrap();
        }

        let error = reconcile_report_wakes(&state).unwrap_err();
        assert!(error.contains("remain pending"));
        assert!(
            !state
                .join("parent-outbox/repair-after-capacity.json")
                .exists()
        );
        fs::remove_file(state.join("parent-outbox/filler-0000.json")).unwrap();
        assert!(reconcile_report_wakes(&state).is_err());
        assert!(
            state
                .join("parent-outbox/repair-after-capacity.json")
                .is_file()
        );
        assert!(
            state
                .join("message-outbox/repair-after-capacity.json")
                .is_file()
        );
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
    fn subagent_guard_allows_delegation_and_blocks_only_remote_merge_actions() {
        let root = primary_fixture();
        let allowed = subagent_guard(
            &["--tool".into(), "FutureAgentDispatch".into()],
            "",
            root.path(),
        );
        assert_eq!(allowed.status, 0);
        assert!(allowed.stdout.is_empty());
        assert!(allowed.stderr.is_empty());

        let claude = subagent_guard(
            &["--tool=TaskCreate".into(), "--claude".into()],
            "",
            root.path(),
        );
        assert_eq!(claude.status, 0);
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
            0
        );
        for command in [
            "git merge feature",
            "git rebase main",
            "git push origin HEAD:refs/heads/mx/task",
            "gh pr create --fill",
        ] {
            assert_eq!(
                subagent_guard(&["--command".into(), command.into()], "", root.path()).status,
                0,
                "{command}"
            );
        }
        for command in [
            "gh pr merge 9 --squash",
            "gh api graphql -f 'query=mutation { enqueuePullRequest(input: {}) }'",
            "git push origin HEAD:main",
        ] {
            let denied = subagent_guard(&["--command".into(), command.into()], "", root.path());
            assert_eq!(denied.status, 2, "{command}");
            assert!(denied.stderr.contains("permissionDecision"));
        }
        let claude = subagent_guard(
            &["--claude".into()],
            r#"{"tool_name":"Bash","tool_input":{"command":"gh pr merge 9 --auto"}}"#,
            root.path(),
        );
        assert_eq!(claude.status, 2);
        assert!(claude.stdout.is_empty());
        assert!(claude.stderr.contains("remote-pr-merge"));
        for tool in ["exec_command", "shell_command", "run_terminal_cmd"] {
            let payload =
                format!(r#"{{"tool_name":"{tool}","tool_input":{{"command":"gh pr merge 9"}}}}"#);
            assert_eq!(
                subagent_guard(&[], &payload, root.path()).status,
                2,
                "{tool}"
            );
        }
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

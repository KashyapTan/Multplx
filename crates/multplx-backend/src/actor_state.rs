//! Actor-state reconciliation over backend and durable local observations.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use multplx_core::classification::{
    Heuristic, NativeState as SignalNativeState, RunStep, resolve_signal, status_line_note,
    status_line_verb,
};
use multplx_core::identifiers::TaskId;
use multplx_core::journal::{JournalEvent, JournalWriter};
use regex::RegexBuilder;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use crate::command::{CommandRequest, CommandRunner};
use crate::facade::{
    BackendError, BackendTarget, CaptureRequest, NativeState, RuntimeBackend, backend_of_meta,
    meta_get, target_of_meta,
};

const SEP: &str = " · ";
const STATUS_LIMIT: usize = 1024 * 1024;

/// Inputs for one actor-state read.
#[derive(Clone, Debug)]
pub struct ActorStateRequest {
    /// Operational state directory.
    pub state: PathBuf,
    /// Validated task identity.
    pub task: TaskId,
    /// Busy-footer override.
    pub busy_pattern: String,
    /// Pause-verb override.
    pub pause_verb: String,
    /// Whether classification journaling is enabled.
    pub journal_classify: bool,
    /// Calling journal source.
    pub journal_source: Option<String>,
    /// Deterministic journal timestamp override.
    pub journal_now: Option<String>,
}

impl ActorStateRequest {
    /// Construct a request from the current compatibility environment.
    #[must_use]
    pub fn from_environment(state: PathBuf, task: TaskId) -> Self {
        Self {
            state,
            task,
            busy_pattern: std::env::var("MX_BUSY_REGEX")
                .unwrap_or_else(|_| multplx_core::tmux::BUSY_REGEX_DEFAULT.to_owned()),
            pause_verb: std::env::var("MX_CLASSIFY_PAUSED_VERB")
                .unwrap_or_else(|_| "paused".to_owned()),
            journal_classify: std::env::var("MX_JOURNAL_CLASSIFY").as_deref() == Ok("1"),
            journal_source: std::env::var("MX_JOURNAL_SOURCE").ok(),
            journal_now: std::env::var("MX_JOURNAL_NOW").ok(),
        }
    }
}

/// Rendered current state plus best-effort journal warnings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorStateOutput {
    /// Exact one-line compatibility output.
    pub line: String,
    /// At most one best-effort journal diagnostic.
    pub warnings: Vec<String>,
}

/// Narrow backend dependency required for current-state reconciliation.
pub trait ActorStateBackend {
    /// Backend identity recorded alongside the endpoint.
    fn name(&self) -> crate::facade::BackendName;
    /// Read a native semantic state when the backend provides one.
    fn native_state(&mut self, target: &BackendTarget) -> Result<NativeState, BackendError>;
    /// Verify that the exact recorded target remains readable.
    fn target_ready(&mut self, target: &BackendTarget) -> Result<(), BackendError>;
    /// Capture bounded endpoint text for the heuristic fallback.
    fn capture(&mut self, request: &CaptureRequest) -> Result<Vec<u8>, BackendError>;
}

impl<T: RuntimeBackend> ActorStateBackend for T {
    fn name(&self) -> crate::facade::BackendName {
        RuntimeBackend::name(self)
    }

    fn native_state(&mut self, target: &BackendTarget) -> Result<NativeState, BackendError> {
        RuntimeBackend::native_state(self, target)
    }

    fn target_ready(&mut self, target: &BackendTarget) -> Result<(), BackendError> {
        RuntimeBackend::target_ready(self, target)
    }

    fn capture(&mut self, request: &CaptureRequest) -> Result<Vec<u8>, BackendError> {
        RuntimeBackend::capture(self, request)
    }
}

impl ActorStateOutput {
    fn plain(state: &str, source: &str, detail: &str) -> Self {
        let mut line = format!("state: {state}{SEP}source: {source}");
        if !detail.is_empty() {
            line.push_str(SEP);
            line.push_str(detail);
        }
        line.push('\n');
        Self {
            line,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct GateRun {
    status: String,
    step: String,
    round: u64,
}

enum GateObservation {
    Valid(GateRun),
    Unattributed,
    Invalid,
}

fn last_nonblank(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() > STATUS_LIMIT {
        return None;
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .map(str::to_owned)
}

fn git_read(runner: &mut impl CommandRunner, worktree: &Path, args: &[&str]) -> Option<String> {
    let mut request_args = vec![OsString::from("-C"), worktree.as_os_str().to_owned()];
    request_args.extend(args.iter().map(OsString::from));
    let output = runner.run(&CommandRequest::new("git", request_args)).ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn gate_observation(
    path: &Path,
    request: &ActorStateRequest,
    worktree: &Path,
    kind: &str,
    runner: &mut impl CommandRunner,
) -> GateObservation {
    if kind != "delivery" || !path.is_file() || path.is_symlink() {
        return GateObservation::Unattributed;
    }
    let Ok(bytes) = fs::read(path) else {
        return GateObservation::Unattributed;
    };
    if bytes.len() > STATUS_LIMIT {
        return GateObservation::Invalid;
    }
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return GateObservation::Invalid;
    };
    let Some(object) = value.as_object() else {
        return GateObservation::Invalid;
    };
    let string = |key: &str| object.get(key).and_then(Value::as_str);
    let version = object.get("version").and_then(Value::as_u64);
    let task = string("task");
    let recorded_worktree = string("worktree");
    let branch = string("branch");
    let approved_head = string("approved_head");
    let status = string("status");
    let step = string("step");
    let round = object.get("round").and_then(Value::as_u64);
    let approved_valid = approved_head.is_some_and(|head| {
        matches!(head.len(), 40 | 64)
            && head
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    let schema_valid = version == Some(1)
        && task.is_some_and(|value| !value.is_empty())
        && recorded_worktree.is_some_and(|value| !value.is_empty())
        && branch.is_some_and(|value| !value.is_empty())
        && approved_valid
        && status.is_some_and(|value| matches!(value, "running" | "parked" | "passed" | "failed"))
        && step.is_some_and(|value| {
            matches!(
                value,
                "intent" | "rebase" | "review" | "test" | "document" | "lint"
            )
        })
        && round.is_some_and(|value| value >= 1);
    if !schema_valid
        || task != Some(request.task.as_str())
        || recorded_worktree != worktree.to_str()
    {
        return GateObservation::Invalid;
    }
    let Some(current_branch) = git_read(
        runner,
        worktree,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    ) else {
        return GateObservation::Unattributed;
    };
    if branch != Some(current_branch.as_str()) {
        return GateObservation::Unattributed;
    }
    let Some(current_head) = git_read(runner, worktree, &["rev-parse", "--verify", "HEAD"]) else {
        return GateObservation::Unattributed;
    };
    if approved_head != Some(current_head.as_str()) {
        return GateObservation::Unattributed;
    }
    GateObservation::Valid(GateRun {
        status: status.expect("validated status").to_owned(),
        step: step.expect("validated step").to_owned(),
        round: round.expect("validated round"),
    })
}

fn finding_count(state: &Path, task: &TaskId) -> u64 {
    let directory = state.join(format!("{}.gate/findings", task.as_str()));
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|path| fs::read(path).ok())
        .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter_map(|value| {
            value
                .get("findings")?
                .as_array()
                .map(|items| items.len() as u64)
        })
        .sum()
}

fn map_log_state(verb: &str, pause_verb: &str) -> &'static str {
    if verb == pause_verb {
        return "paused";
    }
    match verb {
        "working" => "working",
        "needs-decision" => "parked",
        "blocked" => "blocked",
        "done" => "done",
        "failed" => "failed",
        _ => "unknown",
    }
}

fn normalize_signal<'a>(tier: &str, signal: &'a str) -> &'a str {
    match tier {
        "validated-report" => match signal {
            "needs-decision" => "parked",
            "resolved" => "unknown",
            _ => signal,
        },
        "regex-heuristic" => {
            if signal == "busy" {
                "working"
            } else {
                "unknown"
            }
        }
        _ => signal,
    }
}

fn journal_timestamp(override_value: Option<&str>) -> Option<String> {
    if let Some(value) = override_value {
        return Some(value.to_owned());
    }
    let now = OffsetDateTime::now_utc();
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    ))
}

#[derive(Clone, Copy)]
struct Evidence<'a> {
    winner: &'a str,
    native: &'a str,
    run: &'a str,
    report: &'a str,
    heuristic: &'a str,
}

fn classified(
    request: &ActorStateRequest,
    state: &str,
    source: &str,
    detail: &str,
    evidence: Evidence<'_>,
) -> ActorStateOutput {
    let mut output = ActorStateOutput::plain(state, source, detail);
    if !request.journal_classify
        || !matches!(
            request.journal_source.as_deref(),
            Some("mx-watch" | "mx-supervise-daemon")
        )
    {
        return output;
    }
    let (winner_tier, winner_rank) = if evidence.winner.starts_with("native:") {
        ("native-event", 1)
    } else if evidence.winner.starts_with("run-step:") {
        ("attributed-run-step", 2)
    } else if evidence.winner.starts_with("self-report:") {
        ("validated-report", 3)
    } else {
        ("regex-heuristic", 4)
    };
    let signals = [
        ("native-event", 1, evidence.native),
        ("attributed-run-step", 2, evidence.run),
        ("validated-report", 3, evidence.report),
        ("regex-heuristic", 4, evidence.heuristic),
    ];
    let conflicts = signals
        .into_iter()
        .filter(|(_, rank, signal)| *rank > winner_rank && !signal.is_empty())
        .filter(|(tier, _, signal)| normalize_signal(tier, signal) != state)
        .map(|(tier, _, signal)| json!({"tier":tier,"signal":signal}))
        .collect::<Vec<_>>();
    let mut detail_object = Map::new();
    detail_object.insert("verdict".to_owned(), Value::String(state.to_owned()));
    detail_object.insert("tier".to_owned(), Value::String(winner_tier.to_owned()));
    detail_object.insert("conflicts".to_owned(), Value::Array(conflicts));
    let writer = JournalWriter::new(&request.state);
    if let (Some(source), Some(timestamp)) = (
        request.journal_source.as_deref(),
        journal_timestamp(request.journal_now.as_deref()),
    ) && let Some(warning) = writer.try_emit(
        &request.task,
        JournalEvent::StatusClassified,
        &Value::Object(detail_object),
        source,
        &timestamp,
    ) {
        output.warnings.push(warning);
    }
    output
}

/// Reconcile one actor's current state with the legacy precedence and wording.
pub fn reconcile(
    request: &ActorStateRequest,
    backend: &mut impl ActorStateBackend,
    command_runner: &mut impl CommandRunner,
) -> Result<ActorStateOutput, BackendError> {
    let id = request.task.as_str();
    let meta = request.state.join(format!("{id}.meta"));
    let status_path = request.state.join(format!("{id}.status"));
    let gate_path = request.state.join(format!("{id}.gate/run.json"));
    if !meta.is_file() {
        return Ok(ActorStateOutput::plain(
            "unknown",
            "none",
            &format!("no metadata for {id}"),
        ));
    }
    let Some(worktree_value) = meta_get(&meta, "worktree")? else {
        return Ok(ActorStateOutput::plain(
            "unknown",
            "none",
            "worktree gone (torn down?)",
        ));
    };
    let worktree = PathBuf::from(worktree_value);
    if !worktree.is_dir() {
        return Ok(ActorStateOutput::plain(
            "unknown",
            "none",
            "worktree gone (torn down?)",
        ));
    }
    let worktree =
        fs::canonicalize(&worktree).map_err(|error| BackendError::Metadata(error.to_string()))?;
    let kind = meta_get(&meta, "kind")?
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "delivery".to_owned());
    let log_line = last_nonblank(&status_path).unwrap_or_default();
    let log_verb = status_line_verb(&log_line).to_owned();
    let task_backend = backend_of_meta(&meta)?;
    if backend.name() != task_backend {
        return Err(BackendError::Unsupported {
            backend: task_backend,
            capability: "Rust actor-state reconciliation",
        });
    }
    let endpoint = target_of_meta(&meta)?.unwrap_or_default();
    let target = (!endpoint.is_empty())
        .then(|| BackendTarget::new(task_backend, endpoint.clone(), Some(format!("mx-{id}"))))
        .transpose()?;
    let native_signal = target
        .as_ref()
        .and_then(|target| backend.native_state(target).ok())
        .map(|state| match state {
            NativeState::Idle => "",
            NativeState::Working => "working",
            NativeState::Blocked => "blocked",
            NativeState::Done => "done",
        })
        .unwrap_or("");

    match gate_observation(&gate_path, request, &worktree, &kind, command_runner) {
        GateObservation::Invalid => {
            return Ok(ActorStateOutput::plain(
                "unknown",
                "none",
                "invalid deep-review run record",
            ));
        }
        GateObservation::Valid(run) => {
            let (run_state, mut run_detail) = match run.status.as_str() {
                "running" => (
                    "working",
                    format!("validating ({} round {})", run.step, run.round),
                ),
                "parked" => {
                    let count = finding_count(&request.state, &request.task);
                    let mut detail = format!("parked at {} round {}", run.step, run.round);
                    if count != 0 {
                        detail.push_str(&format!(": {count} recorded finding(s)"));
                    }
                    ("parked", detail)
                }
                "passed" => ("done", "validated local branch".to_owned()),
                "failed" => ("failed", format!("validation failed at {}", run.step)),
                _ => unreachable!("validated status"),
            };
            if matches!(log_verb.as_str(), "needs-decision" | "blocked")
                && run_state != "parked"
                && native_signal != "blocked"
            {
                run_detail.push_str(SEP);
                run_detail.push_str("status-log superseded by deep-review run");
            }
            let winner = resolve_signal(
                SignalNativeState::parse(native_signal),
                RunStep::parse(run_state),
                &log_verb,
                Heuristic::Unknown,
                &request.pause_verb,
            );
            let evidence = Evidence {
                winner: &winner,
                native: native_signal,
                run: run_state,
                report: &log_verb,
                heuristic: "",
            };
            if let Some(state) = winner.strip_prefix("native:") {
                return Ok(classified(
                    request,
                    state,
                    "native-event",
                    &format!("runtime {state}{SEP}run-step still {run_detail}"),
                    evidence,
                ));
            }
            return Ok(classified(
                request,
                run_state,
                "run-step",
                &run_detail,
                evidence,
            ));
        }
        GateObservation::Unattributed => {}
    }

    let Some(target) = target else {
        return Ok(ActorStateOutput::plain(
            "unknown",
            "none",
            "no backend target recorded",
        ));
    };
    if backend.target_ready(&target).is_err() {
        return Ok(ActorStateOutput::plain(
            "unknown",
            "none",
            &format!("backend target gone: {}", target.endpoint()),
        ));
    }
    let log_state = map_log_state(&log_verb, &request.pause_verb);
    let report_signal = if log_state == "unknown" {
        ""
    } else {
        log_verb.as_str()
    };
    let heuristic_signal = if kind == "daemon" {
        ""
    } else {
        let capture = backend.capture(&CaptureRequest {
            target: target.clone(),
            lines: 40,
            byte_limit: 256 * 1024,
        });
        let busy = capture.ok().is_some_and(|bytes| {
            let text = String::from_utf8_lossy(&bytes);
            RegexBuilder::new(&request.busy_pattern)
                .case_insensitive(true)
                .build()
                .ok()
                .is_some_and(|regex| {
                    text.lines()
                        .filter(|line| !line.trim().is_empty())
                        .rev()
                        .take(6)
                        .any(|line| regex.is_match(line))
                })
        });
        if busy { "busy" } else { "idle" }
    };
    let winner = resolve_signal(
        SignalNativeState::parse(native_signal),
        RunStep::Unknown,
        report_signal,
        Heuristic::parse(heuristic_signal),
        &request.pause_verb,
    );
    let evidence = Evidence {
        winner: &winner,
        native: native_signal,
        run: "",
        report: report_signal,
        heuristic: heuristic_signal,
    };
    if let Some(state) = winner.strip_prefix("native:") {
        Ok(classified(
            request,
            state,
            "native-event",
            &format!("runtime {state}"),
            evidence,
        ))
    } else if winner.starts_with("self-report:") {
        Ok(classified(
            request,
            log_state,
            "status-log",
            status_line_note(&log_line),
            evidence,
        ))
    } else if winner == "heuristic:busy" {
        Ok(classified(
            request,
            "working",
            "pane",
            "harness busy",
            evidence,
        ))
    } else {
        Ok(ActorStateOutput::plain(
            "unknown",
            "none",
            "no current-state source available",
        ))
    }
}

#[cfg(test)]
mod tests {
    use multplx_core::identifiers::TaskId;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use crate::command::SystemCommandRunner;
    use crate::facade::{BackendError, BackendName, BackendTarget, CaptureRequest, NativeState};

    use super::{ActorStateBackend, ActorStateRequest, STATUS_LIMIT, reconcile};

    struct FakeBackend {
        name: BackendName,
        ready: bool,
        capture: Vec<u8>,
        native: Option<NativeState>,
    }

    impl ActorStateBackend for FakeBackend {
        fn name(&self) -> BackendName {
            self.name
        }
        fn target_ready(&mut self, _: &BackendTarget) -> Result<(), BackendError> {
            self.ready
                .then_some(())
                .ok_or_else(|| BackendError::Command("gone".to_owned()))
        }
        fn capture(&mut self, _: &CaptureRequest) -> Result<Vec<u8>, BackendError> {
            Ok(self.capture.clone())
        }
        fn native_state(&mut self, _: &BackendTarget) -> Result<NativeState, BackendError> {
            self.native.ok_or(BackendError::Unsupported {
                backend: BackendName::Tmux,
                capability: "native-state",
            })
        }
    }

    fn git_fixture(path: &Path, task: &str) -> (String, String) {
        fs::create_dir_all(path).expect("repo");
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "test"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["commit", "-q", "--allow-empty", "-m", "base"],
            vec!["checkout", "-qb", &format!("mx/{task}")],
            vec!["commit", "-q", "--allow-empty", "-m", "change"],
        ] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(path)
                    .args(args)
                    .status()
                    .expect("git")
                    .success()
            );
        }
        let branch = format!("mx/{task}");
        let head = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(path)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("head")
                .stdout,
        )
        .expect("utf8")
        .trim()
        .to_owned();
        (branch, head)
    }

    fn request(state: &Path, task: &str) -> ActorStateRequest {
        ActorStateRequest {
            state: state.to_owned(),
            task: TaskId::parse(task).expect("task"),
            busy_pattern: "Working".to_owned(),
            pause_verb: "paused".to_owned(),
            journal_classify: false,
            journal_source: None,
            journal_now: None,
        }
    }

    fn scaffold(temp: &Path, task: &str) -> (PathBuf, PathBuf, String, String) {
        let state = temp.join("state");
        let repo = temp.join("repo");
        fs::create_dir_all(&state).expect("state");
        let (branch, head) = git_fixture(&repo, task);
        let repo = fs::canonicalize(repo).expect("canonical repo");
        fs::write(
            state.join(format!("{task}.meta")),
            format!(
                "window=broker:mx-{task}\nworktree={}\nkind=delivery\n",
                repo.display()
            ),
        )
        .expect("meta");
        (state, repo, branch, head)
    }

    #[test]
    fn missing_torn_down_report_and_busy_fallbacks_are_exact() {
        let temp = tempfile::tempdir().expect("temp");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let mut backend = FakeBackend {
            name: BackendName::Tmux,
            ready: true,
            capture: b"Working...\n".to_vec(),
            native: None,
        };
        let mut commands = SystemCommandRunner;
        let output =
            reconcile(&request(&state, "one"), &mut backend, &mut commands).expect("missing");
        assert_eq!(
            output.line,
            "state: unknown · source: none · no metadata for one\n"
        );

        let (state, _, _, _) = scaffold(temp.path(), "two");
        fs::write(state.join("two.status"), "paused: release window\n").expect("status");
        let output =
            reconcile(&request(&state, "two"), &mut backend, &mut commands).expect("paused");
        assert_eq!(
            output.line,
            "state: paused · source: status-log · release window\n"
        );
        fs::remove_file(state.join("two.status")).expect("remove");
        let output = reconcile(&request(&state, "two"), &mut backend, &mut commands).expect("busy");
        assert_eq!(
            output.line,
            "state: working · source: pane · harness busy\n"
        );
        backend.ready = false;
        let output = reconcile(&request(&state, "two"), &mut backend, &mut commands).expect("gone");
        assert_eq!(
            output.line,
            "state: unknown · source: none · backend target gone: broker:mx-two\n"
        );
    }

    #[test]
    fn gate_binding_statuses_findings_and_native_precedence_are_exact() {
        let temp = tempfile::tempdir().expect("temp");
        let task = "gate";
        let (state, repo, branch, head) = scaffold(temp.path(), task);
        let gate = state.join(format!("{task}.gate"));
        fs::create_dir_all(gate.join("findings")).expect("gate");
        fs::write(gate.join("findings/one.json"), r#"{"findings":[{},{}]}"#).expect("findings");
        let run_path = gate.join("run.json");
        let write_run = |status: &str| {
            fs::write(
                &run_path,
                serde_json::to_vec(&serde_json::json!({
                    "version":1,"task":task,"worktree":repo,"branch":branch,
                    "approved_head":head,"status":status,"step":"review","round":2
                }))
                .expect("json"),
            )
            .expect("run")
        };
        let mut backend = FakeBackend {
            name: BackendName::Tmux,
            ready: false,
            capture: Vec::new(),
            native: None,
        };
        let mut commands = SystemCommandRunner;
        for (status, expected) in [
            (
                "running",
                "state: working · source: run-step · validating (review round 2)\n",
            ),
            (
                "parked",
                "state: parked · source: run-step · parked at review round 2: 2 recorded finding(s)\n",
            ),
            (
                "passed",
                "state: done · source: run-step · validated local branch\n",
            ),
            (
                "failed",
                "state: failed · source: run-step · validation failed at review\n",
            ),
        ] {
            write_run(status);
            assert_eq!(
                reconcile(&request(&state, task), &mut backend, &mut commands)
                    .expect("run")
                    .line,
                expected
            );
        }
        write_run("running");
        backend.native = Some(NativeState::Blocked);
        assert_eq!(
            reconcile(&request(&state, task), &mut backend, &mut commands)
                .expect("native")
                .line,
            "state: blocked · source: native-event · runtime blocked · run-step still validating (review round 2)\n"
        );
        fs::write(&run_path, "{}").expect("invalid");
        assert_eq!(
            reconcile(&request(&state, task), &mut backend, &mut commands)
                .expect("invalid")
                .line,
            "state: unknown · source: none · invalid deep-review run record\n"
        );
    }

    #[test]
    fn classification_journal_is_best_effort_and_byte_compatible() {
        let temp = tempfile::tempdir().expect("temp");
        let (state, _, _, _) = scaffold(temp.path(), "journal");
        fs::write(state.join("journal.status"), "done: finished\n").expect("status");
        let mut req = request(&state, "journal");
        req.journal_classify = true;
        req.journal_source = Some("mx-watch".to_owned());
        req.journal_now = Some("2026-08-11T12:34:56Z".to_owned());
        let mut backend = FakeBackend {
            name: BackendName::Tmux,
            ready: true,
            capture: Vec::new(),
            native: None,
        };
        let mut commands = SystemCommandRunner;
        let output = reconcile(&req, &mut backend, &mut commands).expect("state");
        assert_eq!(output.line, "state: done · source: status-log · finished\n");
        let journal = fs::read_to_string(state.join("journal.journal")).expect("journal");
        assert_eq!(
            journal,
            "{\"ts\":\"2026-08-11T12:34:56Z\",\"task\":\"journal\",\"source\":\"mx-watch\",\"event\":\"status.classified\",\"detail\":{\"verdict\":\"done\",\"tier\":\"validated-report\",\"conflicts\":[{\"tier\":\"regex-heuristic\",\"signal\":\"idle\"}]}}\n"
        );
        req.journal_now = None;
        backend.native = Some(NativeState::Working);
        assert!(
            reconcile(&req, &mut backend, &mut commands)
                .expect("current timestamp")
                .warnings
                .is_empty()
        );
    }

    #[test]
    fn environment_metadata_native_and_unattributed_edges_fail_closed() {
        let temp = tempfile::tempdir().expect("temp");
        let task = TaskId::parse("edges").expect("task");
        let from_environment =
            ActorStateRequest::from_environment(temp.path().join("environment"), task.clone());
        assert_eq!(from_environment.task, task);

        let state = temp.path().join("torn/state");
        fs::create_dir_all(&state).expect("state");
        fs::write(state.join("edges.meta"), "window=broker:mx-edges\n").expect("meta");
        let mut backend = FakeBackend {
            name: BackendName::Tmux,
            ready: true,
            capture: Vec::new(),
            native: None,
        };
        let mut commands = SystemCommandRunner;
        assert_eq!(
            reconcile(&request(&state, "edges"), &mut backend, &mut commands)
                .expect("missing worktree")
                .line,
            "state: unknown · source: none · worktree gone (torn down?)\n"
        );
        fs::write(
            state.join("edges.meta"),
            "window=broker:mx-edges\nworktree=/definitely/missing/mx-worktree\n",
        )
        .expect("meta");
        assert_eq!(
            reconcile(&request(&state, "edges"), &mut backend, &mut commands)
                .expect("gone worktree")
                .line,
            "state: unknown · source: none · worktree gone (torn down?)\n"
        );

        let good = temp.path().join("good");
        let (state, repo, branch, head) = scaffold(&good, "native");
        backend.name = BackendName::Herdr;
        assert!(matches!(
            reconcile(&request(&state, "native"), &mut backend, &mut commands),
            Err(BackendError::Unsupported { .. })
        ));
        backend.name = BackendName::Tmux;
        fs::write(
            state.join("native.meta"),
            format!("worktree={}\nkind=daemon\n", repo.display()),
        )
        .expect("meta");
        assert_eq!(
            reconcile(&request(&state, "native"), &mut backend, &mut commands)
                .expect("no endpoint")
                .line,
            "state: unknown · source: none · no backend target recorded\n"
        );
        fs::write(
            state.join("native.meta"),
            format!(
                "window=broker:mx-native\nworktree={}\nkind=daemon\n",
                repo.display()
            ),
        )
        .expect("meta");
        assert_eq!(
            reconcile(&request(&state, "native"), &mut backend, &mut commands)
                .expect("daemon unknown")
                .line,
            "state: unknown · source: none · no current-state source available\n"
        );
        for (native, expected) in [
            (
                NativeState::Working,
                "state: working · source: native-event · runtime working\n",
            ),
            (
                NativeState::Done,
                "state: done · source: native-event · runtime done\n",
            ),
        ] {
            backend.native = Some(native);
            assert_eq!(
                reconcile(&request(&state, "native"), &mut backend, &mut commands)
                    .expect("native")
                    .line,
                expected
            );
        }
        backend.native = Some(NativeState::Idle);
        assert_eq!(
            reconcile(&request(&state, "native"), &mut backend, &mut commands)
                .expect("native idle")
                .line,
            "state: unknown · source: none · no current-state source available\n"
        );

        backend.native = None;
        fs::write(
            state.join("native.meta"),
            format!(
                "window=broker:mx-native\nworktree={}\nkind=delivery\n",
                repo.display()
            ),
        )
        .expect("meta");
        fs::write(state.join("native.status"), "blocked: old blocker\n").expect("status");
        fs::create_dir_all(state.join("native.gate")).expect("gate");
        fs::write(
            state.join("native.gate/run.json"),
            serde_json::to_vec(&serde_json::json!({
                "version":1,"task":"native","worktree":repo,"branch":branch,
                "approved_head":head,"status":"running","step":"test","round":1
            }))
            .expect("json"),
        )
        .expect("run");
        assert_eq!(
            reconcile(&request(&state, "native"), &mut backend, &mut commands)
                .expect("superseded")
                .line,
            "state: working · source: run-step · validating (test round 1) · status-log superseded by deep-review run\n"
        );
        fs::write(
            state.join("native.gate/run.json"),
            serde_json::to_vec(&serde_json::json!({
                "version":1,"task":"native","worktree":repo,"branch":"wrong",
                "approved_head":head,"status":"running","step":"test","round":1
            }))
            .expect("json"),
        )
        .expect("run");
        assert_eq!(
            reconcile(&request(&state, "native"), &mut backend, &mut commands)
                .expect("unattributed")
                .line,
            "state: blocked · source: status-log · old blocker\n"
        );

        fs::write(state.join("native.status"), vec![b'x'; STATUS_LIMIT + 1]).expect("large status");
        fs::remove_file(state.join("native.gate/run.json")).expect("remove run");
        assert_eq!(
            reconcile(&request(&state, "native"), &mut backend, &mut commands)
                .expect("bounded status")
                .line,
            "state: unknown · source: none · no current-state source available\n"
        );
    }
}

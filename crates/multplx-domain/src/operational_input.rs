//! Canonical construction and classification for the Multplx operational-input
//! protocol formerly owned by `bin/mx-operational-input.sh`.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::lifecycle::subagent_model::{Acknowledgement, Attempt, MessageEnvelope, SCHEMA_VERSION};
use multplx_core::filesystem::{
    TransitionFault, TransitionWrite, read_bounded_regular, read_transition_writes,
    recover_transition, recoverable_transition,
};
use multplx_core::identifiers::TaskId;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::ProcessIdentity;

const TRANSITION_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

fn recoverable_transition_wait(
    state: &Path,
    operation: &str,
    writes: &[TransitionWrite],
    fault: Option<TransitionFault>,
) -> Result<(), String> {
    let started = Instant::now();
    loop {
        match recoverable_transition(state, operation, writes, fault) {
            Ok(()) => return Ok(()),
            Err(multplx_core::error::CoreError::LockHeld { .. })
                if started.elapsed() < TRANSITION_LOCK_TIMEOUT =>
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
        match recover_transition(state, operation) {
            Ok(()) => return Ok(()),
            Err(multplx_core::error::CoreError::LockHeld { .. })
                if started.elapsed() < TRANSITION_LOCK_TIMEOUT =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Permanent compatibility prefix, including U+2063 INVISIBLE SEPARATOR.
pub const PREFIX: &str = "\u{2063}MULTPLX_OP: ";
/// Current wire version.
pub const VERSION: &str = "v1";
/// Live-charter-compatible from-broker carrier.
pub const FROM_BROKER_MARK: &str = "[mx-from-broker]\u{2063}";
/// Durable routed terminal-request schema.
pub const REQUEST_SCHEMA: &str = "mx-routed-request.v1";
const MAX_REQUEST_BYTES: usize = 256 * 1024;
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// Exact pre-protocol session-start prompt retained for transcript parsing.
pub const LEGACY_SESSION_START: &str =
    "Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.";
/// Exact pre-protocol watcher prefix.
pub const LEGACY_WATCHER_PREFIX: &str = "MULTPLX WATCHER WAKE: ";
/// Exact pre-protocol watcher suffix.
pub const LEGACY_WATCHER_SUFFIX: &str = "\n\nRun bin/mx-wake-drain.sh first and handle the queued wake. Watcher continuity is extension-owned.";
/// Exact pre-protocol turn-end prefix.
pub const LEGACY_TURN_END_PREFIX: &str = "TURN WOULD END BLIND - supervision is off. The watcher cycle is missing, failed, or unhealthy. Follow the harness recovery instruction below before ending the turn.\n\n";

fn message_receipt_path(message_id: &str) -> Result<PathBuf, String> {
    TaskId::parse(message_id.to_owned()).map_err(|error| error.to_string())?;
    Ok(Path::new("message-outbox").join(format!("{message_id}.json")))
}

fn acknowledgement_rank(value: &Acknowledgement) -> u8 {
    match value {
        Acknowledgement::Pending => 0,
        Acknowledgement::Delivered => 1,
        Acknowledgement::Acknowledged => 2,
        Acknowledgement::Answered => 3,
        Acknowledgement::Completed => 4,
    }
}

/// Persist a validated shared message envelope before transport. Reusing a
/// message identity is accepted only when every immutable field matches.
pub fn persist_message_envelope(
    state: &Path,
    envelope: &MessageEnvelope,
) -> Result<MessageEnvelope, String> {
    let relative = message_receipt_path(&envelope.message_id)?;
    let path = state.join(&relative);
    fs::create_dir_all(state.join("message-outbox")).map_err(|error| error.to_string())?;
    if path.is_file() {
        let existing: MessageEnvelope = serde_json::from_slice(
            &read_bounded_regular(&path, MAX_MESSAGE_BYTES).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let mut left = existing.clone();
        let mut right = envelope.clone();
        left.acknowledgement = Acknowledgement::Pending;
        right.acknowledgement = Acknowledgement::Pending;
        if left != right {
            return Err("message identity conflicts with durable envelope".into());
        }
        return Ok(existing);
    }
    let bytes = serde_json::to_vec_pretty(envelope).map_err(|error| error.to_string())?;
    let operation = format!("message-create-{}", envelope.message_id);
    recoverable_transition_wait(
        state,
        &operation,
        &[TransitionWrite {
            path: relative,
            before: None,
            after: bytes,
        }],
        None,
    )?;
    Ok(envelope.clone())
}

pub fn read_message_envelope(state: &Path, message_id: &str) -> Result<MessageEnvelope, String> {
    let relative = message_receipt_path(message_id)?;
    serde_json::from_slice(
        &read_bounded_regular(state.join(relative), MAX_MESSAGE_BYTES)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

/// Publish one critical shared envelope to the durable wake path. A committed
/// caller repeats this after recovery; the message receipt and stable wake key
/// make commit-to-notification repair idempotent while preserving distinct
/// message IDs for separate transitions on the same task.
pub fn publish_message_wake(
    state: &Path,
    envelope: &MessageEnvelope,
    payload: &str,
    now: SystemTime,
    processes: &impl multplx_core::process::ProcessProbe,
) -> Result<String, String> {
    persist_message_envelope(state, envelope)?;
    let key = format!("message-{}", envelope.message_id);
    let (record, _) = multplx_core::wake::WakeQueue::new(state)
        .append_once(
            multplx_core::wake::WakeKind::Signal,
            &key,
            payload,
            now,
            processes,
        )
        .map_err(|error| error.to_string())?;
    Ok(format!("wake-{:020}", record.sequence))
}

/// Advance delivery/response acknowledgement without changing route identity.
pub fn advance_message_envelope(
    state: &Path,
    message_id: &str,
    acknowledgement: Acknowledgement,
) -> Result<MessageEnvelope, String> {
    let relative = message_receipt_path(message_id)?;
    let before = read_bounded_regular(state.join(&relative), MAX_MESSAGE_BYTES)
        .map_err(|error| error.to_string())?;
    let mut envelope: MessageEnvelope =
        serde_json::from_slice(&before).map_err(|error| error.to_string())?;
    if acknowledgement_rank(&acknowledgement) <= acknowledgement_rank(&envelope.acknowledgement) {
        return Ok(envelope);
    }
    envelope.acknowledgement = acknowledgement;
    let after = serde_json::to_vec_pretty(&envelope).map_err(|error| error.to_string())?;
    let operation = format!(
        "message-state-{message_id}-{}",
        acknowledgement_rank(&envelope.acknowledgement)
    );
    recoverable_transition_wait(
        state,
        &operation,
        &[TransitionWrite {
            path: relative,
            before: Some(before),
            after,
        }],
        None,
    )?;
    Ok(envelope)
}
/// Exact pre-protocol away prefix.
pub const LEGACY_AWAY_PREFIX: &str = "\u{2063}Supervisor escalate (";

/// Closed current construction vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    SessionStart,
    Watcher,
    TurnEndGuard,
    AwaySupervisor,
    LaunchBrief,
    FromBroker,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OperationalInputCodec;

impl OperationalInputCodec {
    #[must_use]
    pub fn construct(self, kind: Kind, body: &str) -> Option<String> {
        construct(kind, body)
    }

    #[must_use]
    pub fn kind(self, message: &str) -> Option<Kind> {
        current_kind(message)
    }

    #[must_use]
    pub fn body(self, message: &str) -> Option<&str> {
        body(message)
    }

    #[must_use]
    pub fn classify(self, message: &str) -> Option<&'static str> {
        classify(message)
    }
}

impl Kind {
    /// Parse a current producer kind.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "session-start" => Some(Self::SessionStart),
            "watcher" => Some(Self::Watcher),
            "turn-end-guard" => Some(Self::TurnEndGuard),
            "away-supervisor" => Some(Self::AwaySupervisor),
            "launch-brief" => Some(Self::LaunchBrief),
            "from-broker" => Some(Self::FromBroker),
            _ => None,
        }
    }

    /// Wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session-start",
            Self::Watcher => "watcher",
            Self::TurnEndGuard => "turn-end-guard",
            Self::AwaySupervisor => "away-supervisor",
            Self::LaunchBrief => "launch-brief",
            Self::FromBroker => "from-broker",
        }
    }

    const fn is_generic(self) -> bool {
        !matches!(self, Self::FromBroker)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Construct one current operational input without interpreting its body.
pub fn construct(kind: Kind, body: &str) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    if kind == Kind::FromBroker {
        return Some(mark_from_broker(body));
    }
    Some(format!("{PREFIX}{VERSION} {}: {body}", kind.as_str()))
}

/// Add the established from-broker carrier idempotently.
#[must_use]
pub fn mark_from_broker(body: &str) -> String {
    if body.starts_with(FROM_BROKER_MARK) && body.len() > FROM_BROKER_MARK.len() {
        body.to_owned()
    } else {
        format!("{FROM_BROKER_MARK}{body}")
    }
}

/// Parse only current typed inputs.
#[must_use]
pub fn current_kind(message: &str) -> Option<Kind> {
    if message.starts_with(FROM_BROKER_MARK) && message.len() > FROM_BROKER_MARK.len() {
        return Some(Kind::FromBroker);
    }
    let remainder = message.strip_prefix(&format!("{PREFIX}{VERSION} "))?;
    let (raw_kind, body) = remainder.split_once(": ")?;
    let kind = Kind::parse(raw_kind)?;
    if !kind.is_generic() || body.is_empty() {
        return None;
    }
    Some(kind)
}

/// Recover the exact body of a current input.
#[must_use]
pub fn body(message: &str) -> Option<&str> {
    if message.starts_with(FROM_BROKER_MARK) && message.len() > FROM_BROKER_MARK.len() {
        return message.strip_prefix(FROM_BROKER_MARK);
    }
    let kind = current_kind(message)?;
    message.strip_prefix(&format!("{PREFIX}{VERSION} {}: ", kind.as_str()))
}

/// Classify a historical pre-protocol transcript input.
#[must_use]
pub fn legacy_kind(message: &str) -> Option<&'static str> {
    if message.starts_with(PREFIX) && message.len() > PREFIX.len() {
        return Some("legacy-operational");
    }
    if message == LEGACY_SESSION_START {
        return Some("session-start");
    }
    if message.starts_with(LEGACY_AWAY_PREFIX) {
        return Some("away-supervisor");
    }
    if message.starts_with(LEGACY_WATCHER_PREFIX)
        && message.ends_with(LEGACY_WATCHER_SUFFIX)
        && message.len() > LEGACY_WATCHER_PREFIX.len() + LEGACY_WATCHER_SUFFIX.len()
    {
        return Some("watcher");
    }
    if message.starts_with(LEGACY_TURN_END_PREFIX) && message.len() > LEGACY_TURN_END_PREFIX.len() {
        return Some("turn-end-guard");
    }
    None
}

/// Classify current input first and historical input second.
#[must_use]
pub fn classify(message: &str) -> Option<&'static str> {
    current_kind(message)
        .map(Kind::as_str)
        .or_else(|| legacy_kind(message))
}

/// One separately tracked transport, response or completion fact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestResult {
    pub id: String,
    pub recorded_at: u64,
    pub summary: String,
    pub artifact: Option<String>,
}

/// Stable per-item request routed into the one owning orchestrator inbox.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedRequest {
    pub schema: String,
    pub batch_id: String,
    pub request_id: String,
    pub task_id: String,
    pub client_id: String,
    pub recipient_home: String,
    pub recipient_owner: Option<ProcessIdentity>,
    #[serde(default)]
    pub prior_recipient_owners: Vec<ProcessIdentity>,
    pub parent_task_id: Option<String>,
    pub parent_home: Option<String>,
    pub attempt_id: Option<String>,
    pub attempt_generation: Option<u64>,
    pub project_id: String,
    pub checkout_id: String,
    pub starting_revision: String,
    pub brief_revision: u64,
    pub scope: String,
    pub dependencies: Vec<String>,
    pub context_artifact: Option<String>,
    pub created_at: u64,
    pub delivered_at: u64,
    pub notification_event_id: Option<String>,
    pub acknowledged_at: Option<u64>,
    pub response: Option<RequestResult>,
    pub completion: Option<RequestResult>,
    /// Shared A11 message contract used by reports and task messaging. The
    /// request fields remain the terminal-intake index; this envelope is the
    /// canonical routed message identity and acknowledgement state.
    pub envelope: MessageEnvelope,
}

impl RoutedRequest {
    fn validate(&self) -> Result<(), String> {
        for id in [
            &self.batch_id,
            &self.request_id,
            &self.task_id,
            &self.client_id,
        ] {
            TaskId::parse(id.clone()).map_err(|error| error.to_string())?;
        }
        for id in [&self.project_id, &self.checkout_id] {
            if id.is_empty()
                || id.len() > 192
                || id.starts_with('.')
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            {
                return Err("invalid project or checkout identity".into());
            }
        }
        for dependency in &self.dependencies {
            TaskId::parse(dependency.clone()).map_err(|error| error.to_string())?;
        }
        for optional in [&self.parent_task_id, &self.attempt_id]
            .into_iter()
            .flatten()
        {
            TaskId::parse(optional.clone()).map_err(|error| error.to_string())?;
        }
        if self.schema != REQUEST_SCHEMA
            || self.brief_revision == 0
            || self.scope.trim().is_empty()
            || self.scope.len() > 16 * 1024
            || invalid_text(&self.starting_revision, 4096)
            || self.starting_revision.trim().is_empty()
            || !Path::new(&self.recipient_home).is_absolute()
            || self
                .parent_home
                .as_ref()
                .is_some_and(|home| !Path::new(home).is_absolute())
            || self.attempt_id.is_some() != self.attempt_generation.is_some()
            || self.parent_task_id.is_some() != self.parent_home.is_some()
            || self.attempt_generation == Some(0)
            || self.created_at == 0
            || self.delivered_at == 0
        {
            return Err("invalid routed request binding".into());
        }
        let expected_attempt = self.attempt_id.as_ref().map(|id| Attempt {
            id: id.clone(),
            generation: self.attempt_generation.expect("paired attempt generation"),
            brief_revision: self.brief_revision,
        });
        if self.envelope.schema_version != SCHEMA_VERSION
            || self.envelope.message_id != self.request_id
            || self.envelope.task_id != self.task_id
            || self.envelope.task_home.as_deref() != Some(self.recipient_home.as_str())
            || self.envelope.parent_home != self.parent_home
            || self.envelope.attempt != expected_attempt
            || self.envelope.parent_id != self.parent_task_id
            || self.envelope.sender != self.client_id
            || self.envelope.recipient != self.recipient_home
            || self.envelope.brief_revision != Some(self.brief_revision)
            || self.envelope.kind != "decision-request"
            || self.envelope.correlation_id != self.request_id
            || self.envelope.created_at.is_empty()
            || self.envelope.summary != self.scope
            || self.envelope.artifact != self.context_artifact
        {
            return Err("shared message envelope does not match routed request".into());
        }
        let expected_ack = if self.completion.is_some() {
            Acknowledgement::Completed
        } else if self.response.is_some() {
            Acknowledgement::Answered
        } else if self.acknowledged_at.is_some() {
            Acknowledgement::Acknowledged
        } else {
            Acknowledgement::Delivered
        };
        if self.envelope.acknowledgement != expected_ack
            || time::OffsetDateTime::parse(
                &self.envelope.created_at,
                &time::format_description::well_known::Rfc3339,
            )
            .is_err()
        {
            return Err("shared message envelope state is invalid".into());
        }
        if self.completion.is_some() && self.response.is_none() {
            return Err("request completion requires a recorded response".into());
        }
        if let Some(path) = &self.context_artifact
            && (invalid_text(path, 16 * 1024) || path.trim().is_empty())
        {
            return Err("invalid request context artifact".into());
        }
        for result in [&self.response, &self.completion].into_iter().flatten() {
            TaskId::parse(result.id.clone()).map_err(|error| error.to_string())?;
            if result.recorded_at == 0
                || result.summary.trim().is_empty()
                || invalid_text(&result.summary, 16 * 1024)
                || result
                    .artifact
                    .as_ref()
                    .is_some_and(|path| invalid_text(path, 16 * 1024) || path.trim().is_empty())
            {
                return Err("invalid routed request result".into());
            }
        }
        Ok(())
    }

    fn same_submission(&self, request: &RequestSubmission<'_>, home: &Path) -> bool {
        self.batch_id == request.batch_id
            && self.request_id == request.request_id
            && self.task_id == request.task_id
            && self.client_id == request.client_id
            && self.recipient_home == home.to_string_lossy()
            && self.parent_task_id.as_deref() == request.parent_task_id
            && self.parent_home.as_deref() == request.parent_home
            && self.attempt_id.as_deref() == request.attempt_id
            && self.attempt_generation == request.attempt_generation
            && self.project_id == request.project_id
            && self.checkout_id == request.checkout_id
            && self.starting_revision == request.starting_revision
            && self.brief_revision == request.brief_revision
            && self.scope == request.scope
            && self.dependencies == request.dependencies
            && self.context_artifact.as_deref() == request.context_artifact
    }
}

fn invalid_text(value: &str, max: usize) -> bool {
    value.len() > max || value.chars().any(|character| character == '\0')
}

/// Immutable submission fields supplied by a terminal client.
pub struct RequestSubmission<'a> {
    pub batch_id: &'a str,
    pub request_id: &'a str,
    pub task_id: &'a str,
    pub client_id: &'a str,
    pub recipient_owner: Option<&'a ProcessIdentity>,
    pub parent_task_id: Option<&'a str>,
    pub parent_home: Option<&'a str>,
    pub attempt_id: Option<&'a str>,
    pub attempt_generation: Option<u64>,
    pub project_id: &'a str,
    pub checkout_id: &'a str,
    pub starting_revision: &'a str,
    pub brief_revision: u64,
    pub scope: &'a str,
    pub dependencies: &'a [String],
    pub context_artifact: Option<&'a str>,
}

/// Result of repeat-safe inbox acceptance.
#[derive(Debug)]
pub struct RequestAcceptance {
    pub request: RoutedRequest,
    pub newly_accepted: bool,
}

/// Filesystem owner for terminal client outbox and orchestrator inbox receipts.
pub struct RequestStore {
    state: PathBuf,
    recipient_home: PathBuf,
}

impl RequestStore {
    #[must_use]
    pub fn new(state: impl Into<PathBuf>, recipient_home: impl Into<PathBuf>) -> Self {
        Self {
            state: state.into(),
            recipient_home: recipient_home.into(),
        }
    }

    fn outbox_path(request_id: &str) -> PathBuf {
        Path::new("request-outbox").join(format!("{request_id}.json"))
    }

    fn inbox_path(request_id: &str) -> PathBuf {
        Path::new("request-inbox").join(format!("{request_id}.json"))
    }

    fn operation(prefix: &str, request_id: &str, suffix: Option<&str>) -> Result<String, String> {
        TaskId::parse(request_id.to_owned()).map_err(|error| error.to_string())?;
        if let Some(suffix) = suffix {
            TaskId::parse(suffix.to_owned()).map_err(|error| error.to_string())?;
        }
        Ok(match suffix {
            Some(suffix) => format!("request-{prefix}-{request_id}-{suffix}"),
            None => format!("request-{prefix}-{request_id}"),
        })
    }

    fn ensure_dirs(&self) -> Result<(), String> {
        fs::create_dir_all(self.state.join("request-outbox")).map_err(|error| error.to_string())?;
        fs::create_dir_all(self.state.join("request-inbox")).map_err(|error| error.to_string())
    }

    fn decode(bytes: &[u8]) -> Result<RoutedRequest, String> {
        let request: RoutedRequest =
            serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        request.validate()?;
        Ok(request)
    }

    fn read(&self, request_id: &str) -> Result<RoutedRequest, String> {
        Self::operation("read", request_id, None)?;
        let bytes = read_bounded_regular(
            self.state.join(Self::inbox_path(request_id)),
            MAX_REQUEST_BYTES,
        )
        .map_err(|error| error.to_string())?;
        let request = Self::decode(&bytes)?;
        self.validate_scope(&request)?;
        Ok(request)
    }

    fn validate_scope(&self, request: &RoutedRequest) -> Result<(), String> {
        let expected = fs::canonicalize(&self.recipient_home)
            .map_err(|error| format!("recipient home is unavailable: {error}"))?;
        let recorded = fs::canonicalize(&request.recipient_home)
            .map_err(|_| "request recipient home is unavailable")?;
        if expected != recorded {
            return Err("request belongs to a different recipient home".into());
        }
        Ok(())
    }

    /// Accept once using a durable two-record transition. The outbox is
    /// preparatory; the inbox write is the authoritative delivery point.
    pub fn submit(
        &self,
        request: &RequestSubmission<'_>,
        now: SystemTime,
        fault: Option<TransitionFault>,
    ) -> Result<RequestAcceptance, String> {
        self.ensure_dirs()?;
        let _request_lock = DirectoryLock::acquire_wait(
            self.state
                .join(format!(".request-{}.lock", request.request_id)),
            &multplx_core::process::SystemProcessProbe::default(),
            TRANSITION_LOCK_TIMEOUT,
        )
        .map_err(|error| error.to_string())?;
        let recipient_home = fs::canonicalize(&self.recipient_home)
            .map_err(|error| format!("recipient home is unavailable: {error}"))?;
        let operation = Self::operation("submit", request.request_id, None)?;
        let receipt = self
            .state
            .join(".transitions")
            .join(format!("{operation}.json"));
        if receipt.exists() {
            let writes = read_transition_writes(&self.state, &operation)
                .map_err(|error| error.to_string())?;
            let bytes = writes
                .last()
                .filter(|write| write.path == Self::inbox_path(request.request_id))
                .map(|write| write.after.as_slice())
                .ok_or("request receipt has no authoritative inbox write")?;
            let recorded = Self::decode(bytes)?;
            self.validate_scope(&recorded)?;
            if !recorded.same_submission(request, &recipient_home) {
                return Err("request identity conflicts with durable submission intent".into());
            }
            recover_transition_wait(&self.state, &operation)?;
            return Ok(RequestAcceptance {
                request: recorded,
                newly_accepted: false,
            });
        }
        let epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let created_at = time::OffsetDateTime::from_unix_timestamp(epoch as i64)
            .map_err(|_| "request time is outside RFC3339 range")?
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|error| error.to_string())?;
        let home_text = recipient_home.to_string_lossy().into_owned();
        let attempt = request.attempt_id.map(|id| Attempt {
            id: id.to_owned(),
            generation: request.attempt_generation.expect("validated attempt pair"),
            brief_revision: request.brief_revision,
        });
        let recorded = RoutedRequest {
            schema: REQUEST_SCHEMA.into(),
            batch_id: request.batch_id.into(),
            request_id: request.request_id.into(),
            task_id: request.task_id.into(),
            client_id: request.client_id.into(),
            recipient_home: home_text.clone(),
            recipient_owner: request.recipient_owner.cloned(),
            prior_recipient_owners: Vec::new(),
            parent_task_id: request.parent_task_id.map(str::to_owned),
            parent_home: request.parent_home.map(str::to_owned),
            attempt_id: request.attempt_id.map(str::to_owned),
            attempt_generation: request.attempt_generation,
            project_id: request.project_id.into(),
            checkout_id: request.checkout_id.into(),
            starting_revision: request.starting_revision.into(),
            brief_revision: request.brief_revision,
            scope: request.scope.into(),
            dependencies: request.dependencies.to_vec(),
            context_artifact: request.context_artifact.map(str::to_owned),
            created_at: epoch,
            delivered_at: epoch,
            notification_event_id: None,
            acknowledged_at: None,
            response: None,
            completion: None,
            envelope: MessageEnvelope {
                schema_version: SCHEMA_VERSION,
                message_id: request.request_id.to_owned(),
                task_id: request.task_id.to_owned(),
                task_home: Some(home_text.clone()),
                parent_home: request.parent_home.map(str::to_owned),
                attempt,
                parent_id: request.parent_task_id.map(str::to_owned),
                sender: request.client_id.to_owned(),
                recipient: home_text,
                brief_revision: Some(request.brief_revision),
                kind: "decision-request".into(),
                correlation_id: request.request_id.to_owned(),
                created_at,
                summary: request.scope.to_owned(),
                artifact: request.context_artifact.map(str::to_owned),
                acknowledgement: Acknowledgement::Delivered,
            },
        };
        recorded.validate()?;
        let bytes = serde_json::to_vec_pretty(&recorded).map_err(|error| error.to_string())?;
        let writes = [
            TransitionWrite {
                path: Self::outbox_path(request.request_id),
                before: None,
                after: bytes.clone(),
            },
            TransitionWrite {
                path: Self::inbox_path(request.request_id),
                before: None,
                after: bytes,
            },
        ];
        recoverable_transition_wait(&self.state, &operation, &writes, fault)?;
        Ok(RequestAcceptance {
            request: recorded,
            newly_accepted: true,
        })
    }

    fn update(
        &self,
        operation: &str,
        request_id: &str,
        mut change: impl FnMut(&mut RoutedRequest) -> Result<(), String>,
    ) -> Result<RoutedRequest, String> {
        self.ensure_dirs()?;
        let _request_lock = DirectoryLock::acquire_wait(
            self.state.join(format!(".request-{request_id}.lock")),
            &multplx_core::process::SystemProcessProbe::default(),
            TRANSITION_LOCK_TIMEOUT,
        )
        .map_err(|error| error.to_string())?;
        if self
            .state
            .join(".transitions")
            .join(format!("{operation}.json"))
            .exists()
        {
            let writes = read_transition_writes(&self.state, operation)
                .map_err(|error| error.to_string())?;
            let intended = writes
                .last()
                .filter(|write| write.path == Self::inbox_path(request_id))
                .map(|write| Self::decode(&write.after))
                .transpose()?
                .ok_or("request update receipt has no authoritative inbox write")?;
            let inbox = self.state.join(Self::inbox_path(request_id));
            if inbox.is_file() {
                let mut current = Self::decode(
                    &read_bounded_regular(&inbox, MAX_REQUEST_BYTES)
                        .map_err(|error| error.to_string())?,
                )?;
                self.validate_scope(&current)?;
                let before_change = current.clone();
                change(&mut current)?;
                if current == before_change {
                    return Ok(before_change);
                }
                if current != intended {
                    return Err("retry payload conflicts with durable request update intent".into());
                }
            }
            recover_transition_wait(&self.state, operation)?;
            let recovered = self.read(request_id)?;
            let mut verified = recovered.clone();
            change(&mut verified)?;
            if verified != recovered {
                return Err("recovered request update does not satisfy retry intent".into());
            }
            return Ok(recovered);
        }
        let before = read_bounded_regular(
            self.state.join(Self::inbox_path(request_id)),
            MAX_REQUEST_BYTES,
        )
        .map_err(|error| error.to_string())?;
        let outbox_before = read_bounded_regular(
            self.state.join(Self::outbox_path(request_id)),
            MAX_REQUEST_BYTES,
        )
        .map_err(|error| error.to_string())?;
        if before != outbox_before {
            return Err("request inbox/outbox receipts diverged".into());
        }
        let mut request = Self::decode(&before)?;
        self.validate_scope(&request)?;
        change(&mut request)?;
        request.validate()?;
        let after = serde_json::to_vec_pretty(&request).map_err(|error| error.to_string())?;
        recoverable_transition_wait(
            &self.state,
            operation,
            &[
                TransitionWrite {
                    path: Self::outbox_path(request_id),
                    before: Some(outbox_before),
                    after: after.clone(),
                },
                TransitionWrite {
                    path: Self::inbox_path(request_id),
                    before: Some(before),
                    after,
                },
            ],
            None,
        )?;
        Ok(request)
    }

    pub fn acknowledge(
        &self,
        request_id: &str,
        owner: &ProcessIdentity,
        now: SystemTime,
    ) -> Result<RoutedRequest, String> {
        let operation = Self::operation("ack", request_id, None)?;
        self.update(&operation, request_id, |request| {
            if request
                .recipient_owner
                .as_ref()
                .is_some_and(|bound| bound != owner)
            {
                return Err(
                    "request acknowledgement owner mismatch; rebind after fencing the prior owner"
                        .into(),
                );
            }
            request.recipient_owner.get_or_insert_with(|| owner.clone());
            request.acknowledged_at.get_or_insert_with(|| {
                now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
            });
            request.envelope.acknowledgement = Acknowledgement::Acknowledged;
            Ok(())
        })
    }

    /// Rebind handling after the home session lock has fenced a prior owner.
    /// The caller supplies both exact lifetimes so the prior delivery remains
    /// inspectable instead of being silently rewritten.
    pub fn rebind_owner(
        &self,
        request_id: &str,
        prior: &ProcessIdentity,
        current: &ProcessIdentity,
        processes: &impl multplx_core::process::ProcessProbe,
    ) -> Result<RoutedRequest, String> {
        if processes.is_alive(prior.pid)
            && processes
                .identity(prior.pid)
                .map_err(|error| error.to_string())?
                == *prior
        {
            return Err("request prior owner is still active".into());
        }
        if !processes.is_alive(current.pid)
            || processes
                .identity(current.pid)
                .map_err(|error| error.to_string())?
                != *current
        {
            return Err("request replacement owner is not active".into());
        }
        let lock_pid = fs::read_to_string(self.state.join(".lock"))
            .map_err(|error| format!("cannot verify request home owner: {error}"))?
            .trim()
            .parse::<u32>()
            .map_err(|_| "invalid request home owner lock")?;
        if lock_pid != current.pid {
            return Err("request replacement owner does not own the home session".into());
        }
        let mut hash = Sha256::new();
        hash.update(current.pid.to_le_bytes());
        hash.update(current.marker.as_bytes());
        let suffix = format!("{:x}", hash.finalize());
        let operation = Self::operation("owner", request_id, Some(&suffix[..32]))?;
        self.update(&operation, request_id, |request| {
            if request.recipient_owner.as_ref() == Some(current) {
                return Ok(());
            }
            if request.recipient_owner.as_ref() != Some(prior) {
                return Err("request prior owner mismatch".into());
            }
            request.prior_recipient_owners.push(
                request
                    .recipient_owner
                    .clone()
                    .expect("matched prior owner"),
            );
            request.recipient_owner = Some(current.clone());
            Ok(())
        })
    }

    /// Link acceptance to the durable notification published for this item.
    pub fn record_notification(
        &self,
        request_id: &str,
        event_id: &str,
    ) -> Result<RoutedRequest, String> {
        TaskId::parse(event_id.to_owned()).map_err(|error| error.to_string())?;
        let operation = Self::operation("notification", request_id, Some(event_id))?;
        self.update(&operation, request_id, |request| {
            match &request.notification_event_id {
                Some(existing) if existing == event_id => Ok(()),
                Some(_) => Err("request already links a different wake event".into()),
                None => {
                    request.notification_event_id = Some(event_id.to_owned());
                    Ok(())
                }
            }
        })
    }

    pub fn record_response(
        &self,
        request_id: &str,
        owner: &ProcessIdentity,
        response: RequestResult,
    ) -> Result<RoutedRequest, String> {
        let operation = Self::operation("response", request_id, Some(&response.id))?;
        self.update(&operation, request_id, |request| {
            if request.recipient_owner.as_ref() != Some(owner) || request.acknowledged_at.is_none()
            {
                return Err("request response requires owner acknowledgement".into());
            }
            match &request.response {
                Some(existing) if existing == &response => Ok(()),
                Some(_) => Err("request already has a different response".into()),
                None => {
                    request.response = Some(response.clone());
                    request.envelope.acknowledgement = Acknowledgement::Answered;
                    Ok(())
                }
            }
        })
    }

    pub fn record_completion(
        &self,
        request_id: &str,
        owner: &ProcessIdentity,
        completion: RequestResult,
    ) -> Result<RoutedRequest, String> {
        let operation = Self::operation("completion", request_id, Some(&completion.id))?;
        self.update(&operation, request_id, |request| {
            if request.recipient_owner.as_ref() != Some(owner) || request.response.is_none() {
                return Err("request completion requires an owner response".into());
            }
            match &request.completion {
                Some(existing) if existing == &completion => Ok(()),
                Some(_) => Err("request already has different completion evidence".into()),
                None => {
                    request.completion = Some(completion.clone());
                    request.envelope.acknowledgement = Acknowledgement::Completed;
                    Ok(())
                }
            }
        })
    }

    pub fn get(&self, request_id: &str) -> Result<RoutedRequest, String> {
        self.read(request_id)
    }

    /// Count accepted requests that have not yet been linked to a wake event.
    /// This snapshot performs no recovery, locking, or filesystem writes and is
    /// suitable for read-only session diagnostics.
    pub fn observe_unnotified_count(&self) -> Result<usize, String> {
        let directory = self.state.join("request-inbox");
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err("request inbox is not a regular directory".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.to_string()),
        }
        let mut count = 0;
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let request = Self::decode(
                &read_bounded_regular(&path, MAX_REQUEST_BYTES)
                    .map_err(|error| error.to_string())?,
            )?;
            self.validate_scope(&request)?;
            if path != self.state.join(Self::inbox_path(&request.request_id)) {
                return Err("request inbox filename does not match request identity".into());
            }
            count += usize::from(request.notification_event_id.is_none());
        }
        Ok(count)
    }

    /// Recover accepted requests whose process stopped before publishing or
    /// linking the durable wake. Existing queue/inbox identity makes this safe
    /// without a client retry.
    pub fn reconcile_notifications(
        &self,
        now: SystemTime,
        processes: &impl multplx_core::process::ProcessProbe,
    ) -> Result<usize, String> {
        self.ensure_dirs()?;
        let transitions = self.state.join(".transitions");
        if transitions.is_dir() {
            let mut receipts = fs::read_dir(&transitions)
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter_map(|name| {
                    name.strip_prefix("request-submit-")
                        .and_then(|name| name.strip_suffix(".json"))
                        .map(|request_id| format!("request-submit-{request_id}"))
                })
                .collect::<Vec<_>>();
            receipts.sort();
            for operation in receipts {
                recover_transition(&self.state, &operation).map_err(|error| error.to_string())?;
            }
        }
        let directory = self.state.join("request-inbox");
        let mut paths = fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        let mut reconciled = 0;
        for path in paths {
            let bytes = read_bounded_regular(&path, MAX_REQUEST_BYTES)
                .map_err(|error| error.to_string())?;
            let request = Self::decode(&bytes)?;
            self.validate_scope(&request)?;
            if path != self.state.join(Self::inbox_path(&request.request_id)) {
                return Err("request inbox filename does not match request identity".into());
            }
            if request.notification_event_id.is_some() {
                continue;
            }
            let key = format!("request-{}", request.request_id);
            let payload = format!(
                "request: batch={} request={} task={} project={} brief={}",
                request.batch_id,
                request.request_id,
                request.task_id,
                request.project_id,
                request.brief_revision
            );
            let (wake, _) = multplx_core::wake::WakeQueue::new(&self.state)
                .append_once(
                    multplx_core::wake::WakeKind::Signal,
                    &key,
                    &payload,
                    now,
                    processes,
                )
                .map_err(|error| error.to_string())?;
            self.record_notification(&request.request_id, &format!("wake-{:020}", wake.sequence))?;
            reconciled += 1;
        }
        Ok(reconciled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submission<'a>(
        owner: &'a ProcessIdentity,
        dependencies: &'a [String],
    ) -> RequestSubmission<'a> {
        RequestSubmission {
            batch_id: "batch-1",
            request_id: "request-1",
            task_id: "task-1",
            client_id: "terminal-1",
            recipient_owner: Some(owner),
            parent_task_id: None,
            parent_home: None,
            attempt_id: None,
            attempt_generation: None,
            project_id: "project-1",
            checkout_id: "checkout-1",
            starting_revision: "0123456789abcdef",
            brief_revision: 2,
            scope: "implement the accepted request",
            dependencies,
            context_artifact: Some("data/brief-2.md"),
        }
    }

    #[test]
    fn current_kinds_round_trip_literal_multiline_bodies() {
        for kind in [
            Kind::SessionStart,
            Kind::Watcher,
            Kind::TurnEndGuard,
            Kind::AwaySupervisor,
            Kind::LaunchBrief,
            Kind::FromBroker,
        ] {
            let payload = "line one\nline two\n";
            let encoded = construct(kind, payload).expect("encoded");
            assert_eq!(current_kind(&encoded), Some(kind));
            assert_eq!(body(&encoded), Some(payload));
            assert_eq!(classify(&encoded), Some(kind.as_str()));
        }
    }

    #[test]
    fn malformed_and_near_miss_inputs_remain_unclassified() {
        for value in [
            "MULTPLX_OP: v1 watcher: body",
            "\u{2063} arbitrary maintainer text",
            "[mx-from-broker] inspect this label",
        ] {
            assert_eq!(classify(value), None, "{value:?}");
        }
        for value in [
            "\u{2063}MULTPLX_OP: v2 watcher: body",
            "\u{2063}MULTPLX_OP: v1 unknown: body",
            "\u{2063}MULTPLX_OP: v1 watcher: ",
        ] {
            assert_eq!(current_kind(value), None, "{value:?}");
            assert_eq!(classify(value), Some("legacy-operational"), "{value:?}");
        }
        assert!(construct(Kind::Watcher, "").is_none());
    }

    #[test]
    fn legacy_shapes_are_isolated_from_current_parser() {
        let watcher = format!("{LEGACY_WATCHER_PREFIX}signal{LEGACY_WATCHER_SUFFIX}");
        let cases = [
            (LEGACY_SESSION_START, "session-start"),
            (&watcher, "watcher"),
            (
                "TURN WOULD END BLIND - supervision is off. The watcher cycle is missing, failed, or unhealthy. Follow the harness recovery instruction below before ending the turn.\n\nfailed",
                "turn-end-guard",
            ),
            ("\u{2063}Supervisor escalate (one)", "away-supervisor"),
            ("\u{2063}MULTPLX_OP: old", "legacy-operational"),
        ];
        for (message, expected) in cases {
            assert_eq!(current_kind(message), None);
            assert_eq!(legacy_kind(message), Some(expected));
        }
    }

    #[test]
    fn typed_codec_and_kind_parsing_cover_all_public_paths() {
        let codec = OperationalInputCodec;
        for (text, kind) in [
            ("session-start", Kind::SessionStart),
            ("watcher", Kind::Watcher),
            ("turn-end-guard", Kind::TurnEndGuard),
            ("away-supervisor", Kind::AwaySupervisor),
            ("from-broker", Kind::FromBroker),
            ("launch-brief", Kind::LaunchBrief),
        ] {
            assert_eq!(Kind::parse(text), Some(kind));
            assert_eq!(kind.to_string(), text);
            let encoded = codec.construct(kind, "payload").expect("construct");
            assert_eq!(codec.kind(&encoded), Some(kind));
            assert_eq!(codec.body(&encoded), Some("payload"));
            assert_eq!(codec.classify(&encoded), Some(text));
        }
        assert_eq!(Kind::parse("legacy-operational"), None);
        assert_eq!(codec.construct(Kind::Watcher, ""), None);
        assert_eq!(codec.kind("plain"), None);
        assert_eq!(codec.body("plain"), None);
        assert_eq!(codec.classify("plain"), None);
        assert_eq!(mark_from_broker("body"), format!("{FROM_BROKER_MARK}body"));
    }

    #[test]
    fn routed_request_recovers_each_transition_boundary_and_rejects_changed_retry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(&state).expect("state");
        let store = RequestStore::new(&state, &home);
        let owner = ProcessIdentity {
            pid: 42,
            marker: "process-generation-1".into(),
        };
        let dependencies = vec!["task-0".to_owned()];
        let request = submission(&owner, &dependencies);
        let error = store
            .submit(
                &request,
                UNIX_EPOCH + std::time::Duration::from_secs(10),
                Some(TransitionFault::AfterWrite(0)),
            )
            .expect_err("fault");
        assert!(error.contains("injected after write"));
        let accepted = store
            .submit(
                &request,
                UNIX_EPOCH + std::time::Duration::from_secs(99),
                None,
            )
            .expect("recover");
        assert!(!accepted.newly_accepted);
        assert_eq!(accepted.request.created_at, 10);
        assert_eq!(accepted.request.brief_revision, 2);
        assert_eq!(accepted.request.project_id, "project-1");
        assert_eq!(accepted.request.dependencies, dependencies);
        assert_eq!(
            fs::read(state.join("request-outbox/request-1.json")).expect("outbox"),
            fs::read(state.join("request-inbox/request-1.json")).expect("inbox")
        );

        let mut changed = submission(&owner, &[]);
        changed.project_id = "different-project";
        assert!(
            store
                .submit(&changed, SystemTime::now(), None)
                .expect_err("changed retry")
                .contains("conflicts")
        );
    }

    #[test]
    fn request_delivery_ack_response_and_completion_remain_distinct() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(&state).expect("state");
        let store = RequestStore::new(&state, &home);
        let owner = ProcessIdentity {
            pid: 42,
            marker: "process-generation-1".into(),
        };
        let wrong = ProcessIdentity {
            pid: 42,
            marker: "process-generation-0".into(),
        };
        let request = submission(&owner, &[]);
        let accepted = store
            .submit(
                &request,
                UNIX_EPOCH + std::time::Duration::from_secs(1),
                None,
            )
            .expect("submit");
        assert!(accepted.newly_accepted);
        assert!(accepted.request.acknowledged_at.is_none());
        assert!(accepted.request.response.is_none());
        assert!(accepted.request.completion.is_none());
        assert!(
            store
                .acknowledge("request-1", &wrong, SystemTime::now())
                .is_err()
        );

        let completion = RequestResult {
            id: "completion-1".into(),
            recorded_at: 4,
            summary: "implementation complete".into(),
            artifact: Some("data/result.md".into()),
        };
        assert!(
            store
                .record_completion("request-1", &owner, completion.clone())
                .is_err()
        );
        let acknowledged = store
            .acknowledge(
                "request-1",
                &owner,
                UNIX_EPOCH + std::time::Duration::from_secs(2),
            )
            .expect("acknowledge");
        assert_eq!(acknowledged.acknowledged_at, Some(2));
        let response = RequestResult {
            id: "response-1".into(),
            recorded_at: 3,
            summary: "accepted and queued".into(),
            artifact: None,
        };
        let responded = store
            .record_response("request-1", &owner, response.clone())
            .expect("response");
        assert_eq!(responded.response, Some(response));
        assert!(responded.completion.is_none());
        let conflicting_response = RequestResult {
            id: "response-1".into(),
            recorded_at: 3,
            summary: "different response".into(),
            artifact: None,
        };
        assert!(
            store
                .record_response("request-1", &owner, conflicting_response)
                .is_err()
        );
        let completed = store
            .record_completion("request-1", &owner, completion.clone())
            .expect("completion");
        assert_eq!(completed.completion, Some(completion));
        assert_eq!(
            store
                .record_completion(
                    "request-1",
                    &owner,
                    completed.completion.clone().expect("completion")
                )
                .expect("repeat-safe completion"),
            completed
        );
    }

    #[test]
    fn one_batch_accepts_independent_items_without_retargeting_context() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(&state).expect("state");
        let store = RequestStore::new(&state, &home);
        let owner = ProcessIdentity {
            pid: 42,
            marker: "process-generation-1".into(),
        };
        let first = submission(&owner, &[]);
        store
            .submit(&first, SystemTime::now(), None)
            .expect("first");
        let mut second = submission(&owner, &[]);
        second.request_id = "request-2";
        second.task_id = "task-2";
        second.project_id = "project-2";
        second.checkout_id = "checkout-2";
        second.starting_revision = "fedcba9876543210";
        store
            .submit(&second, SystemTime::now(), None)
            .expect("second");
        let mut third = submission(&owner, &[]);
        third.request_id = "request-3";
        third.task_id = "task-3";
        third.project_id = "project-3";
        third.checkout_id = "checkout-3";
        third.starting_revision = "0011223344556677";
        store
            .submit(&third, SystemTime::now(), None)
            .expect("third");
        assert_eq!(
            store.get("request-1").expect("first").project_id,
            "project-1"
        );
        assert_eq!(
            store.get("request-2").expect("second").project_id,
            "project-2"
        );
        assert_eq!(
            store.get("request-3").expect("third").project_id,
            "project-3"
        );
        assert_eq!(store.get("request-1").expect("first").batch_id, "batch-1");
        assert_eq!(store.get("request-2").expect("second").batch_id, "batch-1");
    }

    #[test]
    fn owner_recovery_and_notification_link_retain_prior_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(&state).expect("state");
        let store = RequestStore::new(&state, &home);
        let old = ProcessIdentity {
            pid: 999_999,
            marker: "process-generation-1".into(),
        };
        let processes = multplx_core::process::SystemProcessProbe::default();
        let current = multplx_core::process::ProcessProbe::identity(&processes, std::process::id())
            .expect("current identity");
        fs::write(state.join(".lock"), format!("{}\n", current.pid)).expect("owner lock");
        store
            .submit(&submission(&old, &[]), SystemTime::now(), None)
            .expect("submit");
        let notified = store
            .record_notification("request-1", "wake-00000000000000000001")
            .expect("notification");
        assert_eq!(
            notified.notification_event_id.as_deref(),
            Some("wake-00000000000000000001")
        );
        store
            .acknowledge("request-1", &old, SystemTime::now())
            .expect("old owner acknowledgement");
        let rebound = store
            .rebind_owner("request-1", &old, &current, &processes)
            .expect("rebind");
        assert_eq!(rebound.recipient_owner, Some(current.clone()));
        assert_eq!(rebound.prior_recipient_owners, vec![old]);
        assert!(
            store
                .acknowledge(
                    "request-1",
                    &rebound.prior_recipient_owners[0],
                    SystemTime::now()
                )
                .expect_err("historical owner cannot replay an old receipt")
                .contains("owner mismatch")
        );
        assert_eq!(
            store
                .rebind_owner(
                    "request-1",
                    &ProcessIdentity {
                        pid: 888_888,
                        marker: "wrong-prior".into(),
                    },
                    &current,
                    &processes,
                )
                .expect("current owner retry"),
            rebound
        );
    }

    #[test]
    fn offline_acceptance_is_visible_and_current_owner_can_adopt_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(&state).expect("state");
        let store = RequestStore::new(&state, &home);
        let placeholder = ProcessIdentity {
            pid: 1,
            marker: "unused".into(),
        };
        let mut request = submission(&placeholder, &[]);
        request.recipient_owner = None;
        store
            .submit(
                &request,
                UNIX_EPOCH + std::time::Duration::from_secs(5),
                None,
            )
            .expect("offline acceptance");
        assert_eq!(store.observe_unnotified_count().expect("unnotified"), 1);
        let owner = ProcessIdentity {
            pid: 44,
            marker: "current".into(),
        };
        let adopted = store
            .acknowledge(
                "request-1",
                &owner,
                UNIX_EPOCH + std::time::Duration::from_secs(6),
            )
            .expect("adopt");
        assert_eq!(adopted.recipient_owner, Some(owner));
        assert_eq!(
            adopted.envelope.acknowledgement,
            Acknowledgement::Acknowledged
        );
    }

    #[test]
    fn shared_message_receipt_rejects_identity_reuse_and_advances_monotonically() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let envelope = MessageEnvelope {
            schema_version: SCHEMA_VERSION,
            message_id: "message-1".into(),
            task_id: "task-1".into(),
            task_home: Some("/tmp/home".into()),
            parent_home: Some("/tmp/parent".into()),
            attempt: Some(Attempt {
                id: "attempt-1".into(),
                generation: 1,
                brief_revision: 2,
            }),
            parent_id: Some("parent-1".into()),
            sender: "parent-1".into(),
            recipient: "task-1".into(),
            brief_revision: Some(2),
            kind: "task-note".into(),
            correlation_id: "correlation-1".into(),
            created_at: "2026-09-15T12:00:00Z".into(),
            summary: "please inspect".into(),
            artifact: None,
            acknowledgement: Acknowledgement::Pending,
        };
        assert_eq!(
            persist_message_envelope(&state, &envelope).expect("persist"),
            envelope
        );
        assert_eq!(
            persist_message_envelope(&state, &envelope).expect("repeat"),
            envelope
        );
        let mut conflict = envelope.clone();
        conflict.summary = "different".into();
        assert!(persist_message_envelope(&state, &conflict).is_err());
        let delivered = advance_message_envelope(&state, "message-1", Acknowledgement::Delivered)
            .expect("delivered");
        assert_eq!(delivered.acknowledgement, Acknowledgement::Delivered);
        assert_eq!(
            advance_message_envelope(&state, "message-1", Acknowledgement::Pending)
                .expect("monotonic retry")
                .acknowledgement,
            Acknowledgement::Delivered
        );
        assert_eq!(
            read_message_envelope(&state, "message-1")
                .expect("read")
                .summary,
            "please inspect"
        );
        assert!(read_message_envelope(&state, "missing").is_err());
        let processes = multplx_core::process::SystemProcessProbe::default();
        let wake = publish_message_wake(
            &state,
            &envelope,
            "done: message-1",
            SystemTime::now(),
            &processes,
        )
        .expect("publish wake");
        assert_eq!(
            publish_message_wake(
                &state,
                &envelope,
                "done: message-1",
                SystemTime::now(),
                &processes,
            )
            .expect("repeat publish"),
            wake
        );
        assert!(
            publish_message_wake(
                &state,
                &envelope,
                "different payload",
                SystemTime::now(),
                &processes,
            )
            .is_err()
        );
    }

    #[test]
    fn routed_request_validation_rejects_corrupt_bindings_and_result_order() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(&state).expect("state");
        let owner = ProcessIdentity {
            pid: 42,
            marker: "owner-generation".into(),
        };
        let store = RequestStore::new(&state, &home);
        let mut request = submission(&owner, &[]);
        request.parent_task_id = Some("parent-1");
        request.parent_home = Some("/tmp/parent");
        request.attempt_id = Some("attempt-1");
        request.attempt_generation = Some(3);
        let accepted = store
            .submit(&request, UNIX_EPOCH + Duration::from_secs(10), None)
            .expect("submit")
            .request;
        assert_eq!(
            accepted
                .envelope
                .attempt
                .as_ref()
                .expect("attempt")
                .generation,
            3
        );

        let mut invalid = accepted.clone();
        invalid.project_id = ".hidden".into();
        assert!(invalid.validate().is_err());
        let mut invalid = accepted.clone();
        invalid.dependencies.push("../escape".into());
        assert!(invalid.validate().is_err());
        let mut invalid = accepted.clone();
        invalid.schema = "future".into();
        assert!(invalid.validate().is_err());
        let mut invalid = accepted.clone();
        invalid.attempt_generation = None;
        assert!(invalid.validate().is_err());
        let mut invalid = accepted.clone();
        invalid.parent_home = Some("relative".into());
        assert!(invalid.validate().is_err());
        let mut invalid = accepted.clone();
        invalid.envelope.summary = "changed".into();
        assert!(invalid.validate().is_err());
        let mut invalid = accepted.clone();
        invalid.envelope.created_at = "not-a-time".into();
        assert!(invalid.validate().is_err());
        let mut invalid = accepted.clone();
        invalid.response = Some(RequestResult {
            id: "response-1".into(),
            recorded_at: 0,
            summary: "recorded".into(),
            artifact: None,
        });
        invalid.envelope.acknowledgement = Acknowledgement::Answered;
        assert!(invalid.validate().is_err());
        let mut invalid = accepted.clone();
        invalid.response = Some(RequestResult {
            id: "response-1".into(),
            recorded_at: 11,
            summary: "recorded".into(),
            artifact: None,
        });
        invalid.completion = Some(RequestResult {
            id: "completion-1".into(),
            recorded_at: 12,
            summary: " ".into(),
            artifact: Some("".into()),
        });
        invalid.envelope.acknowledgement = Acknowledgement::Completed;
        assert!(invalid.validate().is_err());
        let mut invalid = accepted;
        invalid.completion = Some(RequestResult {
            id: "completion-1".into(),
            recorded_at: 12,
            summary: "complete".into(),
            artifact: None,
        });
        invalid.envelope.acknowledgement = Acknowledgement::Completed;
        assert!(invalid.validate().is_err());
    }

    #[derive(Default)]
    struct AdoptionProcesses {
        identities: std::collections::HashMap<u32, ProcessIdentity>,
    }

    impl multplx_core::process::ProcessProbe for AdoptionProcesses {
        fn is_alive(&self, pid: u32) -> bool {
            self.identities.contains_key(&pid)
        }

        fn identity(&self, pid: u32) -> multplx_core::error::Result<ProcessIdentity> {
            self.identities.get(&pid).cloned().ok_or(
                multplx_core::error::CoreError::InvalidIdentifier {
                    kind: "fixture PID",
                    value: pid.to_string(),
                },
            )
        }

        fn ancestry_row(
            &self,
            pid: u32,
        ) -> multplx_core::error::Result<multplx_core::process::AncestryRow> {
            Err(multplx_core::error::CoreError::InvalidIdentifier {
                kind: "fixture PID",
                value: pid.to_string(),
            })
        }
    }

    #[test]
    fn owner_adoption_and_receipt_observation_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(&state).expect("state");
        let store = RequestStore::new(&state, &home);
        let prior = ProcessIdentity {
            pid: 41,
            marker: "prior".into(),
        };
        let current = ProcessIdentity {
            pid: 42,
            marker: "current".into(),
        };
        store
            .submit(&submission(&prior, &[]), SystemTime::now(), None)
            .expect("submit");

        let mut processes = AdoptionProcesses::default();
        processes.identities.insert(prior.pid, prior.clone());
        processes.identities.insert(current.pid, current.clone());
        assert!(
            store
                .rebind_owner("request-1", &prior, &current, &processes)
                .expect_err("live prior")
                .contains("still active")
        );
        processes.identities.remove(&prior.pid);
        processes.identities.remove(&current.pid);
        assert!(
            store
                .rebind_owner("request-1", &prior, &current, &processes)
                .expect_err("inactive replacement")
                .contains("not active")
        );
        processes.identities.insert(current.pid, current.clone());
        assert!(
            store
                .rebind_owner("request-1", &prior, &current, &processes)
                .expect_err("missing owner lock")
                .contains("cannot verify")
        );
        fs::write(state.join(".lock"), "invalid\n").expect("invalid lock");
        assert!(
            store
                .rebind_owner("request-1", &prior, &current, &processes)
                .expect_err("invalid lock")
                .contains("invalid")
        );
        fs::write(state.join(".lock"), "43\n").expect("wrong lock");
        assert!(
            store
                .rebind_owner("request-1", &prior, &current, &processes)
                .expect_err("wrong owner")
                .contains("does not own")
        );
        fs::write(state.join(".lock"), "42\n").expect("owner lock");
        let wrong_prior = ProcessIdentity {
            pid: 99,
            marker: "wrong".into(),
        };
        assert!(
            store
                .rebind_owner("request-1", &wrong_prior, &current, &processes)
                .expect_err("prior mismatch")
                .contains("prior owner mismatch")
        );
        store
            .record_notification("request-1", "wake-00000000000000000001")
            .expect("notification");
        assert!(
            store
                .record_notification("request-1", "wake-00000000000000000002")
                .expect_err("notification conflict")
                .contains("different wake")
        );
        let response = RequestResult {
            id: "response-1".into(),
            recorded_at: 1,
            summary: "response".into(),
            artifact: None,
        };
        assert!(
            store
                .record_response("request-1", &prior, response)
                .expect_err("response before acknowledgement")
                .contains("requires owner acknowledgement")
        );

        fs::write(state.join("request-inbox/ignored.txt"), "ignored").expect("ignored");
        assert_eq!(store.observe_unnotified_count().expect("observed"), 0);
        let receipt = fs::read(state.join("request-inbox/request-1.json")).expect("receipt");
        fs::write(state.join("request-inbox/wrong-name.json"), receipt).expect("wrong name");
        assert!(
            store
                .observe_unnotified_count()
                .expect_err("filename mismatch")
                .contains("filename")
        );

        let other = tempfile::tempdir().expect("other");
        fs::create_dir_all(other.path().join("state/request-inbox")).expect("inbox");
        fs::write(
            other.path().join("state/request-inbox/bad.json"),
            b"not json",
        )
        .expect("bad receipt");
        assert!(
            RequestStore::new(other.path().join("state"), other.path())
                .observe_unnotified_count()
                .is_err()
        );
        let file_state = tempfile::tempdir().expect("file state");
        fs::create_dir(file_state.path().join("state")).expect("state");
        fs::write(file_state.path().join("state/request-inbox"), b"file").expect("file");
        assert!(
            RequestStore::new(file_state.path().join("state"), file_state.path())
                .observe_unnotified_count()
                .is_err()
        );
    }

    #[test]
    fn request_submit_waits_for_brief_shared_transition_lock_contention() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(&state).expect("state");
        let owner = ProcessIdentity {
            pid: 42,
            marker: "owner".into(),
        };
        let dependencies = Vec::new();
        let lock = DirectoryLock::try_acquire(
            state.join(".transition.lock"),
            &multplx_core::process::SystemProcessProbe::default(),
        )
        .expect("transition lock");
        let child_state = state.clone();
        let child_home = home.clone();
        let handle = std::thread::spawn(move || {
            RequestStore::new(&child_state, &child_home).submit(
                &submission(&owner, &dependencies),
                UNIX_EPOCH + Duration::from_secs(1),
                None,
            )
        });
        std::thread::sleep(Duration::from_millis(30));
        drop(lock);
        assert!(handle.join().expect("submit thread").is_ok());

        let writes = [TransitionWrite {
            path: PathBuf::from("recovered.txt"),
            before: None,
            after: b"durable".to_vec(),
        }];
        assert!(
            recoverable_transition_wait(
                &state,
                "fixture-recovery",
                &writes,
                Some(TransitionFault::AfterWrite(0)),
            )
            .is_err()
        );
        let lock = DirectoryLock::try_acquire(
            state.join(".transition.lock"),
            &multplx_core::process::SystemProcessProbe::default(),
        )
        .expect("recovery lock");
        let child_state = state.clone();
        let recovery =
            std::thread::spawn(move || recover_transition_wait(&child_state, "fixture-recovery"));
        std::thread::sleep(Duration::from_millis(30));
        drop(lock);
        recovery
            .join()
            .expect("recovery thread")
            .expect("recover after contention");
        assert_eq!(
            fs::read(state.join("recovered.txt")).expect("record"),
            b"durable"
        );
        assert!(recover_transition_wait(&state, "missing-operation").is_err());
    }
}

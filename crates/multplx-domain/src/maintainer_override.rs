//! Exact, single-use maintainer policy exceptions.
//!
//! This module is the typed owner of the registry, closed JSON schema, record
//! validation, and lifecycle transition table.  The retained shell library is
//! a source-compatible adapter for callers that move in Portion 11.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use multplx_core::filesystem::atomic_replace;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::{ProcessProbe, SystemProcessProbe};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_TTL: u64 = 3600;
const STATES: [RecordState; 5] = [
    RecordState::Pending,
    RecordState::Granted,
    RecordState::Denied,
    RecordState::Consumed,
    RecordState::Stale,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryClass {
    Policy,
    Integrity,
    Capability,
}

impl BoundaryClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Integrity => "integrity",
            Self::Capability => "capability",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Boundary {
    pub id: &'static str,
    pub class: BoundaryClass,
    pub alternate: &'static str,
}

pub const REGISTRY: [Boundary; 20] = [
    Boundary {
        id: "workflow.skip-stage",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-workflow.sh",
    },
    Boundary {
        id: "workflow.reorder-stage",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-workflow.sh",
    },
    Boundary {
        id: "validation.waive-gate",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-deep-review.sh",
    },
    Boundary {
        id: "delivery.merge-red",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-pr-merge.sh",
    },
    Boundary {
        id: "cleanup.discard-unlanded",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-teardown.sh",
    },
    Boundary {
        id: "project.direct-write",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-override-run.sh",
    },
    Boundary {
        id: "isolation.single-checkout",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-spawn.sh",
    },
    Boundary {
        id: "session.terminate-owner",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-lock.sh",
    },
    Boundary {
        id: "security.one-action-elevation",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-override-run.sh",
    },
    Boundary {
        id: "delivery.credentialed-action",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-maintainer-override.sh handoff",
    },
    Boundary {
        id: "dependency.install",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-override-run.sh",
    },
    Boundary {
        id: "authentication.login",
        class: BoundaryClass::Policy,
        alternate: "bin/mx-maintainer-override.sh handoff",
    },
    Boundary {
        id: "integrity.validation-state",
        class: BoundaryClass::Integrity,
        alternate: "coded alternate required; facts remain unchanged",
    },
    Boundary {
        id: "integrity.object-identity",
        class: BoundaryClass::Integrity,
        alternate: "coded alternate required; facts remain unchanged",
    },
    Boundary {
        id: "integrity.session-lock",
        class: BoundaryClass::Integrity,
        alternate: "coded alternate required; facts remain unchanged",
    },
    Boundary {
        id: "integrity.worktree-isolation",
        class: BoundaryClass::Integrity,
        alternate: "isolation.single-checkout",
    },
    Boundary {
        id: "capability.tool-unavailable",
        class: BoundaryClass::Capability,
        alternate: "dependency.install or operator handoff",
    },
    Boundary {
        id: "capability.authentication-required",
        class: BoundaryClass::Capability,
        alternate: "authentication.login or operator handoff",
    },
    Boundary {
        id: "capability.credential-unavailable",
        class: BoundaryClass::Capability,
        alternate: "delivery.credentialed-action or operator handoff",
    },
    Boundary {
        id: "capability.host-restriction",
        class: BoundaryClass::Capability,
        alternate: "operator handoff only",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordState {
    Pending,
    Granted,
    Denied,
    Consumed,
    Stale,
}

impl RecordState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Granted => "granted",
            Self::Denied => "denied",
            Self::Consumed => "consumed",
            Self::Stale => "stale",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    Pending,
    Granted,
    Denied,
    Consumed,
    Stale,
}

impl Decision {
    fn state(&self) -> RecordState {
        match self {
            Self::Pending => RecordState::Pending,
            Self::Granted => RecordState::Granted,
            Self::Denied => RecordState::Denied,
            Self::Consumed => RecordState::Consumed,
            Self::Stale => RecordState::Stale,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Pending,
    NotRun,
    Succeeded,
    Failed,
    Denied,
    Expired,
    StateChanged,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OverrideRecord {
    pub schema_version: u32,
    pub request_id: String,
    pub boundary_id: String,
    pub boundary_class: String,
    pub task_id: String,
    pub project: String,
    pub action_argv_or_operation: String,
    pub action_digest: String,
    pub target_identity: String,
    pub expected_state_digest: String,
    pub consequence: String,
    pub requested_at: u64,
    pub expires_at: u64,
    pub decision: Decision,
    pub decided_at: Option<u64>,
    pub maintainer_words_digest: Option<String>,
    pub consumed_at: Option<u64>,
    pub outcome: Outcome,
    pub outcome_digest: Option<String>,
    pub alternate: String,
}

#[derive(Clone, Debug)]
pub struct Request<'a> {
    pub boundary: &'a str,
    pub task: &'a str,
    pub project: &'a str,
    pub operation: &'a str,
    pub target: &'a str,
    pub expected_state_digest: &'a str,
    pub consequence: &'a str,
    pub ttl: u64,
}

#[derive(Clone, Debug)]
pub struct Binding<'a> {
    pub boundary: &'a str,
    pub task: &'a str,
    pub project: &'a str,
    pub operation: &'a str,
    pub target: &'a str,
    pub expected_state_digest: &'a str,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct OverrideError(String);

impl OverrideError {
    fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

pub type Result<T> = std::result::Result<T, OverrideError>;

#[must_use]
pub fn sha256_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[must_use]
pub fn boundary(id: &str) -> Option<&'static Boundary> {
    REGISTRY.iter().find(|entry| entry.id == id)
}

#[must_use]
pub fn registry_text() -> String {
    REGISTRY
        .iter()
        .map(|entry| {
            format!(
                "{}\t{}\t{}\n",
                entry.id,
                entry.class.as_str(),
                entry.alternate
            )
        })
        .collect()
}

fn slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn one_line(value: &str) -> bool {
    !value.is_empty() && !value.contains(['\n', '\r'])
}

impl OverrideRecord {
    pub fn validate(&self, expected: Option<RecordState>) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION
            || !slug(&self.request_id)
            || !slug(&self.boundary_id)
            || self.boundary_class != "policy"
            || !slug(&self.task_id)
            || !slug(&self.project)
            || self.action_argv_or_operation.is_empty()
            || !digest(&self.action_digest)
            || !one_line(&self.target_identity)
            || !digest(&self.expected_state_digest)
            || !one_line(&self.consequence)
            || self.requested_at == 0
            || self.expires_at <= self.requested_at
        {
            return Err(OverrideError::new(
                "override record violates the closed schema",
            ));
        }
        let registered = boundary(&self.boundary_id)
            .filter(|entry| entry.class == BoundaryClass::Policy)
            .ok_or_else(|| OverrideError::new("record boundary is not a policy exception"))?;
        if self.alternate != registered.alternate
            || self.action_digest != sha256_text(&self.action_argv_or_operation)
        {
            return Err(OverrideError::new(
                "record binding digest or alternate changed",
            ));
        }
        let shape = match self.decision {
            Decision::Pending => {
                self.decided_at.is_none()
                    && self.maintainer_words_digest.is_none()
                    && self.consumed_at.is_none()
                    && self.outcome == Outcome::Pending
                    && self.outcome_digest.is_none()
            }
            Decision::Granted => {
                self.decided_at.is_some()
                    && self.maintainer_words_digest.as_deref().is_some_and(digest)
                    && self.consumed_at.is_none()
                    && self.outcome == Outcome::Pending
                    && self.outcome_digest.is_none()
            }
            Decision::Denied => {
                self.decided_at.is_some()
                    && self.maintainer_words_digest.as_deref().is_some_and(digest)
                    && self.consumed_at.is_none()
                    && self.outcome == Outcome::Denied
                    && self.outcome_digest.as_deref().is_some_and(digest)
            }
            Decision::Consumed => {
                self.decided_at.is_some()
                    && self.maintainer_words_digest.as_deref().is_some_and(digest)
                    && self.consumed_at.is_some()
                    && matches!(
                        self.outcome,
                        Outcome::NotRun | Outcome::Succeeded | Outcome::Failed
                    )
                    && if self.outcome == Outcome::NotRun {
                        self.outcome_digest.is_none()
                    } else {
                        self.outcome_digest.as_deref().is_some_and(digest)
                    }
            }
            Decision::Stale => {
                self.decided_at.is_some()
                    && matches!(self.outcome, Outcome::Expired | Outcome::StateChanged)
                    && self.outcome_digest.as_deref().is_some_and(digest)
            }
        };
        if !shape || expected.is_some_and(|state| state != self.decision.state()) {
            return Err(OverrideError::new(
                "override record transition shape is invalid",
            ));
        }
        Ok(())
    }

    pub fn grant(&self, words: &str, now: u64) -> Result<Self> {
        self.validate(Some(RecordState::Pending))?;
        if now >= self.expires_at {
            return Err(OverrideError::new("request expired before decision"));
        }
        if words.is_empty()
            || !words.contains(&self.boundary_id)
            || !words.contains(&self.target_identity)
            || !words.contains(&self.action_argv_or_operation)
        {
            return Err(OverrideError::new(
                "grant words must name the exact boundary, target, and operation",
            ));
        }
        let mut next = self.clone();
        next.decision = Decision::Granted;
        next.decided_at = Some(now);
        next.maintainer_words_digest = Some(sha256_text(words));
        next.validate(Some(RecordState::Granted))?;
        Ok(next)
    }

    pub fn deny(&self, words: &str, now: u64) -> Result<Self> {
        self.validate(Some(RecordState::Pending))?;
        if words.is_empty() {
            return Err(OverrideError::new("maintainer words must not be empty"));
        }
        let mut next = self.clone();
        next.decision = Decision::Denied;
        next.decided_at = Some(now);
        next.maintainer_words_digest = Some(sha256_text(words));
        next.outcome = Outcome::Denied;
        next.outcome_digest = Some(sha256_text("denied"));
        next.validate(Some(RecordState::Denied))?;
        Ok(next)
    }

    pub fn consume(&self, binding: &Binding<'_>, now: u64) -> Result<Self> {
        self.validate(Some(RecordState::Granted))?;
        let mismatch = if now >= self.expires_at {
            Some(Outcome::Expired)
        } else if self.boundary_id != binding.boundary
            || self.task_id != binding.task
            || self.project != binding.project
            || self.action_digest != sha256_text(binding.operation)
            || self.target_identity != binding.target
            || self.expected_state_digest != binding.expected_state_digest
        {
            Some(Outcome::StateChanged)
        } else {
            None
        };
        let mut next = self.clone();
        next.consumed_at = Some(now);
        if let Some(outcome) = mismatch {
            let label = if outcome == Outcome::Expired {
                "expired"
            } else {
                "state-changed"
            };
            next.decision = Decision::Stale;
            next.outcome = outcome;
            next.outcome_digest = Some(sha256_text(label));
            next.validate(Some(RecordState::Stale))?;
            return Err(OverrideError::new(
                "grant binding changed or expired; a new maintainer decision is required",
            ));
        }
        next.decision = Decision::Consumed;
        next.outcome = Outcome::NotRun;
        next.validate(Some(RecordState::Consumed))?;
        Ok(next)
    }

    pub fn record_result(&self, succeeded: bool, detail: &str) -> Result<Self> {
        self.validate(Some(RecordState::Consumed))?;
        if self.outcome != Outcome::NotRun {
            return Err(OverrideError::new(
                "consumed request already records an outcome",
            ));
        }
        if detail.is_empty() {
            return Err(OverrideError::new("result detail must not be empty"));
        }
        let mut next = self.clone();
        next.outcome = if succeeded {
            Outcome::Succeeded
        } else {
            Outcome::Failed
        };
        next.outcome_digest = Some(sha256_text(detail));
        next.validate(Some(RecordState::Consumed))?;
        Ok(next)
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn request_id(seed: &str, now: u64) -> String {
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let digest = sha256_text(&format!("{now}:{}:{sequence}:{seed}", std::process::id()));
    let format =
        time::format_description::parse_borrowed::<2>("[year][month][day][hour][minute][second]")
            .expect("static timestamp format");
    let timestamp = time::OffsetDateTime::from_unix_timestamp(now as i64)
        .ok()
        .and_then(|value| value.format(&format).ok())
        .unwrap_or_else(|| now.to_string());
    format!("mo-{timestamp}-{}", &digest[..12])
}

pub struct OverrideStore {
    root: PathBuf,
}

impl OverrideStore {
    #[must_use]
    pub fn new(state: &Path) -> Self {
        Self {
            root: state.join("maintainer-overrides"),
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn prepare(&self) -> Result<()> {
        if let Ok(metadata) = fs::symlink_metadata(&self.root)
            && (!metadata.is_dir() || metadata.file_type().is_symlink())
        {
            return Err(OverrideError::new(format!(
                "override root is not a real directory: {}",
                self.root.display()
            )));
        }
        fs::create_dir_all(&self.root).map_err(|error| OverrideError::new(error.to_string()))?;
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
            .map_err(|error| OverrideError::new(error.to_string()))?;
        for state in STATES {
            let path = self.root.join(state.as_str());
            if let Ok(metadata) = fs::symlink_metadata(&path)
                && (!metadata.is_dir() || metadata.file_type().is_symlink())
            {
                return Err(OverrideError::new(format!(
                    "override state path is not a real directory: {}",
                    path.display()
                )));
            }
            fs::create_dir_all(&path).map_err(|error| OverrideError::new(error.to_string()))?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .map_err(|error| OverrideError::new(error.to_string()))?;
        }
        Ok(())
    }

    fn lock(&self) -> Result<DirectoryLock> {
        self.prepare()?;
        let path = self.root.join(".transition.lock");
        let processes = SystemProcessProbe::default();
        for _ in 0..500 {
            match DirectoryLock::try_acquire(&path, &processes) {
                Ok(lock) => return Ok(lock),
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
        Err(OverrideError::new(
            "could not acquire override transition lock",
        ))
    }

    fn path(&self, state: RecordState, request: &str) -> Result<PathBuf> {
        if !slug(request) {
            return Err(OverrideError::new("invalid request id"));
        }
        Ok(self
            .root
            .join(state.as_str())
            .join(format!("{request}.json")))
    }

    fn secure(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.permissions().mode() & 0o777 == 0o600
                && metadata.nlink() == 1
        })
    }

    fn locations(&self, request: &str) -> Result<Vec<(RecordState, PathBuf)>> {
        let mut found = Vec::new();
        for state in STATES {
            let path = self.path(state, request)?;
            if fs::symlink_metadata(&path).is_ok() {
                found.push((state, path));
            }
        }
        Ok(found)
    }

    pub fn find(&self, request: &str) -> Result<(RecordState, PathBuf, OverrideRecord)> {
        let locations = self.locations(request)?;
        if locations.len() != 1 {
            return Err(OverrideError::new(format!(
                "request identity is duplicated, missing, or misplaced: {request}"
            )));
        }
        let (state, path) = locations.into_iter().next().expect("one location");
        if !Self::secure(&path) {
            return Err(OverrideError::new(
                "override record is not a private single-link file",
            ));
        }
        let bytes = fs::read(&path).map_err(|error| OverrideError::new(error.to_string()))?;
        let record: OverrideRecord = serde_json::from_slice(&bytes)
            .map_err(|error| OverrideError::new(error.to_string()))?;
        record.validate(Some(state))?;
        Ok((state, path, record))
    }

    fn publish(&self, state: RecordState, record: &OverrideRecord) -> Result<PathBuf> {
        let path = self.path(state, &record.request_id)?;
        if fs::symlink_metadata(&path).is_ok() {
            return Err(OverrideError::new("override destination already exists"));
        }
        let mut bytes =
            serde_json::to_vec(record).map_err(|error| OverrideError::new(error.to_string()))?;
        bytes.push(b'\n');
        atomic_replace(&path, &bytes, 0o600)
            .map_err(|error| OverrideError::new(error.to_string()))?;
        Ok(path)
    }

    fn transition(
        &self,
        source: &Path,
        state: RecordState,
        record: &OverrideRecord,
    ) -> Result<PathBuf> {
        let destination = self.publish(state, record)?;
        fs::remove_file(source).map_err(|error| OverrideError::new(error.to_string()))?;
        Ok(destination)
    }

    pub fn request(&self, request: &Request<'_>) -> Result<String> {
        let registered = boundary(request.boundary)
            .filter(|entry| entry.class == BoundaryClass::Policy)
            .ok_or_else(|| {
                OverrideError::new(format!(
                    "boundary is not a registered policy exception: {}",
                    request.boundary
                ))
            })?;
        if !slug(request.task) || !slug(request.project) {
            return Err(OverrideError::new("invalid task or project id"));
        }
        if request.operation.is_empty()
            || !one_line(request.target)
            || !digest(request.expected_state_digest)
            || !one_line(request.consequence)
            || request.ttl == 0
        {
            return Err(OverrideError::new("request binding fields are invalid"));
        }
        let _lock = self.lock()?;
        let now = now_epoch();
        let id = request_id(
            &format!("{}:{}:{}", request.boundary, request.task, request.target),
            now,
        );
        let record = OverrideRecord {
            schema_version: SCHEMA_VERSION,
            request_id: id.clone(),
            boundary_id: request.boundary.to_owned(),
            boundary_class: "policy".to_owned(),
            task_id: request.task.to_owned(),
            project: request.project.to_owned(),
            action_argv_or_operation: request.operation.to_owned(),
            action_digest: sha256_text(request.operation),
            target_identity: request.target.to_owned(),
            expected_state_digest: request.expected_state_digest.to_owned(),
            consequence: request.consequence.to_owned(),
            requested_at: now,
            expires_at: now.saturating_add(request.ttl),
            decision: Decision::Pending,
            decided_at: None,
            maintainer_words_digest: None,
            consumed_at: None,
            outcome: Outcome::Pending,
            outcome_digest: None,
            alternate: registered.alternate.to_owned(),
        };
        record.validate(Some(RecordState::Pending))?;
        self.publish(RecordState::Pending, &record)?;
        Ok(id)
    }

    pub fn decide(&self, request: &str, words: &str, grant: bool) -> Result<()> {
        let _lock = self.lock()?;
        let (state, path, record) = self.find(request)?;
        if state != RecordState::Pending {
            return Err(OverrideError::new(
                "pending request is missing, unsafe, or invalid",
            ));
        }
        let now = now_epoch();
        if now >= record.expires_at {
            let mut stale = record;
            stale.decision = Decision::Stale;
            stale.decided_at = Some(now);
            stale.outcome = Outcome::Expired;
            stale.outcome_digest = Some(sha256_text("expired"));
            self.transition(&path, RecordState::Stale, &stale)?;
            return Err(OverrideError::new(format!(
                "request expired before decision: {request}"
            )));
        }
        let next = if grant {
            record.grant(words, now)?
        } else {
            record.deny(words, now)?
        };
        self.transition(&path, next.decision.state(), &next)?;
        Ok(())
    }

    pub fn consume(&self, request: &str, binding: &Binding<'_>) -> Result<PathBuf> {
        let _lock = self.lock()?;
        let (state, path, record) = self.find(request)?;
        if state != RecordState::Granted {
            return Err(OverrideError::new(format!(
                "granted request is missing, unsafe, invalid, or already consumed: {request}"
            )));
        }
        let now = now_epoch();
        match record.consume(binding, now) {
            Ok(next) => self.transition(&path, RecordState::Consumed, &next),
            Err(error) => {
                let mut stale = record;
                stale.decision = Decision::Stale;
                stale.decided_at = stale.decided_at.or(Some(now));
                stale.consumed_at = Some(now);
                let expired = now >= stale.expires_at;
                stale.outcome = if expired {
                    Outcome::Expired
                } else {
                    Outcome::StateChanged
                };
                stale.outcome_digest = Some(sha256_text(if expired {
                    "expired"
                } else {
                    "state-changed"
                }));
                self.transition(&path, RecordState::Stale, &stale)?;
                Err(error)
            }
        }
    }

    pub fn result(&self, request: &str, succeeded: bool, detail: &str) -> Result<()> {
        let _lock = self.lock()?;
        let (state, path, record) = self.find(request)?;
        if state != RecordState::Consumed {
            return Err(OverrideError::new("consumed request is missing or invalid"));
        }
        let next = record.record_result(succeeded, detail)?;
        let mut bytes =
            serde_json::to_vec(&next).map_err(|error| OverrideError::new(error.to_string()))?;
        bytes.push(b'\n');
        atomic_replace(&path, &bytes, 0o600).map_err(|error| OverrideError::new(error.to_string()))
    }

    pub fn audit(&self) -> (Vec<(RecordState, OverrideRecord)>, Vec<PathBuf>) {
        let mut valid = Vec::new();
        let mut invalid = Vec::new();
        if !self.root.is_dir()
            || fs::symlink_metadata(&self.root)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return (valid, invalid);
        }
        let mut identities: BTreeMap<String, usize> = BTreeMap::new();
        for state in STATES {
            let directory = self.root.join(state.as_str());
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            let mut paths = entries
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.path())
                .collect::<Vec<_>>();
            paths.sort();
            for path in paths {
                if path.extension().and_then(|value| value.to_str()) != Some("json")
                    || !Self::secure(&path)
                {
                    invalid.push(path);
                    continue;
                }
                let parsed = fs::read(&path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<OverrideRecord>(&bytes).ok());
                match parsed {
                    Some(record) if record.validate(Some(state)).is_ok() => {
                        *identities.entry(record.request_id.clone()).or_default() += 1;
                        valid.push((state, record));
                    }
                    _ => invalid.push(path),
                }
            }
        }
        let duplicated = identities
            .into_iter()
            .filter_map(|(id, count)| (count > 1).then_some(id))
            .collect::<Vec<_>>();
        if !duplicated.is_empty() {
            valid.retain(|(_, record)| {
                if duplicated.contains(&record.request_id) {
                    invalid.push(
                        self.root
                            .join(format!("duplicate:{}.json", record.request_id)),
                    );
                    false
                } else {
                    true
                }
            });
        }
        valid.sort_by(|left, right| {
            left.1
                .requested_at
                .cmp(&right.1.requested_at)
                .then_with(|| left.1.request_id.cmp(&right.1.request_id))
        });
        (valid, invalid)
    }
}

/// Prove that the current process descends from the harness owning `state/.lock`.
pub fn require_primary_lock(state: &Path) -> Result<()> {
    let lock = state.join(".lock");
    let metadata = fs::symlink_metadata(&lock).map_err(|_| {
        OverrideError::new("grant and denial require the lock-owning primary session")
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(OverrideError::new(
            "grant and denial require the lock-owning primary session",
        ));
    }
    let owner = fs::read_to_string(&lock)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .ok_or_else(|| {
            OverrideError::new("grant and denial require the lock-owning primary session")
        })?;
    let probe = SystemProcessProbe::default();
    let mut pid = std::process::id();
    for _ in 0..8 {
        if pid == owner {
            return Ok(());
        }
        let row = probe.ancestry_row(pid).map_err(|_| {
            OverrideError::new("grant and denial require the lock-owning primary session")
        })?;
        if row.parent_pid <= 1 || row.parent_pid == pid {
            break;
        }
        pid = row.parent_pid;
    }
    Err(OverrideError::new(
        "grant and denial require the lock-owning primary session",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn request<'a>(digest: &'a str) -> Request<'a> {
        Request {
            boundary: "workflow.skip-stage",
            task: "run-1",
            project: "multplx",
            operation: "skip workflow stage build in run run-1",
            target: "run-1#build",
            expected_state_digest: digest,
            consequence: "Skip only the named stage.",
            ttl: 300,
        }
    }

    #[test]
    fn transition_table_refuses_generic_grants_replay_and_result_rewrite() {
        let digest = sha256_text("state-v1");
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let store = OverrideStore::new(&state);
        let id = store.request(&request(&digest)).expect("request");
        let (_, _, pending) = store.find(&id).expect("pending");
        assert!(pending.grant("yes", pending.requested_at + 1).is_err());
        let words = format!(
            "Grant {} for exact operation {} on exact target {}.",
            pending.boundary_id, pending.action_argv_or_operation, pending.target_identity
        );
        let granted = pending
            .grant(&words, pending.requested_at + 1)
            .expect("grant");
        let binding = Binding {
            boundary: "workflow.skip-stage",
            task: "run-1",
            project: "multplx",
            operation: "skip workflow stage build in run run-1",
            target: "run-1#build",
            expected_state_digest: &digest,
        };
        let consumed = granted
            .consume(&binding, pending.requested_at + 2)
            .expect("consume");
        assert!(
            consumed
                .consume(&binding, pending.requested_at + 3)
                .is_err()
        );
        let finished = consumed
            .record_result(true, "skipped exact stage")
            .expect("result");
        assert!(finished.record_result(false, "rewrite").is_err());
    }

    #[test]
    fn changed_binding_becomes_stale_and_private_store_rejects_hardlinks() {
        let digest = sha256_text("state-v1");
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let store = OverrideStore::new(&state);
        let id = store.request(&request(&digest)).expect("request");
        let (_, path, pending) = store.find(&id).expect("pending");
        assert_eq!(
            fs::metadata(store.root())
                .expect("root mode")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("record mode")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::hard_link(&path, temp.path().join("copy.json")).expect("hardlink");
        assert!(store.find(&id).is_err());
        fs::remove_file(temp.path().join("copy.json")).expect("remove hardlink");
        let words = format!(
            "Grant {} for exact operation {} on exact target {}.",
            pending.boundary_id, pending.action_argv_or_operation, pending.target_identity
        );
        let granted = pending
            .grant(&words, pending.requested_at + 1)
            .expect("grant");
        let changed = Binding {
            boundary: "workflow.skip-stage",
            task: "run-1",
            project: "multplx",
            operation: "skip workflow stage other in run run-1",
            target: "run-1#build",
            expected_state_digest: &digest,
        };
        assert!(granted.consume(&changed, pending.requested_at + 2).is_err());
    }

    #[test]
    fn registry_keeps_factual_boundaries_non_consumable() {
        assert_eq!(REGISTRY.len(), 20);
        assert_eq!(
            boundary("integrity.validation-state")
                .expect("integrity")
                .class,
            BoundaryClass::Integrity
        );
        assert_eq!(
            boundary("dependency.install").expect("policy").class,
            BoundaryClass::Policy
        );
    }

    #[test]
    fn record_validation_covers_denial_expiry_and_result_shapes() {
        let digest = sha256_text("state-v1");
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let store = OverrideStore::new(&state);
        let id = store.request(&request(&digest)).expect("request");
        let (_, _, pending) = store.find(&id).expect("pending");

        let denied = pending
            .deny("Deny this exact request.", pending.requested_at + 1)
            .expect("denied");
        assert_eq!(denied.decision, Decision::Denied);
        assert!(denied.validate(Some(RecordState::Denied)).is_ok());
        assert!(pending.deny("", pending.requested_at + 1).is_err());
        assert!(
            pending
                .grant(
                    "Grant workflow.skip-stage for the wrong operation and target.",
                    pending.expires_at,
                )
                .is_err()
        );

        let words = format!(
            "Grant {} for exact operation {} on exact target {}.",
            pending.boundary_id, pending.action_argv_or_operation, pending.target_identity
        );
        let granted = pending
            .grant(&words, pending.requested_at + 1)
            .expect("granted");
        let expired_binding = Binding {
            boundary: "workflow.skip-stage",
            task: "run-1",
            project: "multplx",
            operation: "skip workflow stage build in run run-1",
            target: "run-1#build",
            expected_state_digest: &digest,
        };
        assert!(
            granted
                .consume(&expired_binding, granted.expires_at)
                .is_err()
        );
        let consumed = granted
            .consume(&expired_binding, pending.requested_at + 2)
            .expect("consumed");
        assert!(consumed.record_result(false, "").is_err());
        let failed = consumed
            .record_result(false, "command exited 7")
            .expect("failed result");
        assert_eq!(failed.outcome, Outcome::Failed);

        for corrupt in [
            {
                let mut value = pending.clone();
                value.schema_version = 2;
                value
            },
            {
                let mut value = pending.clone();
                value.boundary_class = "integrity".to_owned();
                value
            },
            {
                let mut value = pending.clone();
                value.action_digest = sha256_text("changed");
                value
            },
            {
                let mut value = pending.clone();
                value.decision = Decision::Consumed;
                value
            },
        ] {
            assert!(corrupt.validate(None).is_err());
        }
        assert!(pending.validate(Some(RecordState::Granted)).is_err());
    }

    #[test]
    fn store_transitions_results_and_audit_are_restart_safe() {
        let digest = sha256_text("state-v1");
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let store = OverrideStore::new(&state);
        assert_eq!(store.audit(), (Vec::new(), Vec::new()));
        assert!(store.find("missing").is_err());
        assert!(store.find("../bad").is_err());

        let id = store.request(&request(&digest)).expect("request");
        let (_, _, pending) = store.find(&id).expect("pending");
        let words = format!(
            "Grant {} for exact operation {} on exact target {}.",
            pending.boundary_id, pending.action_argv_or_operation, pending.target_identity
        );
        store.decide(&id, &words, true).expect("decide");
        assert!(store.decide(&id, &words, true).is_err());
        let binding = Binding {
            boundary: "workflow.skip-stage",
            task: "run-1",
            project: "multplx",
            operation: "skip workflow stage build in run run-1",
            target: "run-1#build",
            expected_state_digest: &digest,
        };
        let consumed = store.consume(&id, &binding).expect("consume");
        assert!(consumed.ends_with(format!("{id}.json")));
        store
            .result(&id, false, "command exited 7")
            .expect("result");
        assert!(store.result(&id, true, "rewrite").is_err());
        let (record_state, _, record) = store.find(&id).expect("finished record");
        assert_eq!(record_state, RecordState::Consumed);
        assert_eq!(record.outcome, Outcome::Failed);

        let denied_id = store.request(&request(&digest)).expect("denial request");
        store
            .decide(&denied_id, "Maintainer denies this request.", false)
            .expect("deny");
        let changed_id = store.request(&request(&digest)).expect("changed request");
        let (_, _, changed_pending) = store.find(&changed_id).expect("pending");
        let changed_words = format!(
            "Grant {} for exact operation {} on exact target {}.",
            changed_pending.boundary_id,
            changed_pending.action_argv_or_operation,
            changed_pending.target_identity
        );
        store
            .decide(&changed_id, &changed_words, true)
            .expect("grant changed");
        let changed = Binding {
            operation: "skip some other stage",
            ..binding
        };
        assert!(store.consume(&changed_id, &changed).is_err());
        assert_eq!(
            store.find(&changed_id).expect("stale").0,
            RecordState::Stale
        );

        let malformed = store.root().join("pending/malformed.json");
        fs::write(&malformed, b"not json\n").expect("malformed");
        fs::set_permissions(&malformed, fs::Permissions::from_mode(0o600)).expect("mode");
        let ignored = store.root().join("pending/ignored.txt");
        fs::write(&ignored, b"ignored\n").expect("ignored");
        let (valid, invalid) = store.audit();
        assert_eq!(valid.len(), 3);
        assert!(invalid.iter().any(|path| path == &malformed));
        assert!(invalid.iter().any(|path| path == &ignored));

        let duplicate = store.root().join("consumed/copy.json");
        fs::copy(store.root().join(format!("consumed/{id}.json")), &duplicate)
            .expect("duplicate record");
        fs::set_permissions(&duplicate, fs::Permissions::from_mode(0o600)).expect("duplicate mode");
        let (valid, invalid) = store.audit();
        assert_eq!(valid.len(), 2);
        assert!(
            invalid
                .iter()
                .any(|path| path.to_string_lossy().contains("duplicate:"))
        );

        let expired_id = store.request(&request(&digest)).expect("expired request");
        let (_, expired_path, mut expired_record) =
            store.find(&expired_id).expect("pending expiry");
        expired_record.requested_at = 1;
        expired_record.expires_at = 2;
        let mut expired_bytes = serde_json::to_vec(&expired_record).expect("expired JSON");
        expired_bytes.push(b'\n');
        fs::write(expired_path, expired_bytes).expect("expired fixture");
        assert!(
            store
                .decide(&expired_id, "Decision arrived too late.", false)
                .is_err()
        );
        assert_eq!(
            store.find(&expired_id).expect("expired record").0,
            RecordState::Stale
        );
    }

    #[test]
    fn request_store_and_primary_lock_refuse_unsafe_shapes() {
        let digest = sha256_text("state-v1");
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let store = OverrideStore::new(&state);
        let mut invalid = request(&digest);
        invalid.boundary = "integrity.validation-state";
        assert!(store.request(&invalid).is_err());
        invalid = request(&digest);
        invalid.task = "../task";
        assert!(store.request(&invalid).is_err());
        invalid = request(&digest);
        invalid.ttl = 0;
        assert!(store.request(&invalid).is_err());
        invalid = request(&digest);
        invalid.target = "two\nlines";
        assert!(store.request(&invalid).is_err());

        assert!(require_primary_lock(&state).is_err());
        fs::write(state.join(".lock"), b"not-a-pid\n").expect("bad lock");
        assert!(require_primary_lock(&state).is_err());
        fs::write(state.join(".lock"), format!("{}\n", std::process::id())).expect("lock");
        assert!(require_primary_lock(&state).is_ok());

        let unsafe_state = temp.path().join("unsafe-state");
        fs::create_dir(&unsafe_state).expect("unsafe state");
        std::os::unix::fs::symlink(temp.path(), unsafe_state.join("maintainer-overrides"))
            .expect("root symlink");
        assert!(
            OverrideStore::new(&unsafe_state)
                .request(&request(&digest))
                .is_err()
        );

        let unsafe_child_state = temp.path().join("unsafe-child-state");
        let unsafe_root = unsafe_child_state.join("maintainer-overrides");
        fs::create_dir_all(&unsafe_root).expect("unsafe root");
        fs::write(unsafe_root.join("pending"), b"not a directory\n").expect("unsafe child");
        assert!(
            OverrideStore::new(&unsafe_child_state)
                .request(&request(&digest))
                .is_err()
        );

        let foreign_state = temp.path().join("foreign-state");
        fs::create_dir(&foreign_state).expect("foreign state");
        fs::write(foreign_state.join(".lock"), b"1\n").expect("foreign lock");
        assert!(require_primary_lock(&foreign_state).is_err());
    }
}

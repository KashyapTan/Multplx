//! Revision-bound implementation and publication evidence.
//!
//! The canonical task `.meta` record owns this history. A new commit, attempt,
//! or accepted brief makes prior evidence historical without deleting it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use multplx_core::filesystem::{TransitionWrite, recoverable_transition};
use multplx_core::identifiers::TaskId;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::subagent_model::{Acknowledgement, MessageEnvelope, TaskRecord};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum CheckOutcome {
    Passed,
    Failed,
    NotRun,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryCheck {
    pub name: String,
    pub outcome: CheckOutcome,
    pub summary: String,
    pub artifact: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewOutcome {
    Passed,
    Findings,
    NotRun,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryReview {
    pub outcome: ReviewOutcome,
    pub summary: String,
    pub artifact: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum DeliveryOutcome {
    EvidenceUpdated,
    Published,
    PublicationFailed,
    HumanMerged,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryEvidence {
    pub evidence_id: String,
    pub attempt_id: String,
    pub attempt_generation: u64,
    pub brief_revision: u64,
    pub commit: String,
    pub accepted_scope: String,
    pub checks: Vec<DeliveryCheck>,
    pub review: Option<DeliveryReview>,
    pub limitations: Vec<String>,
    pub pr_url: Option<String>,
    pub outcome: DeliveryOutcome,
    pub observed_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryFacts {
    pub current_commit: Option<String>,
    pub history: Vec<DeliveryEvidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequest {
    pub evidence_id: String,
    pub attempt_id: String,
    pub attempt_generation: u64,
    pub brief_revision: u64,
    pub commit: String,
    pub checks: Vec<DeliveryCheck>,
    pub review: Option<DeliveryReview>,
    pub limitations: Vec<String>,
    pub pr_url: Option<String>,
    pub outcome: DeliveryOutcome,
    pub observed_at: String,
    /// Current implementation/publication evidence advances this pointer.
    /// Read-only forge observations use false and must match retained history.
    pub mark_current: bool,
    /// Optimistic concurrency token. `None` means no current delivery revision.
    pub expected_current_commit: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum HumanReviewState {
    Ready,
    NeedsChecks,
    BlockedByDependencies,
    StaleRevision,
    FreshnessUnknown,
    PublicationFailed,
    ReviewFindings,
    ReviewNotRun,
    Merged,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum RevisionFreshness {
    Current,
    Stale,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HumanReviewEntry {
    pub task_key: String,
    pub task_id: String,
    pub owner_home: Option<String>,
    pub project_id: Option<String>,
    pub priority: i32,
    pub commit: String,
    pub pr_url: String,
    pub outcome: DeliveryOutcome,
    pub state: HumanReviewState,
    pub revision_freshness: RevisionFreshness,
    pub checks_passing: bool,
    pub review_complete: bool,
    pub review_clear: bool,
    pub pr_ready: bool,
    pub checks: Vec<DeliveryCheck>,
    pub review: Option<DeliveryReview>,
    pub limitations: Vec<String>,
    pub dependencies: Vec<String>,
    pub blocked_by: Vec<String>,
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains(['\r', '\n'])
}

fn valid_scope(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 100_000
        && value
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\n' | '\t'))
}

fn valid_commit(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_time(value: &str) -> bool {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).is_ok()
}

fn check_valid(check: &DeliveryCheck) -> bool {
    valid_text(&check.name, 200)
        && valid_text(&check.summary, 20_000)
        && check
            .artifact
            .as_deref()
            .is_none_or(|artifact| valid_text(artifact, 4096))
}

fn review_valid(review: &DeliveryReview) -> bool {
    valid_text(&review.summary, 20_000)
        && review
            .artifact
            .as_deref()
            .is_none_or(|artifact| valid_text(artifact, 4096))
}

fn attempt_known(task: &TaskRecord, evidence: &DeliveryEvidence) -> bool {
    let brief_known = task
        .briefs
        .iter()
        .any(|brief| brief.revision == evidence.brief_revision);
    brief_known
        && (task.attempt.as_ref().is_some_and(|attempt| {
            attempt.id == evidence.attempt_id && attempt.generation == evidence.attempt_generation
        }) || task.prior_attempts.iter().any(|attempt| {
            attempt.id == evidence.attempt_id && attempt.generation == evidence.attempt_generation
        }))
}

impl DeliveryFacts {
    pub fn validate(&self, task: &TaskRecord) -> Result<(), String> {
        if self
            .current_commit
            .as_deref()
            .is_some_and(|commit| !valid_commit(commit))
        {
            return Err("invalid current delivery commit".into());
        }
        let mut ids = BTreeSet::new();
        for evidence in &self.history {
            if multplx_core::identifiers::TaskId::parse(&evidence.evidence_id).is_err()
                || !ids.insert(&evidence.evidence_id)
                || !attempt_known(task, evidence)
                || evidence.attempt_generation == 0
                || evidence.brief_revision == 0
                || !valid_commit(&evidence.commit)
                || !valid_scope(&evidence.accepted_scope)
                || evidence.checks.iter().any(|check| !check_valid(check))
                || evidence
                    .review
                    .as_ref()
                    .is_some_and(|review| !review_valid(review))
                || evidence
                    .limitations
                    .iter()
                    .any(|limitation| !valid_text(limitation, 20_000))
                || evidence
                    .pr_url
                    .as_deref()
                    .is_some_and(|url| crate::review_delivery::PrIdentity::parse(url).is_err())
                || matches!(
                    evidence.outcome,
                    DeliveryOutcome::Published | DeliveryOutcome::HumanMerged
                ) && evidence.pr_url.is_none()
                || !valid_time(&evidence.observed_at)
            {
                return Err("invalid revision-bound delivery evidence".into());
            }
        }
        Ok(())
    }
}

impl TaskRecord {
    /// Fold retained facts for one exact attempt, accepted brief, and commit.
    /// This is also used for a forge observation that belongs to a historical
    /// revision after the task has moved on.
    #[must_use]
    pub fn delivery_evidence_for_revision(
        &self,
        attempt_id: &str,
        attempt_generation: u64,
        brief_revision: u64,
        commit: &str,
    ) -> Option<DeliveryEvidence> {
        let matching = self
            .delivery
            .history
            .iter()
            .filter(|evidence| {
                evidence.commit == commit
                    && evidence.attempt_id == attempt_id
                    && evidence.attempt_generation == attempt_generation
                    && evidence.brief_revision == brief_revision
            })
            .collect::<Vec<_>>();
        let mut current = (*matching.last()?).clone();
        let mut publication = None;
        let mut pr_url = None;
        let mut review = None;
        for evidence in matching {
            if evidence.outcome != DeliveryOutcome::EvidenceUpdated {
                publication = Some(evidence.outcome);
            }
            if evidence.pr_url.is_some() {
                pr_url = evidence.pr_url.clone();
            }
            if evidence.review.is_some() {
                review = evidence.review.clone();
            }
        }
        current.outcome = publication.unwrap_or(DeliveryOutcome::EvidenceUpdated);
        current.pr_url = pr_url;
        current.review = review;
        Some(current)
    }

    /// The latest fact for the current commit, attempt, and accepted brief.
    #[must_use]
    pub fn current_delivery_evidence(&self) -> Option<DeliveryEvidence> {
        let commit = self.delivery.current_commit.as_deref()?;
        let attempt = self.attempt.as_ref()?;
        self.delivery_evidence_for_revision(
            &attempt.id,
            attempt.generation,
            self.accepted_brief_revision?,
            commit,
        )
    }

    pub fn apply_delivery_evidence(
        &mut self,
        request: &EvidenceRequest,
    ) -> Result<DeliveryEvidence, String> {
        if let Some(existing) = self
            .delivery
            .history
            .iter()
            .find(|evidence| evidence.evidence_id == request.evidence_id)
        {
            let scope = self
                .briefs
                .iter()
                .find(|brief| brief.revision == request.brief_revision)
                .map(|brief| brief.scope.clone())
                .ok_or("delivery evidence brief is unknown")?;
            let requested = DeliveryEvidence {
                evidence_id: request.evidence_id.clone(),
                attempt_id: request.attempt_id.clone(),
                attempt_generation: request.attempt_generation,
                brief_revision: request.brief_revision,
                commit: request.commit.clone(),
                accepted_scope: scope,
                checks: request.checks.clone(),
                review: request.review.clone(),
                limitations: request.limitations.clone(),
                pr_url: request.pr_url.clone(),
                outcome: request.outcome,
                observed_at: request.observed_at.clone(),
            };
            return if existing == &requested {
                Ok(existing.clone())
            } else {
                Err("delivery evidence identity reused with changed facts".into())
            };
        }
        if self.legacy_unknown {
            return Err("legacy task requires migration before delivery evidence".into());
        }
        let current_attempt = self.attempt.as_ref().ok_or("task attempt missing")?;
        let exact_current = current_attempt.id == request.attempt_id
            && current_attempt.generation == request.attempt_generation
            && self.accepted_brief_revision == Some(request.brief_revision);
        if request.mark_current {
            if !exact_current {
                return Err("stale attempt or accepted brief delivery evidence".into());
            }
            if self.delivery.current_commit != request.expected_current_commit {
                return Err("current delivery commit changed".into());
            }
        } else if !self.delivery.history.iter().any(|evidence| {
            evidence.attempt_id == request.attempt_id
                && evidence.attempt_generation == request.attempt_generation
                && evidence.brief_revision == request.brief_revision
                && evidence.commit == request.commit
                && (request.pr_url.is_none() || evidence.pr_url == request.pr_url)
        }) {
            return Err("delivery observation has no retained revision evidence".into());
        }
        let accepted_scope = self
            .briefs
            .iter()
            .find(|brief| brief.revision == request.brief_revision)
            .map(|brief| brief.scope.clone())
            .ok_or("delivery evidence brief is unknown")?;
        let commit_changed = request.mark_current
            && self.delivery.current_commit.as_deref() != Some(request.commit.as_str());
        let evidence = DeliveryEvidence {
            evidence_id: request.evidence_id.clone(),
            attempt_id: request.attempt_id.clone(),
            attempt_generation: request.attempt_generation,
            brief_revision: request.brief_revision,
            commit: request.commit.clone(),
            accepted_scope,
            checks: request.checks.clone(),
            review: request.review.clone(),
            limitations: request.limitations.clone(),
            pr_url: request.pr_url.clone(),
            outcome: request.outcome,
            observed_at: request.observed_at.clone(),
        };
        let mut projected = self.delivery.clone();
        if request.mark_current {
            projected.current_commit = Some(request.commit.clone());
        }
        projected.history.push(evidence.clone());
        projected.validate(self)?;
        self.delivery = projected;
        if commit_changed && self.schedule.state == super::subagent_model::WorkState::Completed {
            self.schedule.state = super::subagent_model::WorkState::Runnable;
            self.schedule.waiting_condition = None;
        }
        Ok(evidence)
    }
}

/// Persist evidence under the same task identity lock used by revisions.
pub fn record(
    state: &Path,
    task_id: &str,
    request: &EvidenceRequest,
) -> Result<(TaskRecord, DeliveryEvidence), String> {
    record_with_fault(state, task_id, request, None)
}

fn record_with_fault(
    state: &Path,
    task_id: &str,
    request: &EvidenceRequest,
    fault: Option<multplx_core::filesystem::TransitionFault>,
) -> Result<(TaskRecord, DeliveryEvidence), String> {
    TaskId::parse(task_id).map_err(|error| error.to_string())?;
    let _lock = DirectoryLock::acquire_wait(
        state.join(format!(".{task_id}.identity.lock")),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    super::subagent_model::require_writer_version(state)?;
    let path = state.join(format!("{task_id}.meta"));
    let before = multplx_core::filesystem::read_bounded_regular(&path, 4 * 1024 * 1024)
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8(before.clone()).map_err(|error| error.to_string())?;
    let mut task = super::subagent_model::read_meta(task_id, &text)?;
    let key = evidence_key(task_id, &request.evidence_id);
    let operation = format!("delivery-evidence-{key}");
    let intent = state.join(".transitions").join(format!("{operation}.json"));
    if let Some(existing) = task
        .delivery
        .history
        .iter()
        .find(|evidence| evidence.evidence_id == request.evidence_id)
        .cloned()
    {
        task.apply_delivery_evidence(request)?;
        if intent.exists() {
            multplx_core::filesystem::recover_transition(state, &operation)
                .map_err(|error| error.to_string())?;
        }
        if request.mark_current {
            persist_frozen_outcome(state, task_id, &request.evidence_id)?;
        }
        return Ok((task, existing));
    }
    if intent.exists() {
        multplx_core::filesystem::recover_transition(state, &operation)
            .map_err(|error| error.to_string())?;
        let bytes = multplx_core::filesystem::read_bounded_regular(&path, 4 * 1024 * 1024)
            .map_err(|error| error.to_string())?;
        let recovered_text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
        let recovered = super::subagent_model::read_meta(task_id, &recovered_text)?;
        let evidence = recovered
            .delivery
            .history
            .iter()
            .find(|evidence| evidence.evidence_id == request.evidence_id)
            .cloned()
            .ok_or("recovered delivery intent has no canonical evidence")?;
        let mut verified = recovered.clone();
        verified.apply_delivery_evidence(request)?;
        if request.mark_current {
            persist_frozen_outcome(state, task_id, &request.evidence_id)?;
        }
        return Ok((recovered, evidence));
    }
    let evidence = task.apply_delivery_evidence(request)?;
    let after = super::subagent_model::write_meta(&text, &task)?.into_bytes();
    if before != after {
        if request.mark_current {
            let event = outcome_envelope(&task, &evidence)?;
            // Freeze routing before the metadata commit. The envelope is already bound
            // to the current attempt/brief, and its immutable route survives a later
            // coordinator replacement or task revision.
            let outcome = super::parent_channel::prepare_outcome(state, &event)?;
            std::fs::create_dir_all(state.join("delivery-outcomes"))
                .map_err(|error| error.to_string())?;
            recoverable_transition(
                state,
                &operation,
                &[
                    TransitionWrite {
                        path: PathBuf::from("delivery-outcomes").join(format!("{key}.json")),
                        before: None,
                        after: serde_json::to_vec_pretty(&outcome)
                            .map_err(|error| error.to_string())?,
                    },
                    TransitionWrite {
                        path: PathBuf::from(format!("{task_id}.meta")),
                        before: Some(before),
                        after,
                    },
                ],
                fault,
            )
            .map_err(|error| error.to_string())?;
        } else {
            // A late forge observation for a superseded revision is retained as
            // history, but cannot emit a current-revision A11 outcome.
            recoverable_transition(
                state,
                &operation,
                &[TransitionWrite {
                    path: PathBuf::from(format!("{task_id}.meta")),
                    before: Some(before),
                    after,
                }],
                fault,
            )
            .map_err(|error| error.to_string())?;
        }
    }
    if request.mark_current {
        persist_frozen_outcome(state, task_id, &request.evidence_id)?;
    }
    Ok((task, evidence))
}

fn evidence_key(task_id: &str, evidence_id: &str) -> String {
    let digest = Sha256::digest(format!("{task_id}\n{evidence_id}").as_bytes());
    format!("{digest:x}")
}

fn persist_frozen_outcome(state: &Path, task_id: &str, evidence_id: &str) -> Result<(), String> {
    let path = state
        .join("delivery-outcomes")
        .join(format!("{}.json", evidence_key(task_id, evidence_id)));
    let bytes = multplx_core::filesystem::read_bounded_regular(path, 1024 * 1024)
        .map_err(|error| format!("delivery outcome intent is unavailable: {error}"))?;
    let outcome: super::parent_channel::ParentOutcome =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    super::parent_channel::persist_prepared(state, &outcome)
}

/// Build the exact current-revision A11 event after canonical evidence commit.
pub fn outcome_envelope(
    task: &TaskRecord,
    evidence: &DeliveryEvidence,
) -> Result<MessageEnvelope, String> {
    let current = task
        .current_delivery_evidence()
        .filter(|current| {
            current.commit == evidence.commit
                && current.attempt_id == evidence.attempt_id
                && current.attempt_generation == evidence.attempt_generation
                && current.brief_revision == evidence.brief_revision
        })
        .ok_or("historical delivery evidence cannot become a current parent outcome")?;
    let kind = match evidence.outcome {
        DeliveryOutcome::Published => "evidence",
        DeliveryOutcome::PublicationFailed => "publication-failed",
        DeliveryOutcome::EvidenceUpdated => "evidence-changed",
        DeliveryOutcome::HumanMerged => "human-merge",
    };
    let recipient = task
        .parent_id
        .clone()
        .ok_or("delivery task parent missing")?;
    Ok(MessageEnvelope {
        schema_version: super::subagent_model::SCHEMA_VERSION,
        message_id: format!(
            "delivery-{}",
            &evidence_key(&task.task_id, &evidence.evidence_id)[..32]
        ),
        task_id: task.task_id.clone(),
        task_home: task.owner_home.clone(),
        parent_home: task.parent_home.clone(),
        attempt: task.attempt.clone(),
        parent_id: Some(recipient.clone()),
        sender: task.task_id.clone(),
        recipient,
        brief_revision: task.accepted_brief_revision,
        kind: kind.into(),
        correlation_id: format!("delivery-{}", current.commit),
        created_at: evidence.observed_at.clone(),
        summary: match &evidence.pr_url {
            Some(url) => format!("{kind}: {} at {} ({url})", task.task_id, current.commit),
            None => format!("{kind}: {} at {}", task.task_id, current.commit),
        },
        artifact: task
            .owner_state
            .as_ref()
            .map(|state| Path::new(state).join(format!("{}.meta", task.task_id)))
            .map(|path| path.to_string_lossy().into_owned()),
        acknowledgement: Acknowledgement::Pending,
    })
}

fn qualified(task: &TaskRecord) -> String {
    super::subagent_model::qualified_task_id(
        task.owner_home.as_deref().unwrap_or("legacy-unknown"),
        &task.task_id,
    )
}

fn revision_freshness(task: &TaskRecord, commit: &str) -> RevisionFreshness {
    let Some(allocation) = task.allocation.as_ref() else {
        return RevisionFreshness::Unknown;
    };
    let output = super::worktree::command_output(std::process::Command::new("git").args([
        "-C",
        &allocation.path,
        "rev-parse",
        "HEAD",
    ]));
    match output {
        Ok(output) if output.status.success() => {
            if String::from_utf8_lossy(&output.stdout).trim() == commit {
                RevisionFreshness::Current
            } else {
                RevisionFreshness::Stale
            }
        }
        _ => RevisionFreshness::Unknown,
    }
}

/// Add delivery-producing descendant records referenced by durable parent
/// outcomes. The frozen route proves the sender state; the current task owner
/// still supplies the mutable evidence record.
pub fn extend_review_records_from_outcomes(
    state: &Path,
    records: &mut Vec<TaskRecord>,
) -> Result<(), String> {
    let mut seen = records.iter().map(qualified).collect::<BTreeSet<_>>();
    let mut observed = 0usize;
    for directory in ["parent-inbox", "parent-outbox", "parent-receipts"] {
        let directory = state.join(directory);
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        for entry in entries {
            if observed >= 4096 {
                return Err("human review queue parent evidence exceeds bounded scan".into());
            }
            observed += 1;
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let bytes = multplx_core::filesystem::read_bounded_regular(&path, 1024 * 1024)
                .map_err(|error| error.to_string())?;
            let outcome: super::parent_channel::ParentOutcome = match serde_json::from_slice(&bytes)
            {
                Ok(outcome) => outcome,
                Err(_) => continue,
            };
            if outcome.event.task_id != outcome.route.sender_id
                || !matches!(
                    outcome.event.kind.as_str(),
                    "evidence" | "publication-failed" | "evidence-changed" | "human-merge"
                )
            {
                continue;
            }
            let expected = Path::new(&outcome.route.sender_state)
                .join(format!("{}.meta", outcome.route.sender_id));
            let Some(artifact) = outcome.event.artifact.as_deref() else {
                continue;
            };
            let artifact = std::fs::canonicalize(artifact)
                .map_err(|_| "delivery outcome artifact is unavailable")?;
            let expected = std::fs::canonicalize(&expected)
                .map_err(|_| "delivery outcome sender task is unavailable")?;
            if artifact != expected {
                return Err("delivery outcome artifact conflicts with frozen sender route".into());
            }
            let bytes = multplx_core::filesystem::read_bounded_regular(&expected, 4 * 1024 * 1024)
                .map_err(|error| error.to_string())?;
            let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
            let record = super::subagent_model::read_meta(&outcome.route.sender_id, &text)?;
            if record.owner_home.as_deref() != Some(outcome.route.sender_home.as_str()) {
                return Err("delivery outcome owner conflicts with current task record".into());
            }
            if seen.insert(qualified(&record)) {
                records.push(record);
            }
        }
    }
    Ok(())
}

/// Return every current PR outcome in dependency order. Optional review is not
/// a gate; checks and blockers are exposed as independent facts.
pub fn human_review_queue(records: &[TaskRecord]) -> Result<Vec<HumanReviewEntry>, String> {
    let by_id = records
        .iter()
        .map(|task| (qualified(task), task))
        .collect::<BTreeMap<_, _>>();
    let resolve = |task: &TaskRecord, dependency: &str| {
        if dependency.contains("#task:") {
            dependency.to_owned()
        } else {
            super::subagent_model::qualified_task_id(
                task.owner_home.as_deref().unwrap_or("legacy-unknown"),
                dependency,
            )
        }
    };
    let dependency_satisfied = |task: &&TaskRecord| match task.current_delivery_evidence() {
        Some(evidence) if evidence.pr_url.is_some() => {
            evidence.outcome == DeliveryOutcome::HumanMerged
        }
        Some(_) => task.schedule.state == super::subagent_model::WorkState::Completed,
        None => {
            task.schedule.state == super::subagent_model::WorkState::Completed
                && !task
                    .delivery
                    .history
                    .iter()
                    .any(|evidence| evidence.pr_url.is_some())
        }
    };
    let mut candidates = records
        .iter()
        .filter_map(|task| {
            let evidence = task.current_delivery_evidence()?;
            let pr_url = evidence.pr_url.clone()?;
            let blocked_by = task
                .schedule
                .dependencies
                .iter()
                .filter(|dependency| {
                    by_id
                        .get(&resolve(task, dependency))
                        .is_none_or(|dependency| !dependency_satisfied(dependency))
                })
                .cloned()
                .collect::<Vec<_>>();
            let checks_pass = !evidence.checks.is_empty()
                && evidence
                    .checks
                    .iter()
                    .all(|check| check.outcome == CheckOutcome::Passed);
            let freshness = revision_freshness(task, &evidence.commit);
            let review_complete = evidence.review.as_ref().is_some_and(|review| {
                matches!(
                    review.outcome,
                    ReviewOutcome::Passed | ReviewOutcome::Findings
                )
            });
            let review_clear = evidence
                .review
                .as_ref()
                .is_none_or(|review| review.outcome == ReviewOutcome::Passed);
            let pr_ready = evidence.outcome == DeliveryOutcome::Published
                && checks_pass
                && blocked_by.is_empty()
                && freshness == RevisionFreshness::Current
                && review_clear;
            let state = if freshness == RevisionFreshness::Stale {
                HumanReviewState::StaleRevision
            } else if evidence.outcome == DeliveryOutcome::HumanMerged {
                HumanReviewState::Merged
            } else if evidence.outcome == DeliveryOutcome::PublicationFailed {
                HumanReviewState::PublicationFailed
            } else if !blocked_by.is_empty() {
                HumanReviewState::BlockedByDependencies
            } else if evidence
                .review
                .as_ref()
                .is_some_and(|review| review.outcome == ReviewOutcome::Findings)
            {
                HumanReviewState::ReviewFindings
            } else if evidence
                .review
                .as_ref()
                .is_some_and(|review| review.outcome == ReviewOutcome::NotRun)
            {
                HumanReviewState::ReviewNotRun
            } else if checks_pass {
                if freshness == RevisionFreshness::Current {
                    HumanReviewState::Ready
                } else {
                    HumanReviewState::FreshnessUnknown
                }
            } else {
                HumanReviewState::NeedsChecks
            };
            Some((
                qualified(task),
                HumanReviewEntry {
                    task_key: qualified(task),
                    task_id: task.task_id.clone(),
                    owner_home: task.owner_home.clone(),
                    project_id: task
                        .project
                        .as_ref()
                        .map(|project| project.project_id.clone()),
                    priority: task.schedule.priority,
                    commit: evidence.commit.clone(),
                    pr_url,
                    outcome: evidence.outcome,
                    state,
                    revision_freshness: freshness,
                    checks_passing: checks_pass,
                    review_complete,
                    review_clear,
                    pr_ready,
                    checks: evidence.checks.clone(),
                    review: evidence.review.clone(),
                    limitations: evidence.limitations.clone(),
                    dependencies: task.schedule.dependencies.clone(),
                    blocked_by,
                },
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut queue = Vec::with_capacity(candidates.len());
    let mut emitted = BTreeSet::new();
    while !candidates.is_empty() {
        let mut ready = candidates
            .iter()
            .filter(|(id, _)| {
                let task = by_id.get(*id).expect("candidate task");
                task.schedule.dependencies.iter().all(|dependency| {
                    let dependency = resolve(task, dependency);
                    !candidates.contains_key(&dependency) || emitted.contains(&dependency)
                })
            })
            .map(|(id, entry)| (id.clone(), entry.priority))
            .collect::<Vec<_>>();
        if ready.is_empty() {
            return Err("cyclic delivery review dependencies".into());
        }
        ready.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let id = ready[0].0.clone();
        emitted.insert(id.clone());
        queue.push(candidates.remove(&id).expect("selected candidate"));
    }
    Ok(queue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::subagent_model::{ArtifactKind, AssignmentRole};

    fn task(id: &str) -> TaskRecord {
        let home = "/tmp/mx-delivery-evidence".to_owned();
        let mut task = TaskRecord::new(
            id.into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            format!("root-home:{home}"),
            format!("root-home:{home}"),
            home,
        );
        task.briefs[0].scope = format!("implement {id}");
        task
    }

    fn request(task: &TaskRecord, id: &str, commit: char) -> EvidenceRequest {
        let attempt = task.attempt.as_ref().unwrap();
        EvidenceRequest {
            evidence_id: id.into(),
            attempt_id: attempt.id.clone(),
            attempt_generation: attempt.generation,
            brief_revision: attempt.brief_revision,
            commit: commit.to_string().repeat(40),
            checks: vec![DeliveryCheck {
                name: "cargo test".into(),
                outcome: CheckOutcome::Passed,
                summary: "passed".into(),
                artifact: None,
            }],
            review: None,
            limitations: vec![],
            pr_url: Some("https://github.com/example/project/pull/1".into()),
            outcome: DeliveryOutcome::Published,
            observed_at: "2026-09-15T12:00:00Z".into(),
            mark_current: true,
            expected_current_commit: task.delivery.current_commit.clone(),
        }
    }

    #[test]
    fn new_revision_preserves_history_but_invalidates_readiness() {
        let mut task = task("worker");
        task.apply_delivery_evidence(&request(&task, "first", 'a'))
            .unwrap();
        assert!(task.current_delivery_evidence().is_some());
        task.revise_assignment(
            1,
            AssignmentRole::Implementer,
            "changed scope".into(),
            vec![],
            vec![],
            "human correction".into(),
        )
        .unwrap();
        assert_eq!(task.delivery.history.len(), 1);
        assert!(task.current_delivery_evidence().is_none());
    }

    #[test]
    fn multiline_accepted_scope_is_preserved_as_exact_bounded_evidence() {
        let mut task = task("worker");
        task.briefs[0].scope =
            "Implement publication recovery.\n\n- Preserve receipts\n- Keep human merges\n\twith exact revision identity"
                .into();
        let evidence = task
            .apply_delivery_evidence(&request(&task, "multiline", 'a'))
            .unwrap();
        assert_eq!(evidence.accepted_scope, task.briefs[0].scope);

        let mut invalid = task.clone();
        invalid.delivery.history[0].accepted_scope = "unsafe\rshape".into();
        assert!(invalid.delivery.validate(&invalid).is_err());
        invalid.delivery.history[0].accepted_scope = "\0unsafe".into();
        assert!(invalid.delivery.validate(&invalid).is_err());
    }

    #[test]
    fn stale_and_conflicting_evidence_cannot_become_current() {
        let mut task = task("worker");
        let first = request(&task, "first", 'a');
        task.apply_delivery_evidence(&first).unwrap();
        assert_eq!(
            task.apply_delivery_evidence(&first).unwrap().evidence_id,
            "first"
        );
        let mut conflict = request(&task, "second", 'b');
        conflict.expected_current_commit = None;
        assert_eq!(
            task.apply_delivery_evidence(&conflict).unwrap_err(),
            "current delivery commit changed"
        );
    }

    #[test]
    fn review_queue_orders_dependencies_and_reports_current_outcomes() {
        let mut base = task("base");
        base.schedule.priority = 1;
        base.apply_delivery_evidence(&request(&base, "base-published", 'a'))
            .unwrap();
        let mut dependent = task("dependent");
        dependent.schedule.priority = 100;
        dependent.schedule.dependencies.push("base".into());
        dependent
            .apply_delivery_evidence(&request(&dependent, "dependent-published", 'b'))
            .unwrap();
        let queue = human_review_queue(&[dependent, base]).unwrap();
        assert_eq!(queue[0].task_id, "base");
        assert_eq!(queue[1].task_id, "dependent");
        assert_eq!(queue[1].state, HumanReviewState::BlockedByDependencies);
        assert_eq!(queue[1].blocked_by, ["base"]);
    }

    #[test]
    fn new_commit_keeps_prior_review_historical() {
        let mut task = task("worker");
        let mut reviewed = request(&task, "reviewed", 'a');
        reviewed.review = Some(DeliveryReview {
            outcome: ReviewOutcome::Passed,
            summary: "independent review passed".into(),
            artifact: Some("artifacts/review.json".into()),
        });
        task.apply_delivery_evidence(&reviewed).unwrap();
        let mut changed = request(&task, "changed", 'b');
        changed.expected_current_commit = Some("a".repeat(40));
        changed.review = None;
        task.apply_delivery_evidence(&changed).unwrap();
        assert_eq!(task.delivery.history.len(), 2);
        assert_eq!(
            task.delivery.history[0].review.as_ref().unwrap().outcome,
            ReviewOutcome::Passed
        );
        assert!(task.current_delivery_evidence().unwrap().review.is_none());
        let queue = human_review_queue(&[task]).unwrap();
        assert!(!queue[0].review_complete);
        assert!(!queue[0].pr_ready);
        assert_eq!(queue[0].revision_freshness, RevisionFreshness::Unknown);
    }

    #[test]
    fn evidence_validation_rejects_fabricated_or_malformed_facts() {
        let task = task("worker");
        let mut cases = Vec::new();
        let mut bad_id = request(&task, "../escape", 'a');
        cases.push(bad_id.clone());
        bad_id.evidence_id = "ok".into();
        bad_id.commit = "not-a-commit".into();
        cases.push(bad_id.clone());
        bad_id.commit = "a".repeat(40);
        bad_id.observed_at = "yesterday".into();
        cases.push(bad_id.clone());
        bad_id.observed_at = "2026-09-15T12:00:00Z".into();
        bad_id.pr_url = Some("https://example.invalid/pull/1".into());
        cases.push(bad_id.clone());
        bad_id.pr_url = None;
        cases.push(bad_id);
        for invalid in cases {
            let mut candidate = task.clone();
            assert!(candidate.apply_delivery_evidence(&invalid).is_err());
        }

        let mut stale = request(&task, "stale", 'a');
        stale.attempt_generation += 1;
        let mut candidate = task.clone();
        assert_eq!(
            candidate.apply_delivery_evidence(&stale).unwrap_err(),
            "stale attempt or accepted brief delivery evidence"
        );
        let mut historical = request(&task, "historical", 'a');
        historical.mark_current = false;
        let mut candidate = task;
        assert_eq!(
            candidate.apply_delivery_evidence(&historical).unwrap_err(),
            "delivery observation has no retained revision evidence"
        );
    }

    #[test]
    fn queue_detects_worktree_head_newer_than_cached_evidence() {
        use crate::lifecycle::subagent_model::AllocationBinding;
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        git(&["init", "-b", "main", "--quiet"]);
        std::fs::write(repo.join("file"), "one").unwrap();
        git(&["add", "file"]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "one",
            "--quiet",
        ]);
        let first = git(&["rev-parse", "HEAD"]);
        let mut task = task("worker");
        let mut evidence = request(&task, "published", 'a');
        evidence.commit = first.clone();
        task.apply_delivery_evidence(&evidence).unwrap();
        task.allocation = Some(AllocationBinding {
            allocation_id: "a".repeat(64),
            lease_id: "lease".into(),
            generation: 1,
            project_id: "project".into(),
            checkout_id: "checkout".into(),
            common_git_identity: "git".into(),
            path: repo.to_string_lossy().into_owned(),
            base_revision: first.clone(),
            task_id: "worker".into(),
            attempt_id: task.attempt.as_ref().unwrap().id.clone(),
            persistent: false,
        });
        assert_eq!(
            human_review_queue(&[task.clone()]).unwrap()[0].revision_freshness,
            RevisionFreshness::Current
        );
        std::fs::write(repo.join("file"), "two").unwrap();
        git(&["add", "file"]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "two",
            "--quiet",
        ]);
        let second = git(&["rev-parse", "HEAD"]);
        let entry = human_review_queue(&[task.clone()]).unwrap().remove(0);
        assert_eq!(entry.revision_freshness, RevisionFreshness::Stale);
        assert_eq!(entry.state, HumanReviewState::StaleRevision);
        assert!(!entry.pr_ready);
        task.schedule.state = super::super::subagent_model::WorkState::Completed;
        let mut updated = request(&task, "updated", 'b');
        updated.commit = second;
        updated.expected_current_commit = Some(first);
        task.apply_delivery_evidence(&updated).unwrap();
        assert_eq!(
            task.schedule.state,
            super::super::subagent_model::WorkState::Runnable
        );
    }

    #[test]
    fn completed_remote_free_dependency_does_not_block_review() {
        let mut local = task("local");
        local.schedule.state = super::super::subagent_model::WorkState::Completed;
        let mut dependent = task("dependent");
        dependent.schedule.dependencies.push("local".into());
        dependent
            .apply_delivery_evidence(&request(&dependent, "published", 'd'))
            .unwrap();
        let entry = human_review_queue(&[dependent, local]).unwrap().remove(0);
        assert!(entry.blocked_by.is_empty());

        let mut missing = task("missing-dependent");
        missing.schedule.dependencies.push("unknown".into());
        missing
            .apply_delivery_evidence(&request(&missing, "missing-published", 'e'))
            .unwrap();
        let entry = human_review_queue(&[missing]).unwrap().remove(0);
        assert_eq!(entry.blocked_by, ["unknown"]);

        let mut stale_pr = task("stale-pr");
        stale_pr
            .apply_delivery_evidence(&request(&stale_pr, "old-pr", 'f'))
            .unwrap();
        stale_pr.schedule.state = super::super::subagent_model::WorkState::Completed;
        stale_pr
            .revise_assignment(
                1,
                AssignmentRole::Implementer,
                "new scope".into(),
                vec![],
                vec![],
                "scope changed".into(),
            )
            .unwrap();
        let mut after_stale = task("after-stale");
        after_stale.schedule.dependencies.push("stale-pr".into());
        after_stale
            .apply_delivery_evidence(&request(&after_stale, "after-stale-pr", '1'))
            .unwrap();
        let entry = human_review_queue(&[after_stale, stale_pr])
            .unwrap()
            .remove(0);
        assert_eq!(entry.blocked_by, ["stale-pr"]);
    }

    #[test]
    fn concurrent_distinct_forges_and_remote_free_borrowed_checkout_keep_separate_outcomes() {
        use crate::project_registry::{
            CheckoutOwnership, register_project, validate_publication_location,
        };
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();
        let one_home = temp.path().join("one-home");
        let two_home = temp.path().join("two-home");
        let local_home = temp.path().join("local-home");
        let one_state = one_home.join("state");
        let two_state = two_home.join("state");
        let local_state = local_home.join("state");
        for state in [&one_state, &two_state, &local_state] {
            std::fs::create_dir_all(state).unwrap();
        }
        let make_repo = |name: &str, remote: Option<&str>| {
            let path = temp.path().join(name);
            std::fs::create_dir(&path).unwrap();
            let git = |args: &[&str]| {
                let output = Command::new("git")
                    .arg("-C")
                    .arg(&path)
                    .args(args)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                String::from_utf8_lossy(&output.stdout).trim().to_owned()
            };
            git(&["init", "-b", "main", "--quiet"]);
            std::fs::write(path.join("tracked"), name).unwrap();
            git(&["add", "tracked"]);
            git(&[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "baseline",
                "--quiet",
            ]);
            if let Some(remote) = remote {
                git(&["remote", "add", "origin", remote]);
            }
            path
        };
        let repo_one = make_repo("repo-one", Some("https://github.com/one/project.git"));
        let repo_two = make_repo("repo-two", Some("git@github.com:two/project.git"));
        let local = make_repo("local-only", None);
        std::fs::write(repo_one.join("borrowed-dirty"), "keep one").unwrap();
        std::fs::write(repo_two.join("borrowed-dirty"), "keep two").unwrap();
        std::fs::write(local.join("borrowed-dirty"), "keep local").unwrap();
        let snapshot = |path: &Path| {
            let output = Command::new("git")
                .arg("-C")
                .arg(path)
                .args(["status", "--porcelain=v1", "--branch"])
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap()
        };
        let before = [snapshot(&repo_one), snapshot(&repo_two), snapshot(&local)];
        let one_binding =
            register_project(&one_home, &repo_one, None, CheckoutOwnership::UserOwned).unwrap();
        let two_binding =
            register_project(&two_home, &repo_two, None, CheckoutOwnership::UserOwned).unwrap();
        let local_binding =
            register_project(&local_home, &local, None, CheckoutOwnership::UserOwned).unwrap();
        assert_eq!(
            validate_publication_location(&one_home, &one_binding, &repo_one).unwrap(),
            "one/project"
        );
        assert_eq!(
            validate_publication_location(&two_home, &two_binding, &repo_two).unwrap(),
            "two/project"
        );
        assert!(
            validate_publication_location(&local_home, &local_binding, &local)
                .unwrap_err()
                .contains("remote-free")
        );

        let make_task =
            |id: &str, owner_home: &Path, binding: crate::project_registry::ProjectBinding| {
                let root = format!("root-home:{}", owner_home.display());
                let mut task = TaskRecord::new(
                    id.into(),
                    AssignmentRole::Implementer,
                    ArtifactKind::Implementation,
                    false,
                    root.clone(),
                    root,
                    owner_home.to_string_lossy().into_owned(),
                );
                task.briefs[0].scope = format!("deliver {id}");
                task.project = Some(binding);
                task
            };
        let one = make_task("one", &one_home, one_binding);
        let two = make_task("two", &two_home, two_binding);
        let mut local_task = make_task("local", &local_home, local_binding);
        local_task.schedule.state = super::super::subagent_model::WorkState::Completed;
        persist_task(&one_state, &one);
        persist_task(&two_state, &two);
        persist_task(&local_state, &local_task);
        let mut one_request = request(&one, "one-published", '1');
        one_request.pr_url = Some("https://github.com/one/project/pull/1".into());
        let mut two_request = request(&two, "two-published", '2');
        two_request.pr_url = Some("https://github.com/two/project/pull/2".into());
        let mut local_request = request(&local_task, "local-complete", '3');
        local_request.outcome = DeliveryOutcome::EvidenceUpdated;
        local_request.pr_url = None;
        std::thread::scope(|scope| {
            let one = scope.spawn(|| record(&one_state, "one", &one_request));
            let two = scope.spawn(|| record(&two_state, "two", &two_request));
            let local = scope.spawn(|| record(&local_state, "local", &local_request));
            one.join().unwrap().unwrap();
            two.join().unwrap().unwrap();
            local.join().unwrap().unwrap();
        });

        let read = |state: &Path, id: &str| {
            let text = std::fs::read_to_string(state.join(format!("{id}.meta"))).unwrap();
            super::super::subagent_model::read_meta(id, &text).unwrap()
        };
        assert_eq!(
            read(&one_state, "one")
                .current_delivery_evidence()
                .unwrap()
                .pr_url,
            one_request.pr_url
        );
        assert_eq!(
            read(&two_state, "two")
                .current_delivery_evidence()
                .unwrap()
                .pr_url,
            two_request.pr_url
        );
        assert_eq!(
            read(&local_state, "local")
                .current_delivery_evidence()
                .unwrap()
                .pr_url,
            None
        );
        assert_eq!(
            [snapshot(&repo_one), snapshot(&repo_two), snapshot(&local)],
            before
        );
        assert_eq!(
            std::fs::read_to_string(repo_one.join("borrowed-dirty")).unwrap(),
            "keep one"
        );
        assert_eq!(
            std::fs::read_to_string(repo_two.join("borrowed-dirty")).unwrap(),
            "keep two"
        );
        assert_eq!(
            std::fs::read_to_string(local.join("borrowed-dirty")).unwrap(),
            "keep local"
        );
    }

    fn persist_task(state: &Path, record: &TaskRecord) {
        std::fs::create_dir_all(state).unwrap();
        let text = super::super::subagent_model::write_meta("kind=delivery\n", record).unwrap();
        std::fs::write(state.join(format!("{}.meta", record.task_id)), text).unwrap();
    }

    #[test]
    fn owner_record_is_repeat_safe_and_replays_frozen_parent_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let state = home.join("state");
        std::fs::create_dir_all(&state).unwrap();
        let root = format!("root-home:{}", home.display());
        let mut task = TaskRecord::new(
            "worker".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            root.clone(),
            root,
            home.to_string_lossy().into_owned(),
        );
        task.briefs[0].scope = "publish the implementation".into();
        persist_task(&state, &task);

        let first = request(&task, "first", 'a');
        let (task, _) = record(&state, "worker", &first).unwrap();
        assert_eq!(
            std::fs::read_dir(state.join("delivery-outcomes"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            std::fs::read_dir(state.join("parent-outbox"))
                .unwrap()
                .count(),
            1
        );
        let mut second = request(&task, "second", 'b');
        second.expected_current_commit = Some("a".repeat(40));
        let (current, _) = record(&state, "worker", &second).unwrap();
        assert_eq!(current.delivery.current_commit, Some("b".repeat(40)));

        let (replayed, historical) = record(&state, "worker", &first).unwrap();
        assert_eq!(historical.commit, "a".repeat(40));
        assert_eq!(replayed.delivery.current_commit, Some("b".repeat(40)));
        assert_eq!(replayed.delivery.history.len(), 2);
    }

    #[test]
    fn every_evidence_transition_boundary_recovers_without_losing_route_or_metadata() {
        use multplx_core::filesystem::TransitionFault;

        for fault in [
            TransitionFault::AfterIntent,
            TransitionFault::AfterWrite(0),
            TransitionFault::AfterProgress(0),
            TransitionFault::AfterWrite(1),
            TransitionFault::AfterProgress(1),
            TransitionFault::AfterCommit,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let home = temp.path().join("home");
            let state = home.join("state");
            std::fs::create_dir_all(&state).unwrap();
            let root = format!("root-home:{}", home.display());
            let mut task = TaskRecord::new(
                "worker".into(),
                AssignmentRole::Implementer,
                ArtifactKind::Implementation,
                false,
                root.clone(),
                root,
                home.to_string_lossy().into_owned(),
            );
            task.briefs[0].scope = "publish".into();
            persist_task(&state, &task);
            let request = request(&task, "faulted", 'f');
            assert!(record_with_fault(&state, "worker", &request, Some(fault)).is_err());
            let (recovered, evidence) = record(&state, "worker", &request).unwrap();
            assert_eq!(evidence.evidence_id, "faulted");
            assert_eq!(recovered.delivery.history.len(), 1);
            assert_eq!(
                std::fs::read_dir(state.join("parent-outbox"))
                    .unwrap()
                    .count(),
                1
            );
            let receipt = multplx_core::filesystem::read_transition_writes(
                &state,
                &format!("delivery-evidence-{}", evidence_key("worker", "faulted")),
            )
            .unwrap();
            assert_eq!(receipt.last().unwrap().path, PathBuf::from("worker.meta"));
        }
    }

    #[test]
    fn current_projection_keeps_publication_checks_and_review_independent() {
        let mut task = task("worker");
        let published = request(&task, "published", 'a');
        task.apply_delivery_evidence(&published).unwrap();

        let mut checks = request(&task, "checks", 'a');
        checks.outcome = DeliveryOutcome::EvidenceUpdated;
        checks.pr_url = None;
        checks.expected_current_commit = Some("a".repeat(40));
        task.apply_delivery_evidence(&checks).unwrap();
        let current = task.current_delivery_evidence().unwrap();
        assert_eq!(current.outcome, DeliveryOutcome::Published);
        assert_eq!(
            current.pr_url.as_deref(),
            Some("https://github.com/example/project/pull/1")
        );

        let mut findings = checks.clone();
        findings.evidence_id = "review-findings".into();
        findings.review = Some(DeliveryReview {
            outcome: ReviewOutcome::Findings,
            summary: "one unresolved defect".into(),
            artifact: None,
        });
        task.apply_delivery_evidence(&findings).unwrap();
        let entry = human_review_queue(&[task.clone()]).unwrap().remove(0);
        assert_eq!(entry.outcome, DeliveryOutcome::Published);
        assert_eq!(entry.state, HumanReviewState::ReviewFindings);
        assert!(entry.review_complete);
        assert!(!entry.review_clear);
        assert!(!entry.pr_ready);

        let mut cleared = findings;
        cleared.evidence_id = "review-passed".into();
        cleared.review = Some(DeliveryReview {
            outcome: ReviewOutcome::Passed,
            summary: "findings resolved".into(),
            artifact: None,
        });
        task.apply_delivery_evidence(&cleared).unwrap();
        let current = task.current_delivery_evidence().unwrap();
        assert_eq!(current.outcome, DeliveryOutcome::Published);
        assert_eq!(current.review.unwrap().outcome, ReviewOutcome::Passed);

        let mut failure = cleared;
        failure.evidence_id = "publication-failure".into();
        failure.outcome = DeliveryOutcome::PublicationFailed;
        failure.pr_url = None;
        task.apply_delivery_evidence(&failure).unwrap();
        let entry = human_review_queue(&[task]).unwrap().remove(0);
        assert_eq!(entry.state, HumanReviewState::PublicationFailed);
        assert!(!entry.pr_ready);
    }

    #[test]
    fn idle_coordinator_relays_child_publication_to_root() {
        use crate::lifecycle::subagent_model::{DomainBinding, WorkState, write_meta};

        let temp = tempfile::tempdir().unwrap();
        let root_home = temp.path().join("root");
        let root_state = root_home.join("state");
        let child_home = temp.path().join("child");
        let child_state = child_home.join("state");
        std::fs::create_dir_all(&root_state).unwrap();
        std::fs::create_dir_all(&child_state).unwrap();
        let root_id = format!("root-home:{}", root_home.display());

        let mut coordinator = TaskRecord::new(
            "coord".into(),
            AssignmentRole::SubOrchestrator,
            ArtifactKind::Coordination,
            true,
            root_id.clone(),
            root_id.clone(),
            root_home.to_string_lossy().into_owned(),
        );
        coordinator.private_home = true;
        coordinator.domain = Some(DomainBinding {
            domain_id: "domain".into(),
            coordinator_id: "coord".into(),
            scope_revision: 1,
            assignment_generation: 1,
            projects: vec![],
            idea_id: Some("idea".into()),
            scope: "coordinate delivery".into(),
        });
        coordinator.owning_coordinator = Some("coord".into());
        coordinator.runtime.endpoint = None;
        std::fs::write(
            root_state.join("coord.meta"),
            write_meta("kind=daemon\n", &coordinator).unwrap(),
        )
        .unwrap();

        let mut worker = TaskRecord::new(
            "worker".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            "coord".into(),
            root_id,
            child_home.to_string_lossy().into_owned(),
        );
        worker.parent_home = Some(root_home.to_string_lossy().into_owned());
        worker.parent_state = Some(root_state.to_string_lossy().into_owned());
        worker.owner_state = Some(child_state.to_string_lossy().into_owned());
        worker.owning_coordinator = Some("coord".into());
        worker.briefs[0].scope = "publish child PR".into();
        worker.schedule.state = WorkState::Running;
        persist_task(&child_state, &worker);

        record(
            &child_state,
            "worker",
            &request(&worker, "child-published", 'c'),
        )
        .unwrap();
        let child_health = crate::lifecycle::parent_channel::relay(&child_state, 32).unwrap();
        assert_eq!(child_health.relayed_this_tick, 1);
        assert_eq!(
            crate::lifecycle::parent_channel::pending_outcomes(&root_state).unwrap(),
            1
        );
        let mut visible = vec![coordinator.clone()];
        extend_review_records_from_outcomes(&root_state, &mut visible).unwrap();
        assert!(visible.iter().any(|record| record.task_id == "worker"));
        let queue = human_review_queue(&visible).unwrap();
        assert_eq!(queue[0].task_id, "worker");
        assert_eq!(queue[0].owner_home, worker.owner_home);
        crate::lifecycle::parent_channel::relay(&root_state, 32).unwrap();
        assert!(
            crate::lifecycle::parent_channel::inspect(&root_state)
                .unwrap()
                .archived_inbox
                >= 1
        );
    }
}

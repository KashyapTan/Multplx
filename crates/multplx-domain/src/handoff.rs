//! Validated queued-work transfer between home owners.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::backlog::move_items;
use crate::inheritance::validate_daemon_home;
use crate::lifecycle::subagent_model::{
    AssignmentRole, OwnershipTransfer, TaskRecord, read_meta, write_meta,
};
use multplx_core::filesystem::{atomic_replace, read_bounded_regular};
use multplx_core::identifiers::TaskId;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const TRANSFER_RECORD_LIMIT: usize = 4 * 1024 * 1024;

/// An explicit scope revision accepted as part of an ownership transfer.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScopeChange {
    pub expected_revision: u64,
    pub scope: String,
    pub acceptance_criteria: Vec<String>,
    pub source_artifacts: Vec<String>,
    pub reason: String,
}

/// Stable input for a resumable task ownership transfer.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TransferRequest {
    pub transfer_id: String,
    pub task_id: String,
    pub authoritative_state: PathBuf,
    pub expected_attempt_generation: u64,
    pub from_coordinator: String,
    pub to_coordinator: String,
    /// Home and state that own the destination coordinator's own record.
    pub destination_coordinator_owner_home: PathBuf,
    pub destination_coordinator_owner_state: PathBuf,
    /// Canonical home and state that own the destination coordinator record.
    /// For a private coordinator these are its runtime home and child-task state.
    pub destination_home: PathBuf,
    pub destination_state: PathBuf,
    pub reason: String,
    pub scope_change: Option<ScopeChange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionDisposition {
    /// No endpoint exists for the fenced attempt.
    Absent,
    /// The lifecycle owner stopped the exact endpoint and observed it absent.
    Stopped,
    /// The provider could not prove whether the endpoint remains active.
    Uncertain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferOutcome {
    pub transfer_id: String,
    pub task_id: String,
    pub authoritative_state: PathBuf,
    pub accepted_generation: u64,
    pub resumed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum TransferStage {
    Prepared,
    ExecutionReconciled,
    SourceFenced,
    Committed,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct TransferJournal {
    version: u32,
    request: TransferRequest,
    original_digest: String,
    original_attempt_id: String,
    stage: TransferStage,
    accepted_generation: Option<u64>,
}

fn transfer_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn transferred_brief(
    task: &TaskRecord,
    change: &ScopeChange,
) -> Result<(PathBuf, Vec<u8>), HandoffFailure> {
    let revision = change
        .expected_revision
        .checked_add(1)
        .ok_or_else(|| fail("error: brief revision exhausted"))?;
    let role = match task.role {
        AssignmentRole::Researcher => "researcher",
        AssignmentRole::Implementer => "implementer",
        AssignmentRole::Reviewer => "reviewer",
        AssignmentRole::SubOrchestrator => "sub-orchestrator",
    };
    let artifact = match task.artifact {
        crate::lifecycle::subagent_model::ArtifactKind::Report => "report",
        crate::lifecycle::subagent_model::ArtifactKind::Implementation => "implementation",
        crate::lifecycle::subagent_model::ArtifactKind::Coordination => "coordination",
    };
    let previous = task
        .accepted_brief_path
        .as_deref()
        .unwrap_or("legacy source not recorded");
    let bytes = format!(
        "# Accepted brief revision {revision}\n\nAssignment: {role}\nRequested artifact: {artifact}\n\n## Current scope\n{}\n\n## Acceptance criteria\n{}\n\n## Source artifacts\n{}\n\n## Historical context\nPrevious accepted brief: {previous}\nThis pointer preserves prior evidence; the previous assignment has been superseded by the current scope above.\n\nRevision reason: {}\n",
        change.scope,
        change
            .acceptance_criteria
            .iter()
            .map(|criterion| format!("- {criterion}"))
            .collect::<Vec<_>>()
            .join("\n"),
        change
            .source_artifacts
            .iter()
            .map(|source| format!("- {source}"))
            .collect::<Vec<_>>()
            .join("\n"),
        change.reason,
    )
    .into_bytes();
    Ok((
        PathBuf::from(task.owner_state.as_deref().unwrap_or(""))
            .join("brief-revisions")
            .join(format!("{}-{revision}.md", task.task_id)),
        bytes,
    ))
}

fn transfer_journal_path(request: &TransferRequest) -> PathBuf {
    request
        .authoritative_state
        .join(format!(".ownership-transfer.{}.json", request.transfer_id))
}

/// Read a retained transfer intent for exact CLI retry. The caller still
/// passes the result through `transfer_task`, which revalidates every route and
/// generation before returning a committed receipt.
pub fn retained_transfer_request(
    state: &Path,
    transfer_id: &str,
) -> Result<Option<TransferRequest>, HandoffFailure> {
    TaskId::parse(transfer_id).map_err(|error| fail(format!("error: {error}")))?;
    let path = state.join(format!(".ownership-transfer.{transfer_id}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let bytes = read_bounded_regular(&path, TRANSFER_RECORD_LIMIT).map_err(|error| {
        fail(format!(
            "error: retained ownership transfer is unsafe: {error}"
        ))
    })?;
    let journal: TransferJournal = serde_json::from_slice(&bytes)
        .map_err(|_| fail("error: retained ownership transfer is malformed"))?;
    if journal.version != 1 || journal.request.transfer_id != transfer_id {
        return Err(fail("error: retained ownership transfer identity mismatch"));
    }
    Ok(Some(journal.request))
}

fn publish_transfer_journal(path: &Path, journal: &TransferJournal) -> Result<(), HandoffFailure> {
    let mut bytes = serde_json::to_vec(journal).map_err(|error| fail(error.to_string()))?;
    bytes.push(b'\n');
    atomic_replace(path, &bytes, 0o600).map_err(|error| fail(error.to_string()))
}

fn read_transfer_meta(request: &TransferRequest) -> Result<(Vec<u8>, TaskRecord), HandoffFailure> {
    let path = request
        .authoritative_state
        .join(format!("{}.meta", request.task_id));
    let bytes = read_bounded_regular(&path, TRANSFER_RECORD_LIMIT).map_err(|error| {
        fail(format!(
            "error: cannot read canonical task metadata: {error}"
        ))
    })?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| fail("error: canonical task metadata is not UTF-8"))?;
    let record = read_meta(&request.task_id, text)
        .map_err(|error| fail(format!("error: invalid canonical task metadata: {error}")))?;
    Ok((bytes, record))
}

fn validate_transfer_request(request: &TransferRequest) -> Result<(), HandoffFailure> {
    for value in [&request.transfer_id, &request.task_id] {
        TaskId::parse(value).map_err(|error| fail(format!("error: {error}")))?;
    }
    for value in [&request.from_coordinator, &request.to_coordinator] {
        validate_owner_identity(value)?;
    }
    if request.expected_attempt_generation == 0
        || request.from_coordinator == request.to_coordinator
        || request.reason.trim().is_empty()
        || !request.authoritative_state.is_absolute()
        || !request.destination_coordinator_owner_home.is_absolute()
        || !request.destination_coordinator_owner_state.is_absolute()
        || !request.destination_home.is_absolute()
        || !request.destination_state.is_absolute()
        || (!request.to_coordinator.starts_with("root-home:")
            && request.destination_state != request.destination_home.join("state"))
    {
        return Err(fail("error: invalid ownership transfer request"));
    }
    Ok(())
}

fn validate_owner_identity(value: &str) -> Result<(), HandoffFailure> {
    if let Some(home) = value.strip_prefix("root-home:") {
        if Path::new(home).is_absolute() {
            return Ok(());
        }
    } else if let Some((home, task)) = value.rsplit_once("#task:") {
        if Path::new(home).is_absolute() && TaskId::parse(task).is_ok() {
            return Ok(());
        }
    } else if TaskId::parse(value).is_ok() {
        return Ok(());
    }
    Err(fail("error: invalid canonical coordinator identity"))
}

fn coordinator_task_id(identity: &str) -> Option<&str> {
    identity.rsplit_once("#task:").map_or_else(
        || (!identity.starts_with("root-home:")).then_some(identity),
        |(_, task)| Some(task),
    )
}

fn canonical_owner(identity: &str, home: Option<&str>) -> String {
    if identity.starts_with("root-home:") || identity.contains("#task:") {
        identity.to_owned()
    } else {
        home.map_or_else(
            || identity.to_owned(),
            |home| crate::lifecycle::subagent_model::qualified_task_id(home, identity),
        )
    }
}

fn validate_destination(
    request: &TransferRequest,
    task: &TaskRecord,
) -> Result<(), HandoffFailure> {
    if task.root_id.as_deref() == Some(request.to_coordinator.as_str()) {
        if task
            .root_id
            .as_deref()
            .and_then(|root| root.strip_prefix("root-home:"))
            != request.destination_home.to_str()
            || request.destination_coordinator_owner_home != request.destination_home
            || request.destination_coordinator_owner_state != request.destination_state
        {
            return Err(fail(
                "error: destination root route does not match root identity",
            ));
        }
        return Ok(());
    }
    let coordinator_id = coordinator_task_id(&request.to_coordinator)
        .ok_or_else(|| fail("error: destination coordinator identity has no task"))?;
    if request
        .to_coordinator
        .rsplit_once("#task:")
        .is_some_and(|(home, _)| Path::new(home) != request.destination_coordinator_owner_home)
    {
        return Err(fail("error: destination coordinator route is wrong-home"));
    }
    let path = request
        .destination_coordinator_owner_state
        .join(format!("{coordinator_id}.meta"));
    let bytes = read_bounded_regular(&path, TRANSFER_RECORD_LIMIT).map_err(|_| {
        fail("error: destination coordinator metadata is unavailable; transfer retained")
    })?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| fail("error: destination coordinator metadata is not UTF-8"))?;
    let coordinator = read_meta(coordinator_id, text)
        .map_err(|error| fail(format!("error: invalid destination coordinator: {error}")))?;
    if coordinator.role != AssignmentRole::SubOrchestrator
        || coordinator.root_id != task.root_id
        || coordinator.owner_state.as_deref().map(Path::new)
            != Some(request.destination_coordinator_owner_state.as_path())
        || coordinator.owner_home.as_deref().map(Path::new)
            != Some(request.destination_coordinator_owner_home.as_path())
        || coordinator
            .persistent_home
            .as_deref()
            .map(Path::new)
            .unwrap_or_else(|| Path::new(coordinator.owner_home.as_deref().unwrap_or("")))
            != request.destination_home
    {
        return Err(fail(
            "error: destination is not the current coordinator for this root",
        ));
    }
    let moved = crate::lifecycle::subagent_model::qualified_task_id(
        task.owner_home.as_deref().unwrap_or(""),
        &task.task_id,
    );
    let mut current = coordinator;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..1024 {
        let current_key = crate::lifecycle::subagent_model::qualified_task_id(
            current.owner_home.as_deref().unwrap_or(""),
            &current.task_id,
        );
        if current_key == moved {
            return Err(fail(
                "error: ownership transfer would create a coordinator cycle",
            ));
        }
        if !seen.insert(current_key) {
            return Err(fail(
                "error: destination coordinator lineage already contains a cycle",
            ));
        }
        let Some(parent_id) = current.parent_id.as_deref() else {
            return Ok(());
        };
        if current.root_id.as_deref() == Some(parent_id) || parent_id.starts_with("root-home:") {
            return Ok(());
        }
        let parent_state = current
            .parent_state
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| {
                current
                    .parent_home
                    .as_deref()
                    .map(|home| Path::new(home).join("state"))
            })
            .ok_or_else(|| fail("error: destination coordinator parent route is incomplete"))?;
        if let Some(root_home) = current
            .root_id
            .as_deref()
            .and_then(|root| root.strip_prefix("root-home:"))
            && current.parent_home.as_deref().map(Path::new) == Some(Path::new(root_home))
            && parent_state == Path::new(root_home).join("state")
            && !parent_state.join(format!("{parent_id}.meta")).exists()
        {
            return Ok(());
        }
        let bytes = read_bounded_regular(
            parent_state.join(format!("{parent_id}.meta")),
            TRANSFER_RECORD_LIMIT,
        )
        .map_err(|_| fail("error: destination coordinator ancestry is unavailable"))?;
        current = read_meta(
            parent_id,
            std::str::from_utf8(&bytes)
                .map_err(|_| fail("error: destination coordinator ancestry is not UTF-8"))?,
        )
        .map_err(|error| {
            fail(format!(
                "error: destination coordinator ancestry is invalid: {error}"
            ))
        })?;
    }
    Err(fail(
        "error: destination coordinator ancestry exceeds the transfer bound",
    ))
}

fn committed_transfer(request: &TransferRequest, task: &TaskRecord) -> Option<TransferOutcome> {
    task.transfers
        .iter()
        .find(|transfer| {
            transfer.transfer_id == request.transfer_id
                && transfer.from_coordinator == request.from_coordinator
                && transfer.to_coordinator == request.to_coordinator
                && transfer.expected_generation == request.expected_attempt_generation
        })
        .and_then(|transfer| transfer.accepted_generation)
        .map(|accepted_generation| TransferOutcome {
            transfer_id: request.transfer_id.clone(),
            task_id: request.task_id.clone(),
            authoritative_state: request.destination_state.clone(),
            accepted_generation,
            resumed: true,
        })
}

/// Transfer one canonical task between coordinator routes without duplicating
/// its authority. The reconciler must stop or isolate the exact endpoint and
/// return only after observing its postcondition. A crash before the metadata
/// commit resumes from the retained journal with the same transfer identity.
pub fn transfer_task<F>(
    request: &TransferRequest,
    mut reconcile_execution: F,
) -> Result<TransferOutcome, HandoffFailure>
where
    F: FnMut(&TaskRecord) -> Result<ExecutionDisposition, String>,
{
    validate_transfer_request(request)?;
    fs::create_dir_all(&request.authoritative_state)
        .map_err(|error| fail(format!("error: cannot create task state: {error}")))?;
    fs::create_dir_all(&request.destination_state)
        .map_err(|error| fail(format!("error: cannot create destination state: {error}")))?;
    let mut lock_paths = vec![
        request
            .authoritative_state
            .join(format!(".teardown.{}.lock", request.task_id)),
        request
            .authoritative_state
            .join(format!(".ownership-transfer.{}.lock", request.task_id)),
        request
            .destination_state
            .join(format!(".ownership-transfer.{}.lock", request.task_id)),
    ];
    lock_paths.sort();
    lock_paths.dedup();
    let mut _locks = Vec::new();
    for lock_path in lock_paths {
        _locks.push(
            DirectoryLock::acquire_wait(
                lock_path,
                &SystemProcessProbe::default(),
                Duration::from_secs(5),
            )
            .map_err(|error| fail(format!("error: ownership transfer is busy: {error}")))?,
        );
    }
    let path = transfer_journal_path(request);
    let source_meta = request
        .authoritative_state
        .join(format!("{}.meta", request.task_id));
    let destination_meta = request
        .destination_state
        .join(format!("{}.meta", request.task_id));
    if destination_meta != source_meta && destination_meta.exists() {
        return Err(fail(
            "error: destination already has a different task authority",
        ));
    }
    let mut journal = if path.exists() {
        let bytes = read_bounded_regular(&path, TRANSFER_RECORD_LIMIT).map_err(|error| {
            fail(format!(
                "error: ownership transfer journal is unsafe: {error}"
            ))
        })?;
        let journal: TransferJournal = serde_json::from_slice(&bytes)
            .map_err(|_| fail("error: ownership transfer journal is malformed; retained"))?;
        if journal.version != 1 || journal.request != *request {
            return Err(fail(
                "error: ownership transfer identity conflicts with retained intent",
            ));
        }
        journal
    } else {
        let (meta_bytes, task) = read_transfer_meta(request)?;
        let attempt = task
            .attempt
            .as_ref()
            .ok_or_else(|| fail("error: task attempt identity is unavailable"))?;
        let current_owner = task
            .owning_coordinator
            .as_deref()
            .map(|owner| canonical_owner(owner, task.parent_home.as_deref()));
        let current_parent = task
            .parent_id
            .as_deref()
            .map(|owner| canonical_owner(owner, task.parent_home.as_deref()));
        if attempt.generation != request.expected_attempt_generation
            || current_owner.as_deref() != Some(request.from_coordinator.as_str())
            || current_parent.as_deref() != Some(request.from_coordinator.as_str())
        {
            return Err(fail("error: stale ownership transfer generation or owner"));
        }
        let journal = TransferJournal {
            version: 1,
            request: request.clone(),
            original_digest: transfer_digest(&meta_bytes),
            original_attempt_id: attempt.id.clone(),
            stage: TransferStage::Prepared,
            accepted_generation: None,
        };
        publish_transfer_journal(&path, &journal)?;
        journal
    };

    if journal.stage == TransferStage::Committed {
        let (_, task) = read_transfer_meta(request)?;
        return committed_transfer(request, &task)
            .map(|outcome| TransferOutcome {
                authoritative_state: request.authoritative_state.clone(),
                ..outcome
            })
            .ok_or_else(|| fail("error: committed ownership transfer record is unavailable"));
    }

    let mut meta_bytes = read_bounded_regular(&source_meta, TRANSFER_RECORD_LIMIT)
        .map_err(|_| fail("error: canonical task authority is unavailable; transfer retained"))?;
    let mut task = read_meta(
        &request.task_id,
        std::str::from_utf8(&meta_bytes)
            .map_err(|_| fail("error: fenced task authority is not UTF-8"))?,
    )
    .map_err(|error| fail(format!("error: fenced task authority is invalid: {error}")))?;
    validate_destination(request, &task)?;

    if journal.stage == TransferStage::Prepared {
        match reconcile_execution(&task).map_err(|error| {
            fail(format!(
                "error: prior execution reconciliation failed; transfer retained: {error}"
            ))
        })? {
            ExecutionDisposition::Absent | ExecutionDisposition::Stopped => {}
            ExecutionDisposition::Uncertain => {
                return Err(fail(
                    "error: prior execution remains uncertain; transfer retained",
                ));
            }
        }
        journal.stage = TransferStage::ExecutionReconciled;
        publish_transfer_journal(&path, &journal)?;
    }

    if journal.stage == TransferStage::ExecutionReconciled {
        (meta_bytes, task) = read_transfer_meta(request)?;
        if transfer_digest(&meta_bytes) != journal.original_digest
            || task.attempt.as_ref().map(|attempt| attempt.id.as_str())
                != Some(journal.original_attempt_id.as_str())
        {
            return Err(fail(
                "error: canonical task changed after reconciliation; transfer retained",
            ));
        }
        journal.stage = TransferStage::SourceFenced;
        publish_transfer_journal(&path, &journal)?;
    }

    if journal.stage == TransferStage::SourceFenced {
        meta_bytes = read_bounded_regular(&source_meta, TRANSFER_RECORD_LIMIT).map_err(|_| {
            fail("error: canonical task authority is unavailable; transfer retained")
        })?;
        task = read_meta(
            &request.task_id,
            std::str::from_utf8(&meta_bytes)
                .map_err(|_| fail("error: fenced source authority is not UTF-8"))?,
        )
        .map_err(|error| {
            fail(format!(
                "error: fenced source authority is invalid: {error}"
            ))
        })?;
        if transfer_digest(&meta_bytes) != journal.original_digest
            || task.attempt.as_ref().map(|attempt| attempt.id.as_str())
                != Some(journal.original_attempt_id.as_str())
        {
            return Err(fail(
                "error: canonical task changed after source fencing; transfer retained",
            ));
        }
        task.replace_attempt(&journal.original_attempt_id, true)
            .map_err(|error| fail(format!("error: cannot fence prior attempt: {error}")))?;
        if let Some(change) = &request.scope_change {
            let (brief_path, brief_bytes) = transferred_brief(&task, change)?;
            fs::create_dir_all(
                brief_path
                    .parent()
                    .ok_or_else(|| fail("error: transferred brief path has no parent"))?,
            )
            .map_err(|error| {
                fail(format!(
                    "error: cannot create brief revision directory: {error}"
                ))
            })?;
            if brief_path.exists() {
                let existing =
                    read_bounded_regular(&brief_path, TRANSFER_RECORD_LIMIT).map_err(|error| {
                        fail(format!("error: transferred brief is unsafe: {error}"))
                    })?;
                if existing != brief_bytes {
                    return Err(fail(
                        "error: transferred brief revision conflicts with retained content",
                    ));
                }
            } else {
                atomic_replace(&brief_path, &brief_bytes, 0o600).map_err(|error| {
                    fail(format!("error: cannot persist transferred brief: {error}"))
                })?;
            }
            task.accepted_brief_digest = Some(transfer_digest(&brief_bytes));
            task.accepted_brief_path = Some(brief_path.to_string_lossy().into_owned());
            task.revise_assignment(
                change.expected_revision,
                task.role,
                change.scope.clone(),
                change.acceptance_criteria.clone(),
                change.source_artifacts.clone(),
                change.reason.clone(),
            )
            .map_err(|error| fail(format!("error: cannot record scope revision: {error}")))?;
        }
        let accepted_generation = task
            .attempt
            .as_ref()
            .map(|attempt| attempt.generation)
            .ok_or_else(|| fail("error: replacement attempt is unavailable"))?;
        task.parent_id = Some(
            coordinator_task_id(&request.to_coordinator)
                .unwrap_or(&request.to_coordinator)
                .to_owned(),
        );
        task.parent_home = Some(
            request
                .destination_coordinator_owner_home
                .to_string_lossy()
                .into_owned(),
        );
        task.parent_state = Some(
            request
                .destination_coordinator_owner_state
                .to_string_lossy()
                .into_owned(),
        );
        task.owning_coordinator = Some(request.to_coordinator.clone());
        task.transfers.push(OwnershipTransfer {
            transfer_id: request.transfer_id.clone(),
            from_coordinator: request.from_coordinator.clone(),
            to_coordinator: request.to_coordinator.clone(),
            expected_generation: request.expected_attempt_generation,
            accepted_generation: Some(accepted_generation),
            reason: request.reason.clone(),
        });
        task.validate()
            .map_err(|error| fail(format!("error: transferred task is invalid: {error}")))?;
        let old = std::str::from_utf8(&meta_bytes)
            .map_err(|_| fail("error: canonical task metadata is not UTF-8"))?;
        let old_without_stopped_endpoint = old
            .lines()
            .filter(|line| !line.starts_with("window=") && !line.starts_with("session_id="))
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        let updated = write_meta(&old_without_stopped_endpoint, &task)
            .map_err(|error| fail(format!("error: cannot render transferred task: {error}")))?;
        atomic_replace(&source_meta, updated.as_bytes(), 0o600)
            .map_err(|error| fail(format!("error: cannot commit ownership transfer: {error}")))?;
        journal.accepted_generation = Some(accepted_generation);
        journal.stage = TransferStage::Committed;
        publish_transfer_journal(&path, &journal)?;
    }

    Ok(TransferOutcome {
        transfer_id: request.transfer_id.clone(),
        task_id: request.task_id.clone(),
        authoritative_state: request.authoritative_state.clone(),
        accepted_generation: journal
            .accepted_generation
            .ok_or_else(|| fail("error: transfer receipt lacks accepted generation"))?,
        resumed: false,
    })
}

#[derive(Debug)]
pub struct HandoffFailure {
    pub message: String,
}

fn fail(message: impl Into<String>) -> HandoffFailure {
    HandoffFailure {
        message: message.into(),
    }
}

fn registry_home(registry: &Path, id: &str) -> Result<PathBuf, HandoffFailure> {
    let text = fs::read_to_string(registry).map_err(|_| {
        fail(format!(
            "error: no daemon registry at {}",
            registry.display()
        ))
    })?;
    let mut matching = None;
    for line in text.lines() {
        if (line == format!("- {id}") || line.starts_with(&format!("- {id} ")))
            && matching.replace(line).is_some()
        {
            return Err(fail(format!(
                "error: duplicate persistent sub-agent registry identity: {id}"
            )));
        }
    }
    let line = matching.ok_or_else(|| {
        fail(format!(
            "error: daemon {id} is not registered in {}",
            registry.display()
        ))
    })?;
    let marker = "(home:";
    if line.matches(marker).count() > 1 {
        return Err(fail(format!(
            "error: conflicting persistent sub-agent home identity: {id}"
        )));
    }
    let Some(start) = line.find(marker) else {
        return Err(fail(format!(
            "error: daemon {id} has no home in {}",
            registry.display()
        )));
    };
    let remainder = line[start + marker.len()..].trim_start();
    let Some(end) = remainder.find(';') else {
        return Err(fail(format!(
            "error: daemon {id} has no home in {}",
            registry.display()
        )));
    };
    let home = remainder[..end].trim_end();
    if home.is_empty() {
        return Err(fail(format!(
            "error: daemon {id} has no home in {}",
            registry.display()
        )));
    }
    Ok(PathBuf::from(home))
}

fn validate_backlog(label: &str, path: &Path) -> Result<(), HandoffFailure> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(fail(format!(
            "error: {label} must not be a symlink: {}",
            path.display()
        )));
    }
    if path.exists() && !path.is_file() {
        return Err(fail(format!(
            "error: {label} is not a regular file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn classify(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut section = "## Queued".to_owned();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("##") {
            let normalized = rest.split_whitespace().collect::<Vec<_>>().join(" ");
            section = format!("## {normalized}");
            continue;
        }
        if line.starts_with("- [ ] ") || line.starts_with("- [x] ") {
            let id = line[6..].split_whitespace().next().unwrap_or("");
            if id == key {
                return Some(section);
            }
        }
    }
    None
}

fn noncanonical_lines(path: &Path, key: &str) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut capturing = false;
    let mut output = Vec::new();
    for line in text.lines() {
        if line.starts_with("- [ ] ") || line.starts_with("- [x] ") {
            if capturing {
                break;
            }
            capturing = line[6..].split_whitespace().next().unwrap_or("") == key;
            continue;
        }
        if capturing && line.starts_with("##") {
            break;
        }
        if capturing
            && (line.starts_with(' ') || line.starts_with('\t'))
            && !line.starts_with("  ")
            && !line.trim().is_empty()
        {
            output.push(line.to_owned());
        }
    }
    output
}

pub fn run(
    root: &Path,
    home: &Path,
    data: &Path,
    id: &str,
    keys: &[String],
) -> Result<String, HandoffFailure> {
    if keys.is_empty() {
        return Err(fail(
            "usage: mx backlog-handoff <persistent-subagent-id> <item-key>... (queued work between home owners)",
        ));
    }
    let registry = data.join("daemons.md");
    let raw_home = registry_home(&registry, id)?;
    let destination_home = validate_daemon_home(id, &raw_home, home, root).map_err(|reason| {
        fail(format!(
            "error: Multplx home {} is unsafe: {reason}",
            raw_home.display()
        ))
    })?;
    let source = data.join("backlog.md");
    let destination = destination_home.path.join("data/backlog.md");
    validate_backlog("main backlog", &source)?;
    validate_backlog("daemon backlog", &destination)?;

    let mut to_move = Vec::new();
    let mut already = Vec::new();
    let mut missing = Vec::new();
    let mut in_flight = Vec::new();
    let mut done = Vec::new();
    let mut nonqueued = Vec::new();
    for key in keys {
        if classify(&destination, key).is_some() {
            already.push(key.clone());
        } else {
            match classify(&source, key).as_deref() {
                Some("## Queued") => to_move.push(key.clone()),
                Some("## In flight") => in_flight.push(key.clone()),
                Some("## Done") => done.push(key.clone()),
                Some(_) => nonqueued.push(key.clone()),
                None => missing.push(key.clone()),
            }
        }
    }
    let mut errors = String::new();
    if !in_flight.is_empty() {
        errors.push_str(&format!(
            "error: refusing to hand off in-flight backlog items: {}\n",
            in_flight.join(" ")
        ));
    }
    if !done.is_empty() {
        errors.push_str(&format!(
            "error: refusing to hand off Done (historical) backlog items: {}; handoffs move in-scope queued work only - Done records stay with their home and are pruned/archived.\n",
            done.join(" ")
        ));
    }
    if !nonqueued.is_empty() {
        errors.push_str(&format!(
            "error: refusing to hand off non-queued backlog items: {}; handoffs move in-scope queued work only.\n",
            nonqueued.join(" ")
        ));
    }
    if !missing.is_empty() {
        errors.push_str(&format!(
            "error: no backlog item matched these keys in {}: {}\n",
            source.display(),
            missing.join(" ")
        ));
    }
    if !errors.is_empty() {
        errors.push_str("       nothing was moved.");
        return Err(fail(errors));
    }
    if to_move.is_empty() {
        return Ok(format!(
            "nothing to move: {} already present in {}\n",
            if already.is_empty() {
                "no keys".to_owned()
            } else {
                already.join(" ")
            },
            destination.display()
        ));
    }
    let mut malformed = String::new();
    for key in &to_move {
        for line in noncanonical_lines(&source, key) {
            malformed.push_str(&format!(
                "error: refusing to hand off {key}: non-2-space continuation line: {line}\n"
            ));
        }
    }
    if !malformed.is_empty() {
        malformed.push_str("       nothing was moved.");
        return Err(fail(malformed));
    }
    fs::create_dir_all(destination_home.path.join("data"))
        .map_err(|error| fail(error.to_string()))?;
    move_items(&source, &destination, &to_move).map_err(|error| {
        fail(format!(
            "mx-backlog: {error}\nerror: backlog move failed; nothing was moved."
        ))
    })?;
    let mut output = format!(
        "handed off {} item(s) to {id}: {}\n  into {}\n",
        to_move.len(),
        to_move.join(" "),
        destination.display()
    );
    if !already.is_empty() {
        output.push_str(&format!(
            "  already present (skipped): {}\n",
            already.join(" ")
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backlog(queued: &str, in_flight: &str, done: &str) -> String {
        format!("## In flight\n{in_flight}\n## Queued\n{queued}\n## Done\n{done}")
    }

    fn seeded_homes() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let active = temp.path().join("active");
        let daemon = temp.path().join("daemon");
        for path in [&root, &active, &daemon] {
            fs::create_dir_all(path).expect("home");
        }
        fs::create_dir_all(active.join("data")).expect("active data");
        for name in ["data", "state", "config", "projects", "bin"] {
            fs::create_dir_all(daemon.join(name)).expect("daemon surface");
        }
        fs::write(daemon.join(".mx-daemon-home"), "worker\n").expect("marker");
        fs::write(daemon.join("AGENTS.md"), "# daemon\n").expect("agents");
        fs::write(
            active.join("data/daemons.md"),
            format!(
                "- worker - tests (home: {}; scope: tests)\n",
                daemon.display()
            ),
        )
        .expect("registry");
        (temp, root, active, daemon)
    }

    #[test]
    fn registry_rejects_conflicting_home_fields_after_parenthesized_prose() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("daemons.md");
        fs::write(
            &registry,
            "- daemon - work (id is legacy) (home: /first; note: x) (home: /last; scope: work)\n",
        )
        .expect("registry");
        assert!(registry_home(&registry, "daemon").is_err());
        fs::write(&registry, "- daemon - work (home: /one; scope: work)\n- daemon - work (home: /two; scope: work)\n").unwrap();
        assert!(registry_home(&registry, "daemon").is_err());
    }

    #[test]
    fn lightweight_classification_accepts_whitespace_headings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("backlog.md");
        fs::write(&path, "##\tDone\n- [x] old - old\n").expect("backlog");
        assert_eq!(classify(&path, "old").as_deref(), Some("## Done"));
    }

    #[test]
    fn handoff_moves_queued_items_and_reports_idempotent_keys() {
        let (_temp, root, active, daemon) = seeded_homes();
        fs::write(
            active.join("data/backlog.md"),
            backlog("- [ ] new - New\n  body\n", "", ""),
        )
        .expect("source");
        fs::write(
            daemon.join("data/backlog.md"),
            backlog("- [ ] existing - Existing\n", "", ""),
        )
        .expect("destination");
        let keys = ["new".to_owned(), "existing".to_owned()];
        let output = run(&root, &active, &active.join("data"), "worker", &keys).expect("handoff");
        assert!(output.contains("handed off 1 item(s)"));
        assert!(output.contains("already present (skipped): existing"));
        assert!(
            !fs::read_to_string(active.join("data/backlog.md"))
                .expect("source")
                .contains("new - New")
        );
        assert!(
            fs::read_to_string(daemon.join("data/backlog.md"))
                .expect("destination")
                .contains("  body")
        );

        let second =
            run(&root, &active, &active.join("data"), "worker", &keys).expect("idempotent");
        assert!(second.starts_with("nothing to move:"));
    }

    #[test]
    fn handoff_refuses_every_nonqueued_class_without_mutation() {
        let (_temp, root, active, daemon) = seeded_homes();
        let source = backlog(
            "- [ ] queued - Queued\n",
            "- [ ] active - Active\n",
            "- [x] historical - Historical\n",
        )
        .replace("## Queued", "## Other\n- [ ] other - Other\n\n## Queued");
        fs::write(active.join("data/backlog.md"), &source).expect("source");
        fs::write(daemon.join("data/backlog.md"), backlog("", "", "")).expect("destination");
        let keys = [
            "active".to_owned(),
            "historical".to_owned(),
            "other".to_owned(),
            "missing".to_owned(),
        ];
        let error =
            run(&root, &active, &active.join("data"), "worker", &keys).expect_err("refusal");
        assert!(error.message.contains("in-flight"));
        assert!(error.message.contains("Done (historical)"));
        assert!(error.message.contains("non-queued"));
        assert!(error.message.contains("no backlog item matched"));
        assert!(error.message.ends_with("nothing was moved."));
        assert_eq!(
            fs::read_to_string(active.join("data/backlog.md")).expect("after"),
            source
        );
    }

    #[test]
    fn handoff_refuses_noncanonical_body_and_unsafe_backlogs() {
        let (_temp, root, active, daemon) = seeded_homes();
        fs::write(
            active.join("data/backlog.md"),
            backlog("- [ ] bad - Bad\n body\n", "", ""),
        )
        .expect("source");
        fs::write(daemon.join("data/backlog.md"), backlog("", "", "")).expect("destination");
        let error = run(
            &root,
            &active,
            &active.join("data"),
            "worker",
            &["bad".to_owned()],
        )
        .expect_err("noncanonical");
        assert!(error.message.contains("non-2-space continuation"));

        fs::remove_file(daemon.join("data/backlog.md")).expect("remove");
        fs::create_dir(daemon.join("data/backlog.md")).expect("directory");
        let error = run(
            &root,
            &active,
            &active.join("data"),
            "worker",
            &["bad".to_owned()],
        )
        .expect_err("unsafe destination");
        assert!(
            error
                .message
                .contains("daemon backlog is not a regular file")
        );
    }

    #[test]
    fn registry_and_home_validation_fail_closed() {
        let (_temp, root, active, _daemon) = seeded_homes();
        assert!(
            run(&root, &active, &active.join("data"), "worker", &[])
                .expect_err("usage")
                .message
                .starts_with("usage:")
        );
        assert!(
            run(
                &root,
                &active,
                &active.join("data"),
                "missing",
                &["x".to_owned()]
            )
            .expect_err("missing daemon")
            .message
            .contains("not registered")
        );
        fs::write(active.join("data/daemons.md"), "- worker - no home\n").expect("registry");
        assert!(
            run(
                &root,
                &active,
                &active.join("data"),
                "worker",
                &["x".to_owned()]
            )
            .expect_err("missing home")
            .message
            .contains("has no home")
        );
        fs::remove_file(active.join("data/daemons.md")).expect("remove registry");
        assert!(
            run(
                &root,
                &active,
                &active.join("data"),
                "worker",
                &["x".to_owned()]
            )
            .expect_err("missing registry")
            .message
            .contains("no daemon registry")
        );
    }

    #[test]
    fn malformed_registry_home_fields_and_backlog_symlinks_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("daemons.md");
        for row in [
            "- worker - incomplete (home: /tmp/worker)\n",
            "- worker - empty (home: ; scope: test)\n",
        ] {
            fs::write(&registry, row).expect("registry");
            assert!(
                registry_home(&registry, "worker")
                    .expect_err("malformed home")
                    .message
                    .contains("has no home")
            );
        }
        let target = temp.path().join("target-backlog");
        fs::write(&target, backlog("", "", "")).expect("target");
        let linked = temp.path().join("linked-backlog");
        std::os::unix::fs::symlink(&target, &linked).expect("backlog symlink");
        assert!(
            validate_backlog("fixture backlog", &linked)
                .expect_err("symlink refusal")
                .message
                .contains("must not be a symlink")
        );
    }

    fn write_record(path: &Path, record: &TaskRecord) {
        let kind = if record.persistent {
            "daemon"
        } else if record.artifact == crate::lifecycle::subagent_model::ArtifactKind::Report {
            "scout"
        } else {
            "delivery"
        };
        let text = write_meta(&format!("kind={kind}\n"), record).expect("render record");
        fs::write(path, text).expect("write record");
    }

    fn transfer_fixture() -> (tempfile::TempDir, TransferRequest, PathBuf, String) {
        use crate::lifecycle::subagent_model::ArtifactKind;

        let temp = tempfile::tempdir().expect("tempdir");
        let root_home = temp.path().join("root-home");
        let state = root_home.join("state");
        let destination_home = temp.path().join("destination-home");
        let destination_state = destination_home.join("state");
        fs::create_dir_all(&state).expect("root state");
        fs::create_dir_all(&destination_state).expect("destination state");
        let root_id = format!("root-home:{}", root_home.display());

        let mut destination = TaskRecord::new(
            "new-owner".into(),
            AssignmentRole::SubOrchestrator,
            ArtifactKind::Coordination,
            true,
            root_id.clone(),
            root_id.clone(),
            root_home.to_string_lossy().into_owned(),
        );
        destination.owner_state = Some(state.to_string_lossy().into_owned());
        destination.persistent_home = Some(destination_home.to_string_lossy().into_owned());
        write_record(&state.join("new-owner.meta"), &destination);

        let mut task = TaskRecord::new(
            "task-a".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            "old-owner".into(),
            root_id,
            root_home.to_string_lossy().into_owned(),
        );
        task.parent_home = Some(root_home.to_string_lossy().into_owned());
        task.parent_state = Some(state.to_string_lossy().into_owned());
        let old_owner = crate::lifecycle::subagent_model::qualified_task_id(
            root_home.to_str().expect("home"),
            "old-owner",
        );
        task.owning_coordinator = Some(old_owner.clone());
        task.runtime.provider = "fixture".into();
        task.runtime.endpoint = Some("endpoint-a".into());
        let old_attempt = task.attempt.as_ref().expect("attempt").id.clone();
        write_record(&state.join("task-a.meta"), &task);
        let request = TransferRequest {
            transfer_id: "transfer-a".into(),
            task_id: "task-a".into(),
            authoritative_state: state.clone(),
            expected_attempt_generation: 1,
            from_coordinator: old_owner,
            to_coordinator: crate::lifecycle::subagent_model::qualified_task_id(
                root_home.to_str().expect("home"),
                "new-owner",
            ),
            destination_coordinator_owner_home: root_home,
            destination_coordinator_owner_state: state.clone(),
            destination_home,
            destination_state,
            reason: "move the bounded assignment".into(),
            scope_change: Some(ScopeChange {
                expected_revision: 1,
                scope: "new explicit scope".into(),
                acceptance_criteria: vec!["accepted".into()],
                source_artifacts: vec!["research.md".into()],
                reason: "scope was changed by the owner".into(),
            }),
        };
        (temp, request, state.join("task-a.meta"), old_attempt)
    }

    #[test]
    fn ownership_transfer_fences_generation_and_keeps_one_authoritative_record() {
        let (_temp, request, meta, old_attempt) = transfer_fixture();
        let outcome = transfer_task(&request, |record| {
            assert_eq!(record.runtime.endpoint.as_deref(), Some("endpoint-a"));
            Ok(ExecutionDisposition::Stopped)
        })
        .expect("transfer");
        assert_eq!(outcome.accepted_generation, 2);
        assert!(!outcome.resumed);
        assert!(meta.exists());
        assert!(!request.destination_state.join("task-a.meta").exists());
        let raw = fs::read_to_string(&meta).expect("metadata");
        let task = read_meta("task-a", &raw).expect("task");
        assert_eq!(
            task.owning_coordinator.as_deref(),
            Some(request.to_coordinator.as_str())
        );
        assert_eq!(task.parent_id.as_deref(), Some("new-owner"));
        assert_eq!(task.accepted_brief_revision, Some(2));
        let brief_path = PathBuf::from(task.accepted_brief_path.as_deref().expect("brief path"));
        let brief = fs::read_to_string(&brief_path).expect("transferred brief");
        assert!(brief.contains("## Current scope\nnew explicit scope"));
        assert!(brief.contains("Previous accepted brief: legacy source not recorded"));
        let brief_digest = transfer_digest(brief.as_bytes());
        assert_eq!(
            task.accepted_brief_digest.as_deref(),
            Some(brief_digest.as_str())
        );
        assert_eq!(task.attempt.as_ref().expect("attempt").generation, 2);
        assert_eq!(task.prior_attempts[0].id, old_attempt);
        assert!(task.runtime.endpoint.is_none());
        assert_eq!(task.transfers[0].accepted_generation, Some(2));
        assert_eq!(outcome.authoritative_state, request.authoritative_state);

        let repeat = transfer_task(&request, |_| {
            panic!("committed transfer must not stop twice")
        })
        .expect("repeat");
        assert!(repeat.resumed);
        assert_eq!(repeat.accepted_generation, 2);
        let mut conflicting = request.clone();
        conflicting.reason = "different retry payload".into();
        assert!(
            transfer_task(&conflicting, |_| panic!(
                "conflicting retry must not reconcile"
            ))
            .unwrap_err()
            .message
            .contains("conflicts with retained intent")
        );
    }

    #[test]
    fn uncertain_execution_and_post_reconcile_mutation_fail_closed() {
        let (_temp, request, meta, _old_attempt) = transfer_fixture();
        let error = transfer_task(&request, |_| Ok(ExecutionDisposition::Uncertain))
            .expect_err("uncertain");
        assert!(error.message.contains("remains uncertain"));
        let task =
            read_meta("task-a", &fs::read_to_string(&meta).expect("metadata")).expect("task");
        assert_eq!(
            task.owning_coordinator.as_deref(),
            Some(request.from_coordinator.as_str())
        );

        let path = transfer_journal_path(&request);
        let mut journal: TransferJournal =
            serde_json::from_slice(&fs::read(&path).expect("journal")).expect("journal model");
        journal.stage = TransferStage::ExecutionReconciled;
        publish_transfer_journal(&path, &journal).expect("publish reconciled");
        fs::write(&meta, format!("{}\n", fs::read_to_string(&meta).unwrap()))
            .expect("mutate metadata bytes");
        let error = transfer_task(&request, |_| panic!("reconciled stage must not stop again"))
            .expect_err("mutation fence");
        assert!(error.message.contains("changed after reconciliation"));
        journal.stage = TransferStage::SourceFenced;
        publish_transfer_journal(&path, &journal).expect("publish source-fenced");
        let error = transfer_task(&request, |_| {
            panic!("source-fenced retry must not stop again")
        })
        .expect_err("source-fenced mutation fence");
        assert!(error.message.contains("changed after source fencing"));
    }

    #[test]
    fn transfer_to_a_descendant_coordinator_is_rejected_as_a_cycle() {
        let (_temp, request, _meta, _old_attempt) = transfer_fixture();
        let path = request
            .destination_coordinator_owner_state
            .join("new-owner.meta");
        let raw = fs::read_to_string(&path).unwrap();
        let mut descendant = read_meta("new-owner", &raw).unwrap();
        descendant.parent_id = Some("task-a".into());
        descendant.parent_home = descendant.owner_home.clone();
        descendant.parent_state = Some(request.authoritative_state.to_string_lossy().into_owned());
        write_record(&path, &descendant);
        let error =
            transfer_task(&request, |_| Ok(ExecutionDisposition::Absent)).expect_err("cycle");
        assert!(error.message.contains("coordinator cycle"));
    }

    #[test]
    fn domain_task_can_transfer_back_to_the_root_route() {
        let (_temp, request, _meta, _old_attempt) = transfer_fixture();
        transfer_task(&request, |_| Ok(ExecutionDisposition::Absent)).expect("into domain");
        let task = read_meta(
            "task-a",
            &fs::read_to_string(request.authoritative_state.join("task-a.meta")).unwrap(),
        )
        .unwrap();
        let root_id = task.root_id.clone().expect("root");
        let root_home = PathBuf::from(root_id.strip_prefix("root-home:").expect("root path"));
        let root_request = TransferRequest {
            transfer_id: "transfer-root".into(),
            task_id: "task-a".into(),
            authoritative_state: request.authoritative_state.clone(),
            expected_attempt_generation: 2,
            from_coordinator: request.to_coordinator.clone(),
            to_coordinator: root_id.clone(),
            destination_coordinator_owner_home: root_home.clone(),
            destination_coordinator_owner_state: root_home.join("state"),
            destination_home: root_home.clone(),
            destination_state: root_home.join("state"),
            reason: "return cross-domain priority to root".into(),
            scope_change: None,
        };
        let result = transfer_task(&root_request, |_| Ok(ExecutionDisposition::Absent))
            .expect("return to root");
        assert_eq!(result.accepted_generation, 3);
        let task = read_meta(
            "task-a",
            &fs::read_to_string(root_request.destination_state.join("task-a.meta")).unwrap(),
        )
        .unwrap();
        assert_eq!(task.parent_id.as_deref(), Some(root_id.as_str()));
        assert_eq!(task.owning_coordinator.as_deref(), Some(root_id.as_str()));
    }

    #[test]
    fn same_basename_coordinators_are_disambiguated_by_owner_home() {
        use crate::lifecycle::subagent_model::ArtifactKind;

        let (temp, request, _meta, _old_attempt) = transfer_fixture();
        transfer_task(&request, |_| Ok(ExecutionDisposition::Absent)).expect("first owner");
        let current_meta = request.authoritative_state.join("task-a.meta");
        let task = read_meta("task-a", &fs::read_to_string(&current_meta).unwrap()).unwrap();
        let second_owner_home = temp.path().join("second-owner");
        let second_owner_state = second_owner_home.join("state");
        let second_runtime = temp.path().join("second-runtime");
        fs::create_dir_all(&second_owner_state).unwrap();
        fs::create_dir_all(second_runtime.join("state")).unwrap();
        let mut coordinator = TaskRecord::new(
            "new-owner".into(),
            AssignmentRole::SubOrchestrator,
            ArtifactKind::Coordination,
            true,
            task.root_id.clone().unwrap(),
            task.root_id.clone().unwrap(),
            second_owner_home.to_string_lossy().into_owned(),
        );
        coordinator.owner_state = Some(second_owner_state.to_string_lossy().into_owned());
        coordinator.persistent_home = Some(second_runtime.to_string_lossy().into_owned());
        write_record(&second_owner_state.join("new-owner.meta"), &coordinator);
        let next = TransferRequest {
            transfer_id: "same-name-transfer".into(),
            task_id: "task-a".into(),
            authoritative_state: request.authoritative_state.clone(),
            expected_attempt_generation: 2,
            from_coordinator: request.to_coordinator.clone(),
            to_coordinator: crate::lifecycle::subagent_model::qualified_task_id(
                second_owner_home.to_str().unwrap(),
                "new-owner",
            ),
            destination_coordinator_owner_home: second_owner_home,
            destination_coordinator_owner_state: second_owner_state,
            destination_home: second_runtime.clone(),
            destination_state: second_runtime.join("state"),
            reason: "same basename, different canonical owner".into(),
            scope_change: None,
        };
        transfer_task(&next, |_| Ok(ExecutionDisposition::Absent)).expect("second owner");
        assert!(current_meta.exists());
        assert!(!next.destination_state.join("task-a.meta").exists());
        let moved = read_meta("task-a", &fs::read_to_string(&current_meta).unwrap()).unwrap();
        assert_eq!(
            moved.owning_coordinator.as_deref(),
            Some(next.to_coordinator.as_str())
        );
    }

    #[test]
    fn retained_transfer_reader_rejects_unsafe_malformed_and_mismatched_receipts() {
        let (temp, request, _meta, _old_attempt) = transfer_fixture();
        assert!(retained_transfer_request(temp.path(), "../bad").is_err());
        assert!(
            retained_transfer_request(&request.authoritative_state, "absent")
                .unwrap()
                .is_none()
        );

        let path = transfer_journal_path(&request);
        fs::write(&path, b"not-json\n").unwrap();
        assert!(
            retained_transfer_request(&request.authoritative_state, &request.transfer_id)
                .unwrap_err()
                .message
                .contains("malformed")
        );
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(
            retained_transfer_request(&request.authoritative_state, &request.transfer_id)
                .unwrap_err()
                .message
                .contains("unsafe")
        );
        fs::remove_dir(&path).unwrap();

        let journal = TransferJournal {
            version: 2,
            request: request.clone(),
            original_digest: "digest".into(),
            original_attempt_id: "attempt".into(),
            stage: TransferStage::Prepared,
            accepted_generation: None,
        };
        publish_transfer_journal(&path, &journal).unwrap();
        assert!(
            retained_transfer_request(&request.authoritative_state, &request.transfer_id)
                .unwrap_err()
                .message
                .contains("identity mismatch")
        );
    }

    #[test]
    fn transfer_request_validation_rejects_ambiguous_identity_and_route_shapes() {
        let (_temp, request, _meta, _old_attempt) = transfer_fixture();
        let mut cases = Vec::new();
        let mut changed = request.clone();
        changed.transfer_id = "../bad".into();
        cases.push(changed);
        let mut changed = request.clone();
        changed.task_id = "bad/id".into();
        cases.push(changed);
        let mut changed = request.clone();
        changed.from_coordinator = "root-home:relative".into();
        cases.push(changed);
        let mut changed = request.clone();
        changed.to_coordinator = "relative#task:bad/id".into();
        cases.push(changed);
        let mut changed = request.clone();
        changed.expected_attempt_generation = 0;
        cases.push(changed);
        let mut changed = request.clone();
        changed.to_coordinator = changed.from_coordinator.clone();
        cases.push(changed);
        let mut changed = request.clone();
        changed.reason = " \t".into();
        cases.push(changed);
        let mut changed = request.clone();
        changed.authoritative_state = PathBuf::from("relative");
        cases.push(changed);
        let mut changed = request.clone();
        changed.destination_coordinator_owner_home = PathBuf::from("relative");
        cases.push(changed);
        let mut changed = request.clone();
        changed.destination_coordinator_owner_state = PathBuf::from("relative");
        cases.push(changed);
        let mut changed = request.clone();
        changed.destination_home = PathBuf::from("relative");
        cases.push(changed);
        let mut changed = request.clone();
        changed.destination_state = PathBuf::from("relative");
        cases.push(changed);
        let mut changed = request.clone();
        changed.destination_state = changed.destination_home.join("custom-state");
        cases.push(changed);

        for invalid in cases {
            assert!(validate_transfer_request(&invalid).is_err(), "{invalid:?}");
        }
        assert!(validate_owner_identity("root-home:/tmp/root").is_ok());
        assert!(validate_owner_identity("/tmp/root#task:owner").is_ok());
        assert!(validate_owner_identity("owner").is_ok());
        assert!(validate_owner_identity("bad/id").is_err());
        assert_eq!(coordinator_task_id("root-home:/tmp/root"), None);
        assert_eq!(coordinator_task_id("/tmp/root#task:owner"), Some("owner"));
        assert_eq!(canonical_owner("owner", None), "owner");
    }

    #[test]
    fn destination_validation_refuses_stale_or_unprovable_coordinator_routes() {
        let (_temp, request, meta, _old_attempt) = transfer_fixture();
        let task = read_meta("task-a", &fs::read_to_string(&meta).unwrap()).unwrap();
        let coordinator_path = request
            .destination_coordinator_owner_state
            .join("new-owner.meta");
        let coordinator_raw = fs::read_to_string(&coordinator_path).unwrap();
        let coordinator = read_meta("new-owner", &coordinator_raw).unwrap();

        let mut wrong_home = request.clone();
        wrong_home.to_coordinator =
            crate::lifecycle::subagent_model::qualified_task_id("/tmp/not-the-owner", "new-owner");
        assert!(
            validate_destination(&wrong_home, &task)
                .unwrap_err()
                .message
                .contains("wrong-home")
        );

        fs::remove_file(&coordinator_path).unwrap();
        assert!(
            validate_destination(&request, &task)
                .unwrap_err()
                .message
                .contains("metadata is unavailable")
        );
        fs::write(&coordinator_path, [0xff]).unwrap();
        assert!(
            validate_destination(&request, &task)
                .unwrap_err()
                .message
                .contains("not UTF-8")
        );
        fs::write(&coordinator_path, "kind=daemon\n").unwrap();
        assert!(validate_destination(&request, &task).is_err());

        let mut worker = TaskRecord::new(
            "new-owner".into(),
            AssignmentRole::Implementer,
            crate::lifecycle::subagent_model::ArtifactKind::Implementation,
            false,
            coordinator.parent_id.clone().unwrap(),
            coordinator.root_id.clone().unwrap(),
            coordinator.owner_home.clone().unwrap(),
        );
        worker.owner_state = coordinator.owner_state.clone();
        worker.persistent_home = coordinator.persistent_home.clone();
        write_record(&coordinator_path, &worker);
        assert!(
            validate_destination(&request, &task)
                .unwrap_err()
                .message
                .contains("not the current coordinator")
        );
        let mut wrong_root = coordinator.clone();
        wrong_root.root_id = Some("root-home:/tmp/other".into());
        write_record(&coordinator_path, &wrong_root);
        assert!(validate_destination(&request, &task).is_err());
        let mut wrong_state = coordinator.clone();
        wrong_state.owner_state = Some("/tmp/elsewhere".into());
        write_record(&coordinator_path, &wrong_state);
        assert!(validate_destination(&request, &task).is_err());
        let mut wrong_owner = coordinator.clone();
        wrong_owner.owner_home = Some("/tmp/elsewhere".into());
        write_record(&coordinator_path, &wrong_owner);
        assert!(validate_destination(&request, &task).is_err());
        let mut wrong_runtime = coordinator.clone();
        wrong_runtime.persistent_home = Some("/tmp/elsewhere".into());
        write_record(&coordinator_path, &wrong_runtime);
        assert!(validate_destination(&request, &task).is_err());

        let root_id = task.root_id.clone().unwrap();
        let mut root_request = request.clone();
        root_request.to_coordinator = root_id;
        root_request.destination_home = PathBuf::from("/tmp/wrong-root");
        root_request.destination_state = PathBuf::from("/tmp/wrong-root-state");
        root_request.destination_coordinator_owner_home = PathBuf::from("/tmp/wrong-root");
        root_request.destination_coordinator_owner_state = PathBuf::from("/tmp/wrong-root-state");
        assert!(
            validate_destination(&root_request, &task)
                .unwrap_err()
                .message
                .contains("root route")
        );
    }

    #[test]
    fn transfer_recovery_preserves_journal_across_reconcile_and_commit_faults() {
        let (_temp, request, meta, _old_attempt) = transfer_fixture();
        let error = transfer_task(&request, |_| Err("provider unavailable".into())).unwrap_err();
        assert!(error.message.contains("reconciliation failed"));
        assert!(transfer_journal_path(&request).exists());

        fs::remove_file(&meta).unwrap();
        assert!(
            transfer_task(&request, |_| panic!(
                "retained retry must read authority first"
            ))
            .unwrap_err()
            .message
            .contains("authority is unavailable")
        );

        let (_temp, mut overflow, _meta, _old_attempt) = transfer_fixture();
        overflow.scope_change.as_mut().unwrap().expected_revision = u64::MAX;
        assert!(
            transfer_task(&overflow, |_| Ok(ExecutionDisposition::Absent))
                .unwrap_err()
                .message
                .contains("revision exhausted")
        );

        let (_temp, request, meta, _old_attempt) = transfer_fixture();
        let path = transfer_journal_path(&request);
        let bytes = fs::read(&meta).unwrap();
        let task = read_meta("task-a", std::str::from_utf8(&bytes).unwrap()).unwrap();
        let brief_path = transferred_brief(&task, request.scope_change.as_ref().unwrap())
            .unwrap()
            .0;
        fs::create_dir_all(brief_path.parent().unwrap()).unwrap();
        fs::write(&brief_path, "conflicting retained brief\n").unwrap();
        let journal = TransferJournal {
            version: 1,
            request: request.clone(),
            original_digest: transfer_digest(&bytes),
            original_attempt_id: task.attempt.unwrap().id,
            stage: TransferStage::SourceFenced,
            accepted_generation: None,
        };
        publish_transfer_journal(&path, &journal).unwrap();
        assert!(
            transfer_task(&request, |_| panic!("source is already fenced"))
                .unwrap_err()
                .message
                .contains("brief revision conflicts")
        );

        let (_temp, request, _meta, _old_attempt) = transfer_fixture();
        let path = transfer_journal_path(&request);
        let (bytes, task) = read_transfer_meta(&request).unwrap();
        let journal = TransferJournal {
            version: 1,
            request: request.clone(),
            original_digest: transfer_digest(&bytes),
            original_attempt_id: task.attempt.unwrap().id,
            stage: TransferStage::Committed,
            accepted_generation: Some(2),
        };
        publish_transfer_journal(&path, &journal).unwrap();
        assert!(
            transfer_task(&request, |_| panic!("committed journal never reconciles"))
                .unwrap_err()
                .message
                .contains("committed ownership transfer record is unavailable")
        );
    }

    #[test]
    fn transfer_refuses_competing_authority_and_late_stale_generation_without_reconcile() {
        let (_temp, request, _meta, _old_attempt) = transfer_fixture();
        fs::write(
            request.destination_state.join("task-a.meta"),
            "competing authority\n",
        )
        .unwrap();
        assert!(
            transfer_task(&request, |_| panic!(
                "competing authority must win before reconcile"
            ))
            .unwrap_err()
            .message
            .contains("destination already has a different task authority")
        );

        let (_temp, mut stale_generation, _meta, _old_attempt) = transfer_fixture();
        stale_generation.expected_attempt_generation = 2;
        assert!(
            transfer_task(&stale_generation, |_| panic!(
                "stale generation must not reconcile"
            ))
            .unwrap_err()
            .message
            .contains("stale ownership transfer generation or owner")
        );

        let (_temp, mut stale_owner, _meta, _old_attempt) = transfer_fixture();
        stale_owner.from_coordinator = crate::lifecycle::subagent_model::qualified_task_id(
            stale_owner
                .destination_coordinator_owner_home
                .to_str()
                .unwrap(),
            "another-owner",
        );
        assert!(
            transfer_task(&stale_owner, |_| panic!("stale owner must not reconcile"))
                .unwrap_err()
                .message
                .contains("stale ownership transfer generation or owner")
        );

        let (_temp, request, _meta, _old_attempt) = transfer_fixture();
        fs::write(transfer_journal_path(&request), "not-json\n").unwrap();
        assert!(
            transfer_task(&request, |_| panic!("malformed journal must not reconcile"))
                .unwrap_err()
                .message
                .contains("journal is malformed")
        );
    }

    #[test]
    fn transferred_brief_preserves_role_artifact_and_historical_source_context() {
        let (_temp, request, meta, _old_attempt) = transfer_fixture();
        let base = read_meta("task-a", &fs::read_to_string(meta).unwrap()).unwrap();
        let change = request.scope_change.as_ref().unwrap();
        for (role, artifact, role_text, artifact_text) in [
            (
                AssignmentRole::Researcher,
                crate::lifecycle::subagent_model::ArtifactKind::Report,
                "researcher",
                "report",
            ),
            (
                AssignmentRole::Reviewer,
                crate::lifecycle::subagent_model::ArtifactKind::Report,
                "reviewer",
                "report",
            ),
            (
                AssignmentRole::SubOrchestrator,
                crate::lifecycle::subagent_model::ArtifactKind::Coordination,
                "sub-orchestrator",
                "coordination",
            ),
        ] {
            let mut task = base.clone();
            task.role = role;
            task.artifact = artifact;
            task.accepted_brief_path = Some("prior/accepted-brief.md".into());
            let (_, bytes) = transferred_brief(&task, change).unwrap();
            let rendered = String::from_utf8(bytes).unwrap();
            assert!(rendered.contains(&format!("Assignment: {role_text}")));
            assert!(rendered.contains(&format!("Requested artifact: {artifact_text}")));
            assert!(rendered.contains("Previous accepted brief: prior/accepted-brief.md"));
            assert!(rendered.contains("- accepted"));
            assert!(rendered.contains("- research.md"));
        }
    }

    #[test]
    fn canonical_transfer_authority_reader_distinguishes_missing_encoding_and_model_failures() {
        let (_temp, request, meta, _old_attempt) = transfer_fixture();
        fs::remove_file(&meta).unwrap();
        assert!(
            read_transfer_meta(&request)
                .unwrap_err()
                .message
                .contains("cannot read canonical task metadata")
        );
        fs::write(&meta, [0xff]).unwrap();
        assert!(
            read_transfer_meta(&request)
                .unwrap_err()
                .message
                .contains("not UTF-8")
        );
        fs::write(&meta, "schema_version=2\ncanonical_model={}\n").unwrap();
        assert!(
            read_transfer_meta(&request)
                .unwrap_err()
                .message
                .contains("invalid canonical task metadata")
        );

        let mut no_task_destination = request;
        no_task_destination.to_coordinator = "root-home:/tmp/not-this-root".into();
        let valid_task = TaskRecord::new(
            "task-a".into(),
            AssignmentRole::Implementer,
            crate::lifecycle::subagent_model::ArtifactKind::Implementation,
            false,
            "parent".into(),
            "root-home:/tmp/root".into(),
            "/tmp/owner".into(),
        );
        assert!(
            validate_destination(&no_task_destination, &valid_task)
                .unwrap_err()
                .message
                .contains("identity has no task")
        );
    }

    #[test]
    fn destination_ancestry_is_bounded_and_every_unreadable_link_fails_closed() {
        fn records(request: &TransferRequest) -> (PathBuf, TaskRecord, TaskRecord) {
            let meta = request.authoritative_state.join("task-a.meta");
            let task = read_meta("task-a", &fs::read_to_string(meta).unwrap()).unwrap();
            let coordinator_path = request
                .destination_coordinator_owner_state
                .join("new-owner.meta");
            let coordinator =
                read_meta("new-owner", &fs::read_to_string(&coordinator_path).unwrap()).unwrap();
            (coordinator_path, task, coordinator)
        }

        let (_temp, mut request, _meta, _old_attempt) = transfer_fixture();
        let (coordinator_path, task, mut coordinator) = records(&request);
        coordinator.persistent_home = None;
        request.destination_home = request.destination_coordinator_owner_home.clone();
        request.destination_state = request.destination_coordinator_owner_state.clone();
        write_record(&coordinator_path, &coordinator);
        validate_destination(&request, &task).expect("owner home is coordinator runtime fallback");

        let (_temp, request, _meta, _old_attempt) = transfer_fixture();
        let (coordinator_path, task, mut coordinator) = records(&request);
        coordinator.parent_id = Some("unrecorded-root-child".into());
        write_record(&coordinator_path, &coordinator);
        validate_destination(&request, &task)
            .expect("an absent root-owned parent record terminates lineage at the root boundary");

        for contents in [None, Some(vec![0xff]), Some(b"not-metadata\n".to_vec())] {
            let (_temp, request, _meta, _old_attempt) = transfer_fixture();
            let (coordinator_path, task, mut coordinator) = records(&request);
            let ancestry = request.authoritative_state.join("nested-ancestry");
            fs::create_dir(&ancestry).unwrap();
            coordinator.parent_id = Some("ancestor".into());
            coordinator.parent_home = Some("/tmp/separate-domain".into());
            coordinator.parent_state = Some(ancestry.to_string_lossy().into_owned());
            write_record(&coordinator_path, &coordinator);
            if let Some(contents) = contents {
                fs::write(ancestry.join("ancestor.meta"), contents).unwrap();
            }
            let message = validate_destination(&request, &task).unwrap_err().message;
            assert!(
                message.contains("ancestry is unavailable")
                    || message.contains("ancestry is not UTF-8")
                    || message.contains("ancestry is invalid"),
                "{message}"
            );
        }

        let (_temp, request, _meta, _old_attempt) = transfer_fixture();
        let (coordinator_path, task, mut coordinator) = records(&request);
        coordinator.parent_id = Some("ancestor".into());
        coordinator.parent_state = Some(request.authoritative_state.to_string_lossy().into_owned());
        write_record(&coordinator_path, &coordinator);
        let mut ancestor = TaskRecord::new(
            "ancestor".into(),
            AssignmentRole::SubOrchestrator,
            crate::lifecycle::subagent_model::ArtifactKind::Coordination,
            true,
            "new-owner".into(),
            task.root_id.clone().unwrap(),
            request
                .destination_coordinator_owner_home
                .to_string_lossy()
                .into_owned(),
        );
        ancestor.owner_state = Some(request.authoritative_state.to_string_lossy().into_owned());
        ancestor.parent_home = coordinator.owner_home.clone();
        ancestor.parent_state = Some(request.authoritative_state.to_string_lossy().into_owned());
        ancestor.persistent_home = Some(request.destination_home.to_string_lossy().into_owned());
        write_record(
            &request.authoritative_state.join("ancestor.meta"),
            &ancestor,
        );
        assert!(
            validate_destination(&request, &task)
                .unwrap_err()
                .message
                .contains("lineage already contains a cycle")
        );
    }
}

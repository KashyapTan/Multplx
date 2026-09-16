//! Durable parent-channel relay for accepted task outcomes.
//!
//! The channel composes the canonical [`MessageEnvelope`] with one immutable
//! inbox record and one sender-owned delivery receipt per hop.  It does not
//! create another task or conversation authority: each hop is resolved from
//! the current task's recorded parent identity and the original event remains
//! byte-for-byte stable all the way to the root projection.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use multplx_core::filesystem::{atomic_replace, read_bounded_regular};
use multplx_core::identifiers::TaskId;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::{ProcessProbe, SystemProcessProbe};
use serde::{Deserialize, Serialize};

use super::subagent_model::{Acknowledgement, MessageEnvelope, TaskRecord, read_meta};

pub const SCHEMA_VERSION: u32 = 1;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_INSPECT_RECORDS: usize = 4096;
const MAX_ACTIVE_RECORDS: usize = 4096;
const MAX_SWEEP_RECORDS: usize = MAX_ACTIVE_RECORDS * 2;
const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum DeliveryState {
    Pending,
    Delivered,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RouteBinding {
    pub sender_id: String,
    pub sender_home: String,
    pub sender_state: String,
    pub recipient_id: String,
    pub recipient_home: String,
    pub recipient_state: String,
    pub root_id: String,
    pub attempt_id: String,
    pub attempt_generation: u64,
    pub brief_revision: u64,
    pub scope_revision: Option<u64>,
    pub assignment_generation: u64,
    pub recipient_attempt_id: Option<String>,
    pub recipient_attempt_generation: Option<u64>,
    pub recipient_scope_revision: Option<u64>,
    pub recipient_assignment_generation: Option<u64>,
    pub recipient_domain_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HopReceipt {
    pub sender_id: String,
    pub sender_state: String,
    pub recipient_id: String,
    pub recipient_state: String,
    pub delivered_epoch: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RouteRepair {
    pub prior_route: RouteBinding,
    pub repaired_epoch: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ParentOutcome {
    pub schema_version: u32,
    /// The accepted task fact.  This value never changes between hops.
    pub event: MessageEnvelope,
    pub route: RouteBinding,
    pub hops: Vec<HopReceipt>,
    #[serde(default)]
    pub route_repairs: Vec<RouteRepair>,
    pub created_epoch: u64,
    pub delivery: DeliveryState,
    pub delivered_epoch: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ParentChannelHealth {
    pub pending_inbox: usize,
    pub pending_outbox: usize,
    pub delivered_outbox: usize,
    pub archived_inbox: u64,
    pub oldest_pending_epoch: Option<u64>,
    pub relayed_this_tick: usize,
    pub last_progress_epoch: Option<u64>,
    pub observed_epoch: u64,
    pub last_error: Option<String>,
    pub records_observed: usize,
    pub scan_truncated: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RelaySweep {
    schema_version: u32,
    remaining: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArchiveCounters {
    inbox: u64,
    outbox: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum HumanAnswerDisposition {
    Accepted,
    Historical,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HumanAnswerRecord {
    pub schema_version: u32,
    pub answer_id: String,
    pub task_id: String,
    pub question_id: String,
    pub brief_revision: u64,
    pub workflow_revision: Option<String>,
    pub answer: String,
    pub recorded_epoch: u64,
    pub disposition: HumanAnswerDisposition,
    pub reason: Option<String>,
}

/// Bind a human question to the same canonical task/brief transition that
/// publishes its `needs-decision` outcome. Repeating the exact question is
/// harmless; reusing its id for another revision or question is rejected.
pub fn bind_human_question(
    task: &mut TaskRecord,
    question_id: &str,
    brief_revision: u64,
    workflow_revision: Option<&str>,
    question: &str,
) -> Result<super::subagent_model::HumanDecision, String> {
    if question_id.is_empty()
        || question.is_empty()
        || question.contains(['\n', '\r'])
        || task.accepted_brief_revision != Some(brief_revision)
    {
        return Err("human question requires the exact accepted brief and one-line text".into());
    }
    if let Some(existing) = task
        .schedule
        .decisions
        .iter()
        .find(|decision| decision.id == question_id)
    {
        if existing.task_id != task.task_id
            || existing.brief_revision != brief_revision
            || existing.workflow_revision.as_deref() != workflow_revision
            || existing.question != question
        {
            return Err("human question identity conflicts with retained history".into());
        }
        return Ok(existing.clone());
    }
    let decision = super::subagent_model::HumanDecision {
        id: question_id.to_owned(),
        task_id: task.task_id.clone(),
        brief_revision,
        workflow_revision: workflow_revision.map(str::to_owned),
        question: question.to_owned(),
        answer: None,
    };
    task.schedule.decisions.push(decision.clone());
    task.schedule.state = super::subagent_model::WorkState::WaitingHuman;
    task.schedule.waiting_condition = Some(question_id.to_owned());
    Ok(decision)
}

fn now_epoch() -> u64 {
    std::env::var("MX_PARENT_CHANNEL_NOW")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
}

fn resolved(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| {
        format!(
            "parent channel path {} is unavailable: {error}",
            path.display()
        )
    })
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (resolved(left), resolved(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn outbox(state: &Path) -> PathBuf {
    state.join("parent-outbox")
}

fn inbox(state: &Path) -> PathBuf {
    state.join("parent-inbox")
}

fn record_path(directory: &Path, message_id: &str) -> Result<PathBuf, String> {
    TaskId::parse(message_id.to_owned()).map_err(|error| error.to_string())?;
    Ok(directory.join(format!("{message_id}.json")))
}

fn inbox_record_path(directory: &Path, outcome: &ParentOutcome) -> Result<PathBuf, String> {
    let id = format!(
        "{}-hop-{}-repair-{}",
        outcome.event.message_id,
        outcome.hops.len(),
        outcome.route_repairs.len()
    );
    record_path(directory, &id)
}

fn read_outcome(path: &Path) -> Result<ParentOutcome, String> {
    let record: ParentOutcome = serde_json::from_slice(
        &read_bounded_regular(path, MAX_RECORD_BYTES).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("invalid parent-channel record {}: {error}", path.display()))?;
    if record.schema_version != SCHEMA_VERSION
        || record.event.message_id.is_empty()
        || record.route.sender_id.is_empty()
        || record.route.recipient_id.is_empty()
        || record.route.root_id.is_empty()
        || record.route.attempt_generation == 0
        || record.route.assignment_generation == 0
        || record.route.brief_revision == 0
        || record.route.recipient_attempt_id.is_some()
            != record.route.recipient_attempt_generation.is_some()
        || record.route_repairs.len() > 64
    {
        return Err(format!(
            "invalid parent-channel identity in {}",
            path.display()
        ));
    }
    Ok(record)
}

fn same_outcome_identity(existing: ParentOutcome, record: &ParentOutcome) -> bool {
    let mut left = existing;
    let mut right = record.clone();
    // The sender-owned delivery receipt can advance after the immutable
    // event and route were accepted.
    left.delivery = DeliveryState::Pending;
    left.delivered_epoch = None;
    left.created_epoch = 0;
    for hop in &mut left.hops {
        hop.delivered_epoch = 0;
    }
    right.delivery = DeliveryState::Pending;
    right.delivered_epoch = None;
    right.created_epoch = 0;
    for hop in &mut right.hops {
        hop.delivered_epoch = 0;
    }
    for repair in &mut left.route_repairs {
        repair.repaired_epoch = 0;
    }
    for repair in &mut right.route_repairs {
        repair.repaired_epoch = 0;
    }
    left == right
}

fn archived_receipt_path(path: &Path) -> Option<PathBuf> {
    let directory = path.parent()?;
    let direction = directory.file_name()?.to_str()?;
    if !matches!(direction, "parent-inbox" | "parent-outbox") {
        return None;
    }
    Some(directory.parent()?.join("parent-receipts").join(format!(
        "{direction}-{}",
        path.file_name()?.to_string_lossy()
    )))
}

fn ensure_active_capacity(parent: &Path, limit: usize) -> Result<(), String> {
    let entries = fs::read_dir(parent)
        .map_err(|error| error.to_string())?
        .take(limit + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if entries
        .iter()
        .any(|entry| entry.path().extension().and_then(|value| value.to_str()) != Some("json"))
    {
        return Err("parent channel active queue contains a foreign entry".into());
    }
    if entries.len() >= limit {
        return Err(format!(
            "parent channel active queue reached its {limit}-record bound"
        ));
    }
    Ok(())
}

fn write_immutable(path: &Path, record: &ParentOutcome) -> Result<(), String> {
    if path.is_file() {
        if !same_outcome_identity(read_outcome(path)?, record) {
            return Err("parent-channel event identity conflicts with durable record".into());
        }
        return Ok(());
    }
    if let Some(receipt) = archived_receipt_path(path)
        && receipt.is_file()
    {
        let existing = read_outcome(&receipt)?;
        if existing.delivery != DeliveryState::Delivered || !same_outcome_identity(existing, record)
        {
            return Err("parent-channel event identity conflicts with durable receipt".into());
        }
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or("parent-channel record has no directory")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    if matches!(
        parent.file_name().and_then(|value| value.to_str()),
        Some("parent-inbox" | "parent-outbox")
    ) {
        ensure_active_capacity(parent, MAX_ACTIVE_RECORDS)?;
    }
    atomic_replace(
        path,
        &serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?,
        0o600,
    )
    .map_err(|error| error.to_string())
}

fn rewrite(path: &Path, record: &ParentOutcome) -> Result<(), String> {
    atomic_replace(
        path,
        &serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?,
        0o600,
    )
    .map_err(|error| error.to_string())
}

fn archive_delivered(path: &Path, record: &ParentOutcome) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or("parent-channel record has no directory")?;
    let state = directory
        .parent()
        .ok_or("parent-channel record has no state")?;
    let direction = directory
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("parent-channel directory is not UTF-8")?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("parent-channel record name is not UTF-8")?;
    let receipt = state
        .join("parent-receipts")
        .join(format!("{direction}-{name}"));
    fs::create_dir_all(receipt.parent().expect("parent receipt directory"))
        .map_err(|error| error.to_string())?;
    if receipt.is_file() && !same_outcome_identity(read_outcome(&receipt)?, record) {
        return Err("parent-channel receipt identity conflicts with delivery".into());
    }
    let operation = format!(
        "parent-archive-{}-{}",
        if direction == "parent-inbox" {
            "in"
        } else {
            "out"
        },
        path.file_stem()
            .and_then(|value| value.to_str())
            .ok_or("parent-channel record stem is not UTF-8")?
    );
    let writes = if state
        .join(".transitions")
        .join(format!("{operation}.json"))
        .is_file()
    {
        multplx_core::filesystem::read_transition_writes(state, &operation)
            .map_err(|error| error.to_string())?
    } else {
        let counter_path = state.join("parent-channel-counters.json");
        let counter_before = fs::read(&counter_path).ok();
        let mut counters = counter_before
            .as_deref()
            .and_then(|bytes| serde_json::from_slice::<ArchiveCounters>(bytes).ok())
            .unwrap_or_default();
        if direction == "parent-inbox" {
            counters.inbox = counters.inbox.saturating_add(1);
        } else {
            counters.outbox = counters.outbox.saturating_add(1);
        }
        vec![
            multplx_core::filesystem::TransitionWrite {
                path: receipt
                    .strip_prefix(state)
                    .map_err(|_| "parent receipt left state")?
                    .to_owned(),
                before: fs::read(&receipt).ok(),
                after: serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?,
            },
            multplx_core::filesystem::TransitionWrite {
                path: "parent-channel-counters.json".into(),
                before: counter_before,
                after: serde_json::to_vec_pretty(&counters).map_err(|error| error.to_string())?,
            },
        ]
    };
    multplx_core::filesystem::recoverable_transition(state, &operation, &writes, None)
        .map_err(|error| error.to_string())?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn task_record(state: &Path, id: &str) -> Result<TaskRecord, String> {
    TaskId::parse(id.to_owned()).map_err(|error| error.to_string())?;
    let bytes = read_bounded_regular(state.join(format!("{id}.meta")), 4 * 1024 * 1024)
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| "task metadata is not UTF-8")?;
    let record = read_meta(id, &text)?;
    if record.legacy_unknown {
        return Err("legacy task identity cannot use the durable parent channel".into());
    }
    Ok(record)
}

fn record_state(record: &TaskRecord) -> Result<PathBuf, String> {
    record
        .owner_state
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| {
            record
                .owner_home
                .as_deref()
                .map(|home| Path::new(home).join("state"))
        })
        .ok_or_else(|| "task has no recorded owner state".into())
}

fn route_for(record: &TaskRecord, state: &Path) -> Result<RouteBinding, String> {
    record.validate()?;
    let owner_state = record_state(record)?;
    if !same_path(&owner_state, state) {
        return Err("parent-channel source is outside its recorded owner state".into());
    }
    let attempt = record
        .attempt
        .as_ref()
        .ok_or("task has no current attempt")?;
    let assignment_generation = record
        .domain
        .as_ref()
        .map(|domain| domain.assignment_generation)
        .or_else(|| {
            record
                .assignments
                .last()
                .map(|assignment| assignment.generation)
        })
        .ok_or("task has no assignment generation")?;
    let recipient_id = record
        .parent_id
        .clone()
        .ok_or("task has no recorded parent identity")?;
    let recipient_home = record
        .parent_home
        .clone()
        .ok_or("task has no recorded parent home")?;
    let recipient_state = record.parent_state.clone().unwrap_or_else(|| {
        Path::new(&recipient_home)
            .join("state")
            .to_string_lossy()
            .into_owned()
    });
    let root_id = record.root_id.clone().ok_or("task has no root identity")?;
    let recipient = if recipient_id == root_id {
        None
    } else {
        Some(task_record(Path::new(&recipient_state), &recipient_id)?)
    };
    let recipient_assignment_generation = recipient.as_ref().and_then(|parent| {
        parent
            .domain
            .as_ref()
            .map(|domain| domain.assignment_generation)
            .or_else(|| {
                parent
                    .assignments
                    .last()
                    .map(|assignment| assignment.generation)
            })
    });
    Ok(RouteBinding {
        sender_id: record.task_id.clone(),
        sender_home: record
            .owner_home
            .clone()
            .ok_or("task has no recorded owner home")?,
        sender_state: resolved(state)?.to_string_lossy().into_owned(),
        recipient_id,
        recipient_home,
        recipient_state,
        root_id,
        attempt_id: attempt.id.clone(),
        attempt_generation: attempt.generation,
        brief_revision: attempt.brief_revision,
        scope_revision: record.domain.as_ref().map(|domain| domain.scope_revision),
        assignment_generation,
        recipient_attempt_id: recipient
            .as_ref()
            .and_then(|parent| parent.attempt.as_ref().map(|attempt| attempt.id.clone())),
        recipient_attempt_generation: recipient
            .as_ref()
            .and_then(|parent| parent.attempt.as_ref().map(|attempt| attempt.generation)),
        recipient_scope_revision: recipient
            .as_ref()
            .and_then(|parent| parent.domain.as_ref().map(|domain| domain.scope_revision)),
        recipient_assignment_generation,
        recipient_domain_id: recipient.as_ref().and_then(|parent| {
            parent
                .domain
                .as_ref()
                .map(|domain| domain.domain_id.clone())
        }),
    })
}

fn validate_route_target(route: &RouteBinding) -> Result<PathBuf, String> {
    let target = PathBuf::from(&route.recipient_state);
    let target = resolved(&target)?;
    let expected_home = resolved(Path::new(&route.recipient_home))?;
    if route.recipient_id == route.root_id {
        let root_home = route
            .root_id
            .strip_prefix("root-home:")
            .map(PathBuf::from)
            .ok_or("root identity does not contain a validated home")?;
        if resolved(&root_home)? != expected_home
            || (!target.starts_with(&expected_home)
                && !state_has_live_home_owner(&target, &expected_home))
        {
            return Err("root parent route conflicts with its recorded home/state".into());
        }
        return Ok(target);
    }
    let parent = task_record(&target, &route.recipient_id)?;
    if parent.owner_home.as_deref() != Some(route.recipient_home.as_str())
        || !same_path(&record_state(&parent)?, &target)
        || parent.root_id.as_deref() != Some(route.root_id.as_str())
        || parent.attempt.as_ref().map(|attempt| attempt.id.as_str())
            != route.recipient_attempt_id.as_deref()
        || parent.attempt.as_ref().map(|attempt| attempt.generation)
            != route.recipient_attempt_generation
        || parent.domain.as_ref().map(|domain| domain.scope_revision)
            != route.recipient_scope_revision
        || parent
            .domain
            .as_ref()
            .map(|domain| domain.assignment_generation)
            .or_else(|| {
                parent
                    .assignments
                    .last()
                    .map(|assignment| assignment.generation)
            })
            != route.recipient_assignment_generation
        || parent
            .domain
            .as_ref()
            .map(|domain| domain.domain_id.as_str())
            != route.recipient_domain_id.as_deref()
    {
        return Err("parent route conflicts with validated task identity".into());
    }
    Ok(target)
}

/// An explicit `MX_STATE_OVERRIDE` may live outside the root home.  In that
/// case the root watcher's live directory-lock identity is the authority that
/// binds the state directory back to the frozen root home.  Merely finding an
/// existing directory (or a copied lock) is not sufficient.
fn state_has_live_home_owner(state: &Path, expected_home: &Path) -> bool {
    let lock = state.join(".watch.lock");
    let owner = match fs::symlink_metadata(&lock) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let Ok(target) = fs::read_link(&lock) else {
                return false;
            };
            let target = if target.is_absolute() {
                target
            } else {
                state.join(target)
            };
            let Ok(resolved_owner) = resolved(&target) else {
                return false;
            };
            if !resolved_owner.starts_with(state) {
                return false;
            }
            resolved_owner
        }
        Ok(metadata) if metadata.is_dir() => {
            let Ok(owner) = resolved(&lock) else {
                return false;
            };
            owner
        }
        _ => return false,
    };
    let read = |name: &str| -> Option<String> {
        let bytes = read_bounded_regular(owner.join(name), 64 * 1024).ok()?;
        String::from_utf8(bytes)
            .ok()
            .map(|value| value.trim_end_matches('\n').to_owned())
    };
    let recorded_home = read("mx-home").and_then(|value| resolved(Path::new(&value)).ok());
    let pid = read("pid").and_then(|value| value.parse::<u32>().ok());
    let recorded_identity = read("pid-identity");
    let processes = SystemProcessProbe::default();
    recorded_home.as_deref() == Some(expected_home)
        && pid.is_some_and(|pid| {
            processes.is_alive(pid)
                && processes
                    .identity(pid)
                    .ok()
                    .is_some_and(|identity| Some(identity.marker) == recorded_identity)
        })
}

fn repair_replaced_recipient(outcome: &mut ParentOutcome) -> Result<PathBuf, String> {
    if outcome.route.recipient_id == outcome.route.root_id {
        return validate_route_target(&outcome.route);
    }
    let target = resolved(Path::new(&outcome.route.recipient_state))?;
    let parent = task_record(&target, &outcome.route.recipient_id)?;
    let parent_attempt = parent
        .attempt
        .as_ref()
        .ok_or("replacement parent has no current attempt")?;
    let parent_assignment = parent
        .domain
        .as_ref()
        .map(|domain| domain.assignment_generation)
        .or_else(|| {
            parent
                .assignments
                .last()
                .map(|assignment| assignment.generation)
        })
        .ok_or("replacement parent has no assignment generation")?;
    let stable_identity = parent.owner_home.as_deref()
        == Some(outcome.route.recipient_home.as_str())
        && same_path(&record_state(&parent)?, &target)
        && parent.root_id.as_deref() == Some(outcome.route.root_id.as_str())
        && parent
            .domain
            .as_ref()
            .map(|domain| domain.domain_id.as_str())
            == outcome.route.recipient_domain_id.as_deref();
    let newer = parent_attempt.generation
        > outcome
            .route
            .recipient_attempt_generation
            .unwrap_or_default()
        || parent_assignment
            > outcome
                .route
                .recipient_assignment_generation
                .unwrap_or_default();
    if !stable_identity || !newer {
        return Err("parent route conflicts with validated task identity".into());
    }
    if outcome.route_repairs.len() >= 64 {
        return Err("parent outcome exceeded bounded route repair history".into());
    }
    outcome.route_repairs.push(RouteRepair {
        prior_route: outcome.route.clone(),
        repaired_epoch: now_epoch(),
    });
    outcome.route.recipient_attempt_id = Some(parent_attempt.id.clone());
    outcome.route.recipient_attempt_generation = Some(parent_attempt.generation);
    outcome.route.recipient_scope_revision =
        parent.domain.as_ref().map(|domain| domain.scope_revision);
    outcome.route.recipient_assignment_generation = Some(parent_assignment);
    validate_route_target(&outcome.route)
}

/// Validate an outcome against the current task and freeze its route beside
/// the canonical transition intent.  This must happen before committing the
/// transition so crash recovery never has to reinterpret an old event using a
/// replacement attempt or parent generation.
pub fn prepare_outcome(state: &Path, event: &MessageEnvelope) -> Result<ParentOutcome, String> {
    let state = resolved(state)?;
    let record = task_record(&state, &event.task_id)?;
    let recipient = record.parent_id.clone().unwrap_or_default();
    event.validate_current(&record, &record.task_id, &recipient)?;
    if event.acknowledgement != Acknowledgement::Pending {
        return Err("new parent outcome must start pending".into());
    }
    let route = route_for(&record, &state)?;
    Ok(ParentOutcome {
        schema_version: SCHEMA_VERSION,
        event: event.clone(),
        route,
        hops: Vec::new(),
        route_repairs: Vec::new(),
        created_epoch: now_epoch(),
        delivery: DeliveryState::Pending,
        delivered_epoch: None,
    })
}

fn validate_prepared(state: &Path, outcome: &ParentOutcome) -> Result<(), String> {
    if outcome.schema_version != SCHEMA_VERSION
        || outcome.event.message_id.is_empty()
        || outcome.event.task_id != outcome.route.sender_id
        || outcome.event.sender != outcome.route.sender_id
        || outcome.event.task_home.as_deref() != Some(outcome.route.sender_home.as_str())
        || outcome.event.parent_home.as_deref() != Some(outcome.route.recipient_home.as_str())
        || outcome.event.recipient != outcome.route.recipient_id
        || outcome.event.parent_id.as_deref() != Some(outcome.route.recipient_id.as_str())
        || outcome
            .event
            .attempt
            .as_ref()
            .map(|attempt| attempt.id.as_str())
            != Some(outcome.route.attempt_id.as_str())
        || outcome
            .event
            .attempt
            .as_ref()
            .map(|attempt| attempt.generation)
            != Some(outcome.route.attempt_generation)
        || outcome.event.brief_revision != Some(outcome.route.brief_revision)
        || outcome.delivery != DeliveryState::Pending
        || outcome.delivered_epoch.is_some()
        || !same_path(Path::new(&outcome.route.sender_state), state)
    {
        return Err("prepared parent outcome has invalid frozen identity".into());
    }
    Ok(())
}

/// Persist a route frozen in the same canonical transition as its accepted
/// fact.  This recovery API deliberately does not require the event to remain
/// current after a replacement; the original validation is carried by the
/// retained transition evidence.
pub fn persist_prepared(state: &Path, outcome: &ParentOutcome) -> Result<(), String> {
    let state = resolved(state)?;
    validate_prepared(&state, outcome)?;
    let _lock = DirectoryLock::acquire_wait(
        state.join(".parent-channel.lock"),
        &SystemProcessProbe::default(),
        LOCK_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    write_immutable(
        &record_path(&outbox(&state), &outcome.event.message_id)?,
        outcome,
    )
}

/// Record one already-accepted canonical task outcome before notification.
///
/// Phase 06 delivery transitions and Phase 08 workflow/decision transitions
/// call this same API with their canonical message envelope.
pub fn record_outcome(state: &Path, event: &MessageEnvelope) -> Result<(), String> {
    let outcome = prepare_outcome(state, event)?;
    persist_prepared(state, &outcome)
}

pub fn prepare_report(
    state: &Path,
    event: &MessageEnvelope,
) -> Result<Option<ParentOutcome>, String> {
    if matches!(
        event.kind.as_str(),
        "blocked" | "needs-decision" | "done" | "failed" | "resolved"
    ) {
        prepare_outcome(state, event).map(Some)
    } else {
        Ok(None)
    }
}

/// Canonical report integration. Replaceable progress remains local while
/// blockers, questions and terminal outcomes enter the parent channel.
pub fn record_report(state: &Path, event: &MessageEnvelope) -> Result<bool, String> {
    if let Some(outcome) = prepare_report(state, event)? {
        persist_prepared(state, &outcome)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn packet_for_next_hop(incoming: &ParentOutcome, state: &Path) -> Result<ParentOutcome, String> {
    let parent = task_record(state, &incoming.route.recipient_id)?;
    let route = route_for(&parent, state)?;
    if route.root_id != incoming.route.root_id {
        return Err("parent channel changed root identity between hops".into());
    }
    let mut hops = incoming.hops.clone();
    hops.push(HopReceipt {
        sender_id: incoming.route.sender_id.clone(),
        sender_state: incoming.route.sender_state.clone(),
        recipient_id: incoming.route.recipient_id.clone(),
        recipient_state: incoming.route.recipient_state.clone(),
        delivered_epoch: now_epoch(),
    });
    Ok(ParentOutcome {
        schema_version: SCHEMA_VERSION,
        event: incoming.event.clone(),
        route,
        hops,
        route_repairs: incoming.route_repairs.clone(),
        created_epoch: incoming.created_epoch,
        delivery: DeliveryState::Pending,
        delivered_epoch: None,
    })
}

fn promote_inbox_record(state: &Path, path: &Path) -> Result<bool, String> {
    let mut incoming = read_outcome(path)?;
    if incoming.delivery == DeliveryState::Delivered {
        archive_delivered(path, &incoming)?;
        return Ok(false);
    }
    if !same_path(Path::new(&incoming.route.recipient_state), state) {
        return Err("foreign parent inbox record retained".into());
    }
    if validate_route_target(&incoming.route).is_err() {
        repair_replaced_recipient(&mut incoming)?;
        rewrite(path, &incoming)?;
    }
    if incoming.route.recipient_id == incoming.route.root_id {
        let payload = if matches!(
            incoming.event.kind.as_str(),
            "working" | "paused" | "blocked" | "needs-decision" | "done" | "failed" | "resolved"
        ) {
            format!(
                "report: message={} task={} state={} brief={} summary={}",
                incoming.event.message_id,
                incoming.event.task_id,
                incoming.event.kind,
                incoming.event.brief_revision.unwrap_or_default(),
                incoming.event.summary
            )
        } else {
            format!(
                "parent-outcome: message={} task={} kind={} summary={}",
                incoming.event.message_id,
                incoming.event.task_id,
                incoming.event.kind,
                incoming.event.summary
            )
        };
        crate::operational_input::publish_message_wake(
            state,
            &incoming.event,
            &payload,
            SystemTime::now(),
            &SystemProcessProbe::default(),
        )?;
    } else {
        let outgoing = packet_for_next_hop(&incoming, state)?;
        write_immutable(
            &record_path(&outbox(state), &incoming.event.message_id)?,
            &outgoing,
        )?;
    }
    incoming.delivery = DeliveryState::Delivered;
    incoming.delivered_epoch = Some(now_epoch());
    rewrite(path, &incoming)?;
    archive_delivered(path, &incoming)?;
    Ok(true)
}

fn deliver_outbox_record_with_fault(path: &Path, fault_after_inbox: bool) -> Result<bool, String> {
    let mut outgoing = read_outcome(path)?;
    if outgoing.delivery == DeliveryState::Delivered {
        archive_delivered(path, &outgoing)?;
        return Ok(false);
    }
    let target = match validate_route_target(&outgoing.route) {
        Ok(target) => target,
        Err(_) => {
            let target = repair_replaced_recipient(&mut outgoing)?;
            // Persist repaired delivery intent before publishing it. A crash
            // can therefore replay the exact replacement route.
            rewrite(path, &outgoing)?;
            target
        }
    };
    let _ingress_lock = DirectoryLock::acquire_wait(
        target.join(".parent-channel-ingress.lock"),
        &SystemProcessProbe::default(),
        LOCK_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    write_immutable(&inbox_record_path(&inbox(&target), &outgoing)?, &outgoing)?;
    if fault_after_inbox {
        return Err("injected parent-channel fault after inbox publication".into());
    }
    outgoing.delivery = DeliveryState::Delivered;
    outgoing.delivered_epoch = Some(now_epoch());
    rewrite(path, &outgoing)?;
    archive_delivered(path, &outgoing)?;
    Ok(true)
}

fn deliver_outbox_record(path: &Path) -> Result<bool, String> {
    deliver_outbox_record_with_fault(
        path,
        std::env::var("MX_PARENT_CHANNEL_FAULT").as_deref() == Ok("after-inbox"),
    )
}

fn records(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = match fs::read_dir(directory) {
        Ok(entries) => entries
            .take(MAX_ACTIVE_RECORDS + 1)
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.to_string()),
    };
    if paths
        .iter()
        .any(|path| path.extension().and_then(|value| value.to_str()) != Some("json"))
    {
        return Err("parent channel queue contains a foreign entry".into());
    }
    if paths.len() > MAX_ACTIVE_RECORDS {
        return Err(format!(
            "parent channel active queue exceeds its {MAX_ACTIVE_RECORDS}-record bound"
        ));
    }
    paths.sort();
    Ok(paths)
}

fn inspect_unlocked(state: &Path) -> Result<ParentChannelHealth, String> {
    let mut health = ParentChannelHealth {
        observed_epoch: now_epoch(),
        ..ParentChannelHealth::default()
    };
    if let Ok(bytes) = read_bounded_regular(state.join("parent-channel-counters.json"), 64 * 1024)
        && let Ok(counters) = serde_json::from_slice::<ArchiveCounters>(&bytes)
    {
        health.archived_inbox = counters.inbox;
        health.delivered_outbox = usize::try_from(counters.outbox).unwrap_or(usize::MAX);
    }
    let inbox_records = records(&inbox(state))?;
    let outbox_records = records(&outbox(state))?;
    let total = inbox_records.len().saturating_add(outbox_records.len());
    health.scan_truncated = total > MAX_INSPECT_RECORDS;
    for (kind, path) in inbox_records
        .into_iter()
        .map(|path| (true, path))
        .chain(outbox_records.into_iter().map(|path| (false, path)))
        .take(MAX_INSPECT_RECORDS)
    {
        let record = match read_outcome(&path) {
            Ok(record) => record,
            Err(error) => {
                health.records_observed += 1;
                health.last_error = Some(error);
                continue;
            }
        };
        health.records_observed += 1;
        match record.delivery {
            DeliveryState::Pending => {
                if kind {
                    health.pending_inbox += 1;
                } else {
                    health.pending_outbox += 1;
                }
                health.oldest_pending_epoch = Some(
                    health
                        .oldest_pending_epoch
                        .map_or(record.created_epoch, |old| old.min(record.created_epoch)),
                );
            }
            DeliveryState::Delivered if !kind => {
                health.delivered_outbox = health.delivered_outbox.saturating_add(1)
            }
            DeliveryState::Delivered => {}
        }
    }
    Ok(health)
}

/// Inspect retirement-relevant work without consuming this or another home's
/// queue.
pub fn inspect(state: &Path) -> Result<ParentChannelHealth, String> {
    let state = resolved(state)?;
    let _lock = DirectoryLock::acquire_wait(
        state.join(".parent-channel.lock"),
        &SystemProcessProbe::default(),
        LOCK_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    inspect_unlocked(&state)
}

/// Relay a bounded amount of work owned by this home.  A missing parent leaves
/// the sender's outbox pending; it never causes the root to consume a child's
/// queue.
pub fn relay(state: &Path, limit: usize) -> Result<ParentChannelHealth, String> {
    let state = resolved(state)?;
    let _lock = DirectoryLock::acquire_wait(
        state.join(".parent-channel.lock"),
        &SystemProcessProbe::default(),
        LOCK_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    let mut progressed = 0;
    let mut last_error = None;
    let candidates = records(&inbox(&state))?
        .into_iter()
        .map(|path| {
            (
                format!("inbox/{}", path.file_name().unwrap().to_string_lossy()),
                (true, path),
            )
        })
        .chain(records(&outbox(&state))?.into_iter().map(|path| {
            (
                format!("outbox/{}", path.file_name().unwrap().to_string_lossy()),
                (false, path),
            )
        }))
        .collect::<BTreeMap<_, _>>();
    let cursor_path = state.join(".parent-channel-cursor");
    let mut sweep = read_bounded_regular(&cursor_path, MAX_RECORD_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<RelaySweep>(&bytes).ok())
        .filter(|value| {
            value.schema_version == SCHEMA_VERSION && value.remaining.len() <= MAX_SWEEP_RECORDS
        })
        .unwrap_or_default();
    if sweep.remaining.is_empty() {
        sweep.schema_version = SCHEMA_VERSION;
        sweep.remaining = candidates.keys().cloned().collect();
    }
    let take = limit.clamp(1, 1024).min(sweep.remaining.len());
    let selected = sweep.remaining.drain(..take).collect::<Vec<_>>();
    for key in selected {
        let Some((is_inbox, path)) = candidates.get(&key) else {
            continue;
        };
        match read_outcome(path) {
            Ok(record) if record.delivery == DeliveryState::Delivered => {
                match archive_delivered(path, &record) {
                    Ok(()) => progressed += 1,
                    Err(error) => last_error = Some(error),
                }
                continue;
            }
            Ok(_) => {}
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        }
        let result = if *is_inbox {
            promote_inbox_record(&state, path)
        } else {
            deliver_outbox_record(path)
        };
        match result {
            Ok(true) => progressed += 1,
            Ok(false) => {}
            Err(error) => last_error = Some(error),
        }
    }
    atomic_replace(
        &cursor_path,
        &serde_json::to_vec(&sweep).map_err(|error| error.to_string())?,
        0o600,
    )
    .map_err(|error| error.to_string())?;
    let mut health = inspect_unlocked(&state)?;
    health.relayed_this_tick = progressed;
    health.last_error = last_error.or(health.last_error);
    let health_path = state.join("parent-channel-health.json");
    let previous = fs::read(&health_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ParentChannelHealth>(&bytes).ok());
    health.last_progress_epoch = if progressed > 0 {
        Some(now_epoch())
    } else {
        previous.and_then(|value| value.last_progress_epoch)
    };
    atomic_replace(
        health_path,
        &serde_json::to_vec_pretty(&health).map_err(|error| error.to_string())?,
        0o600,
    )
    .map_err(|error| error.to_string())?;
    Ok(health)
}

/// Number of active channel records that prevent coordinator-home retirement.
/// Delivered records remain active until their durable archive transaction has
/// completed; malformed records also fail closed instead of disappearing from
/// the retirement decision.
pub fn pending_outcomes(state: &Path) -> Result<usize, String> {
    let health = inspect(state)?;
    if health.scan_truncated {
        return Err("parent channel exceeds bounded retirement inspection window".into());
    }
    Ok(health.records_observed)
}

fn publish_human_answer_wake(
    state: &Path,
    record: &HumanAnswerRecord,
    answer_path: &Path,
) -> Result<(), String> {
    let key = format!("human-answer-{}", record.answer_id);
    multplx_core::wake::WakeQueue::new(state)
        .append_once(
            multplx_core::wake::WakeKind::Signal,
            &key,
            &format!(
                "human-answer: task={} question={} brief={} answer-record={}",
                record.task_id,
                record.question_id,
                record.brief_revision,
                answer_path.display()
            ),
            SystemTime::now(),
            &SystemProcessProbe::default(),
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Record a human answer even when its target revision is stale.  Only an
/// exact current question mutates the task; delayed or conflicting answers are
/// retained as historical evidence for Phase 08 and the shared projection.
pub fn record_human_answer(
    state: &Path,
    task_id: &str,
    question_id: &str,
    brief_revision: u64,
    workflow_revision: Option<&str>,
    answer_id: &str,
    answer: &str,
) -> Result<HumanAnswerRecord, String> {
    TaskId::parse(task_id.to_owned()).map_err(|error| error.to_string())?;
    TaskId::parse(answer_id.to_owned()).map_err(|error| error.to_string())?;
    if question_id.is_empty() || answer.is_empty() || answer.contains(['\n', '\r']) {
        return Err("human answer requires a question and one-line answer".into());
    }
    let state = resolved(state)?;
    let _lock = DirectoryLock::acquire_wait(
        state.join(format!(".{task_id}.identity.lock")),
        &SystemProcessProbe::default(),
        LOCK_TIMEOUT,
    )
    .map_err(|error| error.to_string())?;
    let answer_path = state
        .join("human-answers")
        .join(task_id)
        .join(format!("{answer_id}.json"));
    let operation = format!("human-answer-{task_id}-{answer_id}");
    if answer_path.is_file() {
        let existing: HumanAnswerRecord = serde_json::from_slice(
            &read_bounded_regular(&answer_path, MAX_RECORD_BYTES)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if existing.schema_version != SCHEMA_VERSION
            || existing.answer_id != answer_id
            || existing.task_id != task_id
            || existing.question_id != question_id
            || existing.brief_revision != brief_revision
            || existing.workflow_revision.as_deref() != workflow_revision
            || existing.answer != answer
        {
            return Err("human answer identity conflicts with retained history".into());
        }
        let receipt = state.join(".transitions").join(format!("{operation}.json"));
        if receipt.is_file() {
            let committed = serde_json::from_slice::<serde_json::Value>(
                &read_bounded_regular(&receipt, 16 * 1024 * 1024)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?["committed"]
                == true;
            if !committed {
                let writes = multplx_core::filesystem::read_transition_writes(&state, &operation)
                    .map_err(|error| error.to_string())?;
                multplx_core::filesystem::recoverable_transition(&state, &operation, &writes, None)
                    .map_err(|error| error.to_string())?;
            }
        } else if existing.disposition == HumanAnswerDisposition::Accepted {
            return Err("accepted human answer is missing its canonical transition receipt".into());
        }
        if existing.disposition == HumanAnswerDisposition::Accepted {
            publish_human_answer_wake(&state, &existing, &answer_path)?;
        }
        return Ok(existing);
    }
    let meta_path = state.join(format!("{task_id}.meta"));
    let before =
        read_bounded_regular(&meta_path, 4 * 1024 * 1024).map_err(|error| error.to_string())?;
    let text = String::from_utf8(before.clone()).map_err(|_| "task metadata is not UTF-8")?;
    let mut task = read_meta(task_id, &text)?;
    if task.legacy_unknown || !same_path(&record_state(&task)?, &state) {
        return Err("human answer target is not owned by this state".into());
    }
    let decision = task
        .schedule
        .decisions
        .iter()
        .find(|decision| decision.id == question_id);
    let current = decision.is_some_and(|decision| {
        decision.task_id == task_id
            && decision.brief_revision == brief_revision
            && decision.workflow_revision.as_deref() == workflow_revision
            && task.accepted_brief_revision == Some(brief_revision)
    });
    let conflict = decision
        .and_then(|decision| decision.answer.as_deref())
        .is_some_and(|old| old != answer);
    let disposition = if current && !conflict {
        HumanAnswerDisposition::Accepted
    } else {
        HumanAnswerDisposition::Historical
    };
    let reason = match (&disposition, conflict) {
        (HumanAnswerDisposition::Historical, true) => {
            Some("question already has a different answer".into())
        }
        (HumanAnswerDisposition::Historical, false) => {
            Some("answer targets a stale or unknown question revision".into())
        }
        _ => None,
    };
    let record = HumanAnswerRecord {
        schema_version: SCHEMA_VERSION,
        answer_id: answer_id.to_owned(),
        task_id: task_id.to_owned(),
        question_id: question_id.to_owned(),
        brief_revision,
        workflow_revision: workflow_revision.map(str::to_owned),
        answer: answer.to_owned(),
        recorded_epoch: now_epoch(),
        disposition,
        reason,
    };
    fs::create_dir_all(answer_path.parent().expect("answer parent"))
        .map_err(|error| error.to_string())?;
    let answer_bytes = serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?;
    if record.disposition == HumanAnswerDisposition::Accepted {
        let resumes_wait = task.schedule.state == super::subagent_model::WorkState::WaitingHuman
            && task.schedule.waiting_condition.as_deref() == Some(question_id);
        task.answer_decision(
            question_id,
            brief_revision,
            workflow_revision,
            answer.to_owned(),
        )?;
        let current_questions_answered = task
            .schedule
            .decisions
            .iter()
            .filter(|decision| decision.brief_revision == brief_revision)
            .all(|decision| decision.answer.is_some());
        if resumes_wait && current_questions_answered {
            task.schedule.state = super::subagent_model::WorkState::Runnable;
            task.schedule.waiting_condition = None;
        }
        let updated = super::subagent_model::write_meta(&text, &task)?;
        let writes = vec![
            multplx_core::filesystem::TransitionWrite {
                path: answer_path
                    .strip_prefix(&state)
                    .map_err(|_| "human answer path left owning state")?
                    .to_owned(),
                before: None,
                after: answer_bytes,
            },
            multplx_core::filesystem::TransitionWrite {
                path: format!("{task_id}.meta").into(),
                before: Some(before),
                after: updated.into_bytes(),
            },
        ];
        multplx_core::filesystem::recoverable_transition(&state, &operation, &writes, None)
            .map_err(|error| error.to_string())?;
        publish_human_answer_wake(&state, &record, &answer_path)?;
    } else if !answer_path.is_file() {
        atomic_replace(&answer_path, &answer_bytes, 0o600).map_err(|error| error.to_string())?;
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::subagent_model::{ArtifactKind, AssignmentRole, TaskRecord, write_meta};

    fn root_id(root: &Path) -> String {
        format!("root-home:{}", root.display())
    }

    fn write_task(state: &Path, record: &TaskRecord) {
        fs::create_dir_all(state).unwrap();
        let text = write_meta("", record).unwrap();
        atomic_replace(
            state.join(format!("{}.meta", record.task_id)),
            text.as_bytes(),
            0o600,
        )
        .unwrap();
    }

    fn task(
        id: &str,
        owner: &Path,
        parent_id: &str,
        parent_home: &Path,
        parent_state: &Path,
        root: &Path,
    ) -> TaskRecord {
        let mut record = TaskRecord::new(
            id.into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            parent_id.into(),
            root_id(root),
            owner.to_string_lossy().into_owned(),
        );
        record.owner_state = Some(owner.join("state").to_string_lossy().into_owned());
        record.parent_home = Some(parent_home.to_string_lossy().into_owned());
        record.parent_state = Some(parent_state.to_string_lossy().into_owned());
        record
    }

    fn envelope(record: &TaskRecord, message: &str, kind: &str) -> MessageEnvelope {
        MessageEnvelope {
            schema_version: crate::lifecycle::subagent_model::SCHEMA_VERSION,
            message_id: message.into(),
            task_id: record.task_id.clone(),
            task_home: record.owner_home.clone(),
            parent_home: record.parent_home.clone(),
            attempt: record.attempt.clone(),
            parent_id: record.parent_id.clone(),
            sender: record.task_id.clone(),
            recipient: record.parent_id.clone().unwrap(),
            brief_revision: record.accepted_brief_revision,
            kind: kind.into(),
            correlation_id: format!("corr-{message}"),
            created_at: "2026-09-15T00:00:00Z".into(),
            summary: "accepted outcome".into(),
            artifact: Some("result.md".into()),
            acknowledgement: Acknowledgement::Pending,
        }
    }

    #[test]
    fn silent_model_outcome_reaches_root_with_original_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("child");
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(child.join("state")).unwrap();
        let root_identity = root_id(&root);
        let record = task(
            "worker",
            &child,
            &root_identity,
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &record);
        let event = envelope(&record, "outcome-one", "done");
        assert!(record_report(&child.join("state"), &event).unwrap());
        let child_health = relay(&child.join("state"), 8).unwrap();
        assert_eq!(child_health.pending_outbox, 0);
        let root_health = relay(&root.join("state"), 8).unwrap();
        assert_eq!(root_health.pending_inbox, 0);
        let delivered =
            crate::operational_input::read_message_envelope(&root.join("state"), "outcome-one")
                .unwrap();
        assert_eq!(delivered, event);
        record_outcome(&child.join("state"), &event).unwrap();
        assert_eq!(pending_outcomes(&child.join("state")).unwrap(), 0);
        assert_eq!(inspect(&child.join("state")).unwrap().delivered_outbox, 1);
    }

    #[test]
    fn future_producer_uses_home_state_fallback_and_generic_root_payload() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("child");
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(child.join("state")).unwrap();
        let mut record = task(
            "producer-worker",
            &child,
            &root_id(&root),
            &root,
            &root.join("state"),
            &root,
        );
        record.owner_state = None;
        record.parent_state = None;
        write_task(&child.join("state"), &record);
        let event = envelope(&record, "producer-event", "pr-ready");
        record_outcome(&child.join("state"), &event).unwrap();
        relay(&child.join("state"), 8).unwrap();
        relay(&root.join("state"), 8).unwrap();
        assert_eq!(
            crate::operational_input::read_message_envelope(&root.join("state"), "producer-event")
                .unwrap(),
            event
        );
        assert!(
            fs::read_to_string(root.join("state/.wake-queue"))
                .unwrap()
                .contains("parent-outcome: message=producer-event")
        );
    }

    #[test]
    fn wrong_home_same_basename_and_stale_generation_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("a/child");
        let impostor = temp.path().join("b/child");
        for path in [&root, &child, &impostor] {
            fs::create_dir_all(path.join("state")).unwrap();
        }
        let root_identity = root_id(&root);
        let record = task(
            "same",
            &child,
            &root_identity,
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &record);
        write_task(&impostor.join("state"), &record);
        let event = envelope(&record, "outcome-two", "failed");
        assert!(record_outcome(&impostor.join("state"), &event).is_err());

        let mut stale = event.clone();
        stale.attempt.as_mut().unwrap().generation += 1;
        assert!(
            record_outcome(&child.join("state"), &stale)
                .unwrap_err()
                .contains("stale attempt")
        );
    }

    #[test]
    fn root_state_requires_home_containment_or_a_live_recorded_override_owner() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let foreign = temp.path().join("foreign");
        let child = temp.path().join("child");
        let in_home_override = root.join("coordination-state");
        let external_override = temp.path().join("external-coordination-state");
        for path in [
            root.join("state"),
            foreign.join("state"),
            child.join("state"),
            in_home_override.clone(),
            external_override.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        let root_identity = root_id(&root);
        let escaped = task(
            "escaped-worker",
            &child,
            &root_identity,
            &root,
            &foreign.join("state"),
            &root,
        );
        write_task(&child.join("state"), &escaped);
        record_outcome(
            &child.join("state"),
            &envelope(&escaped, "escaped-root-state", "failed"),
        )
        .unwrap();
        let health = relay(&child.join("state"), 8).unwrap();
        assert_eq!(health.pending_outbox, 1);
        assert!(
            health
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("root parent route conflicts"))
        );
        assert!(records(&inbox(&foreign.join("state"))).unwrap().is_empty());

        let allowed = task(
            "override-worker",
            &child,
            &root_identity,
            &root,
            &in_home_override,
            &root,
        );
        write_task(&child.join("state"), &allowed);
        record_outcome(
            &child.join("state"),
            &envelope(&allowed, "allowed-root-state", "done"),
        )
        .unwrap();
        relay(&child.join("state"), 8).unwrap();
        relay(&in_home_override, 8).unwrap();
        assert!(
            crate::operational_input::read_message_envelope(
                &in_home_override,
                "allowed-root-state"
            )
            .is_ok()
        );

        let processes = SystemProcessProbe::default();
        let identity = processes.identity(std::process::id()).unwrap();
        let watcher_lock =
            DirectoryLock::try_acquire(external_override.join(".watch.lock"), &processes).unwrap();
        watcher_lock
            .publish_metadata("mx-home", format!("{}\n", root.display()).as_bytes())
            .unwrap();
        watcher_lock
            .publish_metadata("pid-identity", format!("{}\n", identity.marker).as_bytes())
            .unwrap();
        let external = task(
            "external-override-worker",
            &child,
            &root_identity,
            &root,
            &external_override,
            &root,
        );
        write_task(&child.join("state"), &external);
        record_outcome(
            &child.join("state"),
            &envelope(&external, "external-root-state", "done"),
        )
        .unwrap();
        relay(&child.join("state"), 8).unwrap();
        relay(&external_override, 8).unwrap();
        assert!(
            crate::operational_input::read_message_envelope(
                &external_override,
                "external-root-state"
            )
            .is_ok()
        );

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&foreign, root.join("escaped-state")).unwrap();
            let symlink_escape = task(
                "symlink-escape-worker",
                &child,
                &root_identity,
                &root,
                &root.join("escaped-state/state"),
                &root,
            );
            write_task(&child.join("state"), &symlink_escape);
            record_outcome(
                &child.join("state"),
                &envelope(&symlink_escape, "symlink-root-state", "failed"),
            )
            .unwrap();
            let health = relay(&child.join("state"), 8).unwrap();
            assert!(health.pending_outbox >= 1);
            assert!(
                crate::operational_input::read_message_envelope(
                    &foreign.join("state"),
                    "symlink-root-state"
                )
                .is_err()
            );
        }
    }

    #[test]
    fn crash_between_inbox_and_sender_receipt_retries_without_duplicate() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("child");
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(child.join("state")).unwrap();
        let root_identity = root_id(&root);
        let record = task(
            "worker",
            &child,
            &root_identity,
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &record);
        record_outcome(
            &child.join("state"),
            &envelope(&record, "fault-one", "done"),
        )
        .unwrap();
        let outbox_path = record_path(&outbox(&child.join("state")), "fault-one").unwrap();
        assert!(deliver_outbox_record_with_fault(&outbox_path, true).is_err());
        assert_eq!(inspect(&child.join("state")).unwrap().pending_outbox, 1);
        assert_eq!(relay(&child.join("state"), 8).unwrap().pending_outbox, 0);
        assert_eq!(records(&inbox(&root.join("state"))).unwrap().len(), 1);
        relay(&root.join("state"), 8).unwrap();
        assert_eq!(
            records(&root.join("state/message-outbox")).unwrap().len(),
            1
        );
    }

    #[test]
    fn nested_coordinator_relays_each_hop_without_a_model_turn() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("coordinator-home");
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(child.join("state")).unwrap();
        let root_identity = root_id(&root);
        let mut coordinator = task(
            "coordinator",
            &root,
            &root_identity,
            &root,
            &root.join("state"),
            &root,
        );
        coordinator.role = AssignmentRole::SubOrchestrator;
        coordinator.artifact = ArtifactKind::Coordination;
        coordinator.assignments[0].role = AssignmentRole::SubOrchestrator;
        write_task(&root.join("state"), &coordinator);
        let worker = task(
            "worker",
            &child,
            "coordinator",
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &worker);
        let event = envelope(&worker, "nested-result", "done");
        record_outcome(&child.join("state"), &event).unwrap();
        relay(&child.join("state"), 8).unwrap();
        relay(&root.join("state"), 8).unwrap();
        relay(&root.join("state"), 8).unwrap();
        relay(&root.join("state"), 8).unwrap();
        let delivered =
            crate::operational_input::read_message_envelope(&root.join("state"), "nested-result")
                .unwrap();
        assert_eq!(delivered, event);
        assert_eq!(pending_outcomes(&root.join("state")).unwrap(), 0);
        let inbox_records = records(&root.join("state/parent-receipts"))
            .unwrap()
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.starts_with("parent-inbox-"))
            })
            .collect::<Vec<_>>();
        assert_eq!(inbox_records.len(), 2, "{:?}", inspect(&root.join("state")));
        assert!(
            inbox_records
                .into_iter()
                .all(|path| { read_outcome(&path).unwrap().delivery == DeliveryState::Delivered })
        );
    }

    #[test]
    fn frozen_outcome_survives_origin_and_same_coordinator_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("child");
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(child.join("state")).unwrap();
        let root_identity = root_id(&root);
        let parent = task(
            "coordinator",
            &root,
            &root_identity,
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&root.join("state"), &parent);
        let mut worker = task(
            "worker",
            &child,
            "coordinator",
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &worker);
        let frozen = prepare_outcome(
            &child.join("state"),
            &envelope(&worker, "frozen-result", "failed"),
        )
        .unwrap();
        worker.prior_attempts.push(worker.attempt.take().unwrap());
        worker.attempt = Some(crate::lifecycle::subagent_model::Attempt {
            id: "replacement-attempt".into(),
            generation: 2,
            brief_revision: 1,
        });
        write_task(&child.join("state"), &worker);
        persist_prepared(&child.join("state"), &frozen).unwrap();
        let outbox_path = record_path(&outbox(&child.join("state")), "frozen-result").unwrap();
        assert!(deliver_outbox_record_with_fault(&outbox_path, true).is_err());

        let mut replaced_parent = parent;
        replaced_parent
            .prior_attempts
            .push(replaced_parent.attempt.take().unwrap());
        replaced_parent.attempt = Some(crate::lifecycle::subagent_model::Attempt {
            id: "new-parent-attempt".into(),
            generation: 2,
            brief_revision: 1,
        });
        write_task(&root.join("state"), &replaced_parent);
        let health = relay(&child.join("state"), 8).unwrap();
        assert_eq!(health.pending_outbox, 0);
        let repaired =
            read_outcome(&child.join("state/parent-receipts/parent-outbox-frozen-result.json"))
                .unwrap_or_else(|error| {
                    panic!(
                        "{error}; files={:?}",
                        fs::read_dir(child.join("state"))
                            .unwrap()
                            .map(|entry| entry.unwrap().path())
                            .collect::<Vec<_>>()
                    )
                });
        assert_eq!(repaired.event, frozen.event);
        assert_eq!(repaired.route_repairs.len(), 1);
        assert_eq!(repaired.route.recipient_attempt_generation, Some(2));
        assert_eq!(
            repaired.route_repairs[0]
                .prior_route
                .recipient_attempt_generation,
            Some(1)
        );
        for _ in 0..4 {
            relay(&root.join("state"), 8).unwrap();
        }
        assert_eq!(
            crate::operational_input::read_message_envelope(&root.join("state"), "frozen-result")
                .unwrap(),
            frozen.event
        );
    }

    #[test]
    fn assignment_only_route_repair_respects_the_history_bound() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("child");
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(child.join("state")).unwrap();
        let mut parent = task(
            "coordinator",
            &root,
            &root_id(&root),
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&root.join("state"), &parent);
        let worker = task(
            "worker",
            &child,
            "coordinator",
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &worker);
        let mut outcome = prepare_outcome(
            &child.join("state"),
            &envelope(&worker, "bounded-repair", "failed"),
        )
        .unwrap();

        parent.assignments[0].generation = 2;
        write_task(&root.join("state"), &parent);
        let prior_route = outcome.route.clone();
        outcome.route_repairs = (0..64)
            .map(|epoch| RouteRepair {
                prior_route: prior_route.clone(),
                repaired_epoch: epoch,
            })
            .collect();
        assert!(
            repair_replaced_recipient(&mut outcome)
                .unwrap_err()
                .contains("bounded route repair history")
        );
        assert_eq!(outcome.route, prior_route);
        assert_eq!(outcome.route_repairs.len(), 64);

        outcome.route_repairs.pop();
        assert_eq!(
            repair_replaced_recipient(&mut outcome).unwrap(),
            root.join("state").canonicalize().unwrap()
        );
        assert_eq!(outcome.route_repairs.len(), 64);
        assert_eq!(outcome.route.recipient_attempt_generation, Some(1));
        assert_eq!(outcome.route.recipient_assignment_generation, Some(2));
    }

    #[test]
    fn foreign_parent_replacement_cannot_repair_a_frozen_route() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let foreign_root = temp.path().join("foreign-root");
        let child = temp.path().join("child");
        for path in [&root, &foreign_root, &child] {
            fs::create_dir_all(path.join("state")).unwrap();
        }
        let parent = task(
            "coordinator",
            &root,
            &root_id(&root),
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&root.join("state"), &parent);
        let worker = task(
            "worker",
            &child,
            "coordinator",
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &worker);
        record_outcome(
            &child.join("state"),
            &envelope(&worker, "foreign-repair", "failed"),
        )
        .unwrap();
        let mut replacement = task(
            "coordinator",
            &root,
            &root_id(&foreign_root),
            &foreign_root,
            &foreign_root.join("state"),
            &foreign_root,
        );
        replacement.owner_home = Some(root.to_string_lossy().into_owned());
        replacement.owner_state = Some(root.join("state").to_string_lossy().into_owned());
        replacement
            .prior_attempts
            .push(replacement.attempt.take().unwrap());
        replacement.attempt = Some(crate::lifecycle::subagent_model::Attempt {
            id: "foreign-attempt".into(),
            generation: 2,
            brief_revision: 1,
        });
        write_task(&root.join("state"), &replacement);
        let health = relay(&child.join("state"), 8).unwrap();
        assert_eq!(health.pending_outbox, 1);
        assert!(
            health
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("parent route conflicts"))
        );
    }

    #[test]
    fn unavailable_parent_does_not_block_a_healthy_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let owner = temp.path().join("owner");
        let root = temp.path().join("root");
        let absent = temp.path().join("absent-root");
        fs::create_dir_all(owner.join("state")).unwrap();
        fs::create_dir_all(root.join("state")).unwrap();
        let healthy = task(
            "healthy",
            &owner,
            &root_id(&root),
            &root,
            &root.join("state"),
            &root,
        );
        let unavailable = task(
            "unavailable",
            &owner,
            &root_id(&absent),
            &absent,
            &absent.join("state"),
            &absent,
        );
        write_task(&owner.join("state"), &healthy);
        write_task(&owner.join("state"), &unavailable);
        record_outcome(
            &owner.join("state"),
            &envelope(&healthy, "healthy-result", "done"),
        )
        .unwrap();
        record_outcome(
            &owner.join("state"),
            &envelope(&unavailable, "unavailable-result", "failed"),
        )
        .unwrap();
        let unavailable_path =
            record_path(&outbox(&owner.join("state")), "unavailable-result").unwrap();
        let unavailable_before = fs::read(&unavailable_path).unwrap();
        let health = relay(&owner.join("state"), 8).unwrap();
        assert_eq!(health.relayed_this_tick, 1, "{health:?}");
        assert_eq!(health.delivered_outbox, 1);
        assert_eq!(health.pending_outbox, 1);
        assert_eq!(records(&inbox(&root.join("state"))).unwrap().len(), 1);
        inspect(&root.join("state")).unwrap();
        relay(&root.join("state"), 8).unwrap();
        assert_eq!(
            fs::read(&unavailable_path).unwrap(),
            unavailable_before,
            "root inspection/relay mutated a foreign home's pending outcome"
        );
    }

    #[test]
    fn relay_cursor_prevents_failed_prefix_from_starving_later_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let owner = temp.path().join("owner");
        let root = temp.path().join("root");
        let absent = temp.path().join("absent");
        fs::create_dir_all(owner.join("state")).unwrap();
        fs::create_dir_all(root.join("state")).unwrap();
        for index in 0..70 {
            let task_id = format!("failed-{index:03}");
            let record = task(
                &task_id,
                &owner,
                &root_id(&absent),
                &absent,
                &absent.join("state"),
                &absent,
            );
            write_task(&owner.join("state"), &record);
            record_outcome(
                &owner.join("state"),
                &envelope(&record, &format!("a-failed-{index:03}"), "failed"),
            )
            .unwrap();
        }
        let healthy = task(
            "healthy-late",
            &owner,
            &root_id(&root),
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&owner.join("state"), &healthy);
        record_outcome(
            &owner.join("state"),
            &envelope(&healthy, "z-healthy-result", "done"),
        )
        .unwrap();
        for _ in 0..9 {
            relay(&owner.join("state"), 8).unwrap();
        }
        assert_eq!(records(&inbox(&root.join("state"))).unwrap().len(), 1);
        assert_eq!(inspect(&owner.join("state")).unwrap().pending_outbox, 70);
    }

    #[test]
    fn relay_sweep_retries_a_recovered_old_route_under_continuous_newer_traffic() {
        let temp = tempfile::tempdir().unwrap();
        let owner = temp.path().join("owner");
        let recovered_root = temp.path().join("recovered-root");
        let healthy_root = temp.path().join("healthy-root");
        fs::create_dir_all(owner.join("state")).unwrap();
        fs::create_dir_all(healthy_root.join("state")).unwrap();

        let old = task(
            "old-route",
            &owner,
            &root_id(&recovered_root),
            &recovered_root,
            &recovered_root.join("state"),
            &recovered_root,
        );
        write_task(&owner.join("state"), &old);
        record_outcome(
            &owner.join("state"),
            &envelope(&old, "a-old-recovered", "failed"),
        )
        .unwrap();
        relay(&owner.join("state"), 1).unwrap();

        let first_new = task(
            "new-route-0",
            &owner,
            &root_id(&healthy_root),
            &healthy_root,
            &healthy_root.join("state"),
            &healthy_root,
        );
        write_task(&owner.join("state"), &first_new);
        record_outcome(
            &owner.join("state"),
            &envelope(&first_new, "z-new-000", "done"),
        )
        .unwrap();
        relay(&owner.join("state"), 1).unwrap();

        fs::create_dir_all(recovered_root.join("state")).unwrap();
        for index in 1..=4 {
            let current = task(
                &format!("new-route-{index}"),
                &owner,
                &root_id(&healthy_root),
                &healthy_root,
                &healthy_root.join("state"),
                &healthy_root,
            );
            write_task(&owner.join("state"), &current);
            record_outcome(
                &owner.join("state"),
                &envelope(&current, &format!("z-new-{index:03}"), "done"),
            )
            .unwrap();
            relay(&owner.join("state"), 1).unwrap();
            if inbox(&recovered_root.join("state"))
                .join("a-old-recovered-hop-0-repair-0.json")
                .is_file()
            {
                break;
            }
        }
        assert!(
            inbox(&recovered_root.join("state"))
                .join("a-old-recovered-hop-0-repair-0.json")
                .is_file(),
            "a recovered old route was starved by continuously arriving newer records"
        );
    }

    #[test]
    fn prepared_retry_keeps_original_creation_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("child");
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(child.join("state")).unwrap();
        let root_identity = root_id(&root);
        let record = task(
            "worker",
            &child,
            &root_identity,
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &record);
        let mut prepared = prepare_outcome(
            &child.join("state"),
            &envelope(&record, "retry-result", "done"),
        )
        .unwrap();
        persist_prepared(&child.join("state"), &prepared).unwrap();
        prepared.created_epoch += 50;
        persist_prepared(&child.join("state"), &prepared).unwrap();
        let retained =
            read_outcome(&record_path(&outbox(&child.join("state")), "retry-result").unwrap())
                .unwrap();
        assert_ne!(retained.created_epoch, prepared.created_epoch);
    }

    #[test]
    fn active_queue_capacity_is_bounded_without_scanning_retained_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let queue = temp.path().join("parent-outbox");
        fs::create_dir_all(&queue).unwrap();
        fs::write(queue.join("one.json"), b"one").unwrap();
        fs::write(queue.join("two.json"), b"two").unwrap();
        assert!(
            ensure_active_capacity(&queue, 2)
                .unwrap_err()
                .contains("2-record bound")
        );
        fs::remove_file(queue.join("one.json")).unwrap();
        ensure_active_capacity(&queue, 2).unwrap();
        fs::create_dir_all(temp.path().join("parent-receipts")).unwrap();
        for index in 0..20 {
            fs::write(
                temp.path()
                    .join("parent-receipts")
                    .join(format!("receipt-{index}.json")),
                b"retained",
            )
            .unwrap();
        }
        ensure_active_capacity(&queue, 2).unwrap();
    }

    #[test]
    fn durable_faults_preserve_facts_and_block_retirement_until_archive_recovers() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("child");
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(child.join("state")).unwrap();
        let record = task(
            "durable-worker",
            &child,
            &root_id(&root),
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &record);
        let prepared = prepare_outcome(
            &child.join("state"),
            &envelope(&record, "durable-fact", "done"),
        )
        .unwrap();
        persist_prepared(&child.join("state"), &prepared).unwrap();
        let source = record_path(&outbox(&child.join("state")), "durable-fact").unwrap();

        // A conflicting retry cannot replace the accepted fact already on disk.
        let original_bytes = fs::read(&source).unwrap();
        let mut conflict = prepared.clone();
        conflict.event.summary = "conflicting fact".into();
        assert!(
            write_immutable(&source, &conflict)
                .unwrap_err()
                .contains("identity conflicts")
        );
        assert_eq!(fs::read(&source).unwrap(), original_bytes);
        assert_eq!(pending_outcomes(&child.join("state")).unwrap(), 1);

        // Model the crash boundary after publication and delivery marking, but
        // before the sender can archive its active record. A foreign receipt
        // with the same name must not authorize deletion or retirement.
        let mut delivered = prepared.clone();
        delivered.delivery = DeliveryState::Delivered;
        delivered.delivered_epoch = Some(now_epoch());
        rewrite(&source, &delivered).unwrap();
        let receipt = archived_receipt_path(&source).unwrap();
        fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        let mut unsafe_receipt = delivered.clone();
        unsafe_receipt.event.summary = "unrelated delivered fact".into();
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&unsafe_receipt).unwrap(),
        )
        .unwrap();
        let delivered_bytes = fs::read(&source).unwrap();
        assert!(
            archive_delivered(&source, &delivered)
                .unwrap_err()
                .contains("receipt identity conflicts")
        );
        assert_eq!(fs::read(&source).unwrap(), delivered_bytes);
        assert_eq!(pending_outcomes(&child.join("state")).unwrap(), 1);
        let blocked = relay(&child.join("state"), 1).unwrap();
        assert_eq!(blocked.relayed_this_tick, 0);
        assert!(
            blocked
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("receipt identity conflicts"))
        );
        assert_eq!(fs::read(&source).unwrap(), delivered_bytes);

        // A damaged archive journal also retains the source fact and keeps the
        // home non-retirable. Repair is explicit; relay never guesses through
        // the transaction boundary.
        fs::remove_file(&receipt).unwrap();
        let archive_journal = child.join("state/.transitions/parent-archive-out-durable-fact.json");
        fs::create_dir_all(archive_journal.parent().unwrap()).unwrap();
        fs::write(&archive_journal, b"{broken-journal").unwrap();
        let journal_blocked = relay(&child.join("state"), 1).unwrap();
        assert_eq!(journal_blocked.relayed_this_tick, 0);
        assert!(journal_blocked.last_error.is_some());
        assert_eq!(fs::read(&source).unwrap(), delivered_bytes);
        assert_eq!(pending_outcomes(&child.join("state")).unwrap(), 1);

        // Once the invalid journal is removed, the next bounded relay tick
        // completes the interrupted archive without republishing the event.
        fs::remove_file(&archive_journal).unwrap();
        let recovered = relay(&child.join("state"), 1).unwrap();
        assert_eq!(recovered.relayed_this_tick, 1);
        assert!(!source.exists());
        assert_eq!(read_outcome(&receipt).unwrap(), delivered);
        assert_eq!(pending_outcomes(&child.join("state")).unwrap(), 0);
        persist_prepared(&child.join("state"), &prepared).unwrap();
        assert!(!source.exists(), "an archived retry must remain archived");

        // Malformed active state is observable and blocks retirement rather
        // than being silently skipped or deleted by relay.
        let corrupt = outbox(&child.join("state")).join("corrupt.json");
        fs::write(&corrupt, b"{not-json").unwrap();
        let corrupt_bytes = fs::read(&corrupt).unwrap();
        let unhealthy = relay(&child.join("state"), 1).unwrap();
        assert_eq!(unhealthy.relayed_this_tick, 0);
        assert!(unhealthy.last_error.is_some());
        assert_eq!(pending_outcomes(&child.join("state")).unwrap(), 1);
        assert_eq!(fs::read(&corrupt).unwrap(), corrupt_bytes);
    }

    #[test]
    fn validation_and_identity_conflicts_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let child = temp.path().join("child");
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(child.join("state")).unwrap();
        let record = task(
            "edge-worker",
            &child,
            &root_id(&root),
            &root,
            &root.join("state"),
            &root,
        );
        write_task(&child.join("state"), &record);

        let mut question_task = record.clone();
        assert!(bind_human_question(&mut question_task, "", 1, None, "question").is_err());
        bind_human_question(&mut question_task, "question", 1, None, "question").unwrap();
        assert_eq!(
            bind_human_question(&mut question_task, "question", 1, None, "question")
                .unwrap()
                .id,
            "question"
        );
        assert!(bind_human_question(&mut question_task, "question", 1, None, "changed").is_err());

        let progress = envelope(&record, "progress-edge", "working");
        assert!(!record_report(&child.join("state"), &progress).unwrap());
        let mut acknowledged = envelope(&record, "ack-edge", "done");
        acknowledged.acknowledgement = Acknowledgement::Delivered;
        assert!(prepare_outcome(&child.join("state"), &acknowledged).is_err());
        let mut invalid_prepared = prepare_outcome(
            &child.join("state"),
            &envelope(&record, "prepared-edge", "done"),
        )
        .unwrap();
        invalid_prepared.delivery = DeliveryState::Delivered;
        assert!(persist_prepared(&child.join("state"), &invalid_prepared).is_err());
        let foreign_inbox = child.join("state/parent-inbox/foreign-route.json");
        let prepared = prepare_outcome(
            &child.join("state"),
            &envelope(&record, "foreign-edge", "done"),
        )
        .unwrap();
        write_immutable(&foreign_inbox, &prepared).unwrap();
        assert!(
            promote_inbox_record(&child.join("state"), &foreign_inbox)
                .unwrap_err()
                .contains("foreign parent inbox")
        );
        fs::remove_file(&foreign_inbox).unwrap();

        let event = envelope(&record, "conflict-edge", "done");
        record_outcome(&child.join("state"), &event).unwrap();
        let mut changed = event.clone();
        changed.summary = "different outcome".into();
        assert!(record_outcome(&child.join("state"), &changed).is_err());
        relay(&child.join("state"), 8).unwrap();
        assert!(record_outcome(&child.join("state"), &changed).is_err());

        let foreign_queue = temp.path().join("foreign-queue");
        fs::create_dir(&foreign_queue).unwrap();
        fs::write(foreign_queue.join("foreign.txt"), b"foreign").unwrap();
        assert!(ensure_active_capacity(&foreign_queue, 4).is_err());

        fs::create_dir_all(child.join("state/parent-inbox")).unwrap();
        fs::write(child.join("state/parent-inbox/corrupt.json"), b"{}").unwrap();
        let health = inspect(&child.join("state")).unwrap();
        assert!(health.last_error.is_some());
        assert_eq!(health.records_observed, 1);
    }

    #[test]
    fn human_answers_preserve_stale_revisions_as_history() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir_all(root.join("state")).unwrap();
        let root_identity = root_id(&root);
        let mut record = task(
            "decision-task",
            &root,
            &root_identity,
            &root,
            &root.join("state"),
            &root,
        );
        let question =
            bind_human_question(&mut record, "api-choice", 1, Some("flow-1"), "Which API?")
                .unwrap();
        assert_eq!(question.id, "api-choice");
        assert_eq!(
            record.schedule.state,
            super::super::subagent_model::WorkState::WaitingHuman
        );
        write_task(&root.join("state"), &record);
        let stale = record_human_answer(
            &root.join("state"),
            "decision-task",
            "api-choice",
            1,
            Some("flow-2"),
            "answer-stale",
            "B",
        )
        .unwrap();
        assert_eq!(stale.disposition, HumanAnswerDisposition::Historical);
        let accepted = record_human_answer(
            &root.join("state"),
            "decision-task",
            "api-choice",
            1,
            Some("flow-1"),
            "answer-current",
            "A",
        )
        .unwrap();
        assert_eq!(accepted.disposition, HumanAnswerDisposition::Accepted);
        let updated = task_record(&root.join("state"), "decision-task").unwrap();
        assert_eq!(updated.schedule.decisions[0].answer.as_deref(), Some("A"));
        assert_eq!(
            updated.schedule.state,
            super::super::subagent_model::WorkState::Runnable
        );
        assert!(
            fs::read_to_string(root.join("state/.wake-queue"))
                .unwrap()
                .contains("human-answer-answer-current")
        );
        let repeated = record_human_answer(
            &root.join("state"),
            "decision-task",
            "api-choice",
            1,
            Some("flow-1"),
            "answer-current",
            "A",
        )
        .unwrap();
        assert_eq!(repeated, accepted);
        assert!(
            root.join("state/human-answers/decision-task/answer-stale.json")
                .is_file()
        );
    }

    #[test]
    fn answer_resumes_only_its_current_human_wait() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir_all(root.join("state")).unwrap();
        let mut record = task(
            "revision-task",
            &root,
            &root_id(&root),
            &root,
            &root.join("state"),
            &root,
        );
        bind_human_question(&mut record, "old-question", 1, None, "Old question?").unwrap();
        record
            .revise_assignment(
                1,
                AssignmentRole::Implementer,
                "new scope".into(),
                vec![],
                vec![],
                "new revision".into(),
            )
            .unwrap();
        bind_human_question(
            &mut record,
            "current-question",
            2,
            None,
            "Current question?",
        )
        .unwrap();
        write_task(&root.join("state"), &record);
        record_human_answer(
            &root.join("state"),
            "revision-task",
            "current-question",
            2,
            None,
            "current-answer",
            "yes",
        )
        .unwrap();
        let resumed = task_record(&root.join("state"), "revision-task").unwrap();
        assert_eq!(
            resumed.schedule.state,
            super::super::subagent_model::WorkState::Runnable
        );
        assert!(resumed.schedule.decisions[0].answer.is_none());

        let mut unrelated = resumed;
        bind_human_question(
            &mut unrelated,
            "background-question",
            2,
            None,
            "Background question?",
        )
        .unwrap();
        unrelated.schedule.state = super::super::subagent_model::WorkState::WaitingDependency;
        unrelated.schedule.waiting_condition = Some("dependency-x".into());
        write_task(&root.join("state"), &unrelated);
        let historical = record_human_answer(
            &root.join("state"),
            "revision-task",
            "old-question",
            1,
            None,
            "old-answer",
            "late",
        )
        .unwrap();
        assert_eq!(historical.disposition, HumanAnswerDisposition::Historical);
        let preserved = task_record(&root.join("state"), "revision-task").unwrap();
        assert_eq!(
            preserved.schedule.state,
            super::super::subagent_model::WorkState::WaitingDependency
        );
        assert_eq!(
            preserved.schedule.waiting_condition.as_deref(),
            Some("dependency-x")
        );
        let exact = record_human_answer(
            &root.join("state"),
            "revision-task",
            "background-question",
            2,
            None,
            "background-answer",
            "yes",
        )
        .unwrap();
        assert_eq!(exact.disposition, HumanAnswerDisposition::Accepted);
        let still_preserved = task_record(&root.join("state"), "revision-task").unwrap();
        assert_eq!(
            still_preserved.schedule.state,
            super::super::subagent_model::WorkState::WaitingDependency
        );
        assert_eq!(
            still_preserved.schedule.waiting_condition.as_deref(),
            Some("dependency-x")
        );
    }
}

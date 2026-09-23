//! Versioned task identity in the existing `.meta` authority.
//!
//! `canonical_model` is an embedded JSON record, not a parallel state store.
//! Legacy wire fields remain compatibility projections until home migration.

use crate::project_registry::ProjectBinding;
use multplx_core::identifiers::TaskId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AssignmentRole {
    Researcher,
    Implementer,
    Reviewer,
    SubOrchestrator,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    Report,
    Implementation,
    Coordination,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeReference {
    pub provider: String,
    /// Native providers may expose no session identity. Never invent one.
    pub session_id: Option<String>,
    pub endpoint: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub generation: u64,
    pub brief_revision: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BriefRevision {
    pub revision: u64,
    pub scope: String,
    pub acceptance_criteria: Vec<String>,
    pub source_artifacts: Vec<String>,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AssignmentChange {
    pub generation: u64,
    pub role: AssignmentRole,
    pub brief_revision: u64,
    pub reason: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkState {
    #[default]
    Accepted,
    Runnable,
    Running,
    WaitingExternal,
    WaitingDependency,
    WaitingHuman,
    Completed,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HumanDecision {
    pub id: String,
    pub task_id: String,
    pub brief_revision: u64,
    pub workflow_revision: Option<String>,
    pub question: String,
    pub answer: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScheduleFacts {
    pub priority: i32,
    pub dependencies: Vec<String>,
    pub state: WorkState,
    pub waiting_condition: Option<String>,
    pub decisions: Vec<HumanDecision>,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DomainBinding {
    pub domain_id: String,
    pub coordinator_id: String,
    pub scope_revision: u64,
    pub assignment_generation: u64,
    pub projects: Vec<String>,
    pub idea_id: Option<String>,
    pub scope: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OwnershipTransfer {
    pub transfer_id: String,
    pub from_coordinator: String,
    pub to_coordinator: String,
    pub expected_generation: u64,
    pub accepted_generation: Option<u64>,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AllocationBinding {
    pub allocation_id: String,
    pub lease_id: String,
    pub generation: u64,
    pub project_id: String,
    pub checkout_id: String,
    pub common_git_identity: String,
    pub path: String,
    pub base_revision: String,
    pub task_id: String,
    pub attempt_id: String,
    pub persistent: bool,
}
impl AllocationBinding {
    pub fn validate_token(
        &self,
        allocation: &str,
        lease: &str,
        generation: u64,
        project: &str,
        attempt: &str,
    ) -> Result<(), String> {
        if self.allocation_id != allocation
            || self.lease_id != lease
            || self.generation != generation
            || self.project_id != project
            || self.attempt_id != attempt
        {
            return Err("stale or foreign allocation lease".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RetainedExecution {
    pub attempt: Attempt,
    pub runtime: RuntimeReference,
    pub allocation: Option<AllocationBinding>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum NativeObservationState {
    Started,
    Result,
    Interrupted,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NativeDelegationObservation {
    pub observation_id: String,
    pub provider: String,
    /// Retained only when the provider supplies a child identity.
    pub child_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub turn_id: Option<String>,
    pub parent_attempt: Option<Attempt>,
    pub state: NativeObservationState,
    pub observed_at: String,
    /// A provider-owned transcript or result artifact. Its prose never
    /// becomes an authoritative task completion transition.
    pub artifact: Option<String>,
    /// `provider-identity` is reserved for a provider whose continuation
    /// behavior is independently verified. `session-bound` makes no survival
    /// claim, even when an identifier is available for event correlation.
    pub recovery: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskRecord {
    pub schema_version: u32,
    pub task_id: String,
    pub role: AssignmentRole,
    pub artifact: ArtifactKind,
    pub persistent: bool,
    /// The assignment owns an isolated operational home. This is independent
    /// of whether it remains registered after its current bounded work ends.
    #[serde(default)]
    pub private_home: bool,
    pub parent_id: Option<String>,
    pub root_id: Option<String>,
    pub owner_home: Option<String>,
    pub owner_state: Option<String>,
    pub parent_state: Option<String>,
    pub parent_home: Option<String>,
    pub persistent_home: Option<String>,
    #[serde(default)]
    pub home_allocation: Option<super::home_seed::HomeBinding>,
    pub accepted_brief_digest: Option<String>,
    pub accepted_brief_path: Option<String>,
    pub runtime: RuntimeReference,
    pub attempt: Option<Attempt>,
    pub accepted_brief_revision: Option<u64>,
    pub briefs: Vec<BriefRevision>,
    pub prior_attempts: Vec<Attempt>,
    pub retained_executions: Vec<RetainedExecution>,
    #[serde(default)]
    pub native_observations: Vec<NativeDelegationObservation>,
    pub assignments: Vec<AssignmentChange>,
    pub schedule: ScheduleFacts,
    pub project: Option<ProjectBinding>,
    pub allocation: Option<AllocationBinding>,
    pub domain: Option<DomainBinding>,
    pub owning_coordinator: Option<String>,
    pub transfers: Vec<OwnershipTransfer>,
    #[serde(default)]
    pub delivery: super::delivery_evidence::DeliveryFacts,
    pub legacy_unknown: bool,
}

pub fn new_identity(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{prefix}-{nanos:x}-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
impl TaskRecord {
    pub fn new(
        task_id: String,
        role: AssignmentRole,
        artifact: ArtifactKind,
        persistent: bool,
        parent_id: String,
        root_id: String,
        owner_home: String,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            task_id,
            role,
            artifact,
            persistent,
            private_home: persistent,
            parent_id: Some(parent_id),
            root_id: Some(root_id),
            parent_home: Some(owner_home.clone()),
            owner_state: Some(
                Path::new(&owner_home)
                    .join("state")
                    .to_string_lossy()
                    .into_owned(),
            ),
            parent_state: Some(
                Path::new(&owner_home)
                    .join("state")
                    .to_string_lossy()
                    .into_owned(),
            ),
            owner_home: Some(owner_home),
            persistent_home: None,
            home_allocation: None,
            accepted_brief_digest: None,
            accepted_brief_path: None,
            runtime: RuntimeReference::default(),
            attempt: Some(Attempt {
                id: new_identity("attempt"),
                generation: 1,
                brief_revision: 1,
            }),
            accepted_brief_revision: Some(1),
            briefs: vec![BriefRevision {
                revision: 1,
                scope: String::new(),
                acceptance_criteria: vec![],
                source_artifacts: vec![],
                reason: "initial assignment".into(),
            }],
            prior_attempts: vec![],
            retained_executions: vec![],
            native_observations: vec![],
            assignments: vec![AssignmentChange {
                generation: 1,
                role,
                brief_revision: 1,
                reason: "initial assignment".into(),
            }],
            schedule: ScheduleFacts::default(),
            project: None,
            allocation: None,
            domain: None,
            owning_coordinator: None,
            transfers: vec![],
            delivery: super::delivery_evidence::DeliveryFacts::default(),
            legacy_unknown: false,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err("unsupported task writer schema; migrate the home before writing".into());
        }
        TaskId::parse(&self.task_id).map_err(|e| e.to_string())?;
        if self.role == AssignmentRole::SubOrchestrator
            && self.artifact == ArtifactKind::Implementation
        {
            return Err(
                "sub-orchestrator assignment delegates implementation and test-code changes".into(),
            );
        }
        if !self.legacy_unknown
            && [
                self.parent_id.as_deref(),
                self.root_id.as_deref(),
                self.owner_home.as_deref(),
                self.parent_home.as_deref(),
            ]
            .iter()
            .any(|v| v.is_none_or(str::is_empty))
        {
            return Err("canonical child requires parent, root and owner home".into());
        }
        if self.parent_id.as_deref() == Some(&self.task_id) && self.parent_home == self.owner_home {
            return Err("self-parent cycle".into());
        }
        if let Some(attempt) = &self.attempt {
            if attempt.id.is_empty()
                || attempt.generation == 0
                || attempt.brief_revision == 0
                || Some(attempt.brief_revision) != self.accepted_brief_revision
            {
                return Err("attempt is not bound to accepted brief".into());
            }
            if self
                .prior_attempts
                .iter()
                .any(|old| old.id == attempt.id || old.generation >= attempt.generation)
            {
                return Err("duplicate or conflicting attempt generation".into());
            }
        } else if !self.legacy_unknown {
            return Err("canonical task requires attempt identity".into());
        }
        let mut revisions = BTreeSet::new();
        if self
            .briefs
            .iter()
            .any(|brief| brief.revision == 0 || !revisions.insert(brief.revision))
            || (!self.legacy_unknown
                && self.accepted_brief_revision != self.briefs.last().map(|b| b.revision))
        {
            return Err("invalid accepted brief history".into());
        }
        let mut generations = BTreeSet::new();
        if self.assignments.iter().any(|a| {
            a.generation == 0
                || !generations.insert(a.generation)
                || !revisions.contains(&a.brief_revision)
        }) || (!self.legacy_unknown
            && self.assignments.last().is_none_or(|a| {
                a.role != self.role || Some(a.brief_revision) != self.accepted_brief_revision
            }))
        {
            return Err("invalid assignment history".into());
        }
        let mut decision_ids = BTreeSet::new();
        if self.schedule.decisions.iter().any(|d| {
            d.task_id != self.task_id
                || !revisions.contains(&d.brief_revision)
                || d.id.is_empty()
                || d.question.is_empty()
                || !decision_ids.insert(&d.id)
        }) {
            return Err("invalid decision identity or target".into());
        }
        let mut attempts = BTreeSet::new();
        if self.prior_attempts.iter().any(|a| {
            a.id.is_empty() || a.generation == 0 || !attempts.insert((&a.id, a.generation))
        }) {
            return Err("invalid historical attempt identity".into());
        }
        let mut native_observations = BTreeSet::new();
        if self.native_observations.iter().any(|observation| {
            observation.observation_id.is_empty()
                || observation.provider.is_empty()
                || observation.observed_at.is_empty()
                || !matches!(
                    observation.recovery.as_str(),
                    "provider-identity" | "session-bound"
                )
                || (observation.recovery == "provider-identity" && observation.child_id.is_none())
                || !native_observations.insert(&observation.observation_id)
        }) {
            return Err("invalid native delegation observation".into());
        }
        let mut seen = BTreeSet::new();
        if self
            .schedule
            .dependencies
            .iter()
            .any(|d| d == &self.task_id || !seen.insert(d))
        {
            return Err("duplicate or self dependency".into());
        }
        if let Some(domain) = &self.domain
            && (domain.domain_id.is_empty()
                || domain.scope.is_empty()
                || domain.scope_revision == 0
                || domain.assignment_generation == 0
                || (domain.projects.is_empty() && domain.idea_id.is_none()))
        {
            return Err("domain requires explicit bounded scope and identity".into());
        }

        if let Some(home) = &self.home_allocation
            && (!(self.private_home || self.persistent)
                || home.id != self.task_id
                || home.generation == 0
                || home.lease_id.is_empty()
                || self.owner_home.as_deref().map(Path::new) != Some(home.owner_home.as_path())
                || self.persistent_home.as_deref().map(Path::new) != Some(home.path.as_path()))
        {
            return Err("private home allocation does not match task ownership".into());
        }
        if let Some(allocation) = &self.allocation {
            if allocation.persistent != self.persistent
                || !Path::new(&allocation.path).is_absolute()
                || allocation.task_id != self.task_id
                || self
                    .attempt
                    .as_ref()
                    .is_none_or(|a| a.id != allocation.attempt_id)
                || allocation.generation == 0
                || allocation.allocation_id.is_empty()
                || allocation.lease_id.is_empty()
            {
                return Err("allocation is not bound to current task attempt".into());
            }
            if let Some(project) = &self.project {
                if allocation.project_id != project.project_id
                    || allocation.checkout_id != project.checkout_id
                    || allocation.common_git_identity != project.common_git_identity
                    || allocation.base_revision != project.starting_revision
                {
                    return Err("allocation project identity mismatch".into());
                }
            } else {
                return Err("allocation requires selected project identity".into());
            }
        }
        self.delivery.validate(self)?;
        Ok(())
    }
    /// Caller must reconcile and stop or isolate the old process before replacement.
    pub fn replace_attempt(
        &mut self,
        previous_attempt_id: &str,
        old_execution_isolated: bool,
    ) -> Result<(), String> {
        let old = self
            .attempt
            .as_ref()
            .ok_or("legacy attempt must be reconciled before replacement")?;
        if old.id != previous_attempt_id || !old_execution_isolated {
            return Err("replacement requires matching reconciled isolated execution".into());
        }
        let generation = old
            .generation
            .checked_add(1)
            .ok_or("attempt generation exhausted")?;
        self.retained_executions.push(RetainedExecution {
            attempt: old.clone(),
            runtime: self.runtime.clone(),
            allocation: self.allocation.clone(),
        });
        self.prior_attempts.push(old.clone());
        self.attempt = Some(Attempt {
            id: new_identity("attempt"),
            generation,
            brief_revision: self
                .accepted_brief_revision
                .ok_or("accepted brief missing")?,
        });
        self.runtime.session_id = None;
        self.runtime.endpoint = None;
        self.delivery.current_commit = None;
        self.schedule.state = WorkState::Runnable;
        self.schedule.waiting_condition = None;
        // Retention belongs to allocation owner. Never transfer the old lease.
        self.allocation = None;
        Ok(())
    }
    pub fn resume_attempt(&self, proof: &ResumeProof) -> Result<Attempt, String> {
        let attempt = self
            .attempt
            .as_ref()
            .ok_or("legacy attempt identity is unknown")?;
        if &proof.attempt != attempt
            || self.runtime.provider != proof.provider
            || self.runtime.session_id.as_deref() != Some(&proof.session_id)
            || !proof.reconciled
        {
            return Err("resume identity was not proven".into());
        }
        Ok(attempt.clone())
    }
    /// Explicit steering changes the accepted revision, retaining all old evidence.
    pub fn revise_assignment(
        &mut self,
        expected_revision: u64,
        role: AssignmentRole,
        scope: String,
        acceptance_criteria: Vec<String>,
        source_artifacts: Vec<String>,
        reason: String,
    ) -> Result<(), String> {
        if Some(expected_revision) != self.accepted_brief_revision || reason.trim().is_empty() {
            return Err("revision conflict or missing change reason".into());
        }
        let revision = expected_revision
            .checked_add(1)
            .ok_or("brief revision exhausted")?;
        self.briefs.push(BriefRevision {
            revision,
            scope,
            acceptance_criteria,
            source_artifacts,
            reason: reason.clone(),
        });
        self.assignments.push(AssignmentChange {
            generation: self.assignments.len() as u64 + 1,
            role,
            brief_revision: revision,
            reason,
        });
        self.role = role;
        self.accepted_brief_revision = Some(revision);
        if let Some(attempt) = &mut self.attempt {
            attempt.brief_revision = revision;
        }
        self.delivery.current_commit = None;
        self.schedule.state = WorkState::Runnable;
        self.schedule.waiting_condition = None;
        Ok(())
    }
    pub fn answer_decision(
        &mut self,
        id: &str,
        brief_revision: u64,
        workflow_revision: Option<&str>,
        answer: String,
    ) -> Result<(), String> {
        let decision = self
            .schedule
            .decisions
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or("unknown decision")?;
        if self.accepted_brief_revision != Some(brief_revision)
            || decision.brief_revision != brief_revision
            || decision.workflow_revision.as_deref() != workflow_revision
            || decision.task_id != self.task_id
        {
            return Err("stale decision target".into());
        }
        if decision.answer.as_ref().is_some_and(|old| old != &answer) {
            return Err("conflicting decision answer".into());
        }
        decision.answer = Some(answer);
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct ResumeProof {
    pub attempt: Attempt,
    pub provider: String,
    pub session_id: String,
    pub reconciled: bool,
}

fn fields(text: &str) -> Result<BTreeMap<&str, &str>, String> {
    let mut fields = BTreeMap::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once('=').ok_or("malformed task metadata")?;
        if fields.insert(key, value).is_some() {
            return Err(format!("duplicate task metadata field: {key}"));
        }
    }
    Ok(fields)
}
pub fn read_meta(task_id: &str, text: &str) -> Result<TaskRecord, String> {
    let fields = fields(text)?;
    if let Some(value) = fields.get("canonical_model") {
        let record: TaskRecord =
            serde_json::from_str(value).map_err(|e| format!("malformed canonical task: {e}"))?;
        if record.task_id != task_id {
            return Err("task filename and canonical identity conflict".into());
        }
        if fields.get("schema_version").is_some_and(|v| *v != "2") {
            return Err("conflicting task schema versions".into());
        }
        record.validate()?;
        if let Some(kind) = fields.get("kind") {
            let expected = if record.private_home || record.persistent {
                "daemon"
            } else if record.artifact == ArtifactKind::Report {
                "scout"
            } else {
                "delivery"
            };
            if *kind != expected {
                return Err("legacy kind and canonical output/persistence conflict".into());
            }
        }
        if let Some(home) = fields.get("home")
            && record
                .persistent_home
                .as_deref()
                .is_some_and(|canonical| canonical != *home)
        {
            return Err("legacy and canonical persistent homes conflict".into());
        }
        if let Some(project) = &record.project
            && fields
                .get("project")
                .is_some_and(|legacy| Path::new(legacy) != project.canonical_path)
        {
            return Err("legacy and canonical project identity conflict".into());
        }
        if fields
            .get("window")
            .is_some_and(|endpoint| record.runtime.endpoint.as_deref() != Some(*endpoint))
        {
            return Err("legacy and canonical endpoint conflict".into());
        }
        return Ok(record);
    }
    if fields.get("schema_version").is_some_and(|v| *v != "1") {
        return Err("unsupported task metadata schema".into());
    }
    let kind = fields.get("kind").copied().unwrap_or("delivery");
    let (role, artifact, persistent) = match kind {
        "delivery" | "actor" => (
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
        ),
        "scout" => (AssignmentRole::Researcher, ArtifactKind::Report, false),
        "daemon" => (
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            true,
        ),
        _ => return Err(format!("unknown legacy task kind: {kind}")),
    };
    let mode = fields.get("mode").copied().unwrap_or("");
    if (kind == "daemon" && !matches!(mode, "" | "daemon"))
        || (kind != "daemon" && mode == "daemon")
    {
        return Err("conflicting legacy kind and persistence mode".into());
    }
    let mut record = TaskRecord::new(
        task_id.into(),
        role,
        artifact,
        persistent,
        String::new(),
        String::new(),
        String::new(),
    );
    record.parent_id = fields.get("parent_id").map(|v| (*v).into());
    record.root_id = fields.get("root_id").map(|v| (*v).into());
    record.owner_home = fields.get("home").map(|v| (*v).into());
    record.runtime = RuntimeReference {
        provider: fields.get("backend").copied().unwrap_or("tmux").into(),
        session_id: None,
        endpoint: fields.get("window").map(|v| (*v).into()),
    };
    record.owner_state = None;
    record.parent_state = None;
    record.parent_home = None;
    record.attempt = None;
    record.accepted_brief_revision = None;
    record.briefs.clear();
    record.assignments.clear();
    record.legacy_unknown = true;
    record.validate()?;
    Ok(record)
}
pub fn write_meta(text: &str, record: &TaskRecord) -> Result<String, String> {
    let old_fields = fields(text)?;
    if old_fields
        .get("schema_version")
        .is_some_and(|v| !matches!(*v, "1" | "2"))
    {
        return Err("incompatible metadata writer version".into());
    }
    record.validate()?;
    let mut result = text
        .lines()
        .filter(|line| {
            !line.starts_with("canonical_model=") && !line.starts_with("schema_version=")
        })
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    result.push_str(&format!(
        "schema_version=2\ncanonical_model={}\n",
        serde_json::to_string(record).map_err(|e| e.to_string())?
    ));
    Ok(result)
}

/// IDs in a local state directory are qualified by their owner home for routing.
pub fn qualified_task_id(home: &str, task_id: &str) -> String {
    format!("{home}#task:{task_id}")
}

/// Resolve qualified parent/dependency edges without imposing a nesting limit.
/// Bare dependency/coordinator IDs refer to the record's metadata owner home.
pub fn validate_lineage(records: &[TaskRecord], roots: &[String]) -> Result<(), String> {
    let mut by_id = BTreeMap::new();
    for record in records {
        record.validate()?;
        let key = qualified_task_id(
            record.owner_home.as_deref().unwrap_or("legacy-unknown"),
            &record.task_id,
        );
        if by_id.insert(key, record).is_some() {
            return Err("duplicate/conflicting task identity".into());
        }
    }
    let resolve = |home: &str, id: &str| {
        if id.contains("#task:") {
            id.to_owned()
        } else {
            qualified_task_id(home, id)
        }
    };
    let mut domains = BTreeMap::new();
    for record in records {
        if let Some(domain) = &record.domain {
            let key = (&record.root_id, &domain.domain_id);
            let owner = resolve(
                record.owner_home.as_deref().unwrap_or("legacy-unknown"),
                &domain.coordinator_id,
            );
            if domains
                .insert(key, owner.clone())
                .is_some_and(|old| old != owner)
            {
                return Err("conflicting current domain owners".into());
            }
            if !roots.contains(&domain.coordinator_id) && !by_id.contains_key(&owner) {
                return Err("unresolved domain coordinator".into());
            }
        }
        if !record.legacy_unknown {
            let root = record.root_id.as_ref().ok_or("missing root identity")?;
            if !roots.contains(root) {
                return Err("unresolved root identity".into());
            }
            let mut visited = BTreeSet::new();
            let mut current = record;
            loop {
                let current_key = qualified_task_id(
                    current.owner_home.as_deref().ok_or("missing owner home")?,
                    &current.task_id,
                );
                if !visited.insert(current_key) {
                    return Err("cyclic parent identity".into());
                }
                let parent = current
                    .parent_id
                    .as_ref()
                    .ok_or("unresolved parent identity")?;
                if roots.contains(parent) {
                    if parent != root {
                        return Err("conflicting root lineage".into());
                    }
                    break;
                }
                let parent_key = resolve(
                    current
                        .parent_home
                        .as_deref()
                        .ok_or("missing parent home")?,
                    parent,
                );
                current = by_id.get(&parent_key).ok_or("unresolved parent identity")?;
                if current.root_id.as_ref() != Some(root) {
                    return Err("conflicting root lineage".into());
                }
            }
        }
        if let Some(owner) = &record.owning_coordinator
            && !roots.contains(owner)
            && !by_id.contains_key(&resolve(
                record.owner_home.as_deref().unwrap_or("legacy-unknown"),
                owner,
            ))
        {
            return Err("unresolved owning coordinator".into());
        }
        let initial = qualified_task_id(
            record.owner_home.as_deref().unwrap_or("legacy-unknown"),
            &record.task_id,
        );
        let mut stack = vec![(initial, BTreeSet::new())];
        while let Some((id, mut path)) = stack.pop() {
            if !path.insert(id.clone()) {
                return Err("cyclic task dependencies".into());
            }
            let current = by_id.get(&id).ok_or("unresolved dependency")?;
            for dependency in &current.schedule.dependencies {
                let key = resolve(
                    current.owner_home.as_deref().unwrap_or("legacy-unknown"),
                    dependency,
                );
                if !by_id.contains_key(&key) {
                    return Err("unresolved dependency".into());
                }
                stack.push((key, path.clone()));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Acknowledgement {
    Pending,
    Delivered,
    Acknowledged,
    Answered,
    Completed,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MessageEnvelope {
    pub schema_version: u32,
    pub message_id: String,
    pub task_id: String,
    pub task_home: Option<String>,
    pub parent_home: Option<String>,
    pub attempt: Option<Attempt>,
    pub parent_id: Option<String>,
    pub sender: String,
    pub recipient: String,
    pub brief_revision: Option<u64>,
    pub kind: String,
    pub correlation_id: String,
    pub created_at: String,
    pub summary: String,
    pub artifact: Option<String>,
    pub acknowledgement: Acknowledgement,
}
impl MessageEnvelope {
    pub fn validate_current(
        &self,
        record: &TaskRecord,
        authenticated_sender: &str,
        actual_recipient: &str,
    ) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION
            || self.message_id.is_empty()
            || self.correlation_id.is_empty()
            || self.created_at.is_empty()
            || self.kind.is_empty()
        {
            return Err("invalid message envelope".into());
        }
        if self.task_id != record.task_id
            || self.task_home != record.owner_home
            || self.parent_home != record.parent_home
            || self.sender != authenticated_sender
            || self.recipient != actual_recipient
            || self.parent_id != record.parent_id
        {
            return Err("message sender, recipient or task routing mismatch".into());
        }
        let task_level = matches!(
            self.kind.as_str(),
            "task-note" | "question" | "decision-request"
        );
        if !task_level
            && !crate::supervision::REPORT_STATES.contains(&self.kind.as_str())
            && !matches!(
                self.kind.as_str(),
                "result"
                    | "completion"
                    | "evidence"
                    | "research-available"
                    | "implementation-ready"
                    | "pr-ready"
                    | "publication-failed"
                    | "evidence-changed"
                    | "human-decision"
                    | "human-merge"
                    | "final-disposition"
            )
        {
            return Err("unknown message kind".into());
        }
        if time::OffsetDateTime::parse(
            &self.created_at,
            &time::format_description::well_known::Rfc3339,
        )
        .is_err()
        {
            return Err("invalid message creation time".into());
        }
        if self.brief_revision != record.accepted_brief_revision
            || ((!task_level || self.attempt.is_some()) && self.attempt != record.attempt)
        {
            return Err("stale attempt or accepted brief evidence".into());
        }
        if record.legacy_unknown {
            return Err("legacy evidence has unknown attempt identity".into());
        }
        Ok(())
    }
}

/// Home-level writer marker prevents new incompatible writers joining a home.
/// Phase 09 owns quiescing and transferring already-running legacy writers.
pub fn require_writer_version(state: &Path) -> Result<(), String> {
    let marker = state.join(".task-writer-version");
    match std::fs::read_to_string(&marker) {
        Ok(version) if version.trim() == "2" => Ok(()),
        Ok(_) => Err("incompatible task writer home; migration/reconciliation required".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            for entry in std::fs::read_dir(state).map_err(|e| e.to_string())? {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "meta")
                {
                    let id = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or("invalid task filename")?;
                    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                    if read_meta(id, &text)?.legacy_unknown {
                        return Err(
                            "legacy task writers require home migration before canonical writes"
                                .into(),
                        );
                    }
                }
            }
            multplx_core::filesystem::atomic_replace(marker, b"2\n", 0o600)
                .map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

pub const TASK_MODEL_USAGE: &str = "Usage: mx task-model inspect <task-id> [--authority-state <absolute-path>]\n       mx task-model validate\n       mx task-model review-queue\n       mx task-model evidence <task-id> --request-file <json-path> [--authority-state <absolute-path>]\n       mx task-model revise <task-id> --expected-revision <n> --scope <text> --reason <text> [--role researcher|implementer|reviewer|sub-orchestrator] [--artifact report|implementation|coordination] [--acceptance <text>]... [--source <path>]... [--brief-file <path>] [--authority-state <absolute-path>]\n\nReads and revisions use the existing task .meta authority. evidence accepts the closed typed EvidenceRequest JSON contract for evidence-updated check, optional review and limitation facts. It cannot introduce or replace a canonical PR; verified publication and poll owners record publication and human-merge outcomes. review-queue returns current revision-bound PR evidence in dependency order; optional independent review is not a publication gate. A successor coordinator supplies --authority-state to route through a transferred task's retained canonical record. Revisions preserve historical briefs, attempts, and delivery evidence; running workers must receive the new revision before current evidence is accepted. Replacement/resume are reconciled by the spawn owner.\n";

fn record_state(record: &TaskRecord) -> Result<std::path::PathBuf, String> {
    let path = record
        .owner_state
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            record
                .owner_home
                .as_ref()
                .map(|home| Path::new(home).join("state"))
        })
        .ok_or("missing task state owner")?;
    std::fs::canonicalize(&path).map_err(|error| {
        format!(
            "task state owner {} is unavailable: {error}",
            path.display()
        )
    })
}

/// Read only recorded ancestors; a qualified identity is visited once, and the
/// common graph validator diagnoses cycles after every route is resolved.
fn load_ancestors(records: &mut Vec<TaskRecord>, local_state: &Path) -> Result<(), String> {
    let local_state = std::fs::canonicalize(local_state).map_err(|error| error.to_string())?;
    for record in records.iter().filter(|record| !record.legacy_unknown) {
        if record_state(record)? != local_state {
            return Err("task record is stored outside its owning state".into());
        }
    }
    let mut index = 0;
    while index < records.len() {
        let current = records[index].clone();
        index += 1;
        if current.legacy_unknown || current.parent_id == current.root_id {
            continue;
        }
        let parent = current
            .parent_id
            .as_deref()
            .ok_or("unresolved parent identity")?;
        let home = current
            .parent_home
            .as_deref()
            .ok_or("missing parent home")?;
        let parent_state = current
            .parent_state
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(home).join("state"));
        let parent_state = std::fs::canonicalize(&parent_state).map_err(|error| {
            format!(
                "parent state {} is unavailable: {error}",
                parent_state.display()
            )
        })?;
        let qualified = qualified_task_id(home, parent);
        if let Some(existing) = records.iter().find(|record| {
            qualified_task_id(
                record.owner_home.as_deref().unwrap_or("legacy-unknown"),
                &record.task_id,
            ) == qualified
        }) {
            if existing.legacy_unknown || record_state(existing)? != parent_state {
                return Err("parent route conflicts with recorded owner state".into());
            }
            continue;
        }
        TaskId::parse(parent).map_err(|error| error.to_string())?;
        let path = parent_state.join(format!("{parent}.meta"));
        let bytes = multplx_core::filesystem::read_bounded_regular(&path, 4 * 1024 * 1024)
            .map_err(|error| format!("cannot resolve parent {}: {error}", path.display()))?;
        let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
        let ancestor = read_meta(parent, &text)?;
        if ancestor.legacy_unknown
            || ancestor.owner_home.as_deref() != Some(home)
            || record_state(&ancestor)? != parent_state
        {
            return Err("parent route conflicts with recorded owner home/state".into());
        }
        records.push(ancestor);
    }
    Ok(())
}

/// CLI mutation owner for explicit accepted scope/role/output changes.
pub fn command(args: &[String], state: &Path) -> Result<String, String> {
    if args.is_empty() || args.iter().any(|a| matches!(a.as_str(), "-h" | "--help")) {
        return Ok(TASK_MODEL_USAGE.into());
    }
    let load_records = || -> Result<Vec<TaskRecord>, String> {
        let mut records = vec![];
        for entry in std::fs::read_dir(state).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|ext| ext == "meta") {
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or("invalid task filename")?;
                let bytes = multplx_core::filesystem::read_bounded_regular(&path, 4 * 1024 * 1024)
                    .map_err(|e| e.to_string())?;
                let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                records.push(read_meta(id, &text)?);
            }
        }
        Ok(records)
    };
    if args == ["validate"] {
        let mut records = load_records()?;
        let local_count = records.len();
        load_ancestors(&mut records, state)?;
        let roots = records
            .iter()
            .filter_map(|r| r.root_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        validate_lineage(&records, &roots)?;
        return Ok(format!(
            "validated {} task records (schema 2; legacy identities remain unknown)\n",
            local_count
        ));
    }
    if args == ["review-queue"] {
        let mut records = load_records()?;
        super::delivery_evidence::extend_review_records_from_outcomes(state, &mut records)?;
        let queue = super::delivery_evidence::human_review_queue(&records)?;
        return serde_json::to_string_pretty(&queue)
            .map(|value| format!("{value}\n"))
            .map_err(|error| error.to_string());
    }
    if args.first().map(String::as_str) == Some("evidence") {
        if args.len() != 4 || args.get(2).map(String::as_str) != Some("--request-file") {
            return Err(TASK_MODEL_USAGE.into());
        }
        let id = args.get(1).ok_or(TASK_MODEL_USAGE)?;
        TaskId::parse(id).map_err(|error| error.to_string())?;
        let request_path = args.get(3).ok_or(TASK_MODEL_USAGE)?;
        let bytes = multplx_core::filesystem::read_bounded_regular(request_path, 1024 * 1024)
            .map_err(|error| error.to_string())?;
        let request: super::delivery_evidence::EvidenceRequest = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid closed delivery evidence request: {error}"))?;
        if request.outcome != super::delivery_evidence::DeliveryOutcome::EvidenceUpdated {
            return Err(
                "task-model evidence records reported checks/review only; publication and human merge outcomes require their verified runtime owners"
                    .into(),
            );
        }
        if let Some(pr_url) = request.pr_url.as_deref() {
            let text = std::fs::read_to_string(state.join(format!("{id}.meta")))
                .map_err(|error| error.to_string())?;
            let current = read_meta(id, &text)?;
            if !current
                .current_delivery_evidence()
                .is_some_and(|evidence| evidence.pr_url.as_deref() == Some(pr_url))
            {
                return Err(
                    "reported evidence cannot introduce or replace canonical PR identity".into(),
                );
            }
        }
        let (_, evidence) = super::delivery_evidence::record(state, id, &request)?;
        return serde_json::to_string_pretty(&evidence)
            .map(|value| format!("{value}\n"))
            .map_err(|error| error.to_string());
    }
    let id = args.get(1).ok_or(TASK_MODEL_USAGE)?;
    TaskId::parse(id).map_err(|e| e.to_string())?;
    let _lock = multplx_core::locks::DirectoryLock::try_acquire(
        state.join(format!(".{id}.identity.lock")),
        &multplx_core::process::SystemProcessProbe::default(),
    )
    .map_err(|e| e.to_string())?;
    let path = state.join(format!("{id}.meta"));
    let before = multplx_core::filesystem::read_bounded_regular(&path, 4 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(before.clone()).map_err(|e| e.to_string())?;
    let mut record = read_meta(id, &text)?;
    if args[0] == "inspect" && args.len() == 2 {
        return serde_json::to_string_pretty(&record)
            .map(|s| format!("{s}\n"))
            .map_err(|e| e.to_string());
    }
    if args[0] != "revise" {
        return Err(TASK_MODEL_USAGE.into());
    }
    if record.legacy_unknown {
        return Err("legacy task requires migration before revision".into());
    }
    require_writer_version(state)?;
    let mut expected = None;
    let mut scope = None;
    let mut reason = None;
    let mut role = record.role;
    let mut artifact = record.artifact;
    let mut acceptance = vec![];
    let mut sources = vec![];
    let mut brief_file = None;
    let mut index = 2;
    let mut seen_options = BTreeSet::new();
    while index < args.len() {
        if !matches!(args[index].as_str(), "--source" | "--acceptance")
            && !seen_options.insert(args[index].as_str())
        {
            return Err(format!("duplicate revision option {}", args[index]));
        }
        let value = args
            .get(index + 1)
            .ok_or("revision option requires value")?;
        match args[index].as_str() {
            "--expected-revision" => {
                expected = Some(value.parse::<u64>().map_err(|_| "invalid revision")?)
            }
            "--scope" => scope = Some(value.clone()),
            "--reason" => reason = Some(value.clone()),
            "--acceptance" => acceptance.push(value.clone()),
            "--source" => sources.push(value.clone()),
            "--brief-file" => brief_file = Some(value.clone()),
            "--role" => {
                role = match value.as_str() {
                    "researcher" => AssignmentRole::Researcher,
                    "implementer" => AssignmentRole::Implementer,
                    "reviewer" => AssignmentRole::Reviewer,
                    "sub-orchestrator" => AssignmentRole::SubOrchestrator,
                    _ => return Err("unknown assignment role".into()),
                }
            }
            "--artifact" => {
                artifact = match value.as_str() {
                    "report" => ArtifactKind::Report,
                    "implementation" => ArtifactKind::Implementation,
                    "coordination" => ArtifactKind::Coordination,
                    _ => return Err("unknown artifact kind".into()),
                }
            }
            _ => return Err(format!("unknown revision option {}", args[index])),
        }
        index += 2;
    }
    let expected = expected.ok_or("--expected-revision required")?;
    let scope = scope.ok_or("--scope required")?;
    let reason = reason.ok_or("--reason required")?;
    let operation = format!(
        "revise-{id}-{}",
        expected.checked_add(1).ok_or("revision exhausted")?
    );
    let brief_bytes = if let Some(file) = &brief_file {
        multplx_core::filesystem::read_bounded_regular(file, 4 * 1024 * 1024)
            .map_err(|e| e.to_string())?
    } else {
        let previous = record
            .accepted_brief_path
            .as_deref()
            .unwrap_or("legacy source not recorded");
        let role_name = match role {
            AssignmentRole::Researcher => "researcher",
            AssignmentRole::Implementer => "implementer",
            AssignmentRole::Reviewer => "reviewer",
            AssignmentRole::SubOrchestrator => "sub-orchestrator",
        };
        let artifact_name = match artifact {
            ArtifactKind::Report => "report",
            ArtifactKind::Implementation => "implementation",
            ArtifactKind::Coordination => "coordination",
        };
        format!("# Accepted brief revision {}\n\nAssignment: {role_name}\nRequested artifact: {artifact_name}\n\n## Current scope\n{scope}\n\n## Acceptance criteria\n{}\n\n## Source artifacts\n{}\n\n## Historical context\nPrevious accepted brief: {previous}\nThis pointer preserves prior evidence; the previous assignment has been superseded by the current scope above.\n\nRevision reason: {reason}\n",expected+1,acceptance.iter().map(|c|format!("- {c}")).collect::<Vec<_>>().join("\n"),sources.iter().map(|p|format!("- {p}")).collect::<Vec<_>>().join("\n")).into_bytes()
    };
    // On a repeated invocation compare semantic requested changes to retained intent.
    if state
        .join(".transitions")
        .join(format!("{operation}.json"))
        .exists()
    {
        let writes = multplx_core::filesystem::read_transition_writes(state, &operation)
            .map_err(|e| e.to_string())?;
        let committed = writes.last().ok_or("empty revision intent")?;
        let planned = read_meta(
            id,
            std::str::from_utf8(&committed.after).map_err(|e| e.to_string())?,
        )?;
        if !seen_options.contains("--role") {
            role = planned.role;
        }
        if !seen_options.contains("--artifact") {
            artifact = planned.artifact;
        }
        let brief = planned.briefs.last().ok_or("missing planned revision")?;
        if planned.role != role
            || planned.artifact != artifact
            || brief.scope != scope
            || brief.reason != reason
            || brief.acceptance_criteria != acceptance
            || brief.source_artifacts != sources
            || (brief_file.is_some() && writes.first().is_none_or(|w| w.after != brief_bytes))
        {
            return Err("revision identity reused with changed request".into());
        }
        multplx_core::filesystem::recover_transition(state, &operation)
            .map_err(|e| e.to_string())?;
        return Ok(format!("{id}: accepted brief revision {}\n", expected + 1));
    }
    use sha2::{Digest, Sha256};
    record.accepted_brief_digest = Some(format!("{:x}", Sha256::digest(&brief_bytes)));
    let brief_path = state
        .join("brief-revisions")
        .join(format!("{id}-{}.md", expected + 1));
    record.accepted_brief_path = Some(brief_path.to_string_lossy().into_owned());
    record.revise_assignment(expected, role, scope, acceptance, sources, reason)?;
    record.artifact = artifact;
    let mut compatibility = text
        .lines()
        .filter(|line| !line.starts_with("kind="))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    let kind = if record.private_home || record.persistent {
        "daemon"
    } else if artifact == ArtifactKind::Report {
        "scout"
    } else {
        "delivery"
    };
    compatibility.push_str(&format!("kind={kind}\n"));
    let after = write_meta(&compatibility, &record)?.into_bytes();
    let mut writes = vec![];
    // Brief content is retained by revision before the canonical metadata commit.
    let directory = state.join("brief-revisions");
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    writes.push(multplx_core::filesystem::TransitionWrite {
        path: brief_path
            .strip_prefix(state)
            .map_err(|e| e.to_string())?
            .into(),
        before: None,
        after: brief_bytes,
    });
    writes.push(multplx_core::filesystem::TransitionWrite {
        path: format!("{id}.meta").into(),
        before: Some(before),
        after,
    });
    multplx_core::filesystem::recoverable_transition(state, &operation, &writes, None)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{id}: accepted brief revision {}\n",
        record.accepted_brief_revision.expect("revision")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(id: &str) -> TaskRecord {
        TaskRecord::new(
            id.into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            "root-home:/main".into(),
            "root-home:/main".into(),
            "/main".into(),
        )
    }
    fn envelope(record: &TaskRecord) -> MessageEnvelope {
        MessageEnvelope {
            schema_version: 2,
            message_id: "message-1".into(),
            task_id: record.task_id.clone(),
            task_home: record.owner_home.clone(),
            parent_home: record.parent_home.clone(),
            attempt: record.attempt.clone(),
            parent_id: record.parent_id.clone(),
            sender: record.task_id.clone(),
            recipient: record.parent_id.clone().unwrap(),
            brief_revision: record.accepted_brief_revision,
            kind: "done".into(),
            correlation_id: "correlation-1".into(),
            created_at: "2026-09-14T00:00:00Z".into(),
            summary: "done".into(),
            artifact: Some("result.md".into()),
            acknowledgement: Acknowledgement::Pending,
        }
    }
    #[test]
    fn legacy_kinds_are_roles_output_and_persistence_without_invented_attempts() {
        for (kind, role, artifact, persistent) in [
            (
                "delivery",
                AssignmentRole::Implementer,
                ArtifactKind::Implementation,
                false,
            ),
            (
                "scout",
                AssignmentRole::Researcher,
                ArtifactKind::Report,
                false,
            ),
            (
                "daemon",
                AssignmentRole::Implementer,
                ArtifactKind::Implementation,
                true,
            ),
        ] {
            let record = read_meta("task", &format!("kind={kind}\nyolo=on\n")).unwrap();
            assert_eq!(
                (record.role, record.artifact, record.persistent),
                (role, artifact, persistent)
            );
            assert!(record.legacy_unknown);
            assert!(record.attempt.is_none());
            assert!(record.runtime.session_id.is_none());
        }
        for text in [
            "kind=scout\nkind=delivery\n",
            "kind=scout\nmode=daemon\n",
            "kind=unknown\n",
            "schema_version=77\nkind=scout\n",
            "malformed",
        ] {
            assert!(read_meta("task", text).is_err());
        }
    }
    #[test]
    fn canonical_metadata_refuses_conflicting_identity_and_unknown_fields() {
        let record = task("task");
        let text = write_meta("kind=delivery\n", &record).unwrap();
        assert_eq!(read_meta("task", &text).unwrap(), record);
        assert!(read_meta("other", &text).is_err());
        assert!(read_meta("task", &text.replace("kind=delivery", "kind=scout")).is_err());
        let mut value = serde_json::to_value(record).unwrap();
        value["unknown_field"] = true.into();
        assert!(read_meta("task", &format!("canonical_model={value}\n")).is_err());
    }

    #[test]
    fn canonical_persistent_records_from_before_private_home_field_remain_readable() {
        let mut record = task("persistent");
        record.persistent = true;
        record.private_home = true;
        record.persistent_home = Some("/private/persistent".into());
        let mut value = serde_json::to_value(&record).unwrap();
        value.as_object_mut().unwrap().remove("private_home");
        let decoded = read_meta(
            "persistent",
            &format!("schema_version=2\ncanonical_model={value}\n"),
        )
        .unwrap();
        assert!(decoded.persistent);
        assert!(!decoded.private_home);
        decoded.validate().unwrap();
    }
    #[test]
    fn replacement_retains_old_execution_and_resume_requires_actual_identity() {
        let mut record = task("task");
        record.schedule.state = WorkState::Completed;
        record.runtime.provider = "codex".into();
        record.runtime.session_id = Some("native-session".into());
        record.runtime.endpoint = Some("pane".into());
        let old = record.attempt.clone().unwrap();
        let proof = ResumeProof {
            attempt: old.clone(),
            provider: "codex".into(),
            session_id: "native-session".into(),
            reconciled: true,
        };
        assert_eq!(record.resume_attempt(&proof).unwrap(), old);
        assert!(record.replace_attempt(&old.id, false).is_err());
        record.replace_attempt(&old.id, true).unwrap();
        assert_eq!(record.schedule.state, WorkState::Runnable);
        assert_ne!(record.attempt.as_ref().unwrap().id, old.id);
        assert_eq!(record.attempt.as_ref().unwrap().generation, 2);
        assert_eq!(
            record.retained_executions[0].runtime.endpoint.as_deref(),
            Some("pane")
        );
        assert!(record.resume_attempt(&proof).is_err());
        assert!(record.runtime.session_id.is_none());
        record.validate().unwrap();
    }

    #[test]
    fn revised_brief_reopens_implementation_completion_gate() {
        let mut record = task("task");
        record.schedule.state = WorkState::Completed;
        record.schedule.waiting_condition = Some("old wait".into());
        record
            .revise_assignment(
                1,
                AssignmentRole::Implementer,
                "new scope".into(),
                vec!["new acceptance".into()],
                Vec::new(),
                "changed requirements".into(),
            )
            .unwrap();
        assert_eq!(record.schedule.state, WorkState::Runnable);
        assert!(record.schedule.waiting_condition.is_none());
        assert_eq!(record.accepted_brief_revision, Some(2));
    }
    #[test]
    fn evidence_checks_attempt_revision_sender_recipient_and_nullable_native_identity() {
        let mut record = task("task");
        let old = envelope(&record);
        old.validate_current(&record, "task", "root-home:/main")
            .unwrap();
        assert!(
            old.validate_current(&record, "impostor", "root-home:/main")
                .is_err()
        );
        assert!(
            old.validate_current(&record, "task", "wrong-recipient")
                .is_err()
        );
        record
            .revise_assignment(
                1,
                AssignmentRole::Reviewer,
                "new scope".into(),
                vec!["criterion".into()],
                vec!["research.md".into()],
                "user correction".into(),
            )
            .unwrap();
        assert!(
            old.validate_current(&record, "task", "root-home:/main")
                .is_err()
        );
        assert_eq!(record.briefs.len(), 2);
        let mut note = envelope(&record);
        note.kind = "task-note".into();
        note.attempt = None;
        note.validate_current(&record, "task", "root-home:/main")
            .unwrap();
        note.kind = "done".into();
        assert!(
            note.validate_current(&record, "task", "root-home:/main")
                .is_err()
        );
        assert!(record.runtime.session_id.is_none());
    }
    #[test]
    fn decisions_are_revision_bound_and_old_answers_cannot_steer_new_work() {
        let mut record = task("task");
        record.schedule.decisions.push(HumanDecision {
            id: "decision".into(),
            task_id: "task".into(),
            brief_revision: 1,
            workflow_revision: Some("flow-1".into()),
            question: "Which target?".into(),
            answer: None,
        });
        assert!(
            record
                .answer_decision("decision", 1, Some("flow-2"), "A".into())
                .is_err()
        );
        record
            .revise_assignment(
                1,
                AssignmentRole::Implementer,
                "scope".into(),
                vec![],
                vec![],
                "changed".into(),
            )
            .unwrap();
        assert!(
            record
                .answer_decision("decision", 1, Some("flow-1"), "A".into())
                .is_err()
        );
    }
    #[test]
    fn lineage_is_home_qualified_and_rejects_parent_dependency_and_owner_conflicts() {
        let mut parent = task("same");
        let mut child = task("same");
        child.owner_home = Some("/child".into());
        child.parent_id = Some("same".into());
        child.parent_home = Some("/main".into());
        validate_lineage(
            &[parent.clone(), child.clone()],
            &["root-home:/main".into()],
        )
        .unwrap();
        assert!(
            validate_lineage(
                &[parent.clone(), parent.clone()],
                &["root-home:/main".into()]
            )
            .is_err()
        );
        parent.parent_id = Some("same".into());
        parent.parent_home = Some("/child".into());
        assert!(validate_lineage(&[parent, child], &["root-home:/main".into()]).is_err());
        let mut a = task("a");
        let mut b = task("b");
        a.schedule.dependencies.push("b".into());
        b.schedule.dependencies.push("a".into());
        assert!(validate_lineage(&[a, b], &["root-home:/main".into()]).is_err());
        let mut a = task("a");
        let mut b = task("b");
        for record in [&mut a, &mut b] {
            record.role = AssignmentRole::SubOrchestrator;
            record.assignments[0].role = AssignmentRole::SubOrchestrator;
            record.artifact = ArtifactKind::Coordination;
            record.domain = Some(DomainBinding {
                domain_id: record.task_id.clone(),
                coordinator_id: record.task_id.clone(),
                scope_revision: 1,
                assignment_generation: 1,
                projects: vec!["shared-project".into()],
                idea_id: None,
                scope: record.task_id.clone(),
            });
        }
        validate_lineage(&[a.clone(), b.clone()], &["root-home:/main".into()]).unwrap();
        b.domain.as_mut().unwrap().domain_id = "a".into();
        b.domain.as_mut().unwrap().assignment_generation = 2;
        assert!(validate_lineage(&[a, b], &["root-home:/main".into()]).is_err());
    }
    #[test]
    fn reused_path_never_reuses_allocation_or_lease_authority() {
        let allocation = AllocationBinding {
            allocation_id: "alloc-2".into(),
            lease_id: "lease-2".into(),
            generation: 2,
            project_id: "project-a".into(),
            checkout_id: "checkout".into(),
            common_git_identity: "git-id".into(),
            path: "/same/path".into(),
            base_revision: "commit".into(),
            task_id: "task".into(),
            attempt_id: "attempt-2".into(),
            persistent: false,
        };
        allocation
            .validate_token("alloc-2", "lease-2", 2, "project-a", "attempt-2")
            .unwrap();
        assert!(
            allocation
                .validate_token("alloc-1", "lease-1", 1, "project-a", "attempt-1")
                .is_err()
        );
        assert!(
            allocation
                .validate_token(
                    "alloc-2",
                    "lease-2",
                    2,
                    "same-named-other-project",
                    "attempt-2"
                )
                .is_err()
        );
    }
    #[test]
    fn fresh_writer_claims_version_but_legacy_and_incompatible_homes_refuse() {
        let temp = tempfile::tempdir().unwrap();
        require_writer_version(temp.path()).unwrap();
        require_writer_version(temp.path()).unwrap();
        std::fs::write(temp.path().join(".task-writer-version"), "9\n").unwrap();
        assert!(require_writer_version(temp.path()).is_err());
        let legacy = tempfile::tempdir().unwrap();
        std::fs::write(legacy.path().join("old.meta"), "kind=daemon\n").unwrap();
        assert!(require_writer_version(legacy.path()).is_err());
        assert!(!legacy.path().join(".task-writer-version").exists());
    }
    #[test]
    fn cli_revision_is_owned_and_retains_accepted_brief_and_history() {
        let temp = tempfile::tempdir().unwrap();
        let record = task("task");
        std::fs::write(
            temp.path().join("task.meta"),
            write_meta("kind=delivery\n", &record).unwrap(),
        )
        .unwrap();
        let brief = temp.path().join("new.md");
        std::fs::write(&brief, "new accepted scope\n").unwrap();
        let args = [
            "revise",
            "task",
            "--expected-revision",
            "1",
            "--scope",
            "new scope",
            "--reason",
            "user correction",
            "--role",
            "reviewer",
            "--artifact",
            "report",
            "--brief-file",
            brief.to_str().unwrap(),
        ]
        .map(String::from);
        command(&args, temp.path()).unwrap();
        command(&args, temp.path()).unwrap();
        let result = read_meta(
            "task",
            &std::fs::read_to_string(temp.path().join("task.meta")).unwrap(),
        )
        .unwrap();
        assert_eq!(result.accepted_brief_revision, Some(2));
        assert_eq!(result.role, AssignmentRole::Reviewer);
        assert_eq!(result.artifact, ArtifactKind::Report);
        assert!(result.accepted_brief_digest.is_some());
        assert_eq!(
            std::fs::read_to_string(temp.path().join("brief-revisions/task-2.md")).unwrap(),
            "new accepted scope\n"
        );
    }
    #[test]
    fn duplicate_revision_options_and_coordinator_coding_are_rejected() {
        let mut record = task("task");
        record.role = AssignmentRole::SubOrchestrator;
        record.assignments[0].role = AssignmentRole::SubOrchestrator;
        assert!(record.validate().is_err());
        let temp = tempfile::tempdir().unwrap();
        let record = task("task");
        std::fs::write(
            temp.path().join("task.meta"),
            write_meta("kind=delivery\n", &record).unwrap(),
        )
        .unwrap();
        let args = [
            "revise",
            "task",
            "--expected-revision",
            "1",
            "--expected-revision",
            "2",
            "--scope",
            "new",
            "--reason",
            "correction",
        ]
        .map(String::from);
        assert!(
            command(&args, temp.path())
                .unwrap_err()
                .contains("duplicate")
        );
    }
    #[test]
    fn cli_validation_resolves_exact_nested_parent_state_and_refuses_missing_or_cyclic_routes() {
        let temp = tempfile::tempdir().unwrap();
        let parent_home = temp.path().join("parent-home");
        let parent_state = temp.path().join("custom-parent-state");
        let child_home = temp.path().join("child-home");
        let child_state = temp.path().join("custom-child-state");
        for path in [&parent_home, &parent_state, &child_home, &child_state] {
            std::fs::create_dir(path).unwrap();
        }
        let mut parent = task("parent");
        parent.owner_home = Some(parent_home.to_string_lossy().into_owned());
        parent.owner_state = Some(parent_state.to_string_lossy().into_owned());
        let mut child = task("child");
        child.owner_home = Some(child_home.to_string_lossy().into_owned());
        child.owner_state = Some(child_state.to_string_lossy().into_owned());
        child.parent_id = Some("parent".into());
        child.parent_home = parent.owner_home.clone();
        child.parent_state = parent.owner_state.clone();
        let parent_path = parent_state.join("parent.meta");
        let child_path = child_state.join("child.meta");
        std::fs::write(
            &parent_path,
            write_meta("kind=delivery\n", &parent).unwrap(),
        )
        .unwrap();
        std::fs::write(&child_path, write_meta("kind=delivery\n", &child).unwrap()).unwrap();
        let args = ["validate".into()];
        assert!(
            command(&args, &child_state)
                .unwrap()
                .contains("validated 1 task records")
        );
        std::fs::remove_file(&parent_path).unwrap();
        assert!(command(&args, &child_state).is_err());
        parent.parent_id = Some("child".into());
        parent.parent_home = child.owner_home.clone();
        parent.parent_state = child.owner_state.clone();
        std::fs::write(
            &parent_path,
            write_meta("kind=delivery\n", &parent).unwrap(),
        )
        .unwrap();
        assert!(command(&args, &child_state).unwrap_err().contains("cyclic"));
        parent.parent_id = parent.root_id.clone();
        parent.owner_state = Some(child_state.to_string_lossy().into_owned());
        std::fs::write(
            &parent_path,
            write_meta("kind=delivery\n", &parent).unwrap(),
        )
        .unwrap();
        assert!(
            command(&args, &child_state)
                .unwrap_err()
                .contains("owner home/state")
        );
    }

    #[test]
    fn evidence_cli_accepts_typed_reports_but_not_publication_or_merge_claims() {
        use super::super::delivery_evidence::{
            CheckOutcome, DeliveryCheck, DeliveryOutcome, EvidenceRequest,
        };

        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let state = home.join("state");
        std::fs::create_dir_all(&state).unwrap();
        let root = format!("root-home:{}", home.display());
        let mut record = TaskRecord::new(
            "task".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            root.clone(),
            root,
            home.to_string_lossy().into_owned(),
        );
        record.briefs[0].scope = "implementation".into();
        std::fs::write(
            state.join("task.meta"),
            write_meta("kind=delivery\n", &record).unwrap(),
        )
        .unwrap();
        let attempt = record.attempt.as_ref().unwrap();
        let mut request = EvidenceRequest {
            evidence_id: "reported".into(),
            attempt_id: attempt.id.clone(),
            attempt_generation: attempt.generation,
            brief_revision: attempt.brief_revision,
            commit: "a".repeat(40),
            checks: vec![DeliveryCheck {
                name: "cargo test".into(),
                outcome: CheckOutcome::Passed,
                summary: "passed".into(),
                artifact: None,
            }],
            review: None,
            limitations: vec!["live forge not exercised".into()],
            pr_url: None,
            outcome: DeliveryOutcome::EvidenceUpdated,
            observed_at: "2026-09-15T12:00:00Z".into(),
            mark_current: true,
            expected_current_commit: None,
        };
        let request_path = temp.path().join("evidence.json");
        std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
        let args = [
            "evidence".into(),
            "task".into(),
            "--request-file".into(),
            request_path.to_string_lossy().into_owned(),
        ];
        assert!(command(&args, &state).unwrap().contains("evidence-updated"));
        assert!(
            command(&["review-queue".into()], &state)
                .unwrap()
                .contains("[]")
        );

        request.evidence_id = "fabricated-publish".into();
        request.outcome = DeliveryOutcome::Published;
        request.pr_url = Some("https://github.com/example/project/pull/1".into());
        request.expected_current_commit = Some("a".repeat(40));
        std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
        assert!(
            command(&args, &state)
                .unwrap_err()
                .contains("verified runtime owners")
        );

        request.evidence_id = "fabricated-pr".into();
        request.outcome = DeliveryOutcome::EvidenceUpdated;
        std::fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
        assert!(
            command(&args, &state)
                .unwrap_err()
                .contains("cannot introduce or replace canonical PR")
        );
    }
}

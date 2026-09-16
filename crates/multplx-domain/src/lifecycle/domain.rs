//! Scoped coordinator reads and explicit domain revisions.
//!
//! The coordinator task `.meta` record remains the sole mutable authority.

use std::path::{Path, PathBuf};

use multplx_core::filesystem::{TransitionWrite, recoverable_transition};
use multplx_core::identifiers::TaskId;

use super::subagent_model::{ArtifactKind, AssignmentRole, DomainBinding, TaskRecord};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParentRoute {
    pub task_id: String,
    pub owner_home: PathBuf,
    pub owner_state: PathBuf,
    pub parent_id: String,
    pub parent_home: PathBuf,
    pub parent_state: PathBuf,
    pub root_id: String,
    pub attempt_id: String,
    pub attempt_generation: u64,
    pub brief_revision: u64,
    pub assignment_generation: u64,
    pub scope_revision: u64,
}

pub fn read_coordinator(state: &Path, id: &str) -> Result<TaskRecord, String> {
    TaskId::parse(id).map_err(|e| e.to_string())?;
    let path = state.join(format!("{id}.meta"));
    let bytes = multplx_core::filesystem::read_bounded_regular(&path, 4 * 1024 * 1024)
        .map_err(|e| format!("cannot read coordinator identity {}: {e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let record = super::subagent_model::read_meta(id, &text)?;
    if record.legacy_unknown
        || record.role != AssignmentRole::SubOrchestrator
        || record.artifact != ArtifactKind::Coordination
        || record
            .domain
            .as_ref()
            .is_none_or(|domain| domain.coordinator_id != id)
    {
        return Err("task is not a canonical scoped coordinator".into());
    }
    Ok(record)
}

pub fn validated_parent_route(state: &Path, id: &str) -> Result<ParentRoute, String> {
    let record = read_coordinator(state, id)?;
    let domain = record.domain.as_ref().expect("validated domain");
    let attempt = record
        .attempt
        .as_ref()
        .ok_or("coordinator attempt missing")?;
    let owner_home = PathBuf::from(record.owner_home.as_deref().ok_or("owner home missing")?);
    let owner_state = PathBuf::from(record.owner_state.as_deref().ok_or("owner state missing")?);
    let parent_home = PathBuf::from(record.parent_home.as_deref().ok_or("parent home missing")?);
    let parent_state = PathBuf::from(
        record
            .parent_state
            .as_deref()
            .ok_or("parent state missing")?,
    );
    for path in [&owner_home, &owner_state, &parent_home, &parent_state] {
        if !path.is_absolute() {
            return Err("coordinator route contains a non-canonical path".into());
        }
    }
    Ok(ParentRoute {
        task_id: record.task_id,
        owner_home,
        owner_state,
        parent_id: record.parent_id.ok_or("parent identity missing")?,
        parent_home,
        parent_state,
        root_id: record.root_id.ok_or("root identity missing")?,
        attempt_id: attempt.id.clone(),
        attempt_generation: attempt.generation,
        brief_revision: attempt.brief_revision,
        assignment_generation: domain.assignment_generation,
        scope_revision: domain.scope_revision,
    })
}

/// Revise a stopped coordinator's scope and optional project binding. The
/// current attempt/revision stays identifiable, but a live endpoint must first
/// be reconciled by the lifecycle owner so old execution cannot act on the new
/// generation.
// The explicit identity/revision/scope/project/idea/reason tuple is the
// operation's persisted concurrency contract; keep each field visible here.
#[allow(clippy::too_many_arguments)]
fn update_scope_ids(
    state: &Path,
    runtime_root: &Path,
    id: &str,
    expected_scope_revision: u64,
    scope: String,
    projects: Vec<String>,
    idea_id: Option<String>,
    reason: String,
) -> Result<TaskRecord, String> {
    if scope.trim().is_empty() || (projects.is_empty() && idea_id.is_none()) {
        return Err("domain requires explicit bounded scope and identity".into());
    }
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        state.join(format!(".{id}.identity.lock")),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())?;
    let path = state.join(format!("{id}.meta"));
    let before = multplx_core::filesystem::read_bounded_regular(&path, 4 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(before.clone()).map_err(|e| e.to_string())?;
    let mut record = super::subagent_model::read_meta(id, &text)?;
    let domain = record
        .domain
        .as_ref()
        .filter(|domain| domain.coordinator_id == id)
        .ok_or("task is not a canonical scoped coordinator")?
        .clone();
    if domain.scope_revision == expected_scope_revision.saturating_add(1)
        && domain.scope == scope
        && domain.projects == projects
        && domain.idea_id == idea_id
        && record
            .assignments
            .last()
            .is_some_and(|assignment| assignment.reason == reason)
    {
        return Ok(record);
    }
    if domain.scope_revision != expected_scope_revision {
        return Err("stale domain scope revision".into());
    }
    if record.runtime.endpoint.is_some() {
        return Err(
            "scope update requires the prior coordinator endpoint to be reconciled and stopped"
                .into(),
        );
    }
    let accepted = record
        .accepted_brief_revision
        .ok_or("accepted brief missing")?;
    let acceptance_criteria = record
        .briefs
        .last()
        .map(|brief| brief.acceptance_criteria.clone())
        .unwrap_or_default();
    let source_artifacts = record
        .briefs
        .last()
        .map(|brief| brief.source_artifacts.clone())
        .unwrap_or_default();
    record.revise_assignment(
        accepted,
        AssignmentRole::SubOrchestrator,
        scope.clone(),
        acceptance_criteria,
        source_artifacts,
        reason,
    )?;
    let generation = domain
        .assignment_generation
        .checked_add(1)
        .ok_or("domain assignment generation exhausted")?;
    let revision = expected_scope_revision
        .checked_add(1)
        .ok_or("domain scope revision exhausted")?;
    record.domain = Some(DomainBinding {
        domain_id: domain.domain_id.clone(),
        coordinator_id: id.to_owned(),
        scope_revision: revision,
        assignment_generation: generation,
        projects,
        idea_id,
        scope,
    });
    let domain = record.domain.as_ref().expect("new domain");
    let charter = super::brief::coordinator_charter(
        runtime_root,
        state,
        id,
        &domain.scope,
        &domain.projects,
        domain.idea_id.as_deref(),
        record.persistent,
    )
    .into_bytes();
    use sha2::{Digest, Sha256};
    record.accepted_brief_digest = Some(format!("{:x}", Sha256::digest(&charter)));
    let brief_path = state
        .join("brief-revisions")
        .join(format!("{id}-{}.md", accepted + 1));
    record.accepted_brief_path = Some(brief_path.to_string_lossy().into_owned());
    record.validate()?;
    let after = super::subagent_model::write_meta(&text, &record)?.into_bytes();
    let operation = format!("domain-scope-{id}-{revision}");
    if state
        .join(".transitions")
        .join(format!("{operation}.json"))
        .exists()
    {
        let writes = multplx_core::filesystem::read_transition_writes(state, &operation)
            .map_err(|e| e.to_string())?;
        if writes.last().is_none_or(|write| write.after != after) {
            return Err("domain scope update conflicts with retained intent".into());
        }
        multplx_core::filesystem::recover_transition(state, &operation)
            .map_err(|e| e.to_string())?;
    } else {
        std::fs::create_dir_all(state.join("brief-revisions")).map_err(|e| e.to_string())?;
        let prior_brief = std::fs::read(&brief_path).ok();
        if prior_brief.as_ref().is_some_and(|bytes| bytes != &charter) {
            return Err("domain charter revision conflicts with retained evidence".into());
        }
        recoverable_transition(
            state,
            &operation,
            &[
                TransitionWrite {
                    path: PathBuf::from("brief-revisions")
                        .join(format!("{id}-{}.md", accepted + 1)),
                    before: prior_brief,
                    after: charter.clone(),
                },
                TransitionWrite {
                    path: format!("{id}.meta").into(),
                    before: Some(before),
                    after,
                },
            ],
            None,
        )
        .map_err(|e| e.to_string())?;
    }
    let updated = read_coordinator(state, id)?;
    if let Some(home) = updated.persistent_home.as_deref() {
        // The accepted state revision above is authoritative for restart. Keep
        // the private-home compatibility copy aligned after that commit.
        let charter_path = Path::new(home).join("data/charter.md");
        std::fs::create_dir_all(charter_path.parent().expect("charter parent"))
            .map_err(|e| e.to_string())?;
        multplx_core::filesystem::atomic_replace(charter_path, &charter, 0o600).map_err(|e| {
            format!("scope committed but private charter copy needs reconciliation: {e}")
        })?;
    }
    Ok(updated)
}

/// Revise a domain using project bindings already resolved by the project
/// owner. This validates every canonical binding before changing authority.
#[allow(clippy::too_many_arguments)]
pub fn update_scope(
    state: &Path,
    owner_home: &Path,
    runtime_root: &Path,
    id: &str,
    expected_scope_revision: u64,
    scope: String,
    projects: &[crate::project_registry::ProjectBinding],
    idea_id: Option<String>,
    reason: String,
) -> Result<TaskRecord, String> {
    TaskId::parse(id).map_err(|e| e.to_string())?;
    let current = read_coordinator(state, id)?;
    let current_domain = current.domain.as_ref().expect("validated domain");
    if current_domain.scope_revision != expected_scope_revision
        || current.runtime.endpoint.is_some()
    {
        return update_scope_ids(
            state,
            runtime_root,
            id,
            expected_scope_revision,
            scope,
            projects
                .iter()
                .map(|project| project.project_id.clone())
                .collect(),
            idea_id,
            reason,
        );
    }
    for project in projects {
        crate::project_registry::validate_binding(owner_home, project)?;
    }
    if let Some(private_home) = current.persistent_home.as_deref() {
        let private_home = Path::new(private_home);
        for project in projects {
            let borrowed = crate::project_registry::bind_project_at(
                private_home,
                &private_home.join("data"),
                &private_home.join("projects"),
                &project.canonical_path,
            )?;
            if borrowed.project_id != project.project_id
                || borrowed.common_git_identity != project.common_git_identity
            {
                return Err(
                    "private-home project reference conflicts with canonical binding".into(),
                );
            }
        }
    }
    update_scope_ids(
        state,
        runtime_root,
        id,
        expected_scope_revision,
        scope,
        projects
            .iter()
            .map(|project| project.project_id.clone())
            .collect(),
        idea_id,
        reason,
    )
}

/// Add a project to an idea domain through the same fenced scope revision.
pub fn bind_project(
    state: &Path,
    owner_home: &Path,
    runtime_root: &Path,
    id: &str,
    expected_scope_revision: u64,
    project: &crate::project_registry::ProjectBinding,
    reason: String,
) -> Result<TaskRecord, String> {
    TaskId::parse(id).map_err(|e| e.to_string())?;
    crate::project_registry::validate_binding(owner_home, project)?;
    let current = read_coordinator(state, id)?;
    let domain = current.domain.clone().expect("validated domain");
    if domain.scope_revision == expected_scope_revision.saturating_add(1)
        && domain.projects.contains(&project.project_id)
        && current
            .assignments
            .last()
            .is_some_and(|assignment| assignment.reason == reason)
    {
        return Ok(current);
    }
    if domain.scope_revision != expected_scope_revision {
        return Err("stale domain scope revision".into());
    }
    if current.runtime.endpoint.is_some() {
        return Err(
            "scope update requires the prior coordinator endpoint to be reconciled and stopped"
                .into(),
        );
    }
    let mut projects = domain.projects;
    if projects.contains(&project.project_id) {
        return Ok(current);
    }
    if let Some(private_home) = current.persistent_home.as_deref() {
        let private_home = Path::new(private_home);
        let borrowed = crate::project_registry::bind_project_at(
            private_home,
            &private_home.join("data"),
            &private_home.join("projects"),
            &project.canonical_path,
        )?;
        if borrowed.project_id != project.project_id
            || borrowed.common_git_identity != project.common_git_identity
        {
            return Err("private-home project reference conflicts with canonical binding".into());
        }
    }
    projects.push(project.project_id.clone());
    update_scope_ids(
        state,
        runtime_root,
        id,
        expected_scope_revision,
        domain.scope,
        projects,
        domain.idea_id,
        reason,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn coordinator(state: &Path) -> TaskRecord {
        let owner = state.parent().unwrap().to_string_lossy().into_owned();
        let mut record = TaskRecord::new(
            "coord".into(),
            AssignmentRole::SubOrchestrator,
            ArtifactKind::Coordination,
            false,
            format!("root-home:{owner}"),
            format!("root-home:{owner}"),
            owner,
        );
        record.owner_state = Some(state.to_string_lossy().into_owned());
        record.parent_state = record.owner_state.clone();
        record.private_home = true;
        record.persistent_home = Some(
            state
                .parent()
                .unwrap()
                .join("private")
                .to_string_lossy()
                .into_owned(),
        );
        record.domain = Some(DomainBinding {
            domain_id: "domain".into(),
            coordinator_id: "coord".into(),
            scope_revision: 1,
            assignment_generation: 1,
            projects: Vec::new(),
            idea_id: Some("idea".into()),
            scope: "research the idea".into(),
        });
        record.owning_coordinator = Some("coord".into());
        record
    }

    fn write_coordinator(state: &Path, record: &TaskRecord) {
        fs::create_dir_all(state).unwrap();
        let text = super::super::subagent_model::write_meta("kind=daemon\n", record).unwrap();
        fs::write(state.join("coord.meta"), text).unwrap();
    }

    fn git_project(base: &Path) -> PathBuf {
        let project = base.join("project");
        fs::create_dir(&project).unwrap();
        for arguments in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["config", "user.name", "Domain Test"],
        ] {
            assert!(
                Command::new("git")
                    .args(arguments)
                    .current_dir(&project)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(project.join("README.md"), "project\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "README.md"])
                .current_dir(&project)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-q", "-m", "initial"])
                .current_dir(&project)
                .status()
                .unwrap()
                .success()
        );
        project
    }

    #[test]
    fn route_and_scope_update_are_generation_bound() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let record = coordinator(&state);
        let text = super::super::subagent_model::write_meta("kind=daemon\n", &record).unwrap();
        fs::write(state.join("coord.meta"), text).unwrap();
        let route = validated_parent_route(&state, "coord").unwrap();
        assert_eq!((route.assignment_generation, route.scope_revision), (1, 1));
        let updated = update_scope_ids(
            &state,
            temp.path(),
            "coord",
            1,
            "research and implementation".into(),
            vec!["project-1".into()],
            Some("idea".into()),
            "repository selected".into(),
        )
        .unwrap();
        let domain = updated.domain.unwrap();
        assert_eq!(
            (domain.assignment_generation, domain.scope_revision),
            (2, 2)
        );
        assert_eq!(domain.projects, ["project-1"]);
        assert!(
            update_scope_ids(
                &state,
                temp.path(),
                "coord",
                1,
                "stale".into(),
                vec!["project-2".into()],
                Some("idea".into()),
                "stale".into()
            )
            .is_err()
        );
    }

    #[test]
    fn live_coordinator_must_be_stopped_before_scope_revision() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let mut record = coordinator(&state);
        record.runtime.endpoint = Some("live".into());
        let text = super::super::subagent_model::write_meta("kind=daemon\n", &record).unwrap();
        fs::write(state.join("coord.meta"), text).unwrap();
        assert!(
            update_scope_ids(
                &state,
                temp.path(),
                "coord",
                1,
                "changed".into(),
                Vec::new(),
                Some("idea".into()),
                "reason".into(),
            )
            .unwrap_err()
            .contains("stopped")
        );
    }

    #[test]
    fn coordinator_reads_and_scope_revisions_fail_closed_without_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        let record = coordinator(&state);
        write_coordinator(&state, &record);
        let original = fs::read(state.join("coord.meta")).unwrap();

        assert!(read_coordinator(&state, "bad/id").is_err());
        assert!(
            read_coordinator(&state, "missing")
                .unwrap_err()
                .contains("cannot read")
        );

        let mut ordinary = TaskRecord::new(
            "ordinary".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            record.root_id.clone().unwrap(),
            record.root_id.clone().unwrap(),
            record.owner_home.clone().unwrap(),
        );
        ordinary.owner_state = Some(state.to_string_lossy().into_owned());
        ordinary.parent_state = ordinary.owner_state.clone();
        let text = super::super::subagent_model::write_meta("kind=delivery\n", &ordinary).unwrap();
        fs::write(state.join("ordinary.meta"), text).unwrap();
        assert!(
            read_coordinator(&state, "ordinary")
                .unwrap_err()
                .contains("not a canonical scoped coordinator")
        );

        for (scope, projects, idea) in [
            ("   ", vec!["project".into()], Some("idea".into())),
            ("bounded", Vec::new(), None),
        ] {
            assert!(
                update_scope_ids(
                    &state,
                    temp.path(),
                    "coord",
                    1,
                    scope.into(),
                    projects,
                    idea,
                    "invalid revision".into(),
                )
                .unwrap_err()
                .contains("explicit bounded scope")
            );
        }
        assert_eq!(fs::read(state.join("coord.meta")).unwrap(), original);

        fs::create_dir_all(state.join("brief-revisions")).unwrap();
        fs::write(
            state.join("brief-revisions/coord-2.md"),
            "foreign charter\n",
        )
        .unwrap();
        assert!(
            update_scope_ids(
                &state,
                temp.path(),
                "coord",
                1,
                "bounded revision".into(),
                Vec::new(),
                Some("idea".into()),
                "accepted revision".into(),
            )
            .unwrap_err()
            .contains("retained evidence")
        );
        assert_eq!(fs::read(state.join("coord.meta")).unwrap(), original);
    }

    #[test]
    fn exact_scope_retry_and_project_binding_preserve_one_revision_and_private_catalog() {
        let temp = tempfile::tempdir().unwrap();
        let owner = temp.path().join("owner");
        let state = owner.join("state");
        let record = coordinator(&state);
        write_coordinator(&state, &record);
        let project = git_project(temp.path());
        let binding = crate::project_registry::bind_project_at(
            &owner,
            &owner.join("data"),
            &owner.join("projects"),
            &project,
        )
        .unwrap();

        let updated = bind_project(
            &state,
            &owner,
            temp.path(),
            "coord",
            1,
            &binding,
            "repository selected".into(),
        )
        .unwrap();
        let domain = updated.domain.as_ref().unwrap();
        assert_eq!(domain.scope_revision, 2);
        assert_eq!(domain.assignment_generation, 2);
        assert_eq!(
            domain.projects.as_slice(),
            std::slice::from_ref(&binding.project_id)
        );
        let private = Path::new(updated.persistent_home.as_deref().unwrap());
        assert!(crate::project_registry::resolve_checkout(private, &binding.project_id).is_ok());
        assert!(private.join("data/charter.md").is_file());

        let retried = bind_project(
            &state,
            &owner,
            temp.path(),
            "coord",
            1,
            &binding,
            "repository selected".into(),
        )
        .unwrap();
        assert_eq!(retried, updated);
        let already_bound = bind_project(
            &state,
            &owner,
            temp.path(),
            "coord",
            2,
            &binding,
            "different retry reason".into(),
        )
        .unwrap();
        assert_eq!(already_bound, updated);

        let revised = update_scope(
            &state,
            &owner,
            temp.path(),
            "coord",
            2,
            "implement only the selected component".into(),
            std::slice::from_ref(&binding),
            None,
            "scope narrowed".into(),
        )
        .unwrap();
        assert_eq!(revised.domain.as_ref().unwrap().scope_revision, 3);
        let exact_retry = update_scope(
            &state,
            &owner,
            temp.path(),
            "coord",
            2,
            "implement only the selected component".into(),
            std::slice::from_ref(&binding),
            None,
            "scope narrowed".into(),
        )
        .unwrap();
        assert_eq!(exact_retry, revised);
        assert_eq!(revised.assignments.len(), 3);
    }

    #[test]
    fn project_binding_rejects_stale_live_and_foreign_project_authority() {
        let temp = tempfile::tempdir().unwrap();
        let owner = temp.path().join("owner");
        let state = owner.join("state");
        let mut record = coordinator(&state);
        write_coordinator(&state, &record);
        let project = git_project(temp.path());
        let foreign = temp.path().join("foreign");
        let binding = crate::project_registry::bind_project_at(
            &foreign,
            &foreign.join("data"),
            &foreign.join("projects"),
            &project,
        )
        .unwrap();
        assert!(
            bind_project(
                &state,
                &owner,
                temp.path(),
                "coord",
                1,
                &binding,
                "not owner registered".into(),
            )
            .unwrap_err()
            .contains("unknown project identity")
        );

        let binding = crate::project_registry::bind_project_at(
            &owner,
            &owner.join("data"),
            &owner.join("projects"),
            &project,
        )
        .unwrap();
        assert!(
            bind_project(
                &state,
                &owner,
                temp.path(),
                "coord",
                0,
                &binding,
                "stale".into(),
            )
            .unwrap_err()
            .contains("stale")
        );

        record.runtime.endpoint = Some("live:coord".into());
        write_coordinator(&state, &record);
        assert!(
            bind_project(
                &state,
                &owner,
                temp.path(),
                "coord",
                1,
                &binding,
                "live".into(),
            )
            .unwrap_err()
            .contains("stopped")
        );
    }

    #[test]
    fn interrupted_scope_transition_recovers_the_exact_accepted_revision() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        let record = coordinator(&state);
        write_coordinator(&state, &record);
        let before = fs::read(state.join("coord.meta")).unwrap();
        let scope = "recover the exact bounded revision";
        let reason = "accepted before interruption";
        let updated = update_scope_ids(
            &state,
            temp.path(),
            "coord",
            1,
            scope.into(),
            Vec::new(),
            Some("idea".into()),
            reason.into(),
        )
        .unwrap();
        let receipt_path = state.join(".transitions/domain-scope-coord-2.json");
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        receipt["committed"] = false.into();
        receipt["progress"] = 1.into();
        fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        fs::write(state.join("coord.meta"), before).unwrap();

        let recovered = update_scope_ids(
            &state,
            temp.path(),
            "coord",
            1,
            scope.into(),
            Vec::new(),
            Some("idea".into()),
            reason.into(),
        )
        .unwrap();
        assert_eq!(recovered, updated);
        let final_receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(receipt_path).unwrap()).unwrap();
        assert_eq!(final_receipt["committed"], true);
        assert_eq!(final_receipt["progress"], 2);
    }
}

//! Conservative installer cleanup: preview, bounded backups, exact recheck and
//! canonical lifecycle/wake/channel/PR owners. Never launches a harness.
use multplx_core::{
    filesystem::{atomic_replace, read_bounded_regular},
    locks::DirectoryLock,
    process::SystemProcessProbe,
    session_lock::MaintenanceGuard,
    wake::{WakeQueue, WakeRecord},
};
use multplx_domain::lifecycle::{
    parent_channel, subagent_model, teardown, upgrade_cleanup as evidence,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Plan {
    task_ids: BTreeSet<String>,
    pr_artifacts: BTreeMap<PathBuf, Vec<u8>>,
    tasks: BTreeMap<String, Vec<u8>>,
    retired_intents: BTreeMap<String, crate::retired_intent::Proof>,
    snapshots: BTreeMap<PathBuf, Vec<u8>>,
    retained: BTreeMap<String, String>,
    packets: usize,
    wakes: usize,
    checks: usize,
    holds: BTreeSet<String>,
    report_repairs: usize,
}
impl Plan {
    pub(crate) fn actionable(&self) -> bool {
        !self.retired_intents.is_empty()
            || !self.tasks.is_empty()
            || self.packets > 0
            || self.wakes > 0
            || self.checks > 0
            || !self.holds.is_empty()
            || self.report_repairs > 0
    }
    pub(crate) fn has_findings(&self) -> bool {
        self.actionable() || !self.retained.is_empty()
    }
    pub(crate) fn preview(&self) {
        eprintln!(
            "Upgrade stale-state cleanup: {} completed private homes to archive, {} settled local outcomes, {} obsolete wakes, {} stale PR artifacts to quarantine.",
            self.tasks.len(),
            self.packets,
            self.wakes,
            self.checks
        );
        if self.report_repairs > 0 {
            eprintln!(
                "  REPAIR {} accepted completed report notification(s) with stable wake identity",
                self.report_repairs
            );
        }
        for task in &self.holds {
            eprintln!(
                "  HOLD completed backlog item {task}; prevent redispatch without inventing a new completion"
            );
        }
        for task in self.retired_intents.keys() {
            eprintln!(
                "  QUARANTINE retired task {task} unsubmitted spawn intent; retain exact retirement and launch evidence"
            );
        }
        for task in self.tasks.keys() {
            eprintln!(
                "  ARCHIVE completed task {task}; retain private bytes, task evidence and project worktrees"
            );
        }
        for (task, reason) in &self.retained {
            eprintln!("  RETAIN {task}: {reason}");
        }
    }
}

fn context(root: &Path, home: &Path) -> teardown::Context {
    teardown::Context {
        root: root.into(),
        home: home.into(),
        state: home.join("state"),
        data: home.join("data"),
    }
}
fn snapshot(plan: &mut Plan, path: &Path) -> Result<(), String> {
    let bytes = read_bounded_regular(path, 16 * 1024 * 1024).map_err(|e| e.to_string())?;
    plan.snapshots.insert(path.into(), bytes);
    Ok(())
}
fn settled_wake(state: &Path, record: &WakeRecord) -> bool {
    if let Some(message) = record.key.strip_prefix("message-") {
        return multplx_domain::operational_input::read_message_envelope(state, message)
            .is_ok_and(|event| parent_channel::maintenance_event_settled(state, &event));
    }
    (record.key.ends_with(".status") || record.key.ends_with(".turn-ended"))
        && record
            .task_target()
            .is_some_and(|task| evidence::completed_record(state, task).is_ok())
}

fn settled_wake_for_tasks(state: &Path, record: &WakeRecord, task_ids: &BTreeSet<String>) -> bool {
    let task = if let Some(message) = record.key.strip_prefix("message-") {
        multplx_domain::operational_input::read_message_envelope(state, message)
            .ok()
            .map(|event| event.task_id)
    } else {
        record.task_target().map(str::to_owned)
    };
    task.is_some_and(|task| task_ids.contains(&task)) && settled_wake(state, record)
}

pub(crate) fn inspect(root: &Path, home: &Path) -> Result<Plan, String> {
    let ctx = context(root, home);
    let state = &ctx.state;
    let mut plan = Plan::default();
    if !state.exists() {
        return Ok(plan);
    }
    evidence::private_directory(state, false)?;
    let mut dependency_targets = BTreeSet::new();
    for (index, entry) in fs::read_dir(state).map_err(|e| e.to_string())?.enumerate() {
        if index >= 4096 {
            return Err("upgrade state exceeds bounded 4096-entry preview; retained".into());
        }
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("meta") {
            continue;
        }
        let task = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("invalid task filename")?;
        multplx_core::identifiers::TaskId::parse(task).map_err(|e| e.to_string())?;
        plan.task_ids.insert(task.into());
        let bytes = read_bounded_regular(&path, subagent_model::MAX_TASK_METADATA_BYTES)
            .map_err(|e| e.to_string())?;
        let record = subagent_model::read_meta(
            task,
            std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
        );
        if let Ok(record) = &record
            && record.schedule.state != subagent_model::WorkState::Completed
        {
            dependency_targets.extend(record.schedule.dependencies.iter().cloned());
        }
        let correspondence = multplx_domain::lifecycle::pending_reply::maintenance_retirement_clear(
            state,
            Some(task),
        );
        let reason = match (record, correspondence) {
            (Ok(_), Err(reason)) => Some(reason),
            (Ok(record), Ok(())) if evidence::completed(&record, state) => {
                match teardown::inspect_completed_upgrade_retirement(&ctx, task).and_then(|child| {
                    crate::launcher::verify_cleanup_home_quiescent(root, home, &child, task)?;
                    if let Some(endpoint) = &record.runtime.endpoint {
                        let observation = multplx_backend::facade::observe_endpoint_for_home(
                            &record.runtime.provider,
                            endpoint,
                            Some(format!("mx-{task}")),
                            root,
                            record
                                .persistent_home
                                .as_deref()
                                .map(Path::new)
                                .unwrap_or(home),
                        )
                        .map_err(|e| e.to_string())?;
                        if observation.exists {
                            return Err("live endpoint retained".into());
                        }
                    }
                    Ok(())
                }) {
                    Ok(()) => {
                        plan.tasks.insert(task.into(), bytes.clone());
                        plan.snapshots.insert(path.clone(), bytes.clone());
                        None
                    }
                    Err(reason) => Some(reason),
                }
            }
            (Ok(_), Ok(())) => Some(
                "unfinished, unanswered or ambiguous canonical state; inspect before recovery"
                    .into(),
            ),
            (Err(error), _) => Some(format!("unreadable canonical state: {error}")),
        };
        if let Some(reason) = reason {
            plan.retained.insert(task.into(), reason);
        }
    }
    for entry in fs::read_dir(state).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if let Some(task) = path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix(".spawn-"))
            .and_then(|s| s.strip_suffix(".intent"))
            && fs::symlink_metadata(state.join(format!("{task}.meta"))).is_err()
        {
            let proof = crate::retired_intent::inspect(root, home, task)?;
            plan.snapshots
                .extend(crate::retired_intent::snapshots(&proof).clone());
            snapshot(&mut plan, &path)?;
            plan.task_ids.insert(task.into());
            plan.retired_intents.insert(task.into(), proof);
        }
    }
    let (requests, total) =
        multplx_domain::operational_input::RequestStore::new(state, home).observe(1024)?;
    if total > 1024 {
        return Err("accepted requests exceed bounded upgrade preview; retained".into());
    }
    for request in requests
        .into_iter()
        .filter(|request| request.completion.is_none())
    {
        dependency_targets.extend(request.dependencies);
        dependency_targets.insert(request.task_id);
    }
    for task in plan.tasks.keys().cloned().collect::<Vec<_>>() {
        let qualified = subagent_model::qualified_task_id(&home.to_string_lossy(), &task);
        if dependency_targets.contains(&task) || dependency_targets.contains(&qualified) {
            plan.tasks.remove(&task);
            plan.retained.insert(
                task,
                "current task or accepted request still depends on canonical completion; retained"
                    .into(),
            );
        }
    }
    let retirements = state.join(".upgrade-retirements");
    if retirements.exists() {
        for entry in fs::read_dir(&retirements).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let task = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("invalid retirement filename")?;
            let receipt = evidence::read_retirement(state, task)?;
            plan.task_ids.insert(task.into());
            if !receipt.committed {
                let record = subagent_model::read_meta(
                    task,
                    std::str::from_utf8(&receipt.original_meta).map_err(|e| e.to_string())?,
                )?;
                if !evidence::completed(&record, state) {
                    plan.tasks.remove(task);
                    plan.retained.insert(
                        task.into(),
                        "prepared retirement has pending or ambiguous correspondence; retained"
                            .into(),
                    );
                    continue;
                }
                let allocation =
                    multplx_domain::lifecycle::home_seed::read_home_allocation(&ctx.data, task)?
                        .ok_or("prepared allocation missing")?;
                let private =
                    multplx_domain::lifecycle::home_seed::inspect_retirement_home(&allocation)?;
                if let Err(reason) =
                    multplx_domain::lifecycle::pending_reply::maintenance_retirement_clear(
                        &private.join("state"),
                        None,
                    )
                {
                    plan.tasks.remove(task);
                    plan.retained.insert(task.into(), reason);
                    continue;
                }
                if dependency_targets.contains(task)
                    || dependency_targets.contains(&subagent_model::qualified_task_id(
                        &home.to_string_lossy(),
                        task,
                    ))
                {
                    plan.tasks.remove(task);
                    plan.retained.insert(
                        task.into(),
                        "prepared retirement has a new current dependency; retained".into(),
                    );
                    continue;
                }
                let meta = state.join(format!("{task}.meta"));
                if meta.exists()
                    && read_bounded_regular(&meta, subagent_model::MAX_TASK_METADATA_BYTES)
                        .map_err(|e| e.to_string())?
                        != receipt.original_meta
                {
                    plan.tasks.remove(task);
                    plan.retained.insert(task.into(), "prepared retirement belongs to an older task revision; current work retained".into());
                    continue;
                }
                plan.tasks.insert(task.into(), receipt.original_meta);
                plan.retained.remove(task);
                snapshot(&mut plan, &path)?;
            }
        }
    }
    let backlog = ctx.data.join("backlog.md");
    if backlog.exists() {
        let store = multplx_domain::backlog::BacklogStore::new(&backlog);
        for task in plan.tasks.keys() {
            if store
                .snapshot(task)
                .is_ok_and(|item| item.state == "queued" && !item.held)
            {
                if multplx_domain::backlog::backend_value(home.join("config").as_path()) == "manual"
                {
                    plan.retained.insert(format!("backlog/{task}"), "manual backlog still queues completed task; explicitly hold it before redispatch".into());
                } else {
                    plan.holds.insert(task.clone());
                }
            }
        }
        if !plan.holds.is_empty() {
            snapshot(&mut plan, &backlog)?;
        }
    }
    for path in multplx_domain::supervision::completed_report_repairs(state)? {
        snapshot(&mut plan, &path)?;
        plan.report_repairs += 1;
    }
    for path in parent_channel::settled_local_outcomes(state)? {
        snapshot(&mut plan, &path)?;
        plan.packets += 1;
    }
    for (path, reason) in parent_channel::retained_maintenance_outcomes(state)? {
        plan.retained.insert(path, reason);
    }
    let queue = state.join(".wake-queue");
    if queue.exists() {
        snapshot(&mut plan, &queue)?;
        for line in std::str::from_utf8(&plan.snapshots[&queue])
            .map_err(|e| e.to_string())?
            .lines()
        {
            let record = WakeRecord::parse(line).map_err(|e| e.to_string())?;
            plan.wakes += usize::from(settled_wake(state, &record));
        }
    }
    for item in WakeQueue::new(state)
        .observe_inbox_items()
        .map_err(|e| e.to_string())?
    {
        snapshot(
            &mut plan,
            &state
                .join("wake-inbox")
                .join(format!("{}.json", item.event_id)),
        )?;
        plan.wakes +=
            usize::from(item.acknowledged_at.is_none() && settled_wake(state, &item.record));
    }
    for path in crate::review::dangling_pr_artifacts(state)? {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid poll filename")?;
        let task = [".check.sh", ".pr-poll", ".pr-poll-registration"]
            .into_iter()
            .find_map(|suffix| name.strip_suffix(suffix))
            .ok_or("invalid poll namespace")?;
        multplx_core::identifiers::TaskId::parse(task).map_err(|e| e.to_string())?;
        plan.task_ids.insert(task.into());
        snapshot(&mut plan, &path)?;
        plan.pr_artifacts
            .insert(path.clone(), plan.snapshots[&path].clone());
    }
    for name in ["agents.md", "daemons.md"] {
        let path = ctx.data.join(name);
        if path.exists() {
            snapshot(&mut plan, &path)?;
        }
    }
    for task in plan.tasks.keys().cloned().collect::<Vec<_>>() {
        snapshot(
            &mut plan,
            &ctx.data.join(format!(".home-allocation-{task}.json")),
        )?;
        for suffix in [
            "status",
            "turn-ended",
            "pi-ext.ts",
            "journal",
            "check.sh",
            "pr-poll",
            "pr-poll-registration",
            "pr-poll-retirement",
            "check-trust",
        ] {
            let path = state.join(format!("{task}.{suffix}"));
            if path.exists() {
                snapshot(&mut plan, &path)?;
            }
        }
        // These existing PR files become dangling through the approved
        // retirement itself. Include their exact bytes now, never by rescan.
        if ["pr-poll", "pr-poll-registration"]
            .into_iter()
            .any(|suffix| {
                plan.snapshots
                    .contains_key(&state.join(format!("{task}.{suffix}")))
            })
        {
            for suffix in ["check.sh", "pr-poll", "pr-poll-registration"] {
                let path = state.join(format!("{task}.{suffix}"));
                if let Some(bytes) = plan.snapshots.get(&path) {
                    plan.pr_artifacts.insert(path, bytes.clone());
                }
            }
        }
        let path = state.join(format!(".teardown.transaction.{task}"));
        if path.exists() {
            snapshot(&mut plan, &path)?;
        }
    }
    plan.checks = plan.pr_artifacts.len();
    Ok(plan)
}

pub(crate) fn apply(root: &Path, home: &Path, approved: &Plan) -> Result<(), String> {
    let ctx = context(root, home);
    let state = &ctx.state;
    let processes = SystemProcessProbe::default();
    let guard = MaintenanceGuard::acquire(state, &processes).map_err(|e| e.to_string())?;
    let _watch = DirectoryLock::try_acquire(state.join(".watch.lock"), &processes)
        .map_err(|e| format!("watcher owner retained: {e}"))?;
    let _launch = DirectoryLock::try_acquire(state.join(".workspace-launch.lock"), &processes)
        .map_err(|e| e.to_string())?;
    let mut task_locks = Vec::new();
    let mut child_guards = Vec::new();
    // Nonblocking locks use the lifecycle's teardown-before-identity order.
    // Current, prepared/committed retirement and dangling artifact identities
    // all participate: even metadata-absent repair must exclude a new writer.
    for task in &approved.task_ids {
        for name in [
            format!(".teardown.{task}.lock"),
            format!(".{task}.identity.lock"),
        ] {
            task_locks.push(
                DirectoryLock::try_acquire(state.join(name), &processes)
                    .map_err(|e| e.to_string())?,
            );
        }
    }
    for task in approved.retired_intents.keys() {
        task_locks.push(
            DirectoryLock::try_acquire(state.join(format!(".spawn-{task}.lock")), &processes)
                .map_err(|e| e.to_string())?,
        );
    }
    for path in approved
        .snapshots
        .keys()
        .filter(|p| p.parent() == Some(state.join(".spawn-actions").as_path()))
    {
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("invalid spawn action filename")?;
        task_locks.push(
            DirectoryLock::try_acquire(state.join(format!(".spawn-action-{id}.lock")), &processes)
                .map_err(|e| e.to_string())?,
        );
    }
    for proof in approved.retired_intents.values() {
        let archive = crate::retired_intent::archive(proof);
        child_guards.push(
            MaintenanceGuard::acquire_anchored(&archive.join("state"), state, &processes)
                .map_err(|e| e.to_string())?,
        );
        for name in [".watch.lock", ".workspace-launch.lock"] {
            task_locks.push(
                DirectoryLock::try_acquire_anchored(
                    &archive.join("state").join(name),
                    state,
                    &processes,
                )
                .map_err(|e| e.to_string())?,
            );
        }
    }
    for (task, bytes) in &approved.tasks {
        let record = subagent_model::read_meta(
            task,
            std::str::from_utf8(bytes).map_err(|e| e.to_string())?,
        )?;
        if let Some(token) = &record.home_allocation {
            let allocation =
                multplx_domain::lifecycle::home_seed::read_home_allocation(&ctx.data, task)?
                    .ok_or("prepared home allocation missing")?;
            if allocation.binding != *token {
                return Err("private home lease changed after cleanup preview; retained".into());
            }
            let private_home =
                Some(multplx_domain::lifecycle::home_seed::inspect_retirement_home(&allocation)?);
            if let Some(private_home) = private_home {
                child_guards.push(
                    MaintenanceGuard::acquire_anchored(
                        &private_home.join("state"),
                        state,
                        &processes,
                    )
                    .map_err(|e| e.to_string())?,
                );
                for name in [".watch.lock", ".workspace-launch.lock"] {
                    task_locks.push(
                        DirectoryLock::try_acquire_anchored(
                            &private_home.join("state").join(name),
                            state,
                            &processes,
                        )
                        .map_err(|e| e.to_string())?,
                    );
                }
            }
        }
    }
    if inspect(root, home)? != *approved {
        return Err("cleanup state changed after preview; retained, rerun upgrade".into());
    }
    let bytes = serde_json::to_vec(approved).map_err(|e| e.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let audit = state.join(".upgrade-cleanup").join(&digest);
    evidence::private_directory(&state.join(".upgrade-cleanup"), true)?;
    evidence::private_directory(&audit, true)?;
    atomic_replace(audit.join("backup.json"), &bytes, 0o600).map_err(|e| e.to_string())?;
    for proof in approved.retired_intents.values() {
        crate::retired_intent::quarantine(root, home, proof)?;
    }
    for task in &approved.holds {
        multplx_domain::backlog::BacklogStore::new(ctx.data.join("backlog.md"))
            .hold(
                task,
                "upgrade retained completed historical task for explicit redispatch review",
                "upgrade-completed",
            )
            .map_err(|e| e.to_string())?;
    }
    // Local outcomes first produce stable notifications; retirement never
    // discards an undelivered private-home obligation or a foreign packet.
    multplx_domain::supervision::repair_completed_report_wakes(state, &guard, &approved.task_ids)?;
    parent_channel::settle_local_outcomes(state, &guard, &approved.task_ids)?;
    WakeQueue::new(state)
        .settle_maintenance(&guard, &processes, SystemTime::now(), |record| {
            settled_wake_for_tasks(state, record, &approved.task_ids)
        })
        .map_err(|e| e.to_string())?;
    for (task, original_meta) in &approved.tasks {
        let mut receipt = evidence::Retirement {
            schema: "mx-upgrade-retirement.v1".into(),
            task: task.clone(),
            original_meta: original_meta.clone(),
            committed: false,
        };
        evidence::write_retirement(state, &receipt)?;
        let result = teardown::retire_completed_for_upgrade(&ctx, task, original_meta);
        // Exclusions travel with the atomic home archive; their owner remains
        // outside it, and exact release leaves the private tree byte-identical.
        if let Some(allocation) =
            multplx_domain::lifecycle::home_seed::read_home_allocation(&ctx.data, task)?
            && let Some(archive) = allocation.retained_path
            && archive.exists()
            && !allocation.binding.path.exists()
        {
            for guard in &mut child_guards {
                guard
                    .follow_directory_move(&allocation.binding.path, &archive)
                    .map_err(|e| e.to_string())?;
            }
            for lock in &mut task_locks {
                lock.follow_directory_move(&allocation.binding.path, &archive)
                    .map_err(|e| e.to_string())?;
            }
        }
        result?;
        receipt.committed = true;
        evidence::write_retirement(state, &receipt)?;
    }
    crate::review::quarantine_dangling_pr_artifacts(state, &approved.pr_artifacts)?;
    atomic_replace(audit.join("complete"), b"completed\n", 0o600).map_err(|e| e.to_string())?;
    eprintln!(
        "Upgrade cleanup complete; private backup and receipts: {}",
        audit.display()
    );
    // Keep exclusion and lifecycle claims through the complete receipt.
    drop((task_locks, child_guards));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use multplx_domain::lifecycle::{
        home_seed::{HomeAllocation, HomeBinding},
        subagent_model::{ArtifactKind, AssignmentRole, HumanDecision, TaskRecord, WorkState},
    };
    use std::os::unix::fs::MetadataExt;

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        home: PathBuf,
        private: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let base = fs::canonicalize(temp.path()).unwrap();
            let root = base.join("runtime");
            let home = base.join("owner");
            let private = base.join("private");
            for path in [
                &root,
                &home.join("state"),
                &home.join("data"),
                &private.join("state"),
                &private.join("data"),
                &private.join("config"),
                &private.join("projects"),
            ] {
                fs::create_dir_all(path).unwrap();
            }
            fs::write(private.join(multplx_core::agent_home::MARKER), "done\n").unwrap();
            fs::write(private.join("unfinished.txt"), "uncommitted private bytes").unwrap();
            let token = HomeBinding {
                id: "done".into(),
                owner_home: home.clone(),
                path: private.clone(),
                lease_id: "lease-done".into(),
                generation: 1,
            };
            let metadata = fs::metadata(&private).unwrap();
            let allocation = HomeAllocation {
                version: 1,
                binding: token.clone(),
                runtime_root: root.clone(),
                state: "active".into(),
                retained_path: None,
                directory_identity: Some((metadata.dev(), metadata.ino())),
                git_allocation: None,
            };
            fs::write(
                home.join("data/.home-allocation-done.json"),
                serde_json::to_vec(&allocation).unwrap(),
            )
            .unwrap();
            fs::write(
                home.join("data/agents.md"),
                format!(
                    "- done - researcher (home: {}; route: local)\n",
                    private.display()
                ),
            )
            .unwrap();
            let mut record = TaskRecord::new(
                "done".into(),
                AssignmentRole::Researcher,
                ArtifactKind::Report,
                true,
                "ROOT".into(),
                "ROOT".into(),
                home.to_string_lossy().into_owned(),
            );
            record.persistent_home = Some(private.to_string_lossy().into_owned());
            record.home_allocation = Some(token);
            record.schedule.state = WorkState::Completed;
            fs::write(
                home.join("state/done.meta"),
                subagent_model::write_meta(
                    &format!("kind=daemon\nhome={}\n", private.display()),
                    &record,
                )
                .unwrap(),
            )
            .unwrap();
            Self {
                _temp: temp,
                root,
                home,
                private,
            }
        }
        fn state(&self) -> PathBuf {
            self.home.join("state")
        }
        fn record(&self) -> TaskRecord {
            subagent_model::read_meta(
                "done",
                &fs::read_to_string(self.state().join("done.meta")).unwrap(),
            )
            .unwrap()
        }
        fn edit(&self, mutator: impl FnOnce(&mut TaskRecord)) {
            let path = self.state().join("done.meta");
            let raw = fs::read_to_string(&path).unwrap();
            let mut record = self.record();
            mutator(&mut record);
            fs::write(path, subagent_model::write_meta(&raw, &record).unwrap()).unwrap();
        }
        fn envelope(&self, id: &str, kind: &str) -> subagent_model::MessageEnvelope {
            let record = self.record();
            subagent_model::MessageEnvelope {
                schema_version: subagent_model::SCHEMA_VERSION,
                message_id: id.into(),
                task_id: "done".into(),
                task_home: record.owner_home.clone(),
                parent_home: Some(self.home.to_string_lossy().into_owned()),
                attempt: record.attempt.clone(),
                parent_id: record.parent_id.clone(),
                sender: "done".into(),
                recipient: "ROOT".into(),
                brief_revision: record.accepted_brief_revision,
                kind: kind.into(),
                correlation_id: id.into(),
                created_at: "2026-10-13T00:00:00Z".into(),
                summary: "accepted outcome".into(),
                artifact: None,
                acknowledgement: subagent_model::Acknowledgement::Pending,
                automatic_wake: Some(true),
            }
        }
        fn wake(&self, key: &str, payload: &str) {
            WakeQueue::new(self.state())
                .append_once(
                    multplx_core::wake::WakeKind::Signal,
                    key,
                    payload,
                    SystemTime::now(),
                    &SystemProcessProbe::default(),
                )
                .unwrap();
        }
    }
    #[test]
    fn preview_apply_archive_preserve_work_and_repeat_without_replay() {
        let fixture = Fixture::new();
        let worktree = fixture._temp.path().join("worktree");
        fs::create_dir(&worktree).unwrap();
        fs::write(worktree.join("dirty.txt"), "retain dirty project").unwrap();
        fixture.wake("done.status", "done: historical outcome");
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.wakes, 1);
        assert!(fixture.private.exists());
        assert!(!fixture.state().join(".upgrade-cleanup").exists());
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        assert!(!fixture.private.exists());
        assert!(!fixture.state().join("done.meta").exists());
        let allocation = multplx_domain::lifecycle::home_seed::read_home_allocation(
            &fixture.home.join("data"),
            "done",
        )
        .unwrap()
        .unwrap();
        let archive = allocation.retained_path.unwrap();
        assert_eq!(
            fs::read_to_string(archive.join("unfinished.txt")).unwrap(),
            "uncommitted private bytes"
        );
        assert_eq!(
            fs::read_to_string(worktree.join("dirty.txt")).unwrap(),
            "retain dirty project"
        );
        assert_eq!(
            fs::read_dir(archive.join("state")).unwrap().count(),
            0,
            "maintenance guard files must not be stranded in the archive"
        );
        assert_eq!(
            WakeQueue::new(fixture.state())
                .observe_unfinished_count()
                .unwrap(),
            0
        );
        assert!(
            evidence::read_retirement(&fixture.state(), "done")
                .unwrap()
                .committed
        );
        assert!(!inspect(&fixture.root, &fixture.home).unwrap().actionable());
        fixture.wake("done.status", "done: historical outcome");
        assert_eq!(
            WakeQueue::new(fixture.state())
                .observe_unfinished_count()
                .unwrap(),
            0,
            "stable wake dedupe survives retirement"
        );
    }
    #[test]
    fn incomplete_decision_child_and_foreign_owner_are_retained() {
        let fixture = Fixture::new();
        fixture.edit(|record| record.schedule.state = WorkState::Running);
        assert!(
            inspect(&fixture.root, &fixture.home)
                .unwrap()
                .tasks
                .is_empty()
        );
        fixture.edit(|record| {
            record.schedule.state = WorkState::Completed;
            record.schedule.decisions.push(HumanDecision {
                id: "choice".into(),
                task_id: "done".into(),
                brief_revision: 1,
                workflow_revision: None,
                question: "Choose?".into(),
                answer: None,
            });
        });
        fixture.wake("done.status", "resolved: prose is insufficient");
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert!(plan.tasks.is_empty());
        assert_eq!(plan.wakes, 0);
        fixture.edit(|record| record.schedule.decisions.clear());
        fs::write(fixture.private.join("state/child.meta"), "unknown child").unwrap();
        assert!(
            inspect(&fixture.root, &fixture.home)
                .unwrap()
                .tasks
                .is_empty()
        );
        fs::remove_file(fixture.private.join("state/child.meta")).unwrap();
        fixture.edit(|record| record.owner_state = Some("/foreign/state".into()));
        assert!(
            inspect(&fixture.root, &fixture.home)
                .unwrap()
                .tasks
                .is_empty()
        );
        assert!(fixture.private.join("unfinished.txt").exists());
    }
    #[test]
    fn consent_snapshot_and_live_session_races_fail_without_cleanup() {
        let fixture = Fixture::new();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        fs::write(
            fixture.state().join(".lock"),
            format!("{}\n", std::process::id()),
        )
        .unwrap();
        assert!(
            apply(&fixture.root, &fixture.home, &plan)
                .unwrap_err()
                .contains("live")
        );
        fs::remove_file(fixture.state().join(".lock")).unwrap();
        fixture.edit(|record| record.schedule.state = WorkState::Running);
        assert!(
            apply(&fixture.root, &fixture.home, &plan)
                .unwrap_err()
                .contains("changed")
        );
        assert!(fixture.private.exists());
        assert!(!fixture.state().join(".upgrade-cleanup").exists());
        assert!(!fixture.state().join(".lock.acquire").exists());
    }
    #[test]
    fn accepted_local_packets_settle_and_foreign_or_decision_packets_remain() {
        let fixture = Fixture::new();
        let state = fixture.state();
        let event = fixture.envelope("report-local", "done");
        multplx_domain::operational_input::persist_message_envelope(&state, &event).unwrap();
        let outcome = parent_channel::prepare_outcome(&state, &event).unwrap();
        parent_channel::persist_prepared(&state, &outcome).unwrap();
        let mut foreign = outcome.clone();
        foreign.event.message_id = "report-foreign".into();
        foreign.route.recipient_state = "/foreign/state".into();
        foreign.route.recipient_home = "/foreign".into();
        fs::write(
            state.join("parent-outbox/report-foreign.json"),
            serde_json::to_vec(&foreign).unwrap(),
        )
        .unwrap();
        let decision = fixture.envelope("report-question", "needs-decision");
        multplx_domain::operational_input::persist_message_envelope(&state, &decision).unwrap();
        let mut question = outcome.clone();
        question.event = decision;
        fs::write(
            state.join("parent-outbox/report-question.json"),
            serde_json::to_vec(&question).unwrap(),
        )
        .unwrap();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert_eq!(plan.packets, 1);
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        assert!(!state.join("parent-outbox/report-local.json").exists());
        assert!(
            state
                .join("parent-receipts/parent-outbox-report-local.json")
                .exists()
        );
        assert!(state.join("parent-outbox/report-foreign.json").exists());
        assert!(state.join("parent-outbox/report-question.json").exists());
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
        assert_eq!(
            parent_channel::settled_local_outcomes(&state)
                .unwrap()
                .len(),
            0
        );
        assert!(
            multplx_domain::operational_input::publish_report_notification(
                &state,
                &event,
                &format!(
                    "report: message={} task=done state=done brief={} summary={}",
                    event.message_id,
                    event.brief_revision.unwrap(),
                    event.summary
                ),
                SystemTime::now(),
                &SystemProcessProbe::default()
            )
            .unwrap()
            .is_some()
        );
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
    }
    #[test]
    fn dangling_poll_quarantine_ignores_prior_scan_marker_and_preserves_valid_task() {
        let fixture = Fixture::new();
        fixture.edit(|record| record.schedule.state = WorkState::Running);
        let state = fixture.state();
        for task in ["missing", "done"] {
            for suffix in ["pr-poll", "pr-poll-registration", "check.sh"] {
                fs::write(
                    state.join(format!("{task}.{suffix}")),
                    format!("original {task} {suffix}"),
                )
                .unwrap();
            }
        }
        fs::write(state.join(".pr-check-migration-scan-v1"), "old marker").unwrap();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert_eq!(plan.checks, 3);
        // An unfinished current task is retained, while unrelated dangling
        // namespaces can be quarantined without running a script.
        assert!(plan.tasks.is_empty());
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        for suffix in ["pr-poll", "pr-poll-registration", "check.sh"] {
            assert!(!state.join(format!("missing.{suffix}")).exists());
            assert!(state.join(format!("done.{suffix}")).exists());
        }
        let contents = fs::read_dir(state.join(".pr-check-quarantine"))
            .unwrap()
            .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(contents.len(), 3);
        assert!(
            contents
                .iter()
                .all(|text| text.starts_with("original missing"))
        );
        assert!(!inspect(&fixture.root, &fixture.home).unwrap().actionable());
    }
    fn prepare_retirement(fixture: &Fixture, stage: &str) -> Vec<u8> {
        let original = fs::read(fixture.state().join("done.meta")).unwrap();
        evidence::write_retirement(
            &fixture.state(),
            &evidence::Retirement {
                schema: "mx-upgrade-retirement.v1".into(),
                task: "done".into(),
                original_meta: original.clone(),
                committed: false,
            },
        )
        .unwrap();
        fs::write(fixture.state().join(".teardown.transaction.done"), serde_json::to_vec(&serde_json::json!({
            "id": "done", "home": fixture.private, "stage": stage, "home_allocation": fixture.record().home_allocation
        })).unwrap()).unwrap();
        original
    }
    #[test]
    fn prepared_retirement_never_retires_changed_current_work_or_unrelated_journal() {
        let fixture = Fixture::new();
        let expected = prepare_retirement(&fixture, "prepared");
        fixture.edit(|record| {
            record.schedule.state = WorkState::Running;
            record.attempt.as_mut().unwrap().generation += 1;
        });
        let current = fs::read(fixture.state().join("done.meta")).unwrap();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert!(plan.tasks.is_empty());
        assert!(plan.retained["done"].contains("older task revision"));
        assert!(
            teardown::retire_completed_for_upgrade(
                &context(&fixture.root, &fixture.home),
                "done",
                &expected
            )
            .unwrap_err()
            .contains("changed")
        );
        assert_eq!(
            fs::read(fixture.state().join("done.meta")).unwrap(),
            current
        );
        assert!(fixture.private.exists());
        let other = Fixture::new();
        fs::write(
            other.state().join(".teardown.transaction.done"),
            b"unrelated journal",
        )
        .unwrap();
        assert!(inspect(&other.root, &other.home).unwrap().tasks.is_empty());
        assert!(other.private.exists());
    }
    fn abandon_anchored_claims(fixture: &Fixture) {
        for name in [".lock.acquire", ".watch.lock", ".workspace-launch.lock"] {
            let path = fixture.private.join("state").join(name);
            let lock = DirectoryLock::try_acquire_anchored(
                &path,
                &fixture.state(),
                &SystemProcessProbe::default(),
            )
            .unwrap();
            let owner = fs::read_link(&path).unwrap();
            // Fixture models an exited exact owner; no real session is touched.
            fs::write(owner.join("pid"), "2147483647\n").unwrap();
            std::mem::forget(lock);
        }
    }
    #[test]
    fn interrupted_before_or_after_home_move_resumes_exact_journal_and_releases_claims() {
        for stage in ["prepared", "home-removed", "archiving"] {
            let moved = stage != "prepared";
            let fixture = Fixture::new();
            abandon_anchored_claims(&fixture);
            if moved {
                let token = fixture.record().home_allocation.unwrap();
                multplx_domain::lifecycle::home_seed::retire_home(
                    &fixture.home.join("data"),
                    &token,
                )
                .unwrap();
            }
            prepare_retirement(&fixture, if moved { "home-removed" } else { "prepared" });
            let plan = inspect(&fixture.root, &fixture.home).unwrap();
            assert_eq!(plan.tasks.len(), 1);
            apply(&fixture.root, &fixture.home, &plan).unwrap();
            let allocation = multplx_domain::lifecycle::home_seed::read_home_allocation(
                &fixture.home.join("data"),
                "done",
            )
            .unwrap()
            .unwrap();
            let archive = allocation.retained_path.unwrap();
            assert_eq!(fs::read_dir(archive.join("state")).unwrap().count(), 0);
            assert_eq!(
                fs::read_to_string(archive.join("unfinished.txt")).unwrap(),
                "uncommitted private bytes"
            );
            assert!(
                evidence::read_retirement(&fixture.state(), "done")
                    .unwrap()
                    .committed
            );
            assert!(!inspect(&fixture.root, &fixture.home).unwrap().actionable());
        }
    }
    #[test]
    fn retired_wake_only_repair_and_missing_receipt_or_forged_attempt_fail_closed() {
        let fixture = Fixture::new();
        let event = fixture.envelope("report-before-retire", "done");
        multplx_domain::operational_input::persist_message_envelope(&fixture.state(), &event)
            .unwrap();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        assert!(
            multplx_domain::operational_input::publish_report_notification(
                &fixture.state(),
                &event,
                "report: task=done",
                SystemTime::now(),
                &SystemProcessProbe::default()
            )
            .unwrap()
            .is_none()
        );
        fixture.wake(
            "message-report-before-retire",
            "report: task=done state=done",
        );
        fixture.wake("unknown.status", "done: prose only");
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert!(plan.tasks.is_empty());
        assert_eq!(plan.wakes, 1);
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        assert_eq!(
            WakeQueue::new(fixture.state())
                .observe_unfinished_count()
                .unwrap(),
            1
        );
        let mut forged = event.clone();
        forged.attempt.as_mut().unwrap().generation += 1;
        assert!(!parent_channel::maintenance_event_settled(
            &fixture.state(),
            &forged
        ));
    }
    #[test]
    fn completed_backlog_is_held_and_current_dependency_retains_task_authority() {
        let fixture = Fixture::new();
        let path = fixture.home.join("data/backlog.md");
        multplx_domain::backlog::run_cli(
            &["add".into(), "done".into(), "Completed old task".into()],
            path.clone(),
        )
        .unwrap();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert!(plan.holds.contains("done"));
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        let item = multplx_domain::backlog::BacklogStore::new(&path)
            .snapshot("done")
            .unwrap();
        assert!(item.held);
        assert_eq!(item.state, "queued");
        assert!(
            !multplx_domain::backlog::BacklogStore::new(path)
                .ready()
                .unwrap()
                .lines()
                .any(|id| id == "done")
        );
        let fixture = Fixture::new();
        let mut child = TaskRecord::new(
            "dependent".into(),
            AssignmentRole::Researcher,
            ArtifactKind::Report,
            false,
            "ROOT".into(),
            "ROOT".into(),
            fixture.home.to_string_lossy().into_owned(),
        );
        child.schedule.state = WorkState::WaitingDependency;
        child.schedule.dependencies.push("done".into());
        fs::write(
            fixture.state().join("dependent.meta"),
            subagent_model::write_meta("kind=scout\n", &child).unwrap(),
        )
        .unwrap();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert!(plan.tasks.is_empty());
        assert!(plan.retained["done"].contains("depends"));
        assert!(fixture.state().join("done.meta").exists());
    }
    #[test]
    fn corrupt_or_linked_retirement_backup_boundaries_never_write_foreign_paths() {
        let fixture = Fixture::new();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        let outside = fixture._temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.state().join(".upgrade-cleanup")).unwrap();
        assert!(apply(&fixture.root, &fixture.home, &plan).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert!(fixture.private.exists());
        fs::remove_file(fixture.state().join(".upgrade-cleanup")).unwrap();
        fs::create_dir(fixture.state().join(".upgrade-retirements")).unwrap();
        fs::write(
            fixture.state().join(".upgrade-retirements/done.json"),
            "corrupt",
        )
        .unwrap();
        assert!(inspect(&fixture.root, &fixture.home).is_err());
        assert!(fixture.private.exists());
    }
    #[test]
    fn unpublished_accepted_completed_report_repair_cannot_reappear_on_startup() {
        let fixture = Fixture::new();
        let state = fixture.state();
        fs::write(
            fixture.private.join("state/retained-child.meta"),
            "retained child",
        )
        .unwrap();
        let event = fixture.envelope("report-unpublished", "done");
        let outcome = parent_channel::prepare_outcome(&state, &event).unwrap();
        fs::create_dir(state.join("evidence")).unwrap();
        let accepted = serde_json::to_vec(
            &serde_json::json!({"accepted":true,"envelope":event,"parent_outcome":outcome}),
        )
        .unwrap();
        let path = state.join("evidence/done-report-unpublished.json");
        fs::write(&path, &accepted).unwrap();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert!(plan.tasks.is_empty());
        assert_eq!(plan.report_repairs, 1);
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
        assert_eq!(fs::read(&path).unwrap(), accepted);
        multplx_domain::supervision::reconcile_report_wakes(&state).unwrap();
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
        assert_eq!(parent_channel::pending_outcomes(&state).unwrap(), 0);
        assert!(
            multplx_domain::supervision::completed_report_repairs(&state)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn ambiguous_pending_reply_artifacts_retain_home_and_task() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        for kind in ["malformed", "unknown", "unreadable", "link", "pending"] {
            let fixture = Fixture::new();
            let state = fixture.state();
            let id = multplx_domain::lifecycle::pending_reply::create(
                &fixture.home,
                &state,
                "done",
                "request",
            )
            .unwrap();
            let path = multplx_domain::lifecycle::pending_reply::path(&state, &id);
            match kind {
                "malformed" => fs::write(&path, "unknown bytes").unwrap(),
                "unknown" => {
                    let bytes = fs::read_to_string(&path)
                        .unwrap()
                        .replace("phase=awaiting_report", "phase=unknown");
                    fs::write(&path, bytes).unwrap();
                }
                "unreadable" => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap()
                }
                "link" => {
                    fs::remove_file(&path).unwrap();
                    symlink(state.join("done.meta"), &path).unwrap();
                }
                _ => {}
            }
            let before = fs::read(state.join("done.meta")).unwrap();
            let plan = inspect(&fixture.root, &fixture.home).unwrap();
            assert!(plan.tasks.is_empty(), "{kind}");
            assert!(
                plan.retained
                    .values()
                    .any(|reason| reason.contains("pending") || reason.contains("request")),
                "{kind}: {:?}",
                plan.retained
            );
            assert_eq!(fs::read(state.join("done.meta")).unwrap(), before);
            assert!(fixture.private.exists());
        }
    }

    #[test]
    fn mismatched_frozen_report_cannot_reconstruct_other_outcome() {
        for field in ["task_id", "message_id", "brief_revision"] {
            let fixture = Fixture::new();
            let state = fixture.state();
            let event = fixture.envelope("report-frozen", "done");
            let outcome = parent_channel::prepare_outcome(&state, &event).unwrap();
            let mut frozen = serde_json::to_value(outcome).unwrap();
            frozen["event"][field] = if field == "brief_revision" {
                serde_json::json!(99)
            } else {
                serde_json::json!("different")
            };
            fs::create_dir(state.join("evidence")).unwrap();
            let path = state.join("evidence/done-report-frozen.json");
            let bytes = serde_json::to_vec(
                &serde_json::json!({"accepted":true,"envelope":event,"parent_outcome":frozen}),
            )
            .unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(
                multplx_domain::supervision::completed_report_repairs(&state)
                    .unwrap()
                    .is_empty()
            );
            let guard = MaintenanceGuard::acquire(&state, &SystemProcessProbe::default()).unwrap();
            multplx_domain::supervision::repair_completed_report_wakes(
                &state,
                &guard,
                &BTreeSet::from(["done".into()]),
            )
            .unwrap();
            assert_eq!(parent_channel::pending_outcomes(&state).unwrap(), 0);
            assert_eq!(
                WakeQueue::new(&state).observe_unfinished_count().unwrap(),
                0
            );
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn pending_reply_settlement_requires_valid_closed_identity() {
        use multplx_domain::lifecycle::pending_reply;
        let fixture = Fixture::new();
        let state = fixture.state();
        let correlation = pending_reply::create(&fixture.home, &state, "done", "request").unwrap();
        let path = pending_reply::path(&state, &correlation);
        let original = fs::read_to_string(&path).unwrap();
        let resolved = original
            .replace("phase=awaiting_report", "phase=resolved")
            .replace("resolved_epoch=", "resolved_epoch=100");
        fs::write(&path, &resolved).unwrap();
        assert!(pending_reply::maintenance_retirement_clear(&state, Some("done")).is_ok());
        assert_eq!(
            inspect(&fixture.root, &fixture.home).unwrap().tasks.len(),
            1
        );
        for corrupt in [
            resolved.replace(&format!("request_id={correlation}"), "request_id=other"),
            resolved.replace("created_epoch=", "created_epoch=invalid"),
            resolved.replace("message_id=", "message_id=../foreign"),
            resolved.replace("attempt_id=", "attempt_id=unknown"),
            resolved.replace("resolved_epoch=100", "resolved_epoch=0"),
        ] {
            fs::write(&path, &corrupt).unwrap();
            assert!(pending_reply::maintenance_retirement_clear(&state, Some("done")).is_err());
            assert!(
                inspect(&fixture.root, &fixture.home)
                    .unwrap()
                    .tasks
                    .is_empty()
            );
            assert!(fixture.private.exists());
        }
        // A known pending obligation for another task does not block unrelated
        // completed work, while a private home retains every pending request.
        fs::write(&path, original.replace("task_id=done", "task_id=other")).unwrap();
        assert!(pending_reply::maintenance_retirement_clear(&state, Some("done")).is_ok());
        assert!(pending_reply::maintenance_retirement_clear(&state, None).is_err());
        assert_eq!(
            inspect(&fixture.root, &fixture.home).unwrap().tasks.len(),
            1
        );
    }

    #[test]
    fn metadata_absent_recovery_respects_live_task_owner_and_retries() {
        let fixture = Fixture::new();
        let token = fixture.record().home_allocation.unwrap();
        multplx_domain::lifecycle::home_seed::retire_home(&fixture.home.join("data"), &token)
            .unwrap();
        let allocation = multplx_domain::lifecycle::home_seed::read_home_allocation(
            &fixture.home.join("data"),
            "done",
        )
        .unwrap()
        .unwrap();
        let archive = allocation.retained_path.unwrap();
        prepare_retirement(&fixture, "home-removed");
        let state = fixture.state();
        fs::remove_file(state.join("done.meta")).unwrap();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert_eq!(plan.tasks.len(), 1);
        let journal = fs::read(state.join(".teardown.transaction.done")).unwrap();
        let lock = DirectoryLock::try_acquire(
            state.join(".done.identity.lock"),
            &SystemProcessProbe::default(),
        )
        .unwrap();
        assert!(apply(&fixture.root, &fixture.home, &plan).is_err());
        assert_eq!(
            fs::read(state.join(".teardown.transaction.done")).unwrap(),
            journal
        );
        assert!(archive.exists());
        assert!(!state.join(".upgrade-cleanup").exists());
        drop(lock);
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        assert!(evidence::read_retirement(&state, "done").unwrap().committed);
        assert!(!fixture.private.exists());
        assert!(!state.join(".done.identity.lock").exists());
    }

    #[test]
    fn accepted_report_without_frozen_packet_repairs_only_current_local_truth() {
        let fixture = Fixture::new();
        let state = fixture.state();
        fs::create_dir(state.join("evidence")).unwrap();
        let event = fixture.envelope("report-legacy", "done");
        let valid_path = state.join("evidence/done-report-legacy.json");
        let valid =
            serde_json::to_vec(&serde_json::json!({"accepted":true,"envelope":event})).unwrap();
        fs::write(&valid_path, &valid).unwrap();
        for (name, value) in [
            (
                "rejected",
                serde_json::json!({"accepted":false,"envelope":event}),
            ),
            ("missing", serde_json::json!({"accepted":true})),
            (
                "decision",
                serde_json::json!({"accepted":true,"envelope":fixture.envelope("decision", "needs-decision")}),
            ),
            ("progress", {
                let mut event = fixture.envelope("progress", "working");
                event.automatic_wake = Some(false);
                serde_json::json!({"accepted":true,"envelope":event})
            }),
            ("revision", {
                let mut event = fixture.envelope("revision", "done");
                event.brief_revision = Some(99);
                serde_json::json!({"accepted":true,"envelope":event})
            }),
            ("unknown", {
                let mut event = fixture.envelope("unknown", "done");
                event.task_id = "unknown".into();
                serde_json::json!({"accepted":true,"envelope":event})
            }),
        ] {
            fs::write(
                state.join(format!("evidence/{name}.json")),
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
        }
        fs::write(state.join("evidence/README"), "retained evidence").unwrap();
        assert_eq!(
            multplx_domain::supervision::completed_report_repairs(&state).unwrap(),
            vec![valid_path.clone()]
        );
        let guard = MaintenanceGuard::acquire(&state, &SystemProcessProbe::default()).unwrap();
        assert_eq!(
            multplx_domain::supervision::repair_completed_report_wakes(
                &state,
                &guard,
                &BTreeSet::from(["done".into()])
            )
            .unwrap(),
            1
        );
        assert_eq!(parent_channel::pending_outcomes(&state).unwrap(), 1);
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            1
        );
        assert_eq!(fs::read(valid_path).unwrap(), valid);
        assert!(
            multplx_domain::supervision::completed_report_repairs(&state)
                .unwrap()
                .is_empty()
        );
        parent_channel::settle_local_outcomes(&state, &guard, &BTreeSet::from(["done".into()]))
            .unwrap();
        WakeQueue::new(&state)
            .settle_maintenance(
                &guard,
                &SystemProcessProbe::default(),
                SystemTime::now(),
                |record| settled_wake(&state, record),
            )
            .unwrap();
        assert_eq!(parent_channel::pending_outcomes(&state).unwrap(), 0);
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
    }

    #[test]
    fn retired_wake_requires_untampered_completion_and_archived_home_identity() {
        let fixture = Fixture::new();
        let state = fixture.state();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        let path = evidence::receipt_path(&state, "done").unwrap();
        let original = fs::read(&path).unwrap();
        let allocation = multplx_domain::lifecycle::home_seed::read_home_allocation(
            &fixture.home.join("data"),
            "done",
        )
        .unwrap()
        .unwrap();
        let archive = allocation.retained_path.unwrap();
        let retained = fs::read(archive.join("unfinished.txt")).unwrap();
        fixture.wake("done.status", "done: historical observation");
        let wake = WakeQueue::new(&state)
            .observe_once(multplx_core::wake::WakeKind::Signal, "done.status")
            .unwrap()
            .unwrap();
        assert!(settled_wake(&state, &wake));
        for corruption in ["schema", "task", "completion", "lease", "prepared"] {
            let mut receipt: evidence::Retirement = serde_json::from_slice(&original).unwrap();
            match corruption {
                "schema" => receipt.schema = "unknown".into(),
                "task" => receipt.task = "other".into(),
                "prepared" => receipt.committed = false,
                _ => {
                    let raw = std::str::from_utf8(&receipt.original_meta).unwrap();
                    let mut record = subagent_model::read_meta("done", raw).unwrap();
                    if corruption == "completion" {
                        record.schedule.state = WorkState::Running;
                    } else {
                        record.home_allocation = None;
                    }
                    receipt.original_meta = subagent_model::write_meta(raw, &record)
                        .unwrap()
                        .into_bytes();
                }
            }
            fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
            assert!(!settled_wake(&state, &wake), "{corruption}");
            assert_eq!(
                WakeQueue::new(&state).observe_unfinished_count().unwrap(),
                1
            );
            assert_eq!(fs::read(archive.join("unfinished.txt")).unwrap(), retained);
        }
        fs::write(&path, &original).unwrap();
        // A different directory at the retained path cannot prove the
        // original home identity even while its real archive bytes survive.
        let saved_archive = archive.with_extension("preserved");
        fs::rename(&archive, &saved_archive).unwrap();
        fs::create_dir(&archive).unwrap();
        assert!(evidence::read_retirement(&state, "done").is_err());
        assert!(!settled_wake(&state, &wake));
        assert_eq!(
            fs::read(saved_archive.join("unfinished.txt")).unwrap(),
            retained
        );
    }

    #[test]
    fn dangling_namespace_writer_and_broken_meta_link_retain_artifacts() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let state = fixture.state();
        fs::write(fixture.private.join("state/child.meta"), "retained child").unwrap();
        for suffix in ["pr-poll", "pr-poll-registration", "check.sh"] {
            fs::write(
                state.join(format!("missing.{suffix}")),
                format!("original {suffix}"),
            )
            .unwrap();
        }
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert_eq!(plan.checks, 3);
        assert!(plan.tasks.is_empty());
        let lock = DirectoryLock::try_acquire(
            state.join(".missing.identity.lock"),
            &SystemProcessProbe::default(),
        )
        .unwrap();
        assert!(apply(&fixture.root, &fixture.home, &plan).is_err());
        assert!(!state.join(".upgrade-cleanup").exists());
        assert!(!state.join(".pr-check-quarantine").exists());
        for suffix in ["pr-poll", "pr-poll-registration", "check.sh"] {
            assert_eq!(
                fs::read_to_string(state.join(format!("missing.{suffix}"))).unwrap(),
                format!("original {suffix}")
            );
        }
        drop(lock);
        let check = state.join("missing.check.sh");
        fs::write(&check, "newer check evidence").unwrap();
        assert!(
            crate::review::quarantine_dangling_pr_artifacts(&state, &plan.pr_artifacts).is_err()
        );
        assert_eq!(fs::read_to_string(&check).unwrap(), "newer check evidence");
        assert!(state.join("missing.pr-poll").exists());
        let foreign = fixture.private.join("foreign.pr-poll");
        fs::write(&foreign, "foreign evidence").unwrap();
        assert!(
            crate::review::quarantine_dangling_pr_artifacts(
                &state,
                &BTreeMap::from([(foreign.clone(), b"foreign evidence".to_vec())])
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&foreign).unwrap(), "foreign evidence");
        fs::write(&check, "original check.sh").unwrap();
        symlink(state.join("missing-target"), state.join("missing.meta")).unwrap();
        assert!(
            crate::review::dangling_pr_artifacts(&state)
                .unwrap()
                .is_empty()
        );
        assert!(
            crate::review::quarantine_dangling_pr_artifacts(&state, &plan.pr_artifacts).is_err()
        );
        assert!(inspect(&fixture.root, &fixture.home).is_err());
        assert!(state.join("missing.check.sh").exists());
        fs::remove_file(state.join("missing.meta")).unwrap();
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        assert!(!state.join("missing.check.sh").exists());
        assert!(fixture.private.exists());
    }

    #[test]
    fn retired_wake_only_repair_excludes_new_writer_of_same_task_identity() {
        let fixture = Fixture::new();
        let state = fixture.state();
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        fixture.wake("done.status", "done: historical result");
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert!(plan.tasks.is_empty());
        assert_eq!(plan.wakes, 1);
        let lock = DirectoryLock::try_acquire(
            state.join(".done.identity.lock"),
            &SystemProcessProbe::default(),
        )
        .unwrap();
        assert!(apply(&fixture.root, &fixture.home, &plan).is_err());
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            1
        );
        assert!(evidence::read_retirement(&state, "done").unwrap().committed);
        drop(lock);
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
    }

    #[test]
    fn maintenance_owners_keep_late_unapproved_namespaces_and_notifications() {
        let fixture = Fixture::new();
        let state = fixture.state();
        let original_meta = fs::read(state.join("done.meta")).unwrap();
        fs::remove_file(state.join("done.meta")).unwrap();
        let approved = inspect(&fixture.root, &fixture.home).unwrap();
        assert!(approved.task_ids.is_empty());
        fs::write(state.join("done.meta"), original_meta).unwrap();
        let event = fixture.envelope("report-after-preview", "done");
        multplx_domain::operational_input::persist_message_envelope(&state, &event).unwrap();
        let outcome = parent_channel::prepare_outcome(&state, &event).unwrap();
        parent_channel::persist_prepared(&state, &outcome).unwrap();
        let outbox = state.join("parent-outbox/report-after-preview.json");
        let pending = fs::read(&outbox).unwrap();
        fs::create_dir(state.join("evidence")).unwrap();
        let accepted = state.join("evidence/done-report-after-preview.json");
        let accepted_bytes = serde_json::to_vec(
            &serde_json::json!({"accepted":true,"envelope":event,"parent_outcome":outcome}),
        )
        .unwrap();
        fs::write(&accepted, &accepted_bytes).unwrap();
        let guard = MaintenanceGuard::acquire(&state, &SystemProcessProbe::default()).unwrap();
        assert_eq!(
            multplx_domain::supervision::repair_completed_report_wakes(
                &state,
                &guard,
                &approved.task_ids
            )
            .unwrap(),
            0
        );
        assert_eq!(
            parent_channel::settle_local_outcomes(&state, &guard, &approved.task_ids).unwrap(),
            0
        );
        assert_eq!(fs::read(&outbox).unwrap(), pending);
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
        fs::write(state.join("late.pr-poll"), "late poll").unwrap();
        assert_eq!(
            crate::review::quarantine_dangling_pr_artifacts(&state, &approved.pr_artifacts)
                .unwrap(),
            0
        );
        assert_eq!(
            fs::read_to_string(state.join("late.pr-poll")).unwrap(),
            "late poll"
        );
        // Scope uses the accepted envelope's task, never a misleading payload.
        fixture.wake(
            "message-report-after-preview",
            &format!("report: message=report-after-preview task=done state=done brief={} summary=accepted outcome", fixture.record().accepted_brief_revision.unwrap()),
        );
        let other = BTreeSet::from(["other".into()]);
        let mut misleading = WakeQueue::new(&state)
            .observe_once(
                multplx_core::wake::WakeKind::Signal,
                "message-report-after-preview",
            )
            .unwrap()
            .unwrap();
        misleading.payload = "report: task=other state=done".into();
        assert!(!settled_wake_for_tasks(&state, &misleading, &other));
        WakeQueue::new(&state)
            .settle_maintenance(
                &guard,
                &SystemProcessProbe::default(),
                SystemTime::now(),
                |record| settled_wake_for_tasks(&state, record, &other),
            )
            .unwrap();
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            1
        );
        let allowed = BTreeSet::from(["done".into()]);
        let _task = DirectoryLock::try_acquire(
            state.join(".done.identity.lock"),
            &SystemProcessProbe::default(),
        )
        .unwrap();
        assert_eq!(
            parent_channel::settle_local_outcomes(&state, &guard, &allowed).unwrap(),
            1
        );
        WakeQueue::new(&state)
            .settle_maintenance(
                &guard,
                &SystemProcessProbe::default(),
                SystemTime::now(),
                |record| settled_wake_for_tasks(&state, record, &allowed),
            )
            .unwrap();
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
        assert_eq!(fs::read(accepted).unwrap(), accepted_bytes);
        assert_eq!(
            fs::read_to_string(state.join("late.pr-poll")).unwrap(),
            "late poll"
        );
    }

    #[test]
    fn one_upgrade_retires_settles_quarantines_and_cannot_replay_on_startup() {
        let fixture = Fixture::new();
        let state = fixture.state();
        let mut active = fixture.record();
        active.task_id = "active".into();
        active.home_allocation = None;
        active.persistent_home = None;
        active.schedule.state = WorkState::Running;
        active.attempt.as_mut().unwrap().id = "attempt-active".into();
        let active_meta = subagent_model::write_meta("", &active).unwrap();
        fs::write(state.join("active.meta"), &active_meta).unwrap();
        let mut original_checks = BTreeMap::new();
        for task in ["done", "active"] {
            for suffix in ["check.sh", "pr-poll", "pr-poll-registration"] {
                let path = state.join(format!("{task}.{suffix}"));
                let bytes = format!("retained {task} {suffix}; never execute").into_bytes();
                fs::write(&path, &bytes).unwrap();
                original_checks.insert(path, bytes);
            }
        }
        let event = fixture.envelope("report-one-upgrade", "done");
        multplx_domain::operational_input::persist_message_envelope(&state, &event).unwrap();
        let outcome = parent_channel::prepare_outcome(&state, &event).unwrap();
        parent_channel::persist_prepared(&state, &outcome).unwrap();
        fs::create_dir(state.join("evidence")).unwrap();
        let accepted_path = state.join("evidence/done-report-one-upgrade.json");
        let accepted = serde_json::to_vec(
            &serde_json::json!({"accepted":true,"envelope":event,"parent_outcome":outcome}),
        )
        .unwrap();
        fs::write(&accepted_path, &accepted).unwrap();
        fixture.wake("done.status", "done: historical result");
        let plan = inspect(&fixture.root, &fixture.home).unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.packets, 1);
        assert_eq!(plan.report_repairs, 1);
        assert_eq!(plan.checks, 3);
        apply(&fixture.root, &fixture.home, &plan).unwrap();
        assert!(!state.join("done.meta").exists());
        assert!(!fixture.private.exists());
        assert!(evidence::read_retirement(&state, "done").unwrap().committed);
        let allocation = multplx_domain::lifecycle::home_seed::read_home_allocation(
            &fixture.home.join("data"),
            "done",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            fs::read_to_string(allocation.retained_path.unwrap().join("unfinished.txt")).unwrap(),
            "uncommitted private bytes"
        );
        assert_eq!(parent_channel::pending_outcomes(&state).unwrap(), 0);
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
        assert_eq!(fs::read(&accepted_path).unwrap(), accepted);
        let quarantined = fs::read_dir(state.join(".pr-check-quarantine"))
            .unwrap()
            .map(|entry| fs::read(entry.unwrap().path()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(quarantined.len(), 3);
        for (path, bytes) in &original_checks {
            if path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("done.")
            {
                assert!(!path.exists());
                assert!(quarantined.contains(bytes));
            } else {
                assert_eq!(fs::read(path).unwrap(), *bytes);
            }
        }
        assert_eq!(
            fs::read_to_string(state.join("active.meta")).unwrap(),
            active_meta
        );
        let next = inspect(&fixture.root, &fixture.home).unwrap();
        assert!(!next.actionable());
        apply(&fixture.root, &fixture.home, &next).unwrap();
        multplx_domain::supervision::reconcile_report_wakes(&state).unwrap();
        assert_eq!(parent_channel::pending_outcomes(&state).unwrap(), 0);
        assert_eq!(
            WakeQueue::new(&state).observe_unfinished_count().unwrap(),
            0
        );
        assert_eq!(fs::read(accepted_path).unwrap(), accepted);
        assert_eq!(
            fs::read_dir(state.join(".pr-check-quarantine"))
                .unwrap()
                .count(),
            3
        );
        assert!(!inspect(&fixture.root, &fixture.home).unwrap().actionable());
        assert_eq!(
            fs::read_to_string(state.join("active.meta")).unwrap(),
            active_meta
        );
    }

    #[test]
    fn native_child_absence_is_not_inferred_from_completed_parent() {
        use subagent_model::{NativeDelegationObservation, NativeObservationState};
        let fixture = Fixture::new();
        fixture.edit(|record| {
            record
                .native_observations
                .push(NativeDelegationObservation {
                    observation_id: "native-start".into(),
                    provider: "codex".into(),
                    child_id: Some("child-exact".into()),
                    parent_session_id: Some("session-exact".into()),
                    turn_id: None,
                    parent_attempt: record.attempt.clone(),
                    state: NativeObservationState::Started,
                    observed_at: "2026-10-13T00:00:00Z".into(),
                    artifact: None,
                    recovery: "session-bound".into(),
                })
        });
        assert!(
            inspect(&fixture.root, &fixture.home)
                .unwrap()
                .tasks
                .is_empty()
        );
        fixture.edit(|record| {
            let mut terminal = record.native_observations[0].clone();
            terminal.observation_id = "native-result".into();
            terminal.state = NativeObservationState::Result;
            record.native_observations.push(terminal);
        });
        assert_eq!(
            inspect(&fixture.root, &fixture.home).unwrap().tasks.len(),
            1
        );
    }
    #[test]
    fn completed_dead_endpoint_preserves_real_dirty_owned_git_allocation() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;
        if let Some(base) = std::env::var_os("MX_UPGRADE_CLEANUP_FIXTURE_DRIVER") {
            let base = PathBuf::from(base);
            let root = base.join("runtime");
            let home = base.join("owner");
            let plan = inspect(&root, &home).unwrap();
            assert_eq!(plan.tasks.len(), 1, "{:?}", plan.retained);
            apply(&root, &home, &plan).unwrap();
            return;
        }
        let fixture = Fixture::new();
        let project = fixture._temp.path().join("git-project");
        fs::create_dir(&project).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&project)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        };
        git(&["init", "-q"]);
        fs::write(project.join("tracked"), "base\n").unwrap();
        git(&["add", "tracked"]);
        git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "base",
        ]);
        let binding = multplx_domain::project_registry::register_project(
            &fixture.home,
            &project,
            None,
            multplx_domain::project_registry::CheckoutOwnership::UserOwned,
        )
        .unwrap();
        let store = multplx_domain::lifecycle::worktree::Store::new(&binding).unwrap();
        let attempt = fixture.record().attempt.unwrap();
        let allocation = store
            .acquire(
                &multplx_domain::lifecycle::worktree::Acquire {
                    request_id: "dirty-test",
                    owner_home: &fixture.home,
                    project: &binding,
                    task_id: "done",
                    attempt_id: &attempt.id,
                    persistent: true,
                },
                None,
            )
            .unwrap();
        let path = PathBuf::from(&allocation.binding.path);
        fs::write(path.join("tracked"), "dirty tracked work\n").unwrap();
        fs::write(path.join("game_center"), "untracked game center\n").unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["status", "--porcelain"])
            .output()
            .unwrap()
            .stdout;
        let head = Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        fixture.edit(|record| {
            record.project = Some(binding.clone());
            record.allocation = Some(allocation.binding.clone());
            record.runtime.provider = "tmux".into();
            record.runtime.endpoint = Some("primary:mx-done".into());
        });
        let fake_tmux = fixture._temp.path().join("tmux");
        fs::write(
            &fake_tmux,
            "#!/bin/sh\nprintf \"no server running on fixture\\n\" >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&fake_tmux, fs::Permissions::from_mode(0o755)).unwrap();
        let output = Command::new(std::env::current_exe().unwrap()).args(["--exact", "upgrade_cleanup::tests::completed_dead_endpoint_preserves_real_dirty_owned_git_allocation", "--nocapture"])
            .env("MX_UPGRADE_CLEANUP_FIXTURE_DRIVER", fs::canonicalize(fixture._temp.path()).unwrap()).env("MX_TMUX_BIN", fake_tmux).output().unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            Command::new("git")
                .arg("-C")
                .arg(&path)
                .args(["status", "--porcelain"])
                .output()
                .unwrap()
                .stdout,
            status
        );
        assert_eq!(
            Command::new("git")
                .arg("-C")
                .arg(&path)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
            head
        );
        assert_eq!(
            store
                .inspect(&allocation.binding.allocation_id)
                .unwrap()
                .binding,
            allocation.binding
        );
        assert_eq!(
            fs::read_to_string(path.join("game_center")).unwrap(),
            "untracked game center\n"
        );
        assert!(
            evidence::read_retirement(&fixture.state(), "done")
                .unwrap()
                .committed
        );
    }
}

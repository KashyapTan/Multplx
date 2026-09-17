//! Explicit, reversible operational-home migration.
//!
//! This module only converts Multplx-owned compatibility records. Journals,
//! queues, workflow snapshots, status history, PR state and message receipts
//! remain byte-for-byte owned by their existing readers.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use multplx_core::filesystem::{
    TransitionFault, TransitionWrite, atomic_replace, read_bounded_regular, recover_transition,
    recoverable_transition,
};
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::subagent_model::{self, ArtifactKind, AssignmentRole};
use crate::project_registry::{self, CheckoutOwnership};

const MIGRATION_SCHEMA: u32 = 1;
const TASK_WRITER_SCHEMA: &str = "2\n";
const MARKER: &str = "state/.home-schema-version";
const MAX_RECORD: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationMarker {
    pub schema_version: u32,
    pub task_writer_version: u32,
    pub operation_id: String,
    pub runtime_version: String,
    pub applied_at: String,
    pub backup_manifest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct BackupFile {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
    after_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct BackupManifest {
    version: u32,
    operation_id: String,
    runtime_version: String,
    home_identity: (u64, u64),
    files: Vec<BackupFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum HomeState {
    Legacy,
    Current,
    Incomplete,
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationReport {
    pub schema: String,
    pub home: PathBuf,
    pub state: HomeState,
    pub operation_id: String,
    pub actions: Vec<String>,
    pub retained: Vec<String>,
    pub blockers: Vec<String>,
    pub tasks: usize,
    pub legacy_tasks: usize,
    pub projects: usize,
    pub backup_manifest: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RestartTask {
    pub task_id: String,
    pub objective: Option<String>,
    pub owner_home: Option<String>,
    pub role: String,
    pub attempt_id: Option<String>,
    pub attempt_generation: Option<u64>,
    pub brief_revision: Option<u64>,
    pub dependencies: Vec<String>,
    pub open_questions: Vec<String>,
    pub evidence: Vec<String>,
    pub legacy_unknown: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RestartSummary {
    pub schema: String,
    pub home: PathBuf,
    pub available: bool,
    pub reason: Option<String>,
    pub tasks: Vec<RestartTask>,
    pub omitted: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyWorktreeMapping {
    pub version: u32,
    pub request_id: String,
    pub source_metadata: PathBuf,
    pub source_metadata_sha256: String,
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    pub historical_lease_id: Option<String>,
    pub allocation: super::worktree::Allocation,
}

#[derive(Clone, Debug)]
struct Plan {
    report: MigrationReport,
    writes: Vec<TransitionWrite>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_operation(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && !matches!(value, "." | "..")
}

fn relative_safe(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn existing(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match read_bounded_regular(path, MAX_RECORD) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(multplx_core::error::CoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn push_write(plan: &mut Plan, home: &Path, path: impl Into<PathBuf>, after: Vec<u8>) {
    let path = path.into();
    match existing(&home.join(&path)) {
        Ok(before) if before.as_deref() == Some(after.as_slice()) => {}
        Ok(before) => plan.writes.push(TransitionWrite {
            path,
            before,
            after,
        }),
        Err(error) => plan.report.blockers.push(error),
    }
}

fn home(path: &Path) -> Result<PathBuf, String> {
    let path = fs::canonicalize(path)
        .map_err(|error| format!("operational home is unavailable: {error}"))?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("operational home must be a real directory".into());
    }
    Ok(path)
}

fn marker(home: &Path) -> Result<Option<MigrationMarker>, String> {
    let Some(bytes) = existing(&home.join(MARKER))? else {
        return Ok(None);
    };
    let marker: MigrationMarker = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid home migration marker: {error}"))?;
    if marker.schema_version > MIGRATION_SCHEMA {
        return Err(format!(
            "home schema {} is newer than this runtime supports; left untouched",
            marker.schema_version
        ));
    }
    if marker.schema_version != MIGRATION_SCHEMA || marker.task_writer_version != 2 {
        return Err("unsupported home migration marker; left untouched".into());
    }
    Ok(Some(marker))
}

fn task_paths(state: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = match fs::read_dir(state) {
        Ok(entries) => entries
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|value| value == "meta"))
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.to_string()),
    };
    paths.sort();
    Ok(paths)
}

fn legacy_project_names(path: &Path) -> Result<Vec<String>, String> {
    let Some(bytes) = existing(path)? else {
        return Ok(Vec::new());
    };
    let text = String::from_utf8(bytes).map_err(|_| "legacy projects.md is not UTF-8")?;
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("-") {
            continue;
        }
        let name = fields.next().ok_or("malformed legacy project row")?;
        if name.is_empty()
            || name.contains(['/', '\\'])
            || matches!(name, "." | "..")
            || !seen.insert(name.to_owned())
        {
            return Err(format!(
                "ambiguous or unsafe legacy project identity {name:?}"
            ));
        }
        result.push(name.to_owned());
    }
    Ok(result)
}

fn desired_projects(home: &Path, names: &[String]) -> Result<Option<Vec<u8>>, String> {
    if names.is_empty() {
        return Ok(None);
    }
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    fs::create_dir_all(staging.path().join("data")).map_err(|error| error.to_string())?;
    fs::create_dir_all(staging.path().join("state")).map_err(|error| error.to_string())?;
    atomic_replace(
        staging.path().join("state/.task-writer-version"),
        TASK_WRITER_SCHEMA.as_bytes(),
        0o600,
    )
    .map_err(|error| error.to_string())?;
    if let Some(current) = existing(&home.join("data/projects.json"))? {
        atomic_replace(staging.path().join("data/projects.json"), &current, 0o600)
            .map_err(|error| error.to_string())?;
    }
    for name in names {
        let path = home.join("projects").join(name);
        if !path.exists() {
            continue;
        }
        project_registry::register_project_at(
            staging.path(),
            &home.join("data"),
            &home.join("projects"),
            &path,
            Some(name),
            CheckoutOwnership::Managed,
        )?;
    }
    existing(&staging.path().join("data/projects.json"))
}

fn build_plan(
    raw_home: &Path,
    operation: &str,
    coordinators: &BTreeSet<String>,
) -> Result<Plan, String> {
    if !valid_operation(operation) {
        return Err(
            "operation identity must contain only ASCII letters, digits, '.', '_' or '-'".into(),
        );
    }
    let home = home(raw_home)?;
    let mut plan = Plan {
        report: MigrationReport {
            schema: "multplx-home-migration-report.v1".into(),
            home: home.clone(),
            state: HomeState::Legacy,
            operation_id: operation.into(),
            actions: Vec::new(),
            retained: vec![
                "wake queue, inbox claims, journals, status history, workflow snapshots, PR records and action receipts remain byte-for-byte owned by their existing readers".into(),
                "legacy approvals, yolo values and publication records remain historical and grant no new authority".into(),
            ],
            blockers: Vec::new(),
            tasks: 0,
            legacy_tasks: 0,
            projects: 0,
            backup_manifest: None,
        },
        writes: Vec::new(),
    };
    match marker(&home) {
        Ok(Some(current)) => {
            plan.report.state = HomeState::Current;
            plan.report.operation_id = current.operation_id;
            plan.report.backup_manifest = Some(home.join(current.backup_manifest));
            return Ok(plan);
        }
        Ok(None) => {}
        Err(error) => plan.report.blockers.push(error),
    }
    for directory in ["state", "data", "config"] {
        let path = home.join(directory);
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && (!metadata.is_dir() || metadata.file_type().is_symlink())
        {
            plan.report
                .blockers
                .push(format!("{directory}/ must be a real directory"));
        }
    }

    let aliases = [
        ("config/actor-harness", "config/subagent-harness"),
        (
            "config/daemon-harness",
            "config/persistent-subagent-harness",
        ),
        (
            "config/actor-dispatch.json",
            "config/subagent-dispatch.json",
        ),
    ];
    for (legacy, canonical) in aliases {
        let old = existing(&home.join(legacy));
        let new = existing(&home.join(canonical));
        match (old, new) {
            (Ok(Some(old)), Ok(Some(new))) if old != new => plan.report.blockers.push(format!(
                "conflicting {legacy} and {canonical}; reconcile explicitly"
            )),
            (Ok(Some(old)), Ok(None)) => {
                push_write(&mut plan, &home, canonical, old);
                plan.report.actions.push(format!(
                    "copy {legacy} to canonical {canonical}; retain alias for compatibility"
                ));
            }
            (Err(error), _) | (_, Err(error)) => plan.report.blockers.push(error),
            _ => {}
        }
    }

    let mut mapped_coordinators = BTreeSet::new();
    for path in task_paths(&home.join("state"))? {
        plan.report.tasks += 1;
        let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
            plan.report
                .blockers
                .push("non-UTF-8 task metadata filename".into());
            continue;
        };
        let before = match existing(&path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => continue,
            Err(error) => {
                plan.report.blockers.push(error);
                continue;
            }
        };
        let text = match std::str::from_utf8(&before) {
            Ok(text) => text,
            Err(_) => {
                plan.report
                    .blockers
                    .push(format!("task {id} metadata is not UTF-8"));
                continue;
            }
        };
        match subagent_model::read_meta(id, text) {
            Ok(mut record) if record.legacy_unknown => {
                plan.report.legacy_tasks += 1;
                if coordinators.contains(id) {
                    if !record.persistent {
                        plan.report.blockers.push(format!(
                            "task {id} cannot map to sub-orchestrator without persistent-home evidence"
                        ));
                        continue;
                    }
                    record.role = AssignmentRole::SubOrchestrator;
                    record.artifact = ArtifactKind::Coordination;
                    mapped_coordinators.insert(id.to_owned());
                }
                match subagent_model::write_meta(text, &record) {
                    Ok(after) => {
                        push_write(
                            &mut plan,
                            &home,
                            PathBuf::from("state").join(format!("{id}.meta")),
                            after.into_bytes(),
                        );
                        plan.report.actions.push(format!(
                            "embed canonical compatibility model for task {id}; attempt and brief identity remain unknown"
                        ));
                    }
                    Err(error) => plan.report.blockers.push(format!("task {id}: {error}")),
                }
            }
            Ok(_) => {}
            Err(error) => plan.report.blockers.push(format!("task {id}: {error}")),
        }
    }
    for id in coordinators.difference(&mapped_coordinators) {
        plan.report.blockers.push(format!(
            "requested coordinator task {id} is absent, already canonical, or lacks legacy persistent-home evidence"
        ));
    }

    match legacy_project_names(&home.join("data/projects.md")) {
        Ok(names) => {
            plan.report.projects = names.len();
            for name in &names {
                let path = home.join("projects").join(name);
                if !path.exists() {
                    plan.report.retained.push(format!(
                        "legacy project {name} is missing; keep its route for explicit location repair"
                    ));
                }
            }
            match desired_projects(&home, &names) {
                Ok(Some(bytes)) => {
                    push_write(&mut plan, &home, "data/projects.json", bytes);
                    plan.report.actions.push(
                        "publish stable identities for available flat managed project checkouts without moving them".into(),
                    );
                }
                Ok(None) => {}
                Err(error) => plan.report.blockers.push(error),
            }
        }
        Err(error) => plan.report.blockers.push(error),
    }

    if home.join("data/daemons.md").is_file() {
        plan.report.retained.push(
            "legacy persistent-home routes remain authoritative compatibility evidence; only explicitly named coordinating responsibilities are mapped".into(),
        );
    }
    if home.join(".mx-daemon-home").is_file() {
        plan.report.retained.push(
            "legacy persistent-home marker remains readable during the compatibility window".into(),
        );
    }
    if home.join("state/.lock").is_symlink() {
        plan.report
            .blockers
            .push("session lock is an unexpected symlink".into());
    }
    if let Ok(Some(version)) = existing(&home.join("state/.task-writer-version"))
        && version != TASK_WRITER_SCHEMA.as_bytes()
    {
        plan.report
            .blockers
            .push("unsupported task writer version; left untouched".into());
    }
    push_write(
        &mut plan,
        &home,
        "state/.task-writer-version",
        TASK_WRITER_SCHEMA.as_bytes().to_vec(),
    );
    let backup = PathBuf::from("state/.migration-backups")
        .join(operation)
        .join("manifest.json");
    let marker = MigrationMarker {
        schema_version: MIGRATION_SCHEMA,
        task_writer_version: 2,
        operation_id: operation.into(),
        runtime_version: env!("CARGO_PKG_VERSION").into(),
        applied_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| error.to_string())?,
        backup_manifest: backup.to_string_lossy().into_owned(),
    };
    push_write(
        &mut plan,
        &home,
        MARKER,
        serde_json::to_vec_pretty(&marker).map_err(|error| error.to_string())?,
    );
    plan.report.backup_manifest = Some(home.join(&backup));
    if let Some(bytes) = existing(&home.join(&backup))? {
        let prior: BackupManifest = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid migration backup manifest: {error}"))?;
        if prior.version != 1
            || prior.operation_id != operation
            || prior.runtime_version != env!("CARGO_PKG_VERSION")
            || prior
                .files
                .iter()
                .any(|file| !relative_safe(&file.path) || digest(&file.after) != file.after_sha256)
        {
            plan.report
                .blockers
                .push("existing migration backup is incompatible or corrupt".into());
        } else {
            plan.writes = prior
                .files
                .into_iter()
                .map(|file| TransitionWrite {
                    path: file.path,
                    before: file.before,
                    after: file.after,
                })
                .collect();
            plan.report.state = HomeState::Incomplete;
            plan.report
                .actions
                .push("resume exact writes from durable migration backup".into());
        }
    }
    if !plan.report.blockers.is_empty() {
        plan.report.state = HomeState::Blocked;
    } else if !plan.writes.is_empty() && plan.report.state != HomeState::Incomplete {
        plan.report.state = HomeState::Legacy;
    }
    Ok(plan)
}

pub fn inspect(
    home: &Path,
    operation: &str,
    coordinators: &BTreeSet<String>,
) -> Result<MigrationReport, String> {
    Ok(build_plan(home, operation, coordinators)?.report)
}

fn write_backup(
    home: &Path,
    operation: &str,
    writes: &[TransitionWrite],
) -> Result<PathBuf, String> {
    let root = home.join("state/.migration-backups").join(operation);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(&root).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("migration backup directory is unsafe".into());
    }
    let home_metadata = fs::metadata(home).map_err(|error| error.to_string())?;
    let manifest = BackupManifest {
        version: 1,
        operation_id: operation.into(),
        runtime_version: env!("CARGO_PKG_VERSION").into(),
        home_identity: (home_metadata.dev(), home_metadata.ino()),
        files: writes
            .iter()
            .map(|write| BackupFile {
                path: write.path.clone(),
                before: write.before.clone(),
                after: write.after.clone(),
                after_sha256: digest(&write.after),
            })
            .collect(),
    };
    let path = root.join("manifest.json");
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    if let Some(old) = existing(&path)? {
        if old != bytes {
            return Err("operation identity conflicts with its existing backup".into());
        }
    } else {
        atomic_replace(&path, &bytes, 0o600).map_err(|error| error.to_string())?;
    }
    Ok(path)
}

fn exclude_live_writer(home: &Path) -> Result<(DirectoryLock, DirectoryLock), String> {
    let state = home.join("state");
    fs::create_dir_all(&state).map_err(|error| error.to_string())?;
    let processes = SystemProcessProbe::default();
    let migration = DirectoryLock::acquire_wait(
        state.join(".migration.lock"),
        &processes,
        Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    // SessionLock holds this claim directory while it checks and publishes
    // state/.lock. Keeping the same claim for the whole migration closes the
    // check-then-write race without impersonating a harness owner.
    let session_claim = DirectoryLock::acquire_wait(
        state.join(".lock.acquire"),
        &processes,
        Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    if home.join("state/.lock").is_symlink() {
        return Err("session lock is an unexpected symlink".into());
    }
    match multplx_core::session_lock::status(
        state.join(".lock"),
        &processes,
        &multplx_core::session_lock::harness_regex(),
    ) {
        multplx_core::session_lock::SessionLockStatus::Held(pid) => {
            return Err(format!(
                "home has live harness writer pid {pid}; bring it to a safe boundary before migration"
            ));
        }
        multplx_core::session_lock::SessionLockStatus::Unreadable => {
            return Err("session lock is unreadable; left untouched".into());
        }
        multplx_core::session_lock::SessionLockStatus::Free
        | multplx_core::session_lock::SessionLockStatus::Stale(_) => {}
    }
    Ok((migration, session_claim))
}

pub fn apply(
    raw_home: &Path,
    operation: &str,
    coordinators: &BTreeSet<String>,
) -> Result<MigrationReport, String> {
    let home = home(raw_home)?;
    let _lock = exclude_live_writer(&home)?;
    let transition_operation = format!("home-migration-{operation}");
    let receipt = home
        .join(".transitions")
        .join(format!("{transition_operation}.json"));
    if receipt.is_file() {
        recover_transition(&home, &transition_operation).map_err(|error| error.to_string())?;
    }
    let mut plan = build_plan(&home, operation, coordinators)?;
    if plan.report.state == HomeState::Current {
        return Ok(plan.report);
    }
    if !plan.report.blockers.is_empty() {
        return Err(format!(
            "migration blocked: {}",
            plan.report.blockers.join("; ")
        ));
    }
    fs::create_dir_all(home.join("config")).map_err(|error| error.to_string())?;
    fs::create_dir_all(home.join("data")).map_err(|error| error.to_string())?;
    let backup = write_backup(&home, operation, &plan.writes)?;
    let fault = match std::env::var("MX_MIGRATION_FAULT").as_deref() {
        Ok("before-intent") => Some(TransitionFault::BeforeIntent),
        Ok("after-intent") => Some(TransitionFault::AfterIntent),
        Ok("after-write") => Some(TransitionFault::AfterWrite(0)),
        Ok("after-progress") => Some(TransitionFault::AfterProgress(0)),
        Ok("after-commit") => Some(TransitionFault::AfterCommit),
        Ok(_) | Err(_) => None,
    };
    recoverable_transition(&home, &transition_operation, &plan.writes, fault)
        .map_err(|error| error.to_string())?;
    plan.report.state = HomeState::Current;
    plan.report.backup_manifest = Some(backup);
    Ok(plan.report)
}

fn read_manifest(home: &Path, operation: &str) -> Result<(PathBuf, BackupManifest), String> {
    let path = home
        .join("state/.migration-backups")
        .join(operation)
        .join("manifest.json");
    let manifest: BackupManifest = serde_json::from_slice(
        &read_bounded_regular(&path, 64 * 1024 * 1024).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("invalid migration backup manifest: {error}"))?;
    let metadata = fs::metadata(home).map_err(|error| error.to_string())?;
    if manifest.version != 1
        || manifest.operation_id != operation
        || manifest.runtime_version != env!("CARGO_PKG_VERSION")
        || manifest.home_identity != (metadata.dev(), metadata.ino())
        || manifest.files.is_empty()
        || manifest
            .files
            .iter()
            .any(|file| !relative_safe(&file.path) || digest(&file.after) != file.after_sha256)
    {
        return Err("backup does not match this home, runtime or migration operation".into());
    }
    Ok((path, manifest))
}

pub fn rollback(raw_home: &Path, operation: &str) -> Result<MigrationReport, String> {
    if !valid_operation(operation) {
        return Err("invalid operation identity".into());
    }
    let home = home(raw_home)?;
    let _lock = exclude_live_writer(&home)?;
    let (manifest_path, manifest) = read_manifest(&home, operation)?;
    for file in &manifest.files {
        let current = existing(&home.join(&file.path))?;
        let at_before = current == file.before;
        let at_after = current
            .as_ref()
            .is_some_and(|bytes| digest(bytes) == file.after_sha256);
        if !at_before && !at_after {
            return Err(format!(
                "{} changed after migration; rollback retained without overwrite",
                file.path.display()
            ));
        }
    }
    let rollback_record = home
        .join("state/.migration-backups")
        .join(operation)
        .join("rollback.json");
    atomic_replace(
        &rollback_record,
        &serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "operation_id": operation,
            "runtime_version": env!("CARGO_PKG_VERSION"),
            "manifest": manifest_path,
            "state": "restoring"
        }))
        .map_err(|error| error.to_string())?,
        0o600,
    )
    .map_err(|error| error.to_string())?;
    for file in manifest.files.iter().rev() {
        let target = home.join(&file.path);
        match &file.before {
            Some(bytes) => atomic_replace(&target, bytes, 0o600)
                .map_err(|error| format!("restore {}: {error}", file.path.display()))?,
            None => match fs::symlink_metadata(&target) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    fs::remove_file(&target).map_err(|error| error.to_string())?
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(format!(
                        "refuse to remove unsafe restored path {}",
                        file.path.display()
                    ));
                }
            },
        }
    }
    atomic_replace(
        &rollback_record,
        &serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "operation_id": operation,
            "runtime_version": env!("CARGO_PKG_VERSION"),
            "manifest": manifest_path,
            "state": "restored"
        }))
        .map_err(|error| error.to_string())?,
        0o600,
    )
    .map_err(|error| error.to_string())?;
    let mut report = build_plan(&home, operation, &BTreeSet::new())?.report;
    report.state = HomeState::Legacy;
    report.actions = vec!["matching backup restored; migration evidence retained".into()];
    report.backup_manifest = Some(manifest_path);
    Ok(report)
}

pub fn restart_summary(raw_home: &Path, limit: usize) -> RestartSummary {
    let home = match home(raw_home) {
        Ok(home) => home,
        Err(reason) => {
            return RestartSummary {
                schema: "multplx-restart-summary.v1".into(),
                home: raw_home.to_path_buf(),
                available: false,
                reason: Some(reason),
                tasks: Vec::new(),
                omitted: 0,
            };
        }
    };
    let mut tasks = Vec::new();
    let paths = match task_paths(&home.join("state")) {
        Ok(paths) => paths,
        Err(reason) => {
            return RestartSummary {
                schema: "multplx-restart-summary.v1".into(),
                home,
                available: false,
                reason: Some(reason),
                tasks,
                omitted: 0,
            };
        }
    };
    for path in paths {
        let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let text = match read_bounded_regular(&path, MAX_RECORD)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                String::from_utf8(bytes).map_err(|_| "task metadata is not UTF-8".into())
            }) {
            Ok(text) => text,
            Err(reason) => {
                return RestartSummary {
                    schema: "multplx-restart-summary.v1".into(),
                    home,
                    available: false,
                    reason: Some(format!("task {id} is unavailable: {reason}")),
                    tasks: Vec::new(),
                    omitted: 0,
                };
            }
        };
        let record = match subagent_model::read_meta(id, &text) {
            Ok(record) => record,
            Err(reason) => {
                return RestartSummary {
                    schema: "multplx-restart-summary.v1".into(),
                    home,
                    available: false,
                    reason: Some(format!("task {id} is unavailable: {reason}")),
                    tasks: Vec::new(),
                    omitted: 0,
                };
            }
        };
        let objective = record
            .accepted_brief_revision
            .and_then(|revision| {
                record
                    .briefs
                    .iter()
                    .find(|brief| brief.revision == revision)
            })
            .map(|brief| brief.scope.clone())
            .filter(|scope| !scope.is_empty());
        let open_questions = record
            .schedule
            .decisions
            .iter()
            .filter(|decision| decision.answer.is_none())
            .map(|decision| decision.question.clone())
            .collect();
        let mut evidence = Vec::new();
        if let Some(path) = &record.accepted_brief_path {
            evidence.push(path.clone());
        }
        if let Some(url) = record
            .delivery
            .history
            .iter()
            .rev()
            .find_map(|item| item.pr_url.as_ref())
        {
            evidence.push(url.clone());
        }
        tasks.push(RestartTask {
            task_id: record.task_id,
            objective,
            owner_home: record.owner_home,
            role: format!("{:?}", record.role).to_ascii_lowercase(),
            attempt_id: record.attempt.as_ref().map(|attempt| attempt.id.clone()),
            attempt_generation: record.attempt.as_ref().map(|attempt| attempt.generation),
            brief_revision: record.accepted_brief_revision,
            dependencies: record.schedule.dependencies,
            open_questions,
            evidence,
            legacy_unknown: record.legacy_unknown,
        });
    }
    tasks.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    let omitted = tasks.len().saturating_sub(limit);
    tasks.truncate(limit);
    RestartSummary {
        schema: "multplx-restart-summary.v1".into(),
        home,
        available: true,
        reason: None,
        tasks,
        omitted,
    }
}

/// Transfer one exact, quiescent legacy Git worktree and transactionally
/// publish every known home-owned reference. External pool metadata is inert.
pub fn relocate_worktree(
    raw_home: &Path,
    metadata: &Path,
    old_path: &Path,
    project_selector: &str,
    task_id: &str,
    request_id: &str,
) -> Result<LegacyWorktreeMapping, String> {
    if !valid_operation(request_id) {
        return Err("invalid relocation request identity".into());
    }
    let home = home(raw_home)?;
    let _lock = exclude_live_writer(&home)?;
    if marker(&home)?.is_none() {
        return Err("migrate the home before transferring legacy worktrees".into());
    }
    let metadata = fs::canonicalize(metadata)
        .map_err(|error| format!("legacy metadata unavailable: {error}"))?;
    let mapping_relative =
        PathBuf::from("state/.legacy-worktree-mappings").join(format!("{request_id}.json"));
    if let Some(bytes) = existing(&home.join(&mapping_relative))? {
        let mapping: LegacyWorktreeMapping = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid legacy worktree mapping: {error}"))?;
        if mapping.request_id != request_id
            || mapping.old_path != old_path
            || mapping.source_metadata != metadata
        {
            return Err("relocation request identity conflicts with its durable mapping".into());
        }
        return Ok(mapping);
    }
    let metadata_bytes =
        read_bounded_regular(&metadata, 4 * 1024 * 1024).map_err(|error| error.to_string())?;
    let old_path = old_path.to_path_buf();
    if !old_path.is_absolute()
        || old_path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("legacy worktree path must be a normalized absolute path".into());
    }
    let legacy = super::worktree::read_legacy(&metadata, std::slice::from_ref(&old_path))?
        .into_iter()
        .next()
        .ok_or("legacy inventory did not contain the recorded path")?;
    if legacy.leased == Some(false)
        || legacy
            .lease_holder
            .as_deref()
            .is_some_and(|holder| !holder.is_empty() && holder != task_id)
    {
        return Err("legacy lease does not prove this task owns the worktree; retained".into());
    }
    let task_path = home.join("state").join(format!("{task_id}.meta"));
    let task_before =
        read_bounded_regular(&task_path, MAX_RECORD).map_err(|error| error.to_string())?;
    let task_text = std::str::from_utf8(&task_before).map_err(|_| "task metadata is not UTF-8")?;
    let mut task = subagent_model::read_meta(task_id, task_text)?;
    if task.legacy_unknown {
        return Err(
            "legacy task has no proven attempt identity; worktree retained for reconciliation"
                .into(),
        );
    }
    let attempt = task
        .attempt
        .as_ref()
        .ok_or("task attempt identity is unavailable")?
        .id
        .clone();
    let project = project_registry::resolve_checkout(&home, project_selector)?;
    if task
        .project
        .as_ref()
        .is_some_and(|recorded| recorded != &project)
    {
        return Err("task and selected project identities conflict; retained".into());
    }
    if task.allocation.is_some() {
        return Err("task already owns a canonical allocation; legacy worktree retained".into());
    }
    if !task_text
        .lines()
        .any(|line| line.strip_prefix("worktree=") == Some(old_path.to_string_lossy().as_ref()))
    {
        return Err(
            "task metadata does not record the exact legacy worktree path; retained".into(),
        );
    }
    let persistent_receipt = if task.persistent {
        let receipt_relative =
            PathBuf::from("data").join(format!(".home-allocation-{task_id}.json"));
        match existing(&home.join(&receipt_relative))? {
            Some(receipt_before) => {
                let receipt = super::home_seed::read_home_allocation(&home.join("data"), task_id)?
                    .ok_or("persistent home allocation disappeared during validation")?;
                if receipt.binding.owner_home != home || receipt.binding.path != old_path {
                    return Err(
                        "persistent home allocation does not reference the legacy worktree; retained"
                            .into(),
                    );
                }
                Some((receipt_relative, Some(receipt_before), receipt))
            }
            None => None,
        }
    } else {
        None
    };
    let store = super::worktree::Store::new(&project)?;
    let allocation = store.relocate_legacy(
        &super::worktree::Acquire {
            request_id,
            owner_home: &home,
            project: &project,
            task_id,
            attempt_id: &attempt,
            persistent: task.persistent,
        },
        &legacy,
        None,
    )?;
    if task
        .allocation
        .as_ref()
        .is_some_and(|current| current != &allocation.binding)
    {
        return Err("task already records a different allocation; moved worktree retained".into());
    }
    task.project = Some(project);
    task.allocation = Some(allocation.binding.clone());
    if task.persistent {
        task.persistent_home = Some(allocation.binding.path.clone());
    }
    let persistent_receipt = if task.persistent {
        let (receipt_relative, receipt_before, mut receipt) =
            persistent_receipt.unwrap_or_else(|| {
                (
                    PathBuf::from("data").join(format!(".home-allocation-{task_id}.json")),
                    None,
                    super::home_seed::HomeAllocation {
                        version: 1,
                        binding: super::home_seed::HomeBinding {
                            id: task_id.into(),
                            owner_home: home.clone(),
                            path: PathBuf::from(&allocation.binding.path),
                            lease_id: format!("legacy-{}", allocation.binding.lease_id),
                            generation: 1,
                        },
                        runtime_root: PathBuf::from(&allocation.binding.path),
                        state: "active".into(),
                        retained_path: None,
                        directory_identity: allocation.directory_identity,
                        git_allocation: Some(allocation.clone()),
                    },
                )
            });
        receipt.binding.path = PathBuf::from(&allocation.binding.path);
        receipt.directory_identity = allocation.directory_identity;
        receipt.git_allocation = Some(allocation.clone());
        task.home_allocation = Some(receipt.binding.clone());
        Some((receipt_relative, receipt_before, receipt))
    } else {
        None
    };
    let mut compatibility = task_text
        .lines()
        .filter(|line| {
            !line.starts_with("canonical_model=") && !line.starts_with("schema_version=")
        })
        .map(|line| {
            if line.strip_prefix("worktree=") == Some(old_path.to_string_lossy().as_ref()) {
                format!("worktree={}", allocation.binding.path)
            } else if task.persistent
                && line.strip_prefix("home=") == Some(old_path.to_string_lossy().as_ref())
            {
                format!("home={}", allocation.binding.path)
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !compatibility.is_empty() {
        compatibility.push('\n');
    }
    let task_after = subagent_model::write_meta(&compatibility, &task)?.into_bytes();
    let mapping = LegacyWorktreeMapping {
        version: 1,
        request_id: request_id.into(),
        source_metadata: metadata,
        source_metadata_sha256: digest(&metadata_bytes),
        old_path,
        new_path: PathBuf::from(&allocation.binding.path),
        historical_lease_id: legacy.lease_id,
        allocation: allocation.clone(),
    };
    fs::create_dir_all(home.join("state/.legacy-worktree-mappings"))
        .map_err(|error| error.to_string())?;
    let mut writes = vec![TransitionWrite {
        path: PathBuf::from("state").join(format!("{task_id}.meta")),
        before: Some(task_before),
        after: task_after,
    }];
    if let Some((receipt_relative, receipt_before, receipt)) = persistent_receipt {
        writes.push(TransitionWrite {
            path: receipt_relative,
            before: receipt_before,
            after: serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?,
        });
    }
    writes.push(TransitionWrite {
        path: mapping_relative,
        before: None,
        after: serde_json::to_vec_pretty(&mapping).map_err(|error| error.to_string())?,
    });
    recoverable_transition(
        &home,
        &format!("legacy-worktree-{request_id}"),
        &writes,
        None,
    )
    .map_err(|error| error.to_string())?;
    Ok(mapping)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::process::Command;

    fn home_fixture(root: &Path) -> PathBuf {
        let home = root.join("home");
        for directory in ["state", "config", "data", "projects"] {
            fs::create_dir_all(home.join(directory)).unwrap();
        }
        fs::canonicalize(home).unwrap()
    }

    fn git(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn repository(root: &Path) -> PathBuf {
        let repository = root.join("repository");
        fs::create_dir(&repository).unwrap();
        git(&repository, &["init", "-b", "main"]);
        git(&repository, &["config", "user.name", "Migration Test"]);
        git(
            &repository,
            &["config", "user.email", "migration@example.invalid"],
        );
        fs::write(repository.join("tracked"), "base\n").unwrap();
        git(&repository, &["add", "tracked"]);
        git(&repository, &["commit", "-m", "base"]);
        fs::canonicalize(repository).unwrap()
    }

    #[test]
    fn inspect_is_read_only_apply_repeats_and_rollback_restores_exact_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(home.join("state")).unwrap();
        fs::create_dir_all(home.join("config")).unwrap();
        fs::create_dir_all(home.join("data")).unwrap();
        fs::write(home.join("config/actor-harness"), b"codex high\n").unwrap();
        let legacy = b"kind=delivery\nbackend=tmux\nwindow=mx-work\n";
        fs::write(home.join("state/work.meta"), legacy).unwrap();
        let before = walk(&home);
        let report = inspect(&home, "upgrade-1", &BTreeSet::new()).unwrap();
        assert_eq!(report.state, HomeState::Legacy);
        assert_eq!(before, walk(&home));
        let first = apply(&home, "upgrade-1", &BTreeSet::new()).unwrap();
        assert_eq!(first.state, HomeState::Current);
        let after = walk(&home);
        let second = apply(&home, "upgrade-1", &BTreeSet::new()).unwrap();
        assert_eq!(second.state, HomeState::Current);
        assert_eq!(after, walk(&home));
        assert_eq!(
            fs::read(home.join("config/subagent-harness")).unwrap(),
            b"codex high\n"
        );
        let migrated = fs::read_to_string(home.join("state/work.meta")).unwrap();
        assert!(migrated.contains("canonical_model="));
        assert!(
            subagent_model::read_meta("work", &migrated)
                .unwrap()
                .legacy_unknown
        );
        rollback(&home, "upgrade-1").unwrap();
        assert_eq!(fs::read(home.join("state/work.meta")).unwrap(), legacy);
        assert!(!home.join("config/subagent-harness").exists());
        assert!(!home.join(MARKER).exists());
    }

    #[test]
    fn conflicts_and_symlinks_block_without_partial_conversion() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(home.join("state")).unwrap();
        fs::create_dir_all(home.join("config")).unwrap();
        fs::create_dir_all(home.join("data")).unwrap();
        fs::write(home.join("config/actor-harness"), b"codex\n").unwrap();
        fs::write(home.join("config/subagent-harness"), b"claude\n").unwrap();
        let report = inspect(&home, "upgrade", &BTreeSet::new()).unwrap();
        assert_eq!(report.state, HomeState::Blocked);
        assert!(apply(&home, "upgrade", &BTreeSet::new()).is_err());
        assert!(!home.join(MARKER).exists());
    }

    #[test]
    fn restart_summary_keeps_identity_and_bounds_output() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(home.join("state")).unwrap();
        for index in 0..20 {
            fs::write(
                home.join(format!("state/task-{index}.meta")),
                "kind=delivery\nbackend=tmux\n",
            )
            .unwrap();
        }
        let summary = restart_summary(&home, 7);
        assert!(summary.available);
        assert_eq!(summary.tasks.len(), 7);
        assert_eq!(summary.omitted, 13);
        assert!(summary.tasks.iter().all(|task| task.legacy_unknown));
    }

    #[test]
    fn planning_maps_projects_aliases_and_only_explicit_persistent_coordinators() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let home = home_fixture(&root);
        let managed = home.join("projects/managed");
        fs::create_dir(&managed).unwrap();
        git(&managed, &["init", "-b", "main"]);
        git(&managed, &["config", "user.name", "Migration Test"]);
        git(
            &managed,
            &["config", "user.email", "migration@example.invalid"],
        );
        fs::write(managed.join("tracked"), "base\n").unwrap();
        git(&managed, &["add", "tracked"]);
        git(&managed, &["commit", "-m", "base"]);
        fs::write(home.join("data/projects.md"), "- managed /legacy/path\n").unwrap();
        fs::write(home.join("data/daemons.md"), "- coordinator\n").unwrap();
        fs::write(home.join(".mx-daemon-home"), "coordinator\n").unwrap();
        fs::write(home.join("config/actor-harness"), "codex high\n").unwrap();
        fs::write(home.join("config/daemon-harness"), "claude medium\n").unwrap();
        fs::write(home.join("config/actor-dispatch.json"), "{}\n").unwrap();
        fs::write(
            home.join("state/coordinator.meta"),
            "kind=daemon\nbackend=tmux\nhome=/legacy/coordinator\n",
        )
        .unwrap();
        fs::write(
            home.join("state/worker.meta"),
            "kind=delivery\nbackend=tmux\n",
        )
        .unwrap();
        let coordinators = BTreeSet::from(["coordinator".to_owned()]);

        let before = walk(&home);
        let report = inspect(&home, "mapped", &coordinators).unwrap();
        assert_eq!(report.state, HomeState::Legacy);
        assert_eq!(report.tasks, 2);
        assert_eq!(report.legacy_tasks, 2);
        assert_eq!(report.projects, 1);
        assert!(report.retained.iter().any(|item| item.contains("routes")));
        assert_eq!(walk(&home), before);

        apply(&home, "mapped", &coordinators).unwrap();
        for path in [
            "config/subagent-harness",
            "config/persistent-subagent-harness",
            "config/subagent-dispatch.json",
            "data/projects.json",
        ] {
            assert!(home.join(path).is_file(), "missing {path}");
        }
        let text = fs::read_to_string(home.join("state/coordinator.meta")).unwrap();
        let record = subagent_model::read_meta("coordinator", &text).unwrap();
        assert_eq!(record.role, AssignmentRole::SubOrchestrator);
        assert_eq!(record.artifact, ArtifactKind::Coordination);
        assert!(record.legacy_unknown);
    }

    #[test]
    fn malformed_newer_and_ambiguous_inputs_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let home = home_fixture(&root);
        assert!(inspect(&home, "../unsafe", &BTreeSet::new()).is_err());
        assert!(rollback(&home, "missing").is_err());

        fs::write(home.join("state/bad.meta"), [0xff, 0xfe]).unwrap();
        let report = inspect(&home, "blocked", &BTreeSet::new()).unwrap();
        assert_eq!(report.state, HomeState::Blocked);
        assert!(report.blockers.iter().any(|item| item.contains("UTF-8")));
        fs::remove_file(home.join("state/bad.meta")).unwrap();

        fs::write(home.join("data/projects.md"), "- same x\n- same y\n").unwrap();
        let report = inspect(&home, "projects", &BTreeSet::new()).unwrap();
        assert_eq!(report.state, HomeState::Blocked);
        fs::remove_file(home.join("data/projects.md")).unwrap();

        let report = inspect(&home, "coordinator", &BTreeSet::from(["absent".to_owned()])).unwrap();
        assert_eq!(report.state, HomeState::Blocked);
        assert!(report.blockers.iter().any(|item| item.contains("absent")));

        fs::write(
            home.join(MARKER),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 99,
                "task_writer_version": 2,
                "operation_id": "future",
                "runtime_version": "future",
                "applied_at": "future",
                "backup_manifest": "future"
            }))
            .unwrap(),
        )
        .unwrap();
        let report = inspect(&home, "future", &BTreeSet::new()).unwrap();
        assert_eq!(report.state, HomeState::Blocked);
        assert!(apply(&home, "future", &BTreeSet::new()).is_err());
    }

    #[test]
    fn summaries_and_rollback_refuse_incomplete_or_changed_state() {
        let missing = restart_summary(Path::new("/definitely/missing/multplx-home"), 20);
        assert!(!missing.available);

        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let home = home_fixture(&root);
        fs::write(home.join("state/bad.meta"), "not-metadata\n").unwrap();
        let summary = restart_summary(&home, 20);
        assert!(!summary.available);
        assert!(summary.reason.unwrap().contains("bad"));
        fs::remove_file(home.join("state/bad.meta")).unwrap();

        fs::write(home.join("config/actor-harness"), "codex\n").unwrap();
        apply(&home, "changed", &BTreeSet::new()).unwrap();
        fs::write(home.join("config/subagent-harness"), "user edit\n").unwrap();
        let error = rollback(&home, "changed").unwrap_err();
        assert!(error.contains("changed after migration"));
    }

    #[test]
    fn helper_validation_rejects_unsafe_and_ambiguous_inputs() {
        assert!(!valid_operation(""));
        assert!(!valid_operation(&"a".repeat(129)));
        assert!(!valid_operation("."));
        assert!(!valid_operation("two words"));
        assert!(valid_operation("safe.operation_1"));
        assert!(!relative_safe(Path::new("")));
        assert!(!relative_safe(Path::new("/absolute")));
        assert!(!relative_safe(Path::new("a/../b")));
        assert!(relative_safe(Path::new("state/task.meta")));

        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let regular = root.join("regular");
        fs::write(&regular, "data").unwrap();
        assert!(home(&regular).unwrap_err().contains("real directory"));
        assert!(existing(&root).is_err());
        assert!(task_paths(&root.join("missing")).unwrap().is_empty());
        assert!(task_paths(&regular).is_err());
        assert!(
            legacy_project_names(&root.join("missing-projects"))
                .unwrap()
                .is_empty()
        );

        let projects = root.join("projects.md");
        fs::write(&projects, "heading\n-\n").unwrap();
        assert!(
            legacy_project_names(&projects)
                .unwrap_err()
                .contains("malformed")
        );
        fs::write(&projects, "- ../unsafe /checkout\n").unwrap();
        assert!(
            legacy_project_names(&projects)
                .unwrap_err()
                .contains("unsafe")
        );
        fs::write(&projects, [0xff, 0xfe]).unwrap();
        assert!(
            legacy_project_names(&projects)
                .unwrap_err()
                .contains("UTF-8")
        );

        let fixture = home_fixture(&root);
        fs::write(fixture.join(MARKER), "not json").unwrap();
        assert!(marker(&fixture).unwrap_err().contains("invalid"));
        fs::write(
            fixture.join(MARKER),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": MIGRATION_SCHEMA,
                "task_writer_version": 1,
                "operation_id": "old",
                "runtime_version": env!("CARGO_PKG_VERSION"),
                "applied_at": "2026-09-17T00:00:00Z",
                "backup_manifest": "state/backup.json"
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(marker(&fixture).unwrap_err().contains("unsupported"));
    }

    #[test]
    fn planning_reports_retained_and_unsafe_home_shapes() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let home = home_fixture(&root);
        fs::write(home.join("data/projects.md"), "ignored\n- missing /old\n").unwrap();
        fs::write(home.join("state/.task-writer-version"), "future\n").unwrap();
        let mut current = subagent_model::TaskRecord::new(
            "current".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            "parent".into(),
            "root".into(),
            home.to_string_lossy().into_owned(),
        );
        current.briefs[0].scope = "already current".into();
        fs::write(
            home.join("state/current.meta"),
            subagent_model::write_meta("kind=delivery\n", &current).unwrap(),
        )
        .unwrap();
        let report = inspect(&home, "unsafe-shapes", &BTreeSet::new()).unwrap();
        assert_eq!(report.state, HomeState::Blocked);
        assert_eq!(report.tasks, 1);
        assert_eq!(report.legacy_tasks, 0);
        assert!(report.retained.iter().any(|item| item.contains("missing")));
        assert!(
            report
                .blockers
                .iter()
                .any(|item| item.contains("writer version"))
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            fs::remove_file(home.join("state/.task-writer-version")).unwrap();
            symlink(regular_file(&root), home.join("state/.lock")).unwrap();
            let report = inspect(&home, "unsafe-lock", &BTreeSet::new()).unwrap();
            assert!(report.blockers.iter().any(|item| item.contains("symlink")));
            assert!(
                apply(&home, "unsafe-lock", &BTreeSet::new())
                    .unwrap_err()
                    .contains("symlink")
            );
        }
    }

    #[test]
    fn restart_summary_projects_current_task_context() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let home = home_fixture(&root);
        let mut record = subagent_model::TaskRecord::new(
            "current".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            "parent".into(),
            "root".into(),
            home.to_string_lossy().into_owned(),
        );
        record.briefs[0].scope = "finish migration recovery".into();
        record.accepted_brief_path = Some("state/brief-current.json".into());
        record.schedule.dependencies = vec!["dependency".into()];
        record
            .schedule
            .decisions
            .push(subagent_model::HumanDecision {
                id: "open-question".into(),
                task_id: "current".into(),
                brief_revision: 1,
                workflow_revision: None,
                question: "Which recovery source?".into(),
                answer: None,
            });
        record
            .schedule
            .decisions
            .push(subagent_model::HumanDecision {
                id: "answered-question".into(),
                task_id: "current".into(),
                brief_revision: 1,
                workflow_revision: None,
                question: "Keep the backup?".into(),
                answer: Some("yes".into()),
            });
        let attempt = record.attempt.as_ref().unwrap().clone();
        record
            .delivery
            .history
            .push(super::super::delivery_evidence::DeliveryEvidence {
                evidence_id: "migration-evidence".into(),
                attempt_id: attempt.id,
                attempt_generation: attempt.generation,
                brief_revision: attempt.brief_revision,
                commit: "a".repeat(40),
                accepted_scope: "finish migration recovery".into(),
                checks: Vec::new(),
                review: None,
                limitations: Vec::new(),
                pr_url: Some("https://github.com/example/project/pull/1".into()),
                outcome: super::super::delivery_evidence::DeliveryOutcome::Published,
                observed_at: "2026-09-17T00:00:00Z".into(),
            });
        fs::write(
            home.join("state/current.meta"),
            subagent_model::write_meta("kind=delivery\n", &record).unwrap(),
        )
        .unwrap();

        let summary = restart_summary(&home, 10);
        assert!(summary.available);
        assert_eq!(summary.omitted, 0);
        let task = &summary.tasks[0];
        assert_eq!(task.objective.as_deref(), Some("finish migration recovery"));
        assert_eq!(task.dependencies, ["dependency"]);
        assert_eq!(task.open_questions, ["Which recovery source?"]);
        assert_eq!(
            task.evidence,
            [
                "state/brief-current.json",
                "https://github.com/example/project/pull/1"
            ]
        );
        assert_eq!(task.attempt_generation, Some(1));
        assert!(!task.legacy_unknown);
    }

    fn regular_file(root: &Path) -> PathBuf {
        let path = root.join("lock-target");
        fs::write(&path, "lock").unwrap();
        path
    }

    #[test]
    fn relocation_moves_real_git_worktrees_and_publishes_persistent_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let home = home_fixture(&root);
        apply(&home, "worktrees", &BTreeSet::new()).unwrap();
        let repository = repository(&root);
        let project = project_registry::register_project(
            &home,
            &repository,
            Some("project"),
            CheckoutOwnership::UserOwned,
        )
        .unwrap();

        let ordinary = root.join("legacy-ordinary");
        let persistent = root.join("legacy-persistent");
        git(
            &repository,
            &[
                "worktree",
                "add",
                "--detach",
                ordinary.to_str().unwrap(),
                "HEAD",
            ],
        );
        git(
            &repository,
            &[
                "worktree",
                "add",
                "--detach",
                persistent.to_str().unwrap(),
                "HEAD",
            ],
        );
        let ordinary = fs::canonicalize(ordinary).unwrap();
        let persistent = fs::canonicalize(persistent).unwrap();
        let metadata = root.join("treehouse.json");
        fs::write(
            &metadata,
            serde_json::to_vec(&serde_json::json!({
                "worktrees": [
                    {"path": ordinary, "leased": true, "lease_holder": "ordinary"},
                    {"path": persistent, "leased": true, "lease_holder": "persistent"},
                    {"path": root.join("foreign"), "leased": true, "lease_holder": "foreign"}
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        for (task_id, path, is_persistent) in [
            ("ordinary", ordinary.as_path(), false),
            ("persistent", persistent.as_path(), true),
        ] {
            let mut record = subagent_model::TaskRecord::new(
                task_id.into(),
                AssignmentRole::Implementer,
                ArtifactKind::Implementation,
                is_persistent,
                "orchestrator".into(),
                "orchestrator".into(),
                home.to_string_lossy().into_owned(),
            );
            record.project = Some(project.clone());
            record.briefs[0].scope = format!("migrate {task_id}");
            if is_persistent {
                record.persistent_home = Some(path.to_string_lossy().into_owned());
            }
            let kind = if is_persistent { "daemon" } else { "delivery" };
            let mut compatibility =
                format!("kind={kind}\nbackend=tmux\nworktree={}\n", path.display());
            if is_persistent {
                compatibility.push_str(&format!("home={}\n", path.display()));
            }
            fs::write(
                home.join("state").join(format!("{task_id}.meta")),
                subagent_model::write_meta(&compatibility, &record).unwrap(),
            )
            .unwrap();
        }

        assert!(
            relocate_worktree(
                &home,
                &metadata,
                Path::new("relative"),
                "project",
                "ordinary",
                "bad-request",
            )
            .is_err()
        );
        let ordinary_mapping = relocate_worktree(
            &home,
            &metadata,
            &ordinary,
            "project",
            "ordinary",
            "ordinary-request",
        )
        .unwrap();
        assert!(!ordinary.exists());
        assert!(ordinary_mapping.new_path.is_dir());
        assert_eq!(
            relocate_worktree(
                &home,
                &metadata,
                &ordinary,
                "project",
                "ordinary",
                "ordinary-request",
            )
            .unwrap()
            .new_path,
            ordinary_mapping.new_path
        );

        let persistent_mapping = relocate_worktree(
            &home,
            &metadata,
            &persistent,
            "project",
            "persistent",
            "persistent-request",
        )
        .unwrap();
        let receipt =
            super::super::home_seed::read_home_allocation(&home.join("data"), "persistent")
                .unwrap()
                .unwrap();
        assert_eq!(receipt.binding.path, persistent_mapping.new_path);
        assert_eq!(
            receipt.git_allocation.unwrap().binding,
            persistent_mapping.allocation.binding
        );
    }

    fn walk(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn collect(root: &Path, path: &Path, rows: &mut BTreeMap<PathBuf, Vec<u8>>) {
            let mut entries = fs::read_dir(path)
                .unwrap()
                .collect::<std::io::Result<Vec<_>>>()
                .unwrap();
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    collect(root, &path, rows);
                } else {
                    rows.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut rows = BTreeMap::new();
        collect(root, root, &mut rows);
        rows
    }
}

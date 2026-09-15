//! Canonical Git allocation owner. Records live beside shared Git metadata so
//! independent homes cannot issue competing leases for the same repository.
//! A durable reservation precedes Git; filesystem locks never span Git work.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use multplx_core::filesystem::{atomic_replace, read_bounded_regular};
use multplx_core::locks::DirectoryLock;
use multplx_core::process::{ProcessIdentity, ProcessProbe, SystemProcessProbe};
use serde::{Deserialize, Serialize};

use super::subagent_model::AllocationBinding;
use crate::project_registry::ProjectBinding;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Reserved,
    Active,
    Retained,
    Disposed,
    Removing,
    Removed,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Allocation {
    pub version: u32,
    pub request_id: String,
    pub owner_home: PathBuf,
    pub project: ProjectBinding,
    pub binding: AllocationBinding,
    pub state: State,
    pub reason: Option<String>,
    #[serde(default)]
    pub rebind_from: Option<AllocationBinding>,
    #[serde(default)]
    pub directory_identity: Option<(u64, u64)>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Observation {
    pub allocation: Option<Allocation>,
    pub path: PathBuf,
    pub error: Option<String>,
}

/// Inert, explicitly scoped migration input. No external pool is enumerated or
/// mutated; the caller supplies both the metadata file and its recorded paths.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LegacyAllocation {
    pub metadata_version: u64,
    pub path: PathBuf,
    pub lease_id: Option<String>,
    pub lease_holder: Option<String>,
    pub leased: Option<bool>,
    pub raw: serde_json::Value,
    pub disposition: String,
}

pub fn read_legacy(path: &Path, recorded: &[PathBuf]) -> Result<Vec<LegacyAllocation>> {
    let bytes = read_bounded_regular(path, 4 * 1024 * 1024).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("corrupt legacy inventory retained: {e}"))?;
    let version = match value.get("version") {
        None => 0, // v2.0.1 did not include a version or per-acquisition ID.
        Some(version) => version.as_u64().ok_or("invalid legacy inventory version")?,
    };
    if version > 4 {
        return Err("unknown legacy inventory version; retained".into());
    }
    let entries = value
        .get("worktrees")
        .and_then(serde_json::Value::as_array)
        .ok_or("legacy worktrees array missing")?;
    let mut result = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for entry in entries {
        let path = PathBuf::from(
            entry
                .get("path")
                .and_then(serde_json::Value::as_str)
                .ok_or("legacy path missing")?,
        );
        if !recorded.contains(&path) {
            continue;
        }
        if !path.is_absolute() || !seen.insert(path.clone()) {
            return Err("ambiguous legacy worktree path; retained".into());
        }
        for field in ["lease_id", "lease_holder"] {
            if entry
                .get(field)
                .is_some_and(|value| !value.is_null() && !value.is_string())
            {
                return Err(format!("invalid legacy {field}; retained"));
            }
        }
        if entry
            .get("leased")
            .is_some_and(|value| !value.is_null() && !value.is_boolean())
        {
            return Err("invalid legacy leased observation; retained".into());
        }
        result.push(LegacyAllocation {
            metadata_version: version, path,
            lease_id: entry.get("lease_id").and_then(serde_json::Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned),
            lease_holder: entry.get("lease_holder").and_then(serde_json::Value::as_str).map(str::to_owned),
            leased: entry.get("leased").and_then(serde_json::Value::as_bool),
            raw: entry.clone(), disposition: "retained-legacy: validate ownership, quiesce wrappers, relocate and update references through Phase 09".into(),
        });
    }
    if result.len() != recorded.len() {
        return Err("recorded legacy paths absent from supplied inventory; retained".into());
    }
    Ok(result)
}

/// Phase 09 owns execution. Persist this intent before a Git-supported move;
/// publish the internal allocation only after all reference-owner receipts and
/// the exact old wrapper/process quiescence proof have been reconciled.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RelocationIntent {
    pub version: u32,
    pub operation_id: String,
    pub source_metadata_digest: String,
    pub legacy: LegacyAllocation,
    pub destination: PathBuf,
    pub project: ProjectBinding,
    pub owner_home: PathBuf,
    pub quiesced_processes: Vec<ProcessIdentity>,
    pub reference_receipts: Vec<PathBuf>,
    pub stage: String,
}

impl RelocationIntent {
    /// Structural validation is inert. Phase 09 must additionally verify the
    /// source digest and ownership, prove the complete wrapper inventory
    /// quiesced, and verify each reference owner's receipt before advancing.
    pub fn validate(&self) -> Result<()> {
        let plain_absolute = |path: &Path| {
            path.is_absolute()
                && !path.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
        };
        let digest =
            |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
        if self.version != 1
            || self.operation_id.trim().is_empty()
            || !digest(&self.source_metadata_digest)
            || self.legacy.metadata_version > 4
            || !plain_absolute(&self.legacy.path)
            || !plain_absolute(&self.destination)
            || !plain_absolute(&self.owner_home)
            || self.destination == self.legacy.path
            || self.destination.starts_with(&self.legacy.path)
            || self.legacy.path.starts_with(&self.destination)
            || self.destination.starts_with(&self.project.canonical_path)
            || self.project.canonical_path.starts_with(&self.destination)
            || self.destination.starts_with(&self.project.common_git_dir)
            || self.project.common_git_dir.starts_with(&self.destination)
            || self.destination == self.owner_home
            || self.owner_home.starts_with(&self.destination)
            || self.project.project_id.is_empty()
            || self.project.common_git_identity.is_empty()
            || !plain_absolute(&self.project.canonical_path)
            || !plain_absolute(&self.project.common_git_dir)
            || !matches!(
                self.stage.as_str(),
                "planned" | "quiesced" | "moved" | "references-updated" | "committed"
            )
        {
            return Err("invalid relocation intent; legacy resources retained".into());
        }
        let mut processes = std::collections::BTreeSet::new();
        if self.quiesced_processes.iter().any(|process| {
            process.pid == 0 || process.marker.trim().is_empty() || !processes.insert(process.pid)
        }) {
            return Err("ambiguous relocation process inventory; retained".into());
        }
        let mut receipts = std::collections::BTreeSet::new();
        if self
            .reference_receipts
            .iter()
            .any(|path| !plain_absolute(path) || !receipts.insert(path))
            || (matches!(self.stage.as_str(), "references-updated" | "committed")
                && receipts.is_empty())
        {
            return Err("invalid relocation reference receipts; retained".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Acquire<'a> {
    pub request_id: &'a str,
    pub owner_home: &'a Path,
    pub project: &'a ProjectBinding,
    pub task_id: &'a str,
    pub attempt_id: &'a str,
    pub persistent: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    AfterReservation,
    AfterGit,
    BeforeRemoval,
    AfterRemoval,
}

/// Bounded host operation. Private files avoid pipe deadlocks and bound output;
/// the owned process group is killed on timeout so helpers cannot keep mutating
/// after the repository reservation is released.
pub(crate) fn command_output(command: &mut Command) -> Result<std::process::Output> {
    command_output_with(command, None, Duration::from_secs(30))
}

pub(super) fn command_output_with(
    command: &mut Command,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<std::process::Output> {
    const LIMIT: u64 = 64 * 1024 * 1024;
    let mut stdout = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut stderr = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut stdin = tempfile::tempfile().map_err(|e| e.to_string())?;
    if let Some(input) = input {
        if input.len() as u64 > LIMIT {
            return Err("external operation input exceeds limit; retained".into());
        }
        stdin.write_all(input).map_err(|e| e.to_string())?;
        stdin.rewind().map_err(|e| e.to_string())?;
    }
    command
        .stdin(stdin)
        .stdout(stdout.try_clone().map_err(|e| e.to_string())?)
        .stderr(stderr.try_clone().map_err(|e| e.to_string())?)
        .env("GIT_TERMINAL_PROMPT", "0")
        .process_group(0);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let started = Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => break Err(error.to_string()),
            Ok(None) => {}
        }
        let sizes = stdout
            .metadata()
            .and_then(|a| stderr.metadata().map(|b| (a.len(), b.len())));
        if !matches!(sizes, Ok((a, b)) if a <= LIMIT && b <= LIMIT) {
            break Err("external operation output exceeds limit; retained".into());
        }
        if started.elapsed() >= timeout {
            break Err("external operation timed out; retained".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if result.is_err() {
        let _ = rustix::process::kill_process_group(
            rustix::process::Pid::from_child(&child),
            rustix::process::Signal::KILL,
        );
        let _ = child.kill();
        let _ = child.wait();
    }
    let status = result?;
    let read = |file: &mut fs::File| -> Result<Vec<u8>> {
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > LIMIT {
            return Err("external operation output exceeds limit; retained".into());
        }
        Ok(bytes)
    };
    Ok(std::process::Output {
        status,
        stdout: read(&mut stdout)?,
        stderr: read(&mut stderr)?,
    })
}

fn git(path: &Path, args: &[&str]) -> Result<String> {
    let output = command_output(
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0"),
    )?;
    if !output.status.success() {
        return Err(format!(
            "Git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim_end_matches(['\n', '\r']).to_owned())
        .map_err(|e| e.to_string())
}

fn text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| "non-UTF-8 allocation path".into())
}

fn key(value: &str) -> String {
    crate::maintainer_override::sha256_text(value)
}

fn directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("unsafe allocation directory {}", path.display()));
    }
    Ok(())
}

fn publish(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic_replace(
        path,
        &serde_json::to_vec(value).map_err(|e| e.to_string())?,
        0o600,
    )
    .map_err(|e| e.to_string())
}

fn crash_boundary(point: &str) {
    if std::env::var("MX_WORKTREE_CRASH_AFTER").as_deref() == Ok(point) {
        std::process::exit(96);
    }
}

/// Repository-scoped store. No project registry or task authority is duplicated.
pub struct Store {
    root: PathBuf,
    paths: PathBuf,
    project: ProjectBinding,
}

#[derive(Deserialize, Serialize)]
struct Reservation {
    owner: ProcessIdentity,
    token: String,
}

/// This is an external-operation reservation, not a held filesystem lock.
/// A live or unobservable owner is never displaced on timeout.
struct Operation {
    root: PathBuf,
    token: String,
}
impl Drop for Operation {
    fn drop(&mut self) {
        if let Ok(_lock) = DirectoryLock::acquire_wait(
            self.root.join("lock"),
            &SystemProcessProbe::default(),
            Duration::from_secs(5),
        ) {
            let path = self.root.join("operation.json");
            if read_bounded_regular(&path, 65536)
                .ok()
                .and_then(|b| serde_json::from_slice::<Reservation>(&b).ok())
                .is_some_and(|r| r.token == self.token)
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}

impl Store {
    pub fn new(project: &ProjectBinding) -> Result<Self> {
        crate::project_registry::verify_location(project)?;
        let root = project.common_git_dir.join("multplx-worktrees");
        let parent = project
            .common_git_dir
            .parent()
            .and_then(Path::parent)
            .ok_or("Git directory has no allocation parent")?;
        let paths = parent.join(format!(
            ".multplx-worktrees-{}",
            &key(&project.common_git_identity)[..16]
        ));
        if paths.exists() && fs::canonicalize(&paths).map_err(|e| e.to_string())? != paths {
            return Err("allocation storage traverses a symlink".into());
        }
        Ok(Self {
            root,
            paths,
            project: project.clone(),
        })
    }

    fn operation(&self) -> Result<Operation> {
        directory(&self.root)?;
        directory(&self.root.join("records"))?;
        directory(&self.paths)?;
        let probe = SystemProcessProbe::default();
        let owner = probe
            .identity(std::process::id())
            .map_err(|e| e.to_string())?;
        let token = key(&format!(
            "{}:{:?}:{}",
            owner.marker,
            std::time::SystemTime::now(),
            std::process::id()
        ));
        let started = Instant::now();
        loop {
            {
                let _lock = DirectoryLock::acquire_wait(
                    self.root.join("lock"),
                    &probe,
                    Duration::from_secs(5),
                )
                .map_err(|e| e.to_string())?;
                let path = self.root.join("operation.json");
                let available = match read_bounded_regular(&path, 65536) {
                    Ok(bytes) => {
                        let old: Reservation = serde_json::from_slice(&bytes)
                            .map_err(|_| "corrupt repository reservation; retained")?;
                        !probe.is_alive(old.owner.pid)
                            || probe
                                .identity(old.owner.pid)
                                .is_ok_and(|actual| actual != old.owner)
                    }
                    Err(multplx_core::error::CoreError::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotFound =>
                    {
                        true
                    }
                    Err(e) => return Err(e.to_string()),
                };
                if available {
                    publish(
                        &path,
                        &Reservation {
                            owner: owner.clone(),
                            token: token.clone(),
                        },
                    )?;
                    return Ok(Operation {
                        root: self.root.clone(),
                        token,
                    });
                }
            }
            if started.elapsed() >= Duration::from_secs(5) {
                return Err("repository operation busy; retry the same request".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn record_path(&self, id: &str) -> Result<PathBuf> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid allocation identity".into());
        }
        Ok(self.root.join("records").join(format!("{id}.json")))
    }

    fn validate(&self, allocation: &Allocation) -> Result<()> {
        let b = &allocation.binding;
        if allocation.version != 1
            || b.generation == 0
            || b.lease_id.is_empty()
            || allocation.request_id.is_empty()
            || !allocation.owner_home.is_absolute()
            || b.common_git_identity != self.project.common_git_identity
            || b.project_id != self.project.project_id
            || b.path != text(&self.paths.join(&b.allocation_id))?
            || b.base_revision != allocation.project.starting_revision
            || b.checkout_id != allocation.project.checkout_id
            || b.common_git_identity != allocation.project.common_git_identity
            || b.task_id.is_empty()
            || b.attempt_id.is_empty()
        {
            return Err("corrupt or foreign allocation; retained".into());
        }
        if let Some(previous) = &allocation.rebind_from {
            let mut expected = previous.clone();
            expected.generation = previous
                .generation
                .checked_add(1)
                .ok_or("invalid handoff generation")?;
            expected.lease_id = b.lease_id.clone();
            expected.attempt_id = b.attempt_id.clone();
            if expected != *b
                || previous.lease_id == b.lease_id
                || previous.attempt_id == b.attempt_id
            {
                return Err("corrupt handoff receipt; retained".into());
            }
        }
        self.record_path(&b.allocation_id)?;
        Ok(())
    }

    pub fn inspect(&self, id: &str) -> Result<Allocation> {
        let bytes =
            read_bounded_regular(self.record_path(id)?, 1024 * 1024).map_err(|e| e.to_string())?;
        let allocation: Allocation = serde_json::from_slice(&bytes)
            .map_err(|e| format!("corrupt allocation retained: {e}"))?;
        self.validate(&allocation)?;
        if allocation.binding.allocation_id != id {
            return Err("allocation record identity mismatch".into());
        }
        Ok(allocation)
    }

    /// Observe one exact task binding without enumerating all tasks or treating
    /// a durable active receipt as proof that its directory still exists.
    pub fn observe(&self, token: &AllocationBinding) -> Result<Observation> {
        let allocation = self.inspect(&token.allocation_id)?;
        let error = if allocation.binding != *token {
            Some("task allocation binding is superseded or inconsistent".into())
        } else if allocation.state == State::Removed {
            None
        } else {
            self.verify_worktree(&allocation, false).err()
        };
        Ok(Observation {
            path: PathBuf::from(&allocation.binding.path),
            allocation: Some(allocation),
            error,
        })
    }

    fn save(&self, allocation: &Allocation) -> Result<()> {
        self.validate(allocation)?;
        publish(
            &self.record_path(&allocation.binding.allocation_id)?,
            allocation,
        )
    }

    pub fn list(&self) -> Result<Vec<Observation>> {
        let mut result = Vec::new();
        let mut known = std::collections::BTreeSet::new();
        let entries = if self.root.join("records").exists() {
            fs::read_dir(self.root.join("records"))
                .map_err(|e| e.to_string())?
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?
        } else {
            Vec::new()
        };
        for entry in entries {
            let path = entry.path();
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            match self.inspect(id) {
                Ok(allocation) => {
                    known.insert(PathBuf::from(&allocation.binding.path));
                    let error = if allocation.state == State::Removed {
                        None
                    } else {
                        self.verify_worktree(&allocation, false).err()
                    };
                    result.push(Observation {
                        path: PathBuf::from(&allocation.binding.path),
                        allocation: Some(allocation),
                        error,
                    });
                }
                Err(error) => result.push(Observation {
                    allocation: None,
                    path,
                    error: Some(error),
                }),
            }
        }
        for path in self.inventory()? {
            if known.insert(path.clone()) {
                result.push(Observation {
                    allocation: None,
                    path,
                    error: Some("unmanaged Git worktree; never eligible for cleanup".into()),
                });
            }
        }
        if self.paths.exists() {
            for entry in fs::read_dir(&self.paths).map_err(|e| e.to_string())? {
                let path = entry.map_err(|e| e.to_string())?.path();
                if known.insert(path.clone()) {
                    result.push(Observation {
                        allocation: None,
                        path,
                        error: Some("unexplained allocation directory or file; retained".into()),
                    });
                }
            }
        }
        result.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(result)
    }

    fn inventory(&self) -> Result<Vec<PathBuf>> {
        let value = git(
            &self.project.canonical_path,
            &["worktree", "list", "--porcelain", "-z"],
        )?;
        Ok(value
            .split('\0')
            .filter_map(|s| s.strip_prefix("worktree ").map(PathBuf::from))
            .collect())
    }

    fn verify_worktree(&self, allocation: &Allocation, base: bool) -> Result<()> {
        let path = Path::new(&allocation.binding.path);
        let metadata =
            fs::symlink_metadata(path).map_err(|e| format!("allocation path unavailable: {e}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("allocation path is not a real directory".into());
        }
        if allocation
            .directory_identity
            .is_some_and(|identity| identity != (metadata.dev(), metadata.ino()))
            || (allocation.directory_identity.is_none() && allocation.state != State::Reserved)
        {
            return Err("allocation directory identity changed or unproven; retained".into());
        }
        let top = fs::canonicalize(git(path, &["rev-parse", "--show-toplevel"])?)
            .map_err(|e| e.to_string())?;
        let common = fs::canonicalize(git(
            path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?)
        .map_err(|e| e.to_string())?;
        if top != path || common != self.project.common_git_dir || !self.inventory()?.contains(&top)
        {
            return Err("worktree identity mismatch; retained".into());
        }
        if base && git(path, &["rev-parse", "HEAD"])? != allocation.binding.base_revision {
            return Err("acquisition HEAD differs from recorded base; retained".into());
        }
        Ok(())
    }

    // Rebinding keeps the directory/allocation ID while changing its request.
    // Resolve requests through validated records under the operation reservation
    // so acquire and replacement retries share one idempotence namespace.
    fn find_request(&self, owner_home: &Path, request_id: &str) -> Result<Option<Allocation>> {
        let mut found = None;
        for entry in fs::read_dir(self.root.join("records")).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let id = path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or("invalid allocation record path")?;
            let current = self.inspect(id)?;
            if current.owner_home == owner_home && current.request_id == request_id {
                if found.is_some() {
                    return Err("ambiguous allocation request ownership; retained".into());
                }
                found = Some(current);
            }
        }
        Ok(found)
    }

    pub fn acquire(&self, request: &Acquire<'_>, fault: Option<Fault>) -> Result<Allocation> {
        if request.request_id.is_empty()
            || request.task_id.is_empty()
            || request.attempt_id.is_empty()
        {
            return Err("request, task and attempt identity are required".into());
        }
        if *request.project != self.project {
            return Err("acquisition project mismatch".into());
        }
        let owner_home = fs::canonicalize(request.owner_home).map_err(|e| e.to_string())?;
        let _operation = self.operation()?;
        let id = self
            .find_request(&owner_home, request.request_id)?
            .map(|allocation| allocation.binding.allocation_id)
            .unwrap_or_else(|| key(&format!("{}:{}", owner_home.display(), request.request_id)));
        let path = self.record_path(&id)?;
        let mut allocation = if fs::symlink_metadata(&path).is_ok() {
            let previous = self.inspect(&id)?;
            if previous.request_id != request.request_id
                || previous.owner_home != owner_home
                || previous.project != *request.project
                || previous.binding.task_id != request.task_id
                || previous.binding.attempt_id != request.attempt_id
                || previous.binding.persistent != request.persistent
            {
                return Err("request identity conflicts with recorded acquisition".into());
            }
            if previous.state == State::Active {
                self.verify_worktree(&previous, false)?;
                return Ok(previous);
            }
            if previous.state != State::Reserved {
                return Err(
                    "request already disposed or retained; use a new request for new work".into(),
                );
            }
            previous
        } else {
            let path = self.paths.join(&id);
            if fs::symlink_metadata(&path).is_ok() || self.inventory()?.contains(&path) {
                return Err("unexplained allocation path; retained".into());
            }
            let resolved = git(
                &self.project.canonical_path,
                &[
                    "rev-parse",
                    "--verify",
                    &format!("{}^{{commit}}", self.project.starting_revision),
                ],
            )?;
            if resolved != self.project.starting_revision {
                return Err("base must be an exact recorded commit".into());
            }
            let allocation = Allocation {
                version: 1,
                request_id: request.request_id.into(),
                owner_home,
                project: request.project.clone(),
                state: State::Reserved,
                reason: None,
                rebind_from: None,
                directory_identity: None,
                binding: AllocationBinding {
                    allocation_id: id.clone(),
                    lease_id: key(&format!("{id}:{:?}", std::time::SystemTime::now())),
                    generation: 1,
                    project_id: request.project.project_id.clone(),
                    checkout_id: request.project.checkout_id.clone(),
                    common_git_identity: request.project.common_git_identity.clone(),
                    path: text(&path)?.into(),
                    base_revision: request.project.starting_revision.clone(),
                    task_id: request.task_id.into(),
                    attempt_id: request.attempt_id.into(),
                    persistent: request.persistent,
                },
            };
            self.save(&allocation)?;
            allocation
        };
        crash_boundary("reservation");
        if fault == Some(Fault::AfterReservation) {
            return Err("injected after allocation reservation".into());
        }
        let path = Path::new(&allocation.binding.path);
        if !path.exists() && !self.inventory()?.contains(&path.to_path_buf()) {
            git(
                &self.project.canonical_path,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    "--",
                    text(path)?,
                    &allocation.binding.base_revision,
                ],
            )?;
        }
        crash_boundary("git");
        if fault == Some(Fault::AfterGit) {
            return Err("injected after Git acquisition".into());
        }
        let observed_directory = fs::symlink_metadata(&allocation.binding.path)
            .ok()
            .map(|metadata| (metadata.dev(), metadata.ino()));
        if let Err(error) = self.verify_worktree(&allocation, true) {
            allocation.state = State::Retained;
            allocation.reason = Some(error.clone());
            self.save(&allocation)?;
            return Err(error);
        }
        let metadata = fs::symlink_metadata(&allocation.binding.path).map_err(|e| e.to_string())?;
        if observed_directory != Some((metadata.dev(), metadata.ino())) {
            allocation.state = State::Retained;
            allocation.reason =
                Some("allocation directory changed during acquisition; retained".into());
            self.save(&allocation)?;
            return Err("allocation directory changed during acquisition; retained".into());
        }
        allocation.directory_identity = observed_directory;
        allocation.state = State::Active;
        self.save(&allocation)?;
        Ok(allocation)
    }

    /// Transfer the same retained progress to the next attempt. The previous
    /// full token is retained only as a receipt for idempotent handoff retries;
    /// it never authorizes release, prune, retain or another handoff.
    pub fn rebind(
        &self,
        token: &AllocationBinding,
        request_id: &str,
        task_id: &str,
        attempt_id: &str,
    ) -> Result<Allocation> {
        if request_id.is_empty() || task_id.is_empty() || attempt_id.is_empty() {
            return Err("request, task and attempt identity are required".into());
        }
        let _operation = self.operation()?;
        let mut current = self.inspect(&token.allocation_id)?;
        if current.rebind_from.as_ref() == Some(token)
            && current.request_id == request_id
            && current.binding.task_id == task_id
            && current.binding.attempt_id == attempt_id
            && current.state == State::Active
        {
            self.verify_worktree(&current, false)?;
            return Ok(current);
        }
        if current.binding != *token {
            return Err("stale allocation handoff token; retained".into());
        }
        if token.persistent || !matches!(current.state, State::Active | State::Retained) {
            return Err("handoff requires an active or retained ordinary allocation".into());
        }
        if task_id != token.task_id
            || attempt_id == token.attempt_id
            || request_id == current.request_id
        {
            return Err("handoff requires the same task and a new request and attempt".into());
        }
        if self
            .find_request(&current.owner_home, request_id)?
            .is_some()
        {
            return Err(
                "replacement request already belongs to another allocation; retained".into(),
            );
        }
        self.verify_worktree(&current, false)?;
        occupants(Path::new(&token.path))?;
        current.rebind_from = Some(token.clone());
        current.request_id = request_id.into();
        current.binding.generation = token
            .generation
            .checked_add(1)
            .ok_or("allocation generation exhausted")?;
        current.binding.lease_id = key(&format!(
            "{}:{}:{:?}",
            token.allocation_id,
            current.binding.generation,
            std::time::SystemTime::now()
        ));
        current.binding.task_id = task_id.into();
        current.binding.attempt_id = attempt_id.into();
        current.state = State::Active;
        current.reason = Some("owned progress transferred to replacement attempt".into());
        self.save(&current)?;
        Ok(current)
    }

    fn token(&self, token: &AllocationBinding) -> Result<Allocation> {
        let current = self.inspect(&token.allocation_id)?;
        if current.binding != *token {
            return Err("stale or foreign allocation lease".into());
        }
        Ok(current)
    }

    pub fn retain(&self, token: &AllocationBinding, reason: &str) -> Result<Allocation> {
        if reason.trim().is_empty() {
            return Err("retention requires a reason".into());
        }
        let _operation = self.operation()?;
        let mut current = self.token(token)?;
        if matches!(current.state, State::Removed | State::Removing) {
            return Err("allocation is already removing or removed".into());
        }
        current.state = State::Retained;
        current.reason = Some(reason.into());
        self.save(&current)?;
        Ok(current)
    }

    /// Release is deliberately separate from removal. The caller must stop its
    /// verified endpoint first; the owner independently refuses all occupants,
    /// unknown ignored data, and unlanded work. Persistent homes require their
    /// dedicated home lifecycle; a shell exit cannot release them.
    pub fn release(&self, token: &AllocationBinding) -> Result<Allocation> {
        let _operation = self.operation()?;
        let mut current = self.token(token)?;
        if current.state == State::Disposed || current.state == State::Removed {
            return Ok(current);
        }
        if current.binding.persistent {
            return Err("persistent home allocation requires home retirement; retained".into());
        }
        if let Err(error) = self.safe(&current) {
            current.state = State::Retained;
            current.reason = Some(error.clone());
            self.save(&current)?;
            return Err(error);
        }
        current.state = State::Disposed;
        current.reason = Some("clean, landed and without occupants".into());
        self.save(&current)?;
        Ok(current)
    }

    fn safe(&self, allocation: &Allocation) -> Result<()> {
        self.verify_worktree(allocation, false)?;
        let path = Path::new(&allocation.binding.path);
        super::teardown::git_status_after_stale_lock_cleanup(path)?;
        if !git(
            path,
            &[
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--ignored",
            ],
        )?
        .is_empty()
        {
            return Err("dirty, untracked or unclassified ignored content; retained".into());
        }
        if super::teardown::publication_blocked(path, None) {
            return Err("open pull request or uncertain publication evidence; retained".into());
        }
        let head = git(path, &["rev-parse", "HEAD"])?;
        if head != allocation.binding.base_revision
            && !super::teardown::allocation_landed(path, &self.project.canonical_path)
        {
            return Err("unlanded or uncertain publication work; retained".into());
        }
        occupants(path)?;
        Ok(())
    }

    /// Prune exactly one disposed allocation. Preview uses the same checks as
    /// apply. No broad Git prune, force, reset, clean or recursive deletion.
    pub fn prune(
        &self,
        token: &AllocationBinding,
        apply: bool,
        fault: Option<Fault>,
    ) -> Result<Allocation> {
        let _operation = self.operation()?;
        let mut current = self.token(token)?;
        if current.state == State::Removed {
            return Ok(current);
        }
        if !matches!(current.state, State::Disposed | State::Removing) {
            return Err("prune requires an explicitly disposed allocation".into());
        }
        let path = Path::new(&current.binding.path);
        if current.state == State::Removing
            && !path.exists()
            && !self.inventory()?.contains(&path.to_path_buf())
        {
            current.state = State::Removed;
            self.save(&current)?;
            return Ok(current);
        }
        self.safe(&current)?;
        if !apply {
            return Ok(current);
        }
        current.state = State::Removing;
        self.save(&current)?;
        crash_boundary("remove-intent");
        if fault == Some(Fault::BeforeRemoval) {
            return Err("injected before Git removal".into());
        }
        let directory = fs::symlink_metadata(&current.binding.path).map_err(|e| e.to_string())?;
        if directory.file_type().is_symlink()
            || current.directory_identity != Some((directory.dev(), directory.ino()))
        {
            return Err("allocation directory changed before removal; retained".into());
        }
        git(
            &self.project.canonical_path,
            &["worktree", "remove", "--", &current.binding.path],
        )?;
        crash_boundary("remove-git");
        if fault == Some(Fault::AfterRemoval) {
            return Err("injected after Git removal".into());
        }
        current.state = State::Removed;
        self.save(&current)?;
        Ok(current)
    }
}

pub(super) fn occupants(path: &Path) -> Result<()> {
    let output = command_output(Command::new("lsof").args(["-nP", "-t", "+D"]).arg(path))
        .map_err(|e| format!("occupant liveness unknown; retained: {e}"))?;
    if output.status.code() == Some(1) && output.stdout.is_empty() && output.stderr.is_empty() {
        return Ok(());
    }
    Err("worktree occupants present or liveness unknown; retained".into())
}

/// Probe the installed Git's actual option surface without requiring or
/// modifying a repository (including packaged non-Git runtime homes).
pub fn capability() -> Result<String> {
    for (command, required) in [
        (
            "worktree",
            &[
                "git worktree add",
                "--detach",
                "git worktree list",
                "--porcelain",
                "-z",
                "git worktree remove",
            ][..],
        ),
        ("merge-tree", &["--write-tree"][..]),
    ] {
        let output = command_output(Command::new("git").args([command, "-h"]).env("LC_ALL", "C"))?;
        let help = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        validate_capability_help(command, output.status.code(), &help, required)?;
    }
    Ok("built-in Git allocation, NUL-delimited inventory, scoped removal and landing checks available".into())
}

fn validate_capability_help(
    command: &str,
    status: Option<i32>,
    help: &str,
    required: &[&str],
) -> Result<()> {
    if !matches!(status, Some(0 | 129)) {
        return Err(format!(
            "Git {command} capability probe failed (status {status:?})"
        ));
    }
    for option in required {
        if !help.contains(option) {
            return Err(format!("Git {command} lacks required capability {option}"));
        }
    }
    Ok(())
}

pub const USAGE: &str = "Built-in Git worktree lifecycle (JSON output).\n\
usage: mx worktree acquire PROJECT --request ID --task ID --attempt ID --base COMMIT [--persistent]\n\
       mx worktree legacy-inspect METADATA RECORDED_PATH...\n\
       mx worktree list PROJECT\n\
       mx worktree inspect PROJECT ALLOCATION\n\
       mx worktree {release|retain|prune} PROJECT ALLOCATION --lease ID --generation N --attempt ID [--reason TEXT] [--apply]\n\
PROJECT is a registered selector or existing checkout path. Base is an exact commit, never fetched implicitly.\n\
Release records safe disposition; prune previews one disposed allocation unless --apply is supplied.\n\
Dirty, ignored, unlanded, persistent or uncertain work is retained. Unknown/foreign paths are never adopted.\n";

pub fn run(args: &[std::ffi::OsString], home: &Path) -> Result<String> {
    let args = args
        .iter()
        .map(|s| s.to_str().ok_or_else(|| "non-UTF-8 argument".to_owned()))
        .collect::<Result<Vec<_>>>()?;
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        return Ok(USAGE.into());
    }
    if let ["legacy-inspect", source, paths @ ..] = args.as_slice() {
        if paths.is_empty() {
            return Err("legacy-inspect requires explicitly recorded paths".into());
        }
        return serde_json::to_string_pretty(&read_legacy(
            Path::new(source),
            &paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
        )?)
        .map_err(|e| e.to_string());
    }
    let [operation, selector, tail @ ..] = args.as_slice() else {
        return Err(USAGE.into());
    };
    let mut project = if Path::new(selector).exists() {
        crate::project_registry::bind_project(home, Path::new(selector))?
    } else {
        crate::project_registry::resolve_checkout(home, selector)?
    };
    let (id, options) = if matches!(*operation, "inspect" | "release" | "retain" | "prune") {
        let [id, options @ ..] = tail else {
            return Err(USAGE.into());
        };
        (Some(*id), options)
    } else {
        (None, tail)
    };
    let mut flags = std::collections::BTreeMap::new();
    let mut index = 0;
    while index < options.len() {
        let flag = options[index];
        if !matches!(
            flag,
            "--persistent"
                | "--apply"
                | "--request"
                | "--task"
                | "--attempt"
                | "--base"
                | "--lease"
                | "--generation"
                | "--reason"
        ) {
            return Err(format!("unknown worktree option {flag}"));
        }
        let allowed = match *operation {
            "acquire" => matches!(
                flag,
                "--request" | "--task" | "--attempt" | "--base" | "--persistent"
            ),
            "release" => matches!(flag, "--lease" | "--generation" | "--attempt"),
            "retain" => matches!(flag, "--lease" | "--generation" | "--attempt" | "--reason"),
            "prune" => matches!(flag, "--lease" | "--generation" | "--attempt" | "--apply"),
            _ => false,
        };
        if !allowed {
            return Err(format!("option {flag} is not valid for {operation}"));
        }
        let value = if matches!(flag, "--persistent" | "--apply") {
            "true"
        } else {
            index += 1;
            *options
                .get(index)
                .ok_or_else(|| format!("missing {flag} value"))?
        };
        if flags.insert(flag, value).is_some() {
            return Err(format!("duplicate option {flag}"));
        }
        index += 1;
    }
    let required = |flag| {
        flags
            .get(flag)
            .copied()
            .ok_or_else(|| format!("required option {flag}"))
    };
    if *operation == "acquire" {
        project.starting_revision = required("--base")?.into();
    }
    let store = Store::new(&project)?;
    let value = match *operation {
        "acquire" => serde_json::to_value(store.acquire(
            &Acquire {
                request_id: required("--request")?,
                owner_home: home,
                project: &project,
                task_id: required("--task")?,
                attempt_id: required("--attempt")?,
                persistent: flags.contains_key("--persistent"),
            },
            None,
        )?),
        "list" if flags.is_empty() => serde_json::to_value(store.list()?),
        "inspect" if flags.is_empty() => {
            serde_json::to_value(store.inspect(id.expect("parsed identity"))?)
        }
        "release" | "retain" | "prune" => {
            let current = store.inspect(id.expect("parsed identity"))?;
            current.binding.validate_token(
                &current.binding.allocation_id,
                required("--lease")?,
                required("--generation")?
                    .parse()
                    .map_err(|_| "invalid generation")?,
                &project.project_id,
                required("--attempt")?,
            )?;
            if current.owner_home != fs::canonicalize(home).map_err(|e| e.to_string())? {
                return Err("allocation belongs to another home".into());
            }
            serde_json::to_value(match *operation {
                "release" => store.release(&current.binding)?,
                "retain" => store.retain(&current.binding, required("--reason")?)?,
                _ => store.prune(&current.binding, flags.contains_key("--apply"), None)?,
            })
        }
        _ => return Err(USAGE.into()),
    }
    .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, ProjectBinding) {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let project = root.join("project");
        let home = root.join("home");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&home).unwrap();
        git(&project, &["init", "-b", "main"]).unwrap();
        git(&project, &["config", "user.name", "Test"]).unwrap();
        git(&project, &["config", "user.email", "test@example.invalid"]).unwrap();
        fs::write(project.join("file"), "base\n").unwrap();
        fs::write(project.join(".gitignore"), "cache/\n").unwrap();
        git(&project, &["add", "."]).unwrap();
        git(&project, &["commit", "-m", "base"]).unwrap();
        let binding = crate::project_registry::bind_project(&home, &project).unwrap();
        (temp, home, binding)
    }

    fn request<'a>(home: &'a Path, project: &'a ProjectBinding, id: &'a str) -> Acquire<'a> {
        Acquire {
            request_id: id,
            owner_home: home,
            project,
            task_id: "task",
            attempt_id: id,
            persistent: false,
        }
    }

    #[test]
    fn replacement_of_owned_directory_at_the_same_git_path_rejects_stale_ownership() {
        let (_temp, home, project) = fixture();
        let store = Store::new(&project).unwrap();
        let allocation = store
            .acquire(&request(&home, &project, "directory"), None)
            .unwrap();
        let path = Path::new(&allocation.binding.path);
        // Git-supported move preserves the old inode, making this deterministic
        // even on filesystems that eagerly reuse a just-removed directory inode.
        let moved = path.with_file_name("preserved-old-directory");
        git(
            &project.canonical_path,
            &[
                "worktree",
                "move",
                text(path).unwrap(),
                text(&moved).unwrap(),
            ],
        )
        .unwrap();
        git(
            &project.canonical_path,
            &[
                "worktree",
                "add",
                "--detach",
                text(path).unwrap(),
                &allocation.binding.base_revision,
            ],
        )
        .unwrap();
        assert!(
            store
                .release(&allocation.binding)
                .unwrap_err()
                .contains("directory identity")
        );
        assert!(
            store
                .rebind(
                    &allocation.binding,
                    "replacement",
                    &allocation.binding.task_id,
                    "next"
                )
                .is_err()
        );
        assert!(
            store
                .observe(&allocation.binding)
                .unwrap()
                .error
                .unwrap()
                .contains("directory identity")
        );
        assert!(path.exists() && moved.exists());
    }

    #[test]
    fn relocation_receipts_validate_inertly_and_unknown_legacy_lease_is_not_free() {
        let (_temp, home, project) = fixture();
        let legacy = LegacyAllocation {
            metadata_version: 0,
            path: home.join("legacy"),
            lease_id: None,
            lease_holder: Some("domain".into()),
            leased: None,
            raw: serde_json::json!({"path": home.join("legacy")}),
            disposition: "retained-legacy".into(),
        };
        let mut intent = RelocationIntent {
            version: 1,
            operation_id: "relocate-1".into(),
            source_metadata_digest: "a".repeat(64),
            legacy,
            destination: home.with_file_name("owned-destination"),
            project,
            owner_home: home.clone(),
            quiesced_processes: Vec::new(),
            reference_receipts: Vec::new(),
            stage: "planned".into(),
        };
        intent.validate().unwrap();
        intent.stage = "committed".into();
        assert!(intent.validate().is_err());
        intent
            .reference_receipts
            .push(home.join("reference-receipt.json"));
        intent.validate().unwrap();
        intent.destination = intent.legacy.path.clone();
        assert!(intent.validate().is_err());
        intent.destination = home.with_file_name("owned-destination");
        intent.quiesced_processes.push(ProcessIdentity {
            pid: 0,
            marker: "unknown".into(),
        });
        assert!(intent.validate().is_err());
    }

    #[test]
    fn capability_observation_requires_successful_help_and_every_required_option() {
        assert!(
            validate_capability_help(
                "worktree",
                Some(129),
                "usage: git worktree add --detach",
                &["git worktree add", "--detach"]
            )
            .is_ok()
        );
        assert!(
            validate_capability_help(
                "worktree",
                Some(0),
                "usage: git worktree add",
                &["--detach"]
            )
            .unwrap_err()
            .contains("lacks required")
        );
        assert!(
            validate_capability_help("worktree", Some(1), "--detach", &["--detach"])
                .unwrap_err()
                .contains("failed")
        );
        assert!(validate_capability_help("worktree", None, "--detach", &["--detach"]).is_err());
        assert!(capability().unwrap().contains("NUL-delimited"));
    }

    #[test]
    fn replacement_preserves_dirty_progress_and_retries_without_accepting_stale_tokens() {
        let (_temp, home, project) = fixture();
        let store = Store::new(&project).unwrap();
        let allocation = store
            .acquire(&request(&home, &project, "old"), None)
            .unwrap();
        fs::write(
            Path::new(&allocation.binding.path).join("progress"),
            "unfinished",
        )
        .unwrap();
        let next = store
            .rebind(
                &allocation.binding,
                "replacement",
                &allocation.binding.task_id,
                "next-attempt",
            )
            .unwrap();
        assert_eq!(next.binding.path, allocation.binding.path);
        let repeated = store
            .acquire(
                &Acquire {
                    request_id: "replacement",
                    owner_home: &home,
                    project: &project,
                    task_id: &allocation.binding.task_id,
                    attempt_id: "next-attempt",
                    persistent: false,
                },
                None,
            )
            .unwrap();
        assert_eq!(repeated, next);
        let other = store
            .acquire(&request(&home, &project, "conflicting-replacement"), None)
            .unwrap();
        assert!(
            store
                .rebind(
                    &other.binding,
                    "replacement",
                    &other.binding.task_id,
                    "next"
                )
                .unwrap_err()
                .contains("already belongs")
        );
        assert_eq!(next.owner_home, allocation.owner_home);
        assert_eq!(next.project, allocation.project);
        assert_eq!(
            store
                .list()
                .unwrap()
                .iter()
                .filter(|row| row.path == Path::new(&next.binding.path))
                .count(),
            1
        );
        assert_eq!(next.binding.generation, allocation.binding.generation + 1);
        assert_ne!(next.binding.lease_id, allocation.binding.lease_id);
        assert!(store.observe(&next.binding).unwrap().error.is_none());
        assert!(
            store
                .observe(&allocation.binding)
                .unwrap()
                .error
                .unwrap()
                .contains("superseded")
        );
        assert_eq!(
            next,
            store
                .rebind(
                    &allocation.binding,
                    "replacement",
                    &allocation.binding.task_id,
                    "next-attempt"
                )
                .unwrap()
        );
        assert!(store.release(&allocation.binding).is_err());
        assert!(store.retain(&allocation.binding, "late").is_err());
        assert!(store.prune(&allocation.binding, true, None).is_err());
        assert!(
            store
                .rebind(
                    &allocation.binding,
                    "other",
                    &allocation.binding.task_id,
                    "another"
                )
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(Path::new(&next.binding.path).join("progress")).unwrap(),
            "unfinished"
        );
        assert!(store.release(&next.binding).unwrap_err().contains("dirty"));
        fs::rename(
            &next.binding.path,
            Path::new(&next.binding.path).with_extension("missing"),
        )
        .unwrap();
        assert!(store.observe(&next.binding).unwrap().error.is_some());
    }

    #[test]
    fn bounded_command_terminates_hung_helpers_and_captures_large_output() {
        let started = Instant::now();
        let error = command_output_with(
            Command::new("sh").args(["-c", "sleep 30 & wait"]),
            None,
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(error.contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(3));
        let output = command_output_with(
            Command::new("sh").args(["-c", "cat; printf error >&2"]),
            Some(&vec![b'x'; 128 * 1024]),
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(output.stdout.len(), 128 * 1024);
        assert_eq!(output.stderr, b"error");
    }

    #[test]
    fn inventory_reports_unexplained_directories_and_cli_rejects_irrelevant_flags() {
        let (_temp, home, project) = fixture();
        let store = Store::new(&project).unwrap();
        let allocation = store
            .acquire(&request(&home, &project, "first"), None)
            .unwrap();
        let unexplained = store.paths.join("unexplained");
        fs::create_dir(&unexplained).unwrap();
        fs::write(unexplained.join("keep"), "unfinished").unwrap();
        assert!(
            store
                .list()
                .unwrap()
                .iter()
                .any(|row| row.path == unexplained
                    && row.allocation.is_none()
                    && row.error.as_deref().unwrap().contains("unexplained"))
        );
        for (operation, flag) in [
            ("acquire", "--apply"),
            ("release", "--persistent"),
            ("prune", "--reason"),
            ("retain", "--apply"),
        ] {
            let mut args = vec![
                operation.into(),
                project.canonical_path.as_os_str().to_owned(),
            ];
            if operation != "acquire" {
                args.push(allocation.binding.allocation_id.clone().into());
            }
            args.push(flag.into());
            assert!(run(&args, &home).unwrap_err().contains("is not valid"));
        }
        assert!(unexplained.join("keep").exists());
        assert!(USAGE.contains("legacy-inspect"));
    }

    #[test]
    fn exact_base_retry_and_cross_home_isolation_preserve_dirty_borrowed_source() {
        let (_temp, home, binding) = fixture();
        fs::write(binding.canonical_path.join("file"), "borrowed dirty\n").unwrap();
        let store = Store::new(&binding).unwrap();
        let req = request(&home, &binding, "one");
        let one = store.acquire(&req, None).unwrap();
        assert_eq!(one, store.acquire(&req, None).unwrap());
        assert_eq!(
            git(Path::new(&one.binding.path), &["rev-parse", "HEAD"]).unwrap(),
            binding.starting_revision
        );
        assert_eq!(
            fs::read_to_string(Path::new(&one.binding.path).join("file")).unwrap(),
            "base\n"
        );
        let other = home.with_file_name("other");
        fs::create_dir(&other).unwrap();
        let two = store
            .acquire(&request(&other, &binding, "one"), None)
            .unwrap();
        assert_ne!(one.binding.path, two.binding.path);
        assert_eq!(
            fs::read_to_string(binding.canonical_path.join("file")).unwrap(),
            "borrowed dirty\n"
        );
        let mut conflict = req;
        conflict.attempt_id = "different";
        assert!(store.acquire(&conflict, None).is_err());
    }

    #[test]
    fn each_acquisition_interruption_reconciles_one_git_worktree() {
        for fault in [Fault::AfterReservation, Fault::AfterGit] {
            let (_temp, home, binding) = fixture();
            let store = Store::new(&binding).unwrap();
            let req = request(&home, &binding, "crash");
            assert!(store.acquire(&req, Some(fault)).is_err());
            let allocation = store.acquire(&req, None).unwrap();
            assert_eq!(allocation.state, State::Active);
            assert_eq!(store.inventory().unwrap().len(), 2);
        }
    }

    #[test]
    fn corrupt_and_incomplete_allocations_never_become_free() {
        let (_temp, home, binding) = fixture();
        let store = Store::new(&binding).unwrap();
        let req = request(&home, &binding, "crash");
        assert!(store.acquire(&req, Some(Fault::AfterReservation)).is_err());
        let id = key(&format!("{}:crash", home.display()));
        let record = store.inspect(&id).unwrap();
        fs::create_dir(&record.binding.path).unwrap();
        fs::write(Path::new(&record.binding.path).join("unfinished"), "keep").unwrap();
        assert!(store.acquire(&req, None).is_err());
        assert_eq!(store.inspect(&id).unwrap().state, State::Retained);
        fs::write(store.record_path(&id).unwrap(), "broken").unwrap();
        assert!(store.acquire(&req, None).is_err());
        assert!(
            store
                .list()
                .unwrap()
                .iter()
                .any(|o| o.allocation.is_none() && o.error.is_some())
        );
        assert!(Path::new(&record.binding.path).join("unfinished").exists());
    }

    #[test]
    fn stale_tokens_and_persistent_zero_process_leases_refuse_release() {
        let (_temp, home, binding) = fixture();
        let store = Store::new(&binding).unwrap();
        let mut req = request(&home, &binding, "persistent");
        req.persistent = true;
        let allocation = store.acquire(&req, None).unwrap();
        assert!(store.release(&allocation.binding).is_err());
        assert!(
            store
                .rebind(
                    &allocation.binding,
                    "replacement",
                    &allocation.binding.task_id,
                    "next-attempt"
                )
                .unwrap_err()
                .contains("ordinary allocation")
        );
        let mut token = allocation.binding.clone();
        token.lease_id.push('x');
        assert!(store.retain(&token, "stale").is_err());
        token = allocation.binding.clone();
        token.generation += 1;
        assert!(store.prune(&token, true, None).is_err());
        assert!(Path::new(&allocation.binding.path).exists());
    }

    #[test]
    fn untracked_ignored_dirty_and_unlanded_work_is_retained() {
        let (_temp, home, binding) = fixture();
        let store = Store::new(&binding).unwrap();
        for kind in ["dirty", "untracked", "ignored", "commit"] {
            let allocation = store
                .acquire(&request(&home, &binding, kind), None)
                .unwrap();
            let path = Path::new(&allocation.binding.path);
            match kind {
                "dirty" => fs::write(path.join("file"), "change").unwrap(),
                "untracked" => fs::write(path.join("unknown"), "keep").unwrap(),
                "ignored" => {
                    fs::create_dir(path.join("cache")).unwrap();
                    fs::write(path.join("cache/data"), "keep").unwrap();
                }
                _ => {
                    fs::write(path.join("file"), "change").unwrap();
                    git(path, &["commit", "-am", "work"]).unwrap();
                }
            }
            assert!(store.release(&allocation.binding).is_err(), "{kind}");
            assert!(path.exists());
        }
    }

    #[test]
    fn concurrent_requests_share_one_owner_and_duplicate_requests_converge() {
        let (_temp, home, binding) = fixture();
        let outcomes = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..4 {
                let home = &home;
                let binding = &binding;
                handles.push(scope.spawn(move || {
                    Store::new(binding)
                        .unwrap()
                        .acquire(&request(home, binding, "same"), None)
                        .unwrap()
                }));
            }
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(outcomes.iter().all(|v| v == &outcomes[0]));
        assert_eq!(Store::new(&binding).unwrap().inventory().unwrap().len(), 2);
    }
    #[test]
    fn scoped_prune_rechecks_and_recovers_both_removal_boundaries() {
        for fault in [Fault::BeforeRemoval, Fault::AfterRemoval] {
            let (_temp, home, binding) = fixture();
            let store = Store::new(&binding).unwrap();
            let allocation = store
                .acquire(&request(&home, &binding, "prune"), None)
                .unwrap();
            // Real lsof evidence, no fixture claiming that occupied work is idle.
            assert_eq!(
                store.release(&allocation.binding).unwrap().state,
                State::Disposed
            );
            assert_eq!(
                store.prune(&allocation.binding, false, None).unwrap().state,
                State::Disposed
            );
            assert!(Path::new(&allocation.binding.path).exists());
            assert!(store.prune(&allocation.binding, true, Some(fault)).is_err());
            assert_eq!(
                store.prune(&allocation.binding, true, None).unwrap().state,
                State::Removed
            );
            assert!(binding.canonical_path.join("file").exists());
            assert!(
                store
                    .acquire(&request(&home, &binding, "prune"), None)
                    .is_err()
            );
            let next = store
                .acquire(&request(&home, &binding, "next"), None)
                .unwrap();
            assert_ne!(next.binding.lease_id, allocation.binding.lease_id);
            assert_ne!(next.binding.path, allocation.binding.path);
            assert!(
                store
                    .release(&AllocationBinding {
                        allocation_id: next.binding.allocation_id.clone(),
                        ..allocation.binding.clone()
                    })
                    .is_err()
            );
        }
    }

    #[test]
    fn disposed_paths_with_new_content_and_live_occupants_remain_intact() {
        let (_temp, home, binding) = fixture();
        let store = Store::new(&binding).unwrap();
        let allocation = store
            .acquire(&request(&home, &binding, "occupied"), None)
            .unwrap();
        let mut process = Command::new("sleep")
            .arg("60")
            .current_dir(&allocation.binding.path)
            .spawn()
            .unwrap();
        let handoff = store.rebind(
            &allocation.binding,
            "replacement",
            &allocation.binding.task_id,
            "next-attempt",
        );
        let result = store.release(&allocation.binding);
        process.kill().unwrap();
        process.wait().unwrap();
        assert!(handoff.unwrap_err().contains("occupants"));
        assert!(result.is_err());
        assert_eq!(
            store
                .inspect(&allocation.binding.allocation_id)
                .unwrap()
                .state,
            State::Retained
        );
        store.release(&allocation.binding).unwrap();
        fs::write(
            Path::new(&allocation.binding.path).join("late-result"),
            "keep",
        )
        .unwrap();
        assert!(store.prune(&allocation.binding, true, None).is_err());
        assert!(
            Path::new(&allocation.binding.path)
                .join("late-result")
                .exists()
        );
    }

    #[test]
    fn linked_sources_share_repository_identity_and_existing_branches_are_unchanged() {
        let (_temp, home, binding) = fixture();
        git(&binding.canonical_path, &["branch", "mx/task"]).unwrap();
        let first = Store::new(&binding)
            .unwrap()
            .acquire(&request(&home, &binding, "one"), None)
            .unwrap();
        let linked =
            crate::project_registry::bind_project(&home, Path::new(&first.binding.path)).unwrap();
        let second = Store::new(&linked)
            .unwrap()
            .acquire(&request(&home, &linked, "two"), None)
            .unwrap();
        assert_eq!(
            first.binding.common_git_identity,
            second.binding.common_git_identity
        );
        assert_ne!(first.binding.checkout_id, second.binding.checkout_id);
        assert_eq!(
            git(&binding.canonical_path, &["rev-parse", "mx/task"]).unwrap(),
            binding.starting_revision
        );
        assert_eq!(
            git(
                Path::new(&second.binding.path),
                &["rev-parse", "--abbrev-ref", "HEAD"]
            )
            .unwrap(),
            "HEAD"
        );
    }

    #[test]
    fn legacy_reader_preserves_uncertainty_and_never_takes_foreign_pool_entries() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("treehouse-state.json");
        let original = br#"{"worktrees":[{"name":"one","path":"/owned/work","leased":true,"lease_holder":"worker","owner_pid":0},{"name":"foreign","path":"/foreign/work","leased":true}]}"#;
        fs::write(&path, original).unwrap();
        let entries = read_legacy(&path, &[PathBuf::from("/owned/work")]).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].lease_id.is_none());
        assert_eq!(entries[0].leased, Some(true));
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::write(&path, br#"{"version":4,"worktrees":[{"path":"/owned/work","lease_id":"known","seeded_paths":["cache"],"seed_inventory_known":true}]}"#).unwrap();
        let entries = read_legacy(&path, &[PathBuf::from("/owned/work")]).unwrap();
        assert_eq!(entries[0].lease_id.as_deref(), Some("known"));
        assert_eq!(entries[0].leased, None);
        assert_eq!(entries[0].raw["seeded_paths"][0], "cache");
        for malformed in [
            r#"{"worktrees":[{"path":"/owned/work","leased":"unknown"}]}"#,
            r#"{"worktrees":[{"path":"/owned/work","lease_id":42}]}"#,
            r#"{"worktrees":[{"path":"/owned/work","lease_holder":false}]}"#,
            r#"{"worktrees":[{"path":"/owned/work"},{"path":"/owned/work"}]}"#,
        ] {
            fs::write(&path, malformed).unwrap();
            assert!(read_legacy(&path, &[PathBuf::from("/owned/work")]).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), malformed);
        }
        fs::write(&path, br#"{"version":99,"worktrees":[]}"#).unwrap();
        assert!(read_legacy(&path, &[]).is_err());
    }

    #[test]
    fn public_cli_fences_home_and_token_and_repeats_disposition_safely() {
        let (_temp, home, project) = fixture();
        let call = |args: Vec<String>, owner: &Path| {
            run(&args.into_iter().map(Into::into).collect::<Vec<_>>(), owner)
        };
        assert!(run(&[], &home).unwrap().contains("legacy-inspect"));
        let project_path = project.canonical_path.to_string_lossy().into_owned();
        assert!(
            call(vec!["list".into(), project_path.clone()], &home)
                .unwrap()
                .contains("unmanaged Git worktree")
        );
        let allocation: Allocation = serde_json::from_str(
            &call(
                vec![
                    "acquire".into(),
                    project_path.clone(),
                    "--request".into(),
                    "cli".into(),
                    "--task".into(),
                    "task".into(),
                    "--attempt".into(),
                    "cli".into(),
                    "--base".into(),
                    project.starting_revision.clone(),
                ],
                &home,
            )
            .unwrap(),
        )
        .unwrap();
        let args = |operation: &str| {
            vec![
                operation.into(),
                project_path.clone(),
                allocation.binding.allocation_id.clone(),
                "--lease".into(),
                allocation.binding.lease_id.clone(),
                "--generation".into(),
                allocation.binding.generation.to_string(),
                "--attempt".into(),
                allocation.binding.attempt_id.clone(),
            ]
        };
        let other_home = home.with_file_name("other-home");
        fs::create_dir(&other_home).unwrap();
        assert!(
            call(args("release"), &other_home)
                .unwrap_err()
                .contains("another home")
        );
        let mut invalid = args("release");
        invalid[6] = "not-a-generation".into();
        assert!(
            call(invalid, &home)
                .unwrap_err()
                .contains("invalid generation")
        );
        let mut invalid = args("release");
        invalid[4] = "stale-lease".into();
        assert!(call(invalid, &home).is_err());
        let mut retained = args("retain");
        retained.extend(["--reason".into(), "".into()]);
        assert!(call(retained, &home).is_err());
        assert_eq!(
            serde_json::from_str::<Allocation>(
                &call(
                    vec![
                        "inspect".into(),
                        project_path.clone(),
                        allocation.binding.allocation_id.clone()
                    ],
                    &home
                )
                .unwrap()
            )
            .unwrap(),
            allocation
        );
        let disposed = call(args("release"), &home).unwrap();
        assert_eq!(call(args("release"), &home).unwrap(), disposed);
        let mut prune = args("prune");
        prune.push("--apply".into());
        let removed = call(prune.clone(), &home).unwrap();
        assert_eq!(call(prune, &home).unwrap(), removed);
        assert_eq!(call(args("release"), &home).unwrap(), removed);
        let store = Store::new(&project).unwrap();
        assert!(store.observe(&allocation.binding).unwrap().error.is_none());
        assert!(store.retain(&allocation.binding, "late").is_err());
        assert!(!Path::new(&allocation.binding.path).exists());
        assert!(project.canonical_path.join("file").exists());
        for tail in [
            vec!["inspect"],
            vec!["unknown"],
            vec!["list", "--bad"],
            vec!["acquire", "--request", "a", "--request", "b"],
            vec!["acquire", "--request"],
        ] {
            let mut argv = vec![tail[0].to_string(), project_path.clone()];
            argv.extend(tail[1..].iter().map(|s| s.to_string()));
            assert!(call(argv, &home).is_err());
        }
        assert!(run(&["acquire".into()], &home).is_err());
    }

    #[test]
    fn corrupt_handoff_and_duplicate_request_receipts_block_acquisition() {
        let (_temp, home, project) = fixture();
        let store = Store::new(&project).unwrap();
        let original = store
            .acquire(&request(&home, &project, "first"), None)
            .unwrap();
        let second = store
            .acquire(&request(&home, &project, "second"), None)
            .unwrap();
        let path = store.record_path(&original.binding.allocation_id).unwrap();
        let mut corrupt = original.clone();
        corrupt.binding.generation = 0;
        publish(&path, &corrupt).unwrap();
        assert!(
            store
                .inspect(&original.binding.allocation_id)
                .unwrap_err()
                .contains("corrupt or foreign")
        );
        corrupt = original.clone();
        corrupt.rebind_from = Some(original.binding.clone());
        publish(&path, &corrupt).unwrap();
        assert!(
            store
                .inspect(&original.binding.allocation_id)
                .unwrap_err()
                .contains("handoff receipt")
        );
        publish(&path, &second).unwrap();
        assert!(
            store
                .inspect(&original.binding.allocation_id)
                .unwrap_err()
                .contains("record identity mismatch")
        );
        publish(&path, &original).unwrap();
        let mut duplicate = second.clone();
        duplicate.request_id = original.request_id.clone();
        publish(
            &store.record_path(&second.binding.allocation_id).unwrap(),
            &duplicate,
        )
        .unwrap();
        assert!(
            store
                .acquire(&request(&home, &project, "first"), None)
                .unwrap_err()
                .contains("ambiguous")
        );
        assert!(Path::new(&original.binding.path).exists());
        assert!(Path::new(&second.binding.path).exists());
    }

    #[test]
    fn malformed_acquisition_and_rebind_preserve_existing_ownership() {
        let (_temp, home, project) = fixture();
        let store = Store::new(&project).unwrap();
        for field in 0..3 {
            let mut req = request(&home, &project, "valid");
            match field {
                0 => req.request_id = "",
                1 => req.task_id = "",
                _ => req.attempt_id = "",
            }
            assert!(store.acquire(&req, None).unwrap_err().contains("required"));
        }
        let mut foreign = project.clone();
        foreign.starting_revision = "0".repeat(40);
        assert!(
            store
                .acquire(&request(&home, &foreign, "foreign"), None)
                .unwrap_err()
                .contains("project mismatch")
        );
        let allocation = store
            .acquire(&request(&home, &project, "valid"), None)
            .unwrap();
        for (request_id, task, attempt) in [
            ("", "task", "next"),
            ("next", "different", "next"),
            ("valid", "task", "next"),
            ("next", "task", "valid"),
        ] {
            assert!(
                store
                    .rebind(&allocation.binding, request_id, task, attempt)
                    .is_err()
            );
        }
        assert_eq!(
            store.inspect(&allocation.binding.allocation_id).unwrap(),
            allocation
        );
        let unexplained = store
            .paths
            .join(key(&format!("{}:unknown", home.display())));
        fs::create_dir(&unexplained).unwrap();
        fs::write(unexplained.join("private"), "keep").unwrap();
        assert!(
            store
                .acquire(&request(&home, &project, "unknown"), None)
                .unwrap_err()
                .contains("unexplained")
        );
        assert_eq!(
            fs::read_to_string(unexplained.join("private")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn symlinked_allocation_roots_and_reservations_are_never_followed() {
        use std::os::unix::fs::symlink;
        let (_temp, home, project) = fixture();
        let store = Store::new(&project).unwrap();
        symlink(&home, &store.paths).unwrap();
        assert!(Store::new(&project).is_err());
        assert!(
            directory(&store.paths)
                .unwrap_err()
                .contains("unsafe allocation directory")
        );
        fs::remove_file(&store.paths).unwrap();
        directory(&store.root).unwrap();
        let sentinel = home.join("private");
        fs::write(&sentinel, "private").unwrap();
        symlink(&sentinel, store.root.join("operation.json")).unwrap();
        assert!(
            store
                .acquire(&request(&home, &project, "symlink"), None)
                .is_err()
        );
        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "private");
        assert!(
            store
                .list()
                .unwrap()
                .iter()
                .all(|row| row.allocation.is_none())
        );
    }

    #[test]
    fn legacy_cli_reads_only_explicit_paths_and_reports_missing_inventory() {
        let (_temp, home, _project) = fixture();
        let source = home.join("legacy.json");
        let recorded = home.join("recorded");
        fs::write(
            &source,
            serde_json::to_vec(
                &serde_json::json!({"worktrees": [{"path": recorded, "leased": null}]}),
            )
            .unwrap(),
        )
        .unwrap();
        let args = vec![
            "legacy-inspect".into(),
            source.as_os_str().to_owned(),
            recorded.as_os_str().to_owned(),
        ];
        let observations: Vec<LegacyAllocation> =
            serde_json::from_str(&run(&args, &home).unwrap()).unwrap();
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].leased, None);
        assert!(!recorded.exists());
        assert!(
            run(&args[..2], &home)
                .unwrap_err()
                .contains("explicitly recorded paths")
        );
        assert!(
            read_legacy(&source, &[home.join("absent")])
                .unwrap_err()
                .contains("absent from supplied inventory")
        );
    }

    #[test]
    fn oversized_external_io_is_bounded_before_input_or_after_output() {
        let oversized = vec![0; 64 * 1024 * 1024 + 1];
        assert!(
            command_output_with(
                &mut Command::new("missing-command-must-not-run"),
                Some(&oversized),
                Duration::from_secs(2)
            )
            .unwrap_err()
            .contains("input exceeds limit")
        );
        drop(oversized);
        // The final sleep keeps the process alive so the running-output guard
        // must terminate the owned process group instead of waiting for exit.
        let error = command_output_with(
            Command::new("sh").args([
                "-c",
                "dd if=/dev/zero bs=1048576 count=65 2>/dev/null; sleep 10",
            ]),
            None,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(error.contains("output exceeds limit"), "{error}");
    }
}

//! Primary-authoritative inherited configuration and shared-maintainer state.
//!
//! The on-disk report, quarantine, reread-generation, retry-marker, and
//! pointer-message formats remain unchanged from `bin/mx-config-inherit-lib.sh`.

use std::fs::{self, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use multplx_core::filesystem::atomic_replace;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

pub const DEFAULT_ALLOWLIST: [&str; 4] = [
    "actor-dispatch.json",
    "actor-harness",
    "backlog-backend",
    "herdr-presentation-spaces",
];
pub const SHARED_FILE: &str = "maintainer-shared.md";
pub const SHARED_REL: &str = "data/maintainer-shared.md";
pub const SHARED_MODE: u32 = 0o444;
pub const REREAD_PREFIX: &str = "state/.mx-inherited-config-reread";
pub const REREAD_RETRY_ROOT: &str = "state/.mx-inherited-config-reread-retry";
pub const INHERIT_LOCK_REL: &str = "state/.mx-inherited-config.lock";
pub const REREAD_FRAMING: &str = "These inherited config files changed. Re-read and apply their exact contents at every future intake. They are defaults/rules and do not remove your judgment to choose differently when warranted.";
pub const MAX_PENDING: usize = 16;
pub const MAX_SENT: usize = 16;
pub const MAX_QUARANTINE: usize = 16;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InheritanceError(String);

impl InheritanceError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Status {
    Pushed,
    Unchanged,
    Skipped,
    Error,
}

impl Status {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pushed => "pushed",
            Self::Unchanged => "unchanged",
            Self::Skipped => "skipped",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportRow {
    pub item: String,
    pub status: Status,
    pub reason: String,
}

impl ReportRow {
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}\t{}\t{}\n", self.item, self.status.as_str(), self.reason)
    }
}

#[derive(Default, Debug)]
pub struct Outcome {
    pub rows: Vec<ReportRow>,
    pub stdout: String,
    pub stderr: String,
    pub failed: bool,
}

#[derive(Clone, Debug)]
pub struct InheritancePlanner {
    source_home: PathBuf,
    source_config: PathBuf,
    source_data: PathBuf,
}

impl InheritancePlanner {
    #[must_use]
    pub fn new(
        source_home: impl Into<PathBuf>,
        source_config: impl Into<PathBuf>,
        source_data: impl Into<PathBuf>,
    ) -> Self {
        Self {
            source_home: source_home.into(),
            source_config: source_config.into(),
            source_data: source_data.into(),
        }
    }

    pub fn publish_to(&self, destination_home: &Path) -> Result<Outcome, InheritanceError> {
        propagate_daemon(
            &self.source_home,
            destination_home,
            Some(&self.source_config),
            Some(&self.source_data),
        )
    }
}

impl Outcome {
    fn row(&mut self, item: &str, status: Status, reason: impl Into<String>) {
        self.rows.push(ReportRow {
            item: item.to_owned(),
            status,
            reason: reason.into(),
        });
    }

    pub fn append_report(&self, path: Option<&Path>) {
        let Some(path) = path else { return };
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
        {
            for row in &self.rows {
                let _ = file.write_all(row.render().as_bytes());
            }
        }
    }

    pub fn merge(&mut self, mut other: Self) {
        self.rows.append(&mut other.rows);
        self.stdout.push_str(&other.stdout);
        self.stderr.push_str(&other.stderr);
        self.failed |= other.failed;
    }
}

fn allowlist_from(raw: &str) -> Result<Vec<String>, InheritanceError> {
    let items = raw
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for item in &items {
        let path = Path::new(item);
        if path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(InheritanceError::new("invalid inheritable config item"));
        }
    }
    Ok(items)
}

fn allowlist() -> Result<Vec<String>, InheritanceError> {
    let raw =
        std::env::var("MX_INHERITABLE_CONFIG").unwrap_or_else(|_| DEFAULT_ALLOWLIST.join(" "));
    allowlist_from(&raw)
}

fn bytes_equal(left: &Path, right: &Path) -> bool {
    fs::read(left)
        .ok()
        .zip(fs::read(right).ok())
        .is_some_and(|(a, b)| a == b)
}

fn copy_atomic(source: &Path, destination: &Path, mode: u32) -> Result<(), InheritanceError> {
    if let Ok(metadata) = fs::symlink_metadata(destination)
        && !metadata.is_file()
        && !metadata.file_type().is_symlink()
    {
        return Err(InheritanceError::new("destination is not replaceable"));
    }
    let bytes = fs::read(source)
        .map_err(|error| InheritanceError::new(format!("read source failed: {error}")))?;
    let parent = destination
        .parent()
        .ok_or_else(|| InheritanceError::new("destination has no parent"))?;
    fs::create_dir_all(parent)
        .map_err(|error| InheritanceError::new(format!("create parent failed: {error}")))?;
    if fs::symlink_metadata(destination).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        fs::remove_file(destination)
            .map_err(|error| InheritanceError::new(format!("remove symlink failed: {error}")))?;
    }
    atomic_replace(destination, &bytes, mode)
        .map_err(|error| InheritanceError::new(error.to_string()))
}

fn git_ignored(destination_config: &Path, item: &str) -> bool {
    let Some(parent) = destination_config.parent() else {
        return false;
    };
    let Ok(parent) = fs::canonicalize(parent) else {
        return false;
    };
    let inside = Command::new("git")
        .args(["-C"])
        .arg(&parent)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();
    if !inside.is_ok_and(|output| output.status.success()) {
        return true;
    }
    let Ok(top_output) = Command::new("git")
        .args(["-C"])
        .arg(&parent)
        .args(["rev-parse", "--show-toplevel"])
        .output()
    else {
        return false;
    };
    if !top_output.status.success() {
        return false;
    }
    let top = PathBuf::from(String::from_utf8_lossy(&top_output.stdout).trim());
    let destination = parent
        .join(
            destination_config
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("config")),
        )
        .join(item);
    let Ok(relative) = destination.strip_prefix(&top) else {
        return false;
    };
    Command::new("git")
        .args(["-C"])
        .arg(&top)
        .args(["check-ignore", "-q", "--"])
        .arg(relative)
        .status()
        .is_ok_and(|status| status.success())
}

fn warn_skip(outcome: &mut Outcome, item: &str, destination: &Path, reason: &str) {
    outcome.stderr.push_str(&format!(
        "mx-config-inherit: warning: skipped {item} for {}: {reason}\n",
        destination.display()
    ));
    outcome.row(item, Status::Skipped, reason);
}

fn fail_item(outcome: &mut Outcome, item: &str, destination: &Path, reason: &str) {
    outcome.stderr.push_str(&format!(
        "mx-config-inherit: error: {reason} {item} at {}\n",
        destination.display()
    ));
    outcome.row(item, Status::Error, reason);
    outcome.failed = true;
}

pub fn propagate_config(source: &Path, destination: &Path) -> Result<Outcome, InheritanceError> {
    let mut outcome = Outcome::default();
    let skip_reason = "destination does not allow inherited item (not gitignored or guard failed)";
    for item in allowlist()? {
        let source_item = source.join(&item);
        let destination_item = destination.join(&item);
        if source_item.is_file() {
            if !git_ignored(destination, &item) {
                warn_skip(&mut outcome, &item, destination, skip_reason);
                continue;
            }
            let same = !fs::symlink_metadata(&destination_item)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
                && destination_item.is_file()
                && bytes_equal(&source_item, &destination_item);
            if same {
                outcome.row(&item, Status::Unchanged, "");
            } else if copy_atomic(&source_item, &destination_item, 0o600).is_ok() {
                outcome.row(&item, Status::Pushed, "");
            } else {
                fail_item(&mut outcome, &item, &destination_item, "failed to copy");
            }
        } else if destination_item.exists()
            || fs::symlink_metadata(&destination_item)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            if !git_ignored(destination, &item) {
                warn_skip(&mut outcome, &item, destination, skip_reason);
                continue;
            }
            if fs::remove_file(&destination_item).is_ok() {
                outcome.row(&item, Status::Pushed, "mirrored primary absence");
            } else {
                fail_item(&mut outcome, &item, &destination_item, "failed to remove");
            }
        } else {
            outcome.row(&item, Status::Unchanged, "");
        }
    }
    Ok(outcome)
}

fn single_regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1
    })
}

pub fn sha256(path: &Path) -> Result<String, InheritanceError> {
    let bytes = fs::read(path).map_err(|error| InheritanceError::new(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn header_valid(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let head = text.lines().take(12).collect::<Vec<_>>().join("\n");
    head.contains("main-authoritative")
        && head.contains("read-only in daemon homes")
        && head.contains("must not be edited there")
        && head.contains("main broker")
        && (head.contains("marked status") || head.contains("document pointer"))
}

fn timestamp_compact() -> String {
    if let Ok(output) = Command::new("date")
        .args(["-u", "+%Y%m%dT%H%M%SZ"])
        .output()
        && output.status.success()
    {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if value.len() == 16 {
            return value;
        }
    }
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn quarantine_shared(destination: &Path) -> Result<PathBuf, InheritanceError> {
    if !single_regular(destination) {
        return Err(InheritanceError::new("unsafe destination"));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| InheritanceError::new("no parent"))?;
    let hash = sha256(destination)?;
    let glob_prefix = format!(".{SHARED_FILE}.quarantine.");
    if let Ok(entries) = fs::read_dir(parent) {
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name.starts_with(&glob_prefix)
                && (name.ends_with(&hash)
                    || name.rsplit_once('.').is_some_and(|(prefix, suffix)| {
                        prefix.ends_with(&hash) && suffix.bytes().all(|byte| byte.is_ascii_digit())
                    }))
                && single_regular(&path)
                && sha256(&path).ok().as_deref() == Some(hash.as_str())
            {
                fs::set_permissions(destination, Permissions::from_mode(0o600))
                    .map_err(|error| InheritanceError::new(error.to_string()))?;
                fs::remove_file(destination)
                    .map_err(|error| InheritanceError::new(error.to_string()))?;
                return Ok(path);
            }
        }
    }
    let base = parent.join(format!(
        ".{SHARED_FILE}.quarantine.{}.{}",
        timestamp_compact(),
        hash
    ));
    let mut candidate = base.clone();
    let mut index = 0;
    while candidate.exists() || fs::symlink_metadata(&candidate).is_ok() {
        index += 1;
        candidate = PathBuf::from(format!("{}.{}", base.display(), index));
    }
    fs::set_permissions(destination, Permissions::from_mode(0o600))
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    if let Err(error) = fs::rename(destination, &candidate) {
        let _ = fs::set_permissions(destination, Permissions::from_mode(SHARED_MODE));
        return Err(InheritanceError::new(error.to_string()));
    }
    fs::set_permissions(&candidate, Permissions::from_mode(0o600))
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    Ok(candidate)
}

pub fn propagate_shared(source_data: &Path, destination_data: &Path) -> Outcome {
    let mut outcome = Outcome::default();
    let source = source_data.join(SHARED_FILE);
    let destination = destination_data.join(SHARED_FILE);
    let destination_home = destination_data
        .parent()
        .unwrap_or(destination_data)
        .display();
    if source.exists() || fs::symlink_metadata(&source).is_ok() {
        if !single_regular(&source) {
            fail_item(&mut outcome, SHARED_REL, &source, "unsafe primary source");
            return outcome;
        }
        if !header_valid(&source) {
            fail_item(
                &mut outcome,
                SHARED_REL,
                &source,
                "primary source header missing required main-authoritative warning",
            );
            return outcome;
        }
        if destination.exists() || fs::symlink_metadata(&destination).is_ok() {
            if !single_regular(&destination) {
                fail_item(&mut outcome, SHARED_REL, &destination, "unsafe destination");
                return outcome;
            }
            if bytes_equal(&source, &destination) {
                if fs::set_permissions(&destination, Permissions::from_mode(SHARED_MODE)).is_ok() {
                    outcome.row(SHARED_REL, Status::Unchanged, "");
                } else {
                    fail_item(
                        &mut outcome,
                        SHARED_REL,
                        &destination,
                        "failed to restore read-only mode",
                    );
                }
                return outcome;
            }
            match quarantine_shared(&destination) {
                Ok(path) => {
                    outcome.stdout.push_str(&format!(
                        "DAEMON_SYNC: daemon home {destination_home}: quarantined {SHARED_REL} drift at {}\n",
                        path.display()
                    ));
                    if copy_atomic(&source, &destination, SHARED_MODE).is_ok() {
                        outcome.row(
                            SHARED_REL,
                            Status::Pushed,
                            format!("quarantined local drift at {}", path.display()),
                        );
                    } else {
                        fail_item(&mut outcome, SHARED_REL, &destination, "failed to copy");
                    }
                }
                Err(_) => {
                    let _ = fs::set_permissions(&destination, Permissions::from_mode(SHARED_MODE));
                    fail_item(
                        &mut outcome,
                        SHARED_REL,
                        &destination,
                        "failed to quarantine divergent destination",
                    );
                }
            }
        } else if fs::create_dir_all(destination_data).is_err() {
            fail_item(
                &mut outcome,
                SHARED_REL,
                destination_data,
                "unsafe destination directory",
            );
        } else if copy_atomic(&source, &destination, SHARED_MODE).is_ok() {
            outcome.row(SHARED_REL, Status::Pushed, "");
        } else {
            fail_item(&mut outcome, SHARED_REL, &destination, "failed to copy");
        }
    } else if destination.exists() || fs::symlink_metadata(&destination).is_ok() {
        if !single_regular(&destination) {
            fail_item(&mut outcome, SHARED_REL, &destination, "unsafe destination");
            return outcome;
        }
        match quarantine_shared(&destination) {
            Ok(path) => {
                outcome.stdout.push_str(&format!(
                    "DAEMON_SYNC: daemon home {destination_home}: quarantined {SHARED_REL} drift at {}\n",
                    path.display()
                ));
                outcome.row(
                    SHARED_REL,
                    Status::Pushed,
                    format!(
                        "mirrored primary absence after quarantining local copy at {}",
                        path.display()
                    ),
                );
            }
            Err(_) => fail_item(
                &mut outcome,
                SHARED_REL,
                &destination,
                "failed to quarantine destination before mirroring primary absence",
            ),
        }
    } else {
        outcome.row(SHARED_REL, Status::Unchanged, "");
    }
    outcome
}

pub fn propagate_daemon(
    source_home: &Path,
    destination_home: &Path,
    source_config: Option<&Path>,
    source_data: Option<&Path>,
) -> Result<Outcome, InheritanceError> {
    let default_config = source_home.join("config");
    let default_data = source_home.join("data");
    let mut outcome = propagate_config(
        source_config.unwrap_or(&default_config),
        &destination_home.join("config"),
    )?;
    outcome.merge(propagate_shared(
        source_data.unwrap_or(&default_data),
        &destination_home.join("data"),
    ));
    Ok(outcome)
}

pub fn parse_report(path: &Path) -> Vec<ReportRow> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let item = fields.next()?.to_owned();
            let status = match fields.next()? {
                "pushed" => Status::Pushed,
                "unchanged" => Status::Unchanged,
                "skipped" => Status::Skipped,
                "error" => Status::Error,
                _ => return None,
            };
            Some(ReportRow {
                item,
                status,
                reason: fields.next().unwrap_or("").to_owned(),
            })
        })
        .collect()
}

pub fn changed_items(path: &Path) -> Result<Vec<String>, InheritanceError> {
    let rows = parse_report(path);
    let allowlist = allowlist()?;
    Ok(allowlist
        .into_iter()
        .filter(|item| {
            rows.iter()
                .find(|row| row.item == *item)
                .is_some_and(|row| row.status == Status::Pushed)
        })
        .collect())
}

pub fn write_reread_instruction(
    destination_home: &Path,
    report: &Path,
    instruction: &Path,
) -> Result<bool, InheritanceError> {
    let fault = match std::env::var("MX_CONFIG_INHERIT_TEST_FAIL_WRITE").as_deref() {
        Ok("retain-stage") => WriteFault::RetainStage,
        Ok("retain-temporary") => WriteFault::RetainTemporary,
        _ => WriteFault::None,
    };
    write_reread_instruction_with_fault(destination_home, report, instruction, fault)
}

#[derive(Clone, Copy)]
enum WriteFault {
    None,
    RetainStage,
    RetainTemporary,
}

fn write_reread_instruction_with_fault(
    destination_home: &Path,
    report: &Path,
    instruction: &Path,
    fault: WriteFault,
) -> Result<bool, InheritanceError> {
    let changed = changed_items(report)?;
    if changed.is_empty() {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(REREAD_FRAMING.as_bytes());
    bytes.push(b'\n');
    for item in changed {
        let relative = format!("config/{item}");
        bytes.extend_from_slice(format!("\n{relative}\n-----BEGIN {relative}-----\n").as_bytes());
        let destination = destination_home.join(&relative);
        if single_regular(&destination) {
            bytes.extend_from_slice(
                &fs::read(&destination)
                    .map_err(|error| InheritanceError::new(error.to_string()))?,
            );
        } else {
            bytes.extend_from_slice(b"ABSENT\n");
        }
        bytes.extend_from_slice(format!("-----END {relative}-----\n").as_bytes());
    }
    let parent = instruction
        .parent()
        .ok_or_else(|| InheritanceError::new("instruction has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| InheritanceError::new(error.to_string()))?;
    match fault {
        WriteFault::RetainStage => {
            atomic_replace(instruction, &bytes, 0o600)
                .map_err(|error| InheritanceError::new(error.to_string()))?;
            return Err(InheritanceError::new("injected instruction write failure"));
        }
        WriteFault::RetainTemporary => {
            let retained = PathBuf::from(format!("{}.tmp.injected", instruction.display()));
            atomic_replace(&retained, &bytes, 0o600)
                .map_err(|error| InheritanceError::new(error.to_string()))?;
            let _ = fs::remove_file(instruction);
            return Err(InheritanceError::new("injected temporary adoption failure"));
        }
        WriteFault::None => {}
    }
    atomic_replace(instruction, &bytes, 0o600)
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    Ok(true)
}

fn safe_token(id: &str) -> String {
    let token = id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "_.-".contains(character) {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if token.is_empty() {
        "unknown".to_owned()
    } else {
        token
    }
}

#[must_use]
pub fn retry_dir(source_home: &Path, id: &str) -> PathBuf {
    source_home.join(REREAD_RETRY_ROOT).join(safe_token(id))
}

fn sorted_regular_prefix(directory: &Path, prefix: &str, suffix: Option<&str>) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            name.starts_with(prefix)
                && suffix.is_none_or(|suffix| name.ends_with(suffix))
                && single_regular(path)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

pub fn pending_stages(source_home: &Path, id: &str) -> Vec<PathBuf> {
    sorted_regular_prefix(
        &retry_dir(source_home, id),
        ".mx-inherited-config-reread.",
        None,
    )
    .into_iter()
    .filter(|path| !path.to_string_lossy().ends_with(".report"))
    .filter(|path| fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0))
    .collect()
}

pub fn pending_reports(source_home: &Path, id: &str) -> Vec<PathBuf> {
    sorted_regular_prefix(
        &retry_dir(source_home, id),
        ".mx-inherited-config-reread.",
        Some(".report"),
    )
}

pub fn retry_queue_full(source_home: &Path, id: &str) -> bool {
    pending_stages(source_home, id).len() + pending_reports(source_home, id).len() >= MAX_PENDING
}

pub fn next_stage(source_home: &Path, id: &str) -> Result<PathBuf, InheritanceError> {
    let directory = retry_dir(source_home, id);
    fs::create_dir_all(&directory).map_err(|error| InheritanceError::new(error.to_string()))?;
    fs::set_permissions(&directory, Permissions::from_mode(0o700))
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    let sequence_path = directory.join(".sequence");
    let sequence = fs::read_to_string(&sequence_path)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0)
        + 1;
    atomic_replace(&sequence_path, format!("{sequence}\n").as_bytes(), 0o600)
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    let generation = format!(
        "{}.{sequence:08}",
        timestamp_compact().trim_end_matches('Z')
    );
    let file = tempfile::Builder::new()
        .prefix(&format!(".mx-inherited-config-reread.{generation}."))
        .tempfile_in(&directory)
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    let (_handle, path) = file
        .keep()
        .map_err(|error| InheritanceError::new(error.error.to_string()))?;
    fs::set_permissions(&path, Permissions::from_mode(0o600))
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    Ok(path)
}

pub fn mark_pending(instruction: &Path) -> Result<PathBuf, InheritanceError> {
    let pending = PathBuf::from(format!("{}.pending", instruction.display()));
    mark_pending_at(instruction, &pending)?;
    Ok(pending)
}

pub fn mark_pending_at(instruction: &Path, pending: &Path) -> Result<(), InheritanceError> {
    atomic_replace(
        pending,
        format!("{}\n", instruction.display()).as_bytes(),
        0o600,
    )
    .map_err(|error| InheritanceError::new(error.to_string()))
}

pub fn save_retry_report(report: &Path, stage: &Path) -> Result<PathBuf, InheritanceError> {
    let destination = PathBuf::from(format!("{}.report", stage.display()));
    let bytes = fs::read(report).map_err(|error| InheritanceError::new(error.to_string()))?;
    atomic_replace(&destination, &bytes, 0o600)
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    Ok(destination)
}

pub fn pending_instructions(destination_home: &Path) -> Vec<PathBuf> {
    let state = destination_home.join("state");
    sorted_regular_prefix(&state, ".mx-inherited-config-reread.", Some(".pending"))
        .into_iter()
        .map(|pending| PathBuf::from(pending.to_string_lossy().trim_end_matches(".pending")))
        .collect()
}

pub fn has_pending(destination_home: &Path) -> bool {
    !pending_instructions(destination_home).is_empty()
}

pub fn publish_stage(destination_home: &Path, stage: &Path) -> Result<PathBuf, InheritanceError> {
    publish_stage_with_fault(
        destination_home,
        stage,
        std::env::var("MX_CONFIG_INHERIT_TEST_FAIL_PUBLISH").as_deref() == Ok("1"),
    )
}

fn publish_stage_with_fault(
    destination_home: &Path,
    stage: &Path,
    fail_publish: bool,
) -> Result<PathBuf, InheritanceError> {
    if fail_publish {
        return Err(InheritanceError::new("injected publication failure"));
    }
    if !single_regular(stage) {
        return Err(InheritanceError::new(
            "retry stage is not a private regular file",
        ));
    }
    let state = destination_home.join("state");
    fs::create_dir_all(&state).map_err(|error| InheritanceError::new(error.to_string()))?;
    let final_path = state.join(
        stage
            .file_name()
            .ok_or_else(|| InheritanceError::new("stage has no name"))?,
    );
    let bytes = fs::read(stage).map_err(|error| InheritanceError::new(error.to_string()))?;
    atomic_replace(&final_path, &bytes, 0o600)
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    mark_pending(&final_path)?;
    Ok(final_path)
}

fn send_pointer(
    id: &str,
    instruction: &Path,
    root: &Path,
    home: &Path,
    state: &Path,
) -> Result<(), String> {
    let pending = PathBuf::from(format!("{}.pending", instruction.display()));
    if !single_regular(instruction) {
        return Err("pending instruction file is missing".to_owned());
    }
    if fs::read_to_string(&pending).ok().as_deref().map(str::trim)
        != Some(instruction.to_string_lossy().as_ref())
    {
        return Err("pending instruction file is mismatched".to_owned());
    }
    let send = root.join("bin/mx-send.sh");
    if !send.is_file() {
        return Err(format!("mx-send.sh not executable at {}", send.display()));
    }
    let output = Command::new(&send)
        .arg(format!("mx-{id}"))
        .arg(format!("CONFIG_REREAD: {}", instruction.display()))
        .env("MX_HOME", home)
        .env("MX_ROOT_OVERRIDE", root)
        .env("MX_STATE_OVERRIDE", state)
        .env(
            "MX_SEND_SETTLE",
            std::env::var("MX_SEND_SETTLE").unwrap_or_else(|_| "0".to_owned()),
        )
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        let _ = fs::remove_file(&pending);
        Ok(())
    } else {
        let combined = if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        };
        let first = String::from_utf8_lossy(&combined)
            .lines()
            .next()
            .unwrap_or("")
            .to_owned();
        Err(if first.is_empty() {
            format!("mx-send exited {}", output.status.code().unwrap_or(1))
        } else {
            first
        })
    }
}

pub fn cleanup_sent(destination_home: &Path) {
    let state = destination_home.join("state");
    let mut sent = sorted_regular_prefix(&state, ".mx-inherited-config-reread.", None)
        .into_iter()
        .filter(|path| !path.to_string_lossy().ends_with(".pending"))
        .filter(|path| !PathBuf::from(format!("{}.pending", path.display())).exists())
        .collect::<Vec<_>>();
    sent.sort();
    let remove = sent.len().saturating_sub(MAX_SENT);
    for path in sent.into_iter().take(remove) {
        let _ = fs::remove_file(path);
    }
}

pub struct RereadContext<'a> {
    pub id: &'a str,
    pub destination_home: &'a Path,
    pub report: &'a Path,
    pub source_home: &'a Path,
    pub root: &'a Path,
    pub state: &'a Path,
}

pub fn send_reread(context: &RereadContext<'_>) -> (bool, String) {
    let mut output = String::new();
    let mut failed = false;
    let destination_home = match fs::canonicalize(context.destination_home) {
        Ok(path) => path,
        Err(_) => {
            output.push_str(&format!(
                "CONFIG_REREAD: daemon {}: send failed: destination home is not readable\n",
                context.id
            ));
            return (false, output);
        }
    };
    let changed = changed_items(context.report).unwrap_or_default();
    let mut stages = pending_stages(context.source_home, context.id);
    for report in pending_reports(context.source_home, context.id) {
        let stage = PathBuf::from(report.to_string_lossy().trim_end_matches(".report"));
        match write_reread_instruction(&destination_home, &report, &stage) {
            Ok(true) => {
                let _ = fs::remove_file(&report);
                stages.push(stage);
            }
            _ => {
                output.push_str(&format!(
                    "CONFIG_REREAD: daemon {}: send failed: could not rebuild retry instruction\n",
                    context.id
                ));
                return (false, output);
            }
        }
    }
    if !changed.is_empty() {
        if retry_queue_full(context.source_home, context.id) {
            output.push_str(&format!(
                "CONFIG_REREAD: daemon {}: send failed: retry instruction queue is full\n",
                context.id
            ));
            return (false, output);
        }
        match next_stage(context.source_home, context.id).and_then(|stage| {
            write_reread_instruction(&destination_home, context.report, &stage)?;
            Ok(stage)
        }) {
            Ok(stage) => stages.push(stage),
            Err(_) => {
                let detail = match std::env::var("MX_CONFIG_INHERIT_TEST_FAIL_WRITE").as_deref() {
                    Ok("retain-stage") => "retained exact retry generation",
                    Ok("retain-temporary") => "retained exact retry temporary",
                    _ => "could not write retry instruction",
                };
                output.push_str(&format!(
                    "CONFIG_REREAD: daemon {}: send failed: {detail}\n",
                    context.id
                ));
                return (false, output);
            }
        }
    }
    let mut delivery = pending_instructions(&destination_home);
    for stage in &stages {
        match publish_stage(&destination_home, stage) {
            Ok(path) => {
                if !delivery.contains(&path) {
                    delivery.push(path);
                }
            }
            Err(_) => {
                output.push_str(&format!(
                    "CONFIG_REREAD: daemon {}: send failed: could not publish retry instruction\n",
                    context.id
                ));
                failed = true;
                break;
            }
        }
    }
    delivery.sort();
    for instruction in delivery {
        match send_pointer(
            context.id,
            &instruction,
            context.root,
            context.source_home,
            context.state,
        ) {
            Ok(()) => {
                if let Some(stage) = stages
                    .iter()
                    .find(|stage| stage.file_name() == instruction.file_name())
                {
                    let _ = fs::remove_file(stage);
                }
            }
            Err(detail) => {
                let _ = mark_pending(&instruction);
                output.push_str(&format!(
                    "CONFIG_REREAD: daemon {}: send failed: {detail}\n",
                    context.id
                ));
                failed = true;
                break;
            }
        }
    }
    cleanup_sent(&destination_home);
    (!failed, output)
}

pub fn discard_pending(
    destination_home: &Path,
    id: Option<&str>,
    source_home: Option<&Path>,
) -> bool {
    discard_pending_with_fault(
        destination_home,
        id,
        source_home,
        std::env::var("MX_CONFIG_INHERIT_TEST_FAIL_DISCARD").as_deref() == Ok("1"),
    )
}

fn discard_pending_with_fault(
    destination_home: &Path,
    id: Option<&str>,
    source_home: Option<&Path>,
    fail_discard: bool,
) -> bool {
    if fail_discard {
        return false;
    }
    let mut ok = true;
    for instruction in pending_instructions(destination_home) {
        ok &= fs::remove_file(PathBuf::from(format!("{}.pending", instruction.display()))).is_ok();
        ok &= fs::remove_file(instruction).is_ok();
    }
    if let (Some(id), Some(source_home)) = (id, source_home) {
        let directory = retry_dir(source_home, id);
        if directory.is_dir() {
            for path in pending_stages(source_home, id)
                .into_iter()
                .chain(pending_reports(source_home, id))
            {
                ok &= fs::remove_file(path).is_ok();
            }
            let _ = fs::remove_file(directory.join(".sequence"));
            let _ = fs::remove_dir(directory);
        }
    }
    ok
}

fn quarantine_directory(home: &Path) -> Result<PathBuf, InheritanceError> {
    let root = home.join("state/.mx-inherited-config-reread-quarantine");
    fs::create_dir_all(&root).map_err(|error| InheritanceError::new(error.to_string()))?;
    fs::set_permissions(&root, Permissions::from_mode(0o700))
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    let mut generations = fs::read_dir(&root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && !fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("generation."))
        })
        .collect::<Vec<_>>();
    generations.sort();
    let remove = generations
        .len()
        .saturating_sub(MAX_QUARANTINE.saturating_sub(1));
    for generation in generations.into_iter().take(remove) {
        fs::remove_dir_all(generation).map_err(|error| InheritanceError::new(error.to_string()))?;
    }
    let directory = tempfile::Builder::new()
        .prefix("generation.")
        .tempdir_in(&root)
        .map_err(|error| InheritanceError::new(error.to_string()))?
        .keep();
    fs::set_permissions(&directory, Permissions::from_mode(0o700))
        .map_err(|error| InheritanceError::new(error.to_string()))?;
    Ok(directory)
}

pub fn quarantine_pending(
    destination_home: &Path,
    id: Option<&str>,
    source_home: Option<&Path>,
) -> bool {
    let destination = pending_instructions(destination_home);
    let source = id.zip(source_home).map_or_else(Vec::new, |(id, home)| {
        pending_stages(home, id)
            .into_iter()
            .chain(pending_reports(home, id))
            .collect()
    });
    let mut ok = true;
    if !destination.is_empty() {
        if let Ok(quarantine) = quarantine_directory(destination_home) {
            for instruction in destination {
                for path in [
                    instruction.clone(),
                    PathBuf::from(format!("{}.pending", instruction.display())),
                ] {
                    if path.exists() {
                        ok &= fs::rename(&path, quarantine.join(path.file_name().expect("name")))
                            .is_ok();
                    }
                }
            }
        } else {
            ok = false;
        }
    }
    if !source.is_empty()
        && let Some(home) = source_home
    {
        if let Ok(quarantine) = quarantine_directory(home) {
            for path in source {
                ok &= fs::rename(&path, quarantine.join(path.file_name().expect("name"))).is_ok();
            }
        } else {
            ok = false;
        }
    }
    ok
}

#[derive(Clone, Debug)]
pub struct ValidatedHome {
    pub path: PathBuf,
}

fn ancestor_of(ancestor: &Path, path: &Path) -> bool {
    ancestor != path && path.starts_with(ancestor)
}

pub fn validate_daemon_home(
    id: &str,
    home: &Path,
    active_home: &Path,
    root: &Path,
) -> Result<ValidatedHome, String> {
    let home = fs::canonicalize(home).map_err(|_| "not a directory".to_owned())?;
    if !home.is_dir() {
        return Err("not a directory".to_owned());
    }
    let active = fs::canonicalize(active_home)
        .map_err(|_| "active Multplx home is not a directory".to_owned())?;
    let root = fs::canonicalize(root).map_err(|_| "Multplx repo is not a directory".to_owned())?;
    if home == Path::new("/") {
        return Err("daemon home cannot be the filesystem root".to_owned());
    }
    for (left, right, message) in [
        (
            &home,
            &active,
            "daemon home cannot be the active Multplx home",
        ),
        (&home, &root, "daemon home cannot be the Multplx repo"),
    ] {
        if left == right {
            return Err(message.to_owned());
        }
    }
    if ancestor_of(&active, &home) {
        return Err("daemon home cannot be inside the active Multplx home".to_owned());
    }
    if ancestor_of(&root, &home) {
        return Err("daemon home cannot be inside the Multplx repo".to_owned());
    }
    if ancestor_of(&home, &active) {
        return Err("daemon home cannot be an ancestor of the active Multplx home".to_owned());
    }
    if ancestor_of(&home, &root) {
        return Err("daemon home cannot be an ancestor of the Multplx repo".to_owned());
    }
    for name in ["data", "state", "config", "projects"] {
        let path = home.join(name);
        let resolved = if path.exists() {
            if !path.is_dir() {
                return Err(format!("daemon {name} path is not a directory"));
            }
            fs::canonicalize(&path)
                .map_err(|_| format!("daemon {name} directory cannot be resolved"))?
        } else if fs::symlink_metadata(&path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(format!(
                "daemon {name} directory must resolve inside the daemon home"
            ));
        } else {
            path
        };
        if !ancestor_of(&home, &resolved) {
            return Err(format!(
                "daemon {name} directory must resolve inside the daemon home"
            ));
        }
        if resolved == active || ancestor_of(&active, &resolved) {
            return Err(format!(
                "daemon {name} directory cannot be inside the active Multplx home"
            ));
        }
        if resolved == root || ancestor_of(&root, &resolved) {
            return Err(format!(
                "daemon {name} directory cannot be inside the Multplx repo"
            ));
        }
    }
    let marker = home.join(".mx-daemon-home");
    if fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("daemon marker must not be a symlink".to_owned());
    }
    if !marker.is_file() {
        return Err("not a seeded daemon home".to_owned());
    }
    let marker_id = fs::read_to_string(&marker).unwrap_or_default();
    let marker_id = marker_id.trim_end_matches('\n');
    if marker_id != id {
        return Err(format!(
            "marked for daemon {}, expected {id}",
            if marker_id.is_empty() {
                "unknown"
            } else {
                marker_id
            }
        ));
    }
    if !home.join("AGENTS.md").is_file() {
        return Err("not a Multplx home (missing AGENTS.md)".to_owned());
    }
    if !home.join("bin").is_dir() {
        return Err("not a Multplx home (missing bin/)".to_owned());
    }
    Ok(ValidatedHome { path: home })
}

pub fn inherit_lock(home: &Path) -> PathBuf {
    home.join(INHERIT_LOCK_REL)
}

pub fn acquire_inherit_lock(home: &Path) -> Result<DirectoryLock, InheritanceError> {
    let state = home.join("state");
    fs::create_dir_all(&state).map_err(|error| InheritanceError::new(error.to_string()))?;
    DirectoryLock::acquire_wait(
        inherit_lock(home),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error| InheritanceError::new(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn write_executable(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("parent");
        fs::write(path, body).expect("script");
        fs::set_permissions(path, Permissions::from_mode(0o755)).expect("executable");
    }

    fn seeded_home(path: &Path, id: &str) {
        for name in ["data", "state", "config", "projects", "bin"] {
            fs::create_dir_all(path.join(name)).expect("home surface");
        }
        fs::write(path.join(".mx-daemon-home"), format!("{id}\n")).expect("marker");
        fs::write(path.join("AGENTS.md"), "# daemon\n").expect("agents");
    }

    fn shared_header() -> &'static str {
        "# Shared maintainer preferences\n\nThis file is main-authoritative in the main Multplx home.\nIn daemon homes it is read-only in daemon homes and must not be edited there.\nRoute discoveries to the main broker through marked status or a document pointer.\n"
    }

    #[test]
    fn config_copy_absence_and_report_are_idempotent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(&destination).expect("destination");
        fs::write(source.join("actor-harness"), b"codex\n").expect("source config");
        let first = propagate_config(&source, &destination).expect("propagate");
        assert_eq!(first.rows[1].status, Status::Pushed);
        let second = propagate_config(&source, &destination).expect("propagate");
        assert_eq!(second.rows[1].status, Status::Unchanged);
        fs::remove_file(source.join("actor-harness")).expect("remove source");
        let third = propagate_config(&source, &destination).expect("propagate");
        assert_eq!(third.rows[1].reason, "mirrored primary absence");
        assert!(!destination.join("actor-harness").exists());
    }

    #[test]
    fn shared_copy_is_read_only_and_drift_is_quarantined() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source-data");
        let destination = temp.path().join("daemon/data");
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(&destination).expect("destination");
        fs::write(
            source.join(SHARED_FILE),
            format!("{}primary\n", shared_header()),
        )
        .expect("source");
        let first = propagate_shared(&source, &destination);
        assert!(!first.failed);
        assert_eq!(
            fs::metadata(destination.join(SHARED_FILE))
                .expect("meta")
                .permissions()
                .mode()
                & 0o777,
            SHARED_MODE
        );
        fs::set_permissions(destination.join(SHARED_FILE), Permissions::from_mode(0o600))
            .expect("mode");
        fs::write(
            destination.join(SHARED_FILE),
            format!("{}drift\n", shared_header()),
        )
        .expect("drift");
        let second = propagate_shared(&source, &destination);
        assert!(second.stdout.contains("quarantined"));
        assert_eq!(
            fs::read(source.join(SHARED_FILE)).expect("source"),
            fs::read(destination.join(SHARED_FILE)).expect("destination")
        );
    }

    #[test]
    fn reread_instruction_contains_only_changed_allowlisted_destination_bytes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("daemon");
        fs::create_dir_all(home.join("config")).expect("config");
        fs::write(home.join("config/actor-harness"), b"codex\n").expect("harness");
        let report = temp.path().join("report");
        fs::write(
            &report,
            b"actor-dispatch.json\tunchanged\t\nactor-harness\tpushed\t\ndata/maintainer-shared.md\tpushed\t\n",
        )
        .expect("report");
        let instruction = home.join("state/instruction");
        assert!(write_reread_instruction(&home, &report, &instruction).expect("instruction"));
        let text = fs::read_to_string(instruction).expect("text");
        assert!(text.contains("-----BEGIN config/actor-harness-----\ncodex\n-----END"));
        assert!(!text.contains("maintainer-shared"));
        assert!(!text.contains("actor-dispatch"));
    }

    #[test]
    fn typed_outcomes_reports_and_planner_cover_public_contracts() {
        assert_eq!(Status::Pushed.as_str(), "pushed");
        assert_eq!(Status::Unchanged.as_str(), "unchanged");
        assert_eq!(Status::Skipped.as_str(), "skipped");
        assert_eq!(Status::Error.as_str(), "error");
        let row = ReportRow {
            item: "actor-harness".to_owned(),
            status: Status::Pushed,
            reason: "changed".to_owned(),
        };
        assert_eq!(row.render(), "actor-harness\tpushed\tchanged\n");

        let temp = tempfile::tempdir().expect("tempdir");
        let report = temp.path().join("report");
        let mut outcome = Outcome {
            rows: vec![row.clone()],
            stdout: "one\n".to_owned(),
            stderr: String::new(),
            failed: false,
        };
        outcome.merge(Outcome {
            rows: vec![ReportRow {
                item: "actor-dispatch.json".to_owned(),
                status: Status::Error,
                reason: "bad".to_owned(),
            }],
            stdout: "two\n".to_owned(),
            stderr: "warning\n".to_owned(),
            failed: true,
        });
        outcome.append_report(Some(&report));
        outcome.append_report(None);
        assert!(outcome.failed);
        assert_eq!(outcome.stdout, "one\ntwo\n");
        assert_eq!(outcome.stderr, "warning\n");
        assert_eq!(parse_report(&report).len(), 2);
        fs::write(
            &report,
            "actor-harness\tpushed\tchanged\ninvalid\tunknown\tignored\nshort\n",
        )
        .expect("report matrix");
        assert_eq!(parse_report(&report), vec![row]);
        assert!(parse_report(&temp.path().join("missing")).is_empty());

        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(source.join("config")).expect("source config");
        fs::create_dir_all(source.join("data")).expect("source data");
        fs::create_dir_all(&destination).expect("destination");
        let planner = InheritancePlanner::new(&source, source.join("config"), source.join("data"));
        let planned = planner
            .publish_to(&destination)
            .expect("planned propagation");
        assert_eq!(planned.rows.len(), DEFAULT_ALLOWLIST.len() + 1);
    }

    #[test]
    fn allowlist_and_git_guard_reject_unsafe_or_unignored_items() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(allowlist_from("../escape").is_err());
        assert!(allowlist_from("/absolute").is_err());
        assert_eq!(
            allowlist_from("nested/item").expect("nested"),
            ["nested/item"]
        );
        let source = temp.path().join("source");
        let repository = temp.path().join("repository");
        let destination = repository.join("config");
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(&destination).expect("destination");
        fs::write(source.join("actor-harness"), "codex\n").expect("source item");
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .arg(&repository)
                .status()
                .expect("git")
                .success()
        );
        let skipped = propagate_config(&source, &destination).expect("propagate");
        assert_eq!(skipped.rows[1].status, Status::Skipped);
        assert!(skipped.stderr.contains("warning: skipped"));

        fs::write(repository.join(".gitignore"), "config/actor-harness\n").expect("ignore");
        let pushed = propagate_config(&source, &destination).expect("propagate ignored");
        assert_eq!(pushed.rows[1].status, Status::Pushed);
    }

    #[test]
    fn config_and_shared_failures_are_reported_without_partial_success() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(destination.join("actor-harness")).expect("blocking directory");
        fs::write(source.join("actor-harness"), "codex\n").expect("source item");
        let failed = propagate_config(&source, &destination).expect("outcome");
        assert!(failed.failed);
        assert_eq!(failed.rows[1].status, Status::Error);

        let source_data = temp.path().join("source-data");
        let destination_data = temp.path().join("destination-data");
        fs::create_dir_all(&source_data).expect("source data");
        fs::create_dir_all(&destination_data).expect("destination data");
        fs::write(source_data.join(SHARED_FILE), "missing contract\n").expect("invalid shared");
        let invalid_header = propagate_shared(&source_data, &destination_data);
        assert!(invalid_header.failed);
        assert!(invalid_header.stderr.contains("header missing"));
        fs::remove_file(source_data.join(SHARED_FILE)).expect("remove source");
        fs::create_dir(destination_data.join(SHARED_FILE)).expect("unsafe destination");
        let unsafe_destination = propagate_shared(&source_data, &destination_data);
        assert!(unsafe_destination.failed);
        assert!(unsafe_destination.stderr.contains("unsafe destination"));
    }

    #[test]
    fn reread_generation_publication_cleanup_and_discard_are_complete() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_home = temp.path().join("source");
        let daemon = temp.path().join("daemon");
        fs::create_dir_all(daemon.join("config")).expect("config");
        fs::write(daemon.join("config/actor-harness"), "codex\n").expect("config item");
        let report = temp.path().join("report");
        fs::write(&report, "actor-harness\tpushed\t\n").expect("report");
        assert_eq!(
            changed_items(&report).expect("changed"),
            vec!["actor-harness"]
        );

        let stage = next_stage(&source_home, "bad/id").expect("stage");
        assert!(stage.starts_with(retry_dir(&source_home, "bad/id")));
        assert!(write_reread_instruction(&daemon, &report, &stage).expect("instruction"));
        let saved = save_retry_report(&report, &stage).expect("saved report");
        assert!(pending_stages(&source_home, "bad/id").contains(&stage));
        assert!(pending_reports(&source_home, "bad/id").contains(&saved));
        let published = publish_stage(&daemon, &stage).expect("publish");
        assert!(has_pending(&daemon));
        assert_eq!(pending_instructions(&daemon), vec![published.clone()]);

        fs::remove_file(PathBuf::from(format!("{}.pending", published.display())))
            .expect("remove pending marker");
        for index in 0..MAX_SENT + 4 {
            fs::write(
                daemon
                    .join("state")
                    .join(format!(".mx-inherited-config-reread.sent-{index:03}")),
                "sent\n",
            )
            .expect("sent generation");
        }
        cleanup_sent(&daemon);
        let sent =
            sorted_regular_prefix(&daemon.join("state"), ".mx-inherited-config-reread.", None);
        assert!(sent.len() <= MAX_SENT);

        let discard_stage = next_stage(&source_home, "bad/id").expect("discard stage");
        write_reread_instruction(&daemon, &report, &discard_stage).expect("discard instruction");
        publish_stage(&daemon, &discard_stage).expect("discard publication");
        assert!(discard_pending(&daemon, Some("bad/id"), Some(&source_home)));
        assert!(!has_pending(&daemon));
        assert!(pending_stages(&source_home, "bad/id").is_empty());
        assert!(pending_reports(&source_home, "bad/id").is_empty());
    }

    #[test]
    fn reread_fault_injection_retains_recoverable_material() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_home = temp.path().join("source");
        let daemon = temp.path().join("daemon");
        fs::create_dir_all(daemon.join("config")).expect("config");
        fs::write(daemon.join("config/actor-harness"), "codex\n").expect("config item");
        let report = temp.path().join("report");
        fs::write(&report, "actor-harness\tpushed\t\n").expect("report");
        let stage = next_stage(&source_home, "worker").expect("stage");
        write_reread_instruction(&daemon, &report, &stage).expect("instruction");

        assert!(publish_stage_with_fault(&daemon, &stage, true).is_err());
        assert!(stage.is_file());

        let retained = temp.path().join("retained");
        assert!(
            write_reread_instruction_with_fault(
                &daemon,
                &report,
                &retained,
                WriteFault::RetainStage
            )
            .is_err()
        );
        assert!(retained.is_file());
        let temporary = temp.path().join("temporary");
        assert!(
            write_reread_instruction_with_fault(
                &daemon,
                &report,
                &temporary,
                WriteFault::RetainTemporary
            )
            .is_err()
        );
        assert!(PathBuf::from(format!("{}.tmp.injected", temporary.display())).is_file());

        let published = publish_stage(&daemon, &stage).expect("publish");
        assert!(!discard_pending_with_fault(&daemon, None, None, true));
        assert!(published.is_file());
        assert!(quarantine_pending(
            &daemon,
            Some("worker"),
            Some(&source_home)
        ));
        assert!(!has_pending(&daemon));
    }

    #[test]
    fn reread_delivery_covers_success_failure_and_retry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let source_home = temp.path().join("source");
        let daemon = temp.path().join("daemon");
        let state = source_home.join("state");
        fs::create_dir_all(daemon.join("config")).expect("config");
        fs::create_dir_all(&state).expect("state");
        fs::write(daemon.join("config/actor-harness"), "codex\n").expect("config item");
        let report = temp.path().join("report");
        fs::write(&report, "actor-harness\tpushed\t\n").expect("report");
        write_executable(&root.join("bin/mx-send.sh"), "#!/bin/sh\nexit 0\n");
        let context = RereadContext {
            id: "worker",
            destination_home: &daemon,
            report: &report,
            source_home: &source_home,
            root: &root,
            state: &state,
        };
        let (ok, output) = send_reread(&context);
        assert!(ok, "{output}");
        assert!(output.is_empty());
        assert!(!has_pending(&daemon));

        write_executable(
            &root.join("bin/mx-send.sh"),
            "#!/bin/sh\necho delivery-refused >&2\nexit 9\n",
        );
        let (ok, output) = send_reread(&context);
        assert!(!ok);
        assert!(output.contains("delivery-refused"));
        assert!(has_pending(&daemon));
        write_executable(&root.join("bin/mx-send.sh"), "#!/bin/sh\nexit 0\n");
        let empty_report = temp.path().join("empty-report");
        fs::write(&empty_report, "").expect("empty report");
        let retry = RereadContext {
            report: &empty_report,
            ..context
        };
        assert!(send_reread(&retry).0);
        assert!(!has_pending(&daemon));

        let missing = RereadContext {
            destination_home: &temp.path().join("missing"),
            ..retry
        };
        assert!(!send_reread(&missing).0);
    }

    #[test]
    fn daemon_home_validation_and_locking_cover_safety_boundaries() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let active = temp.path().join("active");
        let daemon = temp.path().join("daemon");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&active).expect("active");
        seeded_home(&daemon, "worker");
        assert_eq!(
            validate_daemon_home("worker", &daemon, &active, &root)
                .expect("valid")
                .path,
            fs::canonicalize(&daemon).expect("canonical")
        );
        assert!(
            validate_daemon_home("other", &daemon, &active, &root)
                .expect_err("wrong marker")
                .contains("marked for daemon")
        );
        assert!(
            validate_daemon_home("worker", &active, &active, &root)
                .expect_err("active")
                .contains("active Multplx home")
        );
        assert!(
            validate_daemon_home("worker", &root, &active, &root)
                .expect_err("root")
                .contains("Multplx repo")
        );
        assert_eq!(inherit_lock(&daemon), daemon.join(INHERIT_LOCK_REL));
        let first = acquire_inherit_lock(&daemon).expect("first lock");
        assert!(inherit_lock(&daemon).is_dir());
        drop(first);
        let second = acquire_inherit_lock(&daemon).expect("reacquire");
        drop(second);
    }

    #[test]
    fn shared_absence_identical_copy_and_quarantine_reuse_are_idempotent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let destination = temp.path().join("daemon/data");
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(&destination).expect("destination");
        let bytes = format!("{}same\n", shared_header());
        fs::write(source.join(SHARED_FILE), &bytes).expect("source file");
        fs::write(destination.join(SHARED_FILE), &bytes).expect("destination file");
        let identical = propagate_shared(&source, &destination);
        assert_eq!(identical.rows[0].status, Status::Unchanged);
        assert_eq!(
            fs::metadata(destination.join(SHARED_FILE))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            SHARED_MODE
        );

        fs::set_permissions(destination.join(SHARED_FILE), Permissions::from_mode(0o600))
            .expect("writable");
        fs::write(
            destination.join(SHARED_FILE),
            format!("{}local\n", shared_header()),
        )
        .expect("local drift");
        let first = propagate_shared(&temp.path().join("absent"), &destination);
        assert_eq!(first.rows[0].status, Status::Pushed);
        let quarantine = fs::read_dir(&destination)
            .expect("entries")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.to_string_lossy().contains(".quarantine."))
            .expect("quarantine");
        fs::copy(&quarantine, destination.join(SHARED_FILE)).expect("restore same drift");
        let second = propagate_shared(&temp.path().join("absent"), &destination);
        assert_eq!(second.rows[0].status, Status::Pushed);
        assert_eq!(
            fs::read_dir(&destination)
                .expect("entries")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().to_string_lossy().contains(".quarantine."))
                .count(),
            1
        );

        fs::remove_file(source.join(SHARED_FILE)).expect("remove source file");
        symlink("missing", source.join(SHARED_FILE)).expect("unsafe source symlink");
        assert!(propagate_shared(&source, &destination).failed);
    }

    #[test]
    fn pointer_delivery_refuses_missing_mismatched_and_silent_failures() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let home = temp.path().join("home");
        let state = home.join("state");
        fs::create_dir_all(&state).expect("state");
        let instruction = state.join(".mx-inherited-config-reread.test");
        assert!(
            send_pointer("worker", &instruction, &root, &home, &state)
                .expect_err("missing instruction")
                .contains("missing")
        );
        fs::write(&instruction, "instruction\n").expect("instruction");
        fs::write(
            PathBuf::from(format!("{}.pending", instruction.display())),
            "wrong\n",
        )
        .expect("mismatch");
        assert!(
            send_pointer("worker", &instruction, &root, &home, &state)
                .expect_err("mismatch")
                .contains("mismatched")
        );
        mark_pending(&instruction).expect("pending");
        assert!(
            send_pointer("worker", &instruction, &root, &home, &state)
                .expect_err("missing sender")
                .contains("not executable")
        );
        write_executable(&root.join("bin/mx-send.sh"), "#!/bin/sh\nexit 7\n");
        assert!(
            send_pointer("worker", &instruction, &root, &home, &state)
                .expect_err("silent failure")
                .contains("mx-send exited 7")
        );
        write_executable(
            &root.join("bin/mx-send.sh"),
            "#!/bin/sh\necho stdout-detail\nexit 8\n",
        );
        assert_eq!(
            send_pointer("worker", &instruction, &root, &home, &state).expect_err("stdout failure"),
            "stdout-detail"
        );
    }

    #[test]
    fn retry_report_rebuild_queue_limit_and_invalid_stage_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let source_home = temp.path().join("source");
        let daemon = temp.path().join("daemon");
        let state = source_home.join("state");
        fs::create_dir_all(daemon.join("config")).expect("config");
        fs::create_dir_all(&state).expect("state");
        fs::write(daemon.join("config/actor-harness"), "codex\n").expect("config");
        write_executable(&root.join("bin/mx-send.sh"), "#!/bin/sh\nexit 0\n");
        let report = temp.path().join("report");
        fs::write(&report, "actor-harness\tpushed\t\n").expect("report");
        assert!(publish_stage(&daemon, &temp.path().join("missing")).is_err());

        let stage = next_stage(&source_home, "worker").expect("stage");
        save_retry_report(&report, &stage).expect("retry report");
        fs::remove_file(&stage).expect("leave report only");
        let empty = temp.path().join("empty");
        fs::write(&empty, "").expect("empty report");
        let context = RereadContext {
            id: "worker",
            destination_home: &daemon,
            report: &empty,
            source_home: &source_home,
            root: &root,
            state: &state,
        };
        assert!(send_reread(&context).0);
        assert!(pending_reports(&source_home, "worker").is_empty());

        let queue_home = temp.path().join("queue");
        let queue_dir = retry_dir(&queue_home, "full");
        fs::create_dir_all(&queue_dir).expect("queue");
        for index in 0..MAX_PENDING {
            fs::write(
                queue_dir.join(format!(".mx-inherited-config-reread.{index:03}")),
                "queued\n",
            )
            .expect("queued generation");
        }
        assert!(retry_queue_full(&queue_home, "full"));
        let full = RereadContext {
            id: "full",
            destination_home: &daemon,
            report: &report,
            source_home: &queue_home,
            root: &root,
            state: &state,
        };
        let (ok, output) = send_reread(&full);
        assert!(!ok);
        assert!(output.contains("queue is full"));
    }

    #[test]
    fn quarantine_retention_and_home_validation_cover_containment_matrix() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("quarantine-home");
        for _ in 0..MAX_QUARANTINE + 3 {
            quarantine_directory(&home).expect("quarantine generation");
        }
        let generations = fs::read_dir(home.join("state/.mx-inherited-config-reread-quarantine"))
            .expect("generations")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .count();
        assert!(generations <= MAX_QUARANTINE);

        let root = temp.path().join("root");
        let active = temp.path().join("active");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&active).expect("active");
        let file = temp.path().join("file-home");
        fs::write(&file, "file\n").expect("file home");
        assert_eq!(
            validate_daemon_home("worker", &file, &active, &root).expect_err("file"),
            "not a directory"
        );

        let inside_active = active.join("daemon");
        seeded_home(&inside_active, "worker");
        assert!(
            validate_daemon_home("worker", &inside_active, &active, &root)
                .expect_err("inside active")
                .contains("inside the active")
        );
        let inside_root = root.join("daemon");
        seeded_home(&inside_root, "worker");
        assert!(
            validate_daemon_home("worker", &inside_root, &active, &root)
                .expect_err("inside root")
                .contains("inside the Multplx repo")
        );

        let daemon = temp.path().join("matrix-daemon");
        seeded_home(&daemon, "worker");
        fs::remove_dir(daemon.join("data")).expect("remove data");
        fs::write(daemon.join("data"), "not a directory\n").expect("data file");
        assert!(
            validate_daemon_home("worker", &daemon, &active, &root)
                .expect_err("data file")
                .contains("data path is not a directory")
        );
        fs::remove_file(daemon.join("data")).expect("remove data file");
        symlink(&active, daemon.join("data")).expect("escaping data symlink");
        assert!(
            validate_daemon_home("worker", &daemon, &active, &root)
                .expect_err("escape")
                .contains("inside the daemon home")
        );
        fs::remove_file(daemon.join("data")).expect("remove data symlink");
        fs::create_dir(daemon.join("data")).expect("restore data");
        fs::remove_file(daemon.join(".mx-daemon-home")).expect("remove marker");
        symlink("marker-target", daemon.join(".mx-daemon-home")).expect("marker symlink");
        assert!(
            validate_daemon_home("worker", &daemon, &active, &root)
                .expect_err("marker symlink")
                .contains("marker must not be a symlink")
        );
        fs::remove_file(daemon.join(".mx-daemon-home")).expect("remove marker symlink");
        assert!(
            validate_daemon_home("worker", &daemon, &active, &root)
                .expect_err("missing marker")
                .contains("not a seeded")
        );
        fs::write(daemon.join(".mx-daemon-home"), "worker\n").expect("marker");
        fs::remove_file(daemon.join("AGENTS.md")).expect("remove agents");
        assert!(
            validate_daemon_home("worker", &daemon, &active, &root)
                .expect_err("missing agents")
                .contains("missing AGENTS.md")
        );
        fs::write(daemon.join("AGENTS.md"), "# daemon\n").expect("agents");
        fs::remove_dir_all(daemon.join("bin")).expect("remove bin");
        assert!(
            validate_daemon_home("worker", &daemon, &active, &root)
                .expect_err("missing bin")
                .contains("missing bin/")
        );
    }

    #[test]
    fn remaining_copy_retry_and_home_edge_contracts_are_observable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        let target = temp.path().join("old-target");
        fs::write(&source, b"new bytes").expect("source");
        fs::write(&target, b"old bytes").expect("target");
        symlink(&target, &destination).expect("destination symlink");
        copy_atomic(&source, &destination, 0o600).expect("replace symlink");
        assert_eq!(fs::read(&destination).expect("destination"), b"new bytes");
        assert_eq!(fs::read(&target).expect("target unchanged"), b"old bytes");
        assert!(copy_atomic(&source, temp.path(), 0o600).is_err());
        assert!(quarantine_shared(temp.path()).is_err());
        assert!(!header_valid(&temp.path().join("missing")));

        let source_home = temp.path().join("primary");
        let daemon = temp.path().join("daemon-defaults");
        fs::create_dir_all(source_home.join("config")).expect("source config");
        fs::create_dir_all(source_home.join("data")).expect("source data");
        fs::create_dir_all(daemon.join("config")).expect("daemon config");
        fs::create_dir_all(daemon.join("data")).expect("daemon data");
        fs::write(source_home.join("config/actor-harness"), b"codex\n").expect("config");
        let outcome = propagate_daemon(&source_home, &daemon, None, None).expect("defaults");
        assert!(outcome.rows.iter().any(|row| row.item == "actor-harness"));

        let empty_report = temp.path().join("empty-report");
        fs::write(&empty_report, b"").expect("empty report");
        let empty_instruction = temp.path().join("empty-instruction");
        assert!(
            !write_reread_instruction(&daemon, &empty_report, &empty_instruction)
                .expect("no changed items")
        );
        let absent_report = temp.path().join("absent-report");
        fs::write(&absent_report, b"actor-harness\tpushed\t\n").expect("absent report");
        fs::remove_file(daemon.join("config/actor-harness")).expect("remove config");
        let absent_instruction = temp.path().join("absent-instruction");
        assert!(
            write_reread_instruction(&daemon, &absent_report, &absent_instruction)
                .expect("absent instruction")
        );
        assert!(
            fs::read_to_string(&absent_instruction)
                .expect("instruction")
                .contains("ABSENT")
        );
        assert!(retry_dir(&source_home, "").ends_with("unknown"));

        let retry_report =
            retry_dir(&source_home, "worker").join(".mx-inherited-config-reread.retry.report");
        fs::create_dir_all(retry_report.parent().expect("retry parent")).expect("retry directory");
        fs::write(&retry_report, b"actor-harness\tunchanged\t\n").expect("retry report");
        let root = temp.path().join("sender-root");
        let state = source_home.join("state");
        fs::create_dir_all(&state).expect("state");
        let context = RereadContext {
            id: "worker",
            destination_home: &daemon,
            report: &empty_report,
            source_home: &source_home,
            root: &root,
            state: &state,
        };
        let (ok, output) = send_reread(&context);
        assert!(!ok);
        assert!(output.contains("could not rebuild retry instruction"));

        let root_home = Path::new("/");
        let active = temp.path().join("active");
        let repo = temp.path().join("repo");
        fs::create_dir(&active).expect("active");
        fs::create_dir(&repo).expect("repo");
        assert!(
            validate_daemon_home("worker", root_home, &active, &repo)
                .expect_err("filesystem root")
                .contains("filesystem root")
        );
        assert!(
            validate_daemon_home("worker", temp.path(), &active, &repo)
                .expect_err("ancestor")
                .contains("ancestor of the active")
        );

        let dangling = temp.path().join("dangling-daemon");
        seeded_home(&dangling, "worker");
        fs::remove_dir(dangling.join("projects")).expect("remove projects");
        symlink("missing-target", dangling.join("projects")).expect("dangling projects");
        assert!(
            validate_daemon_home("worker", &dangling, &active, &repo)
                .expect_err("dangling surface")
                .contains("must resolve inside")
        );
    }
}

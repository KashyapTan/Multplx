//! Durable wake records, queue transactions, annotations, and watcher identity
//! from `bin/mx-wake-lib.sh`.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustix::fs::OFlags;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::filesystem::{append_single_write, atomic_replace, read_bounded_regular};
use crate::locks::DirectoryLock;
use crate::process::{ProcessIdentity, ProcessProbe};

const MAX_QUEUE_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_INBOX_ITEM_BYTES: usize = 256 * 1024;
const INBOX_SCHEMA: &str = "mx-wake-inbox.v1";

/// Closed durable wake kind vocabulary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WakeKind {
    /// Actor status or turn-end signal.
    Signal,
    /// Stale endpoint observation.
    Stale,
    /// Check result.
    Check,
    /// No-change heartbeat.
    Heartbeat,
}

impl WakeKind {
    /// Parse a wire token.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "signal" => Ok(Self::Signal),
            "stale" => Ok(Self::Stale),
            "check" => Ok(Self::Check),
            "heartbeat" => Ok(Self::Heartbeat),
            _ => Err(CoreError::UnknownValue {
                kind: "wake kind",
                value: value.to_owned(),
            }),
        }
    }

    /// Return the exact wire token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Signal => "signal",
            Self::Stale => "stale",
            Self::Check => "check",
            Self::Heartbeat => "heartbeat",
        }
    }
}

fn clean_field(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            _ => character,
        })
        .collect()
}

/// One exact five-field wake queue row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WakeRecord {
    /// Append epoch seconds.
    pub epoch: u64,
    /// Home-local monotonic sequence.
    pub sequence: u64,
    /// Closed wake kind.
    pub kind: WakeKind,
    /// Kind-specific dedupe key.
    pub key: String,
    /// Display payload.
    pub payload: String,
}

/// Identity-bound lease for one inbox item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WakeClaim {
    /// Exact process lifetime that owns handling.
    pub owner: ProcessIdentity,
    /// Claim time in epoch seconds.
    pub claimed_at: u64,
}

/// Closed durable disposition vocabulary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WakeDispositionKind {
    /// The dependent action was recorded or the observation was handled.
    Handled,
    /// A newer authoritative fact made this observation obsolete.
    Superseded,
    /// Handling is paused on a named resumable condition.
    Waiting,
    /// Handling continues through another durable operation.
    FollowUp,
}

impl WakeDispositionKind {
    /// Parse a CLI token.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "handled" => Ok(Self::Handled),
            "superseded" => Ok(Self::Superseded),
            "waiting" => Ok(Self::Waiting),
            "follow-up" => Ok(Self::FollowUp),
            _ => Err(CoreError::UnknownValue {
                kind: "wake disposition",
                value: value.to_owned(),
            }),
        }
    }

    /// Return the wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Handled => "handled",
            Self::Superseded => "superseded",
            Self::Waiting => "waiting",
            Self::FollowUp => "follow-up",
        }
    }
}

/// Recorded handling outcome. Waiting and follow-up variants carry their
/// continuation instead of relying on transcript memory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WakeDisposition {
    /// Closed handling outcome.
    pub kind: WakeDispositionKind,
    /// Durable outcome time in epoch seconds.
    pub recorded_at: u64,
    /// Concise reason or result.
    pub detail: String,
    /// Named condition for a waiting item.
    pub condition: Option<String>,
    /// Stable event or operation that resumes a waiting item.
    pub resume_trigger: Option<String>,
    /// Bounded fallback recheck for a waiting item.
    pub recheck_after_epoch: Option<u64>,
    /// Stable durable operation that owns continued handling.
    pub follow_up_id: Option<String>,
}

impl WakeDisposition {
    /// Validate that the disposition cannot strand remaining work.
    pub fn validate(&self) -> Result<()> {
        if self.detail.trim().is_empty() {
            return Err(CoreError::MalformedRecord {
                kind: "wake disposition",
                reason: "detail is empty",
            });
        }
        match self.kind {
            WakeDispositionKind::Waiting
                if self.condition.as_deref().is_none_or(str::is_empty)
                    || (self.resume_trigger.as_deref().is_none_or(str::is_empty)
                        && self.recheck_after_epoch.is_none()) =>
            {
                Err(CoreError::MalformedRecord {
                    kind: "wake disposition",
                    reason: "waiting requires a condition and trigger or recheck",
                })
            }
            WakeDispositionKind::FollowUp
                if self.follow_up_id.as_deref().is_none_or(str::is_empty) =>
            {
                Err(CoreError::MalformedRecord {
                    kind: "wake disposition",
                    reason: "follow-up requires a stable operation identity",
                })
            }
            WakeDispositionKind::Handled | WakeDispositionKind::Superseded
                if self.condition.is_some()
                    || self.resume_trigger.is_some()
                    || self.recheck_after_epoch.is_some()
                    || self.follow_up_id.is_some() =>
            {
                Err(CoreError::MalformedRecord {
                    kind: "wake disposition",
                    reason: "terminal disposition carries continuation fields",
                })
            }
            _ => Ok(()),
        }
    }
}

/// One durable orchestrator inbox item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WakeInboxItem {
    /// Schema marker retained for migration.
    pub schema: String,
    /// Stable home-local event identity.
    pub event_id: String,
    /// Original durable queue record.
    pub record: WakeRecord,
    /// At most one exact process lifetime owns handling.
    pub claim: Option<WakeClaim>,
    /// Durable handling outcome, recorded before acknowledgement.
    pub disposition: Option<WakeDisposition>,
    /// Prior waiting dispositions retained when their continuation resumes.
    #[serde(default)]
    pub disposition_history: Vec<WakeDisposition>,
    /// Acknowledgement time, separate from transport delivery and response.
    pub acknowledged_at: Option<u64>,
}

impl WakeRecord {
    /// Construct with delimiter-scrubbed text fields.
    #[must_use]
    pub fn new(epoch: u64, sequence: u64, kind: WakeKind, key: &str, payload: &str) -> Self {
        Self {
            epoch,
            sequence,
            kind,
            key: clean_field(key),
            payload: clean_field(payload),
        }
    }

    /// Parse exactly five tab-separated fields.
    pub fn parse(line: &str) -> Result<Self> {
        if line.len() > MAX_RECORD_BYTES {
            return Err(CoreError::RecordTooLarge {
                kind: "wake",
                limit: MAX_RECORD_BYTES,
            });
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 5 {
            return Err(CoreError::MalformedRecord {
                kind: "wake",
                reason: "expected exactly five fields",
            });
        }
        let epoch = fields[0].parse().map_err(|_| CoreError::MalformedRecord {
            kind: "wake",
            reason: "epoch is not numeric",
        })?;
        let sequence = fields[1].parse().map_err(|_| CoreError::MalformedRecord {
            kind: "wake",
            reason: "sequence is not numeric",
        })?;
        Ok(Self::new(
            epoch,
            sequence,
            WakeKind::parse(fields[2])?,
            fields[3],
            fields[4],
        ))
    }

    /// Render exact line bytes with a trailing newline.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\n",
            self.epoch,
            self.sequence,
            self.kind.as_str(),
            self.key,
            self.payload
        )
    }

    fn dedupe_key(&self) -> String {
        if self.kind == WakeKind::Heartbeat {
            "heartbeat".to_owned()
        } else {
            format!("{}\0{}", self.kind.as_str(), self.key)
        }
    }
}

/// Keep the first-seen key ordering and the last row for each key.
#[must_use]
pub fn dedupe(records: &[WakeRecord]) -> Vec<WakeRecord> {
    let mut order = Vec::new();
    let mut latest = HashMap::new();
    for record in records {
        let key = record.dedupe_key();
        if !latest.contains_key(&key) {
            order.push(key.clone());
        }
        latest.insert(key, record.clone());
    }
    order.iter().filter_map(|key| latest.remove(key)).collect()
}

/// Coalesce only observational heartbeat noise before durable inbox delivery.
/// Signal, stale, and check rows are transition facts: even matching keys can
/// describe different state changes and therefore retain distinct event IDs.
fn coalesce_inbox_records(records: &[WakeRecord]) -> Vec<WakeRecord> {
    let last_heartbeat = records
        .iter()
        .rposition(|record| record.kind == WakeKind::Heartbeat);
    records
        .iter()
        .enumerate()
        .filter(|(index, record)| {
            record.kind != WakeKind::Heartbeat || Some(*index) == last_heartbeat
        })
        .map(|(_, record)| record.clone())
        .collect()
}

/// Status file mapping for an annotation-safe signal key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusKey {
    /// Home-local status filename.
    pub filename: String,
    /// True when a turn-ended key only points to historical context.
    pub historical: bool,
}

/// Map a structurally valid signal key without trusting queue payload text.
pub fn status_key_map(key: &str) -> Result<StatusKey> {
    let (id, historical) = if let Some(id) = key.strip_suffix(".status") {
        (id, false)
    } else if let Some(id) = key.strip_suffix(".turn-ended") {
        (id, true)
    } else {
        return Err(CoreError::MalformedRecord {
            kind: "wake status key",
            reason: "unsupported suffix",
        });
    };
    crate::identifiers::TaskId::parse(id)?;
    Ok(StatusKey {
        filename: format!("{id}.status"),
        historical,
    })
}

/// One bounded last-event observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LatestEvent {
    /// Tab and CR scrubbed last nonblank line.
    pub line: String,
    /// Whether the selected row may begin before the bounded tail.
    pub truncated: bool,
}

/// Read at most the last `tail_limit` bytes without following a symlink.
pub fn latest_event(path: impl AsRef<Path>, tail_limit: usize) -> Result<LatestEvent> {
    let path = path.as_ref();
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(OFlags::NOFOLLOW.bits() as i32);
    let mut file = options
        .open(path)
        .map_err(|error| CoreError::io("open status annotation", path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| CoreError::io("inspect status annotation", path, error))?;
    if !metadata.is_file() {
        return Err(CoreError::UnsafePath {
            path: path.to_path_buf(),
            reason: "status annotation target is not a regular file",
        });
    }
    let size = metadata.len();
    let start = size.saturating_sub(tail_limit as u64);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| CoreError::io("seek status annotation", path, error))?;
    let mut bytes = Vec::with_capacity((size - start) as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error| CoreError::io("read status annotation", path, error))?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let (line_number, line) = lines
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| !line.trim().is_empty())
        .ok_or(CoreError::MalformedRecord {
            kind: "status annotation",
            reason: "no nonblank event",
        })?;
    Ok(LatestEvent {
        line: line.replace(['\t', '\r'], " "),
        truncated: size > tail_limit as u64 && line_number == 0,
    })
}

/// Annotation bounds matching the current drain renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnnotationLimits {
    /// Tail bytes read from each file.
    pub tail_bytes: usize,
    /// Maximum bytes per rendered annotation.
    pub item_bytes: usize,
    /// Global rendered-byte cap.
    pub global_bytes: usize,
    /// Maximum files read.
    pub read_cap: usize,
}

impl Default for AnnotationLimits {
    fn default() -> Self {
        Self {
            tail_bytes: 8192,
            item_bytes: 2048,
            global_bytes: 8192,
            read_cap: 8,
        }
    }
}

/// Render best-effort drain-time context after raw queue consumption commits.
#[must_use]
pub fn render_annotations(
    state: &Path,
    records: &[WakeRecord],
    limits: AnnotationLimits,
) -> String {
    let mut order = Vec::new();
    let mut modes: HashMap<String, bool> = HashMap::new();
    for record in records
        .iter()
        .filter(|record| record.kind == WakeKind::Signal)
    {
        let Ok(mapped) = status_key_map(&record.key) else {
            continue;
        };
        if !modes.contains_key(&mapped.filename) {
            order.push(mapped.filename.clone());
            modes.insert(mapped.filename.clone(), mapped.historical);
        } else if !mapped.historical {
            modes.insert(mapped.filename, false);
        }
    }
    let mut output = String::new();
    let mut used = 0;
    let mut omitted = 0;
    let mut read_omitted = 0;
    let marker_reserve = 192;
    for (index, filename) in order.iter().enumerate() {
        if index >= limits.read_cap {
            read_omitted += 1;
            continue;
        }
        let Ok(event) = latest_event(state.join(filename), limits.tail_bytes) else {
            continue;
        };
        let mut prefix =
            "wake annotation: latest wake-EVENT observed at drain, not current state".to_owned();
        if modes.get(filename).copied().unwrap_or(false) {
            prefix.push_str("; historical / not necessarily the triggering event");
        }
        let mut line = format!("{prefix}: {filename}: {}", event.line);
        if event.truncated {
            line.push_str(" [truncated]");
        }
        if line.len() + 1 > limits.item_bytes {
            let suffix = " [truncated]";
            let keep = limits.item_bytes.saturating_sub(suffix.len() + 1);
            line.truncate(keep);
            line.push_str(suffix);
        }
        let bytes = line.len() + 1;
        if used + bytes + marker_reserve > limits.global_bytes {
            omitted += 1;
            continue;
        }
        output.push_str(&line);
        output.push('\n');
        used += bytes;
    }
    if omitted > 0 {
        output.push_str(&format!(
            "wake annotation: {omitted} annotations omitted (global enrichment byte cap)\n"
        ));
    }
    if read_omitted > 0 {
        output.push_str(&format!(
            "wake annotation: {read_omitted} annotations omitted (enrichment read cap)\n"
        ));
    }
    output
}

/// Durable state-local wake queue.
#[derive(Clone, Debug)]
pub struct WakeQueue {
    state: PathBuf,
    queue: PathBuf,
    sequence: PathBuf,
    lock: PathBuf,
    inbox: PathBuf,
}

impl WakeQueue {
    /// Construct current default queue paths below one state directory.
    #[must_use]
    pub fn new(state: impl Into<PathBuf>) -> Self {
        let state = state.into();
        Self {
            queue: state.join(".wake-queue"),
            sequence: state.join(".wake-queue.seq"),
            lock: state.join(".wake-queue.lock"),
            inbox: state.join("wake-inbox"),
            state,
        }
    }

    fn event_id(sequence: u64) -> String {
        format!("wake-{sequence:020}")
    }

    fn item_path(&self, event_id: &str) -> Result<PathBuf> {
        let suffix = event_id
            .strip_prefix("wake-")
            .filter(|suffix| suffix.len() == 20 && suffix.bytes().all(|byte| byte.is_ascii_digit()))
            .ok_or_else(|| CoreError::InvalidIdentifier {
                kind: "wake event",
                value: event_id.to_owned(),
            })?;
        let _ = suffix;
        Ok(self.inbox.join(format!("{event_id}.json")))
    }

    fn ensure_inbox(&self) -> Result<()> {
        fs::create_dir_all(&self.inbox)
            .map_err(|error| CoreError::io("create wake inbox", &self.inbox, error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.inbox, fs::Permissions::from_mode(0o700))
                .map_err(|error| CoreError::io("protect wake inbox", &self.inbox, error))?;
        }
        Ok(())
    }

    fn read_item_path(&self, path: &Path) -> Result<WakeInboxItem> {
        let bytes = read_bounded_regular(path, MAX_INBOX_ITEM_BYTES)?;
        let item: WakeInboxItem =
            serde_json::from_slice(&bytes).map_err(|_| CoreError::MalformedRecord {
                kind: "wake inbox",
                reason: "invalid JSON",
            })?;
        if item.schema != INBOX_SCHEMA
            || self.item_path(&item.event_id)? != path
            || item.event_id != Self::event_id(item.record.sequence)
        {
            return Err(CoreError::MalformedRecord {
                kind: "wake inbox",
                reason: "schema, filename, or event identity mismatch",
            });
        }
        if let Some(disposition) = &item.disposition {
            disposition.validate()?;
        }
        if item.acknowledged_at.is_some() && item.disposition.is_none() {
            return Err(CoreError::MalformedRecord {
                kind: "wake inbox",
                reason: "acknowledgement precedes disposition",
            });
        }
        Ok(item)
    }

    fn write_item(&self, item: &WakeInboxItem) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(item).map_err(|_| CoreError::MalformedRecord {
            kind: "wake inbox",
            reason: "cannot encode JSON",
        })?;
        atomic_replace(self.item_path(&item.event_id)?, &bytes, 0o600)
    }

    fn scan_items_locked(&self) -> Result<Vec<(PathBuf, WakeInboxItem)>> {
        match fs::symlink_metadata(&self.inbox) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(CoreError::MalformedRecord {
                    kind: "wake inbox",
                    reason: "inbox is not a regular directory",
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(CoreError::io("inspect wake inbox", &self.inbox, error)),
        }
        let mut paths = fs::read_dir(&self.inbox)
            .map_err(|error| CoreError::io("scan wake inbox", &self.inbox, error))?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .into_iter()
            .map(|path| self.read_item_path(&path).map(|item| (path, item)))
            .collect()
    }

    /// Copy queued records into stable inbox receipts and clear the source only
    /// after every receipt is durable. Repeating after any interruption converges.
    fn ingest_queue_locked(&self) -> Result<usize> {
        self.ensure_inbox()?;
        let bytes = match read_bounded_regular(&self.queue, MAX_QUEUE_BYTES) {
            Ok(bytes) => bytes,
            Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                atomic_replace(&self.queue, b"", 0o600)?;
                return Ok(0);
            }
            Err(error) => return Err(error),
        };
        if bytes.is_empty() {
            return Ok(0);
        }
        let records = coalesce_inbox_records(&parse_queue_bytes(&bytes)?);
        for record in &records {
            let event_id = Self::event_id(record.sequence);
            let path = self.item_path(&event_id)?;
            if path.exists() {
                let existing = self.read_item_path(&path)?;
                if existing.record != *record {
                    return Err(CoreError::MalformedRecord {
                        kind: "wake inbox",
                        reason: "stable event identity conflicts with queued record",
                    });
                }
                continue;
            }
            self.write_item(&WakeInboxItem {
                schema: INBOX_SCHEMA.to_owned(),
                event_id,
                record: record.clone(),
                claim: None,
                disposition: None,
                disposition_history: Vec::new(),
                acknowledged_at: None,
            })?;
        }
        atomic_replace(&self.queue, b"", 0o600)?;
        Ok(records.len())
    }

    fn owner_is_current(owner: &ProcessIdentity, processes: &impl ProcessProbe) -> Result<bool> {
        if !processes.is_alive(owner.pid) {
            return Ok(false);
        }
        match processes.identity(owner.pid) {
            Ok(current) => Ok(current == *owner),
            Err(_) => Err(CoreError::LockHeld {
                owner: format!("PID {} with ambiguous process identity", owner.pid),
            }),
        }
    }

    fn verify_requester(owner: &ProcessIdentity, processes: &impl ProcessProbe) -> Result<()> {
        if Self::owner_is_current(owner, processes)? {
            Ok(())
        } else {
            Err(CoreError::LockHeld {
                owner: format!("inactive handler PID {}", owner.pid),
            })
        }
    }

    fn next_sequence_locked(&self) -> Result<u64> {
        let persisted = match read_bounded_regular(&self.sequence, 64) {
            Ok(bytes) => {
                let text = std::str::from_utf8(&bytes).map_err(|_| CoreError::MalformedRecord {
                    kind: "wake sequence",
                    reason: "counter is not UTF-8",
                })?;
                text.trim()
                    .parse::<u64>()
                    .map_err(|_| CoreError::MalformedRecord {
                        kind: "wake sequence",
                        reason: "counter is not numeric",
                    })?
            }
            Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        let queued_max = match read_bounded_regular(&self.queue, MAX_QUEUE_BYTES) {
            Ok(bytes) => parse_queue_bytes(&bytes)?
                .into_iter()
                .map(|record| record.sequence)
                .max()
                .unwrap_or(0),
            Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        let inbox_max = self
            .scan_items_locked()?
            .into_iter()
            .map(|(_, item)| item.record.sequence)
            .max()
            .unwrap_or(0);
        persisted
            .max(queued_max)
            .max(inbox_max)
            .checked_add(1)
            .ok_or(CoreError::MalformedRecord {
                kind: "wake sequence",
                reason: "sequence overflow",
            })
    }

    /// Move queued events into the durable inbox and claim available work for
    /// one verified process lifetime. No lock is retained after return.
    pub fn claim_available(
        &self,
        owner: &ProcessIdentity,
        now: SystemTime,
        limit: usize,
        processes: &impl ProcessProbe,
    ) -> Result<Vec<WakeInboxItem>> {
        Self::verify_requester(owner, processes)?;
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        self.ingest_queue_locked()?;
        let epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let mut claimed = Vec::new();
        for (_, mut item) in self.scan_items_locked()? {
            if claimed.len() >= limit || item.acknowledged_at.is_some() {
                continue;
            }
            // A claim is a durable delivery fact. Repeated claim commands by
            // the same process do not redisplay it; `inbox_items` provides
            // explicit crash recovery visibility and abandoned recovery makes
            // it claimable by a replacement process.
            if item.claim.is_some() {
                continue;
            }
            item.claim = Some(WakeClaim {
                owner: owner.clone(),
                claimed_at: epoch,
            });
            self.write_item(&item)?;
            claimed.push(item);
        }
        Ok(claimed)
    }

    /// Clear only claims whose exact former process lifetime is confirmed gone.
    pub fn recover_abandoned_claims(&self, processes: &impl ProcessProbe) -> Result<usize> {
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        let mut recovered = 0;
        for (_, mut item) in self.scan_items_locked()? {
            let Some(claim) = &item.claim else {
                continue;
            };
            if item.acknowledged_at.is_none() && !Self::owner_is_current(&claim.owner, processes)? {
                item.claim = None;
                self.write_item(&item)?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    /// Record one claim owner's disposition before acknowledgement.
    pub fn record_disposition(
        &self,
        event_id: &str,
        owner: &ProcessIdentity,
        disposition: WakeDisposition,
        processes: &impl ProcessProbe,
    ) -> Result<WakeInboxItem> {
        disposition.validate()?;
        Self::verify_requester(owner, processes)?;
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        let path = self.item_path(event_id)?;
        let mut item = self.read_item_path(&path)?;
        if item.claim.as_ref().map(|claim| &claim.owner) != Some(owner) {
            return Err(CoreError::LockHeld {
                owner: "another wake handler".to_owned(),
            });
        }
        if let Some(existing) = &item.disposition {
            if existing.kind == disposition.kind
                && existing.detail == disposition.detail
                && existing.condition == disposition.condition
                && existing.resume_trigger == disposition.resume_trigger
                && existing.recheck_after_epoch == disposition.recheck_after_epoch
                && existing.follow_up_id == disposition.follow_up_id
            {
                return Ok(item);
            }
            return Err(CoreError::MalformedRecord {
                kind: "wake disposition",
                reason: "event already has a different disposition",
            });
        }
        item.disposition = Some(disposition);
        self.write_item(&item)?;
        Ok(item)
    }

    /// Acknowledge only after a durable disposition exists.
    pub fn acknowledge(
        &self,
        event_id: &str,
        owner: &ProcessIdentity,
        now: SystemTime,
        processes: &impl ProcessProbe,
    ) -> Result<WakeInboxItem> {
        Self::verify_requester(owner, processes)?;
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        let path = self.item_path(event_id)?;
        let mut item = self.read_item_path(&path)?;
        if item.claim.as_ref().map(|claim| &claim.owner) != Some(owner) {
            return Err(CoreError::LockHeld {
                owner: "another wake handler".to_owned(),
            });
        }
        if item.disposition.is_none() {
            return Err(CoreError::MalformedRecord {
                kind: "wake acknowledgement",
                reason: "disposition is not recorded",
            });
        }
        if item.acknowledged_at.is_none() {
            item.acknowledged_at =
                Some(now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs());
            self.write_item(&item)?;
        }
        Ok(item)
    }

    /// Resume an acknowledged waiting item from its named trigger or due
    /// fallback recheck, preserving the stable event identity.
    pub fn resume_waiting(
        &self,
        event_id: &str,
        trigger: &str,
        owner: &ProcessIdentity,
        now: SystemTime,
        processes: &impl ProcessProbe,
    ) -> Result<WakeInboxItem> {
        Self::verify_requester(owner, processes)?;
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        let path = self.item_path(event_id)?;
        let mut item = self.read_item_path(&path)?;
        let disposition = item
            .disposition
            .as_ref()
            .ok_or(CoreError::MalformedRecord {
                kind: "wake continuation",
                reason: "missing waiting disposition",
            })?;
        if disposition.kind != WakeDispositionKind::Waiting || item.acknowledged_at.is_none() {
            return Err(CoreError::MalformedRecord {
                kind: "wake continuation",
                reason: "event is not acknowledged waiting work",
            });
        }
        let epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let named = disposition.resume_trigger.as_deref() == Some(trigger) && !trigger.is_empty();
        let due = trigger == "recheck"
            && disposition
                .recheck_after_epoch
                .is_some_and(|deadline| deadline <= epoch);
        if !named && !due {
            return Err(CoreError::MalformedRecord {
                kind: "wake continuation",
                reason: "trigger does not match and recheck is not due",
            });
        }
        if let Some(claim) = &item.claim
            && claim.owner != *owner
            && Self::owner_is_current(&claim.owner, processes)?
        {
            return Err(CoreError::LockHeld {
                owner: format!("PID {}", claim.owner.pid),
            });
        }
        item.claim = Some(WakeClaim {
            owner: owner.clone(),
            claimed_at: epoch,
        });
        item.disposition_history.push(
            item.disposition
                .take()
                .expect("validated waiting disposition"),
        );
        item.acknowledged_at = None;
        self.write_item(&item)?;
        Ok(item)
    }

    /// Resume every acknowledged waiting item whose fallback deadline is due.
    /// This is called at the normal owner claim boundary so a named trigger is
    /// optional for conditions that also requested periodic rechecking.
    pub fn resume_due_waiting(
        &self,
        owner: &ProcessIdentity,
        now: SystemTime,
        processes: &impl ProcessProbe,
    ) -> Result<usize> {
        Self::verify_requester(owner, processes)?;
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        let epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let mut resumed = 0;
        for (_, mut item) in self.scan_items_locked()? {
            let due = item.acknowledged_at.is_some()
                && item.disposition.as_ref().is_some_and(|disposition| {
                    disposition.kind == WakeDispositionKind::Waiting
                        && disposition
                            .recheck_after_epoch
                            .is_some_and(|deadline| deadline <= epoch)
                });
            if !due {
                continue;
            }
            if let Some(claim) = &item.claim
                && claim.owner != *owner
                && Self::owner_is_current(&claim.owner, processes)?
            {
                continue;
            }
            item.claim = None;
            item.disposition_history
                .push(item.disposition.take().expect("due waiting disposition"));
            item.acknowledged_at = None;
            self.write_item(&item)?;
            resumed += 1;
        }
        Ok(resumed)
    }

    /// Read every durable inbox item in stable event order.
    pub fn inbox_items(&self, processes: &impl ProcessProbe) -> Result<Vec<WakeInboxItem>> {
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        Ok(self
            .scan_items_locked()?
            .into_iter()
            .map(|(_, item)| item)
            .collect())
    }

    /// Observe durable inbox history without acquiring a lock or changing
    /// claims. Snapshot readers use this alongside [`Self::observe_unfinished_count`].
    pub fn observe_inbox_items(&self) -> Result<Vec<WakeInboxItem>> {
        Ok(self
            .scan_items_locked()?
            .into_iter()
            .map(|(_, item)| item)
            .collect())
    }

    /// Count events that still need handling or a retained waiting continuation.
    pub fn unfinished_count(&self, processes: &impl ProcessProbe) -> Result<usize> {
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        self.observe_unfinished_count()
    }

    /// Observe unfinished queue and inbox work without locks or filesystem
    /// mutation. This is a conservative session-start/status snapshot; callers
    /// that need a serialized decision use [`Self::unfinished_count`].
    pub fn observe_unfinished_count(&self) -> Result<usize> {
        let items = self.scan_items_locked()?;
        let mut known = items
            .iter()
            .map(|(_, item)| item.record.sequence)
            .collect::<std::collections::HashSet<_>>();
        let mut count = items
            .into_iter()
            .filter(|(_, item)| {
                item.acknowledged_at.is_none()
                    || item
                        .disposition
                        .as_ref()
                        .is_some_and(|disposition| disposition.kind == WakeDispositionKind::Waiting)
            })
            .count();
        let queued = match read_bounded_regular(&self.queue, MAX_QUEUE_BYTES) {
            Ok(bytes) => coalesce_inbox_records(&parse_queue_bytes(&bytes)?),
            Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Vec::new()
            }
            Err(error) => return Err(error),
        };
        count += queued
            .into_iter()
            .filter(|record| known.insert(record.sequence))
            .count();
        Ok(count)
    }

    /// Append one serialized record under the queue lock.
    pub fn append(
        &self,
        kind: WakeKind,
        key: &str,
        payload: &str,
        now: SystemTime,
        processes: &impl ProcessProbe,
    ) -> Result<WakeRecord> {
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        let sequence = self.next_sequence_locked()?;
        atomic_replace(&self.sequence, format!("{sequence}\n").as_bytes(), 0o600)?;
        let epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let record = WakeRecord::new(epoch, sequence, kind, key, payload);
        append_single_write(&self.queue, record.render().as_bytes(), 0o600)?;
        Ok(record)
    }

    /// Append one event only when its stable kind/key identity is absent from
    /// both the source queue and durable inbox. Retained inbox history makes a
    /// repeated external request converge without coalescing unrelated events.
    pub fn append_once(
        &self,
        kind: WakeKind,
        key: &str,
        payload: &str,
        now: SystemTime,
        processes: &impl ProcessProbe,
    ) -> Result<(WakeRecord, bool)> {
        let key = clean_field(key);
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        let queued = match read_bounded_regular(&self.queue, MAX_QUEUE_BYTES) {
            Ok(bytes) => parse_queue_bytes(&bytes)?,
            Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Vec::new()
            }
            Err(error) => return Err(error),
        };
        if let Some(record) = queued
            .into_iter()
            .find(|record| record.kind == kind && record.key == key)
        {
            if record.payload != clean_field(payload) {
                return Err(CoreError::MalformedRecord {
                    kind: "wake idempotency",
                    reason: "stable kind/key conflicts with a different payload",
                });
            }
            return Ok((record, false));
        }
        if let Some(item) = self
            .scan_items_locked()?
            .into_iter()
            .map(|(_, item)| item)
            .find(|item| item.record.kind == kind && item.record.key == key)
        {
            if item.record.payload != clean_field(payload) {
                return Err(CoreError::MalformedRecord {
                    kind: "wake idempotency",
                    reason: "stable kind/key conflicts with a different payload",
                });
            }
            return Ok((item.record, false));
        }
        let sequence = self.next_sequence_locked()?;
        atomic_replace(&self.sequence, format!("{sequence}\n").as_bytes(), 0o600)?;
        let record = WakeRecord::new(
            now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            sequence,
            kind,
            &key,
            payload,
        );
        append_single_write(&self.queue, record.render().as_bytes(), 0o600)?;
        Ok((record, true))
    }

    /// Restore abandoned pre-publication drains left by a killed process.
    ///
    /// A drain filename is identity-local only while its numeric PID is alive.
    /// Dead owners cannot complete publication, so their older bytes are
    /// restored ahead of any rows appended after queue rotation.
    pub fn recover_abandoned_drains(&self, processes: &impl ProcessProbe) -> Result<usize> {
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        let mut abandoned = fs::read_dir(&self.state)
            .map_err(|error| CoreError::io("scan abandoned wake drains", &self.state, error))?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".wake-queue.drain."))
            })
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.rsplit('.').next())
                    .and_then(|pid| pid.parse::<u32>().ok())
                    .is_none_or(|pid| !processes.is_alive(pid))
            })
            .collect::<Vec<_>>();
        abandoned.sort();
        if abandoned.is_empty() {
            return Ok(0);
        }
        let mut restored = Vec::new();
        for path in &abandoned {
            restored.extend(read_bounded_regular(path, MAX_QUEUE_BYTES)?);
            if restored.len() > MAX_QUEUE_BYTES {
                return Err(CoreError::RecordTooLarge {
                    kind: "restored wake queue",
                    limit: MAX_QUEUE_BYTES,
                });
            }
        }
        if self.queue.exists() {
            restored.extend(read_bounded_regular(&self.queue, MAX_QUEUE_BYTES)?);
        }
        atomic_replace(&self.queue, &restored, 0o600)?;
        for path in &abandoned {
            fs::remove_file(path)
                .map_err(|error| CoreError::io("remove abandoned wake drain", path, error))?;
        }
        Ok(abandoned.len())
    }

    /// Transactionally drain, publish under lock, and restore on publication
    /// failure. The callback is the print-before-delete boundary.
    pub fn drain_with_publish<F>(
        &self,
        processes: &impl ProcessProbe,
        mut publish: F,
    ) -> Result<Vec<WakeRecord>>
    where
        F: FnMut(&[WakeRecord]) -> Result<()>,
    {
        let _lock = DirectoryLock::acquire_wait(&self.lock, processes, Duration::from_secs(5))?;
        let bytes = match read_bounded_regular(&self.queue, MAX_QUEUE_BYTES) {
            Ok(bytes) => bytes,
            Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                atomic_replace(&self.queue, b"", 0o600)?;
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        let drain = self
            .state
            .join(format!(".wake-queue.drain.{}", std::process::id()));
        fs::rename(&self.queue, &drain)
            .map_err(|error| CoreError::io("rename wake queue for drain", &self.queue, error))?;
        if let Err(error) = atomic_replace(&self.queue, b"", 0o600) {
            let _ = fs::rename(&drain, &self.queue);
            return Err(error);
        }
        let parsed = parse_queue_bytes(&bytes)?;
        let records = dedupe(&parsed);
        if let Err(error) = publish(&records) {
            self.restore_locked(&drain)?;
            return Err(error);
        }
        fs::remove_file(&drain)
            .map_err(|error| CoreError::io("remove drained wake queue", &drain, error))?;
        Ok(records)
    }

    fn restore_locked(&self, drained: &Path) -> Result<()> {
        let mut restored = read_bounded_regular(drained, MAX_QUEUE_BYTES)?;
        if self.queue.exists() {
            restored.extend(read_bounded_regular(&self.queue, MAX_QUEUE_BYTES)?);
        }
        atomic_replace(&self.queue, &restored, 0o600)?;
        fs::remove_file(drained)
            .map_err(|error| CoreError::io("remove restored drain file", drained, error))
    }
}

fn parse_queue_bytes(bytes: &[u8]) -> Result<Vec<WakeRecord>> {
    let text = std::str::from_utf8(bytes).map_err(|_| CoreError::MalformedRecord {
        kind: "wake queue",
        reason: "queue is not UTF-8",
    })?;
    text.lines().map(WakeRecord::parse).collect()
}

/// Positive watcher health observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatcherHealth {
    /// Verified watcher PID.
    pub pid: u32,
}

/// Verify PID, portable identity, home, executable path, and beacon freshness.
pub fn watcher_healthy(
    state: &Path,
    watcher_path: &Path,
    home: &Path,
    grace: Duration,
    now: SystemTime,
    processes: &impl ProcessProbe,
) -> Result<Option<WatcherHealth>> {
    let lock = state.join(".watch.lock");
    let owner =
        if fs::symlink_metadata(&lock).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            let target = fs::read_link(&lock)
                .map_err(|error| CoreError::io("read watcher lock", &lock, error))?;
            if target.is_absolute() {
                target
            } else {
                state.join(target)
            }
        } else {
            lock
        };
    let read_text = |name: &str| -> Option<String> {
        let bytes = read_bounded_regular(owner.join(name), 64 * 1024).ok()?;
        Some(
            String::from_utf8(bytes)
                .ok()?
                .trim_end_matches('\n')
                .to_owned(),
        )
    };
    let Some(pid) = read_text("pid").and_then(|text| text.parse::<u32>().ok()) else {
        return Ok(None);
    };
    if !processes.is_alive(pid)
        || read_text("mx-home").as_deref() != Some(&home.to_string_lossy())
        || read_text("watcher-path").as_deref() != Some(&watcher_path.to_string_lossy())
    {
        return Ok(None);
    }
    let recorded = read_text("pid-identity").ok_or(CoreError::MalformedRecord {
        kind: "watcher identity",
        reason: "missing PID identity",
    })?;
    let current = processes.identity(pid)?;
    if current.marker != recorded {
        return Ok(None);
    }
    let beat = fs::metadata(state.join(".last-watcher-beat"))
        .and_then(|metadata| metadata.modified())
        .ok();
    let fresh = beat
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age < grace);
    Ok(fresh.then_some(WatcherHealth { pid }))
}

/// Render the exact persisted identity marker for a lock owner.
#[must_use]
pub fn render_identity(identity: &ProcessIdentity) -> String {
    format!("{}\n", identity.marker)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{
        AnnotationLimits, INBOX_SCHEMA, WakeDisposition, WakeDispositionKind, WakeInboxItem,
        WakeKind, WakeQueue, WakeRecord, dedupe, latest_event, render_annotations, render_identity,
        status_key_map, watcher_healthy,
    };
    use crate::error::{CoreError, Result};
    use crate::process::{AncestryRow, ProcessIdentity, ProcessProbe};

    #[derive(Clone, Default)]
    struct FakeProcesses(Arc<Mutex<HashMap<u32, bool>>>);

    impl ProcessProbe for FakeProcesses {
        fn is_alive(&self, pid: u32) -> bool {
            pid == std::process::id()
                || self
                    .0
                    .lock()
                    .expect("processes")
                    .get(&pid)
                    .copied()
                    .unwrap_or(false)
        }

        fn identity(&self, pid: u32) -> Result<ProcessIdentity> {
            Ok(ProcessIdentity {
                pid,
                marker: format!("fixture-{pid}"),
            })
        }

        fn ancestry_row(&self, pid: u32) -> Result<AncestryRow> {
            Err(CoreError::InvalidIdentifier {
                kind: "fixture PID",
                value: pid.to_string(),
            })
        }
    }

    #[test]
    fn record_round_trip_and_dedupe_preserve_first_key_order() {
        let records = vec![
            WakeRecord::new(1, 1, WakeKind::Signal, "a.status", "first"),
            WakeRecord::new(2, 2, WakeKind::Stale, "pane", "stale"),
            WakeRecord::new(3, 3, WakeKind::Signal, "a.status", "latest"),
        ];
        let rendered = records[0].render();
        assert_eq!(
            WakeRecord::parse(rendered.trim_end()).expect("parse"),
            records[0]
        );
        let deduped = dedupe(&records);
        assert_eq!(deduped[0].payload, "latest");
        assert_eq!(deduped[1].payload, "stale");
    }

    #[test]
    fn record_round_trip_holds_across_field_and_kind_matrix() {
        let fields = ["", "ascii", "tab\tvalue", "line\nvalue", "unicode-❯"];
        let kinds = [
            WakeKind::Signal,
            WakeKind::Stale,
            WakeKind::Check,
            WakeKind::Heartbeat,
        ];
        for (index, key) in fields.iter().enumerate() {
            for (kind_index, kind) in kinds.iter().enumerate() {
                for payload in fields {
                    let record =
                        WakeRecord::new(index as u64, kind_index as u64, *kind, key, payload);
                    assert_eq!(
                        WakeRecord::parse(record.render().trim_end_matches('\n'))
                            .expect("round trip"),
                        record
                    );
                }
            }
        }
    }

    #[test]
    fn status_mapping_rejects_traversal_and_marks_history() {
        assert!(!status_key_map("task.status").expect("direct").historical);
        assert!(
            status_key_map("task.turn-ended")
                .expect("history")
                .historical
        );
        assert!(status_key_map("../task.status").is_err());
    }

    #[test]
    fn concurrent_appends_commit_every_sequence_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = Arc::new(WakeQueue::new(temp.path()));
        let processes = FakeProcesses::default();
        let barrier = Arc::new(Barrier::new(12));
        thread::scope(|scope| {
            for index in 0..12 {
                let queue = Arc::clone(&queue);
                let processes = processes.clone();
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    queue
                        .append(
                            WakeKind::Signal,
                            &format!("task-{index}.status"),
                            "signal",
                            UNIX_EPOCH + Duration::from_secs(1000),
                            &processes,
                        )
                        .expect("append");
                });
            }
        });
        let text = fs::read_to_string(temp.path().join(".wake-queue")).expect("queue");
        assert_eq!(text.lines().count(), 12);
        let mut sequences = text
            .lines()
            .map(|line| WakeRecord::parse(line).expect("record").sequence)
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=12).collect::<Vec<_>>());
    }

    #[test]
    fn failed_drain_publication_restores_old_rows_before_new_rows() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        queue
            .append(
                WakeKind::Signal,
                "task.status",
                "first",
                UNIX_EPOCH + Duration::from_secs(1),
                &processes,
            )
            .expect("append");
        let result = queue.drain_with_publish(&processes, |_| {
            Err(CoreError::Command {
                command: "fixture publisher".to_owned(),
                reason: "injected failure".to_owned(),
            })
        });
        assert!(result.is_err());
        let text = fs::read_to_string(temp.path().join(".wake-queue")).expect("restored queue");
        assert_eq!(text.lines().count(), 1);
        assert_eq!(
            WakeRecord::parse(text.trim_end()).expect("record").payload,
            "first"
        );
    }

    #[test]
    fn abandoned_drain_recovery_preserves_old_before_new_and_skips_live_owner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        let old = WakeRecord::new(1, 1, WakeKind::Signal, "old.status", "old");
        let new = WakeRecord::new(2, 2, WakeKind::Signal, "new.status", "new");
        let live = WakeRecord::new(3, 3, WakeKind::Signal, "live.status", "live");
        fs::write(temp.path().join(".wake-queue.drain.424242"), old.render())
            .expect("abandoned drain");
        fs::write(temp.path().join(".wake-queue"), new.render()).expect("queue");
        fs::write(temp.path().join(".wake-queue.drain.515151"), live.render()).expect("live drain");
        processes.0.lock().expect("processes").insert(515151, true);

        assert_eq!(
            queue.recover_abandoned_drains(&processes).expect("recover"),
            1
        );
        let restored = fs::read_to_string(temp.path().join(".wake-queue")).expect("restored");
        let payloads = restored
            .lines()
            .map(|line| WakeRecord::parse(line).expect("record").payload)
            .collect::<Vec<_>>();
        assert_eq!(payloads, ["old", "new"]);
        assert!(!temp.path().join(".wake-queue.drain.424242").exists());
        assert!(temp.path().join(".wake-queue.drain.515151").exists());
    }

    #[test]
    fn durable_claim_survives_display_crash_and_recovers_only_after_owner_death() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        queue
            .append(
                WakeKind::Signal,
                "task.status",
                "dependent action",
                UNIX_EPOCH + Duration::from_secs(1),
                &processes,
            )
            .expect("append");
        let first = ProcessIdentity {
            pid: 424242,
            marker: "fixture-424242".into(),
        };
        let replacement = ProcessIdentity {
            pid: 515151,
            marker: "fixture-515151".into(),
        };
        {
            let mut live = processes.0.lock().expect("processes");
            live.insert(first.pid, true);
            live.insert(replacement.pid, true);
        }
        let displayed = queue
            .claim_available(&first, UNIX_EPOCH + Duration::from_secs(2), 8, &processes)
            .expect("claim before display");
        assert_eq!(displayed.len(), 1);
        assert!(
            queue
                .claim_available(&first, UNIX_EPOCH + Duration::from_secs(2), 8, &processes)
                .expect("same owner does not redeliver")
                .is_empty()
        );
        assert_eq!(queue.unfinished_count(&processes).expect("unfinished"), 1);
        assert!(
            queue
                .claim_available(
                    &replacement,
                    UNIX_EPOCH + Duration::from_secs(3),
                    8,
                    &processes
                )
                .expect("live owner retained")
                .is_empty()
        );
        processes
            .0
            .lock()
            .expect("processes")
            .insert(first.pid, false);
        assert_eq!(
            queue.recover_abandoned_claims(&processes).expect("recover"),
            1
        );
        let reclaimed = queue
            .claim_available(
                &replacement,
                UNIX_EPOCH + Duration::from_secs(4),
                8,
                &processes,
            )
            .expect("replacement claim");
        assert_eq!(reclaimed[0].event_id, displayed[0].event_id);
    }

    #[test]
    fn read_only_observation_creates_nothing_and_counts_queue_plus_waiting_inbox() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("absent-state");
        fs::create_dir(&state).expect("state");
        let queue = WakeQueue::new(&state);
        assert_eq!(queue.observe_unfinished_count().expect("empty"), 0);
        assert!(!state.join("wake-inbox").exists());
        assert!(!state.join(".wake-queue.lock").exists());
        fs::write(
            state.join(".wake-queue"),
            WakeRecord::new(1, 1, WakeKind::Signal, "task.status", "first").render(),
        )
        .expect("queue");
        assert_eq!(queue.observe_unfinished_count().expect("queued"), 1);
        assert!(!state.join("wake-inbox").exists());
    }

    #[test]
    fn inbox_coalesces_only_heartbeat_and_append_once_rejects_conflicting_payload() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        let owner = ProcessIdentity {
            pid: 424242,
            marker: "fixture-424242".into(),
        };
        processes
            .0
            .lock()
            .expect("processes")
            .insert(owner.pid, true);
        for (kind, payload) in [
            (WakeKind::Heartbeat, "old heartbeat"),
            (WakeKind::Signal, "transition one"),
            (WakeKind::Heartbeat, "new heartbeat"),
            (WakeKind::Signal, "transition two"),
        ] {
            queue
                .append(kind, "same", payload, SystemTime::now(), &processes)
                .expect("append");
        }
        let claimed = queue
            .claim_available(&owner, SystemTime::now(), 8, &processes)
            .expect("claim");
        assert_eq!(claimed.len(), 3);
        assert_eq!(
            claimed
                .iter()
                .filter(|item| item.record.kind == WakeKind::Signal)
                .count(),
            2
        );
        assert!(
            claimed
                .iter()
                .any(|item| item.record.payload == "new heartbeat")
        );

        let separate = tempfile::tempdir().expect("separate");
        let once = WakeQueue::new(separate.path());
        let (record, fresh) = once
            .append_once(
                WakeKind::Check,
                "operation-1",
                "result",
                SystemTime::now(),
                &processes,
            )
            .expect("first once");
        assert!(fresh);
        assert_eq!(
            once.append_once(
                WakeKind::Check,
                "operation-1",
                "result",
                SystemTime::now(),
                &processes
            )
            .expect("repeat once"),
            (record, false)
        );
        assert!(
            once.append_once(
                WakeKind::Check,
                "operation-1",
                "different",
                SystemTime::now(),
                &processes
            )
            .is_err()
        );
    }

    #[test]
    fn malformed_sequence_never_resets_and_missing_sequence_reconciles_inbox_maximum() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        fs::write(temp.path().join(".wake-queue.seq"), "not-a-number\n").expect("bad sequence");
        assert!(
            queue
                .append(
                    WakeKind::Signal,
                    "task",
                    "payload",
                    SystemTime::now(),
                    &processes
                )
                .is_err()
        );
        assert!(!temp.path().join(".wake-queue").exists());

        fs::remove_file(temp.path().join(".wake-queue.seq")).expect("remove counter");
        fs::create_dir(temp.path().join("wake-inbox")).expect("inbox");
        let prior = WakeInboxItem {
            schema: INBOX_SCHEMA.into(),
            event_id: WakeQueue::event_id(9),
            record: WakeRecord::new(1, 9, WakeKind::Check, "prior", "prior"),
            claim: None,
            disposition: None,
            disposition_history: Vec::new(),
            acknowledged_at: None,
        };
        queue.write_item(&prior).expect("prior inbox");
        let appended = queue
            .append(
                WakeKind::Signal,
                "next",
                "next",
                SystemTime::now(),
                &processes,
            )
            .expect("reconciled append");
        assert_eq!(appended.sequence, 10);
    }

    #[test]
    fn acknowledgement_requires_disposition_and_waiting_retains_a_resumable_trigger() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        let owner = ProcessIdentity {
            pid: 424242,
            marker: "fixture-424242".into(),
        };
        processes
            .0
            .lock()
            .expect("processes")
            .insert(owner.pid, true);
        queue
            .append(WakeKind::Check, "task", "blocked", UNIX_EPOCH, &processes)
            .expect("append");
        let item = queue
            .claim_available(&owner, UNIX_EPOCH, 1, &processes)
            .expect("claim")
            .remove(0);
        assert!(
            queue
                .acknowledge(&item.event_id, &owner, UNIX_EPOCH, &processes)
                .is_err()
        );
        let invalid = WakeDisposition {
            kind: WakeDispositionKind::Waiting,
            recorded_at: 1,
            detail: "blocked by review".into(),
            condition: Some("review complete".into()),
            resume_trigger: None,
            recheck_after_epoch: None,
            follow_up_id: None,
        };
        assert!(
            queue
                .record_disposition(&item.event_id, &owner, invalid, &processes)
                .is_err()
        );
        let waiting = WakeDisposition {
            kind: WakeDispositionKind::Waiting,
            recorded_at: 2,
            detail: "blocked by review".into(),
            condition: Some("review complete".into()),
            resume_trigger: Some("review-42".into()),
            recheck_after_epoch: Some(10),
            follow_up_id: None,
        };
        queue
            .record_disposition(&item.event_id, &owner, waiting.clone(), &processes)
            .expect("disposition");
        queue
            .record_disposition(&item.event_id, &owner, waiting, &processes)
            .expect("repeat-safe disposition");
        queue
            .acknowledge(
                &item.event_id,
                &owner,
                UNIX_EPOCH + Duration::from_secs(3),
                &processes,
            )
            .expect("acknowledge");
        assert_eq!(queue.unfinished_count(&processes).expect("waiting"), 1);
        assert!(
            queue
                .resume_waiting(
                    &item.event_id,
                    "wrong",
                    &owner,
                    UNIX_EPOCH + Duration::from_secs(4),
                    &processes
                )
                .is_err()
        );
        let resumed = queue
            .resume_waiting(
                &item.event_id,
                "review-42",
                &owner,
                UNIX_EPOCH + Duration::from_secs(4),
                &processes,
            )
            .expect("resume");
        assert!(resumed.disposition.is_none());
        assert!(resumed.acknowledged_at.is_none());
        queue
            .record_disposition(
                &item.event_id,
                &owner,
                WakeDisposition {
                    kind: WakeDispositionKind::Waiting,
                    recorded_at: 5,
                    detail: "periodic check".into(),
                    condition: Some("timer".into()),
                    resume_trigger: None,
                    recheck_after_epoch: Some(6),
                    follow_up_id: None,
                },
                &processes,
            )
            .expect("second wait");
        queue
            .acknowledge(
                &item.event_id,
                &owner,
                UNIX_EPOCH + Duration::from_secs(5),
                &processes,
            )
            .expect("second ack");
        assert_eq!(
            queue
                .resume_due_waiting(&owner, UNIX_EPOCH + Duration::from_secs(5), &processes)
                .expect("not due"),
            0
        );
        assert_eq!(
            queue
                .resume_due_waiting(&owner, UNIX_EPOCH + Duration::from_secs(6), &processes)
                .expect("due"),
            1
        );
        let reclaimed = queue
            .claim_available(&owner, UNIX_EPOCH + Duration::from_secs(6), 1, &processes)
            .expect("reclaimed after due");
        assert_eq!(reclaimed[0].disposition_history.len(), 2);
    }

    #[test]
    fn repeated_queue_ingestion_and_concurrent_claims_do_not_duplicate_ownership() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = Arc::new(WakeQueue::new(temp.path()));
        let processes = FakeProcesses::default();
        let record = queue
            .append(
                WakeKind::Signal,
                "task.status",
                "done",
                UNIX_EPOCH,
                &processes,
            )
            .expect("append");
        let owners = (100..108)
            .map(|pid| ProcessIdentity {
                pid,
                marker: format!("fixture-{pid}"),
            })
            .collect::<Vec<_>>();
        for owner in &owners {
            processes
                .0
                .lock()
                .expect("processes")
                .insert(owner.pid, true);
        }
        let barrier = Arc::new(Barrier::new(owners.len()));
        let claimed = Arc::new(Mutex::new(Vec::new()));
        thread::scope(|scope| {
            for owner in owners {
                let queue = Arc::clone(&queue);
                let processes = processes.clone();
                let barrier = Arc::clone(&barrier);
                let claimed = Arc::clone(&claimed);
                scope.spawn(move || {
                    barrier.wait();
                    let result = queue
                        .claim_available(&owner, UNIX_EPOCH, 1, &processes)
                        .expect("claim");
                    claimed.lock().expect("claimed").extend(result);
                });
            }
        });
        assert_eq!(claimed.lock().expect("claimed").len(), 1);

        fs::write(temp.path().join(".wake-queue"), record.render()).expect("repeat source");
        let items = queue.inbox_items(&processes).expect("items before repeat");
        assert_eq!(items.len(), 1);
        assert_eq!(queue.unfinished_count(&processes).expect("reingest"), 1);
        assert_eq!(
            queue
                .inbox_items(&processes)
                .expect("items after repeat")
                .len(),
            1
        );
    }

    #[test]
    fn watcher_health_rejects_reused_pid_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        let owner = state.join(".watch.lock");
        let home = state.join("home");
        let watcher = state.join("bin/mx-watch.sh");
        let pid = 4142;
        fs::create_dir(&owner).expect("owner");
        fs::write(owner.join("pid"), format!("{pid}\n")).expect("pid");
        fs::write(owner.join("mx-home"), format!("{}\n", home.display())).expect("home");
        fs::write(
            owner.join("watcher-path"),
            format!("{}\n", watcher.display()),
        )
        .expect("watcher path");
        fs::write(owner.join("pid-identity"), format!("fixture-{pid}\n")).expect("identity");
        fs::write(state.join(".last-watcher-beat"), b"").expect("beat");
        let processes = FakeProcesses::default();
        processes.0.lock().expect("processes").insert(pid, true);
        assert_eq!(
            watcher_healthy(
                state,
                &watcher,
                &home,
                Duration::from_secs(300),
                SystemTime::now(),
                &processes,
            )
            .expect("healthy")
            .expect("watcher")
            .pid,
            pid
        );
        fs::write(owner.join("pid-identity"), b"fixture-old-generation\n").expect("old identity");
        assert!(
            watcher_healthy(
                state,
                &watcher,
                &home,
                Duration::from_secs(300),
                SystemTime::now(),
                &processes,
            )
            .expect("PID reuse is an ordinary mismatch")
            .is_none()
        );
    }

    #[test]
    fn malformed_records_and_status_annotations_fail_closed() {
        assert!(WakeRecord::parse(&"x".repeat(super::MAX_RECORD_BYTES + 1)).is_err());
        for line in [
            "one\ttwo",
            "x\t1\tsignal\tkey\tpayload",
            "1\tx\tsignal\tkey\tpayload",
            "1\t1\tunknown\tkey\tpayload",
        ] {
            assert!(WakeRecord::parse(line).is_err(), "{line}");
        }
        assert!(status_key_map("task.other").is_err());

        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("status");
        fs::write(&path, b"prefix\nlatest\trow\r\n").expect("status");
        let event = latest_event(&path, 8).expect("event");
        assert_eq!(event.line, "st row");
        assert!(event.truncated);
        fs::write(&path, b"\n\n").expect("blank");
        assert!(latest_event(&path, 32).is_err());
        fs::remove_file(&path).expect("remove");
        fs::create_dir(&path).expect("directory");
        assert!(latest_event(&path, 32).is_err());
    }

    #[test]
    fn annotation_limits_cover_history_truncation_and_both_omission_caps() {
        let temp = tempfile::tempdir().expect("tempdir");
        for id in ["a", "b", "c"] {
            fs::write(
                temp.path().join(format!("{id}.status")),
                format!("{id}: {}\n", "x".repeat(80)),
            )
            .expect("status");
        }
        let records = vec![
            WakeRecord::new(1, 1, WakeKind::Signal, "a.turn-ended", "old"),
            WakeRecord::new(2, 2, WakeKind::Signal, "a.status", "new"),
            WakeRecord::new(3, 3, WakeKind::Signal, "b.status", "new"),
            WakeRecord::new(4, 4, WakeKind::Signal, "c.status", "new"),
            WakeRecord::new(5, 5, WakeKind::Signal, "bad/key.status", "ignored"),
        ];
        let output = render_annotations(
            temp.path(),
            &records,
            AnnotationLimits {
                tail_bytes: 32,
                item_bytes: 96,
                global_bytes: 300,
                read_cap: 2,
            },
        );
        assert!(output.contains("a.status"));
        assert!(output.contains("[truncated]"));
        assert!(output.contains("enrichment read cap"));

        let capped = render_annotations(
            temp.path(),
            &records,
            AnnotationLimits {
                tail_bytes: 128,
                item_bytes: 256,
                global_bytes: 193,
                read_cap: 8,
            },
        );
        assert!(capped.contains("global enrichment byte cap"));
    }

    #[test]
    fn queue_empty_success_and_parse_failure_paths_are_transactional() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        assert!(
            queue
                .drain_with_publish(&processes, |_| Ok(()))
                .expect("absent")
                .is_empty()
        );
        assert!(
            queue
                .drain_with_publish(&processes, |_| Ok(()))
                .expect("empty")
                .is_empty()
        );
        queue
            .append(WakeKind::Heartbeat, "one", "first", UNIX_EPOCH, &processes)
            .expect("first");
        queue
            .append(WakeKind::Heartbeat, "two", "latest", UNIX_EPOCH, &processes)
            .expect("second");
        let drained = queue
            .drain_with_publish(&processes, |rows| {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].payload, "latest");
                Ok(())
            })
            .expect("drain");
        assert_eq!(drained.len(), 1);

        fs::write(temp.path().join(".wake-queue"), [0xff]).expect("invalid queue");
        assert!(queue.drain_with_publish(&processes, |_| Ok(())).is_err());
    }

    #[test]
    fn watcher_health_mismatch_matrix_and_identity_rendering_are_observable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        let home = state.join("home");
        let watcher = state.join("watcher");
        let processes = FakeProcesses::default();
        assert!(
            watcher_healthy(
                state,
                &watcher,
                &home,
                Duration::from_secs(1),
                SystemTime::now(),
                &processes
            )
            .expect("missing")
            .is_none()
        );
        let owner = state.join("owner");
        fs::create_dir(&owner).expect("owner");
        std::os::unix::fs::symlink("owner", state.join(".watch.lock")).expect("relative owner");
        fs::write(owner.join("pid"), b"99\n").expect("pid");
        processes.0.lock().expect("processes").insert(99, true);
        assert!(
            watcher_healthy(
                state,
                &watcher,
                &home,
                Duration::from_secs(1),
                SystemTime::now(),
                &processes
            )
            .expect("missing home")
            .is_none()
        );
        fs::write(owner.join("mx-home"), format!("{}\n", home.display())).expect("home");
        fs::write(
            owner.join("watcher-path"),
            format!("{}\n", watcher.display()),
        )
        .expect("watcher");
        assert!(
            watcher_healthy(
                state,
                &watcher,
                &home,
                Duration::from_secs(1),
                SystemTime::now(),
                &processes
            )
            .is_err()
        );
        fs::write(owner.join("pid-identity"), b"fixture-99\n").expect("identity");
        assert!(
            watcher_healthy(
                state,
                &watcher,
                &home,
                Duration::ZERO,
                SystemTime::now(),
                &processes
            )
            .expect("stale")
            .is_none()
        );
        assert_eq!(
            render_identity(&ProcessIdentity {
                pid: 99,
                marker: "fixture-99".to_owned()
            }),
            "fixture-99\n"
        );
    }

    #[test]
    fn disposition_validation_and_corrupt_inbox_receipts_fail_closed() {
        assert!(WakeDispositionKind::parse("unknown").is_err());
        for (kind, token) in [
            (WakeDispositionKind::Handled, "handled"),
            (WakeDispositionKind::Superseded, "superseded"),
            (WakeDispositionKind::Waiting, "waiting"),
            (WakeDispositionKind::FollowUp, "follow-up"),
        ] {
            assert_eq!(kind.as_str(), token);
        }
        let disposition = |kind| WakeDisposition {
            kind,
            recorded_at: 1,
            detail: "handled".into(),
            condition: None,
            resume_trigger: None,
            recheck_after_epoch: None,
            follow_up_id: None,
        };
        let mut invalid = disposition(WakeDispositionKind::Handled);
        invalid.detail.clear();
        assert!(invalid.validate().is_err());
        let mut invalid = disposition(WakeDispositionKind::Waiting);
        invalid.condition = Some("condition".into());
        assert!(invalid.validate().is_err());
        let invalid = disposition(WakeDispositionKind::FollowUp);
        assert!(invalid.validate().is_err());
        let mut invalid = disposition(WakeDispositionKind::Superseded);
        invalid.resume_trigger = Some("unexpected".into());
        assert!(invalid.validate().is_err());

        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        assert!(
            queue
                .acknowledge(
                    "bad-event",
                    &ProcessIdentity {
                        pid: 424_242,
                        marker: "fixture-424242".into(),
                    },
                    SystemTime::now(),
                    &processes,
                )
                .is_err()
        );
        fs::write(temp.path().join("wake-inbox"), b"not a directory").expect("inbox file");
        assert!(queue.inbox_items(&processes).is_err());

        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        fs::create_dir(temp.path().join("wake-inbox")).expect("inbox");
        fs::write(
            temp.path()
                .join("wake-inbox/wake-00000000000000000001.json"),
            b"not json",
        )
        .expect("corrupt receipt");
        assert!(queue.inbox_items(&processes).is_err());

        let record = WakeRecord::new(1, 1, WakeKind::Signal, "task.status", "work");
        let mut item = WakeInboxItem {
            schema: INBOX_SCHEMA.into(),
            event_id: "wake-00000000000000000001".into(),
            record,
            claim: None,
            disposition: None,
            disposition_history: Vec::new(),
            acknowledged_at: Some(2),
        };
        fs::write(
            temp.path()
                .join("wake-inbox/wake-00000000000000000001.json"),
            serde_json::to_vec(&item).expect("encode"),
        )
        .expect("receipt");
        assert!(queue.inbox_items(&processes).is_err());
        item.acknowledged_at = None;
        item.schema = "wrong-schema".into();
        fs::write(
            temp.path()
                .join("wake-inbox/wake-00000000000000000001.json"),
            serde_json::to_vec(&item).expect("encode"),
        )
        .expect("receipt");
        assert!(queue.inbox_items(&processes).is_err());
    }

    #[test]
    fn sequence_corruption_and_ambiguous_owner_never_create_work() {
        #[derive(Clone, Copy)]
        struct AmbiguousOwner;
        impl ProcessProbe for AmbiguousOwner {
            fn is_alive(&self, _pid: u32) -> bool {
                true
            }

            fn identity(&self, pid: u32) -> Result<ProcessIdentity> {
                Err(CoreError::InvalidIdentifier {
                    kind: "ambiguous PID",
                    value: pid.to_string(),
                })
            }

            fn ancestry_row(&self, pid: u32) -> Result<AncestryRow> {
                Err(CoreError::InvalidIdentifier {
                    kind: "fixture PID",
                    value: pid.to_string(),
                })
            }
        }

        let owner = ProcessIdentity {
            pid: 424_242,
            marker: "fixture-424242".into(),
        };
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        assert!(
            queue
                .claim_available(&owner, SystemTime::now(), 1, &AmbiguousOwner)
                .is_err()
        );
        assert!(!temp.path().join("wake-inbox").exists());

        let processes = FakeProcesses::default();
        processes
            .0
            .lock()
            .expect("processes")
            .insert(owner.pid, true);
        assert!(
            queue
                .acknowledge("bad-event", &owner, SystemTime::now(), &processes)
                .is_err()
        );
        fs::write(temp.path().join(".wake-queue.seq"), [0xff]).expect("non-UTF-8 counter");
        assert!(
            queue
                .append(
                    WakeKind::Signal,
                    "task.status",
                    "work",
                    SystemTime::now(),
                    &processes,
                )
                .is_err()
        );
        fs::write(
            temp.path().join(".wake-queue.seq"),
            format!("{}\n", u64::MAX),
        )
        .expect("overflow counter");
        assert!(
            queue
                .append(
                    WakeKind::Signal,
                    "task.status",
                    "work",
                    SystemTime::now(),
                    &processes,
                )
                .is_err()
        );

        let other = tempfile::tempdir().expect("other");
        fs::create_dir(other.path().join(".wake-queue")).expect("queue directory");
        assert!(
            WakeQueue::new(other.path())
                .observe_unfinished_count()
                .is_err()
        );
        assert!(
            WakeQueue::new(other.path())
                .drain_with_publish(&processes, |_| Ok(()))
                .is_err()
        );
    }

    #[test]
    fn claim_disposition_ack_and_waiting_fence_competing_owners() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        let first = ProcessIdentity {
            pid: 424_242,
            marker: "fixture-424242".into(),
        };
        let second = ProcessIdentity {
            pid: 515_151,
            marker: "fixture-515151".into(),
        };
        {
            let mut live = processes.0.lock().expect("processes");
            live.insert(first.pid, true);
            live.insert(second.pid, true);
        }
        queue
            .append(
                WakeKind::Signal,
                "one.status",
                "work",
                UNIX_EPOCH + Duration::from_secs(1),
                &processes,
            )
            .expect("append");
        let claimed = queue
            .claim_available(&first, UNIX_EPOCH + Duration::from_secs(2), 1, &processes)
            .expect("claim");
        let event = &claimed[0].event_id;
        let handled = WakeDisposition {
            kind: WakeDispositionKind::Handled,
            recorded_at: 3,
            detail: "done".into(),
            condition: None,
            resume_trigger: None,
            recheck_after_epoch: None,
            follow_up_id: None,
        };
        assert!(
            queue
                .record_disposition(event, &second, handled.clone(), &processes)
                .is_err()
        );
        assert!(
            queue
                .acknowledge(event, &first, SystemTime::now(), &processes)
                .is_err()
        );
        queue
            .record_disposition(event, &first, handled.clone(), &processes)
            .expect("disposition");
        queue
            .record_disposition(event, &first, handled, &processes)
            .expect("idempotent disposition");
        let mut conflict = disposition_for_test(WakeDispositionKind::Handled, "different");
        conflict.recorded_at = 99;
        assert!(
            queue
                .record_disposition(event, &first, conflict, &processes)
                .is_err()
        );
        queue
            .acknowledge(
                event,
                &first,
                UNIX_EPOCH + Duration::from_secs(4),
                &processes,
            )
            .expect("ack");
        queue
            .acknowledge(
                event,
                &first,
                UNIX_EPOCH + Duration::from_secs(5),
                &processes,
            )
            .expect("idempotent ack");
        assert!(
            queue
                .resume_waiting(event, "anything", &first, SystemTime::now(), &processes)
                .is_err()
        );

        queue
            .append(
                WakeKind::Signal,
                "two.status",
                "wait",
                UNIX_EPOCH + Duration::from_secs(6),
                &processes,
            )
            .expect("append waiting");
        let claimed = queue
            .claim_available(&first, UNIX_EPOCH + Duration::from_secs(7), 1, &processes)
            .expect("claim waiting");
        let waiting_event = &claimed[0].event_id;
        let waiting = WakeDisposition {
            kind: WakeDispositionKind::Waiting,
            recorded_at: 8,
            detail: "blocked".into(),
            condition: Some("review complete".into()),
            resume_trigger: Some("review-ready".into()),
            recheck_after_epoch: Some(20),
            follow_up_id: None,
        };
        queue
            .record_disposition(waiting_event, &first, waiting, &processes)
            .expect("waiting disposition");
        queue
            .acknowledge(
                waiting_event,
                &first,
                UNIX_EPOCH + Duration::from_secs(9),
                &processes,
            )
            .expect("waiting ack");
        assert!(
            queue
                .resume_waiting(
                    waiting_event,
                    "wrong-trigger",
                    &first,
                    UNIX_EPOCH + Duration::from_secs(10),
                    &processes,
                )
                .is_err()
        );
        let resumed = queue
            .resume_waiting(
                waiting_event,
                "review-ready",
                &first,
                UNIX_EPOCH + Duration::from_secs(11),
                &processes,
            )
            .expect("resume");
        assert_eq!(resumed.disposition_history.len(), 1);
        assert!(resumed.disposition.is_none());
        assert!(resumed.acknowledged_at.is_none());

        queue
            .append(
                WakeKind::Signal,
                "three.status",
                "periodic wait",
                UNIX_EPOCH + Duration::from_secs(12),
                &processes,
            )
            .expect("append due waiting");
        let claimed = queue
            .claim_available(&first, UNIX_EPOCH + Duration::from_secs(13), 1, &processes)
            .expect("claim due waiting");
        let due_event = &claimed[0].event_id;
        queue
            .record_disposition(
                due_event,
                &first,
                WakeDisposition {
                    kind: WakeDispositionKind::Waiting,
                    recorded_at: 14,
                    detail: "retry later".into(),
                    condition: Some("timer".into()),
                    resume_trigger: None,
                    recheck_after_epoch: Some(15),
                    follow_up_id: None,
                },
                &processes,
            )
            .expect("due disposition");
        queue
            .acknowledge(
                due_event,
                &first,
                UNIX_EPOCH + Duration::from_secs(14),
                &processes,
            )
            .expect("due acknowledgement");
        assert_eq!(
            queue
                .resume_due_waiting(&second, UNIX_EPOCH + Duration::from_secs(16), &processes,)
                .expect("live claim remains fenced"),
            0
        );
        processes
            .0
            .lock()
            .expect("processes")
            .insert(first.pid, false);
        assert_eq!(
            queue
                .resume_due_waiting(&second, UNIX_EPOCH + Duration::from_secs(16), &processes,)
                .expect("dead claim resumed"),
            1
        );
    }

    #[test]
    fn persisted_event_identity_rejects_conflicting_republication_and_foreign_ack() {
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = WakeQueue::new(temp.path());
        let processes = FakeProcesses::default();
        let owner = ProcessIdentity {
            pid: 424_242,
            marker: "fixture-424242".into(),
        };
        let foreign = ProcessIdentity {
            pid: 515_151,
            marker: "fixture-515151".into(),
        };
        {
            let mut live = processes.0.lock().expect("processes");
            live.insert(owner.pid, true);
            live.insert(foreign.pid, true);
        }
        let (record, _) = queue
            .append_once(
                WakeKind::Signal,
                "stable-operation",
                "accepted payload",
                UNIX_EPOCH + Duration::from_secs(1),
                &processes,
            )
            .expect("publish");
        let claimed = queue
            .claim_available(&owner, UNIX_EPOCH + Duration::from_secs(2), 1, &processes)
            .expect("claim");
        assert_eq!(
            queue
                .append_once(
                    WakeKind::Signal,
                    "stable-operation",
                    "accepted payload",
                    UNIX_EPOCH + Duration::from_secs(3),
                    &processes,
                )
                .expect("idempotent republication after claim"),
            (record.clone(), false)
        );
        assert!(
            queue
                .append_once(
                    WakeKind::Signal,
                    "stable-operation",
                    "conflicting payload",
                    UNIX_EPOCH + Duration::from_secs(3),
                    &processes,
                )
                .is_err()
        );
        let mut conflicting_record = record;
        conflicting_record.payload = "different record for stable sequence".into();
        fs::write(temp.path().join(".wake-queue"), conflicting_record.render())
            .expect("conflicting queued record");
        assert!(
            queue
                .claim_available(&owner, UNIX_EPOCH + Duration::from_secs(4), 1, &processes)
                .is_err()
        );
        fs::write(temp.path().join(".wake-queue"), b"").expect("clear injected conflict");
        queue
            .record_disposition(
                &claimed[0].event_id,
                &owner,
                disposition_for_test(WakeDispositionKind::Handled, "handled safely"),
                &processes,
            )
            .expect("disposition");
        assert!(
            queue
                .acknowledge(
                    &claimed[0].event_id,
                    &foreign,
                    UNIX_EPOCH + Duration::from_secs(5),
                    &processes,
                )
                .is_err()
        );
        queue
            .acknowledge(
                &claimed[0].event_id,
                &owner,
                UNIX_EPOCH + Duration::from_secs(5),
                &processes,
            )
            .expect("owner acknowledgement");
    }

    fn disposition_for_test(kind: WakeDispositionKind, detail: &str) -> WakeDisposition {
        WakeDisposition {
            kind,
            recorded_at: 1,
            detail: detail.into(),
            condition: None,
            resume_trigger: None,
            recheck_after_epoch: None,
            follow_up_id: None,
        }
    }
}

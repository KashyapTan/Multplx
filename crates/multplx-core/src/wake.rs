//! Durable wake records, queue transactions, annotations, and watcher identity
//! from `bin/mx-wake-lib.sh`.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustix::fs::OFlags;

use crate::error::{CoreError, Result};
use crate::filesystem::{append_single_write, atomic_replace, read_bounded_regular};
use crate::locks::DirectoryLock;
use crate::process::{ProcessIdentity, ProcessProbe};

const MAX_QUEUE_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;

/// Closed durable wake kind vocabulary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
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
            state,
        }
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
        let current = match read_bounded_regular(&self.sequence, 64) {
            Ok(bytes) => String::from_utf8(bytes)
                .ok()
                .and_then(|text| text.trim().parse::<u64>().ok())
                .unwrap_or(0),
            Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => 0,
            Err(_) => 0,
        };
        let sequence = current.checked_add(1).ok_or(CoreError::MalformedRecord {
            kind: "wake sequence",
            reason: "sequence overflow",
        })?;
        atomic_replace(&self.sequence, format!("{sequence}\n").as_bytes(), 0o600)?;
        let epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let record = WakeRecord::new(epoch, sequence, kind, key, payload);
        append_single_write(&self.queue, record.render().as_bytes(), 0o600)?;
        Ok(record)
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

    use super::{WakeKind, WakeQueue, WakeRecord, dedupe, status_key_map, watcher_healthy};
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
}

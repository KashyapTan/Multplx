//! Best-effort structured task journal writer from `bin/mx-journal-lib.sh`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use serde_json::Value;

use crate::error::{CoreError, Result};
use crate::filesystem::append_single_write;
use crate::identifiers::TaskId;

/// Closed current event vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalEvent {
    /// Task creation.
    TaskSpawned,
    /// Validated actor report.
    StatusReported,
    /// Reconciled current-state classification.
    StatusClassified,
    /// Deep-review step start.
    GateStepStarted,
    /// Deep-review step finish.
    GateStepFinished,
    /// Decision hold opened.
    HoldOpened,
    /// Decision hold resolved.
    HoldResolved,
    /// Workflow stage entered.
    WorkflowStageEntered,
    /// Workflow stage gated.
    WorkflowStageGated,
    /// Delivery queued.
    DeliveryQueued,
    /// Delivery pushed.
    DeliveryPushed,
    /// Pull request opened.
    DeliveryPrOpened,
}

impl JournalEvent {
    /// Parse the exact shell vocabulary.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "task.spawned" => Ok(Self::TaskSpawned),
            "status.reported" => Ok(Self::StatusReported),
            "status.classified" => Ok(Self::StatusClassified),
            "gate.step.started" => Ok(Self::GateStepStarted),
            "gate.step.finished" => Ok(Self::GateStepFinished),
            "hold.opened" => Ok(Self::HoldOpened),
            "hold.resolved" => Ok(Self::HoldResolved),
            "workflow.stage.entered" => Ok(Self::WorkflowStageEntered),
            "workflow.stage.gated" => Ok(Self::WorkflowStageGated),
            "delivery.queued" => Ok(Self::DeliveryQueued),
            "delivery.pushed" => Ok(Self::DeliveryPushed),
            "delivery.pr_opened" => Ok(Self::DeliveryPrOpened),
            _ => Err(CoreError::UnknownValue {
                kind: "journal event",
                value: value.to_owned(),
            }),
        }
    }

    /// Return the wire token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TaskSpawned => "task.spawned",
            Self::StatusReported => "status.reported",
            Self::StatusClassified => "status.classified",
            Self::GateStepStarted => "gate.step.started",
            Self::GateStepFinished => "gate.step.finished",
            Self::HoldOpened => "hold.opened",
            Self::HoldResolved => "hold.resolved",
            Self::WorkflowStageEntered => "workflow.stage.entered",
            Self::WorkflowStageGated => "workflow.stage.gated",
            Self::DeliveryQueued => "delivery.queued",
            Self::DeliveryPushed => "delivery.pushed",
            Self::DeliveryPrOpened => "delivery.pr_opened",
        }
    }
}

fn valid_source(source: &str) -> bool {
    !source.is_empty()
        && source
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_timestamp(timestamp: &str) -> bool {
    let bytes = timestamp.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
}

#[derive(Serialize)]
struct Envelope<'a> {
    ts: &'a str,
    task: &'a str,
    source: &'a str,
    event: &'a str,
    detail: &'a Value,
}

/// State-local journal writer with per-process warning suppression.
#[derive(Debug)]
pub struct JournalWriter {
    state: PathBuf,
    warned: AtomicBool,
}

impl JournalWriter {
    /// Construct a writer for one existing state directory.
    #[must_use]
    pub fn new(state: impl Into<PathBuf>) -> Self {
        Self {
            state: state.into(),
            warned: AtomicBool::new(false),
        }
    }

    /// Emit one strict event and expose validation or append failure.
    pub fn emit(
        &self,
        task: &TaskId,
        event: JournalEvent,
        detail: &Value,
        source: &str,
        timestamp: &str,
    ) -> Result<()> {
        let metadata = std::fs::symlink_metadata(&self.state).map_err(|error| {
            CoreError::io("inspect journal state directory", &self.state, error)
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(CoreError::UnsafePath {
                path: self.state.clone(),
                reason: "journal state must be a real directory",
            });
        }
        if !detail.is_object() {
            return Err(CoreError::MalformedRecord {
                kind: "journal detail",
                reason: "detail must be a JSON object",
            });
        }
        if !valid_source(source) {
            return Err(CoreError::InvalidIdentifier {
                kind: "journal source",
                value: source.to_owned(),
            });
        }
        if !valid_timestamp(timestamp) {
            return Err(CoreError::MalformedRecord {
                kind: "journal timestamp",
                reason: "expected YYYY-MM-DDTHH:MM:SSZ",
            });
        }
        let envelope = Envelope {
            ts: timestamp,
            task: task.as_str(),
            source,
            event: event.as_str(),
            detail,
        };
        let mut line = serde_json::to_vec(&envelope).map_err(|_| CoreError::MalformedRecord {
            kind: "journal envelope",
            reason: "could not serialize envelope",
        })?;
        line.push(b'\n');
        append_single_write(self.path(task), &line, 0o600)
    }

    /// Best-effort emit that never changes authoritative operation success.
    /// Returns a warning string only for the first failure from this writer.
    pub fn try_emit(
        &self,
        task: &TaskId,
        event: JournalEvent,
        detail: &Value,
        source: &str,
        timestamp: &str,
    ) -> Option<String> {
        self.emit(task, event, detail, source, timestamp)
            .err()
            .and_then(|error| {
                (!self.warned.swap(true, Ordering::SeqCst)).then(|| format!("mx-journal: {error}"))
            })
    }

    /// Return the task journal path.
    #[must_use]
    pub fn path(&self, task: &TaskId) -> PathBuf {
        self.state.join(format!("{}.journal", task.as_str()))
    }

    /// Return the writer's state directory.
    #[must_use]
    pub fn state(&self) -> &Path {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use serde_json::json;

    use super::{JournalEvent, JournalWriter};
    use crate::identifiers::TaskId;

    #[test]
    fn envelope_order_and_bytes_are_deterministic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let writer = JournalWriter::new(temp.path());
        let task = TaskId::parse("journal-1").expect("task");
        writer
            .emit(
                &task,
                JournalEvent::StatusReported,
                &json!({"raw":"done: yes","validated":true}),
                "mx-test",
                "2026-08-10T12:00:00Z",
            )
            .expect("emit");
        assert_eq!(
            fs::read_to_string(writer.path(&task)).expect("journal"),
            "{\"ts\":\"2026-08-10T12:00:00Z\",\"task\":\"journal-1\",\"source\":\"mx-test\",\"event\":\"status.reported\",\"detail\":{\"raw\":\"done: yes\",\"validated\":true}}\n"
        );
    }

    #[test]
    fn concurrent_rows_do_not_interleave_or_disappear() {
        let temp = tempfile::tempdir().expect("tempdir");
        let writer = Arc::new(JournalWriter::new(temp.path()));
        let task = TaskId::parse("concurrent").expect("task");
        let barrier = Arc::new(Barrier::new(16));
        thread::scope(|scope| {
            for index in 0..16 {
                let writer = Arc::clone(&writer);
                let barrier = Arc::clone(&barrier);
                let task = task.clone();
                scope.spawn(move || {
                    barrier.wait();
                    writer
                        .emit(
                            &task,
                            JournalEvent::TaskSpawned,
                            &json!({"index":index}),
                            "mx-test",
                            "2026-08-10T12:00:00Z",
                        )
                        .expect("emit");
                });
            }
        });
        let text = fs::read_to_string(writer.path(&task)).expect("journal");
        assert_eq!(text.lines().count(), 16);
        assert!(
            text.lines()
                .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
        );
    }

    #[test]
    fn best_effort_warns_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let task = TaskId::parse("failure").expect("task");
        fs::create_dir(temp.path().join("failure.journal")).expect("blocking directory");
        let writer = JournalWriter::new(temp.path());
        assert!(
            writer
                .try_emit(
                    &task,
                    JournalEvent::TaskSpawned,
                    &json!({}),
                    "mx-test",
                    "2026-08-10T12:00:00Z"
                )
                .is_some()
        );
        assert!(
            writer
                .try_emit(
                    &task,
                    JournalEvent::TaskSpawned,
                    &json!({}),
                    "mx-test",
                    "2026-08-10T12:00:00Z"
                )
                .is_none()
        );
    }
}

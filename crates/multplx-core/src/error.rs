//! Structured errors shared by core primitive modules.

use std::io;
use std::path::PathBuf;

/// A fail-closed core primitive error.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// An identifier or path component failed its grammar.
    #[error("invalid {kind}: {value:?}")]
    InvalidIdentifier {
        /// The identifier class.
        kind: &'static str,
        /// The rejected value.
        value: String,
    },
    /// A path escaped or violated an expected boundary.
    #[error("unsafe path {path}: {reason}")]
    UnsafePath {
        /// The rejected path.
        path: PathBuf,
        /// The failed invariant.
        reason: &'static str,
    },
    /// A bounded record was malformed.
    #[error("malformed {kind} record: {reason}")]
    MalformedRecord {
        /// The record class.
        kind: &'static str,
        /// A bounded diagnostic.
        reason: &'static str,
    },
    /// A record exceeded its explicit byte bound.
    #[error("{kind} record exceeds {limit} bytes")]
    RecordTooLarge {
        /// The record class.
        kind: &'static str,
        /// The maximum accepted bytes.
        limit: usize,
    },
    /// A closed vocabulary did not recognize a value.
    #[error("unknown {kind}: {value}")]
    UnknownValue {
        /// The vocabulary class.
        kind: &'static str,
        /// The rejected value.
        value: String,
    },
    /// An operating-system operation failed.
    #[error("{operation} failed for {path}: {source}")]
    Io {
        /// The operation being attempted.
        operation: &'static str,
        /// The target path.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// A bounded child command failed or returned unusable output.
    #[error("command {command} failed: {reason}")]
    Command {
        /// The executable name.
        command: String,
        /// A bounded explanation.
        reason: String,
    },
    /// Lock acquisition found a live or ambiguous owner.
    #[error("lock is held by {owner}")]
    LockHeld {
        /// The best available owner description.
        owner: String,
    },
}

impl CoreError {
    /// Attach a filesystem operation and target to an I/O error.
    pub(crate) fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}

/// Result type used by core primitive APIs.
pub type Result<T> = std::result::Result<T, CoreError>;

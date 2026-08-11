//! Validated identifiers used before any filesystem join or record render.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

const MAX_TASK_ID_BYTES: usize = 64;

/// A path-safe Multplx task identifier.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TaskId(String);

impl TaskId {
    /// Validate the legacy task-id grammar: one to 64 ASCII bytes, no leading
    /// dot, and only alphanumeric, dot, underscore, or dash characters.
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_TASK_ID_BYTES
            && !value.starts_with('.')
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        if !valid {
            return Err(CoreError::InvalidIdentifier {
                kind: "task id",
                value,
            });
        }
        Ok(Self(value))
    }

    /// Return the validated bytes as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl TryFrom<String> for TaskId {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self> {
        Self::parse(value)
    }
}

impl From<TaskId> for String {
    fn from(value: TaskId) -> Self {
        value.0
    }
}

/// One validated filename component that cannot traverse a directory.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PathComponent(String);

impl PathComponent {
    /// Validate a non-empty component without slash, NUL, `.` or `..`.
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let valid = !value.is_empty()
            && value != "."
            && value != ".."
            && !value.bytes().any(|byte| matches!(byte, b'/' | 0));
        if !valid {
            return Err(CoreError::InvalidIdentifier {
                kind: "path component",
                value,
            });
        }
        Ok(Self(value))
    }

    /// Return the validated component.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PathComponent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A lowercase SHA-256 digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    /// Validate exactly 64 lowercase hexadecimal characters.
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(CoreError::InvalidIdentifier {
                kind: "SHA-256 digest",
                value,
            });
        }
        Ok(Self(value))
    }

    /// Return the validated digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{PathComponent, Sha256Digest, TaskId};

    #[test]
    fn task_ids_match_the_legacy_path_safe_grammar() {
        for accepted in ["a", "task-1", "a.b_c", &"x".repeat(64)] {
            assert!(TaskId::parse(accepted).is_ok(), "{accepted}");
        }
        for rejected in [
            "",
            ".hidden",
            "../task",
            "a/b",
            "white space",
            &"x".repeat(65),
        ] {
            assert!(TaskId::parse(rejected).is_err(), "{rejected}");
        }
        let task = TaskId::try_from("round-trip".to_owned()).expect("try from");
        assert_eq!(task.to_string(), "round-trip");
        let owned: String = task.into();
        assert_eq!(owned, "round-trip");
    }

    #[test]
    fn components_refuse_traversal_and_separators() {
        assert!(PathComponent::parse("state").is_ok());
        for rejected in ["", ".", "..", "a/b", "nul\0byte"] {
            assert!(PathComponent::parse(rejected).is_err());
        }
        assert_eq!(
            PathComponent::parse("display")
                .expect("component")
                .to_string(),
            "display"
        );
    }

    #[test]
    fn sha256_requires_lowercase_hex() {
        assert!(Sha256Digest::parse("a".repeat(64)).is_ok());
        assert!(Sha256Digest::parse("A".repeat(64)).is_err());
        assert!(Sha256Digest::parse("a".repeat(63)).is_err());
    }
}

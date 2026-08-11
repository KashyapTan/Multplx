//! Normalized transition records and exhaustive supervision policy from
//! `bin/mx-transition-lib.sh`.

use crate::error::{CoreError, Result};

/// One backend-neutral five-field transition record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionRecord {
    /// Backend pane identity.
    pub pane_id: String,
    /// Backend workspace identity.
    pub workspace_id: String,
    /// Previous status when available.
    pub from_status: String,
    /// New status.
    pub to_status: String,
    /// Agent name when available.
    pub agent: String,
}

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            _ => character,
        })
        .collect()
}

impl TransitionRecord {
    /// Construct a record while scrubbing row delimiters from every field.
    #[must_use]
    pub fn new(
        pane_id: &str,
        workspace_id: &str,
        from_status: &str,
        to_status: &str,
        agent: &str,
    ) -> Self {
        Self {
            pane_id: clean(pane_id),
            workspace_id: clean(workspace_id),
            from_status: clean(from_status),
            to_status: clean(to_status),
            agent: clean(agent),
        }
    }

    /// Parse exactly five tab-separated fields.
    pub fn parse(record: &str) -> Result<Self> {
        let fields: Vec<&str> = record.split('\t').collect();
        if fields.len() != 5 {
            return Err(CoreError::MalformedRecord {
                kind: "transition",
                reason: "expected exactly five fields",
            });
        }
        Ok(Self::new(
            fields[0], fields[1], fields[2], fields[3], fields[4],
        ))
    }

    /// Render the exact current tab-separated bytes without a trailing newline.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.pane_id, self.workspace_id, self.from_status, self.to_status, self.agent
        )
    }
}

/// Fast-path supervision action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionAction {
    /// Surface immediately.
    Actionable,
    /// Absorb and clear dedupe.
    Absorb,
    /// Leave to ordinary completion semantics.
    Defer,
    /// Fall back to polling.
    Fallback,
}

impl TransitionAction {
    /// Return the exact legacy token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Actionable => "actionable",
            Self::Absorb => "absorb",
            Self::Defer => "defer",
            Self::Fallback => "fallback",
        }
    }
}

/// Apply the single-owner status-to-action table.
#[must_use]
pub fn policy(to_status: &str) -> TransitionAction {
    match to_status {
        "blocked" => TransitionAction::Actionable,
        "working" => TransitionAction::Absorb,
        "idle" | "done" => TransitionAction::Defer,
        _ => TransitionAction::Fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::{TransitionAction, TransitionRecord, policy};

    #[test]
    fn record_round_trip_preserves_exact_five_field_shape() {
        let record = TransitionRecord::new("p\tid", "ws", "", "blocked\n", "claude");
        assert_eq!(record.render(), "p id\tws\t\tblocked \tclaude");
        assert_eq!(
            TransitionRecord::parse(&record.render()).expect("parse"),
            record
        );
    }

    #[test]
    fn record_round_trip_holds_across_field_matrix() {
        let fields = ["", "ascii", "tab\tvalue", "line\nvalue", "unicode-❯"];
        for pane in fields {
            for status in fields {
                let record = TransitionRecord::new(pane, "workspace", "working", status, "agent");
                assert_eq!(
                    TransitionRecord::parse(&record.render()).expect("round trip"),
                    record
                );
            }
        }
    }

    #[test]
    fn policy_table_matches_the_shell_owner() {
        assert_eq!(policy("blocked"), TransitionAction::Actionable);
        assert_eq!(policy("working"), TransitionAction::Absorb);
        assert_eq!(policy("idle"), TransitionAction::Defer);
        assert_eq!(policy("unknown"), TransitionAction::Fallback);
    }
}

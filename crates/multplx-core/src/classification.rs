//! Pure status vocabulary, precedence, folds, and bounded tails.
//!
//! This module transfers the pure contracts from `bin/mx-classify-lib.sh`.
//! Actor-state subprocess reads remain with later lifecycle portions.

use std::collections::VecDeque;
use std::path::Path;

use regex::RegexBuilder;

use crate::error::{CoreError, Result};
use crate::filesystem::read_bounded_regular;

/// Default maximum bytes read from one append-only status stream.
pub const STATUS_READ_LIMIT: usize = 1024 * 1024;

/// The closed native runtime vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeState {
    /// Agent is idle.
    Idle,
    /// Agent is actively working.
    Working,
    /// Agent is blocked on interaction.
    Blocked,
    /// Agent is done.
    Done,
    /// No recognized native evidence.
    Unknown,
}

impl NativeState {
    /// Parse recognized native text, mapping everything else to unknown.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "idle" => Self::Idle,
            "working" => Self::Working,
            "blocked" => Self::Blocked,
            "done" => Self::Done,
            _ => Self::Unknown,
        }
    }

    fn token(self) -> Option<&'static str> {
        match self {
            Self::Idle => Some("idle"),
            Self::Working => Some("working"),
            Self::Blocked => Some("blocked"),
            Self::Done => Some("done"),
            Self::Unknown => None,
        }
    }
}

/// Attributed validation state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunStep {
    /// Validation is active.
    Working,
    /// Validation is parked.
    Parked,
    /// Validation completed.
    Done,
    /// Validation blocked.
    Blocked,
    /// Validation declared an external pause.
    Paused,
    /// Validation failed.
    Failed,
    /// No recognized run evidence.
    Unknown,
}

impl RunStep {
    /// Parse the recognized run-step vocabulary.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "working" => Self::Working,
            "parked" => Self::Parked,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            "paused" => Self::Paused,
            "failed" => Self::Failed,
            _ => Self::Unknown,
        }
    }

    fn token(self) -> Option<&'static str> {
        match self {
            Self::Working => Some("working"),
            Self::Parked => Some("parked"),
            Self::Done => Some("done"),
            Self::Blocked => Some("blocked"),
            Self::Paused => Some("paused"),
            Self::Failed => Some("failed"),
            Self::Unknown => None,
        }
    }
}

/// Text-only pane heuristic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Heuristic {
    /// Busy footer observed.
    Busy,
    /// Idle footer observed.
    Idle,
    /// No recognized heuristic.
    Unknown,
}

impl Heuristic {
    /// Parse the recognized heuristic vocabulary.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "busy" => Self::Busy,
            "idle" => Self::Idle,
            _ => Self::Unknown,
        }
    }

    fn token(self) -> Option<&'static str> {
        match self {
            Self::Busy => Some("busy"),
            Self::Idle => Some("idle"),
            Self::Unknown => None,
        }
    }
}

/// Resolve same-moment evidence in native, run-step, self-report, heuristic order.
#[must_use]
pub fn resolve_signal(
    native: NativeState,
    run_step: RunStep,
    self_report: &str,
    heuristic: Heuristic,
    pause_verb: &str,
) -> String {
    if let Some(token) = native.token() {
        return format!("native:{token}");
    }
    if let Some(token) = run_step.token() {
        return format!("run-step:{token}");
    }
    if matches!(
        self_report,
        "working" | "blocked" | "needs-decision" | "done" | "failed" | "resolved"
    ) || self_report == pause_verb
    {
        return format!("self-report:{self_report}");
    }
    if let Some(token) = heuristic.token() {
        return format!("heuristic:{token}");
    }
    "none".to_owned()
}

/// Parse the leading verb, excluding an optional `[key=...]` token.
#[must_use]
pub fn status_line_verb(line: &str) -> &str {
    let prefix = line.split_once(':').map_or(line, |(prefix, _)| prefix);
    prefix
        .split_once("[key=")
        .map_or(prefix, |(verb, _)| verb)
        .trim()
}

/// Return the text after the first colon, left-trimmed like the shell owner.
#[must_use]
pub fn status_line_note(line: &str) -> &str {
    line.split_once(':')
        .map_or(line, |(_, note)| note.trim_start())
}

fn decision_key(line: &str) -> Option<&str> {
    let prefix = line.split_once(':').map_or(line, |(prefix, _)| prefix);
    let Some((_, rest)) = prefix.split_once("[key=") else {
        return Some("default");
    };
    let (key, _) = rest.split_once(']')?;
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return None;
    }
    Some(key)
}

/// One still-open keyed event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenStatus {
    /// Decision or activity key.
    pub key: String,
    /// Opening verb.
    pub verb: String,
    /// Opening note.
    pub note: String,
}

fn replace_open(open: &mut VecDeque<OpenStatus>, next: OpenStatus) {
    open.retain(|item| item.key != next.key);
    open.push_back(next);
}

fn close_open(open: &mut VecDeque<OpenStatus>, key: &str) {
    open.retain(|item| item.key != key);
}

/// Fold the entire append-only stream into unresolved decisions.
#[must_use]
pub fn open_decisions(text: &str, resolve_verb: &str, held_verb: &str) -> Vec<OpenStatus> {
    let mut open = VecDeque::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Some(key) = decision_key(line) else {
            continue;
        };
        let verb = status_line_verb(line);
        if matches!(verb, "needs-decision" | "blocked") {
            replace_open(
                &mut open,
                OpenStatus {
                    key: key.to_owned(),
                    verb: verb.to_owned(),
                    note: status_line_note(line).to_owned(),
                },
            );
        } else if verb == resolve_verb || verb == held_verb {
            close_open(&mut open, key);
        }
    }
    open.into()
}

/// Fold the append-only stream into still-open material activities.
#[must_use]
pub fn open_activities(
    text: &str,
    pause_verb: &str,
    resolve_verb: &str,
    held_verb: &str,
) -> Vec<OpenStatus> {
    let mut open = VecDeque::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Some(key) = decision_key(line) else {
            continue;
        };
        let verb = status_line_verb(line);
        if verb == "working" || verb == pause_verb {
            replace_open(
                &mut open,
                OpenStatus {
                    key: key.to_owned(),
                    verb: verb.to_owned(),
                    note: status_line_note(line).to_owned(),
                },
            );
        } else if matches!(verb, "done" | "failed" | "needs-decision" | "blocked")
            || verb == resolve_verb
            || verb == held_verb
        {
            close_open(&mut open, key);
        }
    }
    open.into()
}

/// Render the shell-compatible tab-separated open-set rows.
#[must_use]
pub fn render_open_statuses(open: &[OpenStatus]) -> String {
    open.iter()
        .map(|item| format!("{}\t{}\t{}\n", item.key, item.verb, item.note))
        .collect()
}

/// Return whether a line is maintainer relevant under the legacy verb-aware rule.
pub fn is_maintainer_relevant(
    line: &str,
    override_regex: Option<&str>,
    pause_verb: &str,
) -> Result<bool> {
    if line.is_empty() || status_line_verb(line) == pause_verb {
        return Ok(false);
    }
    let verb = status_line_verb(line);
    if matches!(verb, "working" | "resolved" | "maintainer-held") {
        return Ok(false);
    }
    if override_regex.is_none() && matches!(verb, "done" | "needs-decision" | "blocked" | "failed")
    {
        return Ok(true);
    }
    let pattern = override_regex.unwrap_or(
        "done:|needs-decision:|blocked:|failed:|PR ready|checks green|ready in branch|merged",
    );
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|_| CoreError::MalformedRecord {
            kind: "maintainer relevance regex",
            reason: "invalid regular expression",
        })?;
    Ok(regex.is_match(line))
}

/// Read the last nonblank status line through an explicit byte bound.
pub fn last_status_line(path: impl AsRef<Path>, limit: usize) -> Result<Option<String>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }
    let bytes = read_bounded_regular(path, limit)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| CoreError::MalformedRecord {
        kind: "status",
        reason: "status is not UTF-8",
    })?;
    Ok(text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::{
        Heuristic, NativeState, RunStep, open_activities, open_decisions, render_open_statuses,
        resolve_signal,
    };

    #[test]
    fn signal_precedence_is_exhaustive() {
        assert_eq!(
            resolve_signal(
                NativeState::Blocked,
                RunStep::Working,
                "paused",
                Heuristic::Busy,
                "paused"
            ),
            "native:blocked"
        );
        assert_eq!(
            resolve_signal(
                NativeState::Unknown,
                RunStep::Done,
                "working",
                Heuristic::Busy,
                "paused"
            ),
            "run-step:done"
        );
        assert_eq!(
            resolve_signal(
                NativeState::Unknown,
                RunStep::Unknown,
                "blocked",
                Heuristic::Busy,
                "paused"
            ),
            "self-report:blocked"
        );
    }

    #[test]
    fn keyed_folds_preserve_reopen_order() {
        let text = "needs-decision [key=a]: first\nblocked [key=b]: second\nresolved [key=a]: yes\nneeds-decision [key=a]: third\nworking [key=job]: run\n";
        assert_eq!(
            render_open_statuses(&open_decisions(text, "resolved", "maintainer-held")),
            "b\tblocked\tsecond\na\tneeds-decision\tthird\n"
        );
        assert_eq!(
            render_open_statuses(&open_activities(
                text,
                "paused",
                "resolved",
                "maintainer-held"
            )),
            "job\tworking\trun\n"
        );
    }
}

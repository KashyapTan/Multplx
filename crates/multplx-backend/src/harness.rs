//! Verified harness detection and static actor/daemon resolution.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use multplx_core::process::{ProcessProbe, SystemProcessProbe};

/// Verified primary and worker harness vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Harness {
    Claude,
    Codex,
    Cursor,
    Pi,
    Unknown,
}

impl Harness {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Pi => "pi",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Harness {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn classify(command: &str, arguments: &str) -> Option<Harness> {
    let name = Path::new(command)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(command);
    if name.contains("claude") {
        Some(Harness::Claude)
    } else if name.contains("codex") {
        Some(Harness::Codex)
    } else if name == "cursor-agent" {
        Some(Harness::Cursor)
    } else if name == "pi" {
        Some(Harness::Pi)
    } else if name.starts_with("node") || name.starts_with("python") {
        if arguments.contains("claude") {
            Some(Harness::Claude)
        } else if arguments.contains("codex") {
            Some(Harness::Codex)
        } else if arguments.contains("cursor-agent") {
            Some(Harness::Cursor)
        } else if arguments.contains(" pi ") || arguments.ends_with("/pi") {
            Some(Harness::Pi)
        } else {
            None
        }
    } else {
        None
    }
}

/// Detect the current harness using verified environment markers, then a
/// bounded process-ancestry walk.
#[must_use]
pub fn detect_with(
    environment: &impl Fn(&str) -> Option<String>,
    start_pid: u32,
    processes: &impl ProcessProbe,
) -> Harness {
    if environment("CLAUDECODE").as_deref() == Some("1") {
        return Harness::Claude;
    }
    if environment("PI_CODING_AGENT").as_deref() == Some("true") {
        return Harness::Pi;
    }
    let mut pid = start_pid;
    for _ in 0..8 {
        let Ok(row) = processes.ancestry_row(pid) else {
            break;
        };
        if let Some(harness) = classify(&row.command, &row.arguments) {
            return harness;
        }
        if row.parent_pid <= 1 {
            break;
        }
        pid = row.parent_pid;
    }
    Harness::Unknown
}

/// Detect the current process's harness.
#[must_use]
pub fn detect() -> Harness {
    detect_with(
        &|name| std::env::var(name).ok(),
        std::process::id(),
        &SystemProcessProbe::default(),
    )
}

/// Static harness configuration rooted at one effective config directory.
#[derive(Clone, Debug)]
pub struct HarnessConfig {
    directory: PathBuf,
}

impl HarnessConfig {
    #[must_use]
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn actor_token(&self) -> Option<String> {
        fs::read_to_string(self.directory.join("actor-harness"))
            .ok()
            .map(|text| {
                text.chars()
                    .filter(|character| !character.is_whitespace())
                    .collect()
            })
            .filter(|value: &String| !value.is_empty())
    }

    fn daemon_fields(&self) -> Vec<String> {
        fs::read_to_string(self.directory.join("daemon-harness"))
            .ok()
            .and_then(|text| {
                text.lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(|line| line.split_whitespace().map(str::to_owned).collect())
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn actor(&self, own: Harness) -> String {
        match self.actor_token().as_deref() {
            None | Some("default") => own.to_string(),
            Some(value) => value.to_owned(),
        }
    }

    #[must_use]
    pub fn daemon(&self, own: Harness) -> String {
        match self.daemon_fields().first().map(String::as_str) {
            None | Some("default") => self.actor(own),
            Some(value) => value.to_owned(),
        }
    }

    #[must_use]
    pub fn daemon_model(&self) -> Option<String> {
        let fields = self.daemon_fields();
        if fields.first().is_none_or(|value| value == "default") {
            None
        } else {
            fields.get(1).cloned()
        }
    }

    #[must_use]
    pub fn daemon_effort(&self) -> Option<String> {
        let fields = self.daemon_fields();
        if fields.first().is_none_or(|value| value == "default") {
            None
        } else {
            fields.get(2).cloned()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use multplx_core::error::{CoreError, Result};
    use multplx_core::process::{AncestryRow, ProcessIdentity, ProcessProbe};

    use super::{Harness, HarnessConfig, detect_with};

    struct Processes(HashMap<u32, AncestryRow>);

    impl ProcessProbe for Processes {
        fn is_alive(&self, pid: u32) -> bool {
            self.0.contains_key(&pid)
        }
        fn identity(&self, pid: u32) -> Result<ProcessIdentity> {
            Err(CoreError::InvalidIdentifier {
                kind: "pid",
                value: pid.to_string(),
            })
        }
        fn ancestry_row(&self, pid: u32) -> Result<AncestryRow> {
            self.0
                .get(&pid)
                .cloned()
                .ok_or(CoreError::InvalidIdentifier {
                    kind: "pid",
                    value: pid.to_string(),
                })
        }
    }

    #[test]
    fn markers_precede_bounded_ancestry() {
        let processes = Processes(HashMap::from([(
            9,
            AncestryRow {
                parent_pid: 1,
                command: "/usr/bin/codex".to_owned(),
                arguments: "codex".to_owned(),
            },
        )]));
        assert_eq!(
            detect_with(
                &|name| (name == "CLAUDECODE").then(|| "1".to_owned()),
                9,
                &processes
            ),
            Harness::Claude
        );
        assert_eq!(detect_with(&|_| None, 9, &processes), Harness::Codex);
    }

    #[test]
    fn static_actor_and_daemon_tokens_preserve_fallbacks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = HarnessConfig::new(temp.path());
        assert_eq!(config.actor(Harness::Claude), "claude");
        assert_eq!(config.daemon(Harness::Claude), "claude");
        std::fs::write(temp.path().join("actor-harness"), " codex\n").expect("actor");
        std::fs::write(
            temp.path().join("daemon-harness"),
            "# comment\n\n pi   model/x   high \nignored\n",
        )
        .expect("daemon");
        assert_eq!(config.actor(Harness::Claude), "codex");
        assert_eq!(config.daemon(Harness::Claude), "pi");
        assert_eq!(config.daemon_model().as_deref(), Some("model/x"));
        assert_eq!(config.daemon_effort().as_deref(), Some("high"));
    }
}

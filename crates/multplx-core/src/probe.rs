//! Read-only tool and compatibility probes from `bin/mx-probe-lib.sh`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{CoreError, Result};
use crate::tangle::{default_branch, primary_tangle_branch, render_bootstrap_tangle};

/// Verified runtime backend names needed by the probe surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    /// tmux session provider.
    Tmux,
    /// Herdr session provider.
    Herdr,
    /// cmux session provider.
    Cmux,
}

impl Backend {
    /// Parse one current backend name.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "tmux" => Ok(Self::Tmux),
            "herdr" => Ok(Self::Herdr),
            "cmux" => Ok(Self::Cmux),
            _ => Err(CoreError::UnknownValue {
                kind: "backend",
                value: value.to_owned(),
            }),
        }
    }

    /// Required backend-specific tools.
    #[must_use]
    pub fn required_tools(self) -> &'static [&'static str] {
        match self {
            Self::Tmux => &["tmux"],
            Self::Herdr => &["herdr", "jq"],
            Self::Cmux => &["cmux", "jq"],
        }
    }
}

/// One structured bootstrap tool finding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolRecord {
    /// Installable through a package command.
    Missing { tool: String, command: String },
    /// Requires manual installation guidance.
    MissingManual { tool: String, url: String },
    /// Requested backend is not current.
    BackendInvalid { backend: String },
}

impl ToolRecord {
    /// Render the exact tab-separated structured row.
    #[must_use]
    pub fn render_record(&self) -> String {
        match self {
            Self::Missing { tool, command } => format!("MISSING\t{tool}\t{command}\n"),
            Self::MissingManual { tool, url } => format!("MISSING_MANUAL\t{tool}\t{url}\n"),
            Self::BackendInvalid { backend } => {
                format!("BACKEND_INVALID\t{backend}\ttmux herdr cmux\n")
            }
        }
    }

    /// Render the established bootstrap diagnostic.
    #[must_use]
    pub fn render_bootstrap(&self) -> String {
        match self {
            Self::Missing { tool, command } => format!("MISSING: {tool} (install: {command})\n"),
            Self::MissingManual { tool, url } => {
                format!("MISSING_MANUAL: {tool} (instructions: {url})\n")
            }
            Self::BackendInvalid { backend } => {
                format!("BACKEND_INVALID: {backend} (known: tmux herdr cmux)\n")
            }
        }
    }
}

/// Installation guidance for one current external tool.
pub fn install_command(tool: &str) -> Option<&'static str> {
    match tool {
        "tmux" => Some("brew install tmux  # or the platform's package manager"),
        "node" => Some("brew install node  # or the platform's package manager"),
        "git" => Some("brew install git  # or the platform's package manager"),
        "gh" => Some("brew install gh  # or the platform's package manager"),
        "curl" => Some("brew install curl  # or the platform's package manager"),
        "jq" => Some("brew install jq  # or the platform's package manager"),
        "cmux" => Some("brew install --cask cmux  # or see https://cmux.com"),
        "treehouse" => Some("curl -fsSL https://kunchenguid.github.io/treehouse/install.sh | sh"),
        _ => None,
    }
}

/// Manual installation URL for a current tool.
pub fn manual_install_url(tool: &str) -> Option<&'static str> {
    (tool == "herdr").then_some("https://herdr.dev")
}

/// Abstract PATH/tool observation.
pub trait ToolProbe {
    /// Return whether a named executable is available.
    fn available(&self, tool: &str) -> bool;
    /// Return whether Treehouse advertises durable leases.
    fn treehouse_supports_lease(&self) -> bool;
}

/// Host PATH observation.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemToolProbe;

impl ToolProbe for SystemToolProbe {
    fn available(&self, tool: &str) -> bool {
        executable_candidates(tool).any(|candidate| {
            candidate.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
    }

    fn treehouse_supports_lease(&self) -> bool {
        Command::new("treehouse")
            .args(["get", "--help"])
            .output()
            .ok()
            .filter(|output| output.stdout.len() + output.stderr.len() <= 64 * 1024)
            .is_some_and(|output| output_supports_lease(&output.stdout, &output.stderr))
    }
}

fn output_supports_lease(stdout: &[u8], stderr: &[u8]) -> bool {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    text.match_indices("--lease").any(|(index, flag)| {
        let before = text[..index].chars().next_back();
        let after = text[index + flag.len()..].chars().next();
        let boundary = |character: Option<char>| {
            character.is_none_or(|character| {
                !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-')
            })
        };
        boundary(before) && boundary(after)
    })
}

fn executable_candidates(tool: &str) -> impl Iterator<Item = PathBuf> {
    let explicit = Path::new(tool).components().count() > 1;
    let paths = if explicit {
        vec![PathBuf::from(tool)]
    } else {
        std::env::var_os("PATH")
            .map(|path| {
                std::env::split_paths(&path)
                    .map(|directory| directory.join(tool))
                    .collect()
            })
            .unwrap_or_default()
    };
    paths.into_iter()
}

fn missing(tool: &str) -> ToolRecord {
    if let Some(url) = manual_install_url(tool) {
        ToolRecord::MissingManual {
            tool: tool.to_owned(),
            url: url.to_owned(),
        }
    } else {
        ToolRecord::Missing {
            tool: tool.to_owned(),
            command: install_command(tool).unwrap_or("").to_owned(),
        }
    }
}

/// Collect backend and common-tool findings in deterministic legacy order.
pub fn tool_records(backend_name: &str, probe: &impl ToolProbe) -> Vec<ToolRecord> {
    let backend = Backend::parse(backend_name).ok();
    let mut records = Vec::new();
    if let Some(backend) = backend {
        for tool in backend.required_tools() {
            if !probe.available(tool) {
                records.push(missing(tool));
            }
        }
    } else {
        records.push(ToolRecord::BackendInvalid {
            backend: backend_name.to_owned(),
        });
    }
    for tool in ["node", "git", "gh", "jq", "treehouse"] {
        if !probe.available(tool) {
            records.push(missing(tool));
        }
    }
    if probe.available("treehouse") && !probe.treehouse_supports_lease() {
        records.push(missing("treehouse"));
    }
    records
}

/// Return the structured tangle row when present.
pub fn tangle_record(root: impl AsRef<Path>) -> Result<Option<(String, String)>> {
    let root = root.as_ref();
    let Some(branch) = primary_tangle_branch(root)? else {
        return Ok(None);
    };
    let default = default_branch(root)?.unwrap_or_else(|| "main".to_owned());
    Ok(Some((branch, default)))
}

/// Render the bootstrap tangle result when present.
pub fn bootstrap_tangle(root: impl AsRef<Path>, read_only: bool) -> Result<Option<String>> {
    let root = root.as_ref();
    Ok(tangle_record(root)?
        .map(|(branch, default)| render_bootstrap_tangle(root, &branch, &default, read_only)))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::{
        Backend, SystemToolProbe, ToolProbe, ToolRecord, install_command, manual_install_url,
        output_supports_lease, tool_records,
    };

    struct FixtureProbe {
        tools: HashSet<String>,
        lease: bool,
    }

    impl ToolProbe for FixtureProbe {
        fn available(&self, tool: &str) -> bool {
            self.tools.contains(tool)
        }

        fn treehouse_supports_lease(&self) -> bool {
            self.lease
        }
    }

    #[test]
    fn lease_flag_boundaries_and_tool_guidance_are_exhaustive() {
        for (stdout, stderr, expected) in [
            ("--lease", "", true),
            ("usage: get --lease VALUE", "", true),
            ("", "flags:\n  --lease\n", true),
            ("--lease-extra", "", false),
            ("x--lease", "", false),
            ("no lease flag", "", false),
        ] {
            assert_eq!(
                output_supports_lease(stdout.as_bytes(), stderr.as_bytes()),
                expected
            );
        }
        for tool in [
            "tmux",
            "node",
            "git",
            "gh",
            "curl",
            "jq",
            "cmux",
            "treehouse",
        ] {
            assert!(
                install_command(tool).is_some(),
                "missing guidance for {tool}"
            );
        }
        assert_eq!(manual_install_url("herdr"), Some("https://herdr.dev"));
        assert_eq!(manual_install_url("other"), None);
        assert_eq!(install_command("other"), None);
        assert_eq!(Backend::Tmux.required_tools(), ["tmux"]);
        assert_eq!(Backend::Herdr.required_tools(), ["herdr", "jq"]);
        assert_eq!(Backend::Cmux.required_tools(), ["cmux", "jq"]);
    }

    #[test]
    fn tool_records_cover_invalid_backend_and_lease_capability() {
        let all = HashSet::from_iter(
            [
                "tmux",
                "herdr",
                "cmux",
                "node",
                "git",
                "gh",
                "jq",
                "treehouse",
            ]
            .map(str::to_owned),
        );
        assert!(
            tool_records(
                "tmux",
                &FixtureProbe {
                    tools: all.clone(),
                    lease: true
                }
            )
            .is_empty()
        );
        let lease_missing = tool_records(
            "tmux",
            &FixtureProbe {
                tools: all,
                lease: false,
            },
        );
        assert_eq!(lease_missing.len(), 1);
        let invalid = tool_records(
            "invalid",
            &FixtureProbe {
                tools: HashSet::new(),
                lease: false,
            },
        );
        assert!(matches!(invalid[0], ToolRecord::BackendInvalid { .. }));
        assert!(invalid[0].render_record().starts_with("BACKEND_INVALID\t"));
        assert!(
            invalid[0]
                .render_bootstrap()
                .starts_with("BACKEND_INVALID:")
        );
    }

    #[test]
    fn system_probe_requires_regular_executable_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp.path().join("tool");
        fs::write(&executable, b"#!/bin/sh\n").expect("tool");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).expect("mode");
        let probe = SystemToolProbe;
        assert!(probe.available(executable.to_str().expect("path")));
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o600)).expect("mode");
        assert!(!probe.available(executable.to_str().expect("path")));
        assert!(!probe.available(temp.path().to_str().expect("directory")));
        assert!(!probe.available(temp.path().join("absent").to_str().expect("absent")));
        let _ = probe.treehouse_supports_lease();
    }
}

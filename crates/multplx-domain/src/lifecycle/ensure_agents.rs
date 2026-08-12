//! Project agent-memory convention.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAINTENANCE: &str = "## Maintaining this file\n\nKeep this file for knowledge useful to almost every future agent session in this project.\nDo not repeat what the codebase already shows; point to the authoritative file or command instead.\nPrefer rewriting or pruning existing entries over appending new ones.\nWhen updating this file, preserve this bar for all agents and keep entries concise.\n";

const SKELETON: &str = "# Project agent memory\n\nThis file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.\n\n- Add durable project-specific notes here as they are discovered through real work.\n";

#[derive(Debug, thiserror::Error)]
pub enum EnsureAgentsError {
    #[error("not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("{0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaintenanceChange {
    Unchanged,
    Added,
}

fn maintenance_present(bytes: &[u8]) -> bool {
    bytes
        .split(|byte| *byte == b'\n')
        .any(|line| line == b"## Maintaining this file" || line == b"## Maintaining this file\r")
}

fn ensure_maintenance(path: &Path) -> Result<MaintenanceChange, EnsureAgentsError> {
    let mut bytes = fs::read(path)?;
    if maintenance_present(&bytes) {
        return Ok(MaintenanceChange::Unchanged);
    }
    let crlf = bytes.windows(2).any(|pair| pair == b"\r\n");
    let eol = if crlf { "\r\n" } else { "\n" };
    if !bytes.is_empty() {
        if bytes.last() == Some(&b'\n') {
            bytes.extend_from_slice(eol.as_bytes());
        } else {
            bytes.extend_from_slice(eol.as_bytes());
            bytes.extend_from_slice(eol.as_bytes());
        }
    }
    let section = if crlf {
        MAINTENANCE.replace('\n', "\r\n")
    } else {
        MAINTENANCE.to_owned()
    };
    bytes.extend_from_slice(section.as_bytes());
    fs::write(path, bytes)?;
    Ok(MaintenanceChange::Added)
}

fn create_symlink(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(not(unix))]
    {
        let _ = (target, link);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symlinks require Unix",
        ))
    }
}

fn claude_link_is_correct(dir: &Path, agents: &Path, claude: &Path) -> bool {
    if !claude.is_symlink() {
        return false;
    }
    match fs::read_link(claude) {
        Ok(target) if target == Path::new("AGENTS.md") || target == Path::new("./AGENTS.md") => {
            true
        }
        Ok(_) if agents.exists() => fs::canonicalize(claude).ok() == fs::canonicalize(agents).ok(),
        _ => {
            let _ = dir;
            false
        }
    }
}

fn case_variant(dir: &Path) -> Result<Option<String>, io::Error> {
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name();
        let Some(text) = name.to_str() else { continue };
        if text != "AGENTS.md" && text.eq_ignore_ascii_case("AGENTS.md") {
            return Ok(Some(text.to_owned()));
        }
    }
    Ok(None)
}

/// Ensures the real `AGENTS.md`, compatibility symlink, and maintenance section.
pub fn ensure(dir: &Path) -> Result<String, EnsureAgentsError> {
    if !dir.is_dir() {
        return Err(EnsureAgentsError::NotDirectory(dir.to_path_buf()));
    }
    let dir = fs::canonicalize(dir)?;
    if let Some(name) = case_variant(&dir)? {
        return Err(EnsureAgentsError::Conflict(format!(
            "memory file is named {name} in {} but the convention is AGENTS.md; rename it to AGENTS.md so CLAUDE.md links portably",
            dir.display()
        )));
    }
    let agents = dir.join("AGENTS.md");
    let claude = dir.join("CLAUDE.md");
    let agents_meta = fs::symlink_metadata(&agents).ok();
    let claude_meta = fs::symlink_metadata(&claude).ok();

    if agents_meta
        .as_ref()
        .is_some_and(|meta| meta.file_type().is_symlink())
    {
        return Err(EnsureAgentsError::Conflict(format!(
            "AGENTS.md is a symlink in {}; expected AGENTS.md to be the real file",
            dir.display()
        )));
    }
    if agents_meta.as_ref().is_some_and(|meta| !meta.is_file()) {
        return Err(EnsureAgentsError::Conflict(format!(
            "AGENTS.md exists in {} but is not a regular file",
            dir.display()
        )));
    }

    if agents_meta.is_some() {
        match claude_meta {
            Some(meta) if meta.file_type().is_symlink() => {
                if !claude_link_is_correct(&dir, &agents, &claude) {
                    return Err(EnsureAgentsError::Conflict(format!(
                        "CLAUDE.md is a symlink in {} but does not point to AGENTS.md",
                        dir.display()
                    )));
                }
                return Ok(match ensure_maintenance(&agents)? {
                    MaintenanceChange::Added => format!(
                        "updated: added ## Maintaining this file to AGENTS.md in {}",
                        dir.display()
                    ),
                    MaintenanceChange::Unchanged => format!(
                        "unchanged: AGENTS.md with CLAUDE.md -> AGENTS.md in {}",
                        dir.display()
                    ),
                });
            }
            None => {
                let change = ensure_maintenance(&agents)?;
                create_symlink(Path::new("AGENTS.md"), &claude)?;
                return Ok(match change {
                    MaintenanceChange::Added => format!(
                        "updated: added ## Maintaining this file to AGENTS.md and symlinked CLAUDE.md -> AGENTS.md in {}",
                        dir.display()
                    ),
                    MaintenanceChange::Unchanged => {
                        format!("symlinked: CLAUDE.md -> AGENTS.md in {}", dir.display())
                    }
                });
            }
            Some(meta) if meta.is_file() => {
                return Err(EnsureAgentsError::Conflict(format!(
                    "both AGENTS.md and CLAUDE.md are real files in {}; reconcile them manually",
                    dir.display()
                )));
            }
            Some(_) => {
                return Err(EnsureAgentsError::Conflict(format!(
                    "CLAUDE.md exists in {} but is not a regular file or symlink",
                    dir.display()
                )));
            }
        }
    }

    match claude_meta {
        Some(meta) if meta.file_type().is_symlink() => {
            if !claude_link_is_correct(&dir, &agents, &claude) {
                return Err(EnsureAgentsError::Conflict(format!(
                    "CLAUDE.md is a symlink in {} but AGENTS.md is missing and the link does not point to AGENTS.md",
                    dir.display()
                )));
            }
            fs::write(&agents, format!("{SKELETON}\n{MAINTENANCE}"))?;
            Ok(format!(
                "created: AGENTS.md and kept CLAUDE.md -> AGENTS.md in {}",
                dir.display()
            ))
        }
        Some(meta) if meta.is_file() => {
            fs::rename(&claude, &agents)?;
            ensure_maintenance(&agents)?;
            create_symlink(Path::new("AGENTS.md"), &claude)?;
            Ok(format!(
                "promoted: moved CLAUDE.md to AGENTS.md and symlinked CLAUDE.md -> AGENTS.md in {}",
                dir.display()
            ))
        }
        Some(_) => Err(EnsureAgentsError::Conflict(format!(
            "CLAUDE.md exists in {} but is not a regular file or symlink",
            dir.display()
        ))),
        None => {
            fs::write(&agents, format!("{SKELETON}\n{MAINTENANCE}"))?;
            create_symlink(Path::new(OsStr::new("AGENTS.md")), &claude)?;
            Ok(format!(
                "created: AGENTS.md and CLAUDE.md -> AGENTS.md in {}",
                dir.display()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_detection_accepts_lf_and_crlf() {
        assert!(maintenance_present(b"x\n## Maintaining this file\ny\n"));
        assert!(maintenance_present(
            b"x\r\n## Maintaining this file\r\ny\r\n"
        ));
        assert!(!maintenance_present(b"## Maintaining this filename\n"));
    }
}

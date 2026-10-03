//! Standing-agent home identity with bounded, explicit legacy layout reads.
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::filesystem::read_bounded_regular;

/// Identity written in a newly provisioned standing-agent home.
pub const MARKER: &str = ".mx-agent-home";
/// Historical identity retained in existing homes until explicit migration.
pub const LEGACY_MARKER: &str = ".mx-daemon-home";
/// Route registry written in a new home.
pub const REGISTRY: &str = "agents.md";
/// Historical route registry retained in an existing layout.
pub const LEGACY_REGISTRY: &str = "daemons.md";

fn optional_bytes(path: &Path, bound: usize) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_bounded_regular(path, bound).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CoreError::io("inspect agent layout", path, error)),
    }
}

fn select(
    directory: &Path,
    canonical: &str,
    legacy: &str,
    bound: usize,
    marker: bool,
) -> Result<PathBuf> {
    let current = directory.join(canonical);
    let old = directory.join(legacy);
    let new_bytes = optional_bytes(&current, bound)?;
    let old_bytes = optional_bytes(&old, bound)?;
    if let (Some(new), Some(old)) = (&new_bytes, &old_bytes) {
        let equal = if marker {
            new.iter()
                .copied()
                .filter(|byte| !byte.is_ascii_whitespace())
                .eq(old
                    .iter()
                    .copied()
                    .filter(|byte| !byte.is_ascii_whitespace()))
        } else {
            new == old
        };
        if !equal {
            return Err(CoreError::UnsafePath {
                path: current,
                reason: "canonical and legacy agent layouts conflict; reconcile before use",
            });
        }
    }
    Ok(if new_bytes.is_none() && old_bytes.is_some() {
        old
    } else {
        current
    })
}

/// Read the canonical identity, or the sole historical layout; conflicting IDs fail closed.
pub fn marker_path(home: impl AsRef<Path>) -> Result<PathBuf> {
    select(home.as_ref(), MARKER, LEGACY_MARKER, 4096, true)
}

/// Resolve a standing agent's exact path-component identity.
pub fn identity(home: impl AsRef<Path>) -> Result<Option<String>> {
    let path = marker_path(home)?;
    let Some(bytes) = optional_bytes(&path, 4096)? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| CoreError::MalformedRecord {
        kind: "agent-home marker",
        reason: "identity is not UTF-8",
    })?;
    let id = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    crate::identifiers::PathComponent::parse(id.clone())?;
    Ok(Some(id))
}

/// Select a read registry without dropping legacy-only route entries.
pub fn registry_path(data: impl AsRef<Path>) -> Result<PathBuf> {
    select(
        data.as_ref(),
        REGISTRY,
        LEGACY_REGISTRY,
        16 * 1024 * 1024,
        false,
    )
}

/// Keep an existing layout sticky; new registries use the canonical name.
/// Two independent copies require explicit reconciliation before mutation.
pub fn registry_write_path(data: impl AsRef<Path>) -> Result<PathBuf> {
    let data = data.as_ref();
    let selected = registry_path(data)?;
    if data.join(REGISTRY).exists() && data.join(LEGACY_REGISTRY).exists() {
        return Err(CoreError::UnsafePath {
            path: selected,
            reason: "two agent registry copies require explicit reconciliation before mutation",
        });
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn new_and_existing_layouts_are_selected_without_migration() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(marker_path(root.path()).unwrap(), root.path().join(MARKER));
        assert_eq!(
            registry_write_path(root.path()).unwrap(),
            root.path().join(REGISTRY)
        );
        fs::write(root.path().join(LEGACY_MARKER), "worker\n").unwrap();
        assert_eq!(identity(root.path()).unwrap().as_deref(), Some("worker"));
        fs::write(
            root.path().join(LEGACY_REGISTRY),
            "- worker - accepted route\n",
        )
        .unwrap();
        assert_eq!(
            marker_path(root.path()).unwrap(),
            root.path().join(LEGACY_MARKER)
        );
        assert_eq!(
            registry_write_path(root.path()).unwrap(),
            root.path().join(LEGACY_REGISTRY)
        );
        assert!(!root.path().join(MARKER).exists());
        assert!(!root.path().join(REGISTRY).exists());
        fs::write(root.path().join(MARKER), " worker \n").unwrap();
        assert_eq!(marker_path(root.path()).unwrap(), root.path().join(MARKER));
        fs::write(root.path().join(MARKER), "other\n").unwrap();
        assert!(marker_path(root.path()).is_err());
        assert!(identity(root.path()).is_err());
        fs::write(root.path().join(REGISTRY), "- worker - accepted route\n").unwrap();
        assert_eq!(
            registry_path(root.path()).unwrap(),
            root.path().join(REGISTRY)
        );
        assert!(registry_write_path(root.path()).is_err());
        fs::write(root.path().join(REGISTRY), "- other - unrelated route\n").unwrap();
        assert!(registry_path(root.path()).is_err());
    }

    #[test]
    fn malformed_or_unsafe_aliases_are_not_missing_layouts() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        fs::write(&outside, "worker\n").unwrap();
        symlink(&outside, root.path().join(LEGACY_MARKER)).unwrap();
        assert!(marker_path(root.path()).is_err());
        fs::remove_file(root.path().join(LEGACY_MARKER)).unwrap();
        fs::write(root.path().join(MARKER), vec![b'x'; 4097]).unwrap();
        assert!(marker_path(root.path()).is_err());
        fs::create_dir(root.path().join(REGISTRY)).unwrap();
        assert!(registry_path(root.path()).is_err());
    }
}

//! Canonical path-boundary checks for existing and not-yet-created targets.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::identifiers::PathComponent;

/// A canonical existing directory used as a containment boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExistingRoot(PathBuf);

impl ExistingRoot {
    /// Canonicalize an existing, non-symlink directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| CoreError::io("inspect root", path, error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(CoreError::UnsafePath {
                path: path.to_path_buf(),
                reason: "root must be a real directory",
            });
        }
        let canonical = fs::canonicalize(path)
            .map_err(|error| CoreError::io("canonicalize root", path, error))?;
        Ok(Self(canonical))
    }

    /// Return the canonical root.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Join one already-validated filename component.
    #[must_use]
    pub fn join(&self, component: &PathComponent) -> PathBuf {
        self.0.join(component.as_str())
    }

    /// Validate that an existing descendant canonicalizes below this root.
    pub fn existing_descendant(&self, path: impl AsRef<Path>) -> Result<PathBuf> {
        let path = path.as_ref();
        let canonical = fs::canonicalize(path)
            .map_err(|error| CoreError::io("canonicalize descendant", path, error))?;
        if canonical == self.0 || !canonical.starts_with(&self.0) {
            return Err(CoreError::UnsafePath {
                path: path.to_path_buf(),
                reason: "path is outside the allowed root",
            });
        }
        Ok(canonical)
    }

    /// Validate a possibly absent descendant by canonicalizing its nearest
    /// existing ancestor and rejecting lexical traversal.
    pub fn absent_descendant(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        let relative = relative.as_ref();
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(CoreError::UnsafePath {
                path: relative.to_path_buf(),
                reason: "relative path contains traversal or a root",
            });
        }
        let candidate = self.0.join(relative);
        let mut ancestor = candidate.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(|| CoreError::UnsafePath {
                path: candidate.clone(),
                reason: "path has no existing ancestor",
            })?;
        }
        let canonical_ancestor = fs::canonicalize(ancestor)
            .map_err(|error| CoreError::io("canonicalize ancestor", ancestor, error))?;
        if !canonical_ancestor.starts_with(&self.0) {
            return Err(CoreError::UnsafePath {
                path: candidate,
                reason: "existing ancestor escapes the allowed root",
            });
        }
        Ok(self.0.join(relative))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::ExistingRoot;

    #[test]
    fn absent_paths_reject_traversal_and_symlink_ancestors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root_path = temp.path().join("root");
        let outside = temp.path().join("outside");
        fs::create_dir(&root_path).expect("root");
        fs::create_dir(&outside).expect("outside");
        symlink(&outside, root_path.join("link")).expect("symlink");
        let root = ExistingRoot::open(&root_path).expect("existing root");

        assert!(root.absent_descendant("safe/new").is_ok());
        assert!(root.absent_descendant("../outside/file").is_err());
        assert!(root.absent_descendant("link/file").is_err());
    }
}

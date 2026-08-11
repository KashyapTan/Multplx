//! Trusted custom-check validation from `bin/mx-check-lib.sh`.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::OFlags;
use sha2::{Digest, Sha256};

use crate::error::{CoreError, Result};
use crate::identifiers::{Sha256Digest, TaskId};

const TRUST_VERSION: &str = "mx-custom-check-v1";
const MAX_TRUST_BYTES: usize = 256;

fn private_file(path: &Path, mode: u32, device: u64) -> Result<File> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| CoreError::io("inspect private file", path, error))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o7777 != mode
        || metadata.dev() != device
        || metadata.nlink() != 1
    {
        return Err(CoreError::UnsafePath {
            path: path.to_path_buf(),
            reason: "private file metadata does not match",
        });
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(OFlags::NOFOLLOW.bits() as i32);
    let file = options
        .open(path)
        .map_err(|error| CoreError::io("open private file", path, error))?;
    let opened = file
        .metadata()
        .map_err(|error| CoreError::io("recheck private file", path, error))?;
    if opened.dev() != device || opened.ino() != metadata.ino() || opened.nlink() != 1 {
        return Err(CoreError::UnsafePath {
            path: path.to_path_buf(),
            reason: "private file changed during validation",
        });
    }
    Ok(file)
}

/// Read and validate the exact two-line custom-check trust record.
pub fn read_trust(state: impl AsRef<Path>, task: &TaskId) -> Result<Sha256Digest> {
    let state = state.as_ref();
    let state_metadata = fs::symlink_metadata(state)
        .map_err(|error| CoreError::io("inspect state directory", state, error))?;
    if !state_metadata.is_dir() || state_metadata.file_type().is_symlink() {
        return Err(CoreError::UnsafePath {
            path: state.to_path_buf(),
            reason: "state must be a real directory",
        });
    }
    let path = state.join(format!("{}.check-trust", task.as_str()));
    let mut file = private_file(&path, 0o600, state_metadata.dev())?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_TRUST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CoreError::io("read trust record", &path, error))?;
    if bytes.len() > MAX_TRUST_BYTES {
        return Err(CoreError::RecordTooLarge {
            kind: "custom-check trust",
            limit: MAX_TRUST_BYTES,
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| CoreError::MalformedRecord {
        kind: "custom-check trust",
        reason: "record is not UTF-8",
    })?;
    let mut lines = text.split_inclusive('\n');
    let version = lines.next().filter(|line| line.ends_with('\n'));
    let Some(digest) = lines.next().filter(|line| line.ends_with('\n')) else {
        return Err(CoreError::MalformedRecord {
            kind: "custom-check trust",
            reason: "expected exactly two newline-terminated lines",
        });
    };
    if version != Some("mx-custom-check-v1\n") || lines.next().is_some() {
        return Err(CoreError::MalformedRecord {
            kind: "custom-check trust",
            reason: "expected exactly two newline-terminated lines",
        });
    }
    Sha256Digest::parse(digest.trim_end_matches('\n'))
}

/// Return whether the registered executable still matches its trusted digest.
pub fn registered(state: impl AsRef<Path>, task: &TaskId) -> Result<bool> {
    let state = state.as_ref();
    let digest = read_trust(state, task)?;
    let device = fs::metadata(state)
        .map_err(|error| CoreError::io("inspect state directory", state, error))?
        .dev();
    let path = state.join(format!("{}.check.sh", task.as_str()));
    let mut check = private_file(&path, 0o700, device)?;
    let mut bytes = Vec::new();
    check
        .read_to_end(&mut bytes)
        .map_err(|error| CoreError::io("hash custom check", &path, error))?;
    Ok(format!("{:x}", Sha256::digest(bytes)) == digest.as_str())
}

/// A private, single-link snapshot of a trusted custom check.
#[derive(Debug)]
pub struct CheckSnapshot {
    path: PathBuf,
}

impl CheckSnapshot {
    /// Copy and re-verify the trusted executable into a private state-local file.
    pub fn prepare(state: impl AsRef<Path>, task: &TaskId) -> Result<Self> {
        let state = state.as_ref();
        let digest = read_trust(state, task)?;
        let state_metadata = fs::metadata(state)
            .map_err(|error| CoreError::io("inspect state directory", state, error))?;
        let source_path = state.join(format!("{}.check.sh", task.as_str()));
        let mut source = private_file(&source_path, 0o700, state_metadata.dev())?;
        let mut bytes = Vec::new();
        source
            .read_to_end(&mut bytes)
            .map_err(|error| CoreError::io("read custom check", &source_path, error))?;
        if format!("{:x}", Sha256::digest(&bytes)) != digest.as_str() {
            return Err(CoreError::MalformedRecord {
                kind: "custom check",
                reason: "trusted digest does not match",
            });
        }
        let mut temporary = tempfile::Builder::new()
            .prefix(".mx-custom-check.")
            .tempfile_in(state)
            .map_err(|error| CoreError::io("create custom-check snapshot", state, error))?;
        temporary.as_file_mut().write_all(&bytes).map_err(|error| {
            CoreError::io("write custom-check snapshot", temporary.path(), error)
        })?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| {
                CoreError::io("chmod custom-check snapshot", temporary.path(), error)
            })?;
        temporary.as_file().sync_all().map_err(|error| {
            CoreError::io("flush custom-check snapshot", temporary.path(), error)
        })?;
        let path = temporary.into_temp_path().keep().map_err(|error| {
            CoreError::io("retain custom-check snapshot", &error.path, error.error)
        })?;
        let snapshot_metadata = fs::metadata(&path)
            .map_err(|error| CoreError::io("inspect custom-check snapshot", &path, error))?;
        if snapshot_metadata.dev() != state_metadata.dev()
            || snapshot_metadata.nlink() != 1
            || snapshot_metadata.permissions().mode() & 0o7777 != 0o600
        {
            let _ = fs::remove_file(&path);
            return Err(CoreError::UnsafePath {
                path,
                reason: "snapshot metadata does not match",
            });
        }
        Ok(Self { path })
    }

    /// Return the private snapshot path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for CheckSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Render a trust record using the existing wire bytes.
#[must_use]
pub fn render_trust(digest: &Sha256Digest) -> String {
    format!("{TRUST_VERSION}\n{}\n", digest.as_str())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::fs::hard_link;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use sha2::{Digest, Sha256};

    use super::{CheckSnapshot, MAX_TRUST_BYTES, read_trust, registered, render_trust};
    use crate::identifiers::{Sha256Digest, TaskId};

    #[test]
    fn trust_and_snapshot_require_exact_private_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let task = TaskId::parse("check-1").expect("task");
        let check = temp.path().join("check-1.check.sh");
        let bytes = b"#!/bin/sh\nexit 0\n";
        fs::write(&check, bytes).expect("check");
        fs::set_permissions(&check, fs::Permissions::from_mode(0o700)).expect("mode");
        let digest = Sha256Digest::parse(format!("{:x}", Sha256::digest(bytes))).expect("digest");
        let trust = temp.path().join("check-1.check-trust");
        fs::write(&trust, render_trust(&digest)).expect("trust");
        fs::set_permissions(&trust, fs::Permissions::from_mode(0o600)).expect("mode");

        assert_eq!(read_trust(temp.path(), &task).expect("read"), digest);
        assert!(registered(temp.path(), &task).expect("registered"));
        let snapshot = CheckSnapshot::prepare(temp.path(), &task).expect("snapshot");
        assert_eq!(fs::read(snapshot.path()).expect("snapshot bytes"), bytes);
        let snapshot_path = snapshot.path().to_path_buf();
        drop(snapshot);
        assert!(!snapshot_path.exists());
    }

    #[test]
    fn malformed_trust_and_unsafe_metadata_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let task = TaskId::parse("check-2").expect("task");
        let trust = state.join("check-2.check-trust");
        let check = state.join("check-2.check.sh");
        fs::write(&check, b"#!/bin/sh\nexit 0\n").expect("check");
        fs::set_permissions(&check, fs::Permissions::from_mode(0o700)).expect("check mode");

        for bytes in [
            b"mx-custom-check-v1\n".as_slice(),
            b"wrong\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            b"mx-custom-check-v1\nnot-a-digest\n",
            b"mx-custom-check-v1\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nextra\n",
            &[0xff, b'\n'],
        ] {
            fs::write(&trust, bytes).expect("trust");
            fs::set_permissions(&trust, fs::Permissions::from_mode(0o600)).expect("trust mode");
            assert!(read_trust(&state, &task).is_err());
        }
        fs::write(&trust, vec![b'x'; MAX_TRUST_BYTES + 1]).expect("large trust");
        fs::set_permissions(&trust, fs::Permissions::from_mode(0o600)).expect("trust mode");
        assert!(read_trust(&state, &task).is_err());

        let digest = Sha256Digest::parse("a".repeat(64)).expect("digest");
        fs::write(&trust, render_trust(&digest)).expect("trust");
        fs::set_permissions(&trust, fs::Permissions::from_mode(0o644)).expect("public mode");
        assert!(read_trust(&state, &task).is_err());
        fs::set_permissions(&trust, fs::Permissions::from_mode(0o600)).expect("private mode");
        assert!(!registered(&state, &task).expect("hash mismatch"));
        assert!(CheckSnapshot::prepare(&state, &task).is_err());

        let trust_link = state.join("check-2-link.check-trust");
        symlink(&trust, &trust_link).expect("trust link");
        let linked_task = TaskId::parse("check-2-link").expect("linked task");
        assert!(read_trust(&state, &linked_task).is_err());

        let state_link = temp.path().join("state-link");
        symlink(&state, &state_link).expect("state link");
        assert!(read_trust(&state_link, &task).is_err());
    }

    #[test]
    fn missing_non_directory_and_multiply_linked_inputs_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let task = TaskId::parse("check-3").expect("task");
        assert!(read_trust(temp.path().join("missing"), &task).is_err());

        let file_state = temp.path().join("file-state");
        fs::write(&file_state, b"state").expect("file state");
        assert!(read_trust(&file_state, &task).is_err());

        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let bytes = b"#!/bin/sh\nexit 0\n";
        let digest = Sha256Digest::parse(format!("{:x}", Sha256::digest(bytes))).expect("digest");
        let trust = state.join("check-3.check-trust");
        fs::write(&trust, render_trust(&digest)).expect("trust");
        fs::set_permissions(&trust, fs::Permissions::from_mode(0o600)).expect("trust mode");
        hard_link(&trust, state.join("trust-copy")).expect("trust hard link");
        assert!(read_trust(&state, &task).is_err());

        fs::remove_file(state.join("trust-copy")).expect("remove trust link");
        let check = state.join("check-3.check.sh");
        fs::write(&check, bytes).expect("check");
        fs::set_permissions(&check, fs::Permissions::from_mode(0o700)).expect("check mode");
        hard_link(&check, state.join("check-copy")).expect("check hard link");
        assert!(registered(&state, &task).is_err());
        assert!(CheckSnapshot::prepare(&state, &task).is_err());
    }
}

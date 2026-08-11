//! Private, same-directory atomic publication and bounded no-follow reads.

use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use rustix::fs::OFlags;

use crate::error::{CoreError, Result};

/// Publication boundary used by deterministic fault-injection tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationFault {
    /// Fail before creating or writing a temporary file.
    BeforeWrite,
    /// Fail after the bytes are flushed but before publication.
    AfterWrite,
    /// Fail after mode application but before rename.
    AfterMode,
    /// Fail after the rename has published the new bytes.
    AfterRename,
}

fn injected(
    point: PublicationFault,
    requested: Option<PublicationFault>,
    path: &Path,
) -> Result<()> {
    if requested == Some(point) {
        return Err(CoreError::io(
            "injected publication fault",
            path,
            std::io::Error::other(format!("fault at {point:?}")),
        ));
    }
    Ok(())
}

/// Atomically replace a regular file with exact bytes and private mode.
///
/// The temporary file is created in the destination directory, flushed,
/// chmodded, renamed, and followed by a parent-directory sync. Unsafe final
/// symlinks are rejected before publication.
pub fn atomic_replace(path: impl AsRef<Path>, bytes: &[u8], mode: u32) -> Result<()> {
    atomic_replace_with_fault(path, bytes, mode, None)
}

/// The fault-injectable form of [`atomic_replace`].
pub fn atomic_replace_with_fault(
    path: impl AsRef<Path>,
    bytes: &[u8],
    mode: u32,
    fault: Option<PublicationFault>,
) -> Result<()> {
    let path = path.as_ref();
    let parent = path.parent().ok_or_else(|| CoreError::UnsafePath {
        path: path.to_path_buf(),
        reason: "destination has no parent directory",
    })?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| CoreError::io("inspect destination directory", parent, error))?;
    if !parent_metadata.is_dir() || parent_metadata.file_type().is_symlink() {
        return Err(CoreError::UnsafePath {
            path: parent.to_path_buf(),
            reason: "destination parent must be a real directory",
        });
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(CoreError::UnsafePath {
            path: path.to_path_buf(),
            reason: "destination must be absent or a regular file",
        });
    }
    injected(PublicationFault::BeforeWrite, fault, path)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".mx-atomic.")
        .tempfile_in(parent)
        .map_err(|error| CoreError::io("create temporary file", parent, error))?;
    temporary
        .as_file_mut()
        .write_all(bytes)
        .map_err(|error| CoreError::io("write temporary file", temporary.path(), error))?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|error| CoreError::io("flush temporary file", temporary.path(), error))?;
    injected(PublicationFault::AfterWrite, fault, path)?;
    temporary
        .as_file()
        .set_permissions(Permissions::from_mode(mode))
        .map_err(|error| CoreError::io("set temporary file mode", temporary.path(), error))?;
    injected(PublicationFault::AfterMode, fault, path)?;
    temporary
        .persist(path)
        .map_err(|error| CoreError::io("rename temporary file", path, error.error))?;
    injected(PublicationFault::AfterRename, fault, path)?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| CoreError::io("flush destination directory", parent, error))?;
    Ok(())
}

/// Read at most `limit` bytes from a no-follow regular file.
pub fn read_bounded_regular(path: impl AsRef<Path>, limit: usize) -> Result<Vec<u8>> {
    let path = path.as_ref();
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(OFlags::NOFOLLOW.bits() as i32);
    let file = options
        .open(path)
        .map_err(|error| CoreError::io("open no-follow file", path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| CoreError::io("inspect opened file", path, error))?;
    if !metadata.is_file() {
        return Err(CoreError::UnsafePath {
            path: path.to_path_buf(),
            reason: "opened path is not a regular file",
        });
    }
    if metadata.len() > limit as u64 {
        return Err(CoreError::RecordTooLarge {
            kind: "file",
            limit,
        });
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CoreError::io("read bounded file", path, error))?;
    if bytes.len() > limit {
        return Err(CoreError::RecordTooLarge {
            kind: "file",
            limit,
        });
    }
    Ok(bytes)
}

/// Append one payload with one `write(2)` call to an absent-or-regular file.
///
/// This preserves the legacy journal and queue requirement that concurrent
/// writers cannot interleave a committed row. A short write is an error.
pub fn append_single_write(path: impl AsRef<Path>, payload: &[u8], mode: u32) -> Result<()> {
    let path = path.as_ref();
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(CoreError::UnsafePath {
            path: path.to_path_buf(),
            reason: "append target must be absent or a regular file",
        });
    }
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .append(true)
        .mode(mode)
        .custom_flags(OFlags::NOFOLLOW.bits() as i32);
    let mut file = options
        .open(path)
        .map_err(|error| CoreError::io("open append file", path, error))?;
    let written = file
        .write(payload)
        .map_err(|error| CoreError::io("append record", path, error))?;
    if written != payload.len() {
        return Err(CoreError::io(
            "append record",
            path,
            std::io::Error::new(std::io::ErrorKind::WriteZero, "short append"),
        ));
    }
    Ok(())
}

/// Return the Unix mode without following a final symlink.
pub fn mode(path: impl AsRef<Path>) -> Result<u32> {
    use std::os::unix::fs::MetadataExt;

    let path = path.as_ref();
    fs::symlink_metadata(path)
        .map(|metadata| metadata.mode() & 0o7777)
        .map_err(|error| CoreError::io("inspect mode", path, error))
}

/// Remove only the named temporary path if it is a regular file.
pub fn cleanup_regular(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(path)
                .map_err(|error| CoreError::io("remove temporary file", path, error))
        }
        Ok(_) => Err(CoreError::UnsafePath {
            path: path.to_path_buf(),
            reason: "cleanup target is not a regular file",
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CoreError::io("inspect cleanup target", path, error)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::{PublicationFault, append_single_write, atomic_replace, atomic_replace_with_fault};

    #[test]
    fn atomic_replace_preserves_old_bytes_before_rename() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("record");
        fs::write(&path, b"old\n").expect("old record");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("old mode");

        for fault in [
            PublicationFault::BeforeWrite,
            PublicationFault::AfterWrite,
            PublicationFault::AfterMode,
        ] {
            assert!(atomic_replace_with_fault(&path, b"new\n", 0o600, Some(fault)).is_err());
            assert_eq!(fs::read(&path).expect("published record"), b"old\n");
        }
        assert!(
            atomic_replace_with_fault(&path, b"new\n", 0o600, Some(PublicationFault::AfterRename))
                .is_err()
        );
        assert_eq!(fs::read(&path).expect("published new record"), b"new\n");
    }

    #[test]
    fn private_publication_refuses_a_symlink_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let outside = temp.path().join("outside");
        let link = temp.path().join("record");
        fs::write(&outside, b"outside").expect("outside");
        symlink(&outside, &link).expect("link");
        assert!(atomic_replace(&link, b"new", 0o600).is_err());
        assert_eq!(fs::read(&outside).expect("outside bytes"), b"outside");
    }

    #[test]
    fn append_uses_private_creation_and_refuses_symlinks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("journal");
        append_single_write(&path, b"one\n", 0o600).expect("append");
        append_single_write(&path, b"two\n", 0o600).expect("append");
        assert_eq!(fs::read(&path).expect("journal"), b"one\ntwo\n");
        assert_eq!(
            fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
            0o600
        );
    }
}

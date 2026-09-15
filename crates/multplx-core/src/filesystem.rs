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

/// One compare-and-replace step. The final step is the authoritative publication.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TransitionWrite {
    pub path: std::path::PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TransitionReceipt {
    version: u32,
    operation: String,
    writes: Vec<TransitionWrite>,
    progress: usize,
    committed: bool,
}

/// Interruption points include the write/progress gap and the commit/receipt gap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionFault {
    BeforeIntent,
    AfterIntent,
    AfterWrite(usize),
    AfterProgress(usize),
    AfterCommit,
}

fn transition_error(reason: &'static str) -> CoreError {
    CoreError::MalformedRecord {
        kind: "filesystem transition",
        reason,
    }
}

fn receipt_bytes(receipt: &TransitionReceipt) -> Result<Vec<u8>> {
    serde_json::to_vec(receipt).map_err(|_| transition_error("cannot encode receipt"))
}

/// Read exact durable intent for caller comparison before an explicit recovery.
pub fn read_transition_writes(root: &Path, operation: &str) -> Result<Vec<TransitionWrite>> {
    if operation.is_empty()
        || !operation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || matches!(operation, "." | "..")
    {
        return Err(transition_error("invalid operation identity"));
    }
    if fs::symlink_metadata(root.join(".transitions"))
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(transition_error("transition directory is a symlink"));
    }
    let receipt: TransitionReceipt = serde_json::from_slice(&read_bounded_regular(
        root.join(".transitions").join(format!("{operation}.json")),
        16 * 1024 * 1024,
    )?)
    .map_err(|_| transition_error("malformed durable intent"))?;
    if receipt.operation != operation
        || receipt.version != 1
        || receipt.writes.is_empty()
        || receipt.progress > receipt.writes.len()
        || (receipt.committed && receipt.progress != receipt.writes.len())
    {
        return Err(transition_error("incompatible or corrupt receipt"));
    }
    Ok(receipt.writes)
}
/// Explicit recovery reuses recorded compare-and-replace intent, never new bytes.
pub fn recover_transition(root: &Path, operation: &str) -> Result<()> {
    let writes = read_transition_writes(root, operation)?;
    recoverable_transition(root, operation, &writes, None)
}

/// Apply or resume a durable operation under a short filesystem lock.
///
/// Intent precedes all writes. Earlier steps are preparatory; the last atomic
/// replacement is the acceptance point. Readers consume the final record, never
/// the receipt as independent state. A crash after any write is recovered by
/// comparing its exact before/after bytes. Conflicts retain intent for explicit
/// reconciliation. No external execution occurs while this lock is held.
/// Reusing an operation ID with different intent is always an error.
pub fn recoverable_transition(
    root: &Path,
    operation: &str,
    writes: &[TransitionWrite],
    fault: Option<TransitionFault>,
) -> Result<()> {
    if operation.is_empty()
        || !operation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || matches!(operation, "." | "..")
        || writes.is_empty()
    {
        return Err(transition_error(
            "invalid operation identity or empty write set",
        ));
    }
    let _lock = crate::locks::DirectoryLock::try_acquire(
        root.join(".transition.lock"),
        &crate::process::SystemProcessProbe::default(),
    )?;
    let directory = root.join(".transitions");
    fs::create_dir_all(&directory)
        .map_err(|e| CoreError::io("create transition directory", &directory, e))?;
    if fs::symlink_metadata(&directory)
        .map_err(|e| CoreError::io("inspect transition directory", &directory, e))?
        .file_type()
        .is_symlink()
    {
        return Err(transition_error("transition directory is a symlink"));
    }
    File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| CoreError::io("flush transition parent", root, error))?;
    let mut seen = std::collections::BTreeSet::new();
    for write in writes {
        if !seen.insert(write.path.clone())
            || write.path.as_os_str().is_empty()
            || write
                .path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            || write.path.starts_with(".transitions")
            || write.path == Path::new(".transition.lock")
        {
            return Err(transition_error(
                "write targets must be unique confined record paths",
            ));
        }
        let mut current = root.to_path_buf();
        for component in write.path.components() {
            current.push(component);
            if fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(transition_error("write target traverses a symlink"));
            }
        }
    }
    let receipt_path = directory.join(format!("{operation}.json"));
    // An unfinished overlapping operation owns its records until reconciled.
    for entry in fs::read_dir(&directory)
        .map_err(|e| CoreError::io("read transition directory", &directory, e))?
    {
        let path = entry
            .map_err(|e| CoreError::io("read transition entry", &directory, e))?
            .path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        if path == receipt_path {
            continue;
        }
        let other: TransitionReceipt =
            serde_json::from_slice(&read_bounded_regular(&path, 16 * 1024 * 1024)?)
                .map_err(|_| transition_error("unreadable pending receipt"))?;
        if other.version != 1
            || other.writes.is_empty()
            || other.progress > other.writes.len()
            || (other.committed && other.progress != other.writes.len())
        {
            return Err(transition_error("incompatible or corrupt receipt"));
        }
        if !other.committed && other.writes.iter().any(|w| seen.contains(&w.path)) {
            return Err(transition_error(
                "overlapping unfinished transition requires reconciliation",
            ));
        }
    }
    let mut receipt = if receipt_path.exists() {
        let receipt: TransitionReceipt =
            serde_json::from_slice(&read_bounded_regular(&receipt_path, 16 * 1024 * 1024)?)
                .map_err(|_| transition_error("malformed intent"))?;
        if receipt.version != 1
            || receipt.progress > writes.len()
            || (receipt.committed && receipt.progress != writes.len())
            || receipt.operation != operation
            || receipt.writes != writes
        {
            return Err(transition_error(
                "operation identity conflicts with durable intent",
            ));
        }
        receipt
    } else {
        for write in writes {
            let actual = match read_bounded_regular(root.join(&write.path), 16 * 1024 * 1024) {
                Ok(bytes) => Some(bytes),
                Err(CoreError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::NotFound =>
                {
                    None
                }
                Err(error) => return Err(error),
            };
            if actual != write.before {
                return Err(transition_error("revision changed before intent"));
            }
        }
        if fault == Some(TransitionFault::BeforeIntent) {
            return Err(transition_error("injected before intent"));
        }
        let receipt = TransitionReceipt {
            version: 1,
            operation: operation.into(),
            writes: writes.to_vec(),
            progress: 0,
            committed: false,
        };
        atomic_replace(&receipt_path, &receipt_bytes(&receipt)?, 0o600)?;
        receipt
    };
    if receipt.committed {
        return Ok(());
    }
    for write in writes.iter().take(receipt.progress) {
        if read_bounded_regular(root.join(&write.path), 16 * 1024 * 1024)? != write.after {
            return Err(transition_error(
                "completed preparatory record diverged; intent retained",
            ));
        }
    }
    if fault == Some(TransitionFault::AfterIntent) {
        return Err(transition_error("injected after intent"));
    }
    for (index, write) in writes.iter().enumerate().skip(receipt.progress) {
        let target = root.join(&write.path);
        let actual = match read_bounded_regular(&target, 16 * 1024 * 1024) {
            Ok(bytes) => Some(bytes),
            Err(CoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                None
            }
            Err(error) => return Err(error),
        };
        if actual.as_deref() != Some(write.after.as_slice()) {
            if actual != write.before {
                return Err(transition_error(
                    "record diverged; unfinished intent retained",
                ));
            }
            atomic_replace(&target, &write.after, 0o600)?;
        }
        if fault == Some(TransitionFault::AfterWrite(index)) {
            return Err(transition_error("injected after write"));
        }
        receipt.progress = index + 1;
        atomic_replace(&receipt_path, &receipt_bytes(&receipt)?, 0o600)?;
        if fault == Some(TransitionFault::AfterProgress(index)) {
            return Err(transition_error("injected after progress"));
        }
    }
    if fault == Some(TransitionFault::AfterCommit) {
        return Err(transition_error("injected after authoritative commit"));
    }
    receipt.committed = true;
    atomic_replace(receipt_path, &receipt_bytes(&receipt)?, 0o600)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::{
        PublicationFault, append_single_write, atomic_replace, atomic_replace_with_fault,
        cleanup_regular, mode, read_bounded_regular,
    };

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

        let outside = temp.path().join("outside");
        let link = temp.path().join("link");
        fs::write(&outside, "outside\n").expect("outside");
        symlink(&outside, &link).expect("link");
        assert!(append_single_write(&link, b"unsafe\n", 0o600).is_err());
        assert!(append_single_write(temp.path(), b"unsafe\n", 0o600).is_err());
    }

    #[test]
    fn bounded_reads_modes_and_cleanup_cover_safe_and_refusal_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("record");
        fs::write(&path, b"bytes").expect("record");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).expect("mode");
        assert_eq!(read_bounded_regular(&path, 5).expect("bounded"), b"bytes");
        assert!(read_bounded_regular(&path, 4).is_err());
        assert!(read_bounded_regular(temp.path(), 100).is_err());
        assert_eq!(mode(&path).expect("mode") & 0o777, 0o640);
        assert!(mode(temp.path().join("missing")).is_err());

        let link = temp.path().join("link");
        symlink(&path, &link).expect("link");
        assert!(read_bounded_regular(&link, 100).is_err());
        assert!(cleanup_regular(&link).is_err());
        assert!(cleanup_regular(temp.path()).is_err());
        assert!(cleanup_regular(temp.path().join("missing")).is_ok());
        assert!(cleanup_regular(&path).is_ok());
        assert!(!path.exists());
    }

    #[test]
    fn atomic_publication_rejects_unsafe_parents_and_destinations() {
        let temp = tempfile::tempdir().expect("tempdir");
        let real = temp.path().join("real");
        let linked = temp.path().join("linked");
        fs::create_dir(&real).expect("real parent");
        symlink(&real, &linked).expect("linked parent");
        assert!(atomic_replace(linked.join("record"), b"bytes", 0o600).is_err());
        assert!(atomic_replace(temp.path().join("missing/record"), b"bytes", 0o600).is_err());
        let directory = real.join("directory");
        fs::create_dir(&directory).expect("directory destination");
        assert!(atomic_replace(&directory, b"bytes", 0o600).is_err());
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    fn writes() -> Vec<TransitionWrite> {
        vec![
            TransitionWrite {
                path: "artifact".into(),
                before: None,
                after: b"retained result".to_vec(),
            },
            TransitionWrite {
                path: "task.meta".into(),
                before: Some(b"old".to_vec()),
                after: b"new".to_vec(),
            },
        ]
    }
    #[test]
    fn every_transition_boundary_recovers_once() {
        for fault in [
            TransitionFault::BeforeIntent,
            TransitionFault::AfterIntent,
            TransitionFault::AfterWrite(0),
            TransitionFault::AfterProgress(0),
            TransitionFault::AfterWrite(1),
            TransitionFault::AfterProgress(1),
            TransitionFault::AfterCommit,
        ] {
            let temp = tempfile::tempdir().unwrap();
            fs::write(temp.path().join("task.meta"), b"old").unwrap();
            assert!(recoverable_transition(temp.path(), "op-1", &writes(), Some(fault)).is_err());
            recoverable_transition(temp.path(), "op-1", &writes(), None).unwrap();
            recoverable_transition(temp.path(), "op-1", &writes(), None).unwrap();
            assert_eq!(fs::read(temp.path().join("task.meta")).unwrap(), b"new");
            assert_eq!(
                fs::read(temp.path().join("artifact")).unwrap(),
                b"retained result"
            );
            let receipt: TransitionReceipt = serde_json::from_slice(
                &fs::read(temp.path().join(".transitions/op-1.json")).unwrap(),
            )
            .unwrap();
            assert!(receipt.committed);
            assert_eq!(receipt.progress, 2);
        }
    }
    #[test]
    fn unfinished_operations_retain_conflicts_and_block_overlapping_writers() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("task.meta"), b"old").unwrap();
        assert!(
            recoverable_transition(
                temp.path(),
                "op-1",
                &writes(),
                Some(TransitionFault::AfterProgress(0))
            )
            .is_err()
        );
        assert!(recoverable_transition(temp.path(), "op-2", &writes(), None).is_err());
        fs::write(temp.path().join("artifact"), b"foreign").unwrap();
        assert!(recoverable_transition(temp.path(), "op-1", &writes(), None).is_err());
        assert_eq!(fs::read(temp.path().join("task.meta")).unwrap(), b"old");
        assert!(temp.path().join(".transitions/op-1.json").exists());
    }
    #[test]
    fn stable_intent_rejects_changed_operations_and_corrupt_receipts() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("task.meta"), b"old").unwrap();
        assert!(
            recoverable_transition(
                temp.path(),
                "op-1",
                &writes(),
                Some(TransitionFault::AfterIntent)
            )
            .is_err()
        );
        let mut changed = writes();
        changed[1].after = b"different".to_vec();
        assert!(recoverable_transition(temp.path(), "op-1", &changed, None).is_err());
        let path = temp.path().join(".transitions/op-1.json");
        let mut receipt: TransitionReceipt =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        receipt.progress = 3;
        fs::write(path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(recoverable_transition(temp.path(), "op-1", &writes(), None).is_err());
    }
    #[test]
    fn transition_paths_refuse_aliasing_and_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("task.meta"), b"old").unwrap();
        let mut invalid = writes();
        invalid[0].path = "../outside".into();
        assert!(recoverable_transition(temp.path(), "op", &invalid, None).is_err());
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("linked")).unwrap();
        invalid[0].path = "linked/artifact".into();
        assert!(recoverable_transition(temp.path(), "op", &invalid, None).is_err());
        assert!(!outside.path().join("artifact").exists());
        invalid[0].path = "task.meta".into();
        assert!(recoverable_transition(temp.path(), "op", &invalid, None).is_err());
    }
}

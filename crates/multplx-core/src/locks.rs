//! Git-lock abandonment proof and race-safe owner-directory locks from
//! `bin/mx-lock-lib.sh` and the shared lock surface in `bin/mx-wake-lib.sh`.

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{CoreError, Result};
use crate::process::ProcessProbe;

/// Result of probing whether a path has a live process holder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HolderStatus {
    /// At least one holder exists.
    Held,
    /// No holder exists.
    Clear,
    /// The answer could not be proven.
    Unknown,
}

/// Injectable open-file holder evidence.
pub trait HolderProbe {
    /// Probe one file or directory path.
    fn holder_status(&self, path: &Path) -> HolderStatus;
}

/// `lsof`-backed holder evidence matching the current git-lock proof.
#[derive(Clone, Copy, Debug, Default)]
pub struct LsofProbe;

impl HolderProbe for LsofProbe {
    fn holder_status(&self, path: &Path) -> HolderStatus {
        let output = Command::new("lsof").arg("--").arg(path).output();
        match output {
            Ok(output) if output.status.success() => HolderStatus::Held,
            Ok(output)
                if output.status.code() == Some(1)
                    && output.stdout.is_empty()
                    && output.stderr.is_empty() =>
            {
                HolderStatus::Clear
            }
            _ => HolderStatus::Unknown,
        }
    }
}

/// Prove a git lock abandoned only when every required fact is unambiguous.
pub fn git_lock_is_provably_stale(
    lock: impl AsRef<Path>,
    companion_directory: Option<&Path>,
    minimum_age: Duration,
    now: SystemTime,
    probe: &impl HolderProbe,
) -> Result<bool> {
    let lock = lock.as_ref();
    let metadata = match fs::metadata(lock) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(CoreError::io("inspect git lock", lock, error)),
    };
    if probe.holder_status(lock) != HolderStatus::Clear {
        return Ok(false);
    }
    if let Some(directory) = companion_directory
        && probe.holder_status(directory) != HolderStatus::Clear
    {
        return Ok(false);
    }
    let modified = metadata
        .modified()
        .map_err(|error| CoreError::io("read git lock mtime", lock, error))?;
    let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
    Ok(age >= minimum_age)
}

fn current_pid() -> u32 {
    std::process::id()
}

fn owner_target(lock_path: &Path) -> Result<PathBuf> {
    let target = fs::read_link(lock_path)
        .map_err(|error| CoreError::io("read lock owner link", lock_path, error))?;
    if target.is_absolute() {
        Ok(target)
    } else {
        Ok(lock_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(target))
    }
}

fn read_owner_pid(lock_path: &Path) -> Option<u32> {
    fs::read_to_string(lock_path.join("pid"))
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
}

fn remove_owner(lock_path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(lock_path)
        .map_err(|error| CoreError::io("inspect lock", lock_path, error))?;
    if metadata.file_type().is_symlink() {
        let target = owner_target(lock_path)?;
        fs::remove_file(lock_path)
            .map_err(|error| CoreError::io("remove lock link", lock_path, error))?;
        let _ = fs::remove_file(target.join("pid"));
        let _ = fs::remove_file(target.join("mx-home"));
        let _ = fs::remove_file(target.join("pid-identity"));
        let _ = fs::remove_file(target.join("watcher-path"));
        let _ = fs::remove_dir(target);
        Ok(())
    } else if metadata.is_dir() {
        for name in ["pid", "mx-home", "pid-identity", "watcher-path"] {
            let _ = fs::remove_file(lock_path.join(name));
        }
        fs::remove_dir(lock_path)
            .map_err(|error| CoreError::io("remove lock directory", lock_path, error))
    } else {
        Err(CoreError::UnsafePath {
            path: lock_path.to_path_buf(),
            reason: "lock is neither an owner link nor a directory",
        })
    }
}

fn try_publish_owner(lock_path: &Path) -> Result<Option<PathBuf>> {
    let parent = lock_path.parent().ok_or_else(|| CoreError::UnsafePath {
        path: lock_path.to_path_buf(),
        reason: "lock has no parent directory",
    })?;
    let base = lock_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CoreError::UnsafePath {
            path: lock_path.to_path_buf(),
            reason: "lock name is not UTF-8",
        })?;
    let owner = tempfile::Builder::new()
        .prefix(&format!("{base}.owner."))
        .tempdir_in(parent)
        .map_err(|error| CoreError::io("create lock owner directory", parent, error))?
        .keep();
    let pid_path = owner.join("pid");
    let mut pid_file = File::create(&pid_path)
        .map_err(|error| CoreError::io("create lock owner PID", &pid_path, error))?;
    writeln!(pid_file, "{}", current_pid())
        .map_err(|error| CoreError::io("write lock owner PID", &pid_path, error))?;
    pid_file
        .sync_all()
        .map_err(|error| CoreError::io("flush lock owner PID", &pid_path, error))?;
    match symlink(&owner, lock_path) {
        Ok(()) => {
            let actual = owner_target(lock_path)?;
            if actual == owner && read_owner_pid(lock_path) == Some(current_pid()) {
                Ok(Some(owner))
            } else {
                let _ = remove_owner(lock_path);
                Ok(None)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&pid_path);
            let _ = fs::remove_dir(&owner);
            Ok(None)
        }
        Err(error) => {
            let _ = fs::remove_file(&pid_path);
            let _ = fs::remove_dir(&owner);
            Err(CoreError::io("publish lock owner", lock_path, error))
        }
    }
}

/// An acquired owner-directory lock released only by the verified owner.
#[derive(Debug)]
pub struct DirectoryLock {
    path: PathBuf,
    owner: PathBuf,
    released: bool,
}

impl DirectoryLock {
    /// Try to acquire once, safely recovering a provably dead owner under a
    /// serialized `.steal` claim.
    pub fn try_acquire(path: impl AsRef<Path>, processes: &impl ProcessProbe) -> Result<Self> {
        Self::try_acquire_inner(path.as_ref(), processes, 0)
    }

    fn try_acquire_inner(
        path: &Path,
        processes: &impl ProcessProbe,
        steal_depth: usize,
    ) -> Result<Self> {
        if let Some(owner) = try_publish_owner(path)? {
            return Ok(Self {
                path: path.to_path_buf(),
                owner,
                released: false,
            });
        }
        let observed_target =
            if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                match owner_target(path) {
                    Ok(target) => Some(target),
                    Err(_) => {
                        return Err(CoreError::LockHeld {
                            owner: "owner changed during observation".to_owned(),
                        });
                    }
                }
            } else {
                None
            };
        let observed_pid = read_owner_pid(path);
        if observed_pid.is_some_and(|pid| processes.is_alive(pid)) {
            return Err(CoreError::LockHeld {
                owner: observed_pid.expect("present").to_string(),
            });
        }
        if observed_pid.is_none() {
            let age = fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .unwrap_or(Duration::ZERO);
            if age < Duration::from_secs(2) {
                return Err(CoreError::LockHeld {
                    owner: "mid-acquire owner".to_owned(),
                });
            }
        }
        if steal_depth >= 8 {
            return Err(CoreError::LockHeld {
                owner: "stale-lock recovery depth exceeded".to_owned(),
            });
        }
        let mut steal_name = path.as_os_str().to_os_string();
        steal_name.push(".steal");
        let steal_path = PathBuf::from(steal_name);
        let steal_lock = Self::try_acquire_inner(&steal_path, processes, steal_depth + 1)?;
        let same_target = match &observed_target {
            Some(target) => owner_target(path).is_ok_and(|actual| actual == *target),
            None => fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()),
        };
        let same_pid = read_owner_pid(path) == observed_pid;
        let still_dead = observed_pid.is_none_or(|pid| !processes.is_alive(pid));
        if same_target && same_pid && still_dead {
            remove_owner(path)?;
        }
        let acquired = try_publish_owner(path)?;
        drop(steal_lock);
        let owner = acquired.ok_or_else(|| CoreError::LockHeld {
            owner: read_owner_pid(path).map_or_else(|| "unknown".to_owned(), |pid| pid.to_string()),
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            owner,
            released: false,
        })
    }

    /// Wait up to a bounded timeout for acquisition, retrying only lock-held
    /// observations and never hiding structural or I/O failures.
    pub fn acquire_wait(
        path: impl AsRef<Path>,
        processes: &impl ProcessProbe,
        timeout: Duration,
    ) -> Result<Self> {
        let path = path.as_ref();
        let started = std::time::Instant::now();
        loop {
            match Self::try_acquire(path, processes) {
                Ok(lock) => return Ok(lock),
                Err(CoreError::LockHeld { .. }) if started.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Return the published lock path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Publish one known owner metadata file before exposing the lock to a
    /// consumer that requires more than the PID claim.
    pub fn publish_metadata(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if !matches!(name, "mx-home" | "pid-identity" | "watcher-path") {
            return Err(CoreError::InvalidIdentifier {
                kind: "lock metadata name",
                value: name.to_owned(),
            });
        }
        crate::filesystem::atomic_replace(self.owner.join(name), bytes, 0o600)
    }

    /// Release only if the path still points to this owner and this PID.
    pub fn release(mut self) -> Result<()> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<()> {
        if self.released {
            return Ok(());
        }
        let ours = owner_target(&self.path).is_ok_and(|actual| actual == self.owner)
            && read_owner_pid(&self.path) == Some(current_pid());
        if ours {
            remove_owner(&self.path)?;
        }
        self.released = true;
        Ok(())
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

/// Epoch seconds for deterministic lock records.
#[must_use]
pub fn epoch_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;

    use std::os::unix::fs::symlink;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::{
        DirectoryLock, HolderProbe, HolderStatus, LsofProbe, epoch_seconds,
        git_lock_is_provably_stale,
    };
    use crate::error::{CoreError, Result};
    use crate::process::{AncestryRow, ProcessIdentity, ProcessProbe};

    #[derive(Clone, Default)]
    struct FakeProcesses(Arc<Mutex<HashMap<u32, bool>>>);

    struct FakeHolder(HolderStatus);

    impl HolderProbe for FakeHolder {
        fn holder_status(&self, _path: &Path) -> HolderStatus {
            self.0
        }
    }

    impl ProcessProbe for FakeProcesses {
        fn is_alive(&self, pid: u32) -> bool {
            pid == std::process::id()
                || self
                    .0
                    .lock()
                    .expect("processes")
                    .get(&pid)
                    .copied()
                    .unwrap_or(false)
        }

        fn identity(&self, pid: u32) -> Result<ProcessIdentity> {
            Err(CoreError::InvalidIdentifier {
                kind: "fixture PID",
                value: pid.to_string(),
            })
        }

        fn ancestry_row(&self, pid: u32) -> Result<AncestryRow> {
            Err(CoreError::InvalidIdentifier {
                kind: "fixture PID",
                value: pid.to_string(),
            })
        }
    }

    #[test]
    fn concurrent_acquisition_has_one_winner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("lock");
        let barrier = Arc::new(Barrier::new(8));
        let processes = FakeProcesses::default();
        let winners = Arc::new(Mutex::new(0));
        thread::scope(|scope| {
            for _ in 0..8 {
                let barrier = Arc::clone(&barrier);
                let path = path.clone();
                let processes = processes.clone();
                let winners = Arc::clone(&winners);
                scope.spawn(move || {
                    barrier.wait();
                    if let Ok(lock) = DirectoryLock::try_acquire(path, &processes) {
                        *winners.lock().expect("winners") += 1;
                        barrier.wait();
                        drop(lock);
                    } else {
                        barrier.wait();
                    }
                });
            }
        });
        assert_eq!(*winners.lock().expect("winners"), 1);
    }

    #[test]
    fn git_lock_staleness_fails_closed_on_holder_uncertainty() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock = temp.path().join("index.lock");
        std::fs::write(&lock, b"").expect("lock");
        let future = SystemTime::now() + Duration::from_secs(60);
        assert!(
            git_lock_is_provably_stale(
                &lock,
                Some(temp.path()),
                Duration::from_secs(2),
                future,
                &FakeHolder(HolderStatus::Clear)
            )
            .expect("stale proof")
        );
        for status in [HolderStatus::Held, HolderStatus::Unknown] {
            assert!(
                !git_lock_is_provably_stale(
                    &lock,
                    Some(temp.path()),
                    Duration::from_secs(2),
                    future,
                    &FakeHolder(status)
                )
                .expect("fail closed")
            );
        }
        assert!(
            !git_lock_is_provably_stale(
                temp.path().join("absent.lock"),
                None,
                Duration::ZERO,
                future,
                &FakeHolder(HolderStatus::Clear),
            )
            .expect("absent is not stale")
        );
        assert!(
            !git_lock_is_provably_stale(
                &lock,
                Some(temp.path()),
                Duration::from_secs(120),
                SystemTime::now(),
                &FakeHolder(HolderStatus::Clear),
            )
            .expect("young lock")
        );
        assert_eq!(epoch_seconds(SystemTime::UNIX_EPOCH), 0);
        assert_eq!(
            epoch_seconds(SystemTime::UNIX_EPOCH - Duration::from_secs(1)),
            0
        );
        assert!(matches!(
            LsofProbe.holder_status(&temp.path().join("absent")),
            HolderStatus::Clear | HolderStatus::Unknown
        ));
    }

    #[test]
    fn owner_metadata_release_wait_and_stale_recovery_are_covered() {
        let temp = tempfile::tempdir().expect("tempdir");
        let processes = FakeProcesses::default();
        let path = temp.path().join("lock");
        let lock =
            DirectoryLock::acquire_wait(&path, &processes, Duration::ZERO).expect("wait acquire");
        assert_eq!(lock.path(), path);
        lock.publish_metadata("mx-home", b"/tmp/home\n")
            .expect("home metadata");
        lock.publish_metadata("pid-identity", b"identity\n")
            .expect("identity metadata");
        lock.publish_metadata("watcher-path", b"watcher\n")
            .expect("watcher metadata");
        assert!(lock.publish_metadata("unexpected", b"no").is_err());
        lock.release().expect("release");
        assert!(!path.exists());

        let owner = temp.path().join("relative-owner");
        std::fs::create_dir(&owner).expect("relative owner");
        std::fs::write(owner.join("pid"), b"42\n").expect("dead pid");
        symlink("relative-owner", &path).expect("relative owner link");
        let recovered = DirectoryLock::try_acquire(&path, &processes).expect("recover link");
        drop(recovered);
        assert!(!path.exists());
        assert!(!owner.exists());

        std::fs::create_dir(&path).expect("legacy lock directory");
        std::fs::write(path.join("pid"), b"43\n").expect("legacy dead pid");
        let recovered = DirectoryLock::try_acquire(&path, &processes).expect("recover directory");
        drop(recovered);
        assert!(!path.exists());

        let live_owner = temp.path().join("live-owner");
        std::fs::create_dir(&live_owner).expect("live owner");
        std::fs::write(live_owner.join("pid"), format!("{}\n", std::process::id()))
            .expect("live pid");
        symlink(&live_owner, &path).expect("live owner link");
        assert!(DirectoryLock::acquire_wait(&path, &processes, Duration::ZERO).is_err());
    }
}

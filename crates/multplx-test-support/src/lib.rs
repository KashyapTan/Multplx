//! Deterministic fixtures shared by Rust-native and differential tests.
//!
//! These helpers create isolated operational homes, deterministic clocks and
//! process identities, fake executables, exact filesystem snapshots, and
//! bounded test-child cleanup without adding seams to production commands.

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

/// An isolated Multplx operational home rooted in a self-cleaning directory.
#[derive(Debug)]
pub struct TempHome {
    root: TempDir,
}

impl TempHome {
    /// Creates an empty home with the four top-level runtime directories.
    pub fn new() -> io::Result<Self> {
        let root = tempfile::tempdir()?;
        for name in ["config", "data", "projects", "state"] {
            fs::create_dir(root.path().join(name))?;
        }
        Ok(Self { root })
    }

    /// Returns the home root.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.root.path()
    }

    /// Returns a path below the home without creating it.
    #[must_use]
    pub fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.path().join(path)
    }
}

/// A deterministic clock that advances only when instructed by a test.
#[derive(Clone, Debug)]
pub struct ManualClock {
    now: Arc<Mutex<SystemTime>>,
}

impl ManualClock {
    /// Creates a clock fixed at `now`.
    #[must_use]
    pub fn new(now: SystemTime) -> Self {
        Self {
            now: Arc::new(Mutex::new(now)),
        }
    }

    /// Returns the current deterministic time.
    #[must_use]
    pub fn now(&self) -> SystemTime {
        *self.now.lock().expect("manual clock mutex poisoned")
    }

    /// Advances the clock by an exact duration.
    pub fn advance(&self, duration: Duration) {
        let mut now = self.now.lock().expect("manual clock mutex poisoned");
        *now = now
            .checked_add(duration)
            .expect("manual clock advance overflowed SystemTime");
    }
}

/// A deterministic process identity fixture.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    /// Fixture PID.
    pub pid: u32,
    /// Stable fixture start marker used to distinguish PID reuse.
    pub start_marker: String,
}

impl ProcessIdentity {
    /// Creates an identity with explicit fixture values.
    #[must_use]
    pub fn new(pid: u32, start_marker: impl Into<String>) -> Self {
        Self {
            pid,
            start_marker: start_marker.into(),
        }
    }
}

/// A directory of test-controlled executables suitable for prepending to PATH.
#[derive(Debug)]
pub struct FakePath {
    directory: PathBuf,
}

impl FakePath {
    /// Creates a `fakebin` directory below `parent`.
    pub fn new(parent: impl AsRef<Path>) -> io::Result<Self> {
        let directory = parent.as_ref().join("fakebin");
        fs::create_dir(&directory)?;
        Ok(Self { directory })
    }

    /// Returns the fake executable directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.directory
    }

    /// Writes an executable POSIX-shell fixture with mode `0755`.
    pub fn write_shell(&self, name: &str, body: &str) -> io::Result<PathBuf> {
        let path = self.directory.join(name);
        let script = format!("#!/bin/sh\n{body}\n");
        fs::write(&path, script.as_bytes())?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        Ok(path)
    }
}

/// Returns the Unix permission bits for an existing path without following a
/// final symlink.
pub fn permission_mode(path: impl AsRef<Path>) -> io::Result<u32> {
    Ok(fs::symlink_metadata(path)?.permissions().mode() & 0o7777)
}

/// The kind of one path in a filesystem manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind {
    /// A directory.
    Directory,
    /// A regular file.
    File,
    /// A symbolic link.
    Symlink,
    /// A platform file type not otherwise classified.
    Other,
}

/// One exact, deterministically ordered filesystem observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Human-readable relative path.
    pub path: String,
    /// Exact Unix path bytes encoded as lowercase hexadecimal.
    pub path_hex: String,
    /// Observed file type.
    pub kind: EntryKind,
    /// Unix permission bits.
    pub mode: u32,
    /// Exact file length for regular files.
    pub size: Option<u64>,
    /// SHA-256 of regular-file bytes.
    pub sha256: Option<String>,
    /// Exact regular-file bytes encoded as lowercase hexadecimal.
    pub content_hex: Option<String>,
    /// Exact symlink-target bytes encoded as lowercase hexadecimal.
    pub target_hex: Option<String>,
}

/// A deterministic recursive snapshot of a filesystem tree.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FilesystemManifest {
    /// Entries sorted by exact relative path bytes.
    pub entries: Vec<ManifestEntry>,
}

impl FilesystemManifest {
    /// Captures all descendants of `root` without following symlinks.
    pub fn capture(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = root.as_ref();
        let mut paths = Vec::new();
        collect_paths(root, root, &mut paths)?;
        paths.sort_by(|left, right| {
            left.as_os_str()
                .as_bytes()
                .cmp(right.as_os_str().as_bytes())
        });

        let entries = paths
            .into_iter()
            .map(|relative| capture_entry(root, &relative))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self { entries })
    }
}

fn collect_paths(root: &Path, directory: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_path_buf();
        paths.push(relative);
        if entry.file_type()?.is_dir() {
            collect_paths(root, &path, paths)?;
        }
    }
    Ok(())
}

fn capture_entry(root: &Path, relative: &Path) -> io::Result<ManifestEntry> {
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path)?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    };

    let (size, sha256, content_hex) = if file_type.is_file() {
        let mut content = Vec::new();
        File::open(&path)?.read_to_end(&mut content)?;
        let digest = Sha256::digest(&content);
        (Some(metadata.len()), Some(hex(digest)), Some(hex(&content)))
    } else {
        (None, None, None)
    };
    let target_hex = if file_type.is_symlink() {
        Some(hex(fs::read_link(&path)?.as_os_str().as_bytes()))
    } else {
        None
    };

    Ok(ManifestEntry {
        path: relative.to_string_lossy().into_owned(),
        path_hex: hex(relative.as_os_str().as_bytes()),
        kind,
        mode: metadata.permissions().mode() & 0o7777,
        size,
        sha256,
        content_hex,
        target_hex,
    })
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// A test-owned child process launched in its own process group.
///
/// Cleanup sends TERM to the group, waits only for the configured grace period,
/// then sends KILL to the same group and reaps the direct child.
#[derive(Debug)]
pub struct ProcessFixture {
    child: Option<Child>,
    cleanup_timeout: Duration,
}

impl ProcessFixture {
    /// Spawns a child with inherited environment and isolated process group.
    pub fn spawn(command: &mut Command, cleanup_timeout: Duration) -> io::Result<Self> {
        use std::os::unix::process::CommandExt;

        command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command.spawn()?;
        Ok(Self {
            child: Some(child),
            cleanup_timeout,
        })
    }

    /// Returns the direct child's PID while it is owned by this fixture.
    #[must_use]
    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// Stops and reaps the owned process group.
    pub fn stop(&mut self) -> io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| io::Error::other("process fixture already stopped"))?;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let _term_sent = signal_group(child.id(), "TERM")?;
        let deadline = std::time::Instant::now() + self.cleanup_timeout;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            if std::time::Instant::now() >= deadline {
                let _kill_sent = signal_group(child.id(), "KILL")?;
                return child.wait();
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for ProcessFixture {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn signal_group(pid: u32, signal: &str) -> io::Result<bool> {
    let status = Command::new("/bin/kill")
        .args([format!("-{signal}"), format!("-{pid}")])
        .status()?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::UNIX_EPOCH;

    #[test]
    fn temporary_homes_have_isolated_runtime_layouts() {
        let first = TempHome::new().expect("first home");
        let second = TempHome::new().expect("second home");

        assert_ne!(first.path(), second.path());
        for name in ["config", "data", "projects", "state"] {
            assert!(first.join(name).is_dir());
            assert!(second.join(name).is_dir());
        }
    }

    #[test]
    fn fake_path_writes_executable_fixtures() {
        let home = TempHome::new().expect("home");
        let fake_path = FakePath::new(home.path()).expect("fake PATH");
        let executable = fake_path
            .write_shell("example", "printf 'fixture\\n'")
            .expect("fake executable");

        assert_eq!(permission_mode(&executable).expect("mode"), 0o755);
        let output = Command::new(executable).output().expect("run fixture");
        assert_eq!(output.stdout, b"fixture\n");
    }

    #[test]
    fn manual_clock_and_process_identity_are_deterministic() {
        let clock = ManualClock::new(UNIX_EPOCH + Duration::from_secs(10));
        let clone = clock.clone();
        clock.advance(Duration::from_millis(250));

        assert_eq!(clone.now(), UNIX_EPOCH + Duration::from_millis(10_250));
        assert_eq!(
            ProcessIdentity::new(42, "boot:7"),
            ProcessIdentity::new(42, "boot:7")
        );
    }

    #[test]
    fn manifest_preserves_order_bytes_modes_and_symlinks() {
        let home = TempHome::new().expect("home");
        fs::write(home.join("state/z"), b"last\n").expect("z fixture");
        fs::write(home.join("state/a"), b"first\0byte").expect("a fixture");
        fs::set_permissions(home.join("state/a"), fs::Permissions::from_mode(0o600))
            .expect("fixture mode");
        symlink("a", home.join("state/link")).expect("fixture symlink");

        let manifest = FilesystemManifest::capture(home.join("state")).expect("manifest");
        let paths: Vec<&str> = manifest
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();

        assert_eq!(paths, ["a", "link", "z"]);
        assert_eq!(manifest.entries[0].mode, 0o600);
        assert_eq!(
            manifest.entries[0].content_hex.as_deref(),
            Some("66697273740062797465")
        );
        assert_eq!(manifest.entries[1].target_hex.as_deref(), Some("61"));
        assert_eq!(
            serde_json::to_string(&manifest).expect("serialize manifest"),
            serde_json::to_string(&manifest).expect("serialize manifest again")
        );
    }

    #[test]
    fn process_fixture_bounds_cleanup_and_reaps_the_child() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "trap '' TERM; while :; do sleep 1; done"]);
        let mut fixture =
            ProcessFixture::spawn(&mut command, Duration::from_millis(25)).expect("spawn fixture");
        let pid = fixture.id().expect("fixture pid");

        let status = fixture.stop().expect("bounded cleanup");

        assert!(!status.success());
        let probe = Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .expect("probe child");
        assert!(!probe.success());
    }

    #[test]
    fn process_fixture_reaps_a_child_that_already_exited() {
        let mut command = Command::new("/usr/bin/true");
        let mut fixture =
            ProcessFixture::spawn(&mut command, Duration::from_millis(25)).expect("spawn fixture");
        thread::sleep(Duration::from_millis(10));

        let status = fixture.stop().expect("reap completed child");

        assert!(status.success());
    }
}

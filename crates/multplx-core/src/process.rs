//! Portable PID identity and injected process probes.
//!
//! Linux identities use `/proc/<pid>/stat` field 22 plus the exact NUL-delimited
//! command line. macOS and other supported Unix hosts use locale-pinned `ps`
//! start time plus command, matching `bin/mx-wake-lib.sh`.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

const MAX_PS_OUTPUT: usize = 64 * 1024;

/// Stable evidence that distinguishes one process lifetime from PID reuse.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    /// Numeric Unix PID.
    pub pid: u32,
    /// Portable identity text persisted by current lock records.
    pub marker: String,
}

/// Read-only process observation used by locks and session identity.
pub trait ProcessProbe {
    /// Return whether a PID is currently observable.
    fn is_alive(&self, pid: u32) -> bool;
    /// Resolve the stable identity for a live PID.
    fn identity(&self, pid: u32) -> Result<ProcessIdentity>;
    /// Return the parent PID, command name, and argument text.
    fn ancestry_row(&self, pid: u32) -> Result<AncestryRow>;
}

/// One process-table row used during bounded ancestry discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AncestryRow {
    /// Parent PID.
    pub parent_pid: u32,
    /// Executable command name.
    pub command: String,
    /// Full argument display from `ps`.
    pub arguments: String,
}

/// Signal and bounded-disappearance boundary for an already verified process.
pub trait ProcessTerminator {
    /// Recheck the exact identity and send TERM.
    fn terminate(&mut self, process: &ProcessIdentity) -> Result<()>;
    /// Wait until the exact process lifetime disappears or the timeout expires.
    fn wait_gone(&mut self, process: &ProcessIdentity, timeout: Duration) -> bool;
}

/// Host-backed identity-bound process termination.
#[derive(Clone, Debug, Default)]
pub struct SystemProcessTerminator {
    probe: SystemProcessProbe,
}

impl ProcessTerminator for SystemProcessTerminator {
    fn terminate(&mut self, process: &ProcessIdentity) -> Result<()> {
        let current = self.probe.identity(process.pid)?;
        if current != *process {
            return Err(CoreError::Command {
                command: "terminate process".to_owned(),
                reason: "PID identity changed before signaling".to_owned(),
            });
        }
        let raw = i32::try_from(process.pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or(CoreError::InvalidIdentifier {
                kind: "PID",
                value: process.pid.to_string(),
            })?;
        kill_process(raw, Signal::TERM).map_err(|error| CoreError::Command {
            command: "terminate process".to_owned(),
            reason: error.to_string(),
        })
    }

    fn wait_gone(&mut self, process: &ProcessIdentity, timeout: Duration) -> bool {
        let started = Instant::now();
        loop {
            if !self.probe.is_alive(process.pid) {
                return true;
            }
            if self
                .probe
                .identity(process.pid)
                .is_ok_and(|current| current != *process)
            {
                return true;
            }
            if started.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Child process whose owner always reaps it, including during unwinding.
#[derive(Debug)]
pub struct OwnedChild {
    child: Option<Child>,
}

/// Result of bounded cooperative child termination.
#[derive(Debug)]
pub struct OwnedChildExit {
    /// Final wait status after the child was reaped.
    pub status: ExitStatus,
    /// True when the grace interval expired and KILL was required.
    pub forced: bool,
}

impl OwnedChild {
    /// Spawn a child whose lifecycle is now owned by this value.
    pub fn spawn(command: &mut Command) -> Result<Self> {
        let child = command.spawn().map_err(|error| CoreError::Command {
            command: format!("{:?}", command.get_program()),
            reason: error.to_string(),
        })?;
        Ok(Self { child: Some(child) })
    }

    /// Return the still-owned child PID.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.child.as_ref().expect("owned child is present").id()
    }

    /// Wait normally and reap the child.
    pub fn wait(mut self) -> Result<ExitStatus> {
        let status = self
            .child
            .as_mut()
            .expect("owned child is present")
            .wait()
            .map_err(|error| CoreError::Command {
                command: "wait for owned child".to_owned(),
                reason: error.to_string(),
            })?;
        self.child.take();
        Ok(status)
    }

    /// Send TERM, wait for the bounded grace interval, then KILL and reap.
    pub fn terminate_and_reap(mut self, timeout: Duration) -> Result<OwnedChildExit> {
        let child = self.child.as_mut().expect("owned child is present");
        if let Some(status) = child.try_wait().map_err(|error| CoreError::Command {
            command: "inspect owned child".to_owned(),
            reason: error.to_string(),
        })? {
            self.child.take();
            return Ok(OwnedChildExit {
                status,
                forced: false,
            });
        }
        if let Err(error) = kill_process(Pid::from_child(child), Signal::TERM) {
            if let Some(status) = child.try_wait().map_err(|wait_error| CoreError::Command {
                command: "inspect owned child after TERM".to_owned(),
                reason: wait_error.to_string(),
            })? {
                self.child.take();
                return Ok(OwnedChildExit {
                    status,
                    forced: false,
                });
            }
            return Err(CoreError::Command {
                command: "terminate owned child".to_owned(),
                reason: error.to_string(),
            });
        }
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().map_err(|error| CoreError::Command {
                command: "inspect owned child".to_owned(),
                reason: error.to_string(),
            })? {
                self.child.take();
                return Ok(OwnedChildExit {
                    status,
                    forced: false,
                });
            }
            if started.elapsed() >= timeout {
                child.kill().map_err(|error| CoreError::Command {
                    command: "kill owned child".to_owned(),
                    reason: error.to_string(),
                })?;
                let status = child.wait().map_err(|error| CoreError::Command {
                    command: "reap owned child".to_owned(),
                    reason: error.to_string(),
                })?;
                self.child.take();
                return Ok(OwnedChildExit {
                    status,
                    forced: true,
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Host-backed process observations.
#[derive(Clone, Debug)]
pub struct SystemProcessProbe {
    proc_root: PathBuf,
}

impl Default for SystemProcessProbe {
    fn default() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
        }
    }
}

impl SystemProcessProbe {
    /// Override `/proc` for deterministic Linux fixtures.
    #[must_use]
    pub fn with_proc_root(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
        }
    }

    fn ps_output<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new("ps")
            .args(args)
            .env("LC_ALL", "C")
            .output()
            .map_err(|error| CoreError::Command {
                command: "ps".to_owned(),
                reason: error.to_string(),
            })?;
        if !output.status.success() || output.stdout.len() > MAX_PS_OUTPUT {
            return Err(CoreError::Command {
                command: "ps".to_owned(),
                reason: format!("exit {:?} or oversized output", output.status.code()),
            });
        }
        String::from_utf8(output.stdout).map_err(|_| CoreError::Command {
            command: "ps".to_owned(),
            reason: "non-UTF-8 output".to_owned(),
        })
    }

    fn linux_identity(&self, pid: u32) -> Result<Option<ProcessIdentity>> {
        if !cfg!(target_os = "linux") {
            return Ok(None);
        }
        let directory = self.proc_root.join(pid.to_string());
        let stat_path = directory.join("stat");
        let cmdline_path = directory.join("cmdline");
        let stat = match fs::read_to_string(&stat_path) {
            Ok(stat) => stat,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(CoreError::io("read process stat", stat_path, error)),
        };
        let after_command = stat.rsplit_once(')').ok_or(CoreError::MalformedRecord {
            kind: "Linux process stat",
            reason: "missing command delimiter",
        })?;
        let fields: Vec<&str> = after_command.1.split_whitespace().collect();
        let starttime = fields.get(19).ok_or(CoreError::MalformedRecord {
            kind: "Linux process stat",
            reason: "missing starttime field",
        })?;
        if !starttime.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(CoreError::MalformedRecord {
                kind: "Linux process stat",
                reason: "starttime is not numeric",
            });
        }
        let cmdline = fs::read(&cmdline_path)
            .map_err(|error| CoreError::io("read process command line", cmdline_path, error))?;
        if cmdline.is_empty() || cmdline.len() > MAX_PS_OUTPUT {
            return Err(CoreError::MalformedRecord {
                kind: "Linux process command line",
                reason: "empty or oversized command line",
            });
        }
        let hex = cmdline
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Some(ProcessIdentity {
            pid,
            marker: format!("linux-starttime={starttime} cmdline-hex={hex}"),
        }))
    }
}

impl ProcessProbe for SystemProcessProbe {
    fn is_alive(&self, pid: u32) -> bool {
        self.ps_output(["-p", &pid.to_string(), "-o", "pid="])
            .is_ok_and(|output| !output.trim().is_empty())
    }

    fn identity(&self, pid: u32) -> Result<ProcessIdentity> {
        if pid == 0 {
            return Err(CoreError::InvalidIdentifier {
                kind: "PID",
                value: pid.to_string(),
            });
        }
        if let Some(identity) = self.linux_identity(pid)? {
            return Ok(identity);
        }
        let output = self.ps_output(["-p", &pid.to_string(), "-o", "lstart=", "-o", "command="])?;
        let marker = output.trim_start().trim_end_matches('\n').to_owned();
        if marker.is_empty() {
            return Err(CoreError::MalformedRecord {
                kind: "process identity",
                reason: "empty ps identity",
            });
        }
        Ok(ProcessIdentity { pid, marker })
    }

    fn ancestry_row(&self, pid: u32) -> Result<AncestryRow> {
        let output = self.ps_output([
            "-p",
            &pid.to_string(),
            "-o",
            "ppid=",
            "-o",
            "comm=",
            "-o",
            "args=",
        ])?;
        let line = output.lines().next().ok_or(CoreError::MalformedRecord {
            kind: "process ancestry",
            reason: "empty ps row",
        })?;
        let mut fields = line.split_whitespace();
        let parent_pid = fields
            .next()
            .and_then(|field| field.parse::<u32>().ok())
            .ok_or(CoreError::MalformedRecord {
                kind: "process ancestry",
                reason: "invalid parent PID",
            })?;
        let command = fields
            .next()
            .ok_or(CoreError::MalformedRecord {
                kind: "process ancestry",
                reason: "missing command",
            })?
            .to_owned();
        let arguments = fields.collect::<Vec<_>>().join(" ");
        Ok(AncestryRow {
            parent_pid,
            command,
            arguments,
        })
    }
}

/// Resolve a process identity using the host process table.
pub fn identity(pid: u32) -> Result<ProcessIdentity> {
    SystemProcessProbe::default().identity(pid)
}

/// Return a portable mtime age, saturating future timestamps at zero.
pub fn path_age(path: impl AsRef<Path>, now_epoch: u64) -> Result<u64> {
    use std::time::UNIX_EPOCH;

    let path = path.as_ref();
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| CoreError::io("read path mtime", path, error))?;
    let epoch = modified
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CoreError::MalformedRecord {
            kind: "mtime",
            reason: "mtime predates Unix epoch",
        })?
        .as_secs();
    Ok(now_epoch.saturating_sub(epoch))
}

#[cfg(test)]
mod tests {
    #[test]
    fn system_probe_and_terminator_validate_live_identity_without_signaling_mismatch() {
        use std::fs;
        use std::time::{Duration, SystemTime, UNIX_EPOCH};

        use super::{
            ProcessIdentity, ProcessProbe, ProcessTerminator, SystemProcessProbe,
            SystemProcessTerminator, identity, path_age,
        };

        let pid = std::process::id();
        let probe = SystemProcessProbe::default();
        let _fixture_probe = SystemProcessProbe::with_proc_root("/definitely/not-proc");
        assert!(probe.is_alive(pid));
        let current = identity(pid).expect("identity");
        assert_eq!(current.pid, pid);
        assert!(!current.marker.is_empty());
        let ancestry = probe.ancestry_row(pid).expect("ancestry");
        assert!(ancestry.parent_pid > 0);
        assert!(!ancestry.command.is_empty());
        assert!(identity(0).is_err());

        let mut terminator = SystemProcessTerminator::default();
        let mismatch = ProcessIdentity {
            pid,
            marker: "wrong-generation".to_owned(),
        };
        assert!(terminator.terminate(&mismatch).is_err());
        assert!(terminator.wait_gone(&mismatch, Duration::ZERO));
        assert!(!terminator.wait_gone(&current, Duration::ZERO));

        let temp = tempfile::tempdir().expect("tempdir");
        let file = temp.path().join("age");
        fs::write(&file, b"").expect("file");
        let modified = fs::metadata(&file)
            .expect("metadata")
            .modified()
            .expect("modified")
            .duration_since(UNIX_EPOCH)
            .expect("epoch")
            .as_secs();
        assert_eq!(
            path_age(&file, modified.saturating_sub(1)).expect("future"),
            0
        );
        assert!(path_age(&file, modified + 1).expect("past") <= 1);
        assert!(path_age(temp.path().join("absent"), modified).is_err());
        let _ = SystemTime::now();
    }

    #[test]
    fn owned_child_wait_and_cooperative_termination_are_reaped() {
        use std::process::Command;
        use std::time::Duration;

        use super::OwnedChild;

        let mut success = Command::new("/usr/bin/true");
        assert!(
            OwnedChild::spawn(&mut success)
                .expect("spawn")
                .wait()
                .expect("wait")
                .success()
        );

        let mut cooperative = Command::new("/bin/sh");
        cooperative.args(["-c", "while :; do sleep 1; done"]);
        let result = OwnedChild::spawn(&mut cooperative)
            .expect("spawn")
            .terminate_and_reap(Duration::from_secs(1))
            .expect("terminate");
        assert!(!result.forced);
        assert!(!result.status.success());

        let mut missing = Command::new("/definitely/missing/mx-child");
        assert!(OwnedChild::spawn(&mut missing).is_err());
    }

    #[test]
    fn dropping_owned_child_kills_and_reaps_it() {
        use std::process::Command;
        use std::time::{Duration, Instant};

        use super::{OwnedChild, ProcessProbe, SystemProcessProbe};

        let mut command = Command::new("/bin/sh");
        command.args(["-c", "while :; do :; done"]);
        let child = OwnedChild::spawn(&mut command).expect("spawn");
        let pid = child.id();
        drop(child);
        let probe = SystemProcessProbe::default();
        let started = Instant::now();
        while probe.is_alive(pid) && started.elapsed() < Duration::from_secs(2) {
            std::thread::yield_now();
        }
        assert!(!probe.is_alive(pid));
    }

    #[test]
    fn owned_child_escalates_and_reaps_a_term_resistant_process() {
        use std::fs;
        use std::process::Command;
        use std::time::{Duration, Instant};

        use super::OwnedChild;

        let temp = tempfile::tempdir().expect("tempdir");
        let ready = temp.path().join("ready");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "trap '' TERM; : > \"$1\"; while :; do :; done", "sh"])
            .arg(&ready);
        let child = OwnedChild::spawn(&mut command).expect("spawn");
        let started = Instant::now();
        while !ready.is_file() && started.elapsed() < Duration::from_secs(2) {
            std::thread::yield_now();
        }
        assert!(fs::metadata(&ready).is_ok(), "child did not become ready");
        let result = child
            .terminate_and_reap(Duration::from_millis(25))
            .expect("terminate");
        assert!(result.forced);
        assert!(!result.status.success());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_identity_uses_starttime_and_exact_cmdline_bytes() {
        use std::fs;

        use super::{ProcessProbe, SystemProcessProbe};

        let temp = tempfile::tempdir().expect("tempdir");
        let process = temp.path().join("42");
        fs::create_dir(&process).expect("process directory");
        let mut fields = vec!["S"; 20];
        fields[19] = "12345";
        fs::write(
            process.join("stat"),
            format!("42 (name with ) paren) {}\n", fields.join(" ")),
        )
        .expect("stat");
        fs::write(process.join("cmdline"), b"mx\0watch\0").expect("cmdline");
        let identity = SystemProcessProbe::with_proc_root(temp.path())
            .identity(42)
            .expect("identity");
        assert_eq!(
            identity.marker,
            "linux-starttime=12345 cmdline-hex=6d7800776174636800"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_identity_rejects_every_malformed_proc_shape() {
        use std::fs;

        use super::{ProcessProbe, SystemProcessProbe};

        let temp = tempfile::tempdir().expect("tempdir");
        let probe = SystemProcessProbe::with_proc_root(temp.path());
        assert!(probe.identity(41).is_err());

        for (pid, stat, cmdline) in [
            (42, "missing delimiter", b"mx\0".as_slice()),
            (43, "43 (name) S 1 2", b"mx\0".as_slice()),
            (
                44,
                "44 (name) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 nope",
                b"mx\0".as_slice(),
            ),
            (
                45,
                "45 (name) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19",
                b"".as_slice(),
            ),
        ] {
            let directory = temp.path().join(pid.to_string());
            fs::create_dir(&directory).expect("process directory");
            fs::write(directory.join("stat"), stat).expect("stat");
            fs::write(directory.join("cmdline"), cmdline).expect("cmdline");
            assert!(probe.identity(pid).is_err(), "PID {pid} should be rejected");
        }

        let directory = temp.path().join("46");
        fs::create_dir(&directory).expect("process directory");
        fs::write(
            directory.join("stat"),
            "46 (name) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19",
        )
        .expect("stat");
        assert!(probe.identity(46).is_err());

        let directory = temp.path().join("47");
        fs::create_dir(&directory).expect("process directory");
        fs::write(
            directory.join("stat"),
            "47 (name) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19",
        )
        .expect("stat");
        fs::write(
            directory.join("cmdline"),
            vec![b'x'; super::MAX_PS_OUTPUT + 1],
        )
        .expect("oversized cmdline");
        assert!(probe.identity(47).is_err());
    }
}

//! Harness-bound session identity and lock state.
//!
//! Ordinary acquisition and status transfer from `bin/mx-session-lock-lib.sh`
//! and `bin/mx-lock.sh`. The exact maintainer-override state machine stays in
//! Portion 10 and plugs in through [`TerminationAuthority`].

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use regex::Regex;

use crate::error::{CoreError, Result};
use crate::filesystem::atomic_replace;
use crate::locks::DirectoryLock;
use crate::process::{ProcessProbe, ProcessTerminator};

const MAX_ANCESTRY_HOPS: usize = 8;

/// Current verified harness command-name matcher.
pub fn harness_regex() -> Regex {
    Regex::new(r"claude|codex|cursor-agent|^pi$").expect("static harness regex")
}

/// Match the executable or an interpreter's executed script, never argument
/// payloads or a directory whose spelling happens to contain a harness name.
pub fn harness_command_matches(command: &str, arguments: &str, matcher: &Regex) -> bool {
    let basename = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command);
    if matcher.is_match(basename) {
        return true;
    }
    let interpreter = basename.to_ascii_lowercase();
    let shell = matches!(interpreter.as_str(), "sh" | "bash" | "zsh" | "dash" | "ksh");
    if !shell && !interpreter.starts_with("node") && !interpreter.starts_with("python") {
        return false;
    }
    let mut words = arguments.split_whitespace().skip(1);
    let script = loop {
        let Some(word) = words.next() else {
            return false;
        };
        // Shell stdin/eval/options do not establish an executed script.
        if shell && word.starts_with('-') {
            return false;
        }
        if matches!(
            word,
            "-c" | "-m" | "-e" | "--eval" | "--module" | "-r" | "--require" | "--import"
        ) {
            return false;
        }
        if matches!(
            word,
            "-u" | "-B" | "-E" | "-I" | "-O" | "-OO" | "--no-warnings" | "--enable-source-maps"
        ) {
            continue;
        }
        if word.starts_with('-') {
            return false;
        }
        break word;
    };
    let script_name = Path::new(script)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if shell
        && !matches!(
            script_name,
            "codex" | "codex-cli" | "claude" | "cursor-agent" | "pi"
        )
    {
        return false;
    }
    matcher.is_match(script_name)
        || script.ends_with("/@anthropic-ai/claude-code/cli.js") && matcher.is_match("claude")
        || script.ends_with("/@mariozechner/pi-coding-agent/dist/cli.js") && matcher.is_match("pi")
}

/// Resolve the first verified harness in a bounded ancestry walk.
pub fn harness_ancestry_pid(
    start_pid: u32,
    processes: &impl ProcessProbe,
    matcher: &Regex,
) -> Result<u32> {
    let mut pid = start_pid;
    for _ in 0..MAX_ANCESTRY_HOPS {
        let row = processes.ancestry_row(pid)?;
        if harness_command_matches(&row.command, &row.arguments, matcher) {
            return Ok(pid);
        }
        if row.parent_pid <= 1 {
            break;
        }
        pid = row.parent_pid;
    }
    Err(CoreError::Command {
        command: "process ancestry".to_owned(),
        reason: "cannot locate harness process in ancestry".to_owned(),
    })
}

/// Return whether a live PID still matches a verified harness command.
pub fn harness_pid_alive(pid: u32, processes: &impl ProcessProbe, matcher: &Regex) -> bool {
    if !processes.is_alive(pid) {
        return false;
    }
    processes
        .ancestry_row(pid)
        .is_ok_and(|row| harness_command_matches(&row.command, &row.arguments, matcher))
}

/// Observable status for `mx-lock status`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionLockStatus {
    /// No regular lock file exists.
    Free,
    /// The record cannot be read or parsed.
    Unreadable,
    /// A live verified harness owns it.
    Held(u32),
    /// The PID is dead or no longer a harness.
    Stale(String),
}

impl SessionLockStatus {
    /// Render exact legacy status text.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Free => "lock: free".to_owned(),
            Self::Unreadable => "lock: unreadable".to_owned(),
            Self::Held(pid) => format!("lock: held by live harness pid {pid}"),
            Self::Stale(pid) => format!("lock: stale (pid {pid} dead or not a harness)"),
        }
    }
}

/// Inspect the plain PID record without mutating it.
pub fn status(
    path: impl AsRef<Path>,
    processes: &impl ProcessProbe,
    matcher: &Regex,
) -> SessionLockStatus {
    let path = path.as_ref();
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SessionLockStatus::Free;
        }
        Err(_) => return SessionLockStatus::Unreadable,
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return SessionLockStatus::Free;
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return SessionLockStatus::Unreadable,
    };
    let trimmed = text.trim_end_matches('\n');
    match trimmed.parse::<u32>() {
        Ok(pid) if harness_pid_alive(pid, processes, matcher) => SessionLockStatus::Held(pid),
        _ => SessionLockStatus::Stale(trimmed.to_owned()),
    }
}

/// Authority supplied by the later maintainer-override domain state machine.
pub trait TerminationAuthority {
    /// Consume the exact grant for a verified competing owner before signaling.
    fn consume(&mut self, request_id: &str, owner_pid: u32) -> Result<()>;
    /// Record the truthful result after the termination attempt.
    fn record_result(&mut self, request_id: &str, succeeded: bool) -> Result<()>;
}

/// Session-lock manager that serializes publication through a core directory lock.
pub struct SessionLock<'a, P: ProcessProbe> {
    path: PathBuf,
    processes: &'a P,
    matcher: Regex,
}

impl<'a, P: ProcessProbe> SessionLock<'a, P> {
    /// Construct a manager for one `state/.lock` path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, processes: &'a P) -> Self {
        Self {
            path: path.into(),
            processes,
            matcher: harness_regex(),
        }
    }

    /// Acquire for `me`, refusing a different live owner.
    pub fn acquire(&self, me: u32) -> Result<()> {
        let claim_path = PathBuf::from(format!("{}.acquire", self.path.display()));
        let _claim = DirectoryLock::try_acquire(&claim_path, self.processes)?;
        if let SessionLockStatus::Held(owner) = status(&self.path, self.processes, &self.matcher)
            && owner != me
        {
            return Err(CoreError::LockHeld {
                owner: owner.to_string(),
            });
        }
        atomic_replace(&self.path, format!("{me}\n").as_bytes(), 0o600)
    }

    /// Consume one exact grant, terminate the verified competing harness, and acquire.
    pub fn terminate_owner_and_acquire(
        &self,
        me: u32,
        request_id: &str,
        authority: &mut impl TerminationAuthority,
        terminator: &mut impl ProcessTerminator,
    ) -> Result<()> {
        let claim_path = PathBuf::from(format!("{}.acquire", self.path.display()));
        let _claim = DirectoryLock::try_acquire(&claim_path, self.processes)?;
        let owner = match status(&self.path, self.processes, &self.matcher) {
            SessionLockStatus::Held(owner) if owner != me => owner,
            _ => {
                return Err(CoreError::Command {
                    command: "session lock".to_owned(),
                    reason: "grant supplied but no different live owner matched it".to_owned(),
                });
            }
        };
        let owner_identity = self.processes.identity(owner)?;
        authority.consume(request_id, owner)?;
        let result = self
            .processes
            .identity(owner)
            .and_then(|current| {
                if current == owner_identity {
                    Ok(())
                } else {
                    Err(CoreError::Command {
                        command: "session lock".to_owned(),
                        reason: "owner PID identity changed before termination".to_owned(),
                    })
                }
            })
            .and_then(|()| terminator.terminate(&owner_identity))
            .and_then(|()| {
                if terminator.wait_gone(&owner_identity, Duration::from_secs(5)) {
                    Ok(())
                } else {
                    Err(CoreError::LockHeld {
                        owner: owner.to_string(),
                    })
                }
            })
            .and_then(|()| atomic_replace(&self.path, format!("{me}\n").as_bytes(), 0o600));
        let record = authority.record_result(request_id, result.is_ok());
        result.and(record)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{
        SessionLock, SessionLockStatus, TerminationAuthority, harness_ancestry_pid, harness_regex,
        status,
    };
    use crate::error::{CoreError, Result};
    use crate::process::{AncestryRow, ProcessIdentity, ProcessProbe, ProcessTerminator};

    struct ReusedProcess {
        generation: Arc<Mutex<u32>>,
    }

    #[derive(Default)]
    struct FixtureProcesses {
        alive: HashSet<u32>,
        rows: HashMap<u32, AncestryRow>,
    }

    impl ProcessProbe for FixtureProcesses {
        fn is_alive(&self, pid: u32) -> bool {
            pid == std::process::id() || self.alive.contains(&pid)
        }

        fn identity(&self, pid: u32) -> Result<ProcessIdentity> {
            if self.is_alive(pid) {
                Ok(ProcessIdentity {
                    pid,
                    marker: format!("fixture-{pid}"),
                })
            } else {
                Err(CoreError::Command {
                    command: "fixture identity".to_owned(),
                    reason: "not alive".to_owned(),
                })
            }
        }

        fn ancestry_row(&self, pid: u32) -> Result<AncestryRow> {
            self.rows.get(&pid).cloned().ok_or(CoreError::Command {
                command: "fixture ancestry".to_owned(),
                reason: "missing row".to_owned(),
            })
        }
    }

    impl ProcessProbe for ReusedProcess {
        fn is_alive(&self, pid: u32) -> bool {
            pid == 42 || pid == std::process::id()
        }

        fn identity(&self, pid: u32) -> Result<ProcessIdentity> {
            Ok(ProcessIdentity {
                pid,
                marker: format!(
                    "generation-{}",
                    *self.generation.lock().expect("generation")
                ),
            })
        }

        fn ancestry_row(&self, _pid: u32) -> Result<AncestryRow> {
            Ok(AncestryRow {
                parent_pid: 1,
                command: "codex".to_owned(),
                arguments: "codex".to_owned(),
            })
        }
    }

    struct ReuseAuthority {
        generation: Arc<Mutex<u32>>,
        recorded_failure: bool,
    }

    impl TerminationAuthority for ReuseAuthority {
        fn consume(&mut self, _request_id: &str, owner_pid: u32) -> Result<()> {
            assert_eq!(owner_pid, 42);
            *self.generation.lock().expect("generation") = 2;
            Ok(())
        }

        fn record_result(&mut self, _request_id: &str, succeeded: bool) -> Result<()> {
            self.recorded_failure = !succeeded;
            Ok(())
        }
    }

    #[derive(Default)]
    struct RefusingTerminator {
        called: bool,
    }

    #[derive(Default)]
    struct RecordingAuthority {
        consumed: Vec<(String, u32)>,
        results: Vec<(String, bool)>,
    }

    impl TerminationAuthority for RecordingAuthority {
        fn consume(&mut self, request_id: &str, owner_pid: u32) -> Result<()> {
            self.consumed.push((request_id.to_owned(), owner_pid));
            Ok(())
        }

        fn record_result(&mut self, request_id: &str, succeeded: bool) -> Result<()> {
            self.results.push((request_id.to_owned(), succeeded));
            Ok(())
        }
    }

    struct RecordingTerminator {
        terminated: Vec<ProcessIdentity>,
        wait_result: bool,
        fail_termination: bool,
    }

    impl ProcessTerminator for RecordingTerminator {
        fn terminate(&mut self, process: &ProcessIdentity) -> Result<()> {
            self.terminated.push(process.clone());
            if self.fail_termination {
                Err(CoreError::Command {
                    command: "fixture terminate".to_owned(),
                    reason: "requested failure".to_owned(),
                })
            } else {
                Ok(())
            }
        }

        fn wait_gone(&mut self, _process: &ProcessIdentity, _timeout: Duration) -> bool {
            self.wait_result
        }
    }

    #[test]
    fn interpreter_identity_ignores_parent_directory_and_payload_names() {
        let matcher = harness_regex();
        let sourced_match = |command: &str, arguments: &str| {
            std::process::Command::new("bash")
                .args([
                    "-c",
                    ". \"$1\"; mx_harness_command_matches \"$2\" \"$3\"",
                    "fixture",
                ])
                .arg(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../bin/mx-session-lock-lib.sh"),
                )
                .args([command, arguments])
                .status()
                .unwrap()
                .success()
        };
        for (command, args) in [
            ("/usr/bin/sleep", "codex 30"),
            ("/usr/bin/python3", "python3 /tmp/mx-codex-proof/handle.py"),
            ("/usr/bin/python3", "python3 /tmp/handle.py codex"),
            ("/usr/bin/python3", "python3 -c codex"),
            ("/usr/bin/python3", "python3 -m codex"),
            ("/usr/bin/python3", "python3 -X codex /tmp/handle.py"),
            ("/usr/bin/node", "node --conditions codex /tmp/handle.js"),
            ("/usr/bin/node", "node --require codex /tmp/handle.js"),
            ("/usr/bin/node", "node --import codex /tmp/handle.js"),
            ("/usr/bin/node", "node /tmp/handle.js --description claude"),
            ("/bin/zsh", "zsh -c codex"),
            ("/bin/bash", "bash -s /tmp/codex"),
            ("/bin/bash", "bash /tmp/handle.sh --description codex"),
            ("/bin/bash", "bash /tmp/mx-codex-helper.sh"),
            ("/bin/sh", "sh /tmp/notcodex"),
            ("/bin/sh", "sh /tmp/mx-codex-proof/handle.sh"),
        ] {
            assert!(
                !super::harness_command_matches(command, args, &matcher),
                "{args}"
            );
            assert!(!sourced_match(command, args), "sourced ABI: {args}");
        }
        for (command, args) in [
            ("/bin/codex", "codex app-server"),
            ("/bin/bash", "bash /tmp/codex --config arbitrary-payload"),
            ("/bin/zsh", "zsh /tmp/codex-cli"),
            ("/bin/sh", "sh /tmp/claude"),
            ("/bin/bash", "bash /tmp/cursor-agent"),
            ("/bin/sh", "sh /tmp/pi"),
            (
                "/usr/bin/node",
                "node /usr/lib/node_modules/@openai/codex/bin/codex.js",
            ),
            (
                "/usr/bin/node",
                "node /usr/lib/node_modules/@anthropic-ai/claude-code/cli.js",
            ),
            (
                "/usr/bin/node",
                "node /usr/lib/node_modules/@mariozechner/pi-coding-agent/dist/cli.js",
            ),
            ("/usr/bin/python3", "python3 -u /tmp/codex_fixture.py"),
            ("/usr/bin/node", "node cursor-agent"),
            (
                "/Library/Frameworks/Python.framework/Python",
                "python3 /tmp/codex.py",
            ),
        ] {
            assert!(
                super::harness_command_matches(command, args, &matcher),
                "{args}"
            );
            assert!(sourced_match(command, args), "sourced ABI: {args}");
        }
    }

    #[test]
    fn ancestry_and_status_cover_verified_and_fail_closed_shapes() {
        let matcher = harness_regex();
        let mut processes = FixtureProcesses::default();
        processes.alive.extend([10, 11, 12, 13]);
        processes.rows.insert(
            10,
            AncestryRow {
                parent_pid: 1,
                command: "/usr/local/bin/codex".to_owned(),
                arguments: "codex".to_owned(),
            },
        );
        processes.rows.insert(
            11,
            AncestryRow {
                parent_pid: 10,
                command: "/bin/zsh".to_owned(),
                arguments: "zsh".to_owned(),
            },
        );
        processes.rows.insert(
            12,
            AncestryRow {
                parent_pid: 1,
                command: "/usr/bin/node".to_owned(),
                arguments: "node cursor-agent".to_owned(),
            },
        );
        processes.rows.insert(
            13,
            AncestryRow {
                parent_pid: 1,
                command: "/bin/zsh".to_owned(),
                arguments: "zsh".to_owned(),
            },
        );
        assert_eq!(
            harness_ancestry_pid(11, &processes, &matcher).expect("walk"),
            10
        );
        assert_eq!(
            harness_ancestry_pid(12, &processes, &matcher).expect("interpreter"),
            12
        );
        assert!(harness_ancestry_pid(13, &processes, &matcher).is_err());
        assert!(harness_ancestry_pid(99, &processes, &matcher).is_err());

        let temp = tempfile::tempdir().expect("tempdir");
        let lock = temp.path().join(".lock");
        assert_eq!(status(&lock, &processes, &matcher), SessionLockStatus::Free);
        fs::create_dir(&lock).expect("directory");
        assert_eq!(status(&lock, &processes, &matcher), SessionLockStatus::Free);
        fs::remove_dir(&lock).expect("remove directory");
        fs::write(&lock, [0xff]).expect("invalid UTF-8");
        assert_eq!(
            status(&lock, &processes, &matcher),
            SessionLockStatus::Unreadable
        );
        fs::write(&lock, b"10\n").expect("held");
        assert_eq!(
            status(&lock, &processes, &matcher),
            SessionLockStatus::Held(10)
        );
        fs::write(&lock, b"13\n").expect("stale");
        assert_eq!(
            status(&lock, &processes, &matcher),
            SessionLockStatus::Stale("13".to_owned())
        );
        fs::remove_file(&lock).expect("remove lock");
        let target = temp.path().join("target");
        fs::write(&target, b"10\n").expect("target");
        symlink(&target, &lock).expect("symlink");
        assert_eq!(status(&lock, &processes, &matcher), SessionLockStatus::Free);

        assert_eq!(SessionLockStatus::Unreadable.render(), "lock: unreadable");
        assert_eq!(
            SessionLockStatus::Held(10).render(),
            "lock: held by live harness pid 10"
        );
    }

    #[test]
    fn ordinary_acquisition_and_authorized_termination_cover_all_outcomes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock = temp.path().join(".lock");
        let mut processes = FixtureProcesses::default();
        processes.alive.insert(42);
        processes.rows.insert(
            42,
            AncestryRow {
                parent_pid: 1,
                command: "codex".to_owned(),
                arguments: "codex".to_owned(),
            },
        );
        processes.rows.insert(
            std::process::id(),
            AncestryRow {
                parent_pid: 1,
                command: "codex".to_owned(),
                arguments: "codex".to_owned(),
            },
        );
        let manager = SessionLock::new(&lock, &processes);
        manager.acquire(std::process::id()).expect("free acquire");
        manager
            .acquire(std::process::id())
            .expect("same owner acquire");
        fs::write(&lock, b"42\n").expect("competing owner");
        assert!(manager.acquire(std::process::id()).is_err());

        let mut authority = RecordingAuthority::default();
        let mut terminator = RecordingTerminator {
            terminated: Vec::new(),
            wait_result: true,
            fail_termination: false,
        };
        manager
            .terminate_owner_and_acquire(
                std::process::id(),
                "request-ok",
                &mut authority,
                &mut terminator,
            )
            .expect("authorized acquire");
        assert_eq!(authority.consumed, [("request-ok".to_owned(), 42)]);
        assert_eq!(authority.results, [("request-ok".to_owned(), true)]);
        assert_eq!(terminator.terminated[0].pid, 42);
        assert_eq!(
            fs::read_to_string(&lock).expect("new owner"),
            format!("{}\n", std::process::id())
        );

        assert!(
            manager
                .terminate_owner_and_acquire(
                    std::process::id(),
                    "request-none",
                    &mut authority,
                    &mut terminator,
                )
                .is_err()
        );
        fs::write(&lock, b"42\n").expect("competing owner again");
        terminator.wait_result = false;
        assert!(
            manager
                .terminate_owner_and_acquire(
                    std::process::id(),
                    "request-timeout",
                    &mut authority,
                    &mut terminator,
                )
                .is_err()
        );
        assert_eq!(
            authority.results.last(),
            Some(&("request-timeout".to_owned(), false))
        );
        fs::write(&lock, b"42\n").expect("competing owner third time");
        terminator.wait_result = true;
        terminator.fail_termination = true;
        assert!(
            manager
                .terminate_owner_and_acquire(
                    std::process::id(),
                    "request-fail",
                    &mut authority,
                    &mut terminator,
                )
                .is_err()
        );
        assert_eq!(
            authority.results.last(),
            Some(&("request-fail".to_owned(), false))
        );
    }

    impl ProcessTerminator for RefusingTerminator {
        fn terminate(&mut self, _process: &ProcessIdentity) -> Result<()> {
            self.called = true;
            Err(CoreError::Command {
                command: "fixture".to_owned(),
                reason: "must not be called".to_owned(),
            })
        }

        fn wait_gone(&mut self, _process: &ProcessIdentity, _timeout: Duration) -> bool {
            false
        }
    }

    #[test]
    fn pid_reuse_after_grant_consumption_prevents_signaling() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock = temp.path().join(".lock");
        std::fs::write(&lock, b"42\n").expect("lock");
        let generation = Arc::new(Mutex::new(1));
        let processes = ReusedProcess {
            generation: Arc::clone(&generation),
        };
        let manager = SessionLock::new(&lock, &processes);
        let mut authority = ReuseAuthority {
            generation,
            recorded_failure: false,
        };
        let mut terminator = RefusingTerminator::default();
        assert!(
            manager
                .terminate_owner_and_acquire(
                    std::process::id(),
                    "request-1",
                    &mut authority,
                    &mut terminator,
                )
                .is_err()
        );
        assert!(!terminator.called);
        assert!(authority.recorded_failure);
        assert_eq!(std::fs::read(&lock).expect("lock remains"), b"42\n");
    }
}

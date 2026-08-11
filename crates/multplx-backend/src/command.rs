//! Bounded argument-array subprocess execution for runtime backends.

use std::ffi::OsString;
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};

/// Default backend subprocess deadline.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default per-stream capture ceiling.
pub const DEFAULT_OUTPUT_LIMIT: usize = 256 * 1024;

/// One explicit subprocess request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRequest {
    /// Program resolved by the operating system.
    pub program: OsString,
    /// Literal argument array.
    pub args: Vec<OsString>,
    /// Explicit environment additions for this child.
    pub env: Vec<(OsString, OsString)>,
    /// Wall-clock deadline.
    pub timeout: Duration,
    /// Independent stdout and stderr byte ceiling.
    pub output_limit: usize,
}

impl CommandRequest {
    /// Construct a request with the backend defaults.
    #[must_use]
    pub fn new(
        program: impl Into<OsString>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }
}

/// Complete bounded child observation.
#[derive(Clone, Debug)]
pub struct CommandOutput {
    /// Child exit status.
    pub status: ExitStatus,
    /// Exact stdout bytes.
    pub stdout: Vec<u8>,
    /// Exact stderr bytes.
    pub stderr: Vec<u8>,
}

/// Structured subprocess failure classes.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// The child could not be started or observed.
    #[error("could not execute {program}: {source}")]
    Io {
        /// Program name.
        program: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The deadline expired and the owned child was killed and reaped.
    #[error("command {program} timed out after {timeout:?}")]
    TimedOut {
        /// Program name.
        program: String,
        /// Enforced timeout.
        timeout: Duration,
    },
    /// A captured stream crossed its explicit limit.
    #[error("command {program} exceeded the {limit}-byte {stream} limit")]
    OutputTooLarge {
        /// Program name.
        program: String,
        /// Stream name.
        stream: &'static str,
        /// Enforced limit.
        limit: usize,
    },
}

/// Injectable subprocess boundary.
pub trait CommandRunner {
    /// Run one request to completion or a bounded failure.
    fn run(&mut self, request: &CommandRequest) -> Result<CommandOutput, CommandError>;
}

/// Real bounded subprocess runner.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemCommandRunner;

fn read_bounded(mut stream: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    stream
        .by_ref()
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn kill_and_reap(child: &mut Child) {
    if let Some(pid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process_group(pid, Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

impl CommandRunner for SystemCommandRunner {
    fn run(&mut self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
        let display = request.program.to_string_lossy().into_owned();
        let mut child = Command::new(&request.program)
            .args(&request.args)
            .envs(request.env.iter().cloned())
            .env("LC_ALL", "C")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| CommandError::Io {
                program: display.clone(),
                source,
            })?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let limit = request.output_limit;
        let stdout_reader = thread::spawn(move || read_bounded(stdout, limit));
        let stderr_reader = thread::spawn(move || read_bounded(stderr, limit));
        let deadline = Instant::now() + request.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    kill_and_reap(&mut child);
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(CommandError::TimedOut {
                        program: display,
                        timeout: request.timeout,
                    });
                }
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(source) => {
                    kill_and_reap(&mut child);
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(CommandError::Io {
                        program: display,
                        source,
                    });
                }
            }
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| CommandError::Io {
                program: display.clone(),
                source: io::Error::other("stdout reader panicked"),
            })?
            .map_err(|source| CommandError::Io {
                program: display.clone(),
                source,
            })?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| CommandError::Io {
                program: display.clone(),
                source: io::Error::other("stderr reader panicked"),
            })?
            .map_err(|source| CommandError::Io {
                program: display.clone(),
                source,
            })?;
        if stdout.len() > request.output_limit {
            return Err(CommandError::OutputTooLarge {
                program: display,
                stream: "stdout",
                limit: request.output_limit,
            });
        }
        if stderr.len() > request.output_limit {
            return Err(CommandError::OutputTooLarge {
                program: display,
                stream: "stderr",
                limit: request.output_limit,
            });
        }
        Ok(CommandOutput {
            status,
            stdout,
            stderr,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::time::Duration;

    use super::{CommandError, CommandRequest, CommandRunner, SystemCommandRunner};

    #[test]
    fn system_runner_captures_status_bytes_and_stable_locale() {
        let mut runner = SystemCommandRunner;
        let output = runner
            .run(&CommandRequest::new(
                "/bin/sh",
                ["-c", "printf '%s' \"$LC_ALL\"; printf err >&2; exit 7"],
            ))
            .expect("command output");
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"C");
        assert_eq!(output.stderr, b"err");
    }

    #[test]
    fn system_runner_bounds_time_output_and_missing_programs() {
        let mut runner = SystemCommandRunner;
        let mut timeout = CommandRequest::new("/bin/sh", ["-c", "sleep 2"]);
        timeout.timeout = Duration::from_millis(20);
        assert!(matches!(
            runner.run(&timeout),
            Err(CommandError::TimedOut { .. })
        ));

        let mut oversized = CommandRequest::new("/bin/sh", ["-c", "printf 12345"]);
        oversized.output_limit = 4;
        assert!(matches!(
            runner.run(&oversized),
            Err(CommandError::OutputTooLarge { .. })
        ));

        let missing = CommandRequest {
            program: OsString::from("/definitely/missing/mx-command"),
            args: Vec::new(),
            env: Vec::new(),
            timeout: Duration::from_secs(1),
            output_limit: 8,
        };
        assert!(matches!(runner.run(&missing), Err(CommandError::Io { .. })));
    }

    #[test]
    fn timeout_kills_and_reaps_owned_descendants() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pid_file = temp.path().join("child.pid");
        let command = format!(
            "sleep 30 & child=$!; printf '%s' \"$child\" > '{}'; wait",
            pid_file.display()
        );
        let mut request = CommandRequest::new("/bin/sh", ["-c", &command]);
        request.timeout = Duration::from_millis(100);
        let mut runner = SystemCommandRunner;
        assert!(matches!(
            runner.run(&request),
            Err(CommandError::TimedOut { .. })
        ));
        let pid = std::fs::read_to_string(pid_file).expect("child pid");
        let output = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .output()
            .expect("kill probe");
        assert!(
            !output.status.success(),
            "timed-out backend descendant survived"
        );
    }
}

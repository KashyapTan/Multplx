//! Rust-default lifecycle and service boundary for viz and vplan.
//!
//! The retained `bin/mx-viz.sh` and `bin/mx-vplan.sh` files select this
//! boundary before any state mutation. Their legacy bodies and Node servers
//! remain available only through the explicit rollback selector until the
//! Portion 13 deletion gate.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use multplx_core::filesystem::{atomic_replace, read_bounded_regular};
use multplx_core::locks::DirectoryLock;
use multplx_core::process::{ProcessIdentity, ProcessProbe, SystemProcessProbe};
use rustix::process::{Pid, Signal, kill_process_group};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::http::MAX_CONNECTIONS;

mod viz;
mod vplan;

const RUN_RECORD_LIMIT: usize = 64 * 1024;
const READY_WAIT: Duration = Duration::from_secs(5);
const LOCK_WAIT: Duration = Duration::from_secs(5);

pub const VIZ_HELP: &str = "Start, inspect, and stop the disposable Multplx dashboard.\n\nUsage:\n  mx-viz.sh serve\n  mx-viz.sh status\n  mx-viz.sh stop\n  mx-viz.sh --help\n\n`serve` is singleton and idempotent per MX_HOME. It binds loopback only,\ntries MX_VIZ_PORT (default 4890) plus 19 upward ports, prints the URL only,\nand never opens a browser. The server exits after MX_VIZ_IDLE_SECS (default\n1800) without a request. Snapshot polling defaults to MX_VIZ_POLL_MS=2500\nwith a pull-through MX_VIZ_REFRESH_SECS=2 cache.\n\nRun record contract, state/.viz/server.run, mode 0600:\n  version=1\n  home=<canonical MX_HOME>\n  state=<state directory>\n  port=<bound loopback port>\n  pid=<server pid>\n  pid_identity=<portable process identity>\n  token=<random cleanup binding, never served>\n  started_at=<UTC ISO-8601 time>\n`stop` signals only a live identity-matched process. A dead or reused PID\ncauses record cleanup without signaling.\n";

pub const VPLAN_HELP: &str = "Create, serve, inspect, and stop one-shot vplan review artifacts.\n\nUsage:\n  mx-vplan.sh new <file>\n  mx-vplan.sh review <file>\n  mx-vplan.sh comments <file>\n  mx-vplan.sh stop <file>\n  mx-vplan.sh --help\n\n`new` copies the vendored seed template and rewrites only its Mermaid asset\npath relative to the destination. `review` requires an artifact inside this\nMultplx root, starts the loopback-only Rust service, records its PID identity\nunder state/.vplan/, and prints the bound URL. The first attempted port is\nMX_VPLAN_PORT (default 4870), with 19 upward fallbacks. Confirm or the idle\ntimeout (MX_VPLAN_IDLE_SECS, default 1800) removes the run record and exits.\n`comments` prints the persisted #vplan-comments array as formatted JSON.\n`stop` signals only a live process whose PID identity and review token still\nmatch the artifact's run record; stale records are cleaned without signaling.\n";

#[derive(Debug)]
pub struct ServiceError {
    pub message: String,
    pub code: i32,
}

impl ServiceError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 1,
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 2,
        }
    }
}

type Result<T> = std::result::Result<T, ServiceError>;

pub fn run(entry: &str, args: &[OsString], source_root: &Path) -> i32 {
    let result = match entry {
        "mx-viz.sh" => viz::run_cli(args, source_root),
        "mx-vplan.sh" => vplan::run_cli(args, source_root),
        "viz-server" => viz::run_server(args),
        "vplan-server" => vplan::run_server(args),
        _ => Err(ServiceError::usage(format!(
            "unknown local-service entry point: {entry}"
        ))),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            let prefix = if entry.starts_with("mx-viz") || entry == "viz-server" {
                if entry == "viz-server" {
                    "mx-viz-server"
                } else {
                    "mx-viz"
                }
            } else if entry == "vplan-server" {
                "mx-vplan-server"
            } else {
                "mx-vplan"
            };
            eprintln!("{prefix}: {}", error.message);
            error.code
        }
    }
}

fn utf8_arg<'a>(args: &'a [OsString], index: usize, label: &str) -> Result<&'a str> {
    args.get(index)
        .and_then(|value| value.to_str())
        .ok_or_else(|| ServiceError::new(format!("{label} is not valid UTF-8")))
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ServiceError::new(format!("{label} is unavailable: {error}")))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ServiceError::new(format!(
            "{label} must be a real directory"
        )));
    }
    fs::canonicalize(path)
        .map_err(|error| ServiceError::new(format!("could not canonicalize {label}: {error}")))
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|_| {
        ServiceError::new(format!(
            "{label} is not a readable file: {}",
            path.display()
        ))
    })?;
    let metadata = fs::metadata(&canonical).map_err(|_| {
        ServiceError::new(format!(
            "{label} is not a readable file: {}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(ServiceError::new(format!(
            "{label} is not a readable file: {}",
            path.display()
        )));
    }
    Ok(canonical)
}

fn is_within(parent: &Path, candidate: &Path) -> bool {
    candidate == parent || candidate.starts_with(parent)
}

fn ensure_private_directory(path: &Path, label: &str) -> Result<()> {
    fs::create_dir_all(path)
        .map_err(|error| ServiceError::new(format!("could not create {label}: {error}")))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ServiceError::new(format!("could not inspect {label}: {error}")))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ServiceError::new(format!(
            "unsafe {label}: {}",
            path.display()
        )));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| ServiceError::new(format!("could not secure {label}: {error}")))
}

fn parse_port(name: &str, fallback: u16) -> Result<u16> {
    let value = std::env::var(name).unwrap_or_else(|_| fallback.to_string());
    let port = value
        .parse::<u16>()
        .ok()
        .filter(|port| *port >= 1 && *port <= 65_516)
        .ok_or_else(|| {
            ServiceError::new(format!("{name} must be an integer from 1 through 65516"))
        })?;
    Ok(port)
}

fn parse_integer_env(name: &str, fallback: u64, minimum: u64, maximum: u64) -> Result<u64> {
    let value = std::env::var(name).unwrap_or_else(|_| fallback.to_string());
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value >= minimum && *value <= maximum)
        .ok_or_else(|| {
            ServiceError::new(format!(
                "{name} must be an integer from {minimum} through {maximum}"
            ))
        })
}

fn random_token() -> Result<String> {
    let mut file = File::open("/dev/urandom")
        .map_err(|error| ServiceError::new(format!("could not open system randomness: {error}")))?;
    let mut bytes = [0_u8; 32];
    file.read_exact(&mut bytes)
        .map_err(|error| ServiceError::new(format!("could not create service token: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn valid_token(value: &str) -> bool {
    (32..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let mut difference = left.len() ^ right.len();
    let size = left.len().max(right.len());
    for index in 0..size {
        let a = left.as_bytes().get(index).copied().unwrap_or(0);
        let b = right.as_bytes().get(index).copied().unwrap_or(0);
        difference |= usize::from(a ^ b);
    }
    difference == 0
}

fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn utc_now() -> String {
    OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .ok()
        .and_then(|time| time.format(&Rfc3339).ok())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned())
}

fn record_map(path: &Path) -> Result<BTreeMap<String, String>> {
    let bytes = read_bounded_regular(path, RUN_RECORD_LIMIT)
        .map_err(|error| ServiceError::new(format!("unsafe service run record: {error}")))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ServiceError::new("service run record is not UTF-8"))?;
    Ok(text
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| !key.is_empty())
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect())
}

fn record_identity(record: &BTreeMap<String, String>) -> Option<ProcessIdentity> {
    let pid = record.get("pid")?.parse::<u32>().ok()?;
    let marker = record.get("pid_identity")?.to_owned();
    if marker.is_empty() {
        return None;
    }
    Some(ProcessIdentity { pid, marker })
}

fn record_process_live(record: &BTreeMap<String, String>) -> bool {
    let Some(expected) = record_identity(record) else {
        return false;
    };
    let probe = SystemProcessProbe::default();
    probe.is_alive(expected.pid)
        && probe
            .identity(expected.pid)
            .is_ok_and(|actual| actual == expected)
}

fn identity_binds_token(marker: &str, token: &str) -> bool {
    let hex = token
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    marker.contains(token) || marker.contains(&hex)
}

fn record_live(record: &BTreeMap<String, String>, expected_home: Option<&Path>) -> bool {
    if expected_home.is_some_and(|home| record.get("home").map(Path::new) != Some(home)) {
        return false;
    }
    if record.get("version").map(String::as_str) != Some("1") {
        return false;
    }
    let Some(token) = record.get("token").filter(|token| valid_token(token)) else {
        return false;
    };
    record_identity(record).is_some_and(|identity| identity_binds_token(&identity.marker, token))
        && record_process_live(record)
}

fn remove_record_if_matches(path: &Path, pid: u32, token: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let record = record_map(path)?;
    if record.get("pid").map(String::as_str) == Some(pid.to_string().as_str())
        && record
            .get("token")
            .is_some_and(|value| constant_time_eq(value, token))
    {
        fs::remove_file(path)
            .map_err(|error| ServiceError::new(format!("could not remove run record: {error}")))?;
    }
    Ok(())
}

fn write_record(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_replace(path, bytes, 0o600)
        .map_err(|error| ServiceError::new(format!("could not publish run record: {error}")))
}

fn acquire_lock(path: &Path) -> Result<DirectoryLock> {
    DirectoryLock::acquire_wait(path, &SystemProcessProbe::default(), LOCK_WAIT)
        .map_err(|error| ServiceError::new(format!("could not acquire service lock: {error}")))
}

struct StartedService {
    child: Child,
    port: u16,
    ready: tempfile::NamedTempFile,
    errors: tempfile::NamedTempFile,
}

fn start_service(entry: &str, args: &[OsString]) -> Result<StartedService> {
    let executable = std::env::current_exe()
        .map_err(|error| ServiceError::new(format!("could not resolve Rust runtime: {error}")))?;
    let ready = tempfile::Builder::new()
        .prefix(&format!("mx-{entry}-ready."))
        .tempfile()
        .map_err(|error| ServiceError::new(format!("could not create readiness log: {error}")))?;
    let errors = tempfile::Builder::new()
        .prefix(&format!("mx-{entry}-error."))
        .tempfile()
        .map_err(|error| ServiceError::new(format!("could not create error log: {error}")))?;
    let stdout = ready
        .as_file()
        .try_clone()
        .map_err(|error| ServiceError::new(format!("could not prepare readiness log: {error}")))?;
    let stderr = errors
        .as_file()
        .try_clone()
        .map_err(|error| ServiceError::new(format!("could not prepare error log: {error}")))?;
    let mut child = Command::new(executable)
        .args([OsStr::new("services"), OsStr::new(entry)])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| ServiceError::new(format!("could not start Rust service: {error}")))?;
    let deadline = Instant::now() + READY_WAIT;
    loop {
        let text = fs::read_to_string(ready.path()).unwrap_or_default();
        if let Some(line) = text.lines().next()
            && let Some(value) = line.strip_prefix("READY ")
            && let Ok(port) = value.parse::<u16>()
        {
            return Ok(StartedService {
                child,
                port,
                ready,
                errors,
            });
        }
        if child.try_wait().ok().flatten().is_some() || Instant::now() >= deadline {
            let _ = child.kill();
            let status = child
                .wait()
                .ok()
                .and_then(|status| status.code())
                .unwrap_or(1);
            let details = fs::read_to_string(errors.path())
                .unwrap_or_default()
                .lines()
                .take(4)
                .collect::<Vec<_>>()
                .join("\n");
            let message = if details.is_empty() {
                format!("server did not publish readiness (exit {status})")
            } else {
                details
            };
            return Err(ServiceError::new(message));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn bind_loopback(first_port: u16) -> Result<(TcpListener, u16)> {
    for offset in 0..20_u16 {
        let port = first_port + offset;
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                listener.set_nonblocking(true).map_err(|error| {
                    ServiceError::new(format!("could not configure listener: {error}"))
                })?;
                return Ok((listener, port));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(error) => {
                return Err(ServiceError::new(format!(
                    "could not bind loopback: {error}"
                )));
            }
        }
    }
    Err(ServiceError::new(format!(
        "no loopback port available in range {}-{}",
        first_port,
        first_port + 19
    )))
}

fn shutdown_flag() -> Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&flag))
        .and_then(|_| signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&flag)))
        .map_err(|error| ServiceError::new(format!("could not install signal handler: {error}")))?;
    Ok(flag)
}

fn accept_loop<F, I>(listener: TcpListener, shutdown: Arc<AtomicBool>, idle_expired: I, handler: F)
where
    F: Fn(TcpStream) + Send + Sync + 'static,
    I: Fn() -> bool,
{
    let active = Arc::new(AtomicUsize::new(0));
    let handler = Arc::new(handler);
    while !shutdown.load(Ordering::SeqCst) && !idle_expired() {
        match listener.accept() {
            Ok((stream, _)) => {
                if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    drop(stream);
                    continue;
                }
                active.fetch_add(1, Ordering::SeqCst);
                let active = Arc::clone(&active);
                let handler = Arc::clone(&handler);
                thread::spawn(move || {
                    handler(stream);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

#[derive(Debug)]
struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn kill_and_reap(child: &mut Child) {
    if let Some(pid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process_group(pid, Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_limited(mut reader: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    reader
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn run_bounded_command(
    program: &Path,
    args: &[&str],
    environment: &[(OsString, OsString)],
    timeout: Duration,
    limit: usize,
) -> Result<CommandOutput> {
    let mut child = Command::new(program)
        .args(args)
        .envs(environment.iter().cloned())
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            ServiceError::new(format!("could not execute {}: {error}", program.display()))
        })?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdout_thread = thread::spawn(move || read_limited(stdout, limit));
    let stderr_thread = thread::spawn(move || read_limited(stderr, limit));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                kill_and_reap(&mut child);
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(ServiceError::new(format!(
                    "{} timed out after {} seconds",
                    program.display(),
                    timeout.as_secs()
                )));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                kill_and_reap(&mut child);
                return Err(ServiceError::new(format!(
                    "could not inspect child: {error}"
                )));
            }
        }
    };
    let stdout = stdout_thread
        .join()
        .map_err(|_| ServiceError::new("stdout reader failed"))?
        .map_err(|error| ServiceError::new(format!("could not read stdout: {error}")))?;
    let stderr = stderr_thread
        .join()
        .map_err(|_| ServiceError::new("stderr reader failed"))?
        .map_err(|error| ServiceError::new(format!("could not read stderr: {error}")))?;
    if stdout.len() > limit || stderr.len() > limit {
        return Err(ServiceError::new(format!(
            "{} exceeded the {limit}-byte output limit",
            program.display()
        )));
    }
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

fn http_get_json(port: u16, path: &str) -> Option<serde_json::Value> {
    use std::io::{Read, Write};
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().ok()?,
        Duration::from_millis(250),
    )
    .ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut bytes = Vec::new();
    stream.take(1024 * 1024).read_to_end(&mut bytes).ok()?;
    let split = bytes.windows(4).position(|window| window == b"\r\n\r\n")? + 4;
    serde_json::from_slice(&bytes[split..]).ok()
}

fn create_new_file(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(mode);
    let mut file = options.open(path).map_err(|error| {
        ServiceError::new(format!("could not create {}: {error}", path.display()))
    })?;
    use std::io::Write;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| ServiceError::new(format!("could not write {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::{
        bind_loopback, canonical_directory, canonical_file, constant_time_eq, create_new_file,
        ensure_private_directory, identity_binds_token, is_within, record_identity, record_live,
        record_map, remove_record_if_matches, run_bounded_command, sha256_hex, utc_now,
        valid_token, write_record,
    };
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn token_validation_and_comparison_reject_wrong_shapes() {
        assert!(valid_token(&"a".repeat(32)));
        assert!(!valid_token(&"A".repeat(32)));
        assert!(!valid_token("short"));
        assert!(constant_time_eq("same", "same"));
        assert!(!constant_time_eq("same", "different"));
        assert!(identity_binds_token("prefix-feedface-suffix", "feedface"));
        assert!(identity_binds_token("6665656466616365", "feedface"));
        assert!(!identity_binds_token("unrelated", "feedface"));
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(utc_now().ends_with('Z'));
    }

    #[test]
    fn file_record_and_path_helpers_fail_closed_without_following_links() {
        let temporary = tempfile::tempdir().expect("temporary");
        let directory = temporary.path().join("private");
        ensure_private_directory(&directory, "fixture").expect("private directory");
        assert_eq!(
            fs::metadata(&directory)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let canonical = canonical_directory(&directory, "fixture").expect("canonical");
        assert!(is_within(
            &fs::canonicalize(temporary.path()).expect("temporary root"),
            &canonical
        ));
        let file = directory.join("record");
        write_record(
            &file,
            b"version=1\npid=0\ntoken=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
        )
        .expect("record");
        assert_eq!(
            canonical_file(&file, "record").expect("file"),
            fs::canonicalize(&file).expect("canonical file")
        );
        let values = record_map(&file).expect("map");
        assert_eq!(values["version"], "1");
        assert!(record_identity(&values).is_none());
        assert!(!record_live(&values, Some(temporary.path())));
        remove_record_if_matches(&file, 0, &"a".repeat(32)).expect("remove");
        assert!(!file.exists());

        let created = directory.join("created");
        create_new_file(&created, b"bytes", 0o640).expect("create");
        assert_eq!(fs::read(&created).expect("read"), b"bytes");
        assert!(create_new_file(&created, b"again", 0o600).is_err());
        assert!(canonical_file(&directory, "not-file").is_err());
        assert!(canonical_file(&directory.join("missing"), "missing-file").is_err());
        assert!(canonical_directory(&created, "not-directory").is_err());
        assert!(record_map(&directory.join("missing")).is_err());

        let symlink = temporary.path().join("link");
        std::os::unix::fs::symlink(&directory, &symlink).expect("symlink");
        assert!(ensure_private_directory(&symlink, "link").is_err());
        assert!(canonical_directory(&symlink, "link").is_err());
    }

    #[test]
    fn bounded_commands_cover_success_failure_output_and_timeout() {
        let empty: Vec<(OsString, OsString)> = Vec::new();
        let success = run_bounded_command(
            Path::new("/bin/sh"),
            &["-c", "printf out; printf err >&2; exit 7"],
            &empty,
            Duration::from_secs(1),
            32,
        )
        .expect("command");
        assert_eq!(success.status.code(), Some(7));
        assert_eq!(success.stdout, b"out");
        assert_eq!(success.stderr, b"err");
        assert!(
            run_bounded_command(
                Path::new("/bin/sh"),
                &["-c", "printf 123456789"],
                &empty,
                Duration::from_secs(1),
                4,
            )
            .expect_err("output bound")
            .message
            .contains("output limit")
        );
        assert!(
            run_bounded_command(
                Path::new("/bin/sh"),
                &["-c", "sleep 2"],
                &empty,
                Duration::from_millis(20),
                32,
            )
            .expect_err("timeout")
            .message
            .contains("timed out")
        );
        assert!(
            run_bounded_command(
                Path::new("/definitely/missing/mx-command"),
                &[],
                &empty,
                Duration::from_secs(1),
                32,
            )
            .is_err()
        );
    }

    #[test]
    fn loopback_binding_falls_forward_and_reports_exhaustion() {
        let first = TcpListener::bind(("127.0.0.1", 0)).expect("first");
        let port = first.local_addr().expect("address").port();
        if port <= 65_515 {
            let (_listener, selected) = bind_loopback(port).expect("fallback");
            assert!(selected > port);
        }

        let mut held = Vec::new();
        let mut base = None;
        for candidate in 40_000..60_000_u16 {
            let mut listeners = Vec::new();
            for offset in 0..20_u16 {
                match TcpListener::bind(("127.0.0.1", candidate + offset)) {
                    Ok(listener) => listeners.push(listener),
                    Err(_) => break,
                }
            }
            if listeners.len() == 20 {
                base = Some(candidate);
                held = listeners;
                break;
            }
        }
        let base = base.expect("contiguous ports");
        assert!(
            bind_loopback(base)
                .expect_err("exhausted")
                .message
                .contains("no loopback port")
        );
        drop(held);

        let mut malformed = BTreeMap::new();
        malformed.insert("pid".to_owned(), "not-a-pid".to_owned());
        malformed.insert("pid_identity".to_owned(), "marker".to_owned());
        assert!(record_identity(&malformed).is_none());
    }
}

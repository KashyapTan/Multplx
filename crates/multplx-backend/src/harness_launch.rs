//! Primary harness launch validation, serialized startup, and live connection.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use multplx_core::filesystem::atomic_replace;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::{ProcessIdentity, ProcessProbe, SystemProcessProbe};
use multplx_core::session_lock::{SessionLockStatus, harness_regex, status};
use serde::{Deserialize, Serialize};

fn error(message: impl std::fmt::Display) {
    eprintln!("multplx: {message}");
}

fn executable(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn same_file(left: &Path, right: &Path) -> bool {
    let Ok(left) = fs::metadata(left) else {
        return false;
    };
    let Ok(right) = fs::metadata(right) else {
        return false;
    };
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, i32> {
    if !path.is_dir() {
        error(format_args!(
            "{label} directory does not exist: {}",
            path.display()
        ));
        return Err(2);
    }
    fs::canonicalize(path).map_err(|_| {
        error(format_args!(
            "cannot resolve {label} directory: {}",
            path.display()
        ));
        2
    })
}

fn validate_root(root: &Path) -> Result<(), i32> {
    if !root.join("AGENTS.md").is_file() {
        error(format_args!(
            "code root is missing AGENTS.md: {}",
            root.display()
        ));
        return Err(2);
    }
    if !root.join("bin").is_dir() || !root.join(".agents/skills").is_dir() {
        error(format_args!(
            "code root is missing Multplx scripts or skills: {}",
            root.display()
        ));
        return Err(2);
    }
    let launcher = root.join("bin/mx-launcher.sh");
    if !executable(&launcher) {
        error(format_args!(
            "code root is missing an executable launcher: {}",
            launcher.display()
        ));
        return Err(2);
    }
    let release_marker = root.join(".multplx-release");
    if release_marker.is_file() {
        let marker = fs::read_to_string(&release_marker).map_err(|error_value| {
            error(format_args!(
                "cannot read packaged runtime marker {}: {error_value}",
                release_marker.display()
            ));
            2
        })?;
        if marker.trim() != env!("CARGO_PKG_VERSION") {
            error(format_args!(
                "packaged runtime version {} does not match binary {}",
                marker.trim(),
                env!("CARGO_PKG_VERSION")
            ));
            return Err(2);
        }
        return Ok(());
    }
    if fs::symlink_metadata(root.join(".git"))
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
        || !root.join(".git").is_dir()
    {
        error(format_args!(
            "code root must be a plain checkout, not a linked worktree: {}",
            root.display()
        ));
        return Err(2);
    }
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|_| {
            error(format_args!(
                "code root is not a git checkout: {}",
                root.display()
            ));
            2
        })?;
    if !output.status.success() {
        error(format_args!(
            "code root is not a git checkout: {}",
            root.display()
        ));
        return Err(2);
    }
    let top = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let top = canonical_directory(&top, "git top level")?;
    if top != root {
        error(format_args!(
            "code root must be the checkout top level: {}",
            root.display()
        ));
        return Err(2);
    }
    Ok(())
}

fn validate_home(home: &Path) -> Result<(), i32> {
    if home == Path::new("/") {
        error("operational home may not be the filesystem root");
        return Err(2);
    }
    for part in ["config", "data", "projects", "state"] {
        let path = home.join(part);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink())
            || !path.is_dir()
        {
            error(format_args!(
                "operational home is missing a real {part} directory: {}",
                path.display()
            ));
            return Err(2);
        }
    }
    Ok(())
}

fn pending_install_transaction() -> Result<Option<PathBuf>, String> {
    let binary = std::env::current_exe()
        .map_err(|error| format!("cannot locate running Multplx binary: {error}"))?;
    let Some(bin_dir) = binary.parent() else {
        return Err("running Multplx binary has no installation directory".to_owned());
    };
    let pointer = bin_dir.join(".multplx-config");
    let metadata = match fs::symlink_metadata(&pointer) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect launcher config record: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 16 * 1024 {
        return Err(format!(
            "launcher config record is linked, oversized, or not regular: {}",
            pointer.display()
        ));
    }
    let value = fs::read_to_string(&pointer)
        .map_err(|error| format!("cannot read launcher config record: {error}"))?;
    let config = PathBuf::from(value.trim_end_matches('\n'));
    if !config.is_absolute()
        || value.is_empty()
        || value.contains('\r')
        || value.matches('\n').count() != 1
    {
        return Err(format!(
            "launcher config record is malformed: {}",
            pointer.display()
        ));
    }
    let transaction = config.join(".launcher-install.transaction");
    match fs::symlink_metadata(&transaction) {
        Ok(_) => Ok(Some(transaction)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot inspect launcher transaction: {error}")),
    }
}

fn real_executable(harness: &str) -> Option<PathBuf> {
    let variable = match harness {
        "claude" => "MX_REAL_CLAUDE",
        "codex" => "MX_REAL_CODEX",
        "cursor" => "MX_REAL_CURSOR_AGENT",
        "pi" => "MX_REAL_PI",
        _ => return None,
    };
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn cursor_args_safe(args: &[OsString]) -> bool {
    let mut previous: Option<&OsStr> = None;
    for argument in args {
        let value = argument.to_string_lossy();
        if matches!(
            value.as_ref(),
            "-f" | "--force" | "--yolo" | "--sandbox=disabled" | "-w" | "--worktree"
        ) || value.starts_with("--worktree=")
            || (value == "disabled" && previous == Some(OsStr::new("--sandbox")))
        {
            return false;
        }
        previous = Some(argument);
    }
    true
}

const REMEMBERED_HARNESS: &str = "primary-harness";
const CONNECTION_RECORD: &str = "workspace-connection.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionRecord {
    schema: String,
    owner: Option<LifetimeIdentity>,
    harness: String,
    caller_cwd: PathBuf,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    pane: Option<String>,
    #[serde(default)]
    tmux_socket: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchReservation {
    schema: String,
    owner: LifetimeIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifetimeIdentity {
    pid: u32,
    started: String,
}

fn lifetime(identity: ProcessIdentity) -> LifetimeIdentity {
    let started = identity
        .marker
        .split_once(" cmdline-hex=")
        .map(|(start, _)| start.to_owned())
        .unwrap_or_else(|| {
            identity
                .marker
                .split_whitespace()
                .take(5)
                .collect::<Vec<_>>()
                .join(" ")
        });
    LifetimeIdentity {
        pid: identity.pid,
        started,
    }
}

#[derive(Debug, Eq, PartialEq)]
enum LaunchedIdentityError {
    Exited(i32),
    Unidentified(String),
}

fn identify_launched_process<P, F>(
    pid: u32,
    processes: &P,
    timeout: Duration,
    retry_delay: Duration,
    mut exit_code: F,
) -> Result<ProcessIdentity, LaunchedIdentityError>
where
    P: ProcessProbe,
    F: FnMut() -> std::io::Result<Option<i32>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        match processes.identity(pid) {
            Ok(identity) => return Ok(identity),
            Err(identity_failure) => match exit_code() {
                Ok(Some(code)) => return Err(LaunchedIdentityError::Exited(code)),
                Ok(None) if Instant::now() < deadline => thread::sleep(retry_delay),
                Ok(None) => {
                    return Err(LaunchedIdentityError::Unidentified(
                        identity_failure.to_string(),
                    ));
                }
                Err(observe_failure) => {
                    return Err(LaunchedIdentityError::Unidentified(format!(
                        "{identity_failure}; child observation failed: {observe_failure}"
                    )));
                }
            },
        }
    }
}

/// Read-only state presented by the workspace launcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationState {
    Offline {
        remembered: Option<String>,
    },
    Starting,
    Live {
        pid: u32,
        harness: Option<String>,
        attachable: bool,
    },
    Unreadable,
}

impl ConversationState {
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::Offline {
                remembered: Some(harness),
            } => {
                format!("offline · remembered {harness}")
            }
            Self::Offline { remembered: None } => "offline · choose a harness".to_owned(),
            Self::Starting => "starting in another terminal".to_owned(),
            Self::Live {
                pid,
                harness,
                attachable,
            } => format!(
                "live {} · pid {pid} · {}",
                harness.as_deref().unwrap_or("harness"),
                if *attachable {
                    "terminal connection available"
                } else {
                    "send requests through this workspace"
                }
            ),
            Self::Unreadable => "owner state unreadable · repair with multplx doctor".to_owned(),
        }
    }
}

fn valid_harness(value: &str) -> bool {
    matches!(value, "claude" | "codex" | "cursor" | "pi")
}

/// Return the remembered primary harness for this operational home.
#[must_use]
pub fn remembered_harness(home: &Path) -> Option<String> {
    let value = fs::read_to_string(home.join("config").join(REMEMBERED_HARNESS)).ok()?;
    let value = value.trim();
    valid_harness(value).then(|| value.to_owned())
}

fn remember_harness(home: &Path, harness: &str) -> Result<(), String> {
    if !valid_harness(harness) {
        return Err("harness must be claude, codex, cursor, or pi".to_owned());
    }
    atomic_replace(
        home.join("config").join(REMEMBERED_HARNESS),
        format!("{harness}\n").as_bytes(),
        0o600,
    )
    .map_err(|error| error.to_string())
}

fn ambient_endpoint() -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<PathBuf>,
) {
    if let Some(target) = std::env::var("TMUX_PANE")
        .ok()
        .filter(|value| !value.is_empty())
    {
        let socket = std::env::var_os("TMUX").and_then(|value| {
            PathBuf::from(value.to_string_lossy().split(',').next()?.to_owned())
                .is_absolute()
                .then_some(PathBuf::from(
                    value.to_string_lossy().split(',').next()?.to_owned(),
                ))
        });
        let session = tmux_display(socket.as_deref(), &target, "#{session_name}");
        return (Some("tmux".to_owned()), session, Some(target), socket);
    }
    if let Some(target) = std::env::var("MX_SUPERVISOR_TARGET")
        .ok()
        .filter(|value| !value.is_empty())
    {
        let backend = std::env::var("MX_SUPERVISOR_BACKEND")
            .ok()
            .filter(|value| !value.is_empty());
        return (backend, Some(target), None, None);
    }
    if std::env::var_os("HERDR_MANAGED").is_some()
        && let Some(pane) = std::env::var("HERDR_PANE_ID")
            .ok()
            .filter(|value| !value.is_empty())
    {
        let session = std::env::var("HERDR_SESSION").unwrap_or_else(|_| "default".to_owned());
        return (
            Some("herdr".to_owned()),
            Some(format!("{session}:{pane}")),
            None,
            None,
        );
    }
    (None, None, None, None)
}

fn tmux_display(socket: Option<&Path>, target: &str, format: &str) -> Option<String> {
    let mut command = Command::new("tmux");
    if let Some(socket) = socket {
        command.arg("-S").arg(socket);
    }
    command.args(["display-message", "-p", "-t", target, format]);
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn connection_path(home: &Path) -> PathBuf {
    home.join("state").join(CONNECTION_RECORD)
}

fn read_connection(home: &Path) -> Option<ConnectionRecord> {
    let bytes = fs::read(connection_path(home)).ok()?;
    serde_json::from_slice::<ConnectionRecord>(&bytes)
        .ok()
        .filter(|record| {
            record.schema == "mx-workspace-connection.v1" && valid_harness(&record.harness)
        })
}

fn write_connection(home: &Path, harness: &str, owner: LifetimeIdentity) -> Result<(), String> {
    let caller_cwd = std::env::var_os("MX_CALLER_CWD")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let (backend, target, pane, tmux_socket) = ambient_endpoint();
    let record = ConnectionRecord {
        schema: "mx-workspace-connection.v1".to_owned(),
        owner: Some(owner),
        harness: harness.to_owned(),
        caller_cwd,
        backend,
        target,
        pane,
        tmux_socket,
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?;
    atomic_replace(connection_path(home), &bytes, 0o600).map_err(|error| error.to_string())
}

fn update_connection_owner(home: &Path, owner: LifetimeIdentity) -> Result<(), String> {
    let mut record = read_connection(home).ok_or("workspace connection record disappeared")?;
    record.owner = Some(owner);
    let bytes = serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?;
    atomic_replace(connection_path(home), &bytes, 0o600).map_err(|error| error.to_string())
}

fn reservation_path(home: &Path) -> PathBuf {
    home.join("state/workspace-launch.json")
}

fn write_reservation(home: &Path, owner: LifetimeIdentity) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&LaunchReservation {
        schema: "mx-workspace-launch.v1".to_owned(),
        owner,
    })
    .map_err(|error| error.to_string())?;
    atomic_replace(reservation_path(home), &bytes, 0o600).map_err(|error| error.to_string())
}

enum ReservationState {
    MissingOrDead,
    Live,
    Unknown,
}

fn reservation_state(home: &Path, processes: &impl ProcessProbe) -> ReservationState {
    let path = reservation_path(home);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ReservationState::MissingOrDead;
        }
        Err(_) => return ReservationState::Unknown,
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 16 * 1024 {
        return ReservationState::Unknown;
    }
    let Ok(bytes) = fs::read(path) else {
        return ReservationState::Unknown;
    };
    let Ok(record) = serde_json::from_slice::<LaunchReservation>(&bytes) else {
        return ReservationState::Unknown;
    };
    if record.schema != "mx-workspace-launch.v1" {
        return ReservationState::Unknown;
    }
    if !processes.is_alive(record.owner.pid) {
        return ReservationState::MissingOrDead;
    }
    match processes.identity(record.owner.pid) {
        Ok(current) => {
            if lifetime(current) == record.owner {
                ReservationState::Live
            } else {
                ReservationState::MissingOrDead
            }
        }
        Err(_) => ReservationState::Unknown,
    }
}

/// Inspect the live owner without changing or stealing its lock.
#[must_use]
pub fn conversation_state(home: &Path) -> ConversationState {
    let processes = SystemProcessProbe::default();
    match status(home.join("state/.lock"), &processes, &harness_regex()) {
        SessionLockStatus::Held(pid) => {
            let owner = processes.identity(pid).ok().map(lifetime);
            let record = read_connection(home).filter(|record| record.owner == owner);
            ConversationState::Live {
                pid,
                harness: record.as_ref().map(|record| record.harness.clone()),
                attachable: record.as_ref().is_some_and(|record| {
                    record.backend.as_deref() == Some("tmux")
                        && record.target.is_some()
                        && record.pane.is_some()
                }),
            }
        }
        SessionLockStatus::Unreadable => ConversationState::Unreadable,
        SessionLockStatus::Free | SessionLockStatus::Stale(_) => {
            match reservation_state(home, &processes) {
                ReservationState::Live => ConversationState::Starting,
                ReservationState::Unknown => ConversationState::Unreadable,
                ReservationState::MissingOrDead => ConversationState::Offline {
                    remembered: remembered_harness(home),
                },
            }
        }
    }
}

fn attach_live(home: &Path, pid: u32) -> i32 {
    let processes = SystemProcessProbe::default();
    let owner = processes.identity(pid).ok().map(lifetime);
    let Some(record) = read_connection(home).filter(|record| record.owner == owner) else {
        error(format_args!(
            "the existing conversation is live (pid {pid}), but its terminal route is unavailable; submit work through the workspace or return to its terminal"
        ));
        return 3;
    };
    if record.backend.as_deref() != Some("tmux") {
        error(format_args!(
            "the existing {} conversation is live (pid {pid}), but {} does not support terminal attachment; submit work through the workspace or return to its terminal",
            record.harness,
            record.backend.as_deref().unwrap_or("this launch mode")
        ));
        return 3;
    }
    let Some(target) = record.target.filter(|value| !value.is_empty()) else {
        error("the live tmux conversation has no recorded terminal target");
        return 3;
    };
    let pane_owner = record.pane.as_deref().and_then(|pane| {
        tmux_display(record.tmux_socket.as_deref(), pane, "#{pane_pid}")
            .and_then(|value| value.parse::<u32>().ok())
    });
    if pane_owner.is_none_or(|pane_pid| !ancestor_contains(pid, pane_pid, &processes)) {
        error("the recorded tmux route no longer contains the live conversation owner");
        return 3;
    }
    let current_socket = std::env::var_os("TMUX").and_then(|value| {
        value
            .to_string_lossy()
            .split(',')
            .next()
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    });
    let same_server = current_socket.as_deref() == record.tmux_socket.as_deref();
    let mut command = Command::new("tmux");
    if let Some(socket) = &record.tmux_socket {
        command.arg("-S").arg(socket);
    }
    if same_server {
        command.args(["switch-client", "-t", &target]);
    } else {
        command.env_remove("TMUX");
        command.args(["attach-session", "-t", &target]);
    }
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(failure) => {
            error(format_args!(
                "could not attach to the live tmux conversation: {failure}"
            ));
            3
        }
    }
}

fn ancestor_contains(mut pid: u32, wanted: u32, processes: &impl ProcessProbe) -> bool {
    for _ in 0..16 {
        if pid == wanted {
            return true;
        }
        let Ok(row) = processes.ancestry_row(pid) else {
            return false;
        };
        if row.parent_pid <= 1 || row.parent_pid == pid {
            return row.parent_pid == wanted;
        }
        pid = row.parent_pid;
    }
    false
}

/// Validate and supervise one verified real harness or connect to its live terminal.
pub fn run(harness: &str, args: &[OsString]) -> i32 {
    if !matches!(harness, "claude" | "codex" | "cursor" | "pi") {
        error("harness must be claude, codex, cursor, or pi");
        return 2;
    }
    let Some(root_value) = std::env::var_os("MX_ROOT_OVERRIDE").filter(|value| !value.is_empty())
    else {
        error("harness launch requires MX_ROOT_OVERRIDE and MX_HOME from the launcher");
        return 2;
    };
    let Some(home_value) = std::env::var_os("MX_HOME").filter(|value| !value.is_empty()) else {
        error("harness launch requires MX_ROOT_OVERRIDE and MX_HOME from the launcher");
        return 2;
    };
    let (root, home) = if std::env::var("MX_LAUNCH_VALIDATED").as_deref() == Ok("1") {
        let root = PathBuf::from(root_value);
        let home = PathBuf::from(home_value);
        if !root.is_absolute() || !home.is_absolute() {
            error("validated launcher paths must be absolute");
            return 2;
        }
        if !root.join("AGENTS.md").is_file()
            || !executable(&root.join("bin/mx-lock.sh"))
            || !home.join("state").is_dir()
        {
            error("validated launcher root or home disappeared before harness start");
            return 2;
        }
        (root, home)
    } else {
        let root = match canonical_directory(Path::new(&root_value), "code root") {
            Ok(path) => path,
            Err(code) => return code,
        };
        let home = match canonical_directory(Path::new(&home_value), "operational home") {
            Ok(path) => path,
            Err(code) => return code,
        };
        if let Err(code) = validate_root(&root).and_then(|()| validate_home(&home)) {
            return code;
        }
        (root, home)
    };
    match conversation_state(&home) {
        ConversationState::Live { pid, .. } => return attach_live(&home, pid),
        ConversationState::Unreadable => {
            error("the existing conversation owner is unreadable; run multplx doctor");
            return 3;
        }
        ConversationState::Starting => {
            error("the existing conversation is still starting in another terminal");
            return 3;
        }
        ConversationState::Offline { .. } => {}
    }
    let launch_lock = match DirectoryLock::try_acquire(
        home.join("state/.workspace-launch.lock"),
        &SystemProcessProbe::default(),
    ) {
        Ok(lock) => lock,
        Err(_) => {
            error("the existing conversation is still starting in another terminal");
            return 3;
        }
    };
    match pending_install_transaction() {
        Ok(Some(transaction)) => {
            drop(launch_lock);
            error(format_args!(
                "installation transaction is pending at {}; rerun the verified installer to recover it before launch",
                transaction.display()
            ));
            return 2;
        }
        Err(message) => {
            drop(launch_lock);
            error(message);
            return 2;
        }
        Ok(None) => {}
    }
    // The state may have changed between the optimistic inspection and launch reservation.
    match conversation_state(&home) {
        ConversationState::Live { pid, .. } => {
            drop(launch_lock);
            return attach_live(&home, pid);
        }
        ConversationState::Starting | ConversationState::Unreadable => {
            drop(launch_lock);
            error("the existing conversation is still starting or its owner is unreadable");
            return 3;
        }
        ConversationState::Offline { .. } => {}
    }
    let Some(real) = real_executable(harness) else {
        error(format_args!(
            "{harness} is not installed or its captured executable is no longer available"
        ));
        return 127;
    };
    if !real.is_absolute() || !executable(&real) {
        error(format_args!(
            "{harness} is not installed or its captured executable is no longer available"
        ));
        return 127;
    }
    let shim_name = if harness == "cursor" {
        "cursor-agent"
    } else {
        harness
    };
    let shim = root.join("share/shell/shims").join(shim_name);
    let agent_shim = root.join("share/shell/shims/agent");
    if same_file(&real, &shim) || (harness == "cursor" && same_file(&real, &agent_shim)) {
        error(format_args!("refusing recursive {harness} shim resolution"));
        return 127;
    }
    if harness == "cursor" && !cursor_args_safe(args) {
        error("Cursor launch refuses force, sandbox-disabled, and Cursor-owned worktree modes");
        return 2;
    }
    if let Err(message) = remember_harness(&home, harness) {
        error(format_args!(
            "could not remember the workspace harness: {message}"
        ));
        return 2;
    }
    let gate_dir = match tempfile::Builder::new().prefix("mx-launch-gate.").tempdir() {
        Ok(directory) => directory,
        Err(failure) => {
            error(format_args!("could not create launch gate: {failure}"));
            return 2;
        }
    };
    let gate = gate_dir.path().join("ready");
    if !Command::new("mkfifo")
        .arg(&gate)
        .status()
        .is_ok_and(|status| status.success())
    {
        error("could not create launch gate");
        return 2;
    }
    let mut command = Command::new("/bin/sh");
    command.args([
        OsString::from("-c"),
        OsString::from(
            "IFS= read -r mx_gate < \"$1\"; [ \"$mx_gate\" = go ] || exit 125; shift; exec \"$@\"",
        ),
        OsString::from("mx-launch-gate"),
        gate.as_os_str().to_owned(),
        real.as_os_str().to_owned(),
    ]);
    // Deliberately inherit the caller's Git, forge, and SSH authentication.
    // The launcher adds only Multplx routing variables and never serializes
    // credential values into its state or command-line arguments.
    command
        .current_dir(&root)
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", &home);
    if harness == "cursor" {
        command.args([OsString::from("--sandbox"), OsString::from("enabled")]);
    }
    command.args(args);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(failure) => {
            error(format_args!(
                "{harness} is not installed or its captured executable is no longer available: {failure}"
            ));
            return 127;
        }
    };
    let processes = SystemProcessProbe::default();
    // Linux briefly exposes an empty /proc/<pid>/cmdline while a freshly spawned
    // process crosses exec. Keep the FIFO closed while retrying so the exact
    // pre-exec lifetime is recorded before the harness is allowed to run.
    let child_identity = match identify_launched_process(
        child.id(),
        &processes,
        Duration::from_secs(1),
        Duration::from_millis(2),
        || {
            child
                .try_wait()
                .map(|status| status.map(|value| value.code().unwrap_or(1)))
        },
    ) {
        Ok(identity) => lifetime(identity),
        Err(LaunchedIdentityError::Exited(code)) => return code,
        Err(LaunchedIdentityError::Unidentified(failure)) => {
            let _ = child.kill();
            let _ = child.wait();
            error(format_args!(
                "could not identify launched {harness}: {failure}"
            ));
            return 2;
        }
    };
    if let Err(message) = write_reservation(&home, child_identity.clone())
        .and_then(|()| write_connection(&home, harness, child_identity))
    {
        let _ = child.kill();
        let _ = child.wait();
        error(format_args!(
            "could not publish the workspace connection: {message}"
        ));
        return 2;
    }
    let gate_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
            .open(&gate)
        {
            Ok(mut file) => {
                if file.write_all(b"go\n").is_ok() {
                    break;
                }
                if Instant::now() >= gate_deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_file(reservation_path(&home));
                    error("could not release the reserved harness launch");
                    return 2;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) if Instant::now() < gate_deadline => match child.try_wait() {
                Ok(Some(result)) => {
                    let _ = fs::remove_file(reservation_path(&home));
                    return result.code().unwrap_or(1);
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            },
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(reservation_path(&home));
                error("could not release the reserved harness launch");
                return 2;
            }
        }
    }
    drop(gate_dir);
    // The process-bound reservation now serializes peers without keeping a mutation lock
    // across harness startup or model lifetime.
    drop(launch_lock);
    loop {
        match status(
            home.join("state/.lock"),
            &SystemProcessProbe::default(),
            &harness_regex(),
        ) {
            SessionLockStatus::Held(pid) if ancestor_contains(pid, child.id(), &processes) => {
                let Ok(owner) = processes.identity(pid).map(lifetime) else {
                    continue;
                };
                if let Err(message) = update_connection_owner(&home, owner) {
                    error(format_args!(
                        "could not publish the live conversation owner: {message}"
                    ));
                    return 2;
                }
                let _ = fs::remove_file(reservation_path(&home));
                break;
            }
            SessionLockStatus::Held(pid) => {
                let _ = child.kill();
                let _ = child.wait();
                error(format_args!(
                    "another live conversation won the launch race (pid {pid})"
                ));
                return 3;
            }
            _ => match child.try_wait() {
                Ok(Some(result)) => {
                    let _ = fs::remove_file(reservation_path(&home));
                    return result.code().unwrap_or(1);
                }
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(failure) => {
                    error(format_args!("could not observe {harness}: {failure}"));
                    return 1;
                }
            },
        }
    }
    match child.wait() {
        Ok(result) => result.code().unwrap_or(1),
        Err(failure) => {
            error(format_args!("could not wait for {harness}: {failure}"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::Path;
    use std::time::Duration;

    use multplx_core::error::{CoreError, Result as CoreResult};
    use multplx_core::process::{AncestryRow, ProcessIdentity, ProcessProbe, SystemProcessProbe};

    use super::{
        ConnectionRecord, ConversationState, LaunchReservation, LaunchedIdentityError,
        ReservationState, ancestor_contains, attach_live, canonical_directory, connection_path,
        conversation_state, cursor_args_safe, identify_launched_process, lifetime,
        remember_harness, remembered_harness, reservation_path, reservation_state, same_file,
        update_connection_owner, validate_home, validate_root,
    };

    struct FixtureProbe {
        alive: bool,
        identity: CoreResult<ProcessIdentity>,
        parents: HashMap<u32, u32>,
    }

    impl ProcessProbe for FixtureProbe {
        fn is_alive(&self, _pid: u32) -> bool {
            self.alive
        }

        fn identity(&self, _pid: u32) -> CoreResult<ProcessIdentity> {
            self.identity
                .as_ref()
                .map(Clone::clone)
                .map_err(|_| CoreError::InvalidIdentifier {
                    kind: "fixture process",
                    value: "unavailable".to_owned(),
                })
        }

        fn ancestry_row(&self, pid: u32) -> CoreResult<AncestryRow> {
            self.parents
                .get(&pid)
                .copied()
                .map(|parent_pid| AncestryRow {
                    parent_pid,
                    command: "fixture".to_owned(),
                    arguments: String::new(),
                })
                .ok_or_else(|| CoreError::InvalidIdentifier {
                    kind: "fixture process",
                    value: pid.to_string(),
                })
        }
    }

    struct LaunchIdentityProbe {
        attempts: Cell<usize>,
        failures_before_success: usize,
    }

    impl ProcessProbe for LaunchIdentityProbe {
        fn is_alive(&self, _pid: u32) -> bool {
            true
        }

        fn identity(&self, pid: u32) -> CoreResult<ProcessIdentity> {
            let attempt = self.attempts.get();
            self.attempts.set(attempt + 1);
            if attempt < self.failures_before_success {
                return Err(CoreError::MalformedRecord {
                    kind: "Linux process command line",
                    reason: "empty or oversized command line",
                });
            }
            Ok(ProcessIdentity {
                pid,
                marker: "linux-starttime=700 cmdline-hex=6d78".to_owned(),
            })
        }

        fn ancestry_row(&self, _pid: u32) -> CoreResult<AncestryRow> {
            unreachable!("launch identity retry does not inspect ancestry")
        }
    }

    #[test]
    fn launched_identity_retries_transient_reads_but_preserves_failure_and_exit() {
        let transient = LaunchIdentityProbe {
            attempts: Cell::new(0),
            failures_before_success: 2,
        };
        let identity = identify_launched_process(
            42,
            &transient,
            Duration::from_secs(1),
            Duration::ZERO,
            || Ok(None),
        )
        .expect("transient process identity");
        assert_eq!(identity.pid, 42);
        assert_eq!(transient.attempts.get(), 3);

        let permanent = LaunchIdentityProbe {
            attempts: Cell::new(0),
            failures_before_success: usize::MAX,
        };
        assert!(matches!(
            identify_launched_process(43, &permanent, Duration::ZERO, Duration::ZERO, || Ok(None)),
            Err(LaunchedIdentityError::Unidentified(message))
                if message.contains("empty or oversized command line")
        ));
        assert_eq!(permanent.attempts.get(), 1);

        let exited = LaunchIdentityProbe {
            attempts: Cell::new(0),
            failures_before_success: usize::MAX,
        };
        assert_eq!(
            identify_launched_process(44, &exited, Duration::from_secs(1), Duration::ZERO, || Ok(
                Some(17)
            ),),
            Err(LaunchedIdentityError::Exited(17))
        );
        assert_eq!(exited.attempts.get(), 1);
    }

    #[test]
    fn cursor_refuses_unsafe_authority_and_worktree_flags() {
        assert!(cursor_args_safe(&[OsString::from("safe")]));
        assert!(!cursor_args_safe(&[OsString::from("--yolo")]));
        assert!(!cursor_args_safe(&[
            OsString::from("--sandbox"),
            OsString::from("disabled")
        ]));
        assert!(!cursor_args_safe(&[OsString::from("--worktree=owned")]));
    }

    #[test]
    fn lifetime_identity_survives_exec_but_rejects_pid_reuse() {
        let before = lifetime(ProcessIdentity {
            pid: 42,
            marker: "linux-starttime=700 cmdline-hex=6d78".to_owned(),
        });
        let after = lifetime(ProcessIdentity {
            pid: 42,
            marker: "linux-starttime=700 cmdline-hex=636f646578".to_owned(),
        });
        let reused = lifetime(ProcessIdentity {
            pid: 42,
            marker: "linux-starttime=701 cmdline-hex=636f646578".to_owned(),
        });
        assert_eq!(before, after);
        assert_ne!(after, reused);
    }

    #[test]
    fn conversation_descriptions_are_truthful_about_attachment() {
        assert!(
            ConversationState::Offline {
                remembered: Some("codex".into())
            }
            .description()
            .contains("remembered codex")
        );
        assert!(
            ConversationState::Live {
                pid: 7,
                harness: Some("claude".into()),
                attachable: false,
            }
            .description()
            .contains("send requests")
        );
        assert_eq!(
            ConversationState::Offline { remembered: None }.description(),
            "offline · choose a harness"
        );
        assert_eq!(
            ConversationState::Starting.description(),
            "starting in another terminal"
        );
        assert!(
            ConversationState::Live {
                pid: 8,
                harness: None,
                attachable: true,
            }
            .description()
            .contains("terminal connection available")
        );
        assert!(
            ConversationState::Unreadable
                .description()
                .contains("doctor")
        );
    }

    #[test]
    fn corrupt_launch_reservation_fails_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(temp.path().join("state")).expect("state");
        std::fs::write(temp.path().join("state/workspace-launch.json"), b"not json")
            .expect("reservation");
        assert_eq!(
            conversation_state(temp.path()),
            ConversationState::Unreadable
        );
    }

    #[test]
    fn runtime_and_home_validation_cover_package_and_filesystem_failures() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("runtime");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join(".agents/skills")).unwrap();
        std::fs::write(root.join("AGENTS.md"), "fixture\n").unwrap();
        std::fs::write(root.join("bin/mx-launcher.sh"), "#!/bin/sh\n").unwrap();
        let mut permissions = std::fs::metadata(root.join("bin/mx-launcher.sh"))
            .unwrap()
            .permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o700);
        std::fs::set_permissions(root.join("bin/mx-launcher.sh"), permissions).unwrap();
        std::fs::write(root.join(".multplx-release"), env!("CARGO_PKG_VERSION")).unwrap();
        assert_eq!(validate_root(&root), Ok(()));
        std::fs::write(root.join(".multplx-release"), "wrong").unwrap();
        assert_eq!(validate_root(&root), Err(2));
        std::fs::write(root.join(".multplx-release"), [0xff]).unwrap();
        assert_eq!(validate_root(&root), Err(2));
        std::fs::remove_file(root.join("AGENTS.md")).unwrap();
        assert_eq!(validate_root(&root), Err(2));

        let home = temp.path().join("home");
        for part in ["config", "data", "projects", "state"] {
            std::fs::create_dir_all(home.join(part)).unwrap();
        }
        assert_eq!(validate_home(&home), Ok(()));
        std::fs::remove_dir(home.join("projects")).unwrap();
        std::os::unix::fs::symlink(temp.path(), home.join("projects")).unwrap();
        assert_eq!(validate_home(&home), Err(2));
        assert_eq!(validate_home(Path::new("/")), Err(2));
        assert!(canonical_directory(&root, "fixture").is_ok());
        assert_eq!(
            canonical_directory(&temp.path().join("missing"), "fixture"),
            Err(2)
        );
        assert!(!same_file(&root, &temp.path().join("missing")));
        assert!(!same_file(&temp.path().join("missing"), &root));
    }

    #[test]
    fn remembered_harness_is_validated_and_round_trips() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(temp.path().join("config")).unwrap();
        assert!(remembered_harness(temp.path()).is_none());
        assert!(remember_harness(temp.path(), "other").is_err());
        remember_harness(temp.path(), "codex").unwrap();
        assert_eq!(remembered_harness(temp.path()).as_deref(), Some("codex"));
        std::fs::write(temp.path().join("config/primary-harness"), "other\n").unwrap();
        assert!(remembered_harness(temp.path()).is_none());
    }

    #[test]
    fn reservation_records_distinguish_live_dead_and_unreadable_owners() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("state")).unwrap();
        let probe = SystemProcessProbe::default();
        assert!(matches!(
            reservation_state(temp.path(), &probe),
            ReservationState::MissingOrDead
        ));

        std::os::unix::fs::symlink("missing", reservation_path(temp.path())).unwrap();
        assert!(matches!(
            reservation_state(temp.path(), &probe),
            ReservationState::Unknown
        ));
        std::fs::remove_file(reservation_path(temp.path())).unwrap();
        std::fs::write(reservation_path(temp.path()), b"{}").unwrap();
        assert!(matches!(
            reservation_state(temp.path(), &probe),
            ReservationState::Unknown
        ));
        std::fs::write(
            reservation_path(temp.path()),
            br#"{"schema":"future","owner":{"pid":1,"started":"fixture"}}"#,
        )
        .unwrap();
        assert!(matches!(
            reservation_state(temp.path(), &probe),
            ReservationState::Unknown
        ));

        let identity = lifetime(probe.identity(std::process::id()).unwrap());
        let mut record = LaunchReservation {
            schema: "mx-workspace-launch.v1".into(),
            owner: identity.clone(),
        };
        std::fs::write(
            reservation_path(temp.path()),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            reservation_state(temp.path(), &probe),
            ReservationState::Live
        ));
        assert_eq!(conversation_state(temp.path()), ConversationState::Starting);

        let stale = LaunchReservation {
            schema: "mx-workspace-launch.v1".into(),
            owner: super::LifetimeIdentity {
                pid: identity.pid,
                started: "reused-start".into(),
            },
        };
        std::fs::write(
            reservation_path(temp.path()),
            serde_json::to_vec(&stale).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            reservation_state(temp.path(), &probe),
            ReservationState::MissingOrDead
        ));

        let unreadable_identity = FixtureProbe {
            alive: true,
            identity: Err(CoreError::InvalidIdentifier {
                kind: "fixture process",
                value: identity.pid.to_string(),
            }),
            parents: HashMap::new(),
        };
        record.owner = identity;
        std::fs::write(
            reservation_path(temp.path()),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            reservation_state(temp.path(), &unreadable_identity),
            ReservationState::Unknown
        ));
    }

    #[test]
    fn live_connection_refuses_missing_non_tmux_and_stale_tmux_routes() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("state")).unwrap();
        let probe = SystemProcessProbe::default();
        let owner = lifetime(probe.identity(std::process::id()).unwrap());
        assert_eq!(attach_live(temp.path(), owner.pid), 3);

        let mut record = ConnectionRecord {
            schema: "mx-workspace-connection.v1".into(),
            owner: Some(owner.clone()),
            harness: "codex".into(),
            caller_cwd: temp.path().into(),
            backend: Some("herdr".into()),
            target: Some("session:pane".into()),
            pane: None,
            tmux_socket: None,
        };
        std::fs::write(
            connection_path(temp.path()),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(attach_live(temp.path(), owner.pid), 3);
        record.backend = Some("tmux".into());
        record.target = None;
        std::fs::write(
            connection_path(temp.path()),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(attach_live(temp.path(), owner.pid), 3);
        record.target = Some("missing-session".into());
        record.pane = Some("%999999".into());
        std::fs::write(
            connection_path(temp.path()),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(attach_live(temp.path(), owner.pid), 3);
    }

    #[test]
    fn connection_owner_update_requires_and_rewrites_the_valid_record() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("state")).unwrap();
        let owner = super::LifetimeIdentity {
            pid: 71,
            started: "first".to_owned(),
        };
        assert!(update_connection_owner(temp.path(), owner.clone()).is_err());
        let record = ConnectionRecord {
            schema: "mx-workspace-connection.v1".into(),
            owner: Some(owner),
            harness: "codex".into(),
            caller_cwd: temp.path().into(),
            backend: None,
            target: None,
            pane: None,
            tmux_socket: None,
        };
        std::fs::write(
            connection_path(temp.path()),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let replacement = super::LifetimeIdentity {
            pid: 72,
            started: "second".to_owned(),
        };
        update_connection_owner(temp.path(), replacement.clone()).unwrap();
        assert_eq!(
            super::read_connection(temp.path()).unwrap().owner,
            Some(replacement)
        );
    }

    #[test]
    fn ancestry_walk_covers_matches_boundaries_and_unreadable_rows() {
        let probe = FixtureProbe {
            alive: true,
            identity: Ok(ProcessIdentity {
                pid: 9,
                marker: "fixture".to_owned(),
            }),
            parents: HashMap::from([(9, 8), (8, 7), (7, 1)]),
        };
        assert!(ancestor_contains(9, 9, &probe));
        assert!(ancestor_contains(9, 8, &probe));
        assert!(ancestor_contains(9, 1, &probe));
        assert!(!ancestor_contains(9, 6, &probe));
        assert!(!ancestor_contains(44, 8, &probe));

        let self_parent = FixtureProbe {
            alive: true,
            identity: Ok(ProcessIdentity {
                pid: 3,
                marker: "fixture".to_owned(),
            }),
            parents: HashMap::from([(3, 3)]),
        };
        assert!(ancestor_contains(3, 3, &self_parent));
        assert!(!ancestor_contains(3, 2, &self_parent));
    }
}

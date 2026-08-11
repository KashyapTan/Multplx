//! Guarded Herdr lab, CI cleanup, and pinned installation tooling.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Exact required real-Herdr CI pin.
pub const CI_VERSION: &str = "0.7.4";
/// Protocol floor for the complete real-Herdr family.
pub const CI_MIN_PROTOCOL: u64 = 16;
/// Bounded official asset download ceiling.
pub const CI_MAX_BYTES: u64 = 25_000_000;
const CI_REPOSITORY: &str = "ogulcancelik/herdr";

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Message(String),
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("malformed Herdr JSON: {0}")]
    Json(#[from] serde_json::Error),
}

fn io(context: &'static str) -> impl FnOnce(std::io::Error) -> ToolError {
    move |source| ToolError::Io { context, source }
}

fn herdr_bin() -> OsString {
    std::env::var_os("MX_HERDR_BIN").unwrap_or_else(|| OsString::from("herdr"))
}

fn herdr_available() -> bool {
    let executable = PathBuf::from(herdr_bin());
    if executable.components().count() > 1 {
        return executable.is_file();
    }
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join(&executable).is_file())
    })
}

fn herdr_output(
    session: Option<&str>,
    args: &[OsString],
) -> Result<std::process::Output, ToolError> {
    let mut command = Command::new(herdr_bin());
    command.args(args);
    if let Some(session) = session {
        command.args([OsStr::new("--session"), OsStr::new(session)]);
        command.env("HERDR_SESSION", session);
    }
    command.output().map_err(io("execute herdr"))
}

fn herdr_json(session: Option<&str>, args: &[&str]) -> Result<Value, ToolError> {
    let output = herdr_output(
        session,
        &args.iter().map(OsString::from).collect::<Vec<_>>(),
    )?;
    if !output.status.success() {
        return Err(ToolError::Message(format!(
            "Herdr command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn session_list(session: Option<&str>) -> Result<Value, ToolError> {
    herdr_json(session, &["session", "list", "--json"])
}

fn session_entry<'a>(list: &'a Value, name: &str) -> Option<&'a Value> {
    list.pointer("/sessions")
        .and_then(Value::as_array)?
        .iter()
        .find(|entry| entry.get("name").and_then(Value::as_str) == Some(name))
}

/// Exact lab name validation.
pub fn valid_lab_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix("mx-lab-") else {
        return false;
    };
    !suffix.is_empty()
        && suffix.as_bytes()[0].is_ascii_alphanumeric()
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn require_lab_name(name: &str) -> Result<(), ToolError> {
    if valid_lab_name(name) && name != "default" {
        Ok(())
    } else if name == "default" {
        Err(ToolError::Message(
            "refusing session name 'default'".to_owned(),
        ))
    } else if name.is_empty() {
        Err(ToolError::Message(
            "refusing an empty session name".to_owned(),
        ))
    } else {
        Err(ToolError::Message(format!(
            "session name must start with 'mx-lab-' and contain only letters, digits, underscores, or dashes: {name}"
        )))
    }
}

fn lab_state_dir() -> PathBuf {
    std::env::var_os("MX_HERDR_LAB_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "mx-herdr-lab-{}",
                rustix::process::getuid().as_raw()
            ))
        })
}

fn tripwire_path(name: &str) -> PathBuf {
    lab_state_dir().join(format!("{name}.system-state.json"))
}

fn system_state(session: &str) -> Result<Vec<u8>, ToolError> {
    let list = session_list(Some(session))?;
    let defaults = list
        .pointer("/sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("default").and_then(Value::as_bool) == Some(true))
        .collect::<Vec<_>>();
    if defaults.len() != 1
        || defaults[0].get("name").and_then(Value::as_str) != Some("default")
        || defaults[0].get("running").and_then(Value::as_bool) != Some(true)
    {
        return Err(ToolError::Message(
            "system-state tripwire requires exactly one running default session".to_owned(),
        ));
    }
    let entry = defaults[0];
    let compact = json!({
        "name": entry.get("name").cloned().unwrap_or(Value::Null),
        "default": entry.get("default").cloned().unwrap_or(Value::Null),
        "running": entry.get("running").cloned().unwrap_or(Value::Null),
        "socket_path": entry.get("socket_path").cloned().unwrap_or(Value::Null),
    });
    let mut bytes = serde_json::to_vec(&compact)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn prepare_lab(name: &str) -> Result<(), ToolError> {
    require_lab_name(name)?;
    let list = session_list(Some(name))?;
    if session_entry(&list, name).is_some() {
        return Err(ToolError::Message(format!(
            "session '{name}' already exists; refusing to adopt or overwrite it"
        )));
    }
    let state = lab_state_dir();
    fs::create_dir_all(&state).map_err(io("create lab state directory"))?;
    let path = tripwire_path(name);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| {
            ToolError::Message(format!(
                "tripwire already exists for '{name}'; refusing ambiguous ownership: {error}"
            ))
        })?;
    let state = system_state(name)?;
    if let Err(error) = file.write_all(&state).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(ToolError::Io {
            context: "write lab tripwire",
            source: error,
        });
    }
    Ok(())
}

fn refuse_if_default(name: &str) -> Result<(), ToolError> {
    require_lab_name(name)?;
    let list = session_list(Some(name))?;
    if session_entry(&list, name)
        .and_then(|entry| entry.get("default"))
        .and_then(Value::as_bool)
        == Some(false)
    {
        Ok(())
    } else {
        Err(ToolError::Message(format!(
            "refusing destructive call for '{name}': session is absent or default"
        )))
    }
}

fn check_tripwire(name: &str) -> Result<(), ToolError> {
    let before = fs::read(tripwire_path(name)).map_err(|_| {
        ToolError::Message(format!(
            "missing system-state tripwire for '{name}'; refusing unverified teardown"
        ))
    })?;
    let after = system_state(name)?;
    if before == after {
        Ok(())
    } else {
        Err(ToolError::Message(format!(
            "SYSTEM-STATE TRIPWIRE FAILED: default session changed during lab work\nbefore: {}\nafter:  {}",
            String::from_utf8_lossy(&before).trim(),
            String::from_utf8_lossy(&after).trim()
        )))
    }
}

fn cancel_child(child: &mut Child) {
    let _ = rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.id() as i32).unwrap_or(rustix::process::Pid::INIT),
        rustix::process::Signal::TERM,
    );
    for _ in 0..10 {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn provision_lab(name: &str) -> Result<(), ToolError> {
    require_lab_name(name)?;
    let list = session_list(Some(name))?;
    if let Some(entry) = session_entry(&list, name) {
        if !tripwire_path(name).is_file() {
            return Err(ToolError::Message(format!(
                "missing system-state tripwire for existing session '{name}'; refusing to adopt it"
            )));
        }
        refuse_if_default(name)?;
        if entry.get("running").and_then(Value::as_bool) != Some(false) {
            return Err(ToolError::Message(format!(
                "session '{name}' is not stopped; refusing to re-provision it"
            )));
        }
        check_tripwire(name)?;
    } else {
        prepare_lab(name)?;
    }
    let mut child = Command::new(herdr_bin())
        .args([
            OsStr::new("server"),
            OsStr::new("--session"),
            OsStr::new(name),
        ])
        .env("HERDR_SESSION", name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(io("start lab server"))?;
    for _ in 0..300 {
        let running = herdr_json(Some(name), &["status", "--json"])
            .ok()
            .and_then(|value| value.pointer("/server/running").and_then(Value::as_bool))
            == Some(true);
        if running {
            if let Err(error) = refuse_if_default(name) {
                cancel_child(&mut child);
                return Err(error);
            }
            thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
    cancel_child(&mut child);
    Err(ToolError::Message(format!(
        "lab session '{name}' did not report running within 60 seconds"
    )))
}

fn run_allowed(name: &str, args: &[OsString]) -> Result<ExitStatus, ToolError> {
    require_lab_name(name)?;
    let first = args
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if args.is_empty() {
        return Err(ToolError::Message(
            "run requires Herdr arguments".to_owned(),
        ));
    }
    if first.starts_with('-') {
        return Err(ToolError::Message(
            "run forbids a leading option before the Herdr subcommand".to_owned(),
        ));
    }
    if args.iter().any(|argument| {
        argument == "--session"
            || argument
                .to_str()
                .is_some_and(|argument| argument.starts_with("--session="))
    }) {
        return Err(ToolError::Message(
            "run forbids caller-supplied --session; the helper appends the lab session".to_owned(),
        ));
    }
    let second = args
        .get(1)
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if first == "server" {
        return Err(ToolError::Message(
            "run forbids server operations; use provision for the named lab server".to_owned(),
        ));
    }
    if first == "session" && second != "list" {
        return Err(ToolError::Message(
            "run forbids session lifecycle operations; use guarded teardown".to_owned(),
        ));
    }
    Command::new(herdr_bin())
        .args(args)
        .args([OsStr::new("--session"), OsStr::new(name)])
        .env("HERDR_SESSION", name)
        .status()
        .map_err(io("run guarded Herdr command"))
}

fn stop_lab(name: &str) -> Result<(), ToolError> {
    require_lab_name(name)?;
    if !tripwire_path(name).is_file() {
        return Err(ToolError::Message(format!(
            "missing system-state tripwire for '{name}'; refusing stop"
        )));
    }
    refuse_if_default(name)?;
    let output = herdr_output(
        Some(name),
        &["session", "stop", name, "--json"]
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>(),
    )?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ToolError::Message("session stop failed".to_owned()))
    }
}

fn teardown_lab(name: &str) -> Result<(), ToolError> {
    require_lab_name(name)?;
    let tripwire = tripwire_path(name);
    if !tripwire.is_file() {
        return Err(ToolError::Message(format!(
            "missing system-state tripwire for '{name}'; refusing destructive calls"
        )));
    }
    if session_entry(&session_list(Some(name))?, name).is_none() {
        check_tripwire(name)?;
        fs::remove_file(tripwire).map_err(io("remove lab tripwire"))?;
        return Ok(());
    }
    let _ = stop_lab(name);
    thread::sleep(Duration::from_millis(500));
    refuse_if_default(name)?;
    let _ = herdr_output(
        Some(name),
        &["session", "delete", name, "--json"]
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>(),
    )?;
    if session_entry(&session_list(Some(name))?, name).is_some() {
        return Err(ToolError::Message(format!(
            "lab session '{name}' remains after teardown"
        )));
    }
    check_tripwire(name)?;
    fs::remove_file(tripwire).map_err(io("remove lab tripwire"))?;
    Ok(())
}

/// Run the guarded lab command surface and return its process exit code.
pub fn run_lab(args: &[OsString]) -> i32 {
    let usage = "Usage:\n  mx-herdr-lab.sh name <label>\n  mx-herdr-lab.sh prepare <session>\n  mx-herdr-lab.sh provision <session>\n  mx-herdr-lab.sh run <session> <herdr arguments...>\n  mx-herdr-lab.sh stop <session>\n  mx-herdr-lab.sh teardown <session>";
    let command = args
        .first()
        .and_then(|arg| arg.to_str())
        .unwrap_or_default();
    if command == "run" && args.len() >= 3 {
        return match run_allowed(&args[1].to_string_lossy(), &args[2..]) {
            Ok(status) => status.code().unwrap_or(1),
            Err(error) => {
                eprintln!("mx-herdr-lab: {error}");
                1
            }
        };
    }
    let result = match command {
        "name" if args.len() == 2 => {
            let clean = args[1]
                .to_string_lossy()
                .chars()
                .filter(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
                .skip_while(|character| !character.is_ascii_alphanumeric())
                .take(16)
                .collect::<String>()
                .trim_end_matches('-')
                .to_owned();
            let label = if clean.is_empty() { "lab" } else { &clean };
            let random = random_u16().unwrap_or(0);
            println!("mx-lab-{label}-{}-{random}", std::process::id());
            Ok(())
        }
        "prepare" if args.len() == 2 => prepare_lab(&args[1].to_string_lossy()),
        "provision" if args.len() == 2 => provision_lab(&args[1].to_string_lossy()),
        "stop" if args.len() == 2 => stop_lab(&args[1].to_string_lossy()),
        "teardown" if args.len() == 2 => teardown_lab(&args[1].to_string_lossy()),
        "-h" | "--help" | "help" => {
            println!("{usage}");
            return 0;
        }
        _ => {
            eprintln!("{usage}");
            return 2;
        }
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("mx-herdr-lab: {error}");
            1
        }
    }
}

fn random_u16() -> Result<u16, ToolError> {
    let mut bytes = [0_u8; 2];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(io("read random bytes"))?;
    Ok(u16::from_ne_bytes(bytes))
}

/// Snapshot or tear down only job-owned, non-default lab sessions.
pub fn run_ci_cleanup(args: &[OsString]) -> i32 {
    let command = args
        .first()
        .and_then(|arg| arg.to_str())
        .unwrap_or_default();
    let Some(path) = args.get(1).map(PathBuf::from) else {
        eprintln!("mx-herdr-ci-cleanup.sh: usage: mx-herdr-ci-cleanup.sh snapshot|teardown <path>");
        return 1;
    };
    if args.len() != 2 {
        eprintln!("mx-herdr-ci-cleanup.sh: usage: mx-herdr-ci-cleanup.sh snapshot|teardown <path>");
        return 1;
    }
    if !herdr_available() {
        eprintln!("mx-herdr-ci-cleanup.sh: herdr not on PATH; nothing to {command}");
        return 0;
    }
    let result = match command {
        "snapshot" => ci_snapshot(&path),
        "teardown" => ci_teardown(&path),
        _ => Err(ToolError::Message(format!(
            "unknown command: {command} (use snapshot or teardown)"
        ))),
    };
    match result {
        Ok(message) => {
            eprintln!("mx-herdr-ci-cleanup.sh: {message}");
            0
        }
        Err(error) => {
            eprintln!("mx-herdr-ci-cleanup.sh: {error}");
            1
        }
    }
}

fn ci_snapshot(path: &Path) -> Result<String, ToolError> {
    let list = session_list(None)?;
    let mut names = list
        .pointer("/sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    let mut bytes = serde_json::to_vec(&names)?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(io("write session snapshot"))?;
    Ok(format!(
        "wrote session snapshot to {} ({} names)",
        path.display(),
        names.len()
    ))
}

fn ci_teardown(path: &Path) -> Result<String, ToolError> {
    let before: Vec<String> = serde_json::from_slice(&fs::read(path).map_err(|_| {
        ToolError::Message(format!("snapshot file not found: {}", path.display()))
    })?)?;
    let before = before.into_iter().collect::<std::collections::HashSet<_>>();
    let list = session_list(None)?;
    let candidates = list
        .pointer("/sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("default").and_then(Value::as_bool) == Some(false))
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .filter(|name| valid_lab_name(name) && !before.contains(*name))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok("no job-owned mx-lab-* sessions to clean".to_owned());
    }
    for name in candidates {
        let fresh = session_list(None)?;
        if session_entry(&fresh, &name)
            .and_then(|entry| entry.get("default"))
            .and_then(Value::as_bool)
            != Some(false)
        {
            return Err(ToolError::Message(format!(
                "refusing cleanup of '{name}' because fresh default proof failed"
            )));
        }
        let _ = herdr_output(
            None,
            &["session", "stop", &name, "--json"]
                .iter()
                .map(OsString::from)
                .collect::<Vec<_>>(),
        );
        thread::sleep(Duration::from_millis(300));
        let fresh = session_list(None)?;
        if session_entry(&fresh, &name)
            .and_then(|entry| entry.get("default"))
            .and_then(Value::as_bool)
            != Some(false)
        {
            return Err(ToolError::Message(format!(
                "refusing delete of '{name}' after stop"
            )));
        }
        let _ = herdr_output(
            None,
            &["session", "delete", &name, "--json"]
                .iter()
                .map(OsString::from)
                .collect::<Vec<_>>(),
        );
        if session_entry(&session_list(None)?, &name).is_some() {
            return Err(ToolError::Message(format!(
                "failed to delete lab session {name}"
            )));
        }
    }
    Ok("deleted all job-owned lab sessions".to_owned())
}

/// Install the exact verified Herdr CI asset.
pub fn run_installer(args: &[OsString]) -> i32 {
    if args.len() != 1 {
        eprintln!("mx-install-herdr.sh: usage: mx-install-herdr.sh <destination-directory>");
        return 1;
    }
    match install_herdr(Path::new(&args[0])) {
        Ok(version) => {
            println!("herdr {version}");
            0
        }
        Err(error) => {
            eprintln!("mx-install-herdr.sh: {error}");
            1
        }
    }
}

fn install_herdr(destination: &Path) -> Result<String, ToolError> {
    let (asset, expected) = asset_for_platform(std::env::consts::OS, std::env::consts::ARCH)?;
    let url = format!("https://github.com/{CI_REPOSITORY}/releases/download/v{CI_VERSION}/{asset}");
    let temporary = tempfile::Builder::new()
        .prefix("mx-herdr.")
        .tempdir_in(
            std::env::var_os("RUNNER_TEMP")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir),
        )
        .map_err(io("create installer temporary directory"))?;
    let download = temporary.path().join(asset);
    eprintln!("mx-install-herdr.sh: downloading {asset} from {url}");
    let status = Command::new("curl")
        .args([
            "-fsSL",
            "--max-filesize",
            &CI_MAX_BYTES.to_string(),
            &url,
            "-o",
        ])
        .arg(&download)
        .status()
        .map_err(io("run curl"))?;
    if !status.success() {
        return Err(ToolError::Message(format!(
            "download failed for {url} (bounded at {CI_MAX_BYTES} bytes)"
        )));
    }
    let metadata = fs::metadata(&download).map_err(io("inspect downloaded asset"))?;
    if metadata.len() > CI_MAX_BYTES {
        return Err(ToolError::Message(
            "download exceeded size limit".to_owned(),
        ));
    }
    let mut file = File::open(&download).map_err(io("open downloaded asset"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(io("hash downloaded asset"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(ToolError::Message(format!(
            "checksum mismatch for {asset} (expected {expected}, got {actual})"
        )));
    }
    fs::create_dir_all(destination).map_err(io("create install destination"))?;
    let installed = destination.join("herdr");
    fs::copy(&download, &installed).map_err(io("install Herdr asset"))?;
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o755))
        .map_err(io("set Herdr executable mode"))?;
    let version_output = Command::new(&installed)
        .arg("--version")
        .output()
        .map_err(io("run installed Herdr version"))?;
    let version = String::from_utf8_lossy(&version_output.stdout)
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    if version != CI_VERSION {
        return Err(ToolError::Message(format!(
            "installed herdr version is '{}', expected exact pin {CI_VERSION}",
            if version.is_empty() {
                "<empty>"
            } else {
                &version
            }
        )));
    }
    let status = Command::new(&installed)
        .args(["status", "--json"])
        .output()
        .map_err(io("run installed Herdr status"))?;
    let value: Value = serde_json::from_slice(&status.stdout)?;
    let protocol = value
        .pointer("/client/protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::Message("could not read herdr client protocol".to_owned()))?;
    if protocol < CI_MIN_PROTOCOL {
        return Err(ToolError::Message(format!(
            "herdr protocol {protocol} is below the required floor {CI_MIN_PROTOCOL}"
        )));
    }
    eprintln!(
        "mx-install-herdr.sh: installed herdr {version} (protocol {protocol}) to {}",
        installed.display()
    );
    Ok(version)
}

fn asset_for_platform(os: &str, arch: &str) -> Result<(&'static str, &'static str), ToolError> {
    match (os, arch) {
        ("linux", "x86_64") => Ok((
            "herdr-linux-x86_64",
            "bc0fc02d4ba500f9cac2353a43e67fe036785ecca6eb55378e050fac3c103059",
        )),
        ("linux", "aarch64") => Ok((
            "herdr-linux-aarch64",
            "544e0002de42806d1ab64ccdef3a7e7414f24717b0b6b022bc9e57d2eefd26a2",
        )),
        ("macos", "aarch64") => Ok((
            "herdr-macos-aarch64",
            "24992e1625dbdcb18354a59e299e4b263c312400b31396cdc07cd46ed57f24a7",
        )),
        ("macos", "x86_64") => Ok((
            "herdr-macos-x86_64",
            "ddf430133352e1712413d5d865b34a485546f4658893fc89986257d65a7585a8",
        )),
        _ => Err(ToolError::Message(format!(
            "unsupported platform {os}-{arch}; official Herdr assets are linux/macos x86_64 and aarch64"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{asset_for_platform, valid_lab_name};

    #[test]
    fn lab_names_fail_closed() {
        assert!(valid_lab_name("mx-lab-test_1"));
        for name in ["", "default", "mx-lab-", "mx-lab-/bad", "other"] {
            assert!(!valid_lab_name(name), "accepted {name}");
        }
    }

    #[test]
    fn pinned_platform_matrix_is_complete_and_exact() {
        assert_eq!(
            asset_for_platform("linux", "x86_64").expect("linux").0,
            "herdr-linux-x86_64"
        );
        assert_eq!(
            asset_for_platform("macos", "aarch64").expect("mac").0,
            "herdr-macos-aarch64"
        );
        assert!(asset_for_platform("windows", "x86_64").is_err());
    }
}

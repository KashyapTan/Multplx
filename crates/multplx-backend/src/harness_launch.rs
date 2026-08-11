//! Primary harness launch validation and transparent `exec` handoff.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Validate and replace the current process with one verified real harness.
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
    let lock = Command::new(root.join("bin/mx-lock.sh"))
        .arg("status")
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", &home)
        .output();
    let Ok(lock) = lock else {
        error("could not inspect the broker session lock");
        return 2;
    };
    if !lock.status.success() {
        error("could not inspect the broker session lock");
        return 2;
    }
    let lock_text =
        String::from_utf8_lossy(&lock.stdout).to_string() + &String::from_utf8_lossy(&lock.stderr);
    if let Some(holder) = lock_text
        .trim()
        .strip_prefix("lock: held by live harness pid ")
    {
        error(format_args!(
            "another live broker already owns this home (pid {holder})"
        ));
        return 3;
    }
    if harness == "cursor" && !cursor_args_safe(args) {
        error("Cursor launch refuses force, sandbox-disabled, and Cursor-owned worktree modes");
        return 2;
    }
    let mut command = Command::new(real);
    command
        .current_dir(&root)
        .env("MX_ROOT_OVERRIDE", &root)
        .env("MX_HOME", &home);
    if harness == "cursor" {
        command.args([OsString::from("--sandbox"), OsString::from("enabled")]);
    }
    command.args(args);
    let failure = command.exec();
    error(format_args!(
        "{harness} is not installed or its captured executable is no longer available: {failure}"
    ));
    127
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::cursor_args_safe;

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
}

//! Global launcher and installation boundary.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use multplx_core::filesystem::atomic_replace;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use rustix::fs::OFlags;
use sha2::{Digest, Sha256};

const LAUNCHER_HELP: &str = "Activate or operate one globally configured Multplx control plane.\n\nUsage:\n  multplx [shell]\n  multplx [--backend auto|tmux|herdr|cmux] [shell]\n  multplx [--backend auto|tmux|herdr|cmux] claude|codex|cursor|pi [args...]\n  multplx doctor [args...]\n  multplx update\n  multplx paths\n  multplx --help\n  multplx --version\n";

const INSTALL_HELP: &str = "Install the global `multplx` binary and register one code root and home.\n\nUsage:\n  mx launcher-install [--root PATH] [--home PATH] [--binary PATH] [--checksum SHA256]\n  mx launcher-install --managed [--source GIT-URL] [--binary PATH] [--checksum SHA256]\n  mx launcher-install --upgrade [shared options]\n  mx launcher-install --uninstall [shared options]\n\nShared options:\n  --bin-dir PATH       default ${XDG_BIN_HOME:-$HOME/.local/bin}\n  --config-dir PATH    default ${XDG_CONFIG_HOME:-$HOME/.config}/multplx\n  --data-dir PATH      default ${XDG_DATA_HOME:-$HOME/.local/share}/multplx\n  --binary PATH        verified prebuilt binary or explicit local release build\n  --checksum SHA256    required checksum for an external --binary artifact\n  --managed            clone a clean managed runtime under DATA_DIR/runtime\n  --upgrade            atomically replace a recognized installed binary\n  --uninstall          remove only the owned binary and root/home records\n  -h, --help\n";

fn error(message: impl AsRef<str>) {
    eprintln!("multplx: {}", message.as_ref());
}

fn canonical_dir(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!(
            "{label} directory does not exist: {}",
            path.display()
        ));
    }
    path.canonicalize()
        .map_err(|_| format!("cannot resolve {label} directory: {}", path.display()))
}

fn read_path_file(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| {
        format!(
            "path file is missing, linked, or not regular: {}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "path file is missing, linked, or not regular: {}",
            path.display()
        ));
    }
    let bytes = fs::read(path).map_err(|_| format!("cannot read path file: {}", path.display()))?;
    if bytes.is_empty() || bytes.last() != Some(&b'\n') || bytes[..bytes.len() - 1].contains(&b'\n')
    {
        return Err(format!(
            "path file must contain exactly one newline-terminated path: {}",
            path.display()
        ));
    }
    let body = &bytes[..bytes.len() - 1];
    if body.contains(&0) {
        return Err(format!(
            "path file must contain exactly one newline-terminated path: {}",
            path.display()
        ));
    }
    let value = std::str::from_utf8(body)
        .map_err(|_| format!("path file is not UTF-8: {}", path.display()))?;
    let parsed = PathBuf::from(value);
    if !parsed.is_absolute() {
        return Err(format!("path is not absolute in {}", path.display()));
    }
    Ok(parsed)
}

fn command_output(program: &str, args: &[&OsStr]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn validate_root(root: &Path) -> Result<(), String> {
    if !root.join("AGENTS.md").is_file() {
        return Err(format!(
            "code root is missing AGENTS.md: {}",
            root.display()
        ));
    }
    if !root.join("bin").is_dir() || !root.join(".agents/skills").is_dir() {
        return Err(format!(
            "code root is missing Multplx adapters or skills: {}",
            root.display()
        ));
    }
    let launcher = root.join("bin/mx-launcher.sh");
    if !is_executable(&launcher) {
        return Err(format!(
            "code root is missing an executable launcher: {}",
            launcher.display()
        ));
    }
    let git = fs::symlink_metadata(root.join(".git")).map_err(|_| {
        format!(
            "code root must be a plain checkout, not a linked worktree: {}",
            root.display()
        )
    })?;
    if git.file_type().is_symlink() || !git.is_dir() {
        return Err(format!(
            "code root must be a plain checkout, not a linked worktree: {}",
            root.display()
        ));
    }
    let top = command_output(
        "git",
        &[
            OsStr::new("-C"),
            root.as_os_str(),
            OsStr::new("rev-parse"),
            OsStr::new("--show-toplevel"),
        ],
    )?;
    let top = canonical_dir(Path::new(&top), "git top level")?;
    if top != root {
        return Err(format!(
            "code root must be the checkout top level: {}",
            root.display()
        ));
    }
    Ok(())
}

fn validate_home(home: &Path) -> Result<(), String> {
    if home == Path::new("/") {
        return Err("operational home may not be the filesystem root".to_owned());
    }
    for part in ["config", "data", "projects", "state"] {
        let path = home.join(part);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return Err(format!(
                "operational home is missing a real {part} directory: {}",
                path.display()
            ));
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "operational home is missing a real {part} directory: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn is_managed(root: &Path) -> bool {
    command_output(
        "git",
        &[
            OsStr::new("-C"),
            root.as_os_str(),
            OsStr::new("config"),
            OsStr::new("--local"),
            OsStr::new("--get"),
            OsStr::new("multplx.managed"),
        ],
    )
    .as_deref()
        == Ok("true")
}

fn validate_managed_clean(root: &Path, home: &Path) -> Result<(), String> {
    if root == home {
        return Ok(());
    }
    let dirty = command_output(
        "git",
        &[
            OsStr::new("-C"),
            root.as_os_str(),
            OsStr::new("status"),
            OsStr::new("--porcelain"),
            OsStr::new("--untracked-files=normal"),
        ],
    )?;
    if dirty.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "managed runtime checkout is dirty; inspect or repair it before launch: {}",
            root.display()
        ))
    }
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn same_file(left: &Path, right: &Path) -> bool {
    let (Ok(left), Ok(right)) = (fs::metadata(left), fs::metadata(right)) else {
        return false;
    };
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn find_executable(name: &str, skip: &Path) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(name);
        if is_executable(&candidate) && !same_file(&candidate, &skip.join(name)) {
            return candidate.canonicalize().ok().or(Some(candidate));
        }
    }
    None
}

fn current_binary() -> Result<PathBuf, String> {
    env::current_exe().map_err(|error| format!("cannot locate running Multplx binary: {error}"))
}

fn apply_environment(
    command: &mut Command,
    environment: &[(OsString, OsString)],
    remove_backend: bool,
) {
    command.envs(environment.iter().cloned());
    if remove_backend {
        command.env_remove("MX_BACKEND");
    }
}

fn exec_binary(
    args: &[OsString],
    environment: &[(OsString, OsString)],
    remove_backend: bool,
) -> i32 {
    let binary = match current_binary() {
        Ok(binary) => binary,
        Err(message) => {
            error(message);
            return 2;
        }
    };
    let mut command = Command::new(binary);
    command.args(args);
    command.env("MX_MULTICALL_EXPLICIT", "1");
    apply_environment(&mut command, environment, remove_backend);
    let failure = command.exec();
    error(format!("could not execute Multplx command: {failure}"));
    1
}

fn capture_harnesses(shim: &Path) -> Vec<(OsString, OsString)> {
    let active = env::var("MULTPLX_ACTIVE").as_deref() == Ok("1");
    let mut result = Vec::new();
    for (variable, names) in [
        ("MX_REAL_CLAUDE", &["claude"][..]),
        ("MX_REAL_CODEX", &["codex"][..]),
        ("MX_REAL_CURSOR_AGENT", &["agent", "cursor-agent"][..]),
        ("MX_REAL_PI", &["pi"][..]),
    ] {
        let value = if active {
            env::var_os(variable).unwrap_or_default()
        } else {
            names
                .iter()
                .find_map(|name| find_executable(name, shim))
                .map_or_else(OsString::new, |path| path.into_os_string())
        };
        result.push((OsString::from(variable), value));
    }
    result
}

fn prepend_path_once(wanted: &Path) -> OsString {
    let current = env::var_os("PATH").unwrap_or_default();
    let mut entries = vec![wanted.to_path_buf()];
    entries.extend(env::split_paths(&current).filter(|entry| entry != wanted));
    env::join_paths(entries).unwrap_or_else(|_| wanted.as_os_str().to_owned())
}

fn resolve_launch_paths(
    config: Option<PathBuf>,
    default_root: &Path,
) -> Result<(PathBuf, PathBuf, Option<PathBuf>), String> {
    let (root, home, config) = if let Some(config) = config {
        let config = canonical_dir(&config, "launcher config")?;
        (
            read_path_file(&config.join("root"))?,
            read_path_file(&config.join("home"))?,
            Some(config),
        )
    } else if env::var_os("MX_ROOT_OVERRIDE").is_some() || env::var_os("MX_HOME").is_some() {
        let root = env::var_os("MX_ROOT_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| default_root.to_path_buf());
        let home = env::var_os("MX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone());
        (root, home, None)
    } else {
        let home_dir = env::var_os("HOME").ok_or("HOME is not set")?;
        let config = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home_dir).join(".config"))
            .join("multplx");
        if config.join("root").is_file() && config.join("home").is_file() {
            let config = canonical_dir(&config, "launcher config")?;
            (
                read_path_file(&config.join("root"))?,
                read_path_file(&config.join("home"))?,
                Some(config),
            )
        } else {
            (default_root.to_path_buf(), default_root.to_path_buf(), None)
        }
    };
    Ok((
        canonical_dir(&root, "code root")?,
        canonical_dir(&home, "operational home")?,
        config,
    ))
}

/// Run the installed `multplx` command surface.
pub(crate) fn run(args: &[OsString]) -> i32 {
    let mut values = args.to_vec();
    let mut config = env::var_os("MX_LAUNCH_CONFIG_DIR").map(PathBuf::from);
    if config.is_none()
        && let Ok(binary) = current_binary()
    {
        let companion = binary
            .parent()
            .unwrap_or(Path::new("."))
            .join(".multplx-config");
        match fs::symlink_metadata(&companion) {
            Ok(_) => match read_path_file(&companion) {
                Ok(path) => config = Some(path),
                Err(message) => {
                    error(message);
                    return 2;
                }
            },
            Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {}
            Err(error_value) => {
                error(format!(
                    "cannot inspect launcher config pointer {}: {error_value}",
                    companion.display()
                ));
                return 2;
            }
        }
    }
    if values.first().is_some_and(|value| value == "--config-dir") {
        if values.len() < 2 {
            error("--config-dir requires a path");
            return 2;
        }
        config = Some(PathBuf::from(&values[1]));
        values.drain(..2);
    }
    let mut backend: Option<String> = None;
    while let Some(value) = values.first().and_then(|value| value.to_str()) {
        match value {
            "-h" | "--help" => {
                print!("{LAUNCHER_HELP}");
                return 0;
            }
            "--version" => {
                println!("multplx {}", env!("CARGO_PKG_VERSION"));
                return 0;
            }
            "--backend" => {
                if values.len() < 2 {
                    error("--backend requires auto, tmux, herdr, or cmux");
                    return 2;
                }
                backend = values[1].to_str().map(str::to_owned);
                values.drain(..2);
            }
            _ if value.starts_with("--backend=") => {
                backend = Some(value[10..].to_owned());
                values.remove(0);
            }
            "--" => {
                values.remove(0);
                break;
            }
            _ if value.starts_with('-') => {
                error(format!("unknown option: {value}"));
                return 2;
            }
            _ => break,
        }
    }
    if backend
        .as_deref()
        .is_some_and(|value| !matches!(value, "auto" | "tmux" | "herdr" | "cmux"))
    {
        error(format!(
            "unsupported backend '{}'; use auto, tmux, herdr, or cmux",
            backend.unwrap()
        ));
        return 2;
    }
    let default_root = env::var_os("MX_LAUNCHER_DEFAULT_ROOT")
        .map(PathBuf::from)
        .or_else(|| env::var_os("MX_RUST_SOURCE_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let (root, home, config) = match resolve_launch_paths(config, &default_root) {
        Ok(paths) => paths,
        Err(message) => {
            error(message);
            return 2;
        }
    };
    if let Err(message) = validate_root(&root).and_then(|()| validate_home(&home)) {
        error(message);
        return 2;
    }
    if is_managed(&root)
        && let Err(message) = validate_managed_clean(&root, &home)
    {
        error(message);
        return 2;
    }
    let shim = root.join("share/shell/shims");
    if !shim.is_dir() {
        error(format!(
            "code root is missing harness shims: {}",
            shim.display()
        ));
        return 2;
    }
    let mut launch_environment = vec![
        (
            OsString::from("MX_ROOT_OVERRIDE"),
            root.as_os_str().to_owned(),
        ),
        (OsString::from("MX_HOME"), home.as_os_str().to_owned()),
        (OsString::from("MX_LAUNCH_VALIDATED"), OsString::from("1")),
        (OsString::from("MX_SHIM_DIR"), shim.as_os_str().to_owned()),
    ];
    if let Ok(binary) = current_binary() {
        launch_environment.push((
            OsString::from("MX_LAUNCH_BIN_PATH"),
            binary.into_os_string(),
        ));
    }
    if let Some(config) = &config {
        launch_environment.push((
            OsString::from("MX_LAUNCH_CONFIG_DIR"),
            config.as_os_str().to_owned(),
        ));
    }
    let remove_backend = backend.as_deref() == Some("auto");
    if let Some(value) = &backend {
        launch_environment.push((
            OsString::from("MX_LAUNCH_BACKEND_EXPLICIT"),
            OsString::from("1"),
        ));
        launch_environment.push((
            OsString::from("MX_LAUNCH_BACKEND_VALUE"),
            OsString::from(value),
        ));
        if value != "auto" {
            launch_environment.push((OsString::from("MX_BACKEND"), OsString::from(value)));
        }
    }
    let command = values
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or("shell")
        .to_owned();
    let tail = if values.is_empty() {
        &[][..]
    } else {
        &values[1..]
    };
    match command.as_str() {
        "paths" => {
            if !tail.is_empty() {
                error("paths accepts no arguments");
                return 2;
            }
            let bin = env::var_os("MX_LAUNCH_BIN_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| current_binary().unwrap_or_default());
            println!("root={}", root.display());
            println!("home={}", home.display());
            println!("bin={}", bin.display());
            println!(
                "config={}",
                config.map_or_else(
                    || "unregistered".to_owned(),
                    |path| path.display().to_string()
                )
            );
            0
        }
        "doctor" => {
            let mut forwarded = vec![OsString::from("session"), OsString::from("mx-doctor.sh")];
            forwarded.extend_from_slice(tail);
            exec_binary(&forwarded, &launch_environment, remove_backend)
        }
        "update" => {
            if !tail.is_empty() {
                error("update accepts no arguments");
                return 2;
            }
            exec_binary(
                &[OsString::from("update")],
                &launch_environment,
                remove_backend,
            )
        }
        "claude" | "codex" | "cursor" | "pi" => {
            launch_environment.extend(capture_harnesses(&shim));
            let mut forwarded = vec![OsString::from("launch-harness"), OsString::from(command)];
            forwarded.extend_from_slice(tail);
            exec_binary(&forwarded, &launch_environment, remove_backend)
        }
        "shell" => {
            if !tail.is_empty() {
                error("shell accepts no arguments");
                return 2;
            }
            if env::var("MULTPLX_ACTIVE").as_deref() == Ok("1") {
                error("a Multplx shell is already active; exit it before activating another");
                return 2;
            }
            launch_environment.extend(capture_harnesses(&shim));
            launch_environment.push((OsString::from("MULTPLX_ACTIVE"), OsString::from("1")));
            launch_environment.push((OsString::from("PATH"), prepend_path_once(&shim)));
            let shell = env::var_os("MX_LAUNCH_SHELL")
                .or_else(|| env::var_os("SHELL"))
                .map(PathBuf::from);
            let Some(shell) = shell else {
                error("SHELL is not set; choose Bash or Zsh with MX_LAUNCH_SHELL");
                return 2;
            };
            if !shell.is_absolute() || !is_executable(&shell) {
                error(format!(
                    "interactive shell is not executable: {}",
                    shell.display()
                ));
                return 2;
            }
            let name = shell.file_name().and_then(OsStr::to_str).unwrap_or("");
            let failure = match name {
                "bash" => {
                    let mut child = Command::new(&shell);
                    child
                        .arg("--rcfile")
                        .arg(root.join("share/shell/multplx.bash"))
                        .arg("-i");
                    apply_environment(&mut child, &launch_environment, remove_backend);
                    child.exec()
                }
                "zsh" => {
                    let adapter = match tempfile::Builder::new().prefix("multplx-zsh.").tempdir() {
                        Ok(directory) => directory.keep(),
                        Err(error_value) => {
                            error(format!(
                                "could not create Zsh adapter directory: {error_value}"
                            ));
                            return 2;
                        }
                    };
                    let rc = adapter.join(".zshrc");
                    if fs::copy(root.join("share/shell/multplx.zsh"), &rc).is_err()
                        || fs::set_permissions(&rc, fs::Permissions::from_mode(0o600)).is_err()
                    {
                        error("could not prepare Zsh adapter");
                        return 2;
                    }
                    match env::var_os("ZDOTDIR") {
                        Some(value) => {
                            launch_environment.push((
                                OsString::from("MX_ORIGINAL_ZDOTDIR_SET"),
                                OsString::from("1"),
                            ));
                            launch_environment.push((OsString::from("MX_ORIGINAL_ZDOTDIR"), value));
                        }
                        None => {
                            launch_environment.push((
                                OsString::from("MX_ORIGINAL_ZDOTDIR_SET"),
                                OsString::from("0"),
                            ));
                            launch_environment
                                .push((OsString::from("MX_ORIGINAL_ZDOTDIR"), OsString::new()));
                        }
                    }
                    launch_environment.push((
                        OsString::from("MX_ZSH_ADAPTER_DIR"),
                        adapter.as_os_str().to_owned(),
                    ));
                    launch_environment
                        .push((OsString::from("ZDOTDIR"), adapter.as_os_str().to_owned()));
                    let mut child = Command::new(&shell);
                    child.arg("-i");
                    apply_environment(&mut child, &launch_environment, remove_backend);
                    child.exec()
                }
                "sh" | "dash" | "ksh" | "ksh93" => {
                    eprintln!(
                        "multplx: activated (prompt integration is available for Bash and Zsh)"
                    );
                    let mut child = Command::new(&shell);
                    child.arg("-i");
                    apply_environment(&mut child, &launch_environment, remove_backend);
                    child.exec()
                }
                _ => {
                    error(format!(
                        "unsupported interactive shell '{name}'; use Bash or Zsh"
                    ));
                    return 2;
                }
            };
            error(format!("could not launch interactive shell: {failure}"));
            1
        }
        _ => {
            error(format!("unknown command '{command}'; run multplx --help"));
            2
        }
    }
}

#[derive(Default)]
struct InstallOptions {
    managed: bool,
    upgrade: bool,
    uninstall: bool,
    root: Option<PathBuf>,
    home: Option<PathBuf>,
    source: Option<OsString>,
    binary: Option<PathBuf>,
    checksum: Option<String>,
    bin_dir: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
}

fn parse_installer(args: &[OsString]) -> Result<Option<InstallOptions>, String> {
    let mut options = InstallOptions::default();
    let mut index = 0;
    while index < args.len() {
        let value = args[index].to_string_lossy();
        let take = |index: &mut usize, label: &str| -> Result<OsString, String> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("{label} requires a value"))
        };
        match value.as_ref() {
            "-h" | "--help" => return Ok(None),
            "--managed" => options.managed = true,
            "--upgrade" => options.upgrade = true,
            "--uninstall" => options.uninstall = true,
            "--root" => options.root = Some(take(&mut index, "--root")?.into()),
            "--home" => options.home = Some(take(&mut index, "--home")?.into()),
            "--source" => options.source = Some(take(&mut index, "--source")?),
            "--binary" => options.binary = Some(take(&mut index, "--binary")?.into()),
            "--checksum" => {
                options.checksum = Some(
                    take(&mut index, "--checksum")?
                        .to_string_lossy()
                        .into_owned(),
                )
            }
            "--bin-dir" => options.bin_dir = Some(take(&mut index, "--bin-dir")?.into()),
            "--config-dir" => options.config_dir = Some(take(&mut index, "--config-dir")?.into()),
            "--data-dir" => options.data_dir = Some(take(&mut index, "--data-dir")?.into()),
            _ => return Err(format!("unknown argument: {value}")),
        }
        index += 1;
    }
    Ok(Some(options))
}

fn ensure_dir(path: &Path, mode: u32, secure_existing: bool) -> Result<PathBuf, String> {
    let existed = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!(
                    "refusing linked or non-directory installation path: {}",
                    path.display()
                ));
            }
            true
        }
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => false,
        Err(error_value) => {
            return Err(format!(
                "cannot inspect installation path {}: {error_value}",
                path.display()
            ));
        }
    };
    fs::create_dir_all(path)
        .map_err(|_| format!("could not create directory: {}", path.display()))?;
    let canonical = canonical_dir(path, "installation")?;
    if fs::metadata(&canonical)
        .map_err(|error_value| error_value.to_string())?
        .uid()
        != rustix::process::geteuid().as_raw()
    {
        return Err(format!(
            "installation directory must be owned by the current user: {}",
            canonical.display()
        ));
    }
    if !existed || secure_existing {
        fs::set_permissions(&canonical, fs::Permissions::from_mode(mode))
            .map_err(|_| format!("could not secure directory: {}", canonical.display()))?;
    }
    Ok(canonical)
}

fn require_owned_dir(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error_value| format!("cannot inspect ownership of {label}: {error_value}"))?;
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(format!(
            "{label} must be owned by the current user: {}",
            path.display()
        ));
    }
    Ok(())
}

fn require_existing_owned_dir(path: &Path, label: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(format!(
            "{label} is linked or not a directory: {}",
            path.display()
        )),
        Ok(_) => require_owned_dir(path, label),
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error_value) => Err(format!("cannot inspect {label}: {error_value}")),
    }
}

fn require_recordable_path(path: &Path, label: &str) -> Result<(), String> {
    let Some(value) = path.to_str() else {
        return Err(format!("{label} path is not valid UTF-8"));
    };
    if value.contains(['\n', '\r']) {
        return Err(format!("{label} path contains a line break"));
    }
    Ok(())
}

struct VerifiedArtifact {
    bytes: Vec<u8>,
    hash: String,
}

fn verify_artifact(path: &Path, expected: Option<&str>) -> Result<VerifiedArtifact, String> {
    require_recordable_path(path, "binary artifact")?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(OFlags::NOFOLLOW.bits() as i32);
    let mut source = options.open(path).map_err(|error_value| {
        format!("cannot open binary artifact without following links: {error_value}")
    })?;
    let metadata = source
        .metadata()
        .map_err(|error_value| format!("cannot inspect binary artifact: {error_value}"))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "binary artifact is not an executable regular file: {}",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    source
        .read_to_end(&mut bytes)
        .map_err(|error_value| format!("cannot stage binary artifact: {error_value}"))?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if expected.is_some_and(|expected| expected != hash) {
        return Err("--binary requires its exact lowercase SHA-256 through --checksum".to_owned());
    }
    Ok(VerifiedArtifact { bytes, hash })
}

#[derive(Clone)]
struct GenerationFile {
    key: &'static str,
    path: PathBuf,
    mode: u32,
    desired: Option<Vec<u8>>,
}

fn transaction_path(config_dir: &Path) -> PathBuf {
    config_dir.join(".launcher-install.transaction")
}

fn transaction_manifest(files: &[GenerationFile]) -> Vec<u8> {
    let rows = files
        .iter()
        .map(|file| {
            serde_json::json!({
                "key": file.key,
                "path": file.path.to_str().expect("recordable generation path"),
                "mode": file.mode,
            })
        })
        .collect::<Vec<_>>();
    let mut bytes = serde_json::to_vec(&rows).expect("serialize transaction manifest");
    bytes.push(b'\n');
    bytes
}

fn remove_transaction(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error_value| format!("cannot inspect launcher transaction: {error_value}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "launcher transaction is linked or not a directory: {}",
            path.display()
        ));
    }
    fs::remove_dir_all(path)
        .map_err(|error_value| format!("cannot remove launcher transaction: {error_value}"))
}

fn restore_generation(transaction: &Path, files: &[GenerationFile]) -> Result<(), String> {
    let mut failures = Vec::new();
    for file in files {
        let backup = transaction.join(format!("old-{}", file.key));
        if backup.exists() {
            let restored_mode =
                fs::read_to_string(transaction.join(format!("old-mode-{}", file.key)))
                    .ok()
                    .and_then(|value| u32::from_str_radix(value.trim(), 8).ok())
                    .unwrap_or(file.mode);
            match fs::read(&backup).and_then(|bytes| {
                atomic_replace(&file.path, &bytes, restored_mode).map_err(std::io::Error::other)
            }) {
                Ok(()) => {}
                Err(error_value) => failures.push(format!("{}: {error_value}", file.key)),
            }
        } else if let Err(error_value) = fs::remove_file(&file.path)
            && error_value.kind() != std::io::ErrorKind::NotFound
        {
            failures.push(format!("{}: {error_value}", file.key));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "could not roll back launcher generation: {}",
            failures.join(", ")
        ))
    }
}

fn recover_generation(config_dir: &Path, files: &[GenerationFile]) -> Result<(), String> {
    let transaction = transaction_path(config_dir);
    match fs::symlink_metadata(&transaction) {
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error_value) => {
            return Err(format!(
                "cannot inspect launcher transaction: {error_value}"
            ));
        }
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(format!(
                "launcher transaction is linked or not a directory: {}",
                transaction.display()
            ));
        }
        Ok(_) => {}
    }
    let state = match fs::read(transaction.join("state")) {
        Ok(bytes) => bytes,
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {
            return remove_transaction(&transaction);
        }
        Err(error_value) => return Err(format!("cannot read launcher transaction: {error_value}")),
    };
    let expected = transaction_manifest(files);
    if !fs::read(transaction.join("manifest")).is_ok_and(|bytes| bytes == expected) {
        return Err("launcher transaction targets do not match this invocation".to_owned());
    }
    match state.as_slice() {
        b"prepared\n" => {
            restore_generation(&transaction, files)?;
            remove_transaction(&transaction)
        }
        b"committed\n" => remove_transaction(&transaction),
        _ => Err("launcher transaction state is malformed".to_owned()),
    }
}

fn apply_generation(config_dir: &Path, files: &[GenerationFile]) -> Result<(), String> {
    let transaction = transaction_path(config_dir);
    fs::create_dir(&transaction)
        .map_err(|error_value| format!("cannot create launcher transaction: {error_value}"))?;
    fs::set_permissions(&transaction, fs::Permissions::from_mode(0o700))
        .map_err(|error_value| format!("cannot secure launcher transaction: {error_value}"))?;
    atomic_replace(
        transaction.join("manifest"),
        &transaction_manifest(files),
        0o600,
    )
    .map_err(|error_value| error_value.to_string())?;
    for file in files {
        match fs::symlink_metadata(&file.path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(format!(
                    "refusing linked or non-regular generation target: {}",
                    file.path.display()
                ));
            }
            Ok(metadata) => {
                let old_mode = metadata.permissions().mode() & 0o777;
                let bytes = fs::read(&file.path).map_err(|error_value| error_value.to_string())?;
                atomic_replace(
                    transaction.join(format!("old-{}", file.key)),
                    &bytes,
                    file.mode,
                )
                .map_err(|error_value| error_value.to_string())?;
                atomic_replace(
                    transaction.join(format!("old-mode-{}", file.key)),
                    format!("{old_mode:o}\n").as_bytes(),
                    0o600,
                )
                .map_err(|error_value| error_value.to_string())?;
            }
            Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {}
            Err(error_value) => return Err(error_value.to_string()),
        }
    }
    atomic_replace(transaction.join("state"), b"prepared\n", 0o600)
        .map_err(|error_value| error_value.to_string())?;
    let commit = (|| {
        for file in files {
            match &file.desired {
                Some(bytes) => atomic_replace(&file.path, bytes, file.mode)
                    .map_err(|error_value| error_value.to_string())?,
                None => {
                    if let Err(error_value) = fs::remove_file(&file.path)
                        && error_value.kind() != std::io::ErrorKind::NotFound
                    {
                        return Err(error_value.to_string());
                    }
                }
            }
            if env::var("MX_LAUNCHER_INSTALL_CRASH_AFTER").as_deref() == Ok(file.key) {
                std::process::exit(97);
            }
            if env::var("MX_LAUNCHER_INSTALL_FAIL_AFTER").as_deref() == Ok(file.key) {
                return Err(format!(
                    "injected interruption after publishing {}",
                    file.key
                ));
            }
        }
        atomic_replace(transaction.join("state"), b"committed\n", 0o600)
            .map_err(|error_value| error_value.to_string())?;
        Ok::<(), String>(())
    })();
    if let Err(error_value) = commit {
        let rollback = restore_generation(&transaction, files);
        let cleanup = remove_transaction(&transaction);
        if let Err(rollback_error) = rollback {
            return Err(format!("{error_value}; {rollback_error}"));
        }
        cleanup?;
        return Err(error_value);
    }
    remove_transaction(&transaction)
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error_value| format!("cannot read binary artifact: {error_value}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error_value| error_value.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn absolute_from_cwd(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn default_source_root() -> Result<PathBuf, String> {
    if let Some(root) =
        env::var_os("MX_LAUNCHER_DEFAULT_ROOT").or_else(|| env::var_os("MX_RUST_SOURCE_ROOT"))
    {
        return canonical_dir(Path::new(&root), "code root");
    }
    let cwd = env::current_dir().map_err(|error_value| error_value.to_string())?;
    let top = command_output(
        "git",
        &[
            OsStr::new("-C"),
            cwd.as_os_str(),
            OsStr::new("rev-parse"),
            OsStr::new("--show-toplevel"),
        ],
    )?;
    canonical_dir(Path::new(&top), "code root")
}

/// Install or remove the global binary and path records.
pub(crate) fn run_installer(args: &[OsString]) -> i32 {
    let mut options = match parse_installer(args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{INSTALL_HELP}");
            return 0;
        }
        Err(message) => {
            error(message);
            return 2;
        }
    };
    let home_env = match env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => {
            error("HOME is not set");
            return 2;
        }
    };
    let bin_dir = options.bin_dir.take().unwrap_or_else(|| {
        env::var_os("XDG_BIN_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_env.join(".local/bin"))
    });
    let config_dir = options.config_dir.take().unwrap_or_else(|| {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_env.join(".config"))
            .join("multplx")
    });
    let data_dir = options.data_dir.take().unwrap_or_else(|| {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_env.join(".local/share"))
            .join("multplx")
    });
    if !bin_dir.is_absolute() || !config_dir.is_absolute() || !data_dir.is_absolute() {
        error("bin, config, and data directories must be absolute paths");
        return 2;
    }
    for (path, label) in [
        (&bin_dir, "binary installation"),
        (&config_dir, "launcher config"),
        (&data_dir, "launcher data"),
    ] {
        if let Err(message) = require_recordable_path(path, label) {
            error(message);
            return 2;
        }
    }
    for (path, label) in [
        (options.root.as_deref(), "code root"),
        (options.home.as_deref(), "operational home"),
        (options.binary.as_deref(), "binary artifact"),
    ] {
        if let Some(path) = path
            && let Err(message) = require_recordable_path(path, label)
        {
            error(message);
            return 2;
        }
    }
    if options
        .source
        .as_ref()
        .is_some_and(|source| source.to_str().is_none())
    {
        error("managed source is not valid UTF-8");
        return 2;
    }
    if options.uninstall
        && (options.managed
            || options.upgrade
            || options.root.is_some()
            || options.home.is_some()
            || options.source.is_some()
            || options.binary.is_some()
            || options.checksum.is_some())
    {
        error("--uninstall cannot be combined with install or upgrade options");
        return 2;
    }
    let artifact = if options.uninstall {
        None
    } else {
        let source_binary = options.binary.clone().unwrap_or_else(|| {
            current_binary().unwrap_or_else(|_| PathBuf::from("multplx-unavailable"))
        });
        let expected = options.binary.as_ref().and(options.checksum.as_deref());
        let artifact = match verify_artifact(&source_binary, expected) {
            Ok(artifact) => artifact,
            Err(message) => {
                error(message);
                return 2;
            }
        };
        if options.binary.is_some() && options.checksum.is_none() {
            error("--binary requires its exact lowercase SHA-256 through --checksum");
            return 2;
        }
        if options.checksum.is_some() && options.binary.is_none() {
            error("--checksum requires --binary");
            return 2;
        }
        Some(artifact)
    };
    let result = (|| -> Result<(), (i32, String)> {
        let target = bin_dir.join("multplx");
        let config_pointer = bin_dir.join(".multplx-config");
        let digest_record = config_dir.join("binary.sha256");
        if options.uninstall {
            let _uninstall_bin_lock = match fs::symlink_metadata(&bin_dir) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err((
                        2,
                        format!(
                            "binary installation directory is linked or not a directory: {}",
                            bin_dir.display()
                        ),
                    ));
                }
                Ok(_) => {
                    require_owned_dir(&bin_dir, "binary installation directory")
                        .map_err(|message| (2, message))?;
                    Some(
                        DirectoryLock::acquire_wait(
                            bin_dir.join(".multplx-install.lock"),
                            &SystemProcessProbe::default(),
                            Duration::from_secs(5),
                        )
                        .map_err(|error_value| {
                            (
                                2,
                                format!("cannot acquire launcher binary lock: {error_value}"),
                            )
                        })?,
                    )
                }
                Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => None,
                Err(error_value) => return Err((1, error_value.to_string())),
            };
            let _uninstall_lock = match fs::symlink_metadata(&config_dir) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err((
                        2,
                        format!(
                            "configuration directory is linked or not a directory: {}",
                            config_dir.display()
                        ),
                    ));
                }
                Ok(_) => {
                    require_owned_dir(&config_dir, "launcher config")
                        .map_err(|message| (2, message))?;
                    Some(
                        DirectoryLock::acquire_wait(
                            config_dir.join(".launcher-install.lock"),
                            &SystemProcessProbe::default(),
                            Duration::from_secs(5),
                        )
                        .map_err(|error_value| {
                            (
                                2,
                                format!("cannot acquire launcher install lock: {error_value}"),
                            )
                        })?,
                    )
                }
                Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => None,
                Err(error_value) => return Err((1, error_value.to_string())),
            };
            let generation = vec![
                GenerationFile {
                    key: "multplx",
                    path: target.clone(),
                    mode: 0o755,
                    desired: None,
                },
                GenerationFile {
                    key: "root",
                    path: config_dir.join("root"),
                    mode: 0o600,
                    desired: None,
                },
                GenerationFile {
                    key: "home",
                    path: config_dir.join("home"),
                    mode: 0o600,
                    desired: None,
                },
                GenerationFile {
                    key: "config",
                    path: config_pointer.clone(),
                    mode: 0o600,
                    desired: None,
                },
                GenerationFile {
                    key: "digest",
                    path: digest_record.clone(),
                    mode: 0o600,
                    desired: None,
                },
            ];
            if _uninstall_lock.is_some() {
                recover_generation(&config_dir, &generation).map_err(|message| (2, message))?;
            }
            let records = [
                config_dir.join("root"),
                config_dir.join("home"),
                digest_record.clone(),
                config_pointer.clone(),
            ];
            for path in &records {
                match fs::symlink_metadata(path) {
                    Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                        return Err((
                            2,
                            format!(
                                "refusing to remove linked or non-regular path record: {}",
                                path.display()
                            ),
                        ));
                    }
                    Ok(_) => {}
                    Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error_value) => return Err((1, error_value.to_string())),
                }
            }
            let target_exists = match fs::symlink_metadata(&target) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    return Err((
                        2,
                        format!(
                            "refusing to remove a linked or non-regular binary: {}",
                            target.display()
                        ),
                    ));
                }
                Ok(_) => true,
                Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => false,
                Err(error_value) => return Err((1, error_value.to_string())),
            };
            if target_exists {
                let expected = fs::read_to_string(&digest_record)
                    .ok()
                    .map(|value| value.trim().to_owned());
                if expected
                    .as_deref()
                    .is_none_or(|expected| hash_file(&target).as_deref() != Ok(expected))
                {
                    return Err((
                        2,
                        format!(
                            "refusing to remove an unrecognized binary: {}",
                            target.display()
                        ),
                    ));
                }
            }
            if _uninstall_lock.is_some() {
                apply_generation(&config_dir, &generation).map_err(|message| (1, message))?;
            } else if target_exists || records.iter().any(|path| path.exists()) {
                return Err((
                    2,
                    "refusing uninstall without an owned configuration directory".to_owned(),
                ));
            }
            println!("multplx: launcher removed; runtime and operational data preserved");
            return Ok(());
        }
        let default_root = default_source_root().map_err(|message| (2, message))?;
        require_recordable_path(&default_root, "code root").map_err(|message| (2, message))?;
        if options.managed {
            require_existing_owned_dir(&data_dir.join("runtime"), "managed code root")
                .map_err(|message| (2, message))?;
            require_existing_owned_dir(&data_dir.join("home"), "managed operational home")
                .map_err(|message| (2, message))?;
        } else {
            let requested_root = canonical_dir(
                &absolute_from_cwd(options.root.clone().unwrap_or_else(|| default_root.clone())),
                "code root",
            )
            .map_err(|message| (2, message))?;
            require_recordable_path(&requested_root, "code root")
                .map_err(|message| (2, message))?;
            require_owned_dir(&requested_root, "code root").map_err(|message| (2, message))?;
            if let Some(home) = options.home.as_ref() {
                let home = absolute_from_cwd(home.clone());
                require_existing_owned_dir(&home, "operational home")
                    .map_err(|message| (2, message))?;
            }
        }
        let bin_dir = ensure_dir(&bin_dir, 0o755, false).map_err(|message| (2, message))?;
        let config_dir = ensure_dir(&config_dir, 0o700, true).map_err(|message| (2, message))?;
        let data_dir = ensure_dir(&data_dir, 0o700, true).map_err(|message| (2, message))?;
        let _bin_lock = DirectoryLock::acquire_wait(
            bin_dir.join(".multplx-install.lock"),
            &SystemProcessProbe::default(),
            Duration::from_secs(5),
        )
        .map_err(|error_value| {
            (
                2,
                format!("cannot acquire launcher binary lock: {error_value}"),
            )
        })?;
        let _install_lock = DirectoryLock::acquire_wait(
            config_dir.join(".launcher-install.lock"),
            &SystemProcessProbe::default(),
            Duration::from_secs(5),
        )
        .map_err(|error_value| {
            (
                2,
                format!("cannot acquire launcher install lock: {error_value}"),
            )
        })?;
        let artifact = artifact.as_ref().expect("install artifact validated");
        let (root, home) = if options.managed {
            if options.root.is_some() || options.home.is_some() {
                return Err((
                    2,
                    "--managed cannot be combined with --root or --home".to_owned(),
                ));
            }
            let runtime = data_dir.join("runtime");
            let home = data_dir.join("home");
            if !runtime.exists() {
                let source = options.source.clone().unwrap_or_else(|| {
                    command_output(
                        "git",
                        &[
                            OsStr::new("-C"),
                            default_root.as_os_str(),
                            OsStr::new("remote"),
                            OsStr::new("get-url"),
                            OsStr::new("origin"),
                        ],
                    )
                    .map(OsString::from)
                    .unwrap_or_else(|_| OsString::from("https://github.com/KashyapTan/Multplx.git"))
                });
                let temporary = tempfile::Builder::new()
                    .prefix(".runtime.clone.")
                    .tempdir_in(&data_dir)
                    .map_err(|error_value| (1, error_value.to_string()))?;
                let candidate = temporary.path().to_path_buf();
                fs::remove_dir(&candidate).map_err(|error_value| (1, error_value.to_string()))?;
                let status = Command::new("git")
                    .args([OsStr::new("clone"), OsStr::new("--quiet"), OsStr::new("--")])
                    .arg(&source)
                    .arg(&candidate)
                    .status()
                    .map_err(|error_value| (1, error_value.to_string()))?;
                if !status.success() {
                    return Err((1, "managed runtime clone failed".to_owned()));
                }
                let candidate = canonical_dir(&candidate, "managed runtime candidate")
                    .map_err(|message| (2, message))?;
                validate_root(&candidate).map_err(|message| {
                    (
                        2,
                        format!(
                            "managed source did not produce a valid Multplx runtime: {message}"
                        ),
                    )
                })?;
                let status = Command::new("git")
                    .args([
                        "-C",
                        candidate.to_string_lossy().as_ref(),
                        "config",
                        "--local",
                        "multplx.managed",
                        "true",
                    ])
                    .status()
                    .map_err(|error_value| (1, error_value.to_string()))?;
                if !status.success() {
                    return Err((1, "could not mark managed runtime ownership".to_owned()));
                }
                if env::var("MX_LAUNCHER_INSTALL_FAIL_BEFORE").as_deref() == Ok("runtime") {
                    return Err((
                        1,
                        "injected interruption before publishing runtime".to_owned(),
                    ));
                }
                fs::rename(&candidate, &runtime)
                    .map_err(|error_value| (1, error_value.to_string()))?;
            }
            let root =
                canonical_dir(&runtime, "managed runtime").map_err(|message| (2, message))?;
            if !is_managed(&root) {
                return Err((
                    2,
                    format!(
                        "existing runtime was not created by the managed launcher installer: {}",
                        root.display()
                    ),
                ));
            }
            require_owned_dir(&root, "managed code root").map_err(|message| (2, message))?;
            let home = ensure_dir(&home, 0o700, true).map_err(|message| (2, message))?;
            (root, home)
        } else {
            if options.source.is_some() {
                return Err((2, "--source requires --managed".to_owned()));
            }
            let root = canonical_dir(
                &absolute_from_cwd(options.root.unwrap_or(default_root)),
                "code root",
            )
            .map_err(|message| (2, message))?;
            require_recordable_path(&root, "code root").map_err(|message| (2, message))?;
            require_owned_dir(&root, "code root").map_err(|message| (2, message))?;
            let home = match options.home {
                Some(home) => {
                    let home = absolute_from_cwd(home);
                    require_recordable_path(&home, "operational home")
                        .map_err(|message| (2, message))?;
                    ensure_dir(&home, 0o700, true).map_err(|message| (2, message))?
                }
                None => root.clone(),
            };
            (root, home)
        };
        require_recordable_path(&root, "code root").map_err(|message| (2, message))?;
        require_recordable_path(&home, "operational home").map_err(|message| (2, message))?;
        require_owned_dir(&root, "code root").map_err(|message| (2, message))?;
        require_owned_dir(&home, "operational home").map_err(|message| (2, message))?;
        for part in ["config", "data", "projects", "state"] {
            ensure_dir(&home.join(part), 0o700, true).map_err(|message| (2, message))?;
        }
        validate_root(&root).map_err(|message| (2, message))?;
        validate_home(&home).map_err(|message| (2, message))?;
        if options.managed {
            validate_managed_clean(&root, &home).map_err(|message| (2, message))?;
        }
        let generation = vec![
            GenerationFile {
                key: "multplx",
                path: target.clone(),
                mode: 0o755,
                desired: Some(artifact.bytes.clone()),
            },
            GenerationFile {
                key: "root",
                path: config_dir.join("root"),
                mode: 0o600,
                desired: Some(format!("{}\n", root.display()).into_bytes()),
            },
            GenerationFile {
                key: "home",
                path: config_dir.join("home"),
                mode: 0o600,
                desired: Some(format!("{}\n", home.display()).into_bytes()),
            },
            GenerationFile {
                key: "config",
                path: config_pointer.clone(),
                mode: 0o600,
                desired: Some(format!("{}\n", config_dir.display()).into_bytes()),
            },
            GenerationFile {
                key: "digest",
                path: digest_record.clone(),
                mode: 0o600,
                desired: Some(format!("{}\n", artifact.hash).into_bytes()),
            },
        ];
        recover_generation(&config_dir, &generation).map_err(|message| (2, message))?;
        if matches!(
            env::var("MX_LAUNCHER_INSTALL_FAIL_BEFORE").as_deref(),
            Ok("root" | "home" | "multplx")
        ) {
            return Err((
                1,
                format!(
                    "injected interruption before publishing {}",
                    env::var("MX_LAUNCHER_INSTALL_FAIL_BEFORE").unwrap_or_default()
                ),
            ));
        }
        match fs::symlink_metadata(&config_pointer) {
            Ok(_) => {
                let existing = read_path_file(&config_pointer).map_err(|message| (2, message))?;
                if existing != config_dir {
                    return Err((
                        2,
                        format!(
                            "refusing to replace conflicting config record: {}",
                            config_pointer.display()
                        ),
                    ));
                }
            }
            Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {}
            Err(error_value) => return Err((1, error_value.to_string())),
        }
        let existing_hash = match fs::symlink_metadata(&digest_record) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err((
                    2,
                    format!(
                        "binary digest record is linked or not regular: {}",
                        digest_record.display()
                    ),
                ));
            }
            Ok(_) => {
                let bytes =
                    fs::read(&digest_record).map_err(|error_value| (1, error_value.to_string()))?;
                if bytes.len() != 65
                    || bytes[64] != b'\n'
                    || !bytes[..64]
                        .iter()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
                {
                    return Err((
                        2,
                        format!(
                            "binary digest record is malformed: {}",
                            digest_record.display()
                        ),
                    ));
                }
                Some(String::from_utf8(bytes[..64].to_vec()).expect("ASCII digest"))
            }
            Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => None,
            Err(error_value) => return Err((1, error_value.to_string())),
        };
        for (name, expected) in [("root", &root), ("home", &home)] {
            let path = config_dir.join(name);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    let existing = read_path_file(&path).map_err(|message| (2, message))?;
                    if existing != *expected {
                        return Err((
                            2,
                            format!(
                                "refusing to replace conflicting {name} record: {}",
                                path.display()
                            ),
                        ));
                    }
                }
                Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {}
                Err(error_value) => return Err((1, error_value.to_string())),
            }
        }
        match fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err((
                    2,
                    format!(
                        "refusing linked or non-regular installation target: {}",
                        target.display()
                    ),
                ));
            }
            Ok(_) => {
                let installed_hash = hash_file(&target).map_err(|message| (1, message))?;
                if installed_hash != artifact.hash
                    && (!options.upgrade || existing_hash.as_deref() != Some(&installed_hash))
                {
                    return Err((
                        2,
                        format!(
                            "refusing to overwrite incompatible installation target: {}",
                            target.display()
                        ),
                    ));
                }
            }
            Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {}
            Err(error_value) => return Err((1, error_value.to_string())),
        }
        apply_generation(&config_dir, &generation).map_err(|message| (1, message))?;
        println!("multplx: installed {}", target.display());
        println!("multplx: root {}", root.display());
        println!("multplx: home {}", home.display());
        if !env::split_paths(&env::var_os("PATH").unwrap_or_default()).any(|path| path == bin_dir) {
            println!("multplx: add {} to PATH", bin_dir.display());
        }
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err((code, message)) => {
            error(message);
            code
        }
    }
}

/// Report whether an interrupted post-update launcher rebuild needs retrying.
pub(crate) fn registered_update_pending() -> Result<bool, String> {
    let (Some(config), Some(_installed)) = (
        env::var_os("MX_LAUNCH_CONFIG_DIR").map(PathBuf::from),
        env::var_os("MX_LAUNCH_BIN_PATH").map(PathBuf::from),
    ) else {
        return Ok(false);
    };
    require_recordable_path(&config, "launcher config")?;
    let config = canonical_dir(&config, "launcher config")?;
    require_owned_dir(&config, "launcher config")?;
    let marker = config.join(".launcher-update-pending");
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(format!(
            "launcher update marker is linked or not a regular file: {}",
            marker.display()
        )),
        Ok(_) => Ok(true),
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error_value) => Err(format!(
            "cannot inspect launcher update marker {}: {error_value}",
            marker.display()
        )),
    }
}

/// Rebuild and replace the registered launcher after its source checkout advances.
pub(crate) fn upgrade_registered_after_update(
    root: &Path,
    home: &Path,
) -> Result<Option<String>, String> {
    let (Some(config), Some(installed)) = (
        env::var_os("MX_LAUNCH_CONFIG_DIR").map(PathBuf::from),
        env::var_os("MX_LAUNCH_BIN_PATH").map(PathBuf::from),
    ) else {
        return Ok(None);
    };
    let root = canonical_dir(root, "code root")?;
    let home = canonical_dir(home, "operational home")?;
    require_recordable_path(&config, "launcher config")?;
    require_recordable_path(&installed, "installed binary")?;
    let config = canonical_dir(&config, "launcher config")?;
    require_owned_dir(&config, "launcher config")?;
    let _update_lock = DirectoryLock::acquire_wait(
        config.join(".launcher-update.lock"),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error_value| format!("cannot acquire launcher update lock: {error_value}"))?;
    let registered_root = read_path_file(&config.join("root"))?;
    let registered_home = read_path_file(&config.join("home"))?;
    if registered_root != root || registered_home != home {
        return Err(format!(
            "registered launcher paths changed during update (root {} != {}; home {} != {})",
            registered_root.display(),
            root.display(),
            registered_home.display(),
            home.display()
        ));
    }
    let bin_dir = installed
        .parent()
        .ok_or("installed binary has no parent directory")?;
    if installed.file_name() != Some(OsStr::new("multplx")) {
        return Err(format!(
            "registered launcher binary has an unexpected name: {}",
            installed.display()
        ));
    }
    require_owned_dir(bin_dir, "launcher binary directory")?;
    let pending = config.join(".launcher-update-pending");
    match fs::symlink_metadata(&pending) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(format!(
                "launcher update marker is linked or not a regular file: {}",
                pending.display()
            ));
        }
        Ok(_) => {}
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {}
        Err(error_value) => {
            return Err(format!(
                "cannot inspect launcher update marker {}: {error_value}",
                pending.display()
            ));
        }
    }
    atomic_replace(&pending, b"pending\n", 0o600)
        .map_err(|error_value| format!("cannot record pending launcher update: {error_value}"))?;
    let build = Command::new("cargo")
        .args(["build", "--release", "--locked", "-p", "multplx-cli"])
        .current_dir(&root)
        .status()
        .map_err(|error_value| format!("could not start release build: {error_value}"))?;
    if !build.success() {
        return Err("release build failed after source fast-forward".to_owned());
    }
    let built = root.join("target/release/mx");
    if !is_executable(&built) {
        return Err(format!(
            "release build did not produce an executable: {}",
            built.display()
        ));
    }
    let output = Command::new(&built)
        .env("MX_MULTICALL_EXPLICIT", "1")
        .env("MX_LAUNCHER_DEFAULT_ROOT", &root)
        .env("MX_RUST_SOURCE_ROOT", &root)
        .args(["launcher-install", "--upgrade", "--root"])
        .arg(&root)
        .arg("--home")
        .arg(&home)
        .arg("--bin-dir")
        .arg(bin_dir)
        .arg("--config-dir")
        .arg(&config)
        .arg("--data-dir")
        .arg(home.join("data"))
        .output()
        .map_err(|error_value| format!("could not start launcher upgrade: {error_value}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            "launcher binary upgrade failed".to_owned()
        } else {
            format!("launcher binary upgrade failed: {detail}")
        });
    }
    fs::remove_file(&pending)
        .map_err(|error_value| format!("cannot clear pending launcher update: {error_value}"))?;
    Ok(Some("launcher-binary: updated".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn path_files_are_literal_and_exact() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("root");
        fs::write(&path, b"/tmp/a b;$()\n").unwrap();
        assert_eq!(
            read_path_file(&path).unwrap(),
            PathBuf::from("/tmp/a b;$()")
        );
        fs::write(&path, b"/tmp/a\nextra\n").unwrap();
        assert!(read_path_file(&path).is_err());
        fs::write(&path, b"relative\n").unwrap();
        assert!(read_path_file(&path).is_err());
    }

    #[test]
    fn installer_argument_grammar_is_closed() {
        assert!(parse_installer(&[OsString::from("--unknown")]).is_err());
        assert!(parse_installer(&[OsString::from("--root")]).is_err());
        assert!(
            parse_installer(&[OsString::from("--help")])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn non_utf8_and_multiline_paths_refuse_before_publication() {
        let non_utf8 = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]));
        assert!(require_recordable_path(&non_utf8, "fixture").is_err());
        assert!(require_recordable_path(Path::new("/tmp/one\ntwo"), "fixture").is_err());
    }
}

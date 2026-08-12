use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use multplx_core::checks;
use multplx_core::filesystem::atomic_replace;
use multplx_domain::review_delivery::{
    OperationalTaskId, PrIdentity, agent_ambience, read_private,
};

const ENTRIES: &[&str] = &[
    "mx-check-register.sh",
    "mx-deep-review.sh",
    "mx-deliver.sh",
    "mx-merge-local.sh",
    "mx-pr-check-migrate.sh",
    "mx-pr-check.sh",
    "mx-pr-merge.sh",
    "mx-pr-poll.sh",
    "mx-promote.sh",
    "mx-review-diff.sh",
    "mx-validation-waive.sh",
];

pub fn run(entry: &str, args: &[OsString]) -> i32 {
    if !ENTRIES.contains(&entry) {
        eprintln!("error: unknown review or delivery entry point: {entry}");
        return 2;
    }
    match entry {
        "mx-check-register.sh" => check_register(args),
        "mx-pr-poll.sh" => pr_poll(args),
        "mx-promote.sh" => promote(args),
        _ => run_compat(entry, args),
    }
}

fn source_root() -> PathBuf {
    std::env::var_os("MX_RUST_SOURCE_ROOT")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn state_root() -> PathBuf {
    std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("MX_HOME")
                .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("state")
        })
}

fn run_compat(entry: &str, args: &[OsString]) -> i32 {
    let root = source_root();
    let path = root.join("bin").join(entry);
    if !path.is_file() {
        eprintln!(
            "error: review compatibility body is unavailable at {}",
            path.display()
        );
        return 1;
    }
    let error = Command::new("bash")
        .arg(path)
        .args(args)
        .env("MX_REVIEW_DELIVERY_IMPLEMENTATION", "legacy")
        .env("MX_RUST_SOURCE_ROOT", &root)
        .exec();
    eprintln!("error: could not start {entry}: {error}");
    1
}

fn check_register(args: &[OsString]) -> i32 {
    let Some(raw) = args.first().and_then(|value| value.to_str()) else {
        eprintln!("error: invalid custom check registration");
        return 2;
    };
    if args.len() != 1 {
        eprintln!("error: invalid custom check registration");
        return 2;
    }
    let Ok(task) = OperationalTaskId::parse(raw) else {
        eprintln!("error: invalid custom check registration");
        return 2;
    };
    let state = state_root();
    let Ok(state_meta) = fs::symlink_metadata(&state) else {
        eprintln!("error: state directory is unavailable");
        return 1;
    };
    if !state_meta.is_dir() || state_meta.file_type().is_symlink() {
        eprintln!("error: state directory is unavailable");
        return 1;
    }
    let check = state.join(format!("{task}.check.sh"));
    let Ok(check_meta) = fs::symlink_metadata(&check) else {
        eprintln!("error: custom check is unavailable");
        return 1;
    };
    if !check_meta.is_file()
        || check_meta.file_type().is_symlink()
        || check_meta.permissions().mode() & 0o7777 != 0o700
        || check_meta.nlink() != 1
        || check_meta.dev() != state_meta.dev()
    {
        eprintln!("error: custom check is unavailable");
        return 1;
    }
    let Ok(check_file) = read_private(&check, 0o700, state_meta.dev()) else {
        eprintln!("error: custom check hash is unavailable");
        return 1;
    };
    let digest = check_file.digest;
    let trust = state.join(format!("{task}.check-trust"));
    if fs::symlink_metadata(&trust).is_ok_and(|metadata| {
        !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.nlink() != 1
            || metadata.dev() != state_meta.dev()
    }) {
        eprintln!("error: custom check trust path is unavailable");
        return 1;
    }
    let trust_bytes = checks::render_trust(&digest);
    let published = atomic_replace(&trust, trust_bytes.as_bytes(), 0o600).is_ok()
        && read_private(&trust, 0o600, state_meta.dev())
            .is_ok_and(|file| file.bytes == trust_bytes.as_bytes());
    if !published {
        let _ = fs::remove_file(&trust);
        return 1;
    }
    println!("registered: state/{task}.check.sh");
    0
}

fn pr_poll(args: &[OsString]) -> i32 {
    let Some(values) = args
        .iter()
        .map(|value| value.to_str())
        .collect::<Option<Vec<_>>>()
    else {
        return 0;
    };
    let identity = if values.len() == 6 && values[0] == "--validated" {
        let Ok(identity) = PrIdentity::parse(values[2]) else {
            return 0;
        };
        if values[1] != identity.provider
            || values[3] != identity.host
            || values[4] != identity.project_path()
            || values[5] != identity.number
        {
            return 0;
        }
        identity
    } else if values.is_empty() {
        let Some(path) = std::env::var_os("MX_PR_POLL_CHECK_PATH").map(PathBuf::from) else {
            return 0;
        };
        let Some(raw) = path
            .to_str()
            .and_then(|value| value.strip_suffix(".check.sh"))
        else {
            return 0;
        };
        let sidecar = PathBuf::from(format!("{raw}.pr-poll"));
        let Some(parent) = sidecar.parent() else {
            return 0;
        };
        let Ok(parent_meta) = fs::symlink_metadata(parent) else {
            return 0;
        };
        if !parent_meta.is_dir() || parent_meta.file_type().is_symlink() {
            return 0;
        }
        let Ok(file) = read_private(&sidecar, 0o600, parent_meta.dev()) else {
            return 0;
        };
        let Ok(identity) = PrIdentity::parse_sidecar(&file.bytes) else {
            return 0;
        };
        identity
    } else {
        return 0;
    };
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &identity.url,
            "--json",
            "state",
            "-q",
            ".state",
        ])
        .output();
    if output.is_ok_and(|output| output.status.success() && output.stdout == b"MERGED\n") {
        println!("merged");
    }
    0
}

fn promote(args: &[OsString]) -> i32 {
    let Some(raw) = args.first().and_then(|value| value.to_str()) else {
        eprintln!("usage: mx-promote.sh <task-id>");
        return 1;
    };
    if args.len() != 1 || OperationalTaskId::parse(raw).is_err() {
        eprintln!("usage: mx-promote.sh <task-id>");
        return 1;
    }
    let state = state_root();
    let Ok(state_meta) = fs::symlink_metadata(&state) else {
        eprintln!(
            "error: no meta for task {raw} at {}/{}.meta",
            state.display(),
            raw
        );
        return 1;
    };
    if !state_meta.is_dir() || state_meta.file_type().is_symlink() {
        eprintln!(
            "error: no meta for task {raw} at {}/{}.meta",
            state.display(),
            raw
        );
        return 1;
    }
    let meta = state.join(format!("{raw}.meta"));
    let Ok(file) = read_private(&meta, 0o600, state_meta.dev()) else {
        eprintln!("error: no meta for task {raw} at {}", meta.display());
        return 1;
    };
    let Ok(text) = std::str::from_utf8(&file.bytes) else {
        eprintln!("error: no meta for task {raw} at {}", meta.display());
        return 1;
    };
    if !text.lines().any(|line| line == "kind=scout") {
        eprintln!("error: task {raw} is not a scout task (kind=scout not in meta)");
        return 1;
    }
    let mut output = text
        .lines()
        .filter(|line| !line.starts_with("kind="))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    output.push("kind=delivery".to_owned());
    if atomic_replace(&meta, format!("{}\n", output.join("\n")).as_bytes(), 0o600).is_err() {
        return 1;
    }
    let home = std::env::var_os("MX_HOME")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .unwrap_or_else(|| OsString::from("."));
    let quoted = shell_quote(&home.to_string_lossy());
    println!("promoted {raw} to delivery (teardown protection restored)");
    println!(
        "next: MX_HOME={quoted} bin/mx-send.sh mx-{raw} '<delivery instructions: review scratch state with git status and git log; reset to a clean default-branch base; carry over only intended fix changes; create branch mx/{raw}; implement; report done>'"
    );
    0
}

fn shell_quote(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[allow(dead_code)]
fn _credential_boundary_is_visible_to_rust() -> bool {
    agent_ambience()
}

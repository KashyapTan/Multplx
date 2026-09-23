//! Genuine broker-primary scope predicate from `bin/mx-primary-scope-lib.sh`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::identifiers::PathComponent;

/// Return whether a root carries a valid, non-symlink daemon-home marker.
#[must_use]
pub fn is_daemon_home(root: impl AsRef<Path>) -> bool {
    let marker = root.as_ref().join(".mx-daemon-home");
    let Ok(metadata) = fs::symlink_metadata(&marker) else {
        return false;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(bytes) = fs::read(&marker) else {
        return false;
    };
    if bytes.len() > 4096 {
        return false;
    }
    let id = String::from_utf8_lossy(&bytes)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    PathComponent::parse(id.clone()).is_ok()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn git_value(root: &Path, argument: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", argument])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 4096 {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim_end().to_owned())
}

fn exact_path_file(path: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16 * 1024 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let value = bytes.strip_suffix(b"\n")?;
    if value.is_empty() || value.contains(&b'\n') || value.contains(&b'\r') {
        return None;
    }
    let path = PathBuf::from(std::str::from_utf8(value).ok()?);
    path.is_absolute().then_some(path)
}

fn installed_primary_matches(root: &Path, state: &Path, executable: &Path) -> bool {
    if executable.file_name().and_then(|value| value.to_str()) != Some("multplx") {
        return false;
    }
    let Some(bin_dir) = executable.parent() else {
        return false;
    };
    let Some(config) = exact_path_file(&bin_dir.join(".multplx-config")) else {
        return false;
    };
    let Ok(config) = fs::canonicalize(config) else {
        return false;
    };
    let Some(configured_root) = exact_path_file(&config.join("root")) else {
        return false;
    };
    let Some(configured_home) = exact_path_file(&config.join("home")) else {
        return false;
    };
    let marker = root.join(".multplx-release");
    let Ok(marker_metadata) = fs::symlink_metadata(&marker) else {
        return false;
    };
    if !marker_metadata.is_file()
        || marker_metadata.file_type().is_symlink()
        || marker_metadata.len() > 4096
        || fs::read_to_string(marker)
            .ok()
            .is_none_or(|value| value.trim() != env!("CARGO_PKG_VERSION"))
    {
        return false;
    }
    fs::canonicalize(root).ok() == fs::canonicalize(configured_root).ok()
        && fs::canonicalize(state).ok() == fs::canonicalize(configured_home.join("state")).ok()
}

fn matches_with_context(
    root: &Path,
    state: &Path,
    executable: Option<&Path>,
    spawned_task_identity: bool,
    gate_agent: bool,
) -> bool {
    if gate_agent {
        return false;
    }
    if !is_daemon_home(root) {
        if spawned_task_identity {
            return false;
        }
        if !executable.is_some_and(|path| installed_primary_matches(root, state, path)) {
            let Some(git_dir) = git_value(root, "--git-dir") else {
                return false;
            };
            let Some(common_dir) = git_value(root, "--git-common-dir") else {
                return false;
            };
            if git_dir != common_dir {
                return false;
            }
        }
    }
    root.join("AGENTS.md").is_file() && root.join("bin").is_dir() && state.is_dir()
}

/// Return whether the root is a valid primary or linked daemon home and has
/// the required broker-contract, command, and state surfaces.
#[must_use]
pub fn matches(root: impl AsRef<Path>, state: impl AsRef<Path>) -> bool {
    let root = root.as_ref();
    let state = state.as_ref();
    let executable = std::env::current_exe().ok();
    let spawned_task_identity = std::env::var_os("MX_TASK_ID").is_some()
        || std::env::var_os("MX_CURRENT_ADMISSION_ID").is_some();
    let gate_agent = std::env::var_os("DEEP_REVIEW_GATE").is_some()
        && std::env::var("MX_GATE_REFUSE_BYPASS").as_deref() != Ok("1");
    matches_with_context(
        root,
        state,
        executable.as_deref(),
        spawned_task_identity,
        gate_agent,
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::{installed_primary_matches, is_daemon_home, matches, matches_with_context};

    #[test]
    fn daemon_marker_and_required_surfaces_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let state = root.join("state");
        fs::create_dir_all(root.join("bin")).expect("bin");
        fs::create_dir(&state).expect("state");
        fs::write(root.join("AGENTS.md"), b"# contract\n").expect("contract");
        assert!(!is_daemon_home(&root));
        fs::write(root.join(".mx-daemon-home"), b" daemon-1 \n").expect("marker");
        assert!(is_daemon_home(&root));
        assert!(matches(&root, &state));

        fs::write(root.join(".mx-daemon-home"), b"bad/id\n").expect("invalid marker");
        assert!(!is_daemon_home(&root));
        fs::write(root.join(".mx-daemon-home"), vec![b'x'; 4097]).expect("large marker");
        assert!(!is_daemon_home(&root));
        fs::remove_file(root.join(".mx-daemon-home")).expect("remove marker");
        let outside = temp.path().join("outside");
        fs::write(&outside, b"daemon-1\n").expect("outside");
        symlink(&outside, root.join(".mx-daemon-home")).expect("marker link");
        assert!(!is_daemon_home(&root));
        assert!(!matches(&root, &state));
    }

    #[test]
    fn installed_primary_requires_binary_owned_config_and_exact_primary_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("runtime");
        let home = temp.path().join("home");
        let state = home.join("state");
        let config = temp.path().join("config");
        let bin = temp.path().join("bin");
        fs::create_dir_all(root.join("bin")).expect("runtime bin");
        fs::create_dir_all(&state).expect("state");
        fs::create_dir_all(&config).expect("config");
        fs::create_dir_all(&bin).expect("bin");
        fs::write(root.join("AGENTS.md"), b"# contract\n").expect("contract");
        fs::write(
            root.join(".multplx-release"),
            concat!(env!("CARGO_PKG_VERSION"), "\n"),
        )
        .expect("release marker");
        fs::write(config.join("root"), format!("{}\n", root.display())).expect("root record");
        fs::write(config.join("home"), format!("{}\n", home.display())).expect("home record");
        fs::write(
            bin.join(".multplx-config"),
            format!("{}\n", config.display()),
        )
        .expect("config pointer");
        let executable = bin.join("multplx");
        fs::write(&executable, b"binary").expect("binary");

        assert!(installed_primary_matches(&root, &state, &executable));
        assert!(matches_with_context(
            &root,
            &state,
            Some(&executable),
            false,
            false
        ));
        assert!(!matches_with_context(
            &root,
            &state,
            Some(&executable),
            true,
            false
        ));
        assert!(!matches_with_context(
            &root,
            &state,
            Some(&executable),
            false,
            true
        ));
        let worker_state = temp.path().join("worker/state");
        fs::create_dir_all(&worker_state).expect("worker state");
        assert!(!installed_primary_matches(
            &root,
            &worker_state,
            &executable
        ));
        fs::write(root.join(".multplx-release"), b"wrong\n").expect("wrong marker");
        assert!(!installed_primary_matches(&root, &state, &executable));
        fs::write(
            root.join(".multplx-release"),
            concat!(env!("CARGO_PKG_VERSION"), "\n"),
        )
        .expect("restore marker");
        let outside = temp.path().join("outside");
        fs::write(&outside, format!("{}\n", home.display())).expect("outside record");
        fs::remove_file(config.join("home")).expect("remove home record");
        symlink(&outside, config.join("home")).expect("linked home record");
        assert!(!installed_primary_matches(&root, &state, &executable));
    }

    #[test]
    fn marked_private_coordinator_remains_primary_with_spawn_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("private-home");
        let state = root.join("state");
        fs::create_dir_all(root.join("bin")).expect("bin");
        fs::create_dir(&state).expect("state");
        fs::write(root.join("AGENTS.md"), b"# contract\n").expect("contract");
        fs::write(root.join(".mx-daemon-home"), b"coordinator\n").expect("marker");
        assert!(matches_with_context(&root, &state, None, true, false));
        assert!(!matches_with_context(&root, &state, None, true, true));
    }
}

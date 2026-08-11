//! Genuine broker-primary scope predicate from `bin/mx-primary-scope-lib.sh`.

use std::fs;
use std::path::Path;
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

/// Return whether the root is a valid primary or linked daemon home and has
/// the required broker-contract, command, and state surfaces.
#[must_use]
pub fn matches(root: impl AsRef<Path>, state: impl AsRef<Path>) -> bool {
    let root = root.as_ref();
    let state = state.as_ref();
    if !is_daemon_home(root) {
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
    root.join("AGENTS.md").is_file() && root.join("bin").is_dir() && state.is_dir()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::{is_daemon_home, matches};

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
}

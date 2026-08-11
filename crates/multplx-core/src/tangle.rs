//! Primary-checkout worktree tangle detection from `bin/mx-tangle-lib.sh`.

use std::path::Path;
use std::process::Command;

use crate::error::{CoreError, Result};

const MAX_GIT_OUTPUT: usize = 16 * 1024;

fn git(root: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| CoreError::Command {
            command: "git".to_owned(),
            reason: error.to_string(),
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    if output.stdout.len() > MAX_GIT_OUTPUT {
        return Err(CoreError::RecordTooLarge {
            kind: "git output",
            limit: MAX_GIT_OUTPUT,
        });
    }
    let value = String::from_utf8(output.stdout).map_err(|_| CoreError::Command {
        command: "git".to_owned(),
        reason: "non-UTF-8 output".to_owned(),
    })?;
    Ok(Some(value.trim_end().to_owned()))
}

/// Resolve origin/HEAD, then local main or master.
pub fn default_branch(root: impl AsRef<Path>) -> Result<Option<String>> {
    let root = root.as_ref();
    if let Some(reference) = git(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )? && !reference.is_empty()
    {
        return Ok(Some(
            reference
                .strip_prefix("origin/")
                .unwrap_or(&reference)
                .to_owned(),
        ));
    }
    for branch in ["main", "master"] {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ])
            .status()
            .map_err(|error| CoreError::Command {
                command: "git".to_owned(),
                reason: error.to_string(),
            })?;
        if status.success() {
            return Ok(Some(branch.to_owned()));
        }
    }
    Ok(None)
}

/// Return the named non-default branch that tangles a primary checkout.
pub fn primary_tangle_branch(root: impl AsRef<Path>) -> Result<Option<String>> {
    let root = root.as_ref();
    if git(root, &["rev-parse", "--is-inside-work-tree"])?.as_deref() != Some("true") {
        return Ok(None);
    }
    let Some(current) = git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])? else {
        return Ok(None);
    };
    if current.is_empty() {
        return Ok(None);
    }
    let Some(default) = default_branch(root)? else {
        return Ok(None);
    };
    Ok((current != default).then_some(current))
}

/// Render the exact bootstrap tangle line.
#[must_use]
pub fn render_bootstrap_tangle(
    root: &Path,
    branch: &str,
    default: &str,
    read_only: bool,
) -> String {
    if read_only {
        format!(
            "TANGLE: primary checkout on feature branch '{branch}' (expected '{default}'); the work is safe on that ref - read-only session must leave restore work to the session holding the system lock\n"
        )
    } else {
        format!(
            "TANGLE: primary checkout on feature branch '{branch}' (expected '{default}'); the work is safe on that ref - restore the primary with: git -C {} checkout {default}, then re-validate the branch in a proper worktree\n",
            root.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use super::{default_branch, primary_tangle_branch, render_bootstrap_tangle};

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn committed_repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("tempdir");
        git(temp.path(), &["init", "-b", "main"]);
        git(temp.path(), &["config", "user.name", "Fixture"]);
        git(
            temp.path(),
            &["config", "user.email", "fixture@example.test"],
        );
        fs::write(temp.path().join("fixture"), b"one\n").expect("fixture");
        git(temp.path(), &["add", "fixture"]);
        git(temp.path(), &["commit", "-m", "fixture"]);
        temp
    }

    #[test]
    fn default_branch_prefers_origin_head_and_tangle_handles_detached_head() {
        let temp = committed_repo();
        git(
            temp.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        git(
            temp.path(),
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        assert_eq!(
            default_branch(temp.path()).expect("default").as_deref(),
            Some("main")
        );
        git(temp.path(), &["checkout", "--detach"]);
        assert_eq!(primary_tangle_branch(temp.path()).expect("detached"), None);
    }

    #[test]
    fn non_repository_and_repository_without_default_are_not_tangles() {
        let non_repo = tempfile::tempdir().expect("non-repo");
        assert_eq!(
            primary_tangle_branch(non_repo.path()).expect("non-repo"),
            None
        );
        assert_eq!(default_branch(non_repo.path()).expect("no default"), None);

        let topic = tempfile::tempdir().expect("topic");
        git(topic.path(), &["init", "-b", "topic"]);
        git(topic.path(), &["config", "user.name", "Fixture"]);
        git(
            topic.path(),
            &["config", "user.email", "fixture@example.test"],
        );
        fs::write(topic.path().join("fixture"), b"one\n").expect("fixture");
        git(topic.path(), &["add", "fixture"]);
        git(topic.path(), &["commit", "-m", "fixture"]);
        assert_eq!(
            default_branch(topic.path()).expect("no main or master"),
            None
        );
        assert_eq!(
            primary_tangle_branch(topic.path()).expect("no default"),
            None
        );
    }

    #[test]
    fn tangle_rendering_covers_read_only_and_recovery_guidance() {
        let root = Path::new("/tmp/example root");
        let read_only = render_bootstrap_tangle(root, "topic", "main", true);
        assert!(read_only.contains("read-only session"));
        let writable = render_bootstrap_tangle(root, "topic", "main", false);
        assert!(writable.contains("git -C /tmp/example root checkout main"));
    }
}

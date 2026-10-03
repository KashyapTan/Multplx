//! Canonical backend home identity from `bin/mx-backend-hometag-lib.sh`.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::Result;

/// Historical marker name retained as a read-only compatibility constant.
pub const DAEMON_MARKER: &str = crate::agent_home::LEGACY_MARKER;

/// Derive the canonical primary/agent namespace for newly created containers.
pub fn home_tag(root: impl AsRef<Path>, home: impl AsRef<Path>) -> Result<String> {
    tag(root.as_ref(), home.as_ref(), false)
}

/// Derive this exact home's old namespace for existing-container adoption only.
pub fn legacy_home_tag(root: impl AsRef<Path>, home: impl AsRef<Path>) -> Result<String> {
    tag(root.as_ref(), home.as_ref(), true)
}

fn tag(root: &Path, home: &Path, legacy: bool) -> Result<String> {
    let prefix = match crate::agent_home::identity(home)? {
        Some(id) => format!("{}-{id}", if legacy { "daemon" } else { "agent" }),
        None => if legacy { "broker" } else { "primary" }.to_owned(),
    };
    let resolved = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let root_text = resolved.to_string_lossy();
    let digest = Sha256::digest(root_text.as_bytes());
    Ok(format!(
        "{prefix}-{:08x}",
        u32::from_be_bytes(digest[..4].try_into().expect("four bytes"))
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::home_tag;

    #[test]
    fn tags_are_stable_and_distinguish_daemon_homes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let home = temp.path().join("home");
        fs::create_dir(&root).expect("root");
        fs::create_dir(&home).expect("home");
        let broker = home_tag(&root, &home).expect("broker tag");
        assert!(broker.starts_with("primary-"));
        assert!(
            super::legacy_home_tag(&root, &home)
                .unwrap()
                .starts_with("broker-")
        );
        fs::write(home.join(".mx-daemon-home"), b" build-1 \n").expect("marker");
        let daemon = home_tag(&root, &home).expect("daemon tag");
        assert!(daemon.starts_with("agent-build-1-"));
        assert!(
            super::legacy_home_tag(&root, &home)
                .unwrap()
                .starts_with("daemon-build-1-")
        );
        assert_eq!(&broker[broker.len() - 8..], &daemon[daemon.len() - 8..]);
        fs::write(home.join(".mx-daemon-home"), b" \n\t").expect("empty marker");
        assert!(home_tag(&root, &home).is_err());
        fs::write(home.join(".mx-daemon-home"), vec![b'x'; 4097]).expect("large marker");
        assert!(home_tag(&root, &home).is_err());
        fs::remove_file(home.join(".mx-daemon-home")).expect("remove marker");
        assert!(
            home_tag(temp.path().join("absent-root"), &home)
                .expect("unresolved root")
                .starts_with("primary-")
        );
    }
}

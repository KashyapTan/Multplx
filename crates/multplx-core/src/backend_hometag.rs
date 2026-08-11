//! Canonical backend home identity from `bin/mx-backend-hometag-lib.sh`.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{CoreError, Result};

/// The daemon-home marker used by session-provider backends.
pub const DAEMON_MARKER: &str = ".mx-daemon-home";

/// Derive the stable `<broker|daemon-id>-<root-hash>` namespace label.
pub fn home_tag(root: impl AsRef<Path>, home: impl AsRef<Path>) -> Result<String> {
    let root = root.as_ref();
    let home = home.as_ref();
    let marker = home.join(DAEMON_MARKER);
    let prefix = match fs::read(&marker) {
        Ok(bytes) => {
            if bytes.len() > 4096 {
                return Err(CoreError::RecordTooLarge {
                    kind: "daemon-home marker",
                    limit: 4096,
                });
            }
            let id = String::from_utf8_lossy(&bytes)
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            if id.is_empty() {
                "broker".to_owned()
            } else {
                format!("daemon-{id}")
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "broker".to_owned(),
        Err(error) => return Err(CoreError::io("read daemon-home marker", marker, error)),
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
        assert!(broker.starts_with("broker-"));
        fs::write(home.join(".mx-daemon-home"), b" build-1 \n").expect("marker");
        let daemon = home_tag(&root, &home).expect("daemon tag");
        assert!(daemon.starts_with("daemon-build-1-"));
        assert_eq!(&broker[broker.len() - 8..], &daemon[daemon.len() - 8..]);
        fs::write(home.join(".mx-daemon-home"), b" \n\t").expect("empty marker");
        assert!(
            home_tag(&root, &home)
                .expect("empty tag")
                .starts_with("broker-")
        );
        fs::write(home.join(".mx-daemon-home"), vec![b'x'; 4097]).expect("large marker");
        assert!(home_tag(&root, &home).is_err());
        fs::remove_file(home.join(".mx-daemon-home")).expect("remove marker");
        assert!(
            home_tag(temp.path().join("absent-root"), &home)
                .expect("unresolved root")
                .starts_with("broker-")
        );
    }
}

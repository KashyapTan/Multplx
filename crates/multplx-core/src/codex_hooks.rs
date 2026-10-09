//! One native-consent identity for the owned Codex worker hook bundle.
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const SCRIPTS: [&str; 2] = [
    "bin/mx-native-observe.sh",
    "bin/mx-subagent-pretool-check.sh",
];

/// Resolve the executable that the compatibility scripts actually execute.
pub fn runtime_binary() -> Result<PathBuf, String> {
    let path = std::env::var_os("MX_RUST_BIN")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("MX_LAUNCH_BIN_PATH").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)
        .map_err(|error| format!("cannot resolve runtime binary: {error}"))?;
    if !path.is_absolute() {
        return Err("runtime binary is not absolute".into());
    }
    fs::canonicalize(&path).map_err(|error| format!("cannot resolve {}: {error}", path.display()))
}

fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn path_word(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(shell_word)
        .ok_or_else(|| format!("hook path is not valid UTF-8: {}", path.display()))
}

// Public `multplx` and packaged `mx` may be separate copies of the same release.
// Prefer the stable package executable only when its actual bytes match the
// selected runtime; genuine explicit overrides retain their own identity.
fn same_contents(left: &Path, right: &Path) -> Result<bool, String> {
    let mut left = fs::File::open(left).map_err(|error| error.to_string())?;
    let mut right = fs::File::open(right).map_err(|error| error.to_string())?;
    if left.metadata().map_err(|error| error.to_string())?.len()
        != right.metadata().map_err(|error| error.to_string())?.len()
    {
        return Ok(false);
    }
    let mut left_buffer = [0_u8; 65536];
    let mut right_buffer = [0_u8; 65536];
    loop {
        let left_count = left
            .read(&mut left_buffer)
            .map_err(|error| error.to_string())?;
        let right_count = right
            .read(&mut right_buffer)
            .map_err(|error| error.to_string())?;
        if left_count != right_count || left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
    }
}

/// Hash the actual script and executable bytes, with framed names and lengths.
/// Dynamic task routing never enters the definition or its native trust hash.
pub fn arguments(root: &Path, binary: &Path) -> Result<Vec<OsString>, String> {
    let root =
        fs::canonicalize(root).map_err(|error| format!("cannot resolve hook runtime: {error}"))?;
    let binary = fs::canonicalize(binary)
        .map_err(|error| format!("cannot resolve hook executable: {error}"))?;
    let packaged = root.join("target/release/mx");
    let binary = if packaged.is_file() && same_contents(&binary, &packaged)? {
        fs::canonicalize(packaged).map_err(|error| error.to_string())?
    } else {
        binary
    };
    let mut hash = Sha256::new();
    for (name, path) in SCRIPTS
        .iter()
        .map(|name| (*name, root.join(name)))
        .chain(std::iter::once(("runtime-executable", binary.clone())))
    {
        let mut file = fs::File::open(&path).map_err(|error| {
            format!(
                "cannot fingerprint owned Codex hook {}: {error}",
                path.display()
            )
        })?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(
            file.metadata()
                .map_err(|error| error.to_string())?
                .len()
                .to_le_bytes(),
        );
        let mut buffer = [0_u8; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
    }
    let fingerprint = format!("{:x}", hash.finalize());
    let prefix = format!(
        "[ -n \"${{MX_TASK_ID:-}}\" ] || exit 0; export MX_CODEX_HOOK_BUNDLE={fingerprint} MX_RUST_BIN={}; ",
        path_word(&binary)?
    );
    let observer = path_word(&root.join(SCRIPTS[0]))?;
    let merge = path_word(&root.join(SCRIPTS[1]))?;
    let mut args = Vec::new();
    for (event, handler, matcher, timeout) in [
        (
            "SessionStart",
            format!("{observer} --provider codex --event reconcile || true"),
            None,
            5,
        ),
        (
            "SubagentStart",
            format!("{observer} --provider codex --event start || true"),
            None,
            5,
        ),
        (
            "SubagentStop",
            format!("{observer} --provider codex --event result || true"),
            None,
            5,
        ),
        ("PreToolUse", merge, Some("Bash"), 10),
    ] {
        let command = format!("{prefix}{handler}");
        let matcher = matcher.map_or(String::new(), |value| format!("matcher=\"{value}\","));
        let value = format!(
            "hooks.{event}=[{{{matcher}hooks=[{{type=\"command\",command={},timeout={timeout}}}]}}]",
            serde_json::to_string(&command).map_err(|error| error.to_string())?
        );
        args.extend([OsString::from("-c"), OsString::from(value)]);
    }
    Ok(args)
}

/// Session hook arrays occupy a shared native event/index namespace.
/// Refuse collisions instead of silently replacing a caller's definitions.
pub fn validate_cli_overrides(args: &[OsString]) -> Result<(), String> {
    for (index, argument) in args.iter().enumerate() {
        let argument = argument
            .to_str()
            .ok_or("Codex argument is not valid UTF-8")?;
        let value = if matches!(argument, "-c" | "--config") {
            args.get(index + 1).and_then(|value| value.to_str())
        } else {
            argument.strip_prefix("--config=").or_else(|| {
                argument
                    .strip_prefix("-c")
                    .filter(|value| !value.is_empty())
            })
        };
        if let Some(value) = value {
            let key = value
                .split('=')
                .next()
                .unwrap_or("")
                .trim()
                .replace(['"', '\''], "");
            if key == "hooks"
                || [
                    "SessionStart",
                    "SubagentStart",
                    "SubagentStop",
                    "PreToolUse",
                ]
                .iter()
                .any(|event| {
                    key == format!("hooks.{event}") || key.starts_with(&format!("hooks.{event}."))
                })
            {
                return Err(format!(
                    "Codex CLI override '{key}' conflicts with the shared Multplx worker hook bundle; remove that CLI override or launch Codex directly to review your custom bundle. Project and user hook files remain under native review."
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_identity_canonicalizes_root_and_invalidates_owned_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("bin")).unwrap();
        for path in SCRIPTS {
            fs::write(root.join(path), "#!/bin/sh\nexit 0\n").unwrap();
        }
        let binary = root.join("mx");
        fs::write(&binary, "binary-v1").unwrap();
        let first = arguments(root, &binary).unwrap();
        let alias = root.join("alias");
        std::os::unix::fs::symlink(root, &alias).unwrap();
        assert_eq!(first, arguments(&alias, &binary).unwrap());
        for script in SCRIPTS {
            fs::write(root.join(script), "changed").unwrap();
            assert_ne!(first, arguments(root, &binary).unwrap());
            fs::write(root.join(script), "#!/bin/sh\nexit 0\n").unwrap();
        }
        fs::write(&binary, "binary-v2").unwrap();
        assert_ne!(first, arguments(root, &binary).unwrap());
        for definition in first.iter().skip(1).step_by(2) {
            assert!(definition.to_str().unwrap().contains("MX_TASK_ID"));
            assert!(
                definition
                    .to_str()
                    .unwrap()
                    .contains("MX_CODEX_HOOK_BUNDLE=")
            );
        }
    }
    #[test]
    fn public_launcher_and_packaged_worker_have_one_identity_without_inherited_environment() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("runtime");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("target/release")).unwrap();
        for script in SCRIPTS {
            fs::write(root.join(script), "#!/bin/sh\nexit 0\n").unwrap();
        }
        let public = temp.path().join("multplx");
        let packaged = root.join("target/release/mx");
        fs::write(&public, "same-release-bytes").unwrap();
        fs::write(&packaged, "same-release-bytes").unwrap();
        assert_eq!(
            arguments(&root, &public).unwrap(),
            arguments(&root, &packaged).unwrap()
        );
        fs::write(&public, "explicit-different-runtime").unwrap();
        assert_ne!(
            arguments(&root, &public).unwrap(),
            arguments(&root, &packaged).unwrap()
        );
    }
    #[test]
    fn shared_and_owned_project_handlers_execute_exactly_once_in_primary_and_worker() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("bin")).unwrap();
        fs::write(root.join("AGENTS.md"), "fixture").unwrap();
        for script in SCRIPTS {
            let path = root.join(script);
            fs::write(
                &path,
                "#!/bin/sh\nprintf 'called\\n' >> \"$MX_TEST_CALLS\"\n",
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let binary = root.join("mx");
        fs::write(&binary, "fixture executable").unwrap();
        let marker = root.join("calls");
        let project: serde_json::Value =
            serde_json::from_str(include_str!("../../../.codex/hooks.json")).unwrap();
        for definition in arguments(root, &binary).unwrap().iter().skip(1).step_by(2) {
            let definition = definition.to_str().unwrap();
            let event = definition
                .strip_prefix("hooks.")
                .unwrap()
                .split('=')
                .next()
                .unwrap();
            let encoded = definition
                .split_once("command=")
                .unwrap()
                .1
                .rsplit_once(",timeout=")
                .unwrap()
                .0;
            let command: String = serde_json::from_str(encoded).unwrap();
            let project_handlers = project["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|group| group["hooks"].as_array().unwrap())
                .filter_map(|hook| hook["command"].as_str())
                .filter(|command| {
                    command.contains("mx-native-observe.sh")
                        || command.contains("mx-subagent-pretool-check.sh")
                })
                .collect::<Vec<_>>();
            assert_eq!(project_handlers.len(), 1);
            for worker in [false, true] {
                for command in [&command[..], project_handlers[0]] {
                    let mut process = Command::new("/bin/sh");
                    process
                        .args(["-c", command])
                        .env("MX_RUST_SOURCE_ROOT", root)
                        .env("MX_TEST_CALLS", &marker)
                        .env("MX_CODEX_SHARED_WORKER_HOOKS", "1")
                        .env_remove("MX_TASK_ID")
                        .stdin(Stdio::piped());
                    if worker {
                        process.env("MX_TASK_ID", "owned-fixture-worker");
                    }
                    let mut child = process.spawn().unwrap();
                    child.stdin.take().unwrap().write_all(b"{}").unwrap();
                    assert!(child.wait().unwrap().success());
                }
                assert_eq!(
                    fs::read_to_string(&marker).unwrap(),
                    "called\n",
                    "{event} worker={worker}"
                );
                fs::remove_file(&marker).unwrap();
            }
            // Direct Codex launch retains its owned project handler.
            let mut child = Command::new("/bin/sh")
                .args(["-c", project_handlers[0]])
                .env("MX_TASK_ID", "direct-worker")
                .env_remove("MX_CODEX_SHARED_WORKER_HOOKS")
                .env("MX_RUST_SOURCE_ROOT", root)
                .env("MX_TEST_CALLS", &marker)
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(b"{}").unwrap();
            assert!(child.wait().unwrap().success());
            assert_eq!(fs::read_to_string(&marker).unwrap(), "called\n");
            fs::remove_file(&marker).unwrap();
        }
    }
    #[test]
    fn preserves_native_disable_controls_and_refuses_custom_array_collisions() {
        for value in [
            "hooks.enabled=false",
            "hooks.Stop=[]",
            "model=\"custom-model\"",
        ] {
            validate_cli_overrides(&["-c".into(), value.into()]).unwrap();
        }
        for args in [
            vec!["-c", "hooks.SessionStart=[]"],
            vec!["--config=hooks.PreToolUse=[]"],
            vec!["-chooks={}"],
            vec!["--config", "hooks.SubagentStart.0={}"],
        ] {
            assert!(
                validate_cli_overrides(&args.into_iter().map(OsString::from).collect::<Vec<_>>())
                    .is_err()
            );
        }
    }
}

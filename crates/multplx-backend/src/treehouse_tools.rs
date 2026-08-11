//! Pinned Treehouse CI installer with checksum, mode, version, and lease gates.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use sha2::{Digest, Sha256};

const VERSION: &str = "2.0.1";
const REPOSITORY: &str = "kunchenguid/treehouse";
const MAX_BYTES: u64 = 15_000_000;

#[derive(Debug, thiserror::Error)]
enum InstallError {
    #[error("{0}")]
    Message(String),
    #[error("{operation}: {source}")]
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
}

fn io(operation: &'static str) -> impl FnOnce(std::io::Error) -> InstallError {
    move |source| InstallError::Io { operation, source }
}

fn asset_for_platform(os: &str, arch: &str) -> Result<(&'static str, &'static str), InstallError> {
    match (os, arch) {
        ("linux", "x86_64") => Ok((
            "treehouse-v2.0.1-linux-amd64.tar.gz",
            "1d5a32751ab921670103fd201ddb2b91b47338cb13976f45642b827cf8976af2",
        )),
        ("linux", "aarch64") => Ok((
            "treehouse-v2.0.1-linux-arm64.tar.gz",
            "eaccc9c5b98125df8bd77425598eeecee66cb0371db4eb1cf75f0d813c18fab9",
        )),
        ("macos", "aarch64") => Ok((
            "treehouse-v2.0.1-darwin-arm64.tar.gz",
            "7ee5078f3d1f33c01196548797fce65408e459d53530b77d4ba56e074fa1c1a2",
        )),
        ("macos", "x86_64") => Ok((
            "treehouse-v2.0.1-darwin-amd64.tar.gz",
            "1cf44580a5837f995e1d3bb74f4fbd3112b642acd20406087d9735a8106112fd",
        )),
        _ => Err(InstallError::Message(format!(
            "unsupported platform {os}-{arch}; official Treehouse assets are linux/darwin amd64 and arm64"
        ))),
    }
}

fn hash(path: &Path) -> Result<String, InstallError> {
    let mut file = File::open(path).map_err(io("open Treehouse archive"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(io("hash Treehouse archive"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_checksum(path: &Path, expected: &str, archive: &str) -> Result<(), InstallError> {
    let actual = hash(path)?;
    if actual != expected {
        return Err(InstallError::Message(format!(
            "checksum mismatch for {archive} (expected {expected}, got {actual})"
        )));
    }
    Ok(())
}

fn verified_version(status: ExitStatus, stdout: &[u8]) -> Result<String, InstallError> {
    let version = String::from_utf8_lossy(stdout)
        .trim()
        .trim_start_matches('v')
        .to_owned();
    if !status.success() || version != VERSION {
        return Err(InstallError::Message(format!(
            "installed treehouse version is '{}', expected exact pin v{VERSION}",
            if version.is_empty() {
                "<empty>"
            } else {
                &version
            }
        )));
    }
    Ok(version)
}

fn verify_lease(status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> Result<(), InstallError> {
    let help_text = String::from_utf8_lossy(stdout).to_string() + &String::from_utf8_lossy(stderr);
    if !status.success() || !help_text.contains("--lease") {
        return Err(InstallError::Message(
            "installed treehouse does not support the required 'get --lease' capability".to_owned(),
        ));
    }
    Ok(())
}

fn install_archive(
    destination: &Path,
    temporary: &Path,
    downloaded: &Path,
    archive: &str,
    expected: &str,
) -> Result<String, InstallError> {
    if fs::metadata(downloaded)
        .map_err(io("inspect Treehouse archive"))?
        .len()
        > MAX_BYTES
    {
        return Err(InstallError::Message(
            "download exceeded size limit".to_owned(),
        ));
    }
    verify_checksum(downloaded, expected, archive)?;
    let listing = Command::new("tar")
        .args(["-tzf"])
        .arg(downloaded)
        .output()
        .map_err(io("inspect Treehouse archive"))?;
    if !listing.status.success() {
        return Err(InstallError::Message(format!(
            "could not inspect archive {archive}"
        )));
    }
    let entries = String::from_utf8(listing.stdout)
        .map_err(|_| InstallError::Message("archive listing is not UTF-8".to_owned()))?;
    let wanted = if entries.lines().any(|line| line == "treehouse") {
        "treehouse".to_owned()
    } else {
        format!("treehouse-v{VERSION}/treehouse")
    };
    if !entries.lines().any(|line| line == wanted) {
        return Err(InstallError::Message(format!(
            "archive {archive} did not contain a treehouse binary"
        )));
    }
    let status = Command::new("tar")
        .args(["-xzf"])
        .arg(downloaded)
        .args(["-C"])
        .arg(temporary)
        .arg(&wanted)
        .status()
        .map_err(io("extract Treehouse binary"))?;
    if !status.success() {
        return Err(InstallError::Message(format!(
            "could not extract {archive}"
        )));
    }
    fs::create_dir_all(destination).map_err(io("create Treehouse install destination"))?;
    let installed = destination.join("treehouse");
    fs::copy(temporary.join(wanted), &installed).map_err(io("install Treehouse binary"))?;
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o755))
        .map_err(io("set Treehouse executable mode"))?;
    let version_output = Command::new(&installed)
        .arg("--version")
        .output()
        .map_err(io("run installed Treehouse version"))?;
    let version = verified_version(version_output.status, &version_output.stdout)?;
    let help = Command::new(&installed)
        .args(["get", "--help"])
        .output()
        .map_err(io("inspect Treehouse lease support"))?;
    verify_lease(help.status, &help.stdout, &help.stderr)?;
    eprintln!(
        "mx-install-treehouse.sh: installed treehouse v{version} with get --lease to {}",
        installed.display()
    );
    Ok(format!("v{version}"))
}

fn install(destination: &Path) -> Result<String, InstallError> {
    let (archive, expected) = asset_for_platform(std::env::consts::OS, std::env::consts::ARCH)?;
    let url = format!("https://github.com/{REPOSITORY}/releases/download/v{VERSION}/{archive}");
    let temporary = tempfile::Builder::new()
        .prefix("mx-treehouse.")
        .tempdir_in(
            std::env::var_os("RUNNER_TEMP")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir),
        )
        .map_err(io("create Treehouse installer temporary directory"))?;
    let downloaded = temporary.path().join(archive);
    eprintln!("mx-install-treehouse.sh: downloading {archive} from {url}");
    let status = Command::new("curl")
        .args([
            "-fsSL",
            "--max-filesize",
            &MAX_BYTES.to_string(),
            &url,
            "-o",
        ])
        .arg(&downloaded)
        .status()
        .map_err(io("run curl"))?;
    if !status.success() {
        return Err(InstallError::Message(format!(
            "download failed for {url} (bounded at {MAX_BYTES} bytes)"
        )));
    }
    install_archive(
        destination,
        temporary.path(),
        &downloaded,
        archive,
        expected,
    )
}

pub fn run_installer(args: &[OsString]) -> i32 {
    if args.len() != 1 {
        eprintln!(
            "mx-install-treehouse.sh: usage: mx-install-treehouse.sh <destination-directory>"
        );
        return 1;
    }
    match install(Path::new(&args[0])) {
        Ok(version) => {
            println!("{version}");
            0
        }
        Err(error) => {
            eprintln!("mx-install-treehouse.sh: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;

    use super::{
        asset_for_platform, hash, install_archive, verified_version, verify_checksum, verify_lease,
    };

    #[test]
    fn pinned_platform_matrix_is_complete() {
        assert_eq!(
            asset_for_platform("linux", "x86_64").expect("linux").0,
            "treehouse-v2.0.1-linux-amd64.tar.gz"
        );
        assert_eq!(
            asset_for_platform("macos", "aarch64").expect("mac").0,
            "treehouse-v2.0.1-darwin-arm64.tar.gz"
        );
        assert!(asset_for_platform("windows", "x86_64").is_err());
    }

    #[test]
    fn checksum_gate_accepts_only_the_expected_archive_bytes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let archive = temp.path().join("treehouse.tar.gz");
        std::fs::write(&archive, b"abc").expect("fixture");
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        verify_checksum(&archive, expected, "fixture").expect("checksum");
        assert!(verify_checksum(&archive, "wrong", "fixture").is_err());
    }

    #[test]
    fn version_and_lease_gates_reject_stale_or_incomplete_tools() {
        let success = std::process::ExitStatus::from_raw(0);
        assert_eq!(
            verified_version(success, b"v2.0.1\n").expect("version"),
            "2.0.1"
        );
        assert!(verified_version(success, b"v2.0.0\n").is_err());
        assert!(verify_lease(success, b"get [--lease]\n", b"").is_ok());
        assert!(verify_lease(success, b"get\n", b"").is_err());
    }

    #[test]
    fn verified_archive_installs_an_exact_version_with_lease_support() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        let extraction = temp.path().join("extraction");
        let destination = temp.path().join("destination");
        std::fs::create_dir(&source).expect("source");
        std::fs::create_dir(&extraction).expect("extraction");
        let binary = source.join("treehouse");
        std::fs::write(
            &binary,
            "#!/bin/sh\nif [ \"${1:-}\" = --version ]; then printf 'v2.0.1\\n'; else printf '%s\\n' 'get --lease'; fi\n",
        )
        .expect("binary");
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).expect("mode");
        let archive = temp.path().join("fixture.tar.gz");
        assert!(
            std::process::Command::new("tar")
                .args(["-czf"])
                .arg(&archive)
                .args(["-C"])
                .arg(&source)
                .arg("treehouse")
                .status()
                .expect("tar")
                .success()
        );
        let expected = hash(&archive).expect("hash");
        assert_eq!(
            install_archive(
                &destination,
                &extraction,
                &archive,
                "fixture.tar.gz",
                &expected
            )
            .expect("install"),
            "v2.0.1"
        );
        assert!(destination.join("treehouse").is_file());
    }
}

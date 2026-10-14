//! Installer generation failures must not consume a verified retired intent.
use multplx_domain::lifecycle::{
    home_seed::{HomeAllocation, HomeBinding},
    spawn::{LaunchAction, LaunchStage},
    subagent_model::{ArtifactKind, AssignmentRole, TaskRecord},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    process::Command,
};
fn write(path: &Path, bytes: &[u8], mode: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}
#[test]
fn packaged_upgrade_retired_intent_requires_consent_and_survives_install_failure() {
    let temp = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(temp.path()).unwrap();
    let package = base.join("package");
    // Payload integrity and generation rollback are independent of execution.
    for (name, bytes, mode) in [
        ("VERSION", b"0.1.0\n".as_slice(), 0o644),
        (
            "PLATFORM",
            format!(
                "{}-{}\n",
                if cfg!(target_os = "macos") {
                    "macos"
                } else {
                    "linux"
                },
                std::env::consts::ARCH
            )
            .as_bytes(),
            0o644,
        ),
        ("bin/mx", b"#!/bin/sh\nexit 0\n", 0o755),
        ("bin/multplx", b"#!/bin/sh\nexit 0\n", 0o755),
        ("runtime/AGENTS.md", b"contract\n", 0o644),
        ("runtime/share/shell/multplx.bash", b"#shell\n", 0o644),
        ("runtime/share/shell/multplx.zsh", b"#shell\n", 0o644),
        (
            "runtime/share/shell/shims/multplx",
            b"#!/bin/sh\nexit 0\n",
            0o755,
        ),
        ("runtime/bin/mx-launcher.sh", b"#!/bin/sh\nexit 0\n", 0o755),
        ("runtime/.agents/skills/example.md", b"skill\n", 0o644),
        ("runtime/target/release/mx", b"#!/bin/sh\nexit 0\n", 0o755),
    ] {
        write(&package.join(name), bytes, mode);
    }
    let mut manifest = String::new();
    for name in [
        "VERSION",
        "PLATFORM",
        "bin/mx",
        "bin/multplx",
        "runtime/AGENTS.md",
        "runtime/share/shell/multplx.bash",
        "runtime/share/shell/multplx.zsh",
        "runtime/share/shell/shims/multplx",
        "runtime/bin/mx-launcher.sh",
        "runtime/.agents/skills/example.md",
        "runtime/target/release/mx",
    ] {
        let path = package.join(name);
        let bytes = fs::read(&path).unwrap();
        manifest.push_str(&format!(
            "{:x}\t{:04o}\t{name}\n",
            Sha256::digest(bytes),
            fs::metadata(path).unwrap().permissions().mode() & 0o777
        ));
    }
    write(&package.join("SHA256SUMS"), manifest.as_bytes(), 0o644);
    let home = base.join("home");
    let config = base.join("config");
    let bin = base.join("bin");
    let data = base.join("data");
    let common = [
        "--package".into(),
        package.to_string_lossy().into_owned(),
        "--home".into(),
        home.to_string_lossy().into_owned(),
        "--config-dir".into(),
        config.to_string_lossy().into_owned(),
        "--bin-dir".into(),
        bin.to_string_lossy().into_owned(),
        "--data-dir".into(),
        data.to_string_lossy().into_owned(),
    ];
    let run = |extra: &[&str], failure: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_mx"));
        cmd.arg("launcher-install").args(&common).args(extra);
        if failure {
            cmd.env("MX_LAUNCHER_INSTALL_FAIL_AFTER", "multplx");
        }
        cmd.output().unwrap()
    };
    let installed = run(&[], false);
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let root = data.join("runtime");
    let archive = base.join("archive");
    fs::create_dir_all(archive.join("state")).unwrap();
    fs::create_dir_all(archive.join("data")).unwrap();
    write(
        &archive.join(multplx_core::agent_home::MARKER),
        b"retired\n",
        0o600,
    );
    write(&archive.join("private.txt"), b"private bytes\n", 0o600);
    let token = HomeBinding {
        id: "retired".into(),
        owner_home: home.clone(),
        path: base.join("old-home"),
        lease_id: "retired-lease".into(),
        generation: 1,
    };
    let owner = format!("root-home:{}", home.display());
    let mut record = TaskRecord::new(
        "retired".into(),
        AssignmentRole::Researcher,
        ArtifactKind::Report,
        true,
        owner.clone(),
        owner,
        home.to_string_lossy().into_owned(),
    );
    record.home_allocation = Some(token.clone());
    record.persistent_home = Some(token.path.to_string_lossy().into_owned());
    record.runtime.provider = "tmux".into();
    record.runtime.endpoint = Some("mx-no-such-server:mx-retired".into());
    let action = LaunchAction {
        version: 1,
        request_id: "old-launch".into(),
        task_id: "retired".into(),
        binding: record.clone(),
        backend: "tmux".into(),
        endpoint: record.runtime.endpoint.clone(),
        worktree: None,
        stage: LaunchStage::Running,
        detail: None,
    };
    write(
        &home.join("state/.spawn-actions/old-launch.json"),
        &serde_json::to_vec(&action).unwrap(),
        0o600,
    );
    let previous = record.attempt.as_ref().unwrap().id.clone();
    record.replace_attempt(&previous, true).unwrap();
    let intent = serde_json::to_vec(&record).unwrap();
    let intent_path = home.join("state/.spawn-retired.intent");
    write(&intent_path, &intent, 0o600);
    let meta = fs::metadata(&archive).unwrap();
    let allocation = HomeAllocation {
        version: 1,
        binding: token,
        runtime_root: root,
        state: "retired".into(),
        retained_path: Some(archive.clone()),
        directory_identity: Some((meta.dev(), meta.ino())),
        git_allocation: None,
    };
    write(
        &home.join("data/.home-allocation-retired.json"),
        &serde_json::to_vec(&allocation).unwrap(),
        0o600,
    );
    let refused = run(&["--upgrade"], false);
    assert_eq!(
        refused.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_eq!(fs::read(&intent_path).unwrap(), intent);
    assert!(!home.join("state/.retired-spawn-intents").exists());
    let failed = run(
        &[
            "--upgrade",
            "--allow-stopped-agents",
            "--cleanup-stale-state",
        ],
        true,
    );
    assert_eq!(
        failed.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&failed.stderr)
    );
    assert_eq!(fs::read(&intent_path).unwrap(), intent);
    assert!(!home.join("state/.retired-spawn-intents").exists());
    let upgraded = run(
        &[
            "--upgrade",
            "--allow-stopped-agents",
            "--cleanup-stale-state",
        ],
        false,
    );
    assert!(
        upgraded.status.success(),
        "{}",
        String::from_utf8_lossy(&upgraded.stderr)
    );
    assert!(!intent_path.exists());
    assert_eq!(
        fs::read(archive.join("private.txt")).unwrap(),
        b"private bytes\n"
    );
    assert!(!home.join("state/retired.meta").exists());
    let runtime = data.join("runtime");
    let args = [
        "upgrade-cleanup",
        "retired-intent",
        "apply",
        "--root",
        runtime.to_str().unwrap(),
        "--home",
        home.to_str().unwrap(),
        "--task",
        "retired",
    ];
    let repeat = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        repeat.status.success(),
        "{}",
        String::from_utf8_lossy(&repeat.stderr)
    );
}

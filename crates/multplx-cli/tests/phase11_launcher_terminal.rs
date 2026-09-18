//! Terminal-level Phase 11 acceptance with an isolated tmux server and synthetic harness.

use std::path::Path;
use std::process::Command;

fn launch_harness(args: &[&str], environment: &[(&str, &Path)]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    command
        .arg("launch-harness")
        .args(args)
        .env_remove("MX_ROOT_OVERRIDE")
        .env_remove("MX_HOME")
        .env_remove("MX_LAUNCH_VALIDATED")
        .env_remove("MX_REAL_CLAUDE")
        .env_remove("MX_REAL_CODEX")
        .env_remove("MX_REAL_CURSOR_AGENT")
        .env_remove("MX_REAL_PI");
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().expect("run harness launch contract")
}

fn assert_code(output: &std::process::Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn true_executable() -> &'static Path {
    if cfg!(target_os = "macos") {
        Path::new("/usr/bin/true")
    } else {
        Path::new("/bin/true")
    }
}

#[test]
fn isolated_tmux_attach_and_crash_reservation_keep_one_owner() {
    if !Command::new("tmux")
        .arg("-V")
        .status()
        .is_ok_and(|status| status.success())
    {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("bash")
        .arg(root.join("tests/mx-launcher-connection.test.sh"))
        .env("MX_TEST_BINARY", env!("CARGO_BIN_EXE_mx"))
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .env("MX_RUST_SOURCE_ROOT", &root)
        .current_dir(&root)
        .output()
        .expect("run isolated launcher terminal fixture");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn harness_launch_rejects_invalid_boundaries_and_runs_a_short_lived_child() {
    let temp = tempfile::tempdir().expect("tempdir");
    let runtime = temp.path().join("runtime");
    let home = temp.path().join("home");
    std::fs::create_dir_all(runtime.join("bin")).unwrap();
    std::fs::create_dir_all(home.join("state")).unwrap();
    std::fs::write(runtime.join("AGENTS.md"), "fixture\n").unwrap();
    std::fs::write(runtime.join("bin/mx-lock.sh"), "#!/bin/sh\n").unwrap();
    let mut permissions = std::fs::metadata(runtime.join("bin/mx-lock.sh"))
        .unwrap()
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o700);
    std::fs::set_permissions(runtime.join("bin/mx-lock.sh"), permissions).unwrap();

    assert_code(&launch_harness(&["other"], &[]), 2);
    assert_code(&launch_harness(&["codex"], &[]), 2);
    assert_code(
        &launch_harness(
            &["codex"],
            &[
                ("MX_ROOT_OVERRIDE", Path::new("relative")),
                ("MX_HOME", &home),
                ("MX_LAUNCH_VALIDATED", Path::new("1")),
            ],
        ),
        2,
    );
    assert_code(
        &launch_harness(
            &["codex"],
            &[
                ("MX_ROOT_OVERRIDE", temp.path().join("missing").as_path()),
                ("MX_HOME", &home),
                ("MX_LAUNCH_VALIDATED", Path::new("1")),
            ],
        ),
        2,
    );

    let validated = [
        ("MX_ROOT_OVERRIDE", runtime.as_path()),
        ("MX_HOME", home.as_path()),
        ("MX_LAUNCH_VALIDATED", Path::new("1")),
    ];
    assert_code(&launch_harness(&["codex"], &validated), 127);

    let relative_real = [
        ("MX_ROOT_OVERRIDE", runtime.as_path()),
        ("MX_HOME", home.as_path()),
        ("MX_LAUNCH_VALIDATED", Path::new("1")),
        ("MX_REAL_CODEX", Path::new("codex")),
    ];
    assert_code(&launch_harness(&["codex"], &relative_real), 127);

    let cursor = [
        ("MX_ROOT_OVERRIDE", runtime.as_path()),
        ("MX_HOME", home.as_path()),
        ("MX_LAUNCH_VALIDATED", Path::new("1")),
        ("MX_REAL_CURSOR_AGENT", true_executable()),
    ];
    assert_code(&launch_harness(&["cursor", "--yolo"], &cursor), 2);

    std::fs::write(home.join("state/workspace-launch.json"), "not json").unwrap();
    let codex = [
        ("MX_ROOT_OVERRIDE", runtime.as_path()),
        ("MX_HOME", home.as_path()),
        ("MX_LAUNCH_VALIDATED", Path::new("1")),
        ("MX_REAL_CODEX", true_executable()),
    ];
    assert_code(&launch_harness(&["codex"], &codex), 3);
    std::fs::remove_file(home.join("state/workspace-launch.json")).unwrap();

    assert_code(&launch_harness(&["codex"], &codex), 2);
    std::fs::create_dir(home.join("config")).unwrap();
    assert_code(&launch_harness(&["codex"], &codex), 0);
}

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

fn mx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mx"))
}

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("source root")
}

#[test]
fn unknown_review_entry_is_rejected_before_execution() {
    let output = mx()
        .args(["review", "mx-unknown.sh"])
        .output()
        .expect("run mx");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: unknown review or delivery entry point: mx-unknown.sh\n"
    );
}

#[test]
fn deep_review_is_native_and_does_not_execute_a_same_named_shell_body() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("bin");
    let script = bin.join("mx-deep-review.sh");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$MX_REVIEW_DELIVERY_IMPLEMENTATION\"\n",
    )
    .expect("script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("mode");
    let output = mx()
        .args(["review", "mx-deep-review.sh"])
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .env("MX_REVIEW_DELIVERY_IMPLEMENTATION", "rust")
        .output()
        .expect("run mx");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("legacy"));
}

#[test]
fn deep_review_rejects_closed_usage_without_a_retained_body() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = mx()
        .args(["review", "mx-deep-review.sh"])
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .output()
        .expect("run mx");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("compatibility body"));
}

#[test]
fn every_public_adapter_rejects_an_invalid_selector_before_state_access() {
    let temp = tempfile::tempdir().expect("tempdir");
    for entry in [
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
    ] {
        let output = Command::new(source_root().join("bin").join(entry))
            .env("MX_ROOT_OVERRIDE", source_root())
            .env("MX_STATE_OVERRIDE", temp.path().join("missing-state"))
            .env("MX_REVIEW_DELIVERY_IMPLEMENTATION", "invalid")
            .output()
            .expect("run adapter");
        assert_eq!(output.status.code(), Some(2), "{entry}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "error: MX_REVIEW_DELIVERY_IMPLEMENTATION must be rust or legacy\n",
            "{entry}"
        );
    }
    assert!(!temp.path().join("missing-state").exists());
}

#[test]
fn native_custom_check_registration_is_private_and_content_bound() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let check = state.join("task-a.check.sh");
    fs::write(&check, "#!/bin/sh\nprintf ready\\n\n").expect("check");
    fs::set_permissions(&check, fs::Permissions::from_mode(0o700)).expect("mode");
    let output = mx()
        .args(["review", "mx-check-register.sh", "task-a"])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("run mx");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let trust = state.join("task-a.check-trust");
    assert_eq!(
        fs::metadata(&trust).expect("trust").permissions().mode() & 0o777,
        0o600
    );
    assert!(
        fs::read_to_string(&trust)
            .expect("trust bytes")
            .starts_with("mx-custom-check-v1\n")
    );

    fs::write(&check, "#!/bin/sh\nprintf replaced\\n\n").expect("replace");
    fs::set_permissions(&check, fs::Permissions::from_mode(0o700)).expect("mode");
    assert!(
        !multplx_core::checks::registered(
            &state,
            &multplx_core::identifiers::TaskId::parse("task-a").expect("task")
        )
        .expect("registration")
    );
}

#[test]
fn native_custom_check_registration_preserves_operational_ids_and_refuses_unsafe_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let task = "x".repeat(100);
    let check = state.join(format!("{task}.check.sh"));
    fs::write(&check, b"#!/bin/sh\nexit 0\n").expect("check");
    fs::set_permissions(&check, fs::Permissions::from_mode(0o700)).expect("mode");
    let output = mx()
        .args(["review", "mx-check-register.sh", &task])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("register");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(state.join(format!("{task}.check-trust")).is_file());

    fs::set_permissions(&check, fs::Permissions::from_mode(0o755)).expect("bad mode");
    let output = mx()
        .args(["review", "mx-check-register.sh", &task])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("register");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("custom check is unavailable"));

    let output = mx()
        .args(["review", "mx-check-register.sh", "../bad"])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("register");
    assert_eq!(output.status.code(), Some(2));

    let linked_state = temp.path().join("linked-state");
    symlink(&state, &linked_state).expect("state symlink");
    let output = mx()
        .args(["review", "mx-check-register.sh", "task-a"])
        .env("MX_STATE_OVERRIDE", &linked_state)
        .output()
        .expect("register");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("state directory is unavailable"));

    let output = mx()
        .args(["review", "mx-check-register.sh"])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("register");
    assert_eq!(output.status.code(), Some(2));
    let output = mx()
        .args(["review", "mx-check-register.sh", "task-a", "extra"])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("register");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn native_static_poll_revalidates_every_sidecar_component() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fake = temp.path().join("gh");
    fs::write(&fake, "#!/bin/sh\nprintf 'MERGED\\n'\n").expect("gh");
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).expect("mode");
    let path = format!("{}:/usr/bin:/bin", temp.path().display());
    let good = mx()
        .args([
            "review",
            "mx-pr-poll.sh",
            "--validated",
            "github",
            "https://github.com/o/r/pull/9",
            "github.com",
            "o/r",
            "9",
        ])
        .env("PATH", &path)
        .output()
        .expect("poll");
    assert!(good.status.success());
    assert_eq!(String::from_utf8_lossy(&good.stdout), "merged\n");
    let bad = mx()
        .args([
            "review",
            "mx-pr-poll.sh",
            "--validated",
            "github",
            "https://github.com/o/r/pull/9",
            "evil.example",
            "o/r",
            "9",
        ])
        .env("PATH", path)
        .output()
        .expect("poll");
    assert!(bad.status.success());
    assert!(bad.stdout.is_empty());

    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let check = state.join("task-a.check.sh");
    fs::write(&check, b"#!/bin/sh\n").expect("check");
    let sidecar = state.join("task-a.pr-poll");
    fs::write(
        &sidecar,
        b"github\nhttps://github.com/o/r/pull/9\ngithub.com\no/r\n9\n",
    )
    .expect("sidecar");
    fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o600)).expect("mode");
    let static_poll = mx()
        .args(["review", "mx-pr-poll.sh"])
        .env("MX_PR_POLL_CHECK_PATH", &check)
        .env("PATH", format!("{}:/usr/bin:/bin", temp.path().display()))
        .output()
        .expect("static poll");
    assert_eq!(String::from_utf8_lossy(&static_poll.stdout), "merged\n");
    fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o644)).expect("bad mode");
    let refused = mx()
        .args(["review", "mx-pr-poll.sh"])
        .env("MX_PR_POLL_CHECK_PATH", &check)
        .env("PATH", format!("{}:/usr/bin:/bin", temp.path().display()))
        .output()
        .expect("static poll");
    assert!(refused.status.success());
    assert!(refused.stdout.is_empty());

    let no_sidecar = mx()
        .args(["review", "mx-pr-poll.sh"])
        .env("MX_PR_POLL_CHECK_PATH", state.join("missing.check.sh"))
        .env("PATH", format!("{}:/usr/bin:/bin", temp.path().display()))
        .output()
        .expect("static poll");
    assert!(no_sidecar.stdout.is_empty());
    let wrong_suffix = mx()
        .args(["review", "mx-pr-poll.sh"])
        .env("MX_PR_POLL_CHECK_PATH", state.join("task-a.check"))
        .output()
        .expect("static poll");
    assert!(wrong_suffix.stdout.is_empty());
    let non_utf8 = mx()
        .arg("review")
        .arg("mx-pr-poll.sh")
        .arg(OsString::from_vec(vec![0xff]))
        .output()
        .expect("static poll");
    assert!(non_utf8.stdout.is_empty());
}

#[test]
fn native_promotion_preserves_identity_and_quotes_the_followup_home() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let task = "x".repeat(100);
    let meta = state.join(format!("{task}.meta"));
    fs::write(&meta, "actor=codex\nkind=scout\n").expect("meta");
    fs::set_permissions(&meta, fs::Permissions::from_mode(0o600)).expect("mode");
    let output = mx()
        .args(["review", "mx-promote.sh", &task])
        .env("MX_STATE_OVERRIDE", &state)
        .env("MX_HOME", temp.path().join("home with ' quote"))
        .output()
        .expect("promote");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&meta).expect("meta"),
        "actor=codex\nkind=delivery\n"
    );
    assert_eq!(
        fs::metadata(&meta).expect("meta").permissions().mode() & 0o777,
        0o600
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("promoted {task} to delivery")));
    assert!(stdout.contains("home with '\\'' quote"));

    let output = mx()
        .args(["review", "mx-promote.sh", "../bad"])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("promote");
    assert_eq!(output.status.code(), Some(1));
    let output = mx()
        .args(["review", "mx-promote.sh", "missing"])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("promote");
    assert_eq!(output.status.code(), Some(1));
    fs::write(state.join("delivery.meta"), "kind=delivery\n").expect("meta");
    fs::set_permissions(
        state.join("delivery.meta"),
        fs::Permissions::from_mode(0o600),
    )
    .expect("mode");
    let output = mx()
        .args(["review", "mx-promote.sh", "delivery"])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("promote");
    assert_eq!(output.status.code(), Some(1));

    let missing_state = mx()
        .args(["review", "mx-promote.sh", "missing"])
        .env("MX_STATE_OVERRIDE", temp.path().join("missing-state"))
        .output()
        .expect("promote");
    assert_eq!(missing_state.status.code(), Some(1));
    let linked_state = temp.path().join("linked-state");
    symlink(&state, &linked_state).expect("state link");
    let linked = mx()
        .args(["review", "mx-promote.sh", &task])
        .env("MX_STATE_OVERRIDE", &linked_state)
        .output()
        .expect("promote");
    assert_eq!(linked.status.code(), Some(1));

    let unsafe_meta = state.join("unsafe.meta");
    fs::write(&unsafe_meta, b"kind=scout\n").expect("meta");
    fs::set_permissions(&unsafe_meta, fs::Permissions::from_mode(0o644)).expect("mode");
    let unsafe_output = mx()
        .args(["review", "mx-promote.sh", "unsafe"])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("promote");
    assert_eq!(unsafe_output.status.code(), Some(1));
    fs::write(&unsafe_meta, [0xff]).expect("meta");
    fs::set_permissions(&unsafe_meta, fs::Permissions::from_mode(0o600)).expect("mode");
    let invalid_utf8 = mx()
        .args(["review", "mx-promote.sh", "unsafe"])
        .env("MX_STATE_OVERRIDE", &state)
        .output()
        .expect("promote");
    assert_eq!(invalid_utf8.status.code(), Some(1));
}

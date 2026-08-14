use std::fs;
use std::process::Command;

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn run_bash(script: &str) -> std::process::Output {
    Command::new("/bin/bash")
        .arg("-c")
        .arg(script)
        .env("REPO_ROOT", repo_root())
        .output()
        .expect("run differential harness self-test")
}

#[test]
fn equal_legacy_and_rust_observations_compare_cleanly() {
    let output = run_bash(
        r#"
set -eu
. "$REPO_ROOT/tests/lib.sh"
mx_test_tmproot_into temp mx-parity-equal
mkdir -p "$temp/tools" "$temp/legacy-home" "$temp/rust-home"
cat > "$temp/tools/legacy" <<'SH'
#!/bin/sh
printf 'same stdout\n'
printf 'same stderr\n' >&2
mkdir -p "$MX_HOME/state"
printf 'same bytes\n' > "$MX_HOME/state/record"
chmod 600 "$MX_HOME/state/record"
SH
cat > "$temp/tools/rust" <<'SH'
#!/bin/sh
shift
printf 'same stdout\n'
printf 'same stderr\n' >&2
mkdir -p "$MX_HOME/state"
printf 'same bytes\n' > "$MX_HOME/state/record"
chmod 600 "$MX_HOME/state/record"
SH
chmod +x "$temp/tools/legacy" "$temp/tools/rust"
export MX_TEST_RUST_BIN="$temp/tools/rust"
mx_test_capture_command "$temp/legacy" "$temp/legacy-home" -- \
  "$temp/tools/legacy"
mx_test_capture_command "$temp/rust" "$temp/rust-home" -- \
  "$MX_TEST_RUST_BIN" shadow-fixture
mx_test_assert_differential_equal "$temp/legacy" "$temp/rust"
"#,
    );

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn comparator_detects_every_protected_observation_class() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    fs::create_dir(&left).expect("left capture");
    fs::create_dir(&right).expect("right capture");
    for field in ["status", "stdout", "stderr", "filesystem", "processes"] {
        fs::write(left.join(field), b"same\n").expect("left observation");
        fs::write(right.join(field), b"same\n").expect("right observation");
    }

    for field in ["status", "stdout", "stderr", "filesystem", "processes"] {
        fs::write(right.join(field), b"different\n").expect("inject difference");
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg(r#". "$REPO_ROOT/tests/lib.sh"; mx_test_compare_captures "$LEFT" "$RIGHT""#)
            .env("REPO_ROOT", repo_root())
            .env("LEFT", &left)
            .env("RIGHT", &right)
            .output()
            .expect("run comparator");
        assert!(!output.status.success(), "{field} difference was hidden");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains(&format!("differential mismatch: {field}")),
            "missing {field} diagnostic"
        );
        fs::write(right.join(field), b"same\n").expect("restore observation");
    }
}

#[test]
fn capture_detects_content_mode_ordering_and_real_process_leaks() {
    let output = run_bash(
        r#"
set -eu
. "$REPO_ROOT/tests/lib.sh"
mx_test_tmproot_into temp mx-parity-differences
mkdir -p "$temp/tools"
cat > "$temp/tools/left" <<'SH'
#!/bin/sh
printf 'one\ntwo\n'
mkdir -p "$MX_HOME/state"
printf 'left\n' > "$MX_HOME/state/record"
chmod 600 "$MX_HOME/state/record"
SH
cat > "$temp/tools/right" <<'SH'
#!/bin/sh
printf 'two\none\n'
mkdir -p "$MX_HOME/state"
printf 'right\n' > "$MX_HOME/state/record"
chmod 644 "$MX_HOME/state/record"
/bin/sh -c 'while :; do sleep 30; done' "$MX_HOME/process-leak" >/dev/null 2>&1 &
SH
chmod +x "$temp/tools/left" "$temp/tools/right"
mx_test_capture_command "$temp/left" "$temp/left-home" -- "$temp/tools/left"
mx_test_capture_command "$temp/right" "$temp/right-home" -- "$temp/tools/right"
grep -F '<HOME>/process-leak' "$temp/right/processes" >/dev/null
if grep -F "$temp/right-home" "$temp/right/processes" >/dev/null; then
  exit 1
fi
if mx_test_compare_captures "$temp/left" "$temp/right" 2> "$temp/diff"; then
  exit 1
fi
grep -F 'differential mismatch: stdout' "$temp/diff" >/dev/null
grep -F 'differential mismatch: filesystem' "$temp/diff" >/dev/null
grep -F 'differential mismatch: processes' "$temp/diff" >/dev/null
leaked=$(ps -axo pid=,command= | awk -v needle="$temp/right-home/process-leak" 'index($0, needle) { print $1 }')
[ -n "$leaked" ]
kill $leaked 2>/dev/null || true

# Hold every observation except mode equal and prove mode remains visible.
printf 'left\n' > "$temp/right-home/state/record"
chmod 644 "$temp/right-home/state/record"
mx_test_filesystem_manifest "$temp/right-home" > "$temp/right/filesystem"
cp "$temp/left/status" "$temp/left/stdout" "$temp/left/stderr" \
  "$temp/left/processes" "$temp/right/"
if mx_test_compare_captures "$temp/left" "$temp/right" 2> "$temp/mode-diff"; then
  exit 1
fi
grep -F 'differential mismatch: filesystem' "$temp/mode-diff" >/dev/null

# Hold every observation except content equal and prove exact bytes remain visible.
printf 'right\n' > "$temp/right-home/state/record"
chmod 600 "$temp/right-home/state/record"
mx_test_filesystem_manifest "$temp/right-home" > "$temp/right/filesystem"
if mx_test_compare_captures "$temp/left" "$temp/right" 2> "$temp/content-diff"; then
  exit 1
fi
grep -F 'differential mismatch: filesystem' "$temp/content-diff" >/dev/null
"#,
    );

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

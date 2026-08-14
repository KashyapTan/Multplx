use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn run(script: &str, home: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(root().join("bin").join(script))
        .args(args)
        .env("MX_HOME", home)
        .env("MX_ROOT_OVERRIDE", root())
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start adapter");
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("adapter output")
}

fn scaffold(home: &Path) {
    fs::create_dir_all(home.join("data")).expect("data");
    fs::create_dir_all(home.join("config")).expect("config");
    fs::write(
        home.join("data/backlog.md"),
        "## In flight\n\n## Queued\n\n## Done\n",
    )
    .expect("backlog");
}

#[test]
fn backlog_adapter_preserves_bytes_and_expected_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    scaffold(&home);
    let commands: &[&[&str]] = &[
        &[
            "add",
            "alpha",
            "Literal title",
            "--body",
            "line one\n\n## Intent\nline two",
        ],
        &["add", "beta", "Dependent", "--blocked-by", "alpha"],
        &[
            "hold",
            "alpha",
            "--reason",
            "maintainer answer",
            "--kind",
            "maintainer",
        ],
        &["update", "alpha", "--body", "replacement\nbody"],
        &["unblock", "beta", "--by", "alpha"],
        &["list", "--limit", "80"],
        &["show", "alpha"],
        &["validate"],
    ];
    for args in commands {
        let output = run("mx-backlog.sh", &home, args, b"");
        assert!(output.status.success(), "status for {args:?}");
        assert!(output.stderr.is_empty(), "stderr for {args:?}");
    }
}

#[test]
fn read_only_invalid_utf8_is_lossy_without_rewrite() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    scaffold(&home);
    let bytes = b"## In flight\n\n## Queued\n- [ ] odd - invalid \xff byte\n\n## Done\n";
    fs::write(home.join("data/backlog.md"), bytes).expect("backlog bytes");
    for args in [&["validate"][..], &["list"][..]] {
        let output = run("mx-backlog.sh", &home, args, b"");
        assert!(output.status.success());
    }
    assert_eq!(
        fs::read(home.join("data/backlog.md")).expect("backlog"),
        bytes
    );
}

#[test]
fn project_mode_and_operational_codec_are_native() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path();
    fs::create_dir_all(home.join("data")).expect("data");
    fs::write(
        home.join("data/projects.md"),
        "- app [local-only +yolo] - app\n- bad [unknown] - bad\n",
    )
    .expect("registry");
    for name in ["app", "bad", "missing"] {
        let output = run("mx-project-mode.sh", home, &[name], b"");
        if name == "app" {
            assert!(output.status.success());
            assert_eq!(output.stdout, b"local-only on\n");
        } else {
            assert!(output.status.success());
            assert_eq!(output.stdout, b"deep-review off\n");
            assert!(String::from_utf8_lossy(&output.stderr).contains("defaulting"));
        }
    }
    let body = b"quoted ' body\nsecond line";
    let encoded = run(
        "mx-operational-input.sh",
        home,
        &["encode", "watcher"],
        body,
    );
    assert!(encoded.status.success());
    assert!(encoded.stderr.is_empty());
    assert!(encoded.stdout.ends_with(body));
}

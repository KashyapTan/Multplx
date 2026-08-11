use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn run(script: &str, implementation: &str, home: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(root().join("bin").join(script))
        .args(args)
        .env("MX_HOME", home)
        .env("MX_ROOT_OVERRIDE", root())
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .env("MX_LOCAL_STATE_IMPLEMENTATION", implementation)
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
fn backlog_adapters_match_bytes_streams_and_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let legacy = temp.path().join("legacy");
    let rust = temp.path().join("rust");
    scaffold(&legacy);
    scaffold(&rust);
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
        let old = run("mx-backlog.sh", "legacy", &legacy, args, b"");
        let new = run("mx-backlog.sh", "rust", &rust, args, b"");
        assert_eq!(new.status.code(), old.status.code(), "status for {args:?}");
        assert_eq!(new.stdout, old.stdout, "stdout for {args:?}");
        assert_eq!(new.stderr, old.stderr, "stderr for {args:?}");
        assert_eq!(
            fs::read(rust.join("data/backlog.md")).expect("Rust backlog"),
            fs::read(legacy.join("data/backlog.md")).expect("legacy backlog"),
            "filesystem bytes for {args:?}"
        );
    }
}

#[test]
fn read_only_invalid_utf8_behavior_matches_without_rewrite() {
    let temp = tempfile::tempdir().expect("tempdir");
    let legacy = temp.path().join("legacy");
    let rust = temp.path().join("rust");
    scaffold(&legacy);
    scaffold(&rust);
    let bytes = b"## In flight\n\n## Queued\n- [ ] odd - invalid \xff byte\n\n## Done\n";
    fs::write(legacy.join("data/backlog.md"), bytes).expect("legacy bytes");
    fs::write(rust.join("data/backlog.md"), bytes).expect("Rust bytes");
    for args in [&["validate"][..], &["list"][..]] {
        let old = run("mx-backlog.sh", "legacy", &legacy, args, b"");
        let new = run("mx-backlog.sh", "rust", &rust, args, b"");
        assert_eq!(new.status.code(), old.status.code());
        assert_eq!(new.stdout, old.stdout);
        assert_eq!(new.stderr, old.stderr);
    }
    assert_eq!(
        fs::read(legacy.join("data/backlog.md")).expect("legacy"),
        bytes
    );
    assert_eq!(fs::read(rust.join("data/backlog.md")).expect("Rust"), bytes);
}

#[test]
fn project_mode_and_operational_codec_match_legacy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path();
    fs::create_dir_all(home.join("data")).expect("data");
    fs::write(
        home.join("data/projects.md"),
        "- app [local-only +yolo] - app\n- bad [unknown] - bad\n",
    )
    .expect("registry");
    for name in ["app", "bad", "missing"] {
        let old = run("mx-project-mode.sh", "legacy", home, &[name], b"");
        let new = run("mx-project-mode.sh", "rust", home, &[name], b"");
        assert_eq!(new.status.code(), old.status.code());
        assert_eq!(new.stdout, old.stdout);
        assert_eq!(new.stderr, old.stderr);
    }
    let body = b"quoted ' body\nsecond line";
    let old = run(
        "mx-operational-input.sh",
        "legacy",
        home,
        &["encode", "watcher"],
        body,
    );
    let new = run(
        "mx-operational-input.sh",
        "rust",
        home,
        &["encode", "watcher"],
        body,
    );
    assert_eq!(new.status.code(), old.status.code());
    assert_eq!(new.stdout, old.stdout);
    assert_eq!(new.stderr, old.stderr);
}

#[test]
fn invalid_selector_refuses_before_mutation() {
    let temp = tempfile::tempdir().expect("tempdir");
    scaffold(temp.path());
    let before = fs::read(temp.path().join("data/backlog.md")).expect("before");
    let output = run(
        "mx-backlog.sh",
        "not-an-engine",
        temp.path(),
        &["add", "unsafe", "Must not land"],
        b"",
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        fs::read(temp.path().join("data/backlog.md")).expect("after"),
        before
    );
}

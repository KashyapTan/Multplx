use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Output};

use multplx_domain::maintainer_override::{self, OverrideStore};

fn mx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mx"))
}

fn run(command: &mut Command) -> Output {
    command.output().expect("run mx")
}

fn source_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent");
    fs::write(path, body).expect("script");
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("mode");
}

#[test]
fn compatibility_compositions_are_process_pinned_to_legacy() {
    let temp = tempfile::tempdir().expect("tempdir");
    executable(
        &temp.path().join("bin/mx-workflow.sh"),
        "#!/bin/sh\nprintf '%s|%s\\n' \"${MX_AUTHORITY_IMPLEMENTATION:-unset}\" \"${1:-}\"\n",
    );
    let output = run(mx()
        .env("MX_ROOT_OVERRIDE", temp.path())
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args(["authority", "mx-workflow.sh", "resume"]));
    assert!(output.status.success());
    assert_eq!(output.stdout, b"legacy|resume\n");
}

#[test]
fn native_registry_and_digest_match_the_closed_contract() {
    let registry = run(mx().args([
        "authority",
        "mx-maintainer-override.sh",
        "registry",
        "--json",
    ]));
    assert!(registry.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&registry.stdout).expect("registry JSON");
    assert_eq!(rows.as_array().expect("array").len(), 20);
    assert!(rows.as_array().expect("array").iter().any(|row| {
        row["boundary_id"] == "integrity.validation-state" && row["class"] == "integrity"
    }));

    let digest = run(mx().args([
        "authority",
        "mx-maintainer-override.sh",
        "digest",
        "literal text",
    ]));
    assert!(digest.status.success());
    assert_eq!(
        String::from_utf8_lossy(&digest.stdout).trim(),
        maintainer_override::sha256_text("literal text")
    );
}

#[test]
fn native_request_is_private_and_consumption_is_single_use() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let digest = maintainer_override::sha256_text("state-v1");
    let request = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "request",
        "--boundary",
        "workflow.skip-stage",
        "--task",
        "run-1",
        "--project",
        "multplx",
        "--operation",
        "skip workflow stage build in run run-1",
        "--target",
        "run-1#build",
        "--expected-state",
        &digest,
        "--consequence",
        "Skip only the named stage.",
    ]));
    assert!(
        request.status.success(),
        "{}",
        String::from_utf8_lossy(&request.stderr)
    );
    let id = String::from_utf8_lossy(&request.stdout).trim().to_owned();
    let store = OverrideStore::new(&state);
    let (_, path, record) = store.find(&id).expect("request record");
    assert_eq!(
        fs::metadata(store.root())
            .expect("root")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let metadata = fs::metadata(&path).expect("record");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    let words = format!(
        "Grant {} for exact operation {} on exact target {}.",
        record.boundary_id, record.action_argv_or_operation, record.target_identity
    );
    store.decide(&id, &words, true).expect("grant fixture");
    let consume_args = [
        "authority",
        "mx-maintainer-override.sh",
        "consume",
        &id,
        "--boundary",
        "workflow.skip-stage",
        "--task",
        "run-1",
        "--project",
        "multplx",
        "--operation",
        "skip workflow stage build in run run-1",
        "--target",
        "run-1#build",
        "--expected-state",
        &digest,
    ];
    let consumed = run(mx().env("MX_STATE_OVERRIDE", &state).args(consume_args));
    assert!(
        consumed.status.success(),
        "{}",
        String::from_utf8_lossy(&consumed.stderr)
    );
    let replay = run(mx().env("MX_STATE_OVERRIDE", &state).args(consume_args));
    assert!(!replay.status.success());
}

#[test]
fn native_workflow_validation_and_dry_run_need_no_node_process() {
    let source = source_root();
    let validate = run(mx()
        .env("MX_RUST_SOURCE_ROOT", source)
        .env("PATH", "/usr/bin:/bin")
        .args([
            "authority",
            "mx-workflow.sh",
            "validate",
            source
                .join("workflows/new-feature.workflow.md")
                .to_str()
                .expect("path"),
        ]));
    assert!(
        validate.status.success(),
        "{}",
        String::from_utf8_lossy(&validate.stderr)
    );
    assert!(String::from_utf8_lossy(&validate.stdout).starts_with("valid: new-feature"));

    let dry = run(mx().env("MX_RUST_SOURCE_ROOT", source).args([
        "authority",
        "mx-workflow.sh",
        "dry-run",
        "new-feature",
        "--input",
        "Add a setting",
    ]));
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let text = String::from_utf8_lossy(&dry.stdout);
    assert!(text.contains("workflow: new-feature"));
    assert!(text.contains("input: Add a setting"));
    assert!(text.contains("type=interactive | gate=approve"));
}

#[test]
fn every_public_adapter_rejects_an_invalid_selector_before_mutation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    for entry in [
        "mx-decision-hold.sh",
        "mx-maintainer-override.sh",
        "mx-override-bindings.sh",
        "mx-override-run.sh",
        "mx-workflow.sh",
    ] {
        let output = Command::new(source_root().join("bin").join(entry))
            .env("MX_AUTHORITY_IMPLEMENTATION", "invalid")
            .env("MX_STATE_OVERRIDE", &state)
            .output()
            .expect("run adapter");
        assert_eq!(output.status.code(), Some(2), "{entry}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("MX_AUTHORITY_IMPLEMENTATION must be rust or legacy"),
            "{entry}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!state.exists(), "{entry} mutated state before selection");
    }
}

#[test]
fn decision_identity_is_native_and_rejects_traversal() {
    let valid = run(mx().args([
        "authority",
        "mx-decision-hold.sh",
        "id",
        "review-1",
        "api-choice",
    ]));
    assert!(valid.status.success());
    assert_eq!(valid.stdout, b"review-1-decision-api-choice\n");
    let invalid = run(mx().args([
        "authority",
        "mx-decision-hold.sh",
        "id",
        "../review",
        "choice",
    ]));
    assert!(!invalid.status.success());
}

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Output};

use multplx_domain::maintainer_override::{self, Binding, OverrideStore, Request};

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

#[test]
fn workflow_composition_never_dispatches_to_a_legacy_body() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = run(mx()
        .env("MX_ROOT_OVERRIDE", temp.path())
        .env("MX_RUST_SOURCE_ROOT", temp.path())
        .args(["authority", "mx-workflow.sh", "resume"]));
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("resume requires one run id"));
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

#[test]
fn native_override_cli_covers_decision_result_audit_and_handoff() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    fs::write(state.join(".lock"), format!("{}\n", std::process::id())).expect("primary lock");
    let digest = maintainer_override::sha256_text("state-v2");
    let store = OverrideStore::new(&state);
    let id = store
        .request(&Request {
            boundary: "workflow.skip-stage",
            task: "run-2",
            project: "multplx",
            operation: "skip workflow stage test in run run-2",
            target: "run-2#test",
            expected_state_digest: &digest,
            consequence: "Skip only the exact test stage.",
            ttl: 300,
        })
        .expect("request");
    let words = "Grant workflow.skip-stage for exact operation skip workflow stage test in run run-2 on exact target run-2#test.";
    let grant = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "grant",
        &id,
        "--maintainer-words",
        words,
    ]));
    assert!(
        grant.status.success(),
        "{}",
        String::from_utf8_lossy(&grant.stderr)
    );
    store
        .consume(
            &id,
            &Binding {
                boundary: "workflow.skip-stage",
                task: "run-2",
                project: "multplx",
                operation: "skip workflow stage test in run run-2",
                target: "run-2#test",
                expected_state_digest: &digest,
            },
        )
        .expect("consume");
    let result = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "result",
        &id,
        "--outcome",
        "failed",
        "--detail",
        "test command exited 7",
    ]));
    assert!(result.status.success());
    let inspect = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "inspect",
        &id,
    ]));
    assert!(inspect.status.success());
    let inspected: serde_json::Value =
        serde_json::from_slice(&inspect.stdout).expect("inspect JSON");
    assert_eq!(inspected["outcome"], "failed");

    let handoff_id = store
        .request(&Request {
            boundary: "authentication.login",
            task: "run-2",
            project: "multplx",
            operation: "authenticate gh for delivery",
            target: "github.com/KashyapTan/Multplx",
            expected_state_digest: &digest,
            consequence: "The maintainer performs only this login.",
            ttl: 300,
        })
        .expect("handoff request");
    let handoff_words = "Grant authentication.login for exact operation authenticate gh for delivery on exact target github.com/KashyapTan/Multplx.";
    store
        .decide(&handoff_id, handoff_words, true)
        .expect("handoff grant");
    store
        .consume(
            &handoff_id,
            &Binding {
                boundary: "authentication.login",
                task: "run-2",
                project: "multplx",
                operation: "authenticate gh for delivery",
                target: "github.com/KashyapTan/Multplx",
                expected_state_digest: &digest,
            },
        )
        .expect("handoff consume");
    let handoff = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "handoff",
        &handoff_id,
    ]));
    assert!(handoff.status.success());
    assert!(String::from_utf8_lossy(&handoff.stdout).contains("authentication.login"));

    let audit = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "audit",
    ]));
    assert!(audit.status.success());
    assert!(String::from_utf8_lossy(&audit.stdout).contains(&id));
    let audit_json = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "audit",
        "--json",
    ]));
    assert!(audit_json.status.success());
    let records: serde_json::Value =
        serde_json::from_slice(&audit_json.stdout).expect("audit JSON");
    assert_eq!(records.as_array().expect("records").len(), 2);
}

#[test]
fn native_override_cli_rejects_closed_usage_and_state_failures() {
    let temp = tempfile::tempdir().expect("tempdir");
    let state = temp.path().join("state");
    fs::create_dir(&state).expect("state");
    let cases: &[&[&str]] = &[
        &["authority", "mx-maintainer-override.sh"],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "registry",
            "--bad",
        ],
        &["authority", "mx-maintainer-override.sh", "digest"],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "request",
            "literal",
        ],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "request",
            "--boundary",
        ],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "request",
            "--boundary",
            "",
        ],
        &["authority", "mx-maintainer-override.sh", "inspect"],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "inspect",
            "missing",
        ],
        &["authority", "mx-maintainer-override.sh", "audit", "--bad"],
        &["authority", "mx-maintainer-override.sh", "grant"],
        &["authority", "mx-maintainer-override.sh", "grant", "missing"],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "grant",
            "missing",
            "--maintainer-words",
            "yes",
        ],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "consume",
            "missing",
        ],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "consume",
            "missing",
            "literal",
        ],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "result",
            "missing",
        ],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "result",
            "missing",
            "literal",
        ],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "result",
            "missing",
            "--outcome",
            "maybe",
            "--detail",
            "detail",
        ],
        &[
            "authority",
            "mx-maintainer-override.sh",
            "handoff",
            "missing",
        ],
        &["authority", "mx-maintainer-override.sh", "unknown"],
    ];
    for args in cases {
        let output = run(mx().env("MX_STATE_OVERRIDE", &state).args(*args));
        assert!(!output.status.success(), "unexpected success: {args:?}");
    }

    let help = run(mx().args(["authority", "mx-maintainer-override.sh", "--help"]));
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
    let registry = run(mx().args(["authority", "mx-maintainer-override.sh", "registry"]));
    assert!(registry.status.success());
    assert!(String::from_utf8_lossy(&registry.stdout).contains("workflow.skip-stage\tpolicy"));
    let argv = run(mx().args([
        "authority",
        "mx-maintainer-override.sh",
        "argv",
        "printf",
        "two words",
    ]));
    assert_eq!(argv.stdout, b"[\"printf\",\"two words\"]\n");

    let invalid_request = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "request",
        "--boundary",
        "integrity.validation-state",
        "--task",
        "task",
        "--project",
        "multplx",
        "--operation",
        "waive truth",
        "--target",
        "target",
        "--expected-state",
        "bad-digest",
        "--consequence",
        "No.",
    ]));
    assert!(!invalid_request.status.success());

    let digest = maintainer_override::sha256_text("state");
    let extra_request = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "request",
        "--boundary",
        "workflow.skip-stage",
        "--task",
        "task",
        "--project",
        "multplx",
        "--operation",
        "skip exact stage",
        "--target",
        "run#stage",
        "--expected-state",
        &digest,
        "--consequence",
        "Skip it.",
        "--extra",
        "value",
    ]));
    assert_eq!(extra_request.status.code(), Some(2));
    let extra_consume = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "consume",
        "missing",
        "--boundary",
        "workflow.skip-stage",
        "--task",
        "task",
        "--project",
        "multplx",
        "--operation",
        "skip exact stage",
        "--target",
        "run#stage",
        "--expected-state",
        &digest,
        "--extra",
        "value",
    ]));
    assert_eq!(extra_consume.status.code(), Some(2));

    let store = OverrideStore::new(&state);
    let denied_id = store
        .request(&Request {
            boundary: "workflow.skip-stage",
            task: "task",
            project: "multplx",
            operation: "skip exact stage",
            target: "run#stage",
            expected_state_digest: &digest,
            consequence: "Skip it.",
            ttl: 300,
        })
        .expect("request");
    fs::write(state.join(".lock"), format!("{}\n", std::process::id())).expect("primary lock");
    let denied = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "deny",
        &denied_id,
        "--maintainer-words",
        "Deny this request.",
    ]));
    assert!(denied.status.success());
    let bad_handoff = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "handoff",
        &denied_id,
    ]));
    assert!(!bad_handoff.status.success());

    let malformed = store.root().join("pending/malformed.json");
    fs::write(&malformed, b"not json\n").expect("malformed");
    fs::set_permissions(&malformed, fs::Permissions::from_mode(0o600)).expect("mode");
    let invalid_audit = run(mx().env("MX_STATE_OVERRIDE", &state).args([
        "authority",
        "mx-maintainer-override.sh",
        "audit",
    ]));
    assert_eq!(invalid_audit.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid_audit.stdout).contains("invalid\t"));

    let unknown_entry = run(mx().args(["authority", "not-an-entry"]));
    assert_eq!(unknown_entry.status.code(), Some(2));

    let missing_compat = run(mx().env("MX_RUST_SOURCE_ROOT", temp.path()).args([
        "authority",
        "mx-decision-hold.sh",
        "id",
    ]));
    assert_eq!(missing_compat.status.code(), Some(1));
    let invalid_key =
        run(mx().args(["authority", "mx-decision-hold.sh", "id", "review", "../key"]));
    assert_eq!(invalid_key.status.code(), Some(1));
}

#[test]
fn native_workflow_cli_rejects_missing_invalid_and_extra_arguments() {
    let temp = tempfile::tempdir().expect("tempdir");
    let invalid = temp.path().join("invalid.workflow.md");
    fs::write(&invalid, "not a workflow\n").expect("invalid definition");
    let invalid_path = invalid.to_str().expect("path");
    for args in [
        vec!["authority", "mx-workflow.sh", "validate", invalid_path],
        vec![
            "authority",
            "mx-workflow.sh",
            "validate",
            "missing.workflow.md",
        ],
        vec!["authority", "mx-workflow.sh", "dry-run", "missing"],
    ] {
        let output = run(mx().env("MX_RUST_SOURCE_ROOT", source_root()).args(args));
        assert!(!output.status.success());
    }

    let missing = run(mx().env("MX_RUST_SOURCE_ROOT", temp.path()).args([
        "authority",
        "mx-workflow.sh",
        "dry-run",
    ]));
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("dry-run requires a definition"));
}

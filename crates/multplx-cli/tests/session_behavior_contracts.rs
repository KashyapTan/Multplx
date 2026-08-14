use std::path::Path;
use std::process::Command;

fn run_behavior_contract(script: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("bash")
        .arg(root.join(script))
        .current_dir(&root)
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .env("MX_RUST_SOURCE_ROOT", &root)
        .output()
        .unwrap_or_else(|error| panic!("run {script}: {error}"));
    assert!(
        output.status.success(),
        "{script} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_session_behavior_contracts_run_through_the_instrumented_binary() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_none() {
        return;
    }
    for script in [
        "tests/mx-backlog-lib.test.sh",
        "tests/mx-pending-reply.test.sh",
        "tests/mx-shared-maintainer-inheritance.test.sh",
        "tests/mx-workflow-lib.test.sh",
        "tests/mx-daemon.test.sh",
        "tests/mx-report.test.sh",
        "tests/mx-actor-state.test.sh",
        "tests/mx-backend.test.sh",
        "tests/mx-headroom.test.sh",
        "tests/mx-operational-input.test.sh",
        "tests/mx-backlog-handoff.test.sh",
        "tests/mx-send-strict.test.sh",
        "tests/mx-doctor.test.sh",
        "tests/mx-session-start-lock-bootstrap.test.sh",
        "tests/mx-session-start-digest-render.test.sh",
        "tests/mx-session-start-process-liveness.test.sh",
        "tests/mx-status-snapshot-catchup-forge.test.sh",
        "tests/mx-status-snapshot-landed-bounds.test.sh",
        "tests/mx-status-snapshot-projection-reconciliation.test.sh",
        "tests/mx-system-snapshot-view.test.sh",
        "tests/mx-spawn-batch.test.sh",
        "tests/mx-spawn-dispatch-profile.test.sh",
        "tests/mx-spawn-worktree-settle.test.sh",
        "tests/mx-teardown.test.sh",
        "tests/mx-brief.test.sh",
        "tests/mx-ensure-agents-md.test.sh",
        "tests/mx-upstream-diff.test.sh",
        "tests/mx-system-sync.test.sh",
        "tests/mx-update.test.sh",
        "tests/mx-gate-refuse.test.sh",
        "tests/mx-tangle-guard.test.sh",
        "tests/mx-transition-lib.test.sh",
        "tests/mx-wake-queue.test.sh",
        "tests/mx-timeline.test.sh",
        "tests/mx-pr-check-security-retirement-teardown.test.sh",
        "tests/mx-pr-check-security-parser-entrypoints.test.sh",
        "tests/mx-daemon-harness-model-resolution.test.sh",
        "tests/mx-daemon-harness-reread-retry.test.sh",
        "tests/mx-daemon-harness-spawn-config.test.sh",
        "tests/mx-daemon-liveness.test.sh",
        "tests/mx-daemon-sync.test.sh",
        "tests/mx-gotmp.test.sh",
        "tests/mx-naming.test.sh",
        "tests/mx-pr-check-security-fault-quarantine.test.sh",
        "tests/mx-pr-check-security-publication-migration.test.sh",
        "tests/mx-sessionstart-nudge.test.sh",
        "tests/mx-supervise-daemon-native.test.sh",
        "tests/mx-backend-cmux.test.sh",
        "tests/mx-backend-herdr.test.sh",
        "tests/mx-install-herdr.test.sh",
        "tests/mx-herdr-lab.test.sh",
        "tests/mx-tmux-submit-busy.test.sh",
        "tests/mx-viz.test.sh",
        "tests/mx-vplan.test.sh",
        "tests/mx-maintainer-override.test.sh",
        "tests/mx-lock-override.test.sh",
        "tests/mx-push-service.test.sh",
        "tests/mx-ask-user-authority.test.sh",
        "tests/mx-decision-hold-lifecycle.test.sh",
        "tests/mx-deep-review-lib.test.sh",
        "tests/mx-deep-review-config-contract.test.sh",
        "tests/mx-pr-merge.test.sh",
        "tests/mx-review-diff.test.sh",
        "tests/mx-removed-deps.test.sh",
        "tests/mx-workflow.test.sh",
        "tests/mx-documentation-audiences.test.sh",
    ] {
        run_behavior_contract(script);
    }
}

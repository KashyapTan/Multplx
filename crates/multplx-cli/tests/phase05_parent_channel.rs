use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use multplx_domain::lifecycle::subagent_model::{
    Acknowledgement, ArtifactKind, AssignmentRole, MessageEnvelope, TaskRecord, write_meta,
};

fn run(command: &mut Command) -> Output {
    command.output().expect("run mx")
}

fn mx(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
    command
        .env("MX_HOME", home)
        .env("MX_ROOT_OVERRIDE", home)
        .env("MX_STATE_OVERRIDE", home.join("state"))
        .env("MX_REPORT_STATE_OVERRIDE", home.join("state"))
        .env("MX_NUDGE", "0");
    command
}

fn canonical_task(home: &Path, id: &str) -> TaskRecord {
    fs::create_dir_all(home.join("state")).expect("state");
    let root = format!("root-home:{}", home.display());
    let mut record = TaskRecord::new(
        id.into(),
        AssignmentRole::Implementer,
        ArtifactKind::Implementation,
        false,
        root.clone(),
        root,
        home.to_string_lossy().into_owned(),
    );
    record.owner_state = Some(home.join("state").to_string_lossy().into_owned());
    record.parent_state = record.owner_state.clone();
    let text = write_meta("kind=delivery\n", &record).expect("canonical metadata");
    fs::write(home.join("state").join(format!("{id}.meta")), text).expect("metadata");
    record
}

fn write_task(home: &Path, record: &TaskRecord) {
    fs::create_dir_all(home.join("state")).expect("state");
    fs::write(
        home.join("state").join(format!("{}.meta", record.task_id)),
        write_meta("", record).expect("canonical metadata"),
    )
    .expect("metadata");
}

fn watcher_checkpoint(home: &Path) -> Output {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    run(mx(home)
        .env("MX_RUST_SOURCE_ROOT", source_root)
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .env("MX_POLL", "0.05")
        .env("MX_CHECK_INTERVAL", "999999")
        .env("MX_HEARTBEAT", "999999")
        .args(["supervision", "mx-watch-checkpoint.sh", "--seconds", "1"]))
}

fn watcher_checkpoint_until(home: &Path, description: &str, progressed: impl Fn() -> bool) {
    const MAX_CHECKPOINTS: usize = 5;
    let mut last = None;
    for _ in 0..MAX_CHECKPOINTS {
        let output = watcher_checkpoint(home);
        assert!(
            matches!(output.status.code(), Some(0 | 124)),
            "watcher checkpoint status={:?}\nstdout={}\nstderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        last = Some(output);
        if progressed() {
            return;
        }
    }
    let output = last.expect("at least one bounded watcher checkpoint ran");
    panic!(
        "watcher did not {description} after {MAX_CHECKPOINTS} bounded checkpoints\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn canonical_terminal_report_freezes_parent_outcome_in_same_transition_evidence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let record = canonical_task(&home, "worker");
    let attempt = record.attempt.as_ref().expect("attempt");
    let result = run(mx(&home)
        .env("MX_TASK_ID", "worker")
        .env("MX_ATTEMPT_ID", &attempt.id)
        .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
        .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string())
        .args([
            "supervision",
            "mx-report",
            "--id",
            "worker",
            "--state",
            "done",
            "--message",
            "implementation ready",
            "--message-id",
            "phase05-report",
            "--correlation-id",
            "phase05-correlation",
        ]));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let evidence: serde_json::Value = serde_json::from_slice(
        &fs::read(home.join("state/evidence/worker-phase05-report.json")).expect("evidence"),
    )
    .expect("evidence JSON");
    assert_eq!(evidence["accepted"], true);
    assert_eq!(
        evidence["parent_outcome"]["event"]["message_id"],
        "phase05-report"
    );
    assert_eq!(evidence["parent_outcome"]["route"]["attempt_generation"], 1);
    assert!(
        home.join("state/parent-outbox/phase05-report.json")
            .is_file()
    );
    for _ in 0..2 {
        let relay = run(mx(&home).args(["parent-channel", "relay", "--limit", "8"]));
        assert!(
            relay.status.success(),
            "{}",
            String::from_utf8_lossy(&relay.stderr)
        );
    }
    let inspect = run(mx(&home).args(["parent-channel", "inspect"]));
    let health: serde_json::Value =
        serde_json::from_slice(&inspect.stdout).expect("parent channel health");
    assert_eq!(health["pending_inbox"], 0);
    assert_eq!(health["pending_outbox"], 0);
}

#[test]
fn parent_channel_cli_enforces_help_relay_bounds_and_answer_grammar() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    canonical_task(&home, "grammar-worker");

    let help = run(mx(&home).args(["parent-channel", "--help"]));
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.contains("mx parent-channel inspect"));
    assert!(help_text.contains("relay [--limit <count>]"));
    assert!(help_text.contains("--workflow-revision <id>"));

    for limit in ["1", "1024"] {
        let accepted = run(mx(&home).args(["parent-channel", "relay", "--limit", limit]));
        assert!(
            accepted.status.success(),
            "limit {limit}: {}",
            String::from_utf8_lossy(&accepted.stderr)
        );
    }
    for limit in ["0", "1025", "many"] {
        let rejected = run(mx(&home).args(["parent-channel", "relay", "--limit", limit]));
        assert_eq!(rejected.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&rejected.stderr)
                .contains("--limit must be an integer from 1 through 1024")
        );
    }

    let extra_inspect = run(mx(&home).args(["parent-channel", "inspect", "unexpected"]));
    assert_eq!(extra_inspect.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&extra_inspect.stderr).contains("Usage:"));

    let missing_answer = run(mx(&home).args([
        "parent-channel",
        "answer",
        "--task",
        "grammar-worker",
        "--question",
        "choice",
    ]));
    assert_eq!(missing_answer.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_answer.stderr).contains("Usage:"));

    let invalid_revision = run(mx(&home).args([
        "parent-channel",
        "answer",
        "--task",
        "grammar-worker",
        "--question",
        "choice",
        "--brief-revision",
        "not-a-number",
        "--answer-id",
        "answer-one",
        "--answer",
        "Use A",
    ]));
    assert_eq!(invalid_revision.status.code(), Some(2));

    let unknown_answer = run(mx(&home).args([
        "parent-channel",
        "answer",
        "--task",
        "grammar-worker",
        "--question",
        "choice",
        "--brief-revision",
        "1",
        "--answer-id",
        "answer-one",
        "--answer",
        "Use A",
        "--destination",
        "foreign",
    ]));
    assert_eq!(unknown_answer.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&unknown_answer.stderr)
            .contains("unknown parent-channel answer option '--destination'")
    );
}

#[test]
fn report_cli_covers_explicit_identity_question_and_retry_conflicts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let record = canonical_task(&home, "report-worker");
    let attempt = record.attempt.as_ref().expect("attempt");
    let bound = |command: &mut Command| {
        command
            .env("MX_TASK_ID", "report-worker")
            .env("MX_ATTEMPT_ID", &attempt.id)
            .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
            .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string());
    };

    let mut explicit = mx(&home);
    bound(&mut explicit);
    let accepted = run(explicit.args([
        "supervision",
        "mx-report",
        "--id",
        "report-worker",
        "--state",
        "blocked",
        "--message",
        "explicit identity report",
        "--attempt-id",
        &attempt.id,
        "--generation",
        "1",
        "--brief-revision",
        "1",
        "--message-id",
        "explicit-report",
        "--correlation-id",
        "explicit-correlation",
        "--artifact",
        "artifacts/result.json",
    ]));
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let evidence: serde_json::Value = serde_json::from_slice(
        &fs::read(home.join("state/evidence/report-worker-explicit-report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(evidence["envelope"]["artifact"], "artifacts/result.json");
    assert_eq!(
        evidence["envelope"]["correlation_id"],
        "explicit-correlation"
    );

    let mut conflict = mx(&home);
    bound(&mut conflict);
    let conflict = run(conflict.args([
        "supervision",
        "mx-report",
        "--id",
        "report-worker",
        "--state",
        "blocked",
        "--message",
        "changed retry payload",
        "--message-id",
        "explicit-report",
    ]));
    assert_eq!(conflict.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&conflict.stderr)
            .contains("message identity reused with different evidence")
    );

    for args in [
        vec![
            "--id",
            "report-worker",
            "--state",
            "needs-decision",
            "--message",
            "Choose one",
        ],
        vec![
            "--id",
            "report-worker",
            "--state",
            "working",
            "--message",
            "progress",
            "--workflow-revision",
            "flow-1",
        ],
        vec![
            "--id",
            "report-worker",
            "--state",
            "needs-decision",
            "--message",
            "Choose one",
            "--key",
            "bad/key",
        ],
        vec![
            "--id",
            "report-worker",
            "--state",
            "done",
            "--message",
            "done",
            "--message-id",
            "bad/id",
        ],
    ] {
        let mut command = mx(&home);
        bound(&mut command);
        let mut full = vec!["supervision", "mx-report"];
        full.extend(args);
        let rejected = run(command.args(full));
        assert_eq!(
            rejected.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&rejected.stderr)
        );
    }

    let mut mismatch = mx(&home);
    bound(&mut mismatch);
    let mismatch = run(mismatch.args([
        "supervision",
        "mx-report",
        "--id",
        "another-task",
        "--state",
        "done",
        "--message",
        "wrong binding",
    ]));
    assert_eq!(mismatch.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("task binding mismatch"));
}

#[test]
fn report_retry_faults_fail_closed_with_retained_evidence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let record = canonical_task(&home, "fault-worker");
    let attempt = record.attempt.as_ref().expect("attempt");
    let report = |message_id: &str| {
        let mut command = mx(&home);
        command
            .env("MX_TASK_ID", "fault-worker")
            .env("MX_ATTEMPT_ID", &attempt.id)
            .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
            .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string())
            .args([
                "supervision",
                "mx-report",
                "--id",
                "fault-worker",
                "--state",
                "done",
                "--message",
                "retained result",
                "--message-id",
                message_id,
            ]);
        run(&mut command)
    };

    assert!(report("bad-evidence").status.success());
    fs::write(
        home.join("state/evidence/fault-worker-bad-evidence.json"),
        b"not json",
    )
    .unwrap();
    let retry = report("bad-evidence");
    assert_eq!(retry.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&retry.stderr).contains("corrupt retained evidence"));

    assert!(report("bad-parent-outcome").status.success());
    let evidence_path = home.join("state/evidence/fault-worker-bad-parent-outcome.json");
    let mut evidence: serde_json::Value =
        serde_json::from_slice(&fs::read(&evidence_path).unwrap()).unwrap();
    evidence["parent_outcome"] = serde_json::json!({"invalid": true});
    fs::write(&evidence_path, serde_json::to_vec(&evidence).unwrap()).unwrap();
    let retry = report("bad-parent-outcome");
    assert_eq!(retry.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&retry.stderr).contains("corrupt retained parent outcome"));

    assert!(report("bad-operation").status.success());
    fs::write(
        home.join("state/.transitions/report-fault-worker-bad-operation.json"),
        b"not json",
    )
    .unwrap();
    let retry = report("bad-operation");
    assert_eq!(retry.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&retry.stderr).contains("corrupt report operation"));
}

#[test]
fn needs_decision_report_records_revision_bound_question_and_answer() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let record = canonical_task(&home, "decision-worker");
    let attempt = record.attempt.as_ref().expect("attempt");
    let reported = run(mx(&home)
        .env("MX_TASK_ID", "decision-worker")
        .env("MX_ATTEMPT_ID", &attempt.id)
        .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
        .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string())
        .args([
            "supervision",
            "mx-report",
            "--id",
            "decision-worker",
            "--state",
            "needs-decision",
            "--message",
            "Which API should this use?",
            "--key",
            "api-choice",
            "--workflow-revision",
            "flow-7",
            "--message-id",
            "decision-report",
        ]));
    assert!(
        reported.status.success(),
        "{}",
        String::from_utf8_lossy(&reported.stderr)
    );
    let text = fs::read_to_string(home.join("state/decision-worker.meta")).expect("metadata");
    let waiting = multplx_domain::lifecycle::subagent_model::read_meta("decision-worker", &text)
        .expect("canonical task");
    assert_eq!(waiting.schedule.decisions.len(), 1);
    assert_eq!(waiting.schedule.decisions[0].id, "api-choice");
    assert_eq!(
        waiting.schedule.decisions[0].workflow_revision.as_deref(),
        Some("flow-7")
    );

    let answer = |answer_id: &str, text: &str| {
        run(mx(&home).args([
            "parent-channel",
            "answer",
            "--task",
            "decision-worker",
            "--question",
            "api-choice",
            "--brief-revision",
            "1",
            "--workflow-revision",
            "flow-7",
            "--answer-id",
            answer_id,
            "--answer",
            text,
        ]))
    };
    let answered = answer("answer-one", "Use API A");
    assert!(
        answered.status.success(),
        "{}",
        String::from_utf8_lossy(&answered.stderr)
    );
    let answer_record: serde_json::Value =
        serde_json::from_slice(&answered.stdout).expect("answer result");
    assert_eq!(answer_record["disposition"], "accepted");
    let text = fs::read_to_string(home.join("state/decision-worker.meta")).expect("metadata");
    let updated = multplx_domain::lifecycle::subagent_model::read_meta("decision-worker", &text)
        .expect("canonical task");
    assert_eq!(
        updated.schedule.decisions[0].answer.as_deref(),
        Some("Use API A")
    );
    assert!(
        fs::read_to_string(home.join("state/.wake-queue"))
            .expect("answer wake")
            .contains("human-answer-answer-one")
    );

    let accepted_path = home.join("state/human-answers/decision-worker/answer-one.json");
    let accepted_bytes = fs::read(&accepted_path).expect("accepted answer record");
    let wake_before_retry = fs::read(home.join("state/.wake-queue")).expect("wake queue");
    let receipt = home.join("state/.transitions/human-answer-decision-worker-answer-one.json");
    let mut unfinished: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt).expect("answer transition")).unwrap();
    unfinished["committed"] = false.into();
    fs::write(&receipt, serde_json::to_vec(&unfinished).unwrap()).unwrap();

    let retried = answer("answer-one", "Use API A");
    assert!(
        retried.status.success(),
        "{}",
        String::from_utf8_lossy(&retried.stderr)
    );
    let retried_record: serde_json::Value =
        serde_json::from_slice(&retried.stdout).expect("retried answer");
    assert_eq!(retried_record["disposition"], "accepted");
    let recovered_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(recovered_receipt["committed"], true);
    assert_eq!(fs::read(&accepted_path).unwrap(), accepted_bytes);
    assert_eq!(
        fs::read(home.join("state/.wake-queue")).unwrap(),
        wake_before_retry,
        "an exact retry must not duplicate the actionable wake"
    );

    let conflicting_retry = answer("answer-one", "Use API B");
    assert_eq!(conflicting_retry.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&conflicting_retry.stderr)
            .contains("human answer identity conflicts with retained history")
    );
    assert_eq!(fs::read(&accepted_path).unwrap(), accepted_bytes);

    let historical = answer("answer-two", "Use API B");
    assert!(
        historical.status.success(),
        "{}",
        String::from_utf8_lossy(&historical.stderr)
    );
    let historical_record: serde_json::Value =
        serde_json::from_slice(&historical.stdout).expect("historical answer");
    assert_eq!(historical_record["disposition"], "historical");
    assert_eq!(
        historical_record["reason"],
        "question already has a different answer"
    );
    let historical_path = home.join("state/human-answers/decision-worker/answer-two.json");
    let historical_bytes = fs::read(&historical_path).expect("historical answer record");
    let historical_retry = answer("answer-two", "Use API B");
    assert!(historical_retry.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&historical_retry.stdout).unwrap(),
        historical_record
    );
    let historical_conflict = answer("answer-two", "Use API C");
    assert_eq!(historical_conflict.status.code(), Some(1));
    assert_eq!(fs::read(&historical_path).unwrap(), historical_bytes);

    fs::write(&historical_path, b"{corrupt-answer").unwrap();
    let corrupt_retry = answer("answer-two", "Use API B");
    assert_eq!(corrupt_retry.status.code(), Some(1));
    fs::remove_file(&receipt).unwrap();
    let missing_receipt = answer("answer-one", "Use API A");
    assert_eq!(missing_receipt.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&missing_receipt.stderr)
            .contains("accepted human answer is missing its canonical transition receipt")
    );
    assert_eq!(fs::read(&accepted_path).unwrap(), accepted_bytes);
    let preserved = multplx_domain::lifecycle::subagent_model::read_meta(
        "decision-worker",
        &fs::read_to_string(home.join("state/decision-worker.meta")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        preserved.schedule.decisions[0].answer.as_deref(),
        Some("Use API A")
    );
}

#[test]
fn canonical_daemon_report_derives_owner_and_rejects_arbitrary_status_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let record = canonical_task(&home, "worker");
    let attempt = record.attempt.as_ref().expect("attempt");
    let bound = |command: &mut Command| {
        command
            .env("MX_TASK_ID", "worker")
            .env("MX_ATTEMPT_ID", &attempt.id)
            .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
            .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string());
    };
    let help = run(mx(&home).args(["daemon-report", "--help"]));
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Report a correlated result"));

    let mut short = mx(&home);
    bound(&mut short);
    let short = run(short.args(["daemon-report", "done"]));
    assert_eq!(short.status.code(), Some(2));

    let mut bad_correlation = mx(&home);
    bound(&mut bad_correlation);
    let bad_correlation =
        run(bad_correlation.args(["daemon-report", "done", "too-short", "invalid correlation"]));
    assert_eq!(bad_correlation.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&bad_correlation.stderr)
            .contains("corr_id must be 16 hex characters")
    );
    let mut wrong = mx(&home);
    bound(&mut wrong);
    let wrong_status = temp.path().join("foreign/state/worker.status");
    fs::create_dir_all(wrong_status.parent().expect("foreign parent")).expect("foreign state");
    let rejected = run(wrong.args([
        "daemon-report",
        wrong_status.to_str().expect("path"),
        "done",
        "0123456789abcdef",
        "foreign",
    ]));
    assert!(!rejected.status.success());
    assert!(!wrong_status.exists());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("validated task owner"));

    let mut preferred = mx(&home);
    bound(&mut preferred);
    let accepted =
        run(preferred.args(["daemon-report", "done", "0123456789abcdef", "local outcome"]));
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert!(
        fs::read_to_string(home.join("state/worker.status"))
            .expect("status")
            .contains("done: [corr=0123456789abcdef] local outcome (via-helper)")
    );

    let mut documented = mx(&home);
    bound(&mut documented);
    let documented = run(documented.args([
        "daemon-report",
        "--doc",
        "done",
        "fedcba9876543210",
        "artifacts/report.md",
        "documented outcome",
    ]));
    assert!(
        documented.status.success(),
        "{}",
        String::from_utf8_lossy(&documented.stderr)
    );
    let evidence = fs::read_dir(home.join("state/evidence"))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .find(|value| value["envelope"]["correlation_id"] == "fedcba9876543210")
        .expect("document report evidence");
    assert_eq!(evidence["envelope"]["artifact"], "artifacts/report.md");

    let mut decision = mx(&home);
    bound(&mut decision);
    let decision = run(decision.args([
        "daemon-report",
        "needs-decision",
        "1111111111111111",
        "Choose the compatibility behavior",
    ]));
    assert!(
        decision.status.success(),
        "{}",
        String::from_utf8_lossy(&decision.stderr)
    );
    let updated = multplx_domain::lifecycle::subagent_model::read_meta(
        "worker",
        &fs::read_to_string(home.join("state/worker.meta")).unwrap(),
    )
    .unwrap();
    assert!(
        updated
            .schedule
            .decisions
            .iter()
            .any(|item| item.id == "1111111111111111")
    );

    let mut missing_report_state = mx(&home);
    bound(&mut missing_report_state);
    let missing_report_state = run(missing_report_state
        .env_remove("MX_REPORT_STATE_OVERRIDE")
        .args(["daemon-report", "done", "2222222222222222", "missing route"]));
    assert_eq!(missing_report_state.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&missing_report_state.stderr)
            .contains("canonical parent report requires its bound report-state directory")
    );

    let legacy_status = temp.path().join("legacy/legacy.status");
    let legacy = run(mx(&home).env_remove("MX_TASK_ID").args([
        "daemon-report",
        legacy_status.to_str().unwrap(),
        "done",
        "3333333333333333",
        "legacy result",
    ]));
    assert!(legacy.status.success());
    assert!(
        fs::read_to_string(&legacy_status)
            .unwrap()
            .contains("legacy result")
    );

    fs::write(home.join("state/worker.meta"), b"schema_version=invalid\n").unwrap();
    let mut invalid_identity = mx(&home);
    bound(&mut invalid_identity);
    let invalid_identity = run(invalid_identity.args([
        "daemon-report",
        "done",
        "4444444444444444",
        "invalid identity",
    ]));
    assert_eq!(invalid_identity.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&invalid_identity.stderr).contains("invalid bound report identity")
    );
}

#[test]
fn idle_watcher_checkpoints_relay_a_nested_outcome_without_model_turns() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let coordinator_home = temp.path().join("coordinator");
    let worker_home = temp.path().join("worker");
    for home in [&root, &coordinator_home, &worker_home] {
        for directory in ["state", "data", "config"] {
            fs::create_dir_all(home.join(directory)).unwrap();
        }
    }
    let root_id = format!("root-home:{}", root.display());
    let mut coordinator = TaskRecord::new(
        "coordinator".into(),
        AssignmentRole::SubOrchestrator,
        ArtifactKind::Coordination,
        true,
        root_id.clone(),
        root_id.clone(),
        coordinator_home.to_string_lossy().into_owned(),
    );
    coordinator.owner_state = Some(
        coordinator_home
            .join("state")
            .to_string_lossy()
            .into_owned(),
    );
    coordinator.parent_home = Some(root.to_string_lossy().into_owned());
    coordinator.parent_state = Some(root.join("state").to_string_lossy().into_owned());
    write_task(&coordinator_home, &coordinator);

    let mut worker = TaskRecord::new(
        "worker".into(),
        AssignmentRole::Implementer,
        ArtifactKind::Implementation,
        false,
        "coordinator".into(),
        root_id,
        worker_home.to_string_lossy().into_owned(),
    );
    worker.owner_state = Some(worker_home.join("state").to_string_lossy().into_owned());
    worker.parent_home = Some(coordinator_home.to_string_lossy().into_owned());
    worker.parent_state = Some(
        coordinator_home
            .join("state")
            .to_string_lossy()
            .into_owned(),
    );
    write_task(&worker_home, &worker);
    let event = MessageEnvelope {
        schema_version: multplx_domain::lifecycle::subagent_model::SCHEMA_VERSION,
        message_id: "watcher-relay".into(),
        task_id: worker.task_id.clone(),
        task_home: worker.owner_home.clone(),
        parent_home: worker.parent_home.clone(),
        attempt: worker.attempt.clone(),
        parent_id: worker.parent_id.clone(),
        sender: worker.task_id.clone(),
        recipient: "coordinator".into(),
        brief_revision: worker
            .attempt
            .as_ref()
            .map(|attempt| attempt.brief_revision),
        kind: "failed".into(),
        correlation_id: "watcher-relay-correlation".into(),
        created_at: "2026-09-15T00:00:00Z".into(),
        summary: "critical nested outcome".into(),
        artifact: Some("failure.txt".into()),
        acknowledgement: Acknowledgement::Pending,
    };
    multplx_domain::lifecycle::parent_channel::record_outcome(&worker_home.join("state"), &event)
        .unwrap();

    let first_hop_file = "watcher-relay-hop-0-repair-0.json";
    let second_hop_file = "watcher-relay-hop-1-repair-0.json";
    let message_file = "watcher-relay.json";
    watcher_checkpoint_until(
        &worker_home,
        "deliver the worker outcome to its coordinator",
        || {
            coordinator_home
                .join("state/parent-inbox")
                .join(first_hop_file)
                .is_file()
        },
    );
    watcher_checkpoint_until(&coordinator_home, "accept the worker outcome", || {
        coordinator_home
            .join("state/parent-receipts")
            .join(format!("parent-inbox-{first_hop_file}"))
            .is_file()
    });
    watcher_checkpoint_until(&coordinator_home, "forward the outcome to the root", || {
        root.join("state/parent-inbox")
            .join(second_hop_file)
            .is_file()
    });
    watcher_checkpoint_until(&root, "publish the root's exact message receipt", || {
        root.join("state/message-outbox")
            .join(message_file)
            .is_file()
    });
    assert_eq!(
        multplx_domain::operational_input::read_message_envelope(
            &root.join("state"),
            "watcher-relay"
        )
        .unwrap(),
        event
    );
}

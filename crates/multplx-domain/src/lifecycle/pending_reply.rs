//! Parent-owned daemon reply expectations.

use std::env;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::operational_input::FROM_BROKER_MARK;

pub const SCHEMA: &str = "mx-pending-reply.v1";

fn now() -> u64 {
    env::var("MX_PENDING_REPLY_NOW")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
}

fn grace() -> u64 {
    env::var("MX_PENDING_REPLY_GRACE_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(120)
}

#[must_use]
pub fn directory(state: &Path) -> PathBuf {
    env::var_os("MX_PENDING_REPLY_DIR_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| state.join("pending-replies"))
}

#[must_use]
pub fn path(state: &Path, correlation: &str) -> PathBuf {
    directory(state).join(correlation)
}

fn new_id() -> String {
    let mut random = [0_u8; 8];
    if fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .is_ok()
    {
        return random.iter().map(|byte| format!("{byte:02x}")).collect();
    }
    fallback_id()
}

fn fallback_id() -> String {
    let mut hash = Sha256::new();
    hash.update(std::process::id().to_le_bytes());
    hash.update(now().to_le_bytes());
    hash.update(format!("{:?}", std::thread::current().id()).as_bytes());
    format!("{:x}", hash.finalize())[..16].to_owned()
}

#[must_use]
pub fn extract_correlation(text: &str) -> Option<String> {
    Regex::new(r"corr=([A-Fa-f0-9]{16})")
        .expect("static correlation regex")
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_ascii_lowercase())
}

fn summarize(text: &str) -> String {
    let text = text.strip_prefix(FROM_BROKER_MARK).unwrap_or(text);
    let mut cleaned: String = text
        .chars()
        .map(|ch| {
            if matches!(ch, '\t' | '\r' | '\n') {
                ' '
            } else {
                ch
            }
        })
        .filter(|ch| ch.is_ascii() && (!ch.is_ascii_control() || *ch == '\t'))
        .collect();
    cleaned = cleaned.trim().to_owned();
    cleaned = Regex::new(r"^corr=[A-Fa-f0-9]{16}[ \t]*")
        .expect("static prefix regex")
        .replace(&cleaned, "")
        .into_owned();
    if cleaned.len() > 120 {
        cleaned.truncate(117);
        cleaned.push_str("...");
    }
    cleaned
}

fn record_get(record: &Path, key: &str) -> String {
    fs::read_to_string(record)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix(&format!("{key}=")))
        .next_back()
        .unwrap_or_default()
        .to_owned()
}

fn record_set(record: &Path, key: &str, value: &str) -> Result<(), String> {
    let text = fs::read_to_string(record).map_err(|error| error.to_string())?;
    let mut output = String::new();
    for line in text.lines() {
        if !line.starts_with(&format!("{key}=")) {
            output.push_str(line);
            output.push('\n');
        }
    }
    output.push_str(key);
    output.push('=');
    output.push_str(value);
    output.push('\n');
    multplx_core::filesystem::atomic_replace(record, output.as_bytes(), 0o600)
        .map_err(|error| error.to_string())
}

#[must_use]
pub fn reusable(state: &Path, correlation: &str, task_id: &str) -> bool {
    if !Regex::new(r"^[A-Fa-f0-9]{16}$")
        .expect("static id regex")
        .is_match(correlation)
    {
        return false;
    }
    let record = path(state, correlation);
    record_get(&record, "task_id") == task_id
        && matches!(
            record_get(&record, "phase").as_str(),
            "awaiting_report" | "recovery_sending" | "recovery_sent"
        )
}

#[must_use]
pub fn embed(message: &str, correlation: &str) -> String {
    let marked = if message.starts_with(FROM_BROKER_MARK) {
        message.to_owned()
    } else {
        format!("{FROM_BROKER_MARK}{message}")
    };
    let body = marked.strip_prefix(FROM_BROKER_MARK).unwrap_or(&marked);
    let prefix = Regex::new(r"^corr=[A-Fa-f0-9]{16}[ \t]*").expect("static prefix regex");
    let body = prefix.replace(body, "");
    format!("{FROM_BROKER_MARK}corr={correlation} {body}")
}

pub fn create(
    parent_home: &Path,
    state: &Path,
    task_id: &str,
    request: &str,
) -> Result<String, String> {
    let dir = directory(state);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let mut correlation = new_id();
    if path(state, &correlation).exists() {
        correlation = new_id();
    }
    let record = path(state, &correlation);
    if record.exists() {
        return Err("correlation collision".to_owned());
    }
    let parent_home = fs::canonicalize(parent_home).unwrap_or_else(|_| parent_home.to_owned());
    let status = if state.is_absolute() {
        state.join(format!("{task_id}.status"))
    } else {
        fs::canonicalize(state)
            .unwrap_or_else(|_| state.to_owned())
            .join(format!("{task_id}.status"))
    };
    let text = format!(
        "schema={SCHEMA}\ncorr_id={correlation}\ntask_id={task_id}\nparent_home={}\nparent_status={}\nparent_status_scan_signature=\nrequest_summary={}\ncreated_epoch={}\ndelivered_epoch=\nphase=awaiting_report\nturn_seen_busy=0\nrequest_turn_completed_epoch=\nrecovery_attempted_epoch=\nrecovery_sender_pid=\nrecovery_sender_identity=\nrecovery_sent_epoch=\nrecovery_delivery_outcome=\nrecovery_turn_seen_busy=0\nrecovery_turn_completed_epoch=\nescalated_epoch=\nresolved_epoch=\nresolved_via=\nwrong_home_hits=0\nwrong_home_sightings=\nwrong_home_scan_signature=\ngrace_secs={}\n",
        parent_home.display(),
        status.display(),
        summarize(request),
        now(),
        grace()
    );
    multplx_core::filesystem::atomic_replace(&record, text.as_bytes(), 0o600)
        .map_err(|error| error.to_string())?;
    Ok(correlation)
}

fn confirmation(state: &Path, correlation: &str) -> PathBuf {
    directory(state).join(format!(".delivery-confirmed-{correlation}"))
}

pub fn prepare_delivery(state: &Path, correlation: &str) -> Result<(), String> {
    let record = path(state, correlation);
    if !record.is_file() {
        return Err("missing pending reply".to_owned());
    }
    if !record_get(&record, "delivered_epoch").is_empty()
        || confirmation(state, correlation).is_file()
    {
        return Ok(());
    }
    multplx_core::filesystem::atomic_replace(
        confirmation(state, correlation),
        format!("attempted={}\n", now()).as_bytes(),
        0o600,
    )
    .map_err(|error| error.to_string())
}

pub fn confirm_delivery(state: &Path, correlation: &str) -> Result<(), String> {
    prepare_delivery(state, correlation)?;
    let epoch = now().to_string();
    multplx_core::filesystem::atomic_replace(
        confirmation(state, correlation),
        format!("confirmed={epoch}\n").as_bytes(),
        0o600,
    )
    .map_err(|error| error.to_string())?;
    let record = path(state, correlation);
    if record_get(&record, "delivered_epoch").is_empty() {
        record_set(&record, "delivered_epoch", &epoch)?;
    }
    if record_get(&record, "phase") == "delivery_unknown" {
        record_set(&record, "phase", "awaiting_report")?;
    }
    let _ = fs::remove_file(confirmation(state, correlation));
    Ok(())
}

pub fn discard_undelivered(state: &Path, correlation: &str) -> Result<(), String> {
    let record = path(state, correlation);
    if !record.is_file() {
        return Ok(());
    }
    if !record_get(&record, "delivered_epoch").is_empty() {
        return Err("pending reply is already delivered".to_owned());
    }
    let _ = fs::remove_file(confirmation(state, correlation));
    fs::remove_file(record).map_err(|error| error.to_string())
}

fn line_resolves(line: &str, correlation: &str) -> bool {
    !line.contains("pending-reply-missed")
        && !line.contains("pending-reply-delivery-unknown")
        && !line.contains("pending-reply-recovery-delivery-")
        && extract_correlation(line).as_deref() == Some(correlation)
}

fn resolving_line(path: &Path, correlation: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .find(|line| line_resolves(line, correlation))
        .map(str::to_owned)
}

fn resolve_via(line: &str) -> &'static str {
    if ["data/", "report.md", "document", "pointer"]
        .iter()
        .any(|needle| line.contains(needle))
    {
        "document"
    } else if line.contains("via-helper") || line.contains("mx-daemon-report") {
        "helper"
    } else {
        "status"
    }
}

fn try_resolve(record: &Path, correlation: &str) -> Result<bool, String> {
    if record_get(record, "phase") == "resolved" {
        return Ok(true);
    }
    let delivered = record_get(record, "delivered_epoch");
    let state = record
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid pending reply path")?;
    let marker = confirmation(state, correlation);
    if delivered.is_empty() && !marker.is_file() {
        return Ok(false);
    }
    let status = PathBuf::from(record_get(record, "parent_status"));
    let Some(line) = resolving_line(&status, correlation) else {
        return Ok(false);
    };
    let epoch = now().to_string();
    if delivered.is_empty() {
        record_set(record, "delivered_epoch", &epoch)?;
        let _ = fs::remove_file(marker);
    }
    record_set(record, "phase", "resolved")?;
    record_set(record, "resolved_epoch", &epoch)?;
    record_set(record, "resolved_via", resolve_via(&line))?;
    Ok(true)
}

fn observe_turn(record: &Path, observation: &str) -> Result<(), String> {
    let phase = record_get(record, "phase");
    let (seen_key, completed_key) = match phase.as_str() {
        "awaiting_report" => ("turn_seen_busy", "request_turn_completed_epoch"),
        "recovery_sent" => ("recovery_turn_seen_busy", "recovery_turn_completed_epoch"),
        _ => return Ok(()),
    };
    match observation {
        "busy" => record_set(record, seen_key, "1"),
        "idle" if record_get(record, completed_key).is_empty() => {
            record_set(record, completed_key, &now().to_string())
        }
        "idle" | "unknown" => Ok(()),
        _ => Err("invalid pending reply observation".to_owned()),
    }
}

fn recovery_message(record: &Path, correlation: &str) -> String {
    embed(
        &format!(
            "REPOST REQUIRED: previous marked request had no correlated parent report. Reply on the parent status channel including corr={correlation}. Original request: {}",
            record_get(record, "request_summary")
        ),
        correlation,
    )
}

fn send_recovery(source_root: &Path, record: &Path, correlation: &str) -> Result<(), String> {
    if !record_get(record, "recovery_attempted_epoch").is_empty()
        || record_get(record, "request_turn_completed_epoch").is_empty()
    {
        return Ok(());
    }
    let delivered = record_get(record, "delivered_epoch")
        .parse::<u64>()
        .map_err(|_| "invalid delivered epoch")?;
    let record_grace = record_get(record, "grace_secs")
        .parse::<u64>()
        .unwrap_or_else(|_| grace());
    if now().saturating_sub(delivered) < record_grace {
        return Ok(());
    }
    let task = record_get(record, "task_id");
    let parent_home = record_get(record, "parent_home");
    let message = recovery_message(record, correlation);
    let epoch = now().to_string();
    record_set(record, "recovery_attempted_epoch", &epoch)?;
    record_set(record, "phase", "recovery_sending")?;
    let status = if let Ok(hook) = env::var("MX_PENDING_REPLY_SEND_HOOK") {
        Command::new("bash")
            .arg("-c")
            .arg(hook)
            .arg("mx-pending-reply")
            .arg(&task)
            .arg(&message)
            .status()
    } else {
        Command::new(source_root.join("bin/mx-send.sh"))
            .arg(&task)
            .arg(&message)
            .env("MX_HOME", &parent_home)
            .env("MX_PENDING_REPLY_EXISTING_CORR", correlation)
            .status()
    };
    if status.is_ok_and(|status| status.success()) {
        record_set(record, "recovery_delivery_outcome", "confirmed")?;
        record_set(record, "recovery_sent_epoch", &epoch)?;
        record_set(record, "recovery_turn_seen_busy", "0")?;
        record_set(record, "recovery_turn_completed_epoch", "")?;
        record_set(record, "phase", "recovery_sent")
    } else {
        record_set(record, "recovery_delivery_outcome", "failed")?;
        record_set(record, "phase", "recovery_failed")
    }
}

fn maybe_escalate(record: &Path, correlation: &str) -> Result<(), String> {
    let phase = record_get(record, "phase");
    let eligible = matches!(
        phase.as_str(),
        "delivery_unknown" | "recovery_failed" | "recovery_unknown"
    ) || (phase == "recovery_sent"
        && !record_get(record, "recovery_turn_completed_epoch").is_empty());
    if !eligible || try_resolve(record, correlation)? {
        return Ok(());
    }
    let task = record_get(record, "task_id");
    let summary = record_get(record, "request_summary");
    let outcome = record_get(record, "recovery_delivery_outcome");
    let payload = match phase.as_str() {
        "delivery_unknown" => format!(
            "pending-reply-delivery-unknown: task={task} pending-reply-id={correlation} request={summary}"
        ),
        "recovery_failed" | "recovery_unknown" => format!(
            "pending-reply-recovery-delivery-{outcome}: task={task} pending-reply-id={correlation} request={summary}"
        ),
        _ => format!(
            "pending-reply-missed: task={task} pending-reply-id={correlation} request={summary}"
        ),
    };
    let status = PathBuf::from(record_get(record, "parent_status"));
    let line = format!("blocked: {payload}");
    let current = fs::read_to_string(&status).unwrap_or_default();
    if !current.lines().any(|existing| existing == line) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&status)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{line}").map_err(|error| error.to_string())?;
    }
    record_set(record, "escalated_epoch", &now().to_string())?;
    record_set(record, "phase", "escalated")
}

/// Reconcile every durable parent-owned daemon reply expectation once.
pub fn tick(state: &Path, source_root: &Path, mut observe: impl FnMut(&str) -> &'static str) {
    let directory = directory(state);
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for record in entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && !path
                    .file_name()
                    .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
        })
    {
        let correlation = record_get(&record, "corr_id");
        if correlation.is_empty() || try_resolve(&record, &correlation).unwrap_or(false) {
            continue;
        }
        let task = record_get(&record, "task_id");
        let observation = observe(&task);
        let _ = observe_turn(&record, observation);
        if record_get(&record, "phase") == "awaiting_report" {
            let _ = send_recovery(source_root, &record, &correlation);
        }
        let _ = maybe_escalate(&record, &correlation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn create_embed_confirm_and_reuse_are_durable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let correlation = create(temp.path(), &state, "daemon", "audit\nnow").expect("create");
        assert_eq!(correlation.len(), 16);
        assert_eq!(
            extract_correlation(&embed("audit\n", &correlation)),
            Some(correlation.clone())
        );
        assert!(reusable(&state, &correlation, "daemon"));
        prepare_delivery(&state, &correlation).expect("prepare");
        confirm_delivery(&state, &correlation).expect("confirm");
        assert!(!record_get(&path(&state, &correlation), "delivered_epoch").is_empty());
    }

    #[test]
    fn malformed_reuse_delivery_unknown_and_discard_edges_are_explicit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("relative-state");
        fs::create_dir(&state).expect("state");
        assert_eq!(extract_correlation("no correlation"), None);
        assert_eq!(
            extract_correlation("corr=ABCDEF0123456789"),
            Some("abcdef0123456789".to_owned())
        );
        assert!(!reusable(&state, "bad", "task"));
        assert_eq!(summarize("\u{7f}  hello\tworld\n"), "hello world");
        let summary = summarize(&format!(
            "{FROM_BROKER_MARK}corr=0123456789abcdef {}",
            "x".repeat(140)
        ));
        assert_eq!(summary, format!("{}...", "x".repeat(117)));
        assert_eq!(fallback_id().len(), 16);
        assert_eq!(
            embed(
                &format!("{FROM_BROKER_MARK}corr=fedcba9876543210 old"),
                "0123456789abcdef"
            ),
            format!("{FROM_BROKER_MARK}corr=0123456789abcdef old")
        );

        let correlation = create(temp.path(), &state, "task", "request").expect("create");
        let record = path(&state, &correlation);
        record_set(&record, "phase", "delivery_unknown").expect("unknown phase");
        prepare_delivery(&state, &correlation).expect("prepare");
        prepare_delivery(&state, &correlation).expect("idempotent prepare");
        confirm_delivery(&state, &correlation).expect("confirm");
        assert_eq!(record_get(&record, "phase"), "awaiting_report");
        assert!(
            discard_undelivered(&state, &correlation)
                .unwrap_err()
                .contains("already delivered")
        );
        assert_eq!(discard_undelivered(&state, "missing"), Ok(()));

        let missing = create(temp.path(), &state, "task", "second").expect("second");
        fs::remove_file(path(&state, &missing)).expect("remove record");
        assert!(
            prepare_delivery(&state, &missing)
                .unwrap_err()
                .contains("missing pending reply")
        );
    }

    #[test]
    fn tick_resolves_correlated_status_and_escalates_failed_recovery_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        let correlation = create(temp.path(), &state, "task", "audit now").expect("create");
        confirm_delivery(&state, &correlation).expect("confirm");
        fs::write(
            state.join("task.status"),
            format!("done: corr={correlation} report.md\n"),
        )
        .expect("status");
        tick(&state, temp.path(), |_| "unknown");
        let record = path(&state, &correlation);
        assert_eq!(record_get(&record, "phase"), "resolved");
        assert_eq!(record_get(&record, "resolved_via"), "document");

        let missed = create(temp.path(), &state, "missed", "second").expect("missed");
        confirm_delivery(&state, &missed).expect("confirm missed");
        let missed_record = path(&state, &missed);
        record_set(&missed_record, "phase", "recovery_failed").expect("failed");
        record_set(&missed_record, "recovery_delivery_outcome", "failed").expect("outcome");
        tick(&state, temp.path(), |_| "unknown");
        tick(&state, temp.path(), |_| "unknown");
        assert_eq!(record_get(&missed_record, "phase"), "escalated");
        let status = fs::read_to_string(state.join("missed.status")).expect("escalation");
        assert_eq!(status.lines().count(), 1);
        assert!(status.contains("pending-reply-recovery-delivery-failed"));
    }

    #[test]
    fn recovery_delivery_covers_confirmed_failed_and_observation_edges() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&state).expect("state");
        fs::create_dir(&bin).expect("bin");
        let send = bin.join("mx-send.sh");
        let write_send = |body: &str| {
            fs::write(&send, format!("#!/bin/sh\n{body}\n")).expect("send");
            fs::set_permissions(&send, fs::Permissions::from_mode(0o755)).expect("mode");
        };

        let correlation = create(temp.path(), &state, "task", "audit now").expect("create");
        confirm_delivery(&state, &correlation).expect("confirm");
        let record = path(&state, &correlation);
        record_set(&record, "grace_secs", "0").expect("grace");
        observe_turn(&record, "busy").expect("busy");
        observe_turn(&record, "idle").expect("idle");
        assert!(
            observe_turn(&record, "invalid")
                .expect_err("invalid")
                .contains("invalid")
        );
        write_send("exit 0");
        send_recovery(temp.path(), &record, &correlation).expect("recovery");
        assert_eq!(record_get(&record, "phase"), "recovery_sent");
        assert_eq!(
            record_get(&record, "recovery_delivery_outcome"),
            "confirmed"
        );
        assert!(recovery_message(&record, &correlation).contains("REPOST REQUIRED"));

        let failed = create(temp.path(), &state, "failed", "second").expect("create");
        confirm_delivery(&state, &failed).expect("confirm");
        let failed_record = path(&state, &failed);
        record_set(&failed_record, "grace_secs", "0").expect("grace");
        record_set(&failed_record, "request_turn_completed_epoch", "1").expect("turn");
        write_send("exit 7");
        send_recovery(temp.path(), &failed_record, &failed).expect("failed recovery");
        assert_eq!(record_get(&failed_record, "phase"), "recovery_failed");
        assert_eq!(resolve_via("done via-helper"), "helper");
        assert_eq!(resolve_via("done plain"), "status");
    }

    #[test]
    fn reconciliation_covers_confirmation_grace_and_escalation_variants() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");

        let correlation = create(temp.path(), &state, "task", "request").expect("create");
        let record = path(&state, &correlation);
        assert!(!try_resolve(&record, &correlation).expect("unconfirmed"));
        prepare_delivery(&state, &correlation).expect("prepare");
        fs::write(
            state.join("task.status"),
            format!("done: corr={correlation} via-helper\n"),
        )
        .expect("status");
        assert!(try_resolve(&record, &correlation).expect("resolve"));
        assert_eq!(record_get(&record, "resolved_via"), "helper");
        assert!(try_resolve(&record, &correlation).expect("already resolved"));

        let waiting = create(temp.path(), &state, "waiting", "request").expect("create");
        let waiting_record = path(&state, &waiting);
        send_recovery(temp.path(), &waiting_record, &waiting).expect("not completed");
        assert_eq!(record_get(&waiting_record, "phase"), "awaiting_report");
        confirm_delivery(&state, &waiting).expect("confirm");
        record_set(&waiting_record, "request_turn_completed_epoch", "1").expect("turn");
        record_set(&waiting_record, "grace_secs", "9999999999").expect("grace");
        send_recovery(temp.path(), &waiting_record, &waiting).expect("inside grace");
        assert!(record_get(&waiting_record, "recovery_attempted_epoch").is_empty());

        let unknown = create(temp.path(), &state, "unknown", "third").expect("create");
        let unknown_record = path(&state, &unknown);
        record_set(&unknown_record, "phase", "delivery_unknown").expect("phase");
        maybe_escalate(&unknown_record, &unknown).expect("escalate");
        assert_eq!(record_get(&unknown_record, "phase"), "escalated");
        assert!(
            fs::read_to_string(state.join("unknown.status"))
                .expect("status")
                .contains("pending-reply-delivery-unknown")
        );

        let sent = create(temp.path(), &state, "sent", "fourth").expect("create");
        let sent_record = path(&state, &sent);
        record_set(&sent_record, "phase", "recovery_sent").expect("phase");
        record_set(&sent_record, "recovery_turn_completed_epoch", "1").expect("turn");
        maybe_escalate(&sent_record, &sent).expect("escalate");
        assert!(
            fs::read_to_string(state.join("sent.status"))
                .expect("status")
                .contains("pending-reply-missed")
        );
    }
}

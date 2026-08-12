//! Parent-owned daemon reply expectations.

use std::env;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
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
    if let Some(body) = cleaned.strip_prefix(FROM_BROKER_MARK) {
        cleaned = body.to_owned();
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

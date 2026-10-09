//! Parent-authored recovery observations; never worker completion reports.

use std::{collections::BTreeMap, fs, path::Path, time::Duration};

use multplx_core::{
    filesystem::{TransitionWrite, read_bounded_regular, recoverable_transition},
    identifiers::TaskId,
    locks::DirectoryLock,
    process::SystemProcessProbe,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::subagent_model::{
    MAX_TASK_METADATA_BYTES, WorkState, read_meta, record_state, require_writer_version, write_meta,
};

pub fn command(args: &[String], state: &Path) -> Result<String, String> {
    let id = args.get(1).ok_or("outcome requires a task id")?;
    TaskId::parse(id).map_err(|e| e.to_string())?;
    let mut options = BTreeMap::new();
    for pair in args[2..].chunks(2) {
        if pair.len() != 2
            || !matches!(
                pair[0].as_str(),
                "--outcome-id"
                    | "--attempt-id"
                    | "--generation"
                    | "--expected-revision"
                    | "--state"
                    | "--message"
                    | "--artifact"
            )
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err("invalid or repeated task outcome option".into());
        }
    }
    let get = |key| {
        options
            .get(key)
            .copied()
            .ok_or_else(|| format!("outcome requires {key}"))
    };
    let outcome_id = get("--outcome-id")?;
    TaskId::parse(outcome_id).map_err(|e| e.to_string())?;
    let disposition = get("--state")?;
    if !matches!(disposition, "blocked" | "failed" | "paused") {
        return Err("parent outcomes allow only blocked, failed or paused; completion belongs to the assigned worker".into());
    }
    let message = get("--message")?;
    if message.is_empty() || message.contains(['\n', '\r']) || message.len() > 20000 {
        return Err("outcome message must be one nonempty line of at most 20000 bytes".into());
    }
    let attempt_id = get("--attempt-id")?;
    let generation: u64 = get("--generation")?
        .parse()
        .map_err(|_| "invalid generation")?;
    let revision: u64 = get("--expected-revision")?
        .parse()
        .map_err(|_| "invalid expected revision")?;
    let artifact = fs::canonicalize(get("--artifact")?).map_err(|e| e.to_string())?;
    let artifact_bytes =
        read_bounded_regular(get("--artifact")?, 1024 * 1024).map_err(|e| e.to_string())?;
    let home =
        fs::canonicalize(std::env::var_os("MX_HOME").ok_or("parent outcome requires MX_HOME")?)
            .map_err(|e| e.to_string())?;
    let state = fs::canonicalize(state).map_err(|e| e.to_string())?;
    let _lock = DirectoryLock::acquire_wait(
        state.join(format!(".{id}.identity.lock")),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())?;
    require_writer_version(&state)?;
    let before = read_bounded_regular(state.join(format!("{id}.meta")), MAX_TASK_METADATA_BYTES)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(before.clone()).map_err(|e| e.to_string())?;
    let mut record = read_meta(id, &text)?;
    let attempt = record.attempt.as_ref().ok_or("task attempt missing")?;
    if record.legacy_unknown
        || record_state(&record)? != state
        || attempt.id != attempt_id
        || attempt.generation != generation
        || attempt.brief_revision != revision
        || record.accepted_brief_revision != Some(revision)
    {
        return Err(
            "parent outcome requires the exact current owner, attempt and accepted revision".into(),
        );
    }
    let parent_home = fs::canonicalize(record.parent_home.as_deref().ok_or("parent home missing")?)
        .map_err(|e| e.to_string())?;
    let parent_state = fs::canonicalize(
        record
            .parent_state
            .as_deref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| parent_home.join("state")),
    )
    .map_err(|e| e.to_string())?;
    let parent_id = record.parent_id.as_deref().ok_or("parent id missing")?;
    let caller = std::env::var("MX_TASK_ID").ok().filter(|v| !v.is_empty());
    let actor = match caller {
        Some(actor) => actor,
        None => {
            if !parent_id
                .strip_prefix("root-home:")
                .and_then(|p| fs::canonicalize(p).ok())
                .is_some_and(|p| p == home)
                || record.root_id.as_deref() != Some(parent_id)
            {
                return Err("unbound caller is not this task's recorded root parent".into());
            }
            parent_id.to_owned()
        }
    };
    if actor != parent_id
        || (actor.starts_with("root-home:") && (parent_home != home || parent_state != state))
    {
        return Err("caller is not this task's recorded parent home/state/identity".into());
    }
    if !actor.starts_with("root-home:") {
        let parent_bytes = read_bounded_regular(
            parent_state.join(format!("{actor}.meta")),
            MAX_TASK_METADATA_BYTES,
        )
        .map_err(|e| e.to_string())?;
        let parent = read_meta(
            &actor,
            &String::from_utf8(parent_bytes).map_err(|e| e.to_string())?,
        )?;
        let binding = parent.attempt.as_ref().ok_or("parent attempt missing")?;
        if parent.legacy_unknown
            || parent
                .owner_home
                .as_deref()
                .and_then(|p| fs::canonicalize(p).ok())
                != Some(parent_home.clone())
            || record_state(&parent)? != parent_state
            || parent
                .persistent_home
                .as_deref()
                .or(parent.owner_home.as_deref())
                .and_then(|p| fs::canonicalize(p).ok())
                != Some(home.clone())
            || record
                .owner_home
                .as_deref()
                .and_then(|p| fs::canonicalize(p).ok())
                != Some(home.clone())
            || binding.brief_revision != parent.accepted_brief_revision.unwrap_or_default()
            || std::env::var("MX_ATTEMPT_ID").as_deref() != Ok(binding.id.as_str())
            || std::env::var("MX_ATTEMPT_GENERATION")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                != Some(binding.generation)
            || std::env::var("MX_BRIEF_REVISION")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                != parent.accepted_brief_revision
        {
            return Err("parent caller has a stale or foreign assignment binding".into());
        }
    }
    let evidence = json!({"schema":"mx-parent-task-outcome.v1","outcome_id":outcome_id,"task_id":id,"actor":actor,"actor_home":home,"actor_authority_home":parent_home,"actor_state":parent_state,"owner_state":state,"attempt_id":attempt_id,"generation":generation,"brief_revision":revision,"state":disposition,"message":message,"artifact":artifact,"artifact_sha256":format!("{:x}",Sha256::digest(&artifact_bytes)),"completion_proven":false,"endpoint_stopped":false});
    let bytes = serde_json::to_vec(&evidence).map_err(|e| e.to_string())?;
    let relative = std::path::PathBuf::from(format!("parent-task-outcomes/{id}-{outcome_id}.json"));
    let operation = format!("parent-task-outcome-{id}-{outcome_id}");
    if state
        .join(".transitions")
        .join(format!("{operation}.json"))
        .exists()
    {
        multplx_core::filesystem::recover_transition(&state, &operation)
            .map_err(|e| e.to_string())?;
    }
    if let Ok(old) = read_bounded_regular(state.join(&relative), 1024 * 1024) {
        if old != bytes {
            return Err("parent outcome identity reused with changed facts".into());
        }
        multplx_core::filesystem::recover_transition(&state, &operation)
            .map_err(|e| e.to_string())?;
        return Ok(format!("{evidence}\n"));
    }
    if let Some(question) = record
        .schedule
        .decisions
        .iter()
        .find(|q| q.answer.is_none())
    {
        record.schedule.state = WorkState::WaitingHuman;
        if record.schedule.waiting_condition.is_none() {
            record.schedule.waiting_condition = Some(question.id.clone());
        }
    } else {
        record.schedule.state = WorkState::WaitingExternal;
        record.schedule.waiting_condition = Some(format!("parent-{disposition}: {message}"));
    }
    fs::create_dir_all(state.join("parent-task-outcomes")).map_err(|e| e.to_string())?;
    let status_relative = std::path::PathBuf::from(format!("{id}.status"));
    let status_before = match read_bounded_regular(state.join(&status_relative), 16 * 1024 * 1024) {
        Ok(bytes) => Some(bytes),
        Err(multplx_core::error::CoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            None
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut status_after = status_before.clone().unwrap_or_default();
    status_after.extend_from_slice(
        format!("{disposition}: parent outcome {outcome_id}: {message}\n").as_bytes(),
    );
    let writes = vec![
        TransitionWrite {
            path: format!("{id}.meta").into(),
            before: Some(before),
            after: write_meta(&text, &record)?.into_bytes(),
        },
        TransitionWrite {
            path: relative,
            before: None,
            after: bytes,
        },
        TransitionWrite {
            path: status_relative,
            before: status_before,
            after: status_after,
        },
    ];
    recoverable_transition(&state, &operation, &writes, None).map_err(|e| e.to_string())?;
    Ok(format!("{evidence}\n"))
}

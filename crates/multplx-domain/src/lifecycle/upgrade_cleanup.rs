//! Revision-bound retirement evidence retained by installer maintenance.
use super::subagent_model::{TaskRecord, WorkState, read_meta};
use multplx_core::{
    filesystem::{atomic_replace, read_bounded_regular},
    identifiers::TaskId,
};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Retirement {
    pub schema: String,
    pub task: String,
    pub original_meta: Vec<u8>,
    pub committed: bool,
}

/// Private maintenance directories never traverse a linked or foreign owner.
pub fn private_directory(path: &Path, create: bool) -> Result<(), String> {
    if create {
        match fs::create_dir(path) {
            Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o022 != 0
    {
        return Err(format!(
            "unsafe maintenance directory {}; retained",
            path.display()
        ));
    }
    Ok(())
}

pub fn receipt_path(state: &Path, task: &str) -> Result<PathBuf, String> {
    TaskId::parse(task).map_err(|e| e.to_string())?;
    Ok(state
        .join(".upgrade-retirements")
        .join(format!("{task}.json")))
}

fn completed_facts(record: &TaskRecord, state: &Path) -> bool {
    !record.legacy_unknown
        && record.schedule.state == WorkState::Completed
        && record.schedule.decisions.iter().all(|q| q.answer.is_some())
        && record.owner_state.as_deref() == state.to_str()
        && record.attempt.is_some()
        && record.accepted_brief_revision.is_some()
        && record.retained_executions.is_empty()
        && record.transfers.is_empty()
        && !crate::supervision::has_unsettled_native_children(record)
}

pub fn completed(record: &TaskRecord, state: &Path) -> bool {
    completed_facts(record, state)
        && super::pending_reply::maintenance_retirement_clear(state, Some(&record.task_id)).is_ok()
}

pub fn read_retirement(state: &Path, task: &str) -> Result<Retirement, String> {
    private_directory(&state.join(".upgrade-retirements"), false)?;
    let bytes = read_bounded_regular(receipt_path(state, task)?, 2 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    let receipt: Retirement = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let record = read_meta(
        task,
        std::str::from_utf8(&receipt.original_meta).map_err(|e| e.to_string())?,
    )?;
    if receipt.schema != "mx-upgrade-retirement.v1"
        || receipt.task != task
        || !completed_facts(&record, state)
    {
        return Err("unproven retirement retained".into());
    }
    if receipt.committed {
        let token = record
            .home_allocation
            .as_ref()
            .ok_or("committed retirement lacks a home lease")?;
        let owner = record
            .owner_home
            .as_deref()
            .ok_or("retirement owner home missing")?;
        let allocation =
            super::home_seed::read_home_allocation(&Path::new(owner).join("data"), task)?
                .ok_or("committed home allocation missing")?;
        if allocation.binding != *token {
            return Err("retired home lease changed; retained".into());
        }
        super::home_seed::verify_retired_home(&allocation)?;
    }
    Ok(receipt)
}

pub fn completed_record(state: &Path, task: &str) -> Result<TaskRecord, String> {
    let path = state.join(format!("{task}.meta"));
    let record = if fs::symlink_metadata(&path).is_ok() {
        let bytes = read_bounded_regular(path, super::subagent_model::MAX_TASK_METADATA_BYTES)
            .map_err(|e| e.to_string())?;
        read_meta(
            task,
            std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
        )?
    } else {
        let receipt = read_retirement(state, task)?;
        if !receipt.committed {
            return Err("retirement incomplete; retained".into());
        }
        read_meta(
            task,
            std::str::from_utf8(&receipt.original_meta).map_err(|e| e.to_string())?,
        )?
    };
    if !completed(&record, state) {
        return Err("task not canonically settled; retained".into());
    }
    Ok(record)
}

pub fn write_retirement(state: &Path, receipt: &Retirement) -> Result<(), String> {
    let path = receipt_path(state, &receipt.task)?;
    fs::create_dir_all(path.parent().expect("receipt directory")).map_err(|e| e.to_string())?;
    if path.exists() {
        let previous = read_retirement(state, &receipt.task)?;
        if previous.original_meta != receipt.original_meta
            || previous.committed && !receipt.committed
        {
            return Err("retirement evidence conflicts; retained".into());
        }
    }
    atomic_replace(
        path,
        &serde_json::to_vec(receipt).map_err(|e| e.to_string())?,
        0o600,
    )
    .map_err(|e| e.to_string())
}

//! Composite local/API dispatch capacity and private durable dispatch queue.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use multplx_core::filesystem::{atomic_replace, read_bounded_regular};
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use serde::Serialize;
use serde_json::Value;

use crate::cmux::CmuxBackend;
use crate::facade::{BackendName, BackendTarget, RuntimeBackend};
use crate::herdr::HerdrBackend;
use crate::tmux::TmuxBackend;

const DEFAULT_API_CAPACITY: u64 = 20;
const RECORD_LIMIT: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum HeadroomError {
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Io(#[from] io::Error),
}

type Result<T> = std::result::Result<T, HeadroomError>;

#[derive(Clone, Debug)]
pub struct HeadroomPaths {
    pub state: PathBuf,
    pub config: PathBuf,
    pub proc_root: PathBuf,
}

impl HeadroomPaths {
    #[must_use]
    pub fn from_environment() -> Self {
        let root = std::env::var_os("MX_ROOT_OVERRIDE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let home = std::env::var_os("MX_HOME")
            .map(PathBuf::from)
            .unwrap_or(root);
        Self {
            state: std::env::var_os("MX_STATE_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("state")),
            config: std::env::var_os("MX_CONFIG_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("config")),
            proc_root: std::env::var_os("MX_HEADROOM_PROC_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/proc")),
        }
    }

    fn queue_dir(&self) -> PathBuf {
        self.state.join(".dispatch-queue")
    }
    fn queue_lock(&self) -> PathBuf {
        self.state.join(".dispatch-queue.lock")
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CandidateHeadroom {
    capacity: u64,
    in_use: u64,
    available: u64,
    window: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalHeadroom {
    cpu_count: f64,
    load_one: f64,
    memory_available_bytes: u64,
    available: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiHeadroom {
    source: &'static str,
    capacity: u64,
    available: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Headroom {
    model: &'static str,
    capacity: u64,
    in_use: u64,
    available: u64,
    at_limit: bool,
    local: LocalHeadroom,
    api: ApiHeadroom,
    candidates: BTreeMap<String, CandidateHeadroom>,
}

fn message(value: impl Into<String>) -> HeadroomError {
    HeadroomError::Message(value.into())
}

fn parse_nonnegative_integer(value: &str) -> Option<u64> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().ok())
        .flatten()
}

fn parse_nonnegative_number(value: &str) -> Option<f64> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    let number = value.parse::<f64>().ok()?;
    number.is_finite().then_some(number)
}

fn parse_positive_number(value: &str) -> Option<f64> {
    parse_nonnegative_number(value).filter(|number| *number > 0.0)
}

fn command_text(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

fn platform() -> String {
    std::env::var("MX_HEADROOM_PLATFORM").unwrap_or_else(|_| {
        command_text("uname", &["-s"])
            .map(|value| value.trim().to_owned())
            .unwrap_or_default()
    })
}

fn cpu_count(paths: &HeadroomPaths) -> Result<f64> {
    if let Ok(value) = std::env::var("MX_HEADROOM_CPU_COUNT") {
        return parse_positive_number(&value)
            .ok_or_else(|| message("CPU capacity signal is unreadable"));
    }
    match platform().as_str() {
        "Darwin" => command_text("sysctl", &["-n", "hw.logicalcpu"])
            .and_then(|value| parse_positive_number(value.trim()))
            .ok_or_else(|| message("CPU capacity signal is unreadable")),
        "Linux" => fs::read_to_string(paths.proc_root.join("cpuinfo"))
            .ok()
            .map(|text| {
                text.lines()
                    .filter(|line| line.trim_start().starts_with("processor") && line.contains(':'))
                    .count() as f64
            })
            .filter(|count| *count > 0.0)
            .ok_or_else(|| message("CPU capacity signal is unreadable")),
        _ => Err(message("CPU capacity signal is unreadable")),
    }
}

fn load_one(paths: &HeadroomPaths) -> Result<f64> {
    if let Ok(value) = std::env::var("MX_HEADROOM_LOAD1") {
        return parse_nonnegative_number(&value)
            .ok_or_else(|| message("one-minute load signal is unreadable"));
    }
    match platform().as_str() {
        "Darwin" => command_text("sysctl", &["-n", "vm.loadavg"])
            .and_then(|text| {
                text.split_whitespace()
                    .find_map(|word| parse_nonnegative_number(word.trim_matches(['{', '}'])))
            })
            .ok_or_else(|| message("one-minute load signal is unreadable")),
        "Linux" => fs::read_to_string(paths.proc_root.join("loadavg"))
            .ok()
            .and_then(|text| {
                text.split_whitespace()
                    .next()
                    .and_then(parse_nonnegative_number)
            })
            .ok_or_else(|| message("one-minute load signal is unreadable")),
        _ => Err(message("one-minute load signal is unreadable")),
    }
}

fn memory_available(paths: &HeadroomPaths) -> Result<u64> {
    if let Ok(value) = std::env::var("MX_HEADROOM_MEM_AVAILABLE_BYTES") {
        return parse_nonnegative_integer(&value)
            .ok_or_else(|| message("available-memory signal is unreadable"));
    }
    match platform().as_str() {
        "Darwin" => {
            let text = command_text("vm_stat", &[])
                .ok_or_else(|| message("available-memory signal is unreadable"))?;
            let page = text
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().find_map(parse_nonnegative_integer))
                .ok_or_else(|| message("available-memory signal is unreadable"))?;
            let pages = text
                .lines()
                .filter(|line| {
                    [
                        "Pages free:",
                        "Pages inactive:",
                        "Pages speculative:",
                        "Pages purgeable:",
                    ]
                    .iter()
                    .any(|prefix| line.starts_with(prefix))
                })
                .filter_map(|line| {
                    parse_nonnegative_integer(line.split_whitespace().last()?.trim_end_matches('.'))
                })
                .sum::<u64>();
            Ok(page.saturating_mul(pages))
        }
        "Linux" => fs::read_to_string(paths.proc_root.join("meminfo"))
            .ok()
            .and_then(|text| {
                text.lines().find_map(|line| {
                    let rest = line.strip_prefix("MemAvailable:")?;
                    parse_nonnegative_integer(rest.split_whitespace().next()?)
                        .map(|value| value.saturating_mul(1024))
                })
            })
            .ok_or_else(|| message("available-memory signal is unreadable")),
        _ => Err(message("available-memory signal is unreadable")),
    }
}

fn metadata_value(path: &Path, key: &str) -> Option<String> {
    let bytes = read_bounded_regular(path, RECORD_LIMIT).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let prefix = format!("{key}=");
    text.lines()
        .filter_map(|line| line.strip_prefix(&prefix))
        .next_back()
        .map(str::to_owned)
}

fn target_is_live(backend: &str, target: &str) -> bool {
    let Ok(name) = BackendName::parse(backend) else {
        return false;
    };
    let Ok(target) = BackendTarget::new(name, target.to_owned(), None) else {
        return false;
    };
    match name {
        BackendName::Tmux => TmuxBackend::system().target_ready(&target).is_ok(),
        BackendName::Herdr => HerdrBackend::system().target_ready(&target).is_ok(),
        BackendName::Cmux => CmuxBackend::system().target_ready(&target).is_ok(),
    }
}

fn live_counts(paths: &HeadroomPaths) -> Result<(u64, HashMap<String, u64>)> {
    if let Ok(value) = std::env::var("MX_HEADROOM_IN_USE") {
        let count = parse_nonnegative_integer(&value)
            .ok_or_else(|| message("live actor count is unreadable"))?;
        return Ok((count, HashMap::new()));
    }
    let entries = match fs::read_dir(&paths.state) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((0, HashMap::new())),
        Err(_) => return Err(message("live actor count is unreadable")),
    };
    let mut total = 0_u64;
    let mut harnesses = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("meta")
            || fs::symlink_metadata(&path).is_err()
            || metadata_value(&path, "kind").as_deref() == Some("daemon")
        {
            continue;
        }
        let Some(target) = metadata_value(&path, "window") else {
            continue;
        };
        let backend = metadata_value(&path, "backend")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "tmux".to_owned());
        if target_is_live(&backend, &target) {
            total += 1;
            let harness = metadata_value(&path, "harness")
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "default".to_owned());
            *harnesses.entry(harness).or_insert(0) += 1;
        }
    }
    Ok((total, harnesses))
}

fn read_compact(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|text| {
        text.chars()
            .filter(|character| !character.is_whitespace())
            .collect()
    })
}

fn configured_capacity(paths: &HeadroomPaths, candidate: Option<&str>) -> Result<u64> {
    let value = candidate
        .map(|name| paths.config.join(format!("api-capacity-{name}")))
        .filter(|path| path.is_file())
        .and_then(|path| read_compact(&path))
        .or_else(|| {
            std::env::var("MX_HEADROOM_API_CAPACITY")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or_else(|| read_compact(&paths.config.join("api-capacity")))
        .unwrap_or_else(|| DEFAULT_API_CAPACITY.to_string());
    parse_nonnegative_integer(&value).ok_or_else(|| {
        candidate.map_or_else(
            || message("configured API capacity is invalid"),
            |name| message(format!("configured API capacity for {name} is invalid")),
        )
    })
}

fn profile_harnesses(value: &Value, output: &mut Vec<String>) -> Result<()> {
    match value {
        Value::Object(profile) => {
            let harness = profile
                .get("harness")
                .and_then(Value::as_str)
                .ok_or_else(|| message("configured dispatch candidates are unreadable"))?;
            output.push(harness.to_owned());
            Ok(())
        }
        Value::Array(profiles) if !profiles.is_empty() => {
            for profile in profiles {
                profile_harnesses(profile, output)?;
            }
            Ok(())
        }
        _ => Err(message("configured dispatch candidates are unreadable")),
    }
}

fn configured_candidates(paths: &HeadroomPaths) -> Result<Vec<String>> {
    if std::env::var("MX_HEADROOM_IGNORE_DISPATCH_CONFIG").as_deref() == Ok("1") {
        return Ok(vec!["default".to_owned()]);
    }
    let dispatch = paths.config.join("actor-dispatch.json");
    let mut candidates = Vec::new();
    if dispatch.is_file() {
        let bytes = read_bounded_regular(&dispatch, 1024 * 1024)
            .map_err(|_| message("configured dispatch candidates are unreadable"))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| message("configured dispatch candidates are unreadable"))?;
        if let Some(rules) = value.get("rules").and_then(Value::as_array) {
            for rule in rules {
                if let Some(profiles) = rule.get("use") {
                    profile_harnesses(profiles, &mut candidates)?;
                }
            }
        }
        if let Some(profiles) = value.get("default") {
            profile_harnesses(profiles, &mut candidates)?;
        }
    } else if let Ok(text) = fs::read_to_string(paths.config.join("actor-harness")) {
        let harness = text
            .lines()
            .find_map(|line| line.split_whitespace().next())
            .ok_or_else(|| message("configured dispatch candidates are unreadable"))?;
        candidates.push(harness.to_owned());
    } else {
        candidates.push("default".to_owned());
    }
    candidates.sort();
    candidates.dedup();
    if candidates.is_empty() {
        return Err(message("configured dispatch candidates are empty"));
    }
    if let Some(invalid) = candidates.iter().find(|candidate| {
        candidate.is_empty()
            || !candidate
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    }) {
        return Err(message(format!("invalid configured candidate: {invalid}")));
    }
    Ok(candidates)
}

pub fn evaluate(paths: &HeadroomPaths) -> Result<Headroom> {
    let cpu_count = cpu_count(paths)?;
    let load_one = load_one(paths)?;
    let memory_available = memory_available(paths)?;
    let (in_use, harness_use) = live_counts(paths)?;
    let cpu_per_actor = std::env::var("MX_HEADROOM_CPU_PER_ACTOR")
        .ok()
        .map(|value| parse_positive_number(&value))
        .unwrap_or(Some(0.25))
        .ok_or_else(|| message("MX_HEADROOM_CPU_PER_ACTOR must be positive"))?;
    let memory_per_actor = std::env::var("MX_HEADROOM_MEM_PER_ACTOR_BYTES")
        .ok()
        .map(|value| parse_nonnegative_integer(&value))
        .unwrap_or(Some(268_435_456))
        .filter(|value| *value > 0)
        .ok_or_else(|| message("MX_HEADROOM_MEM_PER_ACTOR_BYTES must be positive"))?;
    let cpu_slots = (((cpu_count - load_one) / cpu_per_actor).floor().max(0.0)) as u64;
    let memory_slots = memory_available / memory_per_actor;
    let local_available = cpu_slots.min(memory_slots);
    let global_capacity = configured_capacity(paths, None)?;
    let global_available = global_capacity.saturating_sub(in_use);
    let overridden_use = std::env::var("MX_HEADROOM_IN_USE")
        .ok()
        .and_then(|value| parse_nonnegative_integer(&value));
    let mut candidates = BTreeMap::new();
    let mut candidate_max = None;
    for candidate in configured_candidates(paths)? {
        let capacity = configured_capacity(paths, Some(&candidate))?;
        let candidate_in_use =
            overridden_use.unwrap_or_else(|| harness_use.get(&candidate).copied().unwrap_or(0));
        let available = capacity
            .saturating_sub(candidate_in_use)
            .min(global_available);
        candidate_max = Some(candidate_max.unwrap_or(0).max(available));
        candidates.insert(
            candidate,
            CandidateHeadroom {
                capacity,
                in_use: candidate_in_use,
                available,
                window: "configured-budget",
            },
        );
    }
    let available = local_available
        .min(candidate_max.ok_or_else(|| message("configured dispatch candidates are empty"))?);
    Ok(Headroom {
        model: "local+api",
        capacity: in_use + available,
        in_use,
        available,
        at_limit: available == 0,
        local: LocalHeadroom {
            cpu_count,
            load_one,
            memory_available_bytes: memory_available,
            available: local_available,
        },
        api: ApiHeadroom {
            source: "configured-budget",
            capacity: global_capacity,
            available: global_available,
        },
        candidates,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueRecord {
    pub task_id: String,
    pub project: String,
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub backend: String,
    pub kind: String,
    pub enqueued_at: u64,
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn one_line(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(message(format!("{label} must not be empty")));
    }
    if value.contains(['\n', '\r']) {
        return Err(message(format!("{label} must be one line")));
    }
    Ok(())
}

impl QueueRecord {
    fn render(&self) -> Vec<u8> {
        format!(
            "version=1\ntask_id={}\nproject={}\nharness={}\nmodel={}\neffort={}\nbackend={}\nkind={}\nenqueued_at={}\n",
            self.task_id, self.project, self.harness, self.model, self.effort, self.backend, self.kind, self.enqueued_at
        ).into_bytes()
    }

    fn parse(path: &Path, expected_id: &str) -> Result<Self> {
        let metadata = fs::symlink_metadata(path).map_err(|_| {
            message(format!(
                "queue record is not a regular private file: {}",
                path.display()
            ))
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(message(format!(
                "queue record is not a regular private file: {}",
                path.display()
            )));
        }
        if metadata.mode() & 0o777 != 0o600 {
            return Err(message(format!(
                "queue record mode must be 0600: {}",
                path.display()
            )));
        }
        let bytes = read_bounded_regular(path, RECORD_LIMIT)
            .map_err(|_| message(format!("queue record is unreadable: {}", path.display())))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| message(format!("queue record is unreadable: {}", path.display())))?;
        let mut fields = HashMap::new();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                fields.insert(key, value);
            }
        }
        if fields.get("version") != Some(&"1") {
            return Err(message(format!(
                "queue record has an unsupported version: {}",
                path.display()
            )));
        }
        let task_id = fields
            .get("task_id")
            .copied()
            .unwrap_or_default()
            .to_owned();
        if !valid_id(&task_id) {
            return Err(message(format!("invalid queue task id: {task_id}")));
        }
        if task_id != expected_id {
            return Err(message(format!(
                "queue record identity does not match its path: {}",
                path.display()
            )));
        }
        let project = fields
            .get("project")
            .copied()
            .unwrap_or_default()
            .to_owned();
        one_line("project", &project)?;
        let harness = fields
            .get("harness")
            .copied()
            .unwrap_or_default()
            .to_owned();
        let model = fields.get("model").copied().unwrap_or_default().to_owned();
        let effort = fields.get("effort").copied().unwrap_or_default().to_owned();
        let backend = fields
            .get("backend")
            .copied()
            .unwrap_or_default()
            .to_owned();
        for value in [&harness, &model, &effort, &backend] {
            if !value.is_empty() {
                one_line("queue profile value", value)?;
            }
        }
        let kind = fields.get("kind").copied().unwrap_or_default().to_owned();
        if !matches!(kind.as_str(), "delivery" | "scout") {
            return Err(message(format!(
                "queue record has invalid kind: {}",
                path.display()
            )));
        }
        let enqueued_at = fields
            .get("enqueued_at")
            .and_then(|value| parse_nonnegative_integer(value))
            .ok_or_else(|| {
                message(format!(
                    "queue record has invalid enqueue time: {}",
                    path.display()
                ))
            })?;
        Ok(Self {
            task_id,
            project,
            harness,
            model,
            effort,
            backend,
            kind,
            enqueued_at,
        })
    }
}

fn acquire(paths: &HeadroomPaths) -> Result<DirectoryLock> {
    fs::create_dir_all(&paths.state)?;
    let processes = SystemProcessProbe::default();
    for _ in 0..100 {
        match DirectoryLock::try_acquire(paths.queue_lock(), &processes) {
            Ok(lock) => return Ok(lock),
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
    Err(message("dispatch queue is busy"))
}

fn queue_records(paths: &HeadroomPaths) -> Result<Vec<QueueRecord>> {
    let directory = paths.queue_dir();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut records = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("request") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| message(format!("queue record path is unsafe: {}", path.display())))?;
        records.push(QueueRecord::parse(&path, id)?);
    }
    records.sort_by(|left, right| {
        (left.enqueued_at, &left.task_id).cmp(&(right.enqueued_at, &right.task_id))
    });
    Ok(records)
}

pub fn queue_list(paths: &HeadroomPaths) -> Result<String> {
    let mut output = String::new();
    for record in queue_records(paths)? {
        fn dash(value: &str) -> &str {
            if value.is_empty() { "-" } else { value }
        }
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            record.enqueued_at,
            record.task_id,
            record.project,
            dash(&record.harness),
            dash(&record.model),
            dash(&record.effort),
            dash(&record.backend),
            record.kind
        ));
    }
    Ok(output)
}

pub fn queue_add(paths: &HeadroomPaths, record: &QueueRecord) -> Result<String> {
    if !valid_id(&record.task_id) {
        return Err(message(format!(
            "invalid queue task id: {}",
            record.task_id
        )));
    }
    one_line("project", &record.project)?;
    for value in [
        &record.harness,
        &record.model,
        &record.effort,
        &record.backend,
    ] {
        if !value.is_empty() {
            one_line("queue profile value", value)?;
        }
    }
    if !matches!(record.kind.as_str(), "delivery" | "scout") {
        return Err(message(format!(
            "queue record has invalid kind: {}",
            record.kind
        )));
    }
    let _lock = acquire(paths)?;
    let directory = paths.queue_dir();
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    let path = directory.join(format!("{}.request", record.task_id));
    if path.exists() {
        let existing = QueueRecord::parse(&path, &record.task_id)?;
        if existing.project == record.project
            && existing.harness == record.harness
            && existing.model == record.model
            && existing.effort == record.effort
            && existing.backend == record.backend
            && existing.kind == record.kind
        {
            return Ok(format!("queued: {} already parked\n", record.task_id));
        }
        return Err(message(format!(
            "queued dispatch {} already exists with a different request",
            record.task_id
        )));
    }
    if fs::symlink_metadata(&path).is_ok() {
        return Err(message(format!(
            "queue record path is unsafe: {}",
            path.display()
        )));
    }
    atomic_replace(&path, &record.render(), 0o600).map_err(|error| message(error.to_string()))?;
    Ok(format!(
        "queued: {} parked until dispatch capacity is available\n",
        record.task_id
    ))
}

pub fn queue_cancel(paths: &HeadroomPaths, id: &str) -> Result<String> {
    if !valid_id(id) {
        return Err(message(format!("invalid queue task id: {id}")));
    }
    let _lock = acquire(paths)?;
    let path = paths.queue_dir().join(format!("{id}.request"));
    if fs::symlink_metadata(&path).is_err() {
        return Err(message(format!("queued dispatch not found: {id}")));
    }
    QueueRecord::parse(&path, id)?;
    fs::remove_file(path)?;
    Ok(format!("cancelled: {id}\n"))
}

pub fn queue_drain(paths: &HeadroomPaths) -> Result<String> {
    if !paths.queue_dir().is_dir() {
        return Ok(String::new());
    }
    let _lock = acquire(paths)?;
    if evaluate(paths)?.available == 0 {
        return Ok(String::new());
    }
    let Some(record) = queue_records(paths)?.into_iter().next() else {
        return Ok(String::new());
    };
    let spawn = std::env::var_os("MX_HEADROOM_SPAWN_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let root = std::env::var_os("MX_ROOT_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
            root.join("bin/mx-spawn.sh")
        });
    let mut command = Command::new(spawn);
    command
        .env("MX_HEADROOM_SKIP_QUEUE", "1")
        .args([&record.task_id, &record.project]);
    if !record.harness.is_empty() {
        command.args(["--harness", &record.harness]);
    }
    if !record.model.is_empty() {
        command.args(["--model", &record.model]);
    }
    if !record.effort.is_empty() {
        command.args(["--effort", &record.effort]);
    }
    if !record.backend.is_empty() {
        command.args(["--backend", &record.backend]);
    }
    if record.kind == "scout" {
        command.arg("--scout");
    }
    let status = command.status().map_err(|_| {
        message(format!(
            "queued dispatch {} could not be launched; record retained",
            record.task_id
        ))
    })?;
    if !status.success() {
        return Err(message(format!(
            "queued dispatch {} could not be launched; record retained",
            record.task_id
        )));
    }
    fs::remove_file(
        paths
            .queue_dir()
            .join(format!("{}.request", record.task_id)),
    )?;
    Ok(format!("dispatch-queue: launched {}\n", record.task_id))
}

#[must_use]
pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::thread;

    use super::{
        HeadroomPaths, QueueRecord, configured_candidates, configured_capacity, metadata_value,
        parse_nonnegative_integer, parse_nonnegative_number, parse_positive_number,
        profile_harnesses, queue_add, queue_cancel, queue_list, queue_records, read_compact,
        valid_id,
    };

    fn paths(temp: &tempfile::TempDir) -> HeadroomPaths {
        let state = temp.path().join("state");
        let config = temp.path().join("config");
        std::fs::create_dir(&state).expect("state");
        std::fs::create_dir(&config).expect("config");
        HeadroomPaths {
            state,
            config,
            proc_root: temp.path().join("proc"),
        }
    }

    #[test]
    fn candidates_are_unique_and_order_independent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        for use_profiles in [
            r#"[{"harness":"pi"},{"harness":"codex"},{"harness":"pi"}]"#,
            r#"[{"harness":"codex"},{"harness":"pi"},{"harness":"pi"}]"#,
            r#"[{"harness":"pi"},{"harness":"pi"},{"harness":"codex"}]"#,
        ] {
            std::fs::write(
                paths.config.join("actor-dispatch.json"),
                format!(
                    r#"{{"rules":[{{"use":{use_profiles}}}],"default":{{"harness":"claude"}}}}"#
                ),
            )
            .expect("dispatch");
            assert_eq!(
                configured_candidates(&paths).expect("candidates"),
                ["claude", "codex", "pi"]
            );
        }
    }

    #[test]
    fn parsing_capacity_and_candidate_refusals_are_exhaustive() {
        assert_eq!(parse_nonnegative_integer("0"), Some(0));
        assert_eq!(parse_nonnegative_integer("42"), Some(42));
        assert_eq!(parse_nonnegative_integer(""), None);
        assert_eq!(parse_nonnegative_integer("-1"), None);
        assert_eq!(parse_nonnegative_number("1.5"), Some(1.5));
        assert_eq!(parse_nonnegative_number("NaN"), None);
        assert_eq!(parse_nonnegative_number("1.2.3"), None);
        assert_eq!(parse_positive_number("0"), None);
        assert!(valid_id("task-1.ok"));
        assert!(!valid_id("bad/id"));

        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let compact = paths.config.join("compact");
        std::fs::write(&compact, " 2 \n 0 \n").expect("compact");
        assert_eq!(read_compact(&compact).as_deref(), Some("20"));
        assert_eq!(configured_capacity(&paths, None).unwrap(), 20);
        std::fs::write(paths.config.join("api-capacity"), "7\n").expect("capacity");
        assert_eq!(configured_capacity(&paths, None).unwrap(), 7);
        std::fs::write(paths.config.join("api-capacity-codex"), "3\n").expect("capacity");
        assert_eq!(configured_capacity(&paths, Some("codex")).unwrap(), 3);
        std::fs::write(paths.config.join("api-capacity-codex"), "bad\n").expect("capacity");
        assert!(configured_capacity(&paths, Some("codex")).is_err());

        let mut output = Vec::new();
        profile_harnesses(&serde_json::json!({"harness":"codex"}), &mut output).unwrap();
        assert_eq!(output, ["codex"]);
        assert!(profile_harnesses(&serde_json::json!([]), &mut output).is_err());
        assert!(profile_harnesses(&serde_json::json!({}), &mut output).is_err());

        for (contents, expected) in [
            ("{", "unreadable"),
            (r#"{"rules":[],"default":[]}"#, "unreadable"),
            (r#"{"rules":[]}"#, "empty"),
            (r#"{"default":{"harness":"bad/name"}}"#, "invalid"),
        ] {
            std::fs::write(paths.config.join("actor-dispatch.json"), contents).expect("dispatch");
            assert!(
                configured_candidates(&paths)
                    .unwrap_err()
                    .to_string()
                    .contains(expected)
            );
        }
        std::fs::remove_file(paths.config.join("actor-dispatch.json")).expect("remove");
        std::fs::write(paths.config.join("actor-harness"), " \n").expect("actor");
        assert!(configured_candidates(&paths).is_err());
    }

    #[test]
    fn metadata_and_malformed_queue_records_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let metadata = paths.state.join("actor.meta");
        std::fs::write(&metadata, "key=first\nother=x\nkey=last\n").expect("metadata");
        assert_eq!(metadata_value(&metadata, "key").as_deref(), Some("last"));
        assert_eq!(metadata_value(&metadata, "missing"), None);

        let directory = paths.queue_dir();
        std::fs::create_dir(&directory).expect("queue");
        let record = directory.join("bad.request");
        for contents in [
            "version=2\n",
            "version=1\ntask_id=bad/id\n",
            "version=1\ntask_id=other\n",
            "version=1\ntask_id=bad\nproject=\n",
            "version=1\ntask_id=bad\nproject=p\nkind=wrong\n",
            "version=1\ntask_id=bad\nproject=p\nkind=delivery\nenqueued_at=x\n",
        ] {
            std::fs::write(&record, contents).expect("record");
            std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o600))
                .expect("mode");
            assert!(queue_records(&paths).is_err(), "{contents}");
        }
        std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o644)).expect("mode");
        assert!(queue_records(&paths).is_err());
        std::fs::remove_file(&record).expect("remove");
        std::fs::create_dir(&record).expect("directory record");
        assert!(queue_records(&paths).is_err());
    }

    #[test]
    fn queue_mutation_rejects_invalid_and_conflicting_requests() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let base = QueueRecord {
            task_id: "one".to_owned(),
            project: "project".to_owned(),
            harness: String::new(),
            model: String::new(),
            effort: String::new(),
            backend: String::new(),
            kind: "delivery".to_owned(),
            enqueued_at: 1,
        };
        for record in [
            QueueRecord {
                task_id: "bad/id".to_owned(),
                ..base.clone()
            },
            QueueRecord {
                project: "".to_owned(),
                ..base.clone()
            },
            QueueRecord {
                harness: "bad\nvalue".to_owned(),
                ..base.clone()
            },
            QueueRecord {
                kind: "wrong".to_owned(),
                ..base.clone()
            },
        ] {
            assert!(queue_add(&paths, &record).is_err());
        }
        queue_add(&paths, &base).expect("add");
        let conflicting = QueueRecord {
            project: "other".to_owned(),
            ..base
        };
        assert!(queue_add(&paths, &conflicting).is_err());
        assert!(queue_cancel(&paths, "bad/id").is_err());
        assert!(queue_cancel(&paths, "missing").is_err());
    }

    #[test]
    fn queue_records_are_private_idempotent_and_cancel_exactly() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let record = QueueRecord {
            task_id: "one".to_owned(),
            project: "projects/one".to_owned(),
            harness: "codex".to_owned(),
            model: String::new(),
            effort: String::new(),
            backend: String::new(),
            kind: "delivery".to_owned(),
            enqueued_at: 1,
        };
        assert!(
            queue_add(&paths, &record)
                .expect("add")
                .contains("parked until")
        );
        assert!(
            queue_add(&paths, &record)
                .expect("repeat")
                .contains("already parked")
        );
        let path = paths.queue_dir().join("one.request");
        assert_eq!(
            std::fs::metadata(path).expect("mode").permissions().mode() & 0o777,
            0o600
        );
        assert!(
            queue_list(&paths)
                .expect("list")
                .contains("\tone\tprojects/one\tcodex\t")
        );
        assert_eq!(
            queue_cancel(&paths, "one").expect("cancel"),
            "cancelled: one\n"
        );
    }

    #[test]
    fn queue_lock_serializes_contending_writers_without_dropping_records() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = Arc::new(paths(&temp));
        let workers = (0..8)
            .map(|index| {
                let paths = Arc::clone(&paths);
                thread::spawn(move || {
                    queue_add(
                        &paths,
                        &QueueRecord {
                            task_id: format!("task-{index}"),
                            project: format!("projects/task-{index}"),
                            harness: "codex".to_owned(),
                            model: String::new(),
                            effort: String::new(),
                            backend: "tmux".to_owned(),
                            kind: "delivery".to_owned(),
                            enqueued_at: index,
                        },
                    )
                    .expect("queue add");
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().expect("writer");
        }
        let listing = queue_list(&paths).expect("listing");
        assert_eq!(listing.lines().count(), 8);
        for index in 0..8 {
            assert!(listing.contains(&format!("\ttask-{index}\t")));
        }
    }

    #[test]
    fn queue_recovery_ignores_unpublished_temporary_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let record = QueueRecord {
            task_id: "published".to_owned(),
            project: "projects/published".to_owned(),
            harness: String::new(),
            model: String::new(),
            effort: String::new(),
            backend: String::new(),
            kind: "delivery".to_owned(),
            enqueued_at: 1,
        };
        queue_add(&paths, &record).expect("published");
        std::fs::write(paths.queue_dir().join(".interrupted.tmp"), b"partial").expect("temporary");
        assert_eq!(queue_list(&paths).expect("recovery").lines().count(), 1);
    }
}

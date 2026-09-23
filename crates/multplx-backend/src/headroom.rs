//! Composite local/API dispatch capacity and private durable dispatch queue.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use multplx_core::filesystem::{atomic_replace, read_bounded_regular};
use multplx_core::locks::DirectoryLock;
use multplx_core::process::{ProcessIdentity, ProcessProbe, SystemProcessProbe};
use serde::{Deserialize, Serialize};
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
    /// Canonical operational root whose capacity is shared by every descendant home.
    pub root_home: PathBuf,
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
        let home = std::env::var_os("MX_ROOT_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("MX_HOME").map(PathBuf::from))
            .unwrap_or(root);
        Self {
            root_home: fs::canonicalize(&home).unwrap_or_else(|_| home.clone()),
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

    /// Build paths for an already validated root home. Callers must derive this
    /// from canonical task lineage rather than a launch-directory override.
    #[must_use]
    pub fn for_root(root_home: &Path) -> Self {
        let root_home = fs::canonicalize(root_home).unwrap_or_else(|_| root_home.to_path_buf());
        Self {
            state: root_home.join("state"),
            config: root_home.join("config"),
            proc_root: std::env::var_os("MX_HEADROOM_PROC_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/proc")),
            root_home,
        }
    }

    fn queue_dir(&self) -> PathBuf {
        self.state.join(".dispatch-queue")
    }
    fn queue_lock(&self) -> PathBuf {
        self.state.join(".dispatch-queue.lock")
    }
    fn admissions_dir(&self) -> PathBuf {
        self.state.join(".admissions")
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
    accounting_scope: &'static str,
    capacity: u64,
    in_use: u64,
    available: u64,
    at_limit: bool,
    worker_headroom: u64,
    workers_in_use: u64,
    coordinators_in_use: u64,
    unknown_session_use: u64,
    local: LocalHeadroom,
    api: ApiHeadroom,
    candidates: BTreeMap<String, CandidateHeadroom>,
    #[serde(skip)]
    observed_active_receipts: BTreeSet<String>,
}

fn admission_classes(paths: &HeadroomPaths) -> Result<(u64, u64, u64)> {
    let mut workers = 0u64;
    let mut coordinators = 0u64;
    let mut unknown = 0u64;
    for receipt in receipts(paths)? {
        if matches!(
            receipt.state,
            AdmissionState::Released | AdmissionState::Retryable
        ) || receipt
            .resources
            .get("session")
            .copied()
            .unwrap_or_default()
            == 0
        {
            continue;
        }
        let units = receipt
            .resources
            .get("session")
            .copied()
            .unwrap_or_default();
        let role = fs::read_to_string(
            receipt
                .owner_state
                .join(format!("{}.meta", receipt.task_id)),
        )
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("canonical_model="))
                .and_then(|model| serde_json::from_str::<Value>(model).ok())
                .and_then(|model| model.get("role").and_then(Value::as_str).map(str::to_owned))
        });
        match role.as_deref() {
            Some("sub-orchestrator") => coordinators = coordinators.saturating_add(units),
            Some(_) => workers = workers.saturating_add(units),
            None => unknown = unknown.saturating_add(units),
        }
    }
    Ok((workers, coordinators, unknown))
}

impl Headroom {
    /// Report whether the composite local/API capacity boundary is exhausted.
    #[must_use]
    pub const fn at_limit(&self) -> bool {
        self.at_limit
    }
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
        BackendName::Tmux => TmuxBackend::system().observe_target(&target).is_ok(),
        BackendName::Herdr => HerdrBackend::system().observe_target(&target).is_ok(),
        BackendName::Cmux => CmuxBackend::system().observe_target(&target).is_ok(),
    }
}

fn active_receipt_matches_metadata(
    receipt: &AdmissionReceipt,
    paths: &HeadroomPaths,
    task_id: &str,
    backend: &str,
    target: &str,
    attempt_id: Option<&str>,
) -> bool {
    receipt.state == AdmissionState::Active
        && same_path(&receipt.owner_state, &paths.state)
        && receipt.task_id == task_id
        && if receipt.backend.is_empty() {
            backend == "tmux"
        } else {
            receipt.backend == backend
        }
        && receipt
            .endpoint
            .as_deref()
            .is_none_or(|endpoint| endpoint == target)
        && attempt_id == Some(receipt.attempt_id.as_str())
}

fn live_counts_with_probe(
    paths: &HeadroomPaths,
    probe: impl Fn(&str, &str) -> bool,
    overridden_native_use: Option<u64>,
) -> Result<(u64, HashMap<String, u64>, BTreeSet<String>)> {
    let active = receipts(paths)?
        .into_iter()
        .filter(|receipt| receipt.state == AdmissionState::Active)
        .collect::<Vec<_>>();
    let observed = active
        .iter()
        .map(|receipt| receipt.request_id.clone())
        .collect::<BTreeSet<_>>();
    // Canonical active receipts are the root-wide source of truth: unlike the
    // root metadata directory, they include descendant homes and persistent
    // sessions. Native metadata below contributes legacy root executions that
    // have no exact active receipt.
    let mut total = 0_u64;
    let mut harnesses = HashMap::new();
    for receipt in &active {
        total = total.saturating_add(
            receipt
                .resources
                .get("session")
                .copied()
                .unwrap_or_default(),
        );
        for (resource, units) in &receipt.resources {
            if let Some(harness) = resource.strip_prefix("harness:") {
                let entry = harnesses.entry(harness.to_owned()).or_insert(0_u64);
                *entry = entry.saturating_add(*units);
            }
        }
    }
    if let Some(native_use) = overridden_native_use {
        return Ok((total.saturating_add(native_use), harnesses, observed));
    }
    let entries = match fs::read_dir(&paths.state) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((total, harnesses, observed));
        }
        Err(_) => return Err(message("live actor count is unreadable")),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("meta")
            || fs::symlink_metadata(&path).is_err()
        {
            continue;
        }
        let Some(task_id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(target) = metadata_value(&path, "window") else {
            continue;
        };
        let backend = metadata_value(&path, "backend")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "tmux".to_owned());
        let attempt = metadata_value(&path, "canonical_model").and_then(|model| {
            serde_json::from_str::<Value>(&model)
                .ok()
                .and_then(|value| {
                    value
                        .pointer("/attempt/id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
        });
        let matching = active
            .iter()
            .filter(|receipt| {
                active_receipt_matches_metadata(
                    receipt,
                    paths,
                    task_id,
                    &backend,
                    &target,
                    attempt.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        let harness = metadata_value(&path, "harness")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "default".to_owned());
        let session_accounted = matching.iter().any(|receipt| {
            receipt
                .resources
                .get("session")
                .is_some_and(|units| *units > 0)
        });
        let harness_accounted = matching.iter().any(|receipt| {
            receipt
                .resources
                .get(&format!("harness:{harness}"))
                .is_some_and(|units| *units > 0)
        });
        if session_accounted && harness_accounted {
            continue;
        }
        if probe(&backend, &target) {
            if !session_accounted {
                total = total.saturating_add(1);
            }
            if !harness_accounted {
                *harnesses.entry(harness).or_insert(0) += 1;
            }
        }
    }
    Ok((total, harnesses, observed))
}

fn live_counts(paths: &HeadroomPaths) -> Result<(u64, HashMap<String, u64>, BTreeSet<String>)> {
    let overridden_native_use = std::env::var("MX_HEADROOM_IN_USE")
        .ok()
        .map(|value| {
            parse_nonnegative_integer(&value)
                .ok_or_else(|| message("live actor count is unreadable"))
        })
        .transpose()?;
    live_counts_with_probe(paths, target_is_live, overridden_native_use)
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
    crate::harness::HarnessConfig::new(&paths.config)
        .validate_aliases()
        .map_err(message)?;
    let canonical_dispatch = paths.config.join("subagent-dispatch.json");
    let legacy_dispatch = paths.config.join("actor-dispatch.json");
    let dispatch = if canonical_dispatch.is_file() {
        canonical_dispatch
    } else {
        legacy_dispatch
    };
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
    } else if let Ok(text) =
        fs::read_to_string(paths.config.join("subagent-harness")).or_else(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                fs::read_to_string(paths.config.join("actor-harness"))
            } else {
                Err(error)
            }
        })
    {
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
    let (in_use, harness_use, observed_active_receipts) = live_counts(paths)?;
    let admission = admission_config(paths)?;
    let (workers_in_use, coordinators_in_use, classified_unknown) = admission_classes(paths)?;
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
        let active_candidate_use = harness_use.get(&candidate).copied().unwrap_or(0);
        let candidate_in_use = overridden_use
            .unwrap_or_default()
            .saturating_add(active_candidate_use);
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
        accounting_scope: "root-home-and-descendants",
        capacity: in_use + available,
        in_use,
        available,
        at_limit: available == 0,
        worker_headroom: admission.worker_headroom,
        workers_in_use,
        coordinators_in_use,
        unknown_session_use: in_use
            .saturating_sub(workers_in_use.saturating_add(coordinators_in_use))
            .max(classified_unknown),
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
        observed_active_receipts,
    })
}

const DEFAULT_AGING_SECONDS: u64 = 300;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum DispatchState {
    Queued,
    Dispatching,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub task_id: String,
    pub owner_state: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueRecord {
    /// Stable caller identity. Repeated submission with this ID must converge.
    pub request_id: String,
    pub task_id: String,
    pub root_home: PathBuf,
    pub owner_home: PathBuf,
    pub owner_state: PathBuf,
    pub parent_task_id: String,
    pub parent_home: PathBuf,
    pub parent_state: PathBuf,
    pub project: String,
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub backend: String,
    pub kind: String,
    pub mode: String,
    pub yolo: String,
    pub enqueued_at: u64,
    pub priority: i32,
    pub dependencies: Vec<Dependency>,
    /// Named root-scoped units. `session` defaults to one when omitted.
    pub resources: BTreeMap<String, u64>,
    pub state: DispatchState,
    pub dispatch_started_at: Option<u64>,
    /// Validated domain model frozen by the launch owner; opaque to the backend.
    pub canonical_model: Option<String>,
}

impl QueueRecord {
    #[must_use]
    pub fn legacy(task_id: String, project: String, enqueued_at: u64) -> Self {
        Self {
            request_id: task_id.clone(),
            task_id,
            root_home: PathBuf::new(),
            owner_home: PathBuf::new(),
            owner_state: PathBuf::new(),
            parent_task_id: String::new(),
            parent_home: PathBuf::new(),
            parent_state: PathBuf::new(),
            project,
            harness: String::new(),
            model: String::new(),
            effort: String::new(),
            backend: String::new(),
            kind: "delivery".into(),
            mode: String::new(),
            yolo: String::new(),
            enqueued_at,
            priority: 0,
            dependencies: Vec::new(),
            resources: BTreeMap::from([("session".into(), 1)]),
            state: DispatchState::Queued,
            dispatch_started_at: None,
            canonical_model: None,
        }
    }
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
        let mut text = format!(
            "version=3\nrequest_id={}\ntask_id={}\nroot_home={}\nowner_home={}\nowner_state={}\nparent_task_id={}\nparent_home={}\nparent_state={}\nproject={}\nharness={}\nmodel={}\neffort={}\nbackend={}\nkind={}\nmode={}\nyolo={}\nenqueued_at={}\npriority={}\ndependencies={}\nresources={}\nstate={}\ndispatch_started_at={}\n",
            self.request_id,
            self.task_id,
            self.root_home.display(),
            self.owner_home.display(),
            self.owner_state.display(),
            self.parent_task_id,
            self.parent_home.display(),
            self.parent_state.display(),
            self.project,
            self.harness,
            self.model,
            self.effort,
            self.backend,
            self.kind,
            self.mode,
            self.yolo,
            self.enqueued_at,
            self.priority,
            serde_json::to_string(&self.dependencies).expect("serializable dependencies"),
            serde_json::to_string(&self.resources).expect("serializable resources"),
            match self.state {
                DispatchState::Queued => "queued",
                DispatchState::Dispatching => "dispatching",
            },
            self.dispatch_started_at
                .map_or_else(String::new, |value| value.to_string()),
        );
        if let Some(model) = &self.canonical_model {
            text.push_str(&format!("canonical_model={model}\n"));
        }
        text.into_bytes()
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
            if let Some((key, value)) = line.split_once('=')
                && fields.insert(key, value).is_some()
            {
                return Err(message(format!("duplicate queue field: {key}")));
            }
        }
        if !matches!(fields.get("version"), Some(&"1" | &"2" | &"3")) {
            return Err(message(format!(
                "queue record has an unsupported version: {}",
                path.display()
            )));
        }
        if fields.get("version") == Some(&"2") && !fields.contains_key("canonical_model") {
            return Err(message("queue version and canonical binding conflict"));
        }
        let version = fields.get("version").copied().unwrap_or_default();
        let task_id = fields
            .get("task_id")
            .copied()
            .unwrap_or_default()
            .to_owned();
        if !valid_id(&task_id) {
            return Err(message(format!("invalid queue task id: {task_id}")));
        }
        if version != "3" && task_id != expected_id {
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
        if !matches!(kind.as_str(), "delivery" | "scout" | "daemon") {
            return Err(message(format!(
                "queue record has invalid kind: {}",
                path.display()
            )));
        }
        let mode = fields.get("mode").copied().unwrap_or_default().to_owned();
        let yolo = fields.get("yolo").copied().unwrap_or_default().to_owned();
        if !matches!(
            mode.as_str(),
            "" | "deep-review" | "direct-PR" | "local-only" | "daemon"
        ) || !matches!(yolo.as_str(), "" | "on" | "off")
        {
            return Err(message("queue record has invalid delivery authority"));
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
        let request_id = fields
            .get("request_id")
            .map_or_else(|| task_id.clone(), |value| (*value).to_owned());
        if !valid_id(&request_id) {
            return Err(message("invalid queue request id"));
        }
        if version == "3" && request_id != expected_id {
            return Err(message(format!(
                "queue request identity does not match its path: {}",
                path.display()
            )));
        }
        let path_field = |name: &str| -> Result<PathBuf> {
            let value = fields.get(name).copied().unwrap_or_default();
            if version == "3" && (value.is_empty() || !Path::new(value).is_absolute()) {
                return Err(message(format!("queue {name} must be absolute")));
            }
            Ok(PathBuf::from(value))
        };
        let priority = fields
            .get("priority")
            .map(|value| value.parse::<i32>())
            .transpose()
            .map_err(|_| message("queue priority is invalid"))?
            .unwrap_or_default();
        let dependencies = fields
            .get("dependencies")
            .map(|value| serde_json::from_str::<Vec<Dependency>>(value))
            .transpose()
            .map_err(|_| message("queue dependencies are unreadable"))?
            .unwrap_or_default();
        let mut resources = fields
            .get("resources")
            .map(|value| serde_json::from_str::<BTreeMap<String, u64>>(value))
            .transpose()
            .map_err(|_| message("queue resources are unreadable"))?
            .unwrap_or_default();
        if resources.is_empty() {
            resources.insert("session".into(), 1);
        }
        let state = match fields.get("state").copied().unwrap_or("queued") {
            "queued" => DispatchState::Queued,
            "dispatching" => DispatchState::Dispatching,
            _ => return Err(message("queue dispatch state is invalid")),
        };
        let dispatch_started_at = fields
            .get("dispatch_started_at")
            .filter(|value| !value.is_empty())
            .map(|value| {
                parse_nonnegative_integer(value)
                    .ok_or_else(|| message("queue dispatch start is invalid"))
            })
            .transpose()?;
        if state == DispatchState::Dispatching && dispatch_started_at.is_none() {
            return Err(message("dispatching queue record lacks its durable start"));
        }
        Ok(Self {
            request_id,
            task_id,
            root_home: path_field("root_home")?,
            owner_home: path_field("owner_home")?,
            owner_state: path_field("owner_state")?,
            parent_task_id: fields
                .get("parent_task_id")
                .copied()
                .unwrap_or_default()
                .to_owned(),
            parent_home: path_field("parent_home")?,
            parent_state: path_field("parent_state")?,
            project,
            harness,
            model,
            effort,
            backend,
            kind,
            mode,
            yolo,
            enqueued_at,
            priority,
            dependencies,
            resources,
            state,
            dispatch_started_at,
            canonical_model: fields
                .get("canonical_model")
                .map(|value| (*value).to_owned()),
        })
    }
}

/// Read the frozen task binding for idempotent queue submission.
pub fn queued_model(paths: &HeadroomPaths, id: &str) -> Result<Option<String>> {
    if !valid_id(id) {
        return Err(message("invalid queue task id"));
    }
    let path = paths.queue_dir().join(format!("{id}.request"));
    if !path.exists() {
        return Ok(None);
    }
    QueueRecord::parse(&path, id).map(|record| record.canonical_model)
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
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            record.enqueued_at,
            record.task_id,
            record.project,
            dash(&record.harness),
            dash(&record.model),
            dash(&record.effort),
            dash(&record.backend),
            record.kind,
            record.request_id,
            record.priority,
            match record.state {
                DispatchState::Queued => "queued",
                DispatchState::Dispatching => "dispatching",
            }
        ));
    }
    Ok(output)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionConfig {
    version: u32,
    #[serde(default = "default_aging_seconds")]
    aging_seconds: u64,
    #[serde(default)]
    resources: BTreeMap<String, u64>,
    /// Session slots kept available to useful workers and parent recovery.
    #[serde(default = "default_worker_headroom")]
    worker_headroom: u64,
}

const fn default_aging_seconds() -> u64 {
    DEFAULT_AGING_SECONDS
}

const fn default_worker_headroom() -> u64 {
    1
}

impl Default for AdmissionConfig {
    fn default() -> Self {
        Self {
            version: 1,
            aging_seconds: DEFAULT_AGING_SECONDS,
            resources: BTreeMap::new(),
            worker_headroom: default_worker_headroom(),
        }
    }
}

fn admission_config(paths: &HeadroomPaths) -> Result<AdmissionConfig> {
    let path = paths.config.join("admission-capacity.json");
    if !path.exists() {
        return Ok(AdmissionConfig::default());
    }
    let bytes = read_bounded_regular(&path, RECORD_LIMIT)
        .map_err(|_| message("configured admission capacity is unreadable"))?;
    let config: AdmissionConfig = serde_json::from_slice(&bytes)
        .map_err(|_| message("configured admission capacity is unreadable"))?;
    if config.version != 1
        || config.aging_seconds == 0
        || config.resources.iter().any(|(name, capacity)| {
            !valid_resource(name)
                || *capacity == 0
                || name == "session"
                || name.starts_with("harness:")
        })
    {
        return Err(message("configured admission capacity is invalid"));
    }
    Ok(config)
}

fn valid_resource(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn validate_record(paths: &HeadroomPaths, record: &QueueRecord) -> Result<()> {
    if !valid_id(&record.request_id) || !valid_id(&record.task_id) {
        return Err(message("invalid queue request or task id"));
    }
    one_line("project", &record.project)?;
    if !record.root_home.as_os_str().is_empty() && !same_path(&record.root_home, &paths.root_home) {
        return Err(message("queued admission belongs to another root home"));
    }
    for (label, path) in [
        ("root home", &record.root_home),
        ("owner home", &record.owner_home),
        ("owner state", &record.owner_state),
        ("parent home", &record.parent_home),
        ("parent state", &record.parent_state),
    ] {
        if !path.as_os_str().is_empty() && !path.is_absolute() {
            return Err(message(format!("queue {label} must be absolute")));
        }
    }
    let root_parent = format!("root-home:{}", record.root_home.display());
    if !record.parent_task_id.is_empty()
        && !valid_id(&record.parent_task_id)
        && record.parent_task_id != root_parent
    {
        return Err(message("invalid queue parent task id"));
    }
    if record.resources.is_empty()
        || record
            .resources
            .iter()
            .any(|(name, units)| !valid_resource(name) || *units == 0)
    {
        return Err(message("queue resources are invalid"));
    }
    let mut dependencies = std::collections::BTreeSet::new();
    if record.dependencies.iter().any(|dependency| {
        !valid_id(&dependency.task_id)
            || !dependency.owner_state.is_absolute()
            || (dependency.task_id == record.task_id
                && dependency.owner_state == record.owner_state)
            || !dependencies.insert((dependency.owner_state.clone(), dependency.task_id.clone()))
    }) {
        return Err(message(
            "queue dependencies contain a duplicate, self edge or invalid identity",
        ));
    }
    if !matches!(
        record.mode.as_str(),
        "" | "deep-review" | "direct-PR" | "local-only" | "daemon"
    ) || !matches!(record.yolo.as_str(), "" | "on" | "off")
    {
        return Err(message("queue record has invalid delivery authority"));
    }
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
    if !matches!(record.kind.as_str(), "delivery" | "scout" | "daemon") {
        return Err(message(format!(
            "queue record has invalid kind: {}",
            record.kind
        )));
    }
    Ok(())
}

fn qualified_dependency(state: &Path, task: &str) -> String {
    format!("{}#task:{task}", state.display())
}

fn validate_dependency_graph(records: &[QueueRecord]) -> Result<()> {
    let graph = records
        .iter()
        .map(|record| {
            (
                qualified_dependency(&record.owner_state, &record.task_id),
                record
                    .dependencies
                    .iter()
                    .map(|dependency| {
                        qualified_dependency(&dependency.owner_state, &dependency.task_id)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    fn visit(
        node: &str,
        graph: &BTreeMap<String, Vec<String>>,
        visiting: &mut std::collections::BTreeSet<String>,
        visited: &mut std::collections::BTreeSet<String>,
    ) -> bool {
        if visited.contains(node) {
            return true;
        }
        if !visiting.insert(node.to_owned()) {
            return false;
        }
        if graph.get(node).is_some_and(|edges| {
            edges
                .iter()
                .any(|edge| graph.contains_key(edge) && !visit(edge, graph, visiting, visited))
        }) {
            return false;
        }
        visiting.remove(node);
        visited.insert(node.to_owned());
        true
    }
    let mut visited = std::collections::BTreeSet::new();
    for node in graph.keys() {
        if !visit(
            node,
            &graph,
            &mut std::collections::BTreeSet::new(),
            &mut visited,
        ) {
            return Err(message("cyclic queued dependencies"));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AdmissionState {
    Reserved,
    Active,
    Uncertain,
    Retryable,
    Released,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AdmissionReceipt {
    pub version: u32,
    pub request_id: String,
    pub task_id: String,
    pub root_home: PathBuf,
    pub owner_home: PathBuf,
    pub owner_state: PathBuf,
    pub attempt_id: String,
    pub backend: String,
    /// Exact process lifetime that currently owns a reservation. Active and
    /// terminal dispositions retain it as audit evidence.
    pub owner: Option<ProcessIdentity>,
    pub resources: BTreeMap<String, u64>,
    pub state: AdmissionState,
    pub endpoint: Option<String>,
    pub allocation_id: Option<String>,
    pub reason: Option<String>,
    pub updated_at: u64,
}

fn receipt_path(paths: &HeadroomPaths, request_id: &str) -> Result<PathBuf> {
    if !valid_id(request_id) {
        return Err(message("invalid admission request id"));
    }
    Ok(paths.admissions_dir().join(format!("{request_id}.json")))
}

fn read_receipt(paths: &HeadroomPaths, request_id: &str) -> Result<Option<AdmissionReceipt>> {
    let path = receipt_path(paths, request_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = read_bounded_regular(&path, RECORD_LIMIT)
        .map_err(|_| message("admission receipt is unreadable; retained"))?;
    let receipt: AdmissionReceipt = serde_json::from_slice(&bytes)
        .map_err(|error| message(format!("admission receipt is corrupt; retained: {error}")))?;
    if receipt.version != 1
        || receipt.request_id != request_id
        || !same_path(&receipt.root_home, &paths.root_home)
        || receipt.resources.is_empty()
    {
        return Err(message("admission receipt identity is invalid; retained"));
    }
    Ok(Some(receipt))
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    fs::canonicalize(left)
        .ok()
        .zip(fs::canonicalize(right).ok())
        .is_some_and(|(canonical_left, canonical_right)| canonical_left == canonical_right)
}

fn write_receipt(paths: &HeadroomPaths, receipt: &AdmissionReceipt) -> Result<()> {
    fs::create_dir_all(paths.admissions_dir())?;
    fs::set_permissions(paths.admissions_dir(), fs::Permissions::from_mode(0o700))?;
    atomic_replace(
        receipt_path(paths, &receipt.request_id)?,
        &serde_json::to_vec(receipt).map_err(|_| message("admission receipt cannot be encoded"))?,
        0o600,
    )
    .map_err(|error| message(error.to_string()))
}

fn attempt_id(record: &QueueRecord) -> Result<String> {
    let model = record
        .canonical_model
        .as_deref()
        .ok_or_else(|| message("canonical admission lacks task binding"))?;
    serde_json::from_str::<Value>(model)
        .ok()
        .and_then(|value| {
            value
                .pointer("/attempt/id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| message("canonical admission lacks attempt identity"))
}

pub fn queue_add(paths: &HeadroomPaths, record: &QueueRecord) -> Result<String> {
    let mut record = record.clone();
    if record.root_home.as_os_str().is_empty() {
        record.root_home = paths.root_home.clone();
    }
    if record.owner_home.as_os_str().is_empty() {
        record.owner_home = paths.root_home.clone();
    }
    if record.owner_state.as_os_str().is_empty() {
        record.owner_state = paths.state.clone();
    }
    if record.parent_home.as_os_str().is_empty() {
        record.parent_home = record.owner_home.clone();
    }
    if record.parent_state.as_os_str().is_empty() {
        record.parent_state = record.owner_state.clone();
    }
    if record.resources.is_empty() {
        record.resources.insert("session".into(), 1);
    }
    validate_record(paths, &record)?;
    let _lock = acquire(paths)?;
    let directory = paths.queue_dir();
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    let path = directory.join(format!("{}.request", record.request_id));
    if path.exists() {
        let existing = QueueRecord::parse(&path, &record.request_id)?;
        let mut equivalent = record.clone();
        equivalent.enqueued_at = existing.enqueued_at;
        equivalent.state = existing.state;
        equivalent.dispatch_started_at = existing.dispatch_started_at;
        if existing == equivalent {
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
    let mut records = queue_records(paths)?;
    records.push(record.clone());
    validate_dependency_graph(&records)?;
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
    let record = QueueRecord::parse(&path, id)?;
    if record.state != DispatchState::Queued {
        return Err(message(format!(
            "queued dispatch {id} is already dispatching; reconcile it before cancellation"
        )));
    }
    fs::remove_file(path)?;
    Ok(format!("cancelled: {id}\n"))
}

pub fn queue_priority(paths: &HeadroomPaths, id: &str, priority: i32) -> Result<String> {
    if !valid_id(id) {
        return Err(message("invalid queue task id"));
    }
    let _lock = acquire(paths)?;
    let path = paths.queue_dir().join(format!("{id}.request"));
    let mut record = QueueRecord::parse(&path, id)?;
    if record.state != DispatchState::Queued {
        return Err(message("cannot reprioritize a dispatch already in flight"));
    }
    record.priority = priority;
    atomic_replace(path, &record.render(), 0o600).map_err(|error| message(error.to_string()))?;
    Ok(format!("priority: {id}={priority}\n"))
}

fn canonical_identity(model: &str) -> Option<Vec<Value>> {
    let value: Value = serde_json::from_str(model).ok()?;
    let pointers = [
        "/schema_version",
        "/task_id",
        "/parent_id",
        "/root_id",
        "/owner_home",
        "/owner_state",
        "/parent_home",
        "/parent_state",
        "/attempt/id",
        "/attempt/generation",
        "/attempt/brief_revision",
        "/accepted_brief_revision",
        "/accepted_brief_digest",
        "/accepted_brief_path",
        "/project/project_id",
        "/project/checkout_id",
        "/project/common_git_identity",
        "/project/canonical_path",
        "/project/starting_revision",
    ];
    pointers
        .iter()
        .map(|pointer| value.pointer(pointer).cloned())
        .collect::<Option<Vec<_>>>()
}

fn exact_metadata_value<'a>(text: &'a str, key: &str) -> Result<Option<&'a str>> {
    let mut values = text.lines().filter_map(|line| {
        line.split_once('=')
            .filter(|(candidate, _)| *candidate == key)
            .map(|(_, value)| value)
    });
    let value = values.next();
    if values.next().is_some() {
        return Err(message(format!("spawn metadata repeats {key}; retained")));
    }
    Ok(value)
}

fn metadata_reconciled(record: &QueueRecord) -> Result<Option<(String, Option<String>)>> {
    let path = record.owner_state.join(format!("{}.meta", record.task_id));
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(None);
    };
    let model = exact_metadata_value(&text, "canonical_model")?
        .ok_or_else(|| message("spawn metadata lacks canonical identity; retained"))?;
    let expected = record
        .canonical_model
        .as_deref()
        .and_then(canonical_identity)
        .ok_or_else(|| message("queued dispatch lacks canonical identity; retained"))?;
    if canonical_identity(model).as_ref() != Some(&expected) {
        return Err(message(
            "spawn metadata belongs to another task attempt; retained",
        ));
    }
    let value: Value = serde_json::from_str(model)
        .map_err(|_| message("spawn metadata canonical identity is unreadable; retained"))?;
    let model_path_matches = |field: &str, expected: &Path| {
        value
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|actual| same_path(Path::new(actual), expected))
    };
    if value.get("task_id").and_then(Value::as_str) != Some(record.task_id.as_str())
        || !model_path_matches("owner_home", &record.owner_home)
        || !model_path_matches("owner_state", &record.owner_state)
        || value.get("parent_id").and_then(Value::as_str) != Some(record.parent_task_id.as_str())
        || !model_path_matches("parent_home", &record.parent_home)
        || !model_path_matches("parent_state", &record.parent_state)
    {
        return Err(message("spawn metadata routing identity changed; retained"));
    }
    let endpoint = exact_metadata_value(&text, "window")?.filter(|value| !value.is_empty());
    let allocation = serde_json::from_str::<Value>(model).ok().and_then(|value| {
        value
            .pointer("/allocation/allocation_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let backend = exact_metadata_value(&text, "backend")?.unwrap_or(record.backend.as_str());
    if endpoint.is_some_and(|target| target_is_live(backend, target)) {
        return Ok(Some((
            endpoint.expect("checked endpoint").to_owned(),
            allocation,
        )));
    }
    Ok(None)
}

fn dependency_completed(dependency: &Dependency) -> bool {
    let path = dependency
        .owner_state
        .join(format!("{}.meta", dependency.task_id));
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let mut models = text
        .lines()
        .filter_map(|line| line.strip_prefix("canonical_model="));
    let model = models.next();
    if model.is_none() || models.next().is_some() {
        return false;
    }
    model
        .and_then(|model| serde_json::from_str::<Value>(model).ok())
        .and_then(|value| {
            value
                .pointer("/schedule/state")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        == Some("completed")
}

fn dispatch_score(record: &QueueRecord, now: u64, aging_seconds: u64) -> i64 {
    let age = now.saturating_sub(record.enqueued_at) / aging_seconds;
    i64::from(record.priority).saturating_add(i64::try_from(age).unwrap_or(i64::MAX))
}

fn receipts(paths: &HeadroomPaths) -> Result<Vec<AdmissionReceipt>> {
    let entries = match fs::read_dir(paths.admissions_dir()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut result = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                message(format!(
                    "admission receipt path is unsafe: {}",
                    path.display()
                ))
            })?;
        if let Some(receipt) = read_receipt(paths, id)? {
            result.push(receipt);
        }
    }
    Ok(result)
}

fn resource_use(
    paths: &HeadroomPaths,
    observed_active_receipts: &BTreeSet<String>,
) -> Result<BTreeMap<String, u64>> {
    let mut used = BTreeMap::<String, u64>::new();
    for receipt in receipts(paths)? {
        if matches!(
            receipt.state,
            AdmissionState::Released | AdmissionState::Retryable
        ) {
            continue;
        }
        for (resource, units) in receipt.resources {
            // Root-wide live counts include canonical active session and
            // harness receipts. Configured custom resources have no native
            // observer, so active receipts continue to count them here.
            if receipt.state == AdmissionState::Active
                && observed_active_receipts.contains(&receipt.request_id)
                && (resource == "session" || resource.starts_with("harness:"))
            {
                continue;
            }
            let entry = used.entry(resource).or_default();
            *entry = entry.saturating_add(units);
        }
    }
    Ok(used)
}

fn is_coordinator(record: &QueueRecord) -> bool {
    record
        .canonical_model
        .as_deref()
        .and_then(|model| serde_json::from_str::<Value>(model).ok())
        .and_then(|model| model.get("role").and_then(Value::as_str).map(str::to_owned))
        .as_deref()
        == Some("sub-orchestrator")
}

fn aged_coordinator_may_yield_worker_reserve(
    record: &QueueRecord,
    headroom: &Headroom,
    now: u64,
    aging_seconds: u64,
) -> bool {
    is_coordinator(record)
        && headroom.coordinators_in_use == 0
        && now.saturating_sub(record.enqueued_at) >= aging_seconds
}

fn fits(
    record: &QueueRecord,
    headroom: &Headroom,
    config: &AdmissionConfig,
    used: &BTreeMap<String, u64>,
    allow_worker_headroom: bool,
) -> bool {
    let coordinator = is_coordinator(record);
    record.resources.iter().all(|(resource, units)| {
        let capacity = if resource == "session" {
            if coordinator && !allow_worker_headroom {
                headroom.available.saturating_sub(
                    config
                        .worker_headroom
                        .saturating_sub(headroom.workers_in_use),
                )
            } else {
                headroom.available
            }
        } else if let Some(harness) = resource.strip_prefix("harness:") {
            headroom
                .candidates
                .get(harness)
                .map(|candidate| candidate.available)
                // Native/provider limits that are not represented in the
                // configured snapshot are opaque. The shared session budget
                // still applies; inventing a zero provider limit would strand
                // otherwise runnable work.
                .unwrap_or(u64::MAX)
        } else {
            config.resources.get(resource).copied().unwrap_or(u64::MAX)
        };
        used.get(resource)
            .copied()
            .unwrap_or_default()
            .saturating_add(*units)
            <= capacity
    })
}

fn absolute_ps_identity(pid: u32) -> Result<ProcessIdentity> {
    if pid == 0 {
        return Err(message("invalid admission owner PID"));
    }
    let ps = [Path::new("/bin/ps"), Path::new("/usr/bin/ps")]
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| message("system process table reader is unavailable"))?;
    let output = Command::new(ps)
        .args(["-p", &pid.to_string(), "-o", "lstart=", "-o", "command="])
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() || output.stdout.len() > RECORD_LIMIT {
        return Err(message("cannot read admission owner process identity"));
    }
    let marker = String::from_utf8(output.stdout)
        .map_err(|_| message("admission owner process identity is not UTF-8"))?
        .trim_start()
        .trim_end_matches('\n')
        .to_owned();
    if marker.is_empty() {
        return Err(message("admission owner process is absent"));
    }
    Ok(ProcessIdentity { pid, marker })
}

fn admission_owner_identity(pid: u32) -> Result<ProcessIdentity> {
    SystemProcessProbe::default()
        .identity(pid)
        .map_err(|error| message(format!("cannot identify admission owner: {error}")))
        .or_else(|_| absolute_ps_identity(pid))
}

fn same_receipt_identity(existing: &AdmissionReceipt, expected: &AdmissionReceipt) -> bool {
    existing.version == expected.version
        && existing.request_id == expected.request_id
        && existing.task_id == expected.task_id
        && same_path(&existing.root_home, &expected.root_home)
        && same_path(&existing.owner_home, &expected.owner_home)
        && same_path(&existing.owner_state, &expected.owner_state)
        && existing.attempt_id == expected.attempt_id
        && existing.backend == expected.backend
        && existing.resources == expected.resources
}

fn reserve(paths: &HeadroomPaths, record: &QueueRecord, now: u64) -> Result<()> {
    let process = admission_owner_identity(std::process::id())?;
    let receipt = AdmissionReceipt {
        version: 1,
        request_id: record.request_id.clone(),
        task_id: record.task_id.clone(),
        root_home: paths.root_home.clone(),
        owner_home: record.owner_home.clone(),
        owner_state: record.owner_state.clone(),
        attempt_id: attempt_id(record)?,
        backend: record.backend.clone(),
        owner: Some(process),
        resources: record.resources.clone(),
        state: AdmissionState::Reserved,
        endpoint: None,
        allocation_id: None,
        reason: None,
        updated_at: now,
    };
    if let Some(mut existing) = read_receipt(paths, &record.request_id)? {
        if !same_receipt_identity(&existing, &receipt) {
            return Err(message(
                "admission request conflicts with its durable receipt",
            ));
        }
        if existing.state == AdmissionState::Retryable {
            existing.state = AdmissionState::Reserved;
            existing.reason = None;
            existing.updated_at = now;
            write_receipt(paths, &existing)?;
        }
        return Ok(());
    }
    write_receipt(paths, &receipt)
}

fn reservation_owner_is_live(receipt: &AdmissionReceipt, processes: &impl ProcessProbe) -> bool {
    receipt.owner.as_ref().is_some_and(|owner| {
        processes.is_alive(owner.pid)
            && processes
                .identity(owner.pid)
                .is_ok_and(|current| current == *owner)
    })
}

fn reservation_owner_is_live_on_host(receipt: &AdmissionReceipt) -> bool {
    if reservation_owner_is_live(receipt, &SystemProcessProbe::default()) {
        return true;
    }
    receipt.owner.as_ref().is_some_and(|owner| {
        admission_owner_identity(owner.pid).is_ok_and(|current| current == *owner)
    })
}

fn claim_reservation_owner(paths: &HeadroomPaths, request_id: &str) -> Result<()> {
    let mut receipt = read_receipt(paths, request_id)?
        .ok_or_else(|| message("admission receipt missing; retained"))?;
    receipt.owner = Some(admission_owner_identity(std::process::id())?);
    receipt.updated_at = now_epoch();
    write_receipt(paths, &receipt)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionDecision {
    Granted,
    AlreadyActive,
    Deferred,
}

fn existing_admission_decision(
    state: AdmissionState,
    allow_reserved: bool,
) -> Result<AdmissionDecision> {
    match state {
        AdmissionState::Active => Ok(AdmissionDecision::AlreadyActive),
        AdmissionState::Retryable => Ok(AdmissionDecision::Granted),
        AdmissionState::Reserved if allow_reserved => Ok(AdmissionDecision::Granted),
        AdmissionState::Reserved => Err(message(
            "admission request is already reserved by an in-progress launch",
        )),
        AdmissionState::Uncertain => Err(message(
            "admission request has an uncertain endpoint; reconcile it before retrying",
        )),
        AdmissionState::Released => Err(message(
            "released admission request cannot be replayed; use a new request identity",
        )),
    }
}

fn admission_try_reserve_inner(
    paths: &HeadroomPaths,
    record: &QueueRecord,
    allow_reserved: bool,
) -> Result<AdmissionDecision> {
    validate_record(paths, record)?;
    if !record.dependencies.iter().all(dependency_completed) {
        return Ok(AdmissionDecision::Deferred);
    }
    let headroom = evaluate(paths)?;
    let config = admission_config(paths)?;
    admission_try_reserve_evaluated(paths, record, allow_reserved, &headroom, &config)
}

fn admission_try_reserve_evaluated(
    paths: &HeadroomPaths,
    record: &QueueRecord,
    allow_reserved: bool,
    headroom: &Headroom,
    config: &AdmissionConfig,
) -> Result<AdmissionDecision> {
    validate_record(paths, record)?;
    if !record.dependencies.iter().all(dependency_completed) {
        return Ok(AdmissionDecision::Deferred);
    }
    let _lock = acquire(paths)?;
    if let Some(existing) = read_receipt(paths, &record.request_id)? {
        let abandoned_reservation = existing.state == AdmissionState::Reserved
            && existing.owner.is_some()
            && !reservation_owner_is_live_on_host(&existing);
        let decision = existing_admission_decision(
            existing.state.clone(),
            allow_reserved || abandoned_reservation,
        )?;
        reserve(paths, record, now_epoch())?;
        if decision == AdmissionDecision::Granted {
            claim_reservation_owner(paths, &record.request_id)?;
        }
        return Ok(decision);
    }
    let used = resource_use(paths, &headroom.observed_active_receipts)?;
    if !fits(record, headroom, config, &used, false) {
        return Ok(AdmissionDecision::Deferred);
    }
    if !allow_reserved {
        let now = now_epoch();
        let incoming_score = dispatch_score(record, now, config.aging_seconds);
        for candidate in queue_records(paths)? {
            if candidate.request_id == record.request_id {
                continue;
            }
            let eligible = candidate.state == DispatchState::Queued
                || read_receipt(paths, &candidate.request_id)?
                    .is_some_and(|receipt| receipt.state == AdmissionState::Retryable);
            let candidate_score = dispatch_score(&candidate, now, config.aging_seconds);
            let candidate_precedes = candidate_score > incoming_score
                || (candidate_score == incoming_score
                    && (candidate.enqueued_at, &candidate.task_id)
                        <= (record.enqueued_at, &record.task_id));
            if eligible
                && candidate_precedes
                && candidate.dependencies.iter().all(dependency_completed)
                && fits(
                    &candidate,
                    headroom,
                    config,
                    &used,
                    aged_coordinator_may_yield_worker_reserve(
                        &candidate,
                        headroom,
                        now,
                        config.aging_seconds,
                    ),
                )
            {
                return Ok(AdmissionDecision::Deferred);
            }
        }
    }
    reserve(paths, record, now_epoch())?;
    Ok(AdmissionDecision::Granted)
}

/// Atomically reserve root-scoped resources for an immediate launch. Resource
/// probes run before the lock; the reservation is then rechecked against all
/// durable receipts so competing homes cannot both spend the final unit.
pub fn admission_try_reserve(
    paths: &HeadroomPaths,
    record: &QueueRecord,
) -> Result<AdmissionDecision> {
    admission_try_reserve_inner(paths, record, false)
}

/// Continue the reservation created by `queue_drain` in the child launch.
/// Only the exact request/binding may consume a pre-existing reservation.
pub fn admission_resume_reserved(
    paths: &HeadroomPaths,
    record: &QueueRecord,
) -> Result<AdmissionDecision> {
    admission_try_reserve_inner(paths, record, true)
}

pub fn admission_finish(
    paths: &HeadroomPaths,
    request_id: &str,
    endpoint: Option<String>,
    allocation_id: Option<String>,
    error: Option<String>,
) -> Result<()> {
    let _lock = acquire(paths)?;
    let mut receipt = read_receipt(paths, request_id)?
        .ok_or_else(|| message("admission receipt missing; retained"))?;
    receipt.updated_at = now_epoch();
    receipt.endpoint = endpoint.or(receipt.endpoint);
    receipt.allocation_id = allocation_id.or(receipt.allocation_id);
    if error.is_none() {
        receipt.state = AdmissionState::Active;
        receipt.reason = None;
    } else if receipt.state != AdmissionState::Retryable {
        receipt.state = AdmissionState::Uncertain;
        receipt.reason = error;
    }
    write_receipt(paths, &receipt)
}

/// Mark a failed launch retryable only with exact task-attempt evidence and a
/// verified absent endpoint. The retained allocation remains on the receipt.
pub fn admission_retryable(
    paths: &HeadroomPaths,
    request_id: &str,
    task_id: &str,
    owner_state: &Path,
    attempt_id: &str,
    reason: &str,
) -> Result<()> {
    let _lock = acquire(paths)?;
    let mut receipt =
        read_receipt(paths, request_id)?.ok_or_else(|| message("admission receipt missing"))?;
    if receipt.task_id != task_id
        || receipt.owner_state != owner_state
        || receipt.attempt_id != attempt_id
    {
        return Err(message(
            "admission retry evidence does not match its receipt",
        ));
    }
    receipt.state = AdmissionState::Retryable;
    receipt.updated_at = now_epoch();
    receipt.reason = Some(reason.into());
    write_receipt(paths, &receipt)
}

/// Release the unique admission proven to own the retired task attempt and
/// endpoint. This prevents a task-id-only cleanup from freeing another home.
pub fn admission_release_execution(
    paths: &HeadroomPaths,
    task_id: &str,
    owner_state: &Path,
    attempt_id: &str,
    endpoint: &str,
) -> Result<bool> {
    let _lock = acquire(paths)?;
    let mut matches = receipts(paths)?
        .into_iter()
        .filter(|receipt| {
            receipt.task_id == task_id
                && receipt.owner_state == owner_state
                && receipt.attempt_id == attempt_id
                && receipt.endpoint.as_deref() == Some(endpoint)
                && matches!(
                    receipt.state,
                    AdmissionState::Active | AdmissionState::Uncertain
                )
        })
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(message(
            "multiple admissions claim the retired execution; retained",
        ));
    }
    let Some(mut receipt) = matches.pop() else {
        return Ok(false);
    };
    receipt.state = AdmissionState::Released;
    receipt.updated_at = now_epoch();
    receipt.reason = Some("execution released by its lifecycle owner".into());
    write_receipt(paths, &receipt)?;
    Ok(true)
}

/// Reclaim active or uncertain receipts only when exact task metadata still
/// binds the same attempt/backend/endpoint and the owning adapter reports that
/// endpoint authoritatively missing. Provider errors remain uncertain.
pub fn admission_reconcile_inactive(paths: &HeadroomPaths) -> Result<usize> {
    admission_reconcile_inactive_with_probe(paths, &|backend, endpoint| {
        crate::facade::observe_endpoint(backend, endpoint, None, false)
            .ok()
            .map(|observation| observation.exists)
    })
}

fn admission_reconcile_inactive_with_probe(
    paths: &HeadroomPaths,
    endpoint_exists: &dyn Fn(&str, &str) -> Option<bool>,
) -> Result<usize> {
    let mut absent = Vec::new();
    for receipt in receipts(paths)? {
        if !matches!(
            receipt.state,
            AdmissionState::Active | AdmissionState::Uncertain
        ) {
            continue;
        }
        let Some(endpoint) = receipt.endpoint.as_deref() else {
            continue;
        };
        let path = receipt
            .owner_state
            .join(format!("{}.meta", receipt.task_id));
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let model = exact_metadata_value(&text, "canonical_model")?
            .ok_or_else(|| message("admission metadata lacks canonical identity; retained"))?;
        let value: Value = serde_json::from_str(model)
            .map_err(|_| message("admission metadata canonical identity is invalid; retained"))?;
        let expected_root = format!("root-home:{}", paths.root_home.display());
        if value.get("task_id").and_then(Value::as_str) != Some(receipt.task_id.as_str())
            || value.get("root_id").and_then(Value::as_str) != Some(expected_root.as_str())
            || value.get("owner_home").and_then(Value::as_str) != receipt.owner_home.to_str()
            || value.get("owner_state").and_then(Value::as_str) != receipt.owner_state.to_str()
            || value.pointer("/attempt/id").and_then(Value::as_str)
                != Some(receipt.attempt_id.as_str())
            || exact_metadata_value(&text, "window")? != Some(endpoint)
        {
            return Err(message(
                "admission metadata does not match its durable receipt; retained",
            ));
        }
        let backend = exact_metadata_value(&text, "backend")?.unwrap_or("tmux");
        let receipt_backend = if receipt.backend.is_empty() {
            "tmux"
        } else {
            receipt.backend.as_str()
        };
        if backend != receipt_backend {
            return Err(message("admission backend changed; retained"));
        }
        if endpoint_exists(backend, endpoint).is_some_and(|exists| !exists) {
            absent.push((
                receipt.task_id,
                receipt.owner_state,
                receipt.attempt_id,
                endpoint.to_owned(),
            ));
        }
    }
    let mut released = 0;
    for (task, state, attempt, endpoint) in absent {
        if admission_release_execution(paths, &task, &state, &attempt, &endpoint)? {
            released += 1;
        }
    }
    Ok(released)
}

fn admission_owner_is_authoritatively_gone(owner: &ProcessIdentity) -> bool {
    #[cfg(target_os = "linux")]
    {
        let proc = PathBuf::from(format!("/proc/{}", owner.pid));
        match fs::symlink_metadata(&proc) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Err(_) => false,
            Ok(_) => admission_owner_identity(owner.pid).is_ok_and(|current| current != *owner),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let Some(ps) = [Path::new("/bin/ps"), Path::new("/usr/bin/ps")]
            .into_iter()
            .find(|path| path.is_file())
        else {
            return false;
        };
        let Ok(output) = Command::new(ps)
            .args(["-p", &owner.pid.to_string(), "-o", "pid="])
            .env("LC_ALL", "C")
            .output()
        else {
            return false;
        };
        if output.stdout.len() > RECORD_LIMIT
            || (!output.status.success() && output.status.code() != Some(1))
        {
            return false;
        }
        if String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            return output.status.code() == Some(1);
        }
        admission_owner_identity(owner.pid).is_ok_and(|current| current != *owner)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = owner;
        false
    }
}

fn path_is_absent(path: &Path) -> bool {
    matches!(
        fs::symlink_metadata(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound
    )
}

fn dispatch_proves_never_started(paths: &HeadroomPaths, record: &QueueRecord) -> Result<bool> {
    if record.state != DispatchState::Dispatching {
        return Ok(false);
    }
    validate_record(paths, record)?;
    let Some(receipt) = read_receipt(paths, &record.request_id)? else {
        return Ok(false);
    };
    let Some(model) = record
        .canonical_model
        .as_deref()
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
    else {
        return Ok(false);
    };
    let expected_root = format!("root-home:{}", record.root_home.display());
    if model.get("task_id").and_then(Value::as_str) != Some(record.task_id.as_str())
        || model.get("parent_id").and_then(Value::as_str) != Some(record.parent_task_id.as_str())
        || model.get("root_id").and_then(Value::as_str) != Some(expected_root.as_str())
        || model
            .get("owner_home")
            .and_then(Value::as_str)
            .is_none_or(|value| !same_path(Path::new(value), &record.owner_home))
        || model
            .get("owner_state")
            .and_then(Value::as_str)
            .is_none_or(|value| !same_path(Path::new(value), &record.owner_state))
        || model
            .get("parent_home")
            .and_then(Value::as_str)
            .is_none_or(|value| !same_path(Path::new(value), &record.parent_home))
        || model
            .get("parent_state")
            .and_then(Value::as_str)
            .is_none_or(|value| !same_path(Path::new(value), &record.parent_state))
        || model
            .pointer("/allocation/allocation_id")
            .and_then(Value::as_str)
            .is_some()
    {
        return Ok(false);
    }
    if !matches!(
        receipt.state,
        AdmissionState::Uncertain | AdmissionState::Retryable
    ) || receipt.endpoint.is_some()
        || receipt.allocation_id.is_some()
        || receipt.task_id != record.task_id
        || !same_path(&receipt.root_home, &record.root_home)
        || !same_path(&receipt.owner_home, &record.owner_home)
        || !same_path(&receipt.owner_state, &record.owner_state)
        || receipt.attempt_id != attempt_id(record)?
        || receipt.resources != record.resources
        || receipt.backend != record.backend
        || !receipt
            .owner
            .as_ref()
            .is_some_and(admission_owner_is_authoritatively_gone)
    {
        return Ok(false);
    }
    let action = record
        .owner_state
        .join(".spawn-actions")
        .join(format!("{}.json", record.request_id));
    let task_lock = record
        .owner_state
        .join(format!(".spawn-{}.lock", record.task_id));
    let action_lock = record
        .owner_state
        .join(format!(".spawn-action-{}.lock", record.request_id));
    let intent = record
        .owner_state
        .join(format!(".spawn-{}.intent", record.task_id));
    let metadata = record.owner_state.join(format!("{}.meta", record.task_id));
    Ok([action, task_lock, action_lock, intent, metadata]
        .iter()
        .all(|path| path_is_absent(path)))
}

fn recover_unstarted_dispatches(paths: &HeadroomPaths) -> Result<usize> {
    let _lock = acquire(paths)?;
    let mut recovered = 0;
    for mut record in queue_records(paths)? {
        if !dispatch_proves_never_started(paths, &record)? {
            continue;
        }
        let mut receipt = read_receipt(paths, &record.request_id)?
            .ok_or_else(|| message("admission receipt disappeared during recovery; retained"))?;
        receipt.state = AdmissionState::Retryable;
        receipt.updated_at = now_epoch();
        receipt.reason = Some(
            "owner exited before any durable launch action; exact queued attempt retained".into(),
        );
        write_receipt(paths, &receipt)?;
        record.state = DispatchState::Queued;
        record.dispatch_started_at = None;
        atomic_replace(
            paths
                .queue_dir()
                .join(format!("{}.request", record.request_id)),
            &record.render(),
            0o600,
        )
        .map_err(|error| message(error.to_string()))?;
        recovered += 1;
    }
    Ok(recovered)
}

fn queued_parent_identity(record: &QueueRecord) -> Result<Option<Value>> {
    let root_parent = format!("root-home:{}", record.root_home.display());
    if record.parent_task_id == root_parent {
        return Ok(None);
    }
    if !valid_id(&record.parent_task_id) {
        return Err(message("queued parent identity is invalid; retained"));
    }
    let path = record
        .parent_state
        .join(format!("{}.meta", record.parent_task_id));
    let bytes = read_bounded_regular(&path, 1024 * 1024)
        .map_err(|_| message("queued parent metadata is unreadable; retained"))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| message("queued parent metadata is not UTF-8; retained"))?;
    let model = exact_metadata_value(text, "canonical_model")?
        .ok_or_else(|| message("queued parent metadata lacks canonical identity; retained"))?;
    let value: Value = serde_json::from_str(model)
        .map_err(|_| message("queued parent canonical identity is invalid; retained"))?;
    if value.get("task_id").and_then(Value::as_str) != Some(record.parent_task_id.as_str())
        || value.get("root_id").and_then(Value::as_str)
            != Some(format!("root-home:{}", record.root_home.display()).as_str())
        || value
            .get("owner_home")
            .and_then(Value::as_str)
            .is_none_or(|home| !same_path(Path::new(home), &record.parent_home))
        || value
            .get("owner_state")
            .and_then(Value::as_str)
            .is_none_or(|state| !same_path(Path::new(state), &record.parent_state))
    {
        return Err(message("queued parent routing identity changed; retained"));
    }
    if value
        .pointer("/attempt/id")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
        || value
            .pointer("/attempt/generation")
            .and_then(Value::as_u64)
            .is_none()
        || value
            .pointer("/attempt/brief_revision")
            .and_then(Value::as_u64)
            .is_none()
    {
        return Err(message(
            "queued parent attempt identity is incomplete; retained",
        ));
    }
    Ok(Some(value))
}

fn bind_queued_launch_environment(command: &mut Command, record: &QueueRecord) -> Result<()> {
    if record.owner_home.as_os_str().is_empty() || record.owner_state.as_os_str().is_empty() {
        return Err(message("queued owner binding is incomplete; retained"));
    }
    command
        .env("MX_HOME", &record.owner_home)
        .env("MX_ROOT_HOME", &record.root_home)
        .env("MX_STATE_OVERRIDE", &record.owner_state)
        .env("MX_DATA_OVERRIDE", record.owner_home.join("data"))
        .env("MX_PROJECTS_OVERRIDE", record.owner_home.join("projects"))
        .env("MX_CONFIG_OVERRIDE", record.owner_home.join("config"));
    let parent = queued_parent_identity(record)?;
    if let Some(parent) = parent {
        let attempt = &parent["attempt"];
        command
            .env("MX_TASK_ID", &record.parent_task_id)
            .env("MX_REPORT_STATE_OVERRIDE", &record.parent_state)
            .env(
                "MX_ATTEMPT_ID",
                attempt["id"]
                    .as_str()
                    .ok_or_else(|| message("queued parent attempt id is invalid; retained"))?,
            )
            .env(
                "MX_ATTEMPT_GENERATION",
                attempt["generation"]
                    .as_u64()
                    .map(|value| value.to_string())
                    .ok_or_else(|| message("queued parent generation is invalid; retained"))?,
            )
            .env(
                "MX_BRIEF_REVISION",
                attempt["brief_revision"]
                    .as_u64()
                    .map(|value| value.to_string())
                    .ok_or_else(|| message("queued parent brief revision is invalid; retained"))?,
            );
    } else {
        for name in [
            "MX_TASK_ID",
            "MX_REPORT_STATE_OVERRIDE",
            "MX_ATTEMPT_ID",
            "MX_ATTEMPT_GENERATION",
            "MX_BRIEF_REVISION",
        ] {
            command.env_remove(name);
        }
    }
    Ok(())
}

pub fn queue_drain(paths: &HeadroomPaths) -> Result<String> {
    queue_drain_with(paths, None, &|backend, endpoint| {
        crate::facade::observe_endpoint(backend, endpoint, None, false)
            .ok()
            .map(|observation| observation.exists)
    })
}

fn queue_drain_with(
    paths: &HeadroomPaths,
    spawn_override: Option<&Path>,
    endpoint_exists: &dyn Fn(&str, &str) -> Option<bool>,
) -> Result<String> {
    queue_drain_with_headroom(paths, spawn_override, endpoint_exists, &evaluate)
}

fn queue_drain_with_headroom(
    paths: &HeadroomPaths,
    spawn_override: Option<&Path>,
    endpoint_exists: &dyn Fn(&str, &str) -> Option<bool>,
    observe_headroom: &dyn Fn(&HeadroomPaths) -> Result<Headroom>,
) -> Result<String> {
    admission_reconcile_inactive_with_probe(paths, endpoint_exists)?;
    if !paths.queue_dir().is_dir() {
        return Ok(String::new());
    }
    recover_unstarted_dispatches(paths)?;
    let headroom = observe_headroom(paths)?;
    let config = admission_config(paths)?;
    let now = now_epoch();
    // Backend liveness can invoke an external provider. Observe before the
    // short queue lock, then revalidate the exact record before committing.
    let observed = queue_records(paths)?
        .into_iter()
        .filter(|record| record.state == DispatchState::Dispatching)
        .map(|record| metadata_reconciled(&record).map(|result| (record, result)))
        .collect::<Result<Vec<_>>>()?;
    let lock = acquire(paths)?;
    let mut records = queue_records(paths)?;
    validate_dependency_graph(&records)?;
    for (observed_record, outcome) in observed {
        if let Some((endpoint, allocation)) = outcome
            && records.iter().any(|record| record == &observed_record)
        {
            let record = observed_record;
            reserve(paths, &record, now)?;
            drop(lock);
            admission_finish(paths, &record.request_id, Some(endpoint), allocation, None)?;
            let _lock = acquire(paths)?;
            let path = paths
                .queue_dir()
                .join(format!("{}.request", record.request_id));
            if QueueRecord::parse(&path, &record.request_id)? != record {
                return Err(message("dispatch changed during reconciliation; retained"));
            }
            fs::remove_file(path)?;
            return Ok(format!("dispatch-queue: reconciled {}\n", record.task_id));
        }
    }
    let used = resource_use(paths, &headroom.observed_active_receipts)?;
    let aging = config.aging_seconds;
    records.sort_by(|left, right| {
        dispatch_score(right, now, aging)
            .cmp(&dispatch_score(left, now, aging))
            .then_with(|| left.enqueued_at.cmp(&right.enqueued_at))
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    let mut selected = None;
    for candidate in records {
        let eligible = candidate.state == DispatchState::Queued
            || read_receipt(paths, &candidate.request_id)?
                .is_some_and(|receipt| receipt.state == AdmissionState::Retryable);
        if eligible
            && candidate.dependencies.iter().all(dependency_completed)
            && fits(
                &candidate,
                &headroom,
                &config,
                &used,
                aged_coordinator_may_yield_worker_reserve(&candidate, &headroom, now, aging),
            )
        {
            selected = Some(candidate);
            break;
        }
    }
    let Some(mut record) = selected else {
        return Ok(String::new());
    };
    let spawn = spawn_override
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("MX_HEADROOM_SPAWN_BIN").map(PathBuf::from))
        .unwrap_or_else(|| {
            let root = std::env::var_os("MX_ROOT_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
            root.join("bin/mx-spawn.sh")
        });
    let mut command = Command::new(spawn);
    command
        .env("MX_HEADROOM_SKIP_QUEUE", "1")
        .env("MX_ADMISSION_REQUEST_ID", &record.request_id)
        .env("MX_SPAWN_RECOVERY_REQUEST", &record.request_id)
        .args([&record.task_id, &record.project]);
    bind_queued_launch_environment(&mut command, &record)?;
    record.state = DispatchState::Dispatching;
    record.dispatch_started_at = Some(now);
    reserve(paths, &record, now)?;
    atomic_replace(
        paths
            .queue_dir()
            .join(format!("{}.request", record.request_id)),
        &record.render(),
        0o600,
    )
    .map_err(|error| message(error.to_string()))?;
    drop(lock);
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
    if !record.mode.is_empty() {
        command.args(["--mode", &record.mode]);
    }
    if !record.yolo.is_empty() {
        command.args(["--yolo", &record.yolo]);
    }
    if let Some(model) = &record.canonical_model {
        command.env("MX_QUEUED_MODEL", model);
    }
    if record.kind == "scout" && record.canonical_model.is_none() {
        command.arg("--scout");
    }
    if record.kind == "daemon" {
        command.arg("--persistent");
    }
    for (name, units) in &record.resources {
        if name != "session" && !name.starts_with("harness:") && !name.starts_with("project:") {
            command.args(["--resource", &format!("{name}={units}")]);
        }
    }
    let status = match command.status() {
        Ok(status) => status,
        Err(_) => {
            admission_retryable(
                paths,
                &record.request_id,
                &record.task_id,
                &record.owner_state,
                &attempt_id(&record)?,
                "spawn command was not created; no endpoint exists",
            )?;
            return Err(message(format!(
                "queued dispatch {} could not be launched; record retained",
                record.task_id
            )));
        }
    };
    if !status.success() {
        admission_finish(
            paths,
            &record.request_id,
            None,
            None,
            Some(
                "spawn command failed; reconcile recorded endpoint and allocation before retry"
                    .into(),
            ),
        )?;
        return Err(message(format!(
            "queued dispatch {} could not be launched; record retained",
            record.task_id
        )));
    }
    // The child process has exited, so this observation also occurs outside
    // the queue lock.
    let reconciled = metadata_reconciled(&record)?;
    let (endpoint, allocation) = reconciled.map_or((None, None), |(endpoint, allocation)| {
        (Some(endpoint), allocation)
    });
    admission_finish(paths, &record.request_id, endpoint, allocation, None)?;
    let _lock = acquire(paths)?;
    let path = paths
        .queue_dir()
        .join(format!("{}.request", record.request_id));
    if QueueRecord::parse(&path, &record.request_id)? != record {
        return Err(message("dispatch changed before acknowledgement; retained"));
    }
    fs::remove_file(path)?;
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
    use std::collections::{BTreeMap, BTreeSet};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread;

    use super::{
        AdmissionConfig, AdmissionDecision, AdmissionState, ApiHeadroom, Dependency, DispatchState,
        Headroom, HeadroomPaths, LocalHeadroom, ProcessIdentity, QueueRecord, absolute_ps_identity,
        admission_config, admission_finish, admission_owner_identity, admission_reconcile_inactive,
        admission_release_execution, admission_resume_reserved, admission_retryable,
        admission_try_reserve, admission_try_reserve_evaluated,
        aged_coordinator_may_yield_worker_reserve, configured_candidates, configured_capacity,
        dependency_completed, dispatch_proves_never_started, dispatch_score, evaluate,
        existing_admission_decision, fits, live_counts_with_probe, metadata_reconciled,
        metadata_value, now_epoch, parse_nonnegative_integer, parse_nonnegative_number,
        parse_positive_number, profile_harnesses, queue_add, queue_cancel, queue_drain_with,
        queue_drain_with_headroom, queue_list, queue_priority, queue_records, read_compact,
        read_receipt, reservation_owner_is_live, reserve, resource_use, same_receipt_identity,
        valid_id, validate_dependency_graph, validate_record, write_receipt,
    };

    fn paths(temp: &tempfile::TempDir) -> HeadroomPaths {
        let state = temp.path().join("state");
        let config = temp.path().join("config");
        std::fs::create_dir(&state).expect("state");
        std::fs::create_dir(&config).expect("config");
        HeadroomPaths {
            root_home: temp.path().to_path_buf(),
            state,
            config,
            proc_root: temp.path().join("proc"),
        }
    }

    fn admission_record(paths: &HeadroomPaths, request: &str, task: &str) -> QueueRecord {
        QueueRecord {
            request_id: request.into(),
            task_id: task.into(),
            root_home: paths.root_home.clone(),
            owner_home: paths.root_home.clone(),
            owner_state: paths.state.clone(),
            parent_task_id: format!("root-home:{}", paths.root_home.display()),
            parent_home: paths.root_home.clone(),
            parent_state: paths.state.clone(),
            project: "/tmp/project".into(),
            harness: "codex".into(),
            model: "default".into(),
            effort: "default".into(),
            backend: "tmux".into(),
            kind: "delivery".into(),
            mode: "deep-review".into(),
            yolo: "off".into(),
            enqueued_at: 10,
            priority: 0,
            dependencies: vec![],
            resources: std::collections::BTreeMap::from([
                ("session".into(), 1),
                ("harness:codex".into(), 1),
                ("project:demo".into(), 1),
            ]),
            state: DispatchState::Queued,
            dispatch_started_at: None,
            canonical_model: Some(r#"{"attempt":{"id":"attempt-1"}}"#.into()),
        }
    }

    fn synthetic_headroom(available: u64) -> Headroom {
        Headroom {
            model: "local+api",
            accounting_scope: "root-home-and-descendants",
            capacity: available,
            in_use: 0,
            available,
            at_limit: available == 0,
            worker_headroom: 1,
            workers_in_use: 0,
            coordinators_in_use: 0,
            unknown_session_use: 0,
            local: LocalHeadroom {
                cpu_count: 8.0,
                load_one: 0.0,
                memory_available_bytes: u64::MAX,
                available,
            },
            api: ApiHeadroom {
                source: "configured-budget",
                capacity: available,
                available,
            },
            candidates: std::collections::BTreeMap::new(),
            observed_active_receipts: BTreeSet::new(),
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
    fn canonical_candidate_files_precede_matching_legacy_aliases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let dispatch = r#"{"rules":[],"default":{"harness":"codex"}}"#;
        std::fs::write(paths.config.join("subagent-dispatch.json"), dispatch).expect("canonical");
        std::fs::write(paths.config.join("actor-dispatch.json"), dispatch).expect("legacy");
        assert_eq!(
            configured_candidates(&paths).expect("candidates"),
            ["codex"]
        );

        std::fs::write(
            paths.config.join("actor-dispatch.json"),
            r#"{"rules":[],"default":{"harness":"pi"}}"#,
        )
        .expect("conflict");
        assert!(
            configured_candidates(&paths)
                .expect_err("conflicting aliases")
                .to_string()
                .contains("conflicting config/subagent-dispatch.json")
        );

        std::fs::remove_file(paths.config.join("subagent-dispatch.json")).expect("remove dispatch");
        std::fs::remove_file(paths.config.join("actor-dispatch.json")).expect("remove alias");
        std::fs::write(paths.config.join("subagent-harness"), "claude\n").expect("harness");
        assert_eq!(
            configured_candidates(&paths).expect("harness candidate"),
            ["claude"]
        );
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

        for invalid in [
            "{",
            r#"{"version":2}"#,
            r#"{"version":1,"aging_seconds":0}"#,
            r#"{"version":1,"resources":{"session":2}}"#,
            r#"{"version":1,"resources":{"harness:codex":2}}"#,
            r#"{"version":1,"resources":{"gpu":0}}"#,
        ] {
            std::fs::write(paths.config.join("admission-capacity.json"), invalid)
                .expect("admission config");
            assert!(admission_config(&paths).is_err(), "{invalid}");
        }
        std::fs::write(
            paths.config.join("admission-capacity.json"),
            r#"{"version":1,"aging_seconds":5,"resources":{"gpu":2}}"#,
        )
        .expect("admission config");
        assert_eq!(admission_config(&paths).expect("valid").resources["gpu"], 2);
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
        let base = QueueRecord::legacy("one".to_owned(), "project".to_owned(), 1);
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
        let mut record = QueueRecord::legacy("one".to_owned(), "projects/one".to_owned(), 1);
        record.harness = "codex".to_owned();
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
                            harness: "codex".to_owned(),
                            backend: "tmux".to_owned(),
                            ..QueueRecord::legacy(
                                format!("task-{index}"),
                                format!("projects/task-{index}"),
                                index,
                            )
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
        let record =
            QueueRecord::legacy("published".to_owned(), "projects/published".to_owned(), 1);
        queue_add(&paths, &record).expect("published");
        std::fs::write(paths.queue_dir().join(".interrupted.tmp"), b"partial").expect("temporary");
        assert_eq!(queue_list(&paths).expect("recovery").lines().count(), 1);
    }

    #[test]
    fn priority_changes_are_exact_and_dispatching_requests_cannot_be_cancelled() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let record = admission_record(&paths, "request-one", "task-one");
        queue_add(&paths, &record).expect("queue");
        assert_eq!(
            queue_priority(&paths, "request-one", 17).expect("priority"),
            "priority: request-one=17\n"
        );
        let mut stored = queue_records(&paths).expect("records").remove(0);
        assert_eq!(stored.priority, 17);
        stored.state = DispatchState::Dispatching;
        stored.dispatch_started_at = Some(20);
        super::atomic_replace(
            paths.queue_dir().join("request-one.request"),
            &stored.render(),
            0o600,
        )
        .expect("dispatching");
        assert!(queue_cancel(&paths, "request-one").is_err());
        assert!(queue_priority(&paths, "request-one", 1).is_err());
        let mut new_high = admission_record(&paths, "new", "new");
        new_high.priority = 5;
        new_high.enqueued_at = 100;
        let mut old_low = admission_record(&paths, "old", "old");
        old_low.enqueued_at = 0;
        assert!(dispatch_score(&old_low, 100, 10) > dispatch_score(&new_high, 100, 10));
        assert_eq!(dispatch_score(&old_low, u64::MAX, 1), i64::MAX);
    }

    #[test]
    fn qualified_dependency_cycles_are_rejected_across_owner_states() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let state_a = temp.path().join("home-a/state");
        let state_b = temp.path().join("home-b/state");
        std::fs::create_dir_all(&state_a).expect("state a");
        std::fs::create_dir_all(&state_b).expect("state b");
        let mut a = admission_record(&paths, "request-a", "task-a");
        a.owner_state = state_a.clone();
        a.dependencies = vec![Dependency {
            task_id: "task-b".into(),
            owner_state: state_b.clone(),
        }];
        let mut b = admission_record(&paths, "request-b", "task-b");
        b.owner_state = state_b;
        b.dependencies = vec![Dependency {
            task_id: "task-a".into(),
            owner_state: state_a,
        }];
        assert!(validate_dependency_graph(&[a.clone()]).is_ok());
        assert!(validate_dependency_graph(&[a, b]).is_err());
    }

    #[test]
    fn small_requests_bypass_unfit_large_requests_and_opaque_providers_do_not_invent_zero() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let config = AdmissionConfig {
            version: 1,
            aging_seconds: 10,
            resources: std::collections::BTreeMap::from([("gpu".into(), 2)]),
            worker_headroom: 1,
        };
        let headroom = synthetic_headroom(1);
        let mut large = admission_record(&paths, "large", "large-task");
        large.priority = 100;
        large.resources.insert("gpu".into(), 3);
        let mut small = admission_record(&paths, "small", "small-task");
        small.resources.insert("gpu".into(), 1);
        let used = std::collections::BTreeMap::new();
        assert!(!fits(&large, &headroom, &config, &used, false));
        assert!(fits(&small, &headroom, &config, &used, false));
        assert!(fits(
            &small,
            &synthetic_headroom(1),
            &AdmissionConfig::default(),
            &used,
            false,
        ));
    }

    #[test]
    fn coordinators_cannot_consume_reserved_worker_headroom() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let mut coordinator = admission_record(&paths, "coord", "coord-task");
        coordinator.resources = BTreeMap::from([("session".into(), 2)]);
        coordinator.canonical_model =
            Some(r#"{"role":"sub-orchestrator","attempt":{"id":"attempt-1"}}"#.into());
        let mut worker = coordinator.clone();
        worker.request_id = "worker".into();
        worker.task_id = "worker-task".into();
        worker.canonical_model = Some(r#"{"role":"implementer"}"#.into());
        let headroom = synthetic_headroom(2);
        let used = BTreeMap::new();
        assert!(!fits(
            &coordinator,
            &headroom,
            &AdmissionConfig::default(),
            &used,
            false,
        ));
        assert!(fits(
            &coordinator,
            &headroom,
            &AdmissionConfig::default(),
            &used,
            true,
        ));
        assert!(fits(
            &worker,
            &headroom,
            &AdmissionConfig::default(),
            &used,
            false,
        ));
        let mut worker_active = synthetic_headroom(1);
        worker_active.workers_in_use = 1;
        coordinator.resources = BTreeMap::from([("session".into(), 1)]);
        assert!(fits(
            &coordinator,
            &worker_active,
            &AdmissionConfig::default(),
            &used,
            false,
        ));

        let configured = serde_json::from_str::<AdmissionConfig>(
            r#"{"version":1,"aging_seconds":60,"worker_headroom":2,"resources":{}}"#,
        )
        .expect("configured reserve");
        assert_eq!(configured.worker_headroom, 2);
    }

    #[test]
    fn aged_domain_gets_bounded_progress_during_a_continuous_direct_worker_stream() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let mut coordinator = admission_record(&paths, "coord", "coord-task");
        coordinator.canonical_model =
            Some(r#"{"role":"sub-orchestrator","attempt":{"id":"attempt-1"}}"#.into());
        coordinator.enqueued_at = 100;
        let worker = admission_record(&paths, "worker", "worker-task");
        let mut only_reserved_slot = synthetic_headroom(1);
        assert!(!aged_coordinator_may_yield_worker_reserve(
            &coordinator,
            &only_reserved_slot,
            109,
            10,
        ));
        assert!(aged_coordinator_may_yield_worker_reserve(
            &coordinator,
            &only_reserved_slot,
            110,
            10,
        ));
        assert!(!aged_coordinator_may_yield_worker_reserve(
            &worker,
            &only_reserved_slot,
            1_000,
            10,
        ));
        only_reserved_slot.coordinators_in_use = 1;
        assert!(!aged_coordinator_may_yield_worker_reserve(
            &coordinator,
            &only_reserved_slot,
            1_000,
            10,
        ));

        let mut queued = coordinator.clone();
        queued.enqueued_at = now_epoch().saturating_sub(10);
        queue_add(&paths, &queued).expect("queue aged coordinator");
        let mut arriving_worker = worker;
        arriving_worker.enqueued_at = now_epoch();
        let config = AdmissionConfig {
            aging_seconds: 1,
            ..AdmissionConfig::default()
        };
        let available = synthetic_headroom(1);
        assert_eq!(
            admission_try_reserve_evaluated(&paths, &arriving_worker, false, &available, &config,)
                .expect("fair immediate admission"),
            AdmissionDecision::Deferred,
            "an aged fitting coordinator must claim the next dispatch opportunity before a fresh worker"
        );
        assert!(
            read_receipt(&paths, &arriving_worker.request_id)
                .expect("worker admission receipt")
                .is_none()
        );
    }

    #[test]
    fn shared_receipts_serialize_competing_homes_and_release_requires_exact_execution() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let first = admission_record(&paths, "request-a", "task");
        let mut second = admission_record(&paths, "request-b", "task");
        second.owner_home = temp.path().join("other-home");
        second.owner_state = second.owner_home.join("state");
        reserve(&paths, &first, 1).expect("first reservation");
        let used = resource_use(&paths, &BTreeSet::new()).expect("resource use");
        assert_eq!(used.get("session"), Some(&1));
        assert!(!fits(
            &second,
            &synthetic_headroom(1),
            &AdmissionConfig::default(),
            &used,
            false,
        ));
        admission_finish(
            &paths,
            "request-a",
            Some("session:task".into()),
            Some("allocation-1".into()),
            None,
        )
        .expect("active");
        assert_eq!(
            resource_use(&paths, &BTreeSet::from(["request-a".into()]))
                .expect("active")
                .get("session"),
            None
        );
        assert!(
            !admission_release_execution(
                &paths,
                "task",
                &paths.state,
                "wrong-attempt",
                "session:task"
            )
            .expect("wrong evidence")
        );
        assert!(
            admission_release_execution(&paths, "task", &paths.state, "attempt-1", "session:task")
                .expect("release")
        );
        assert!(existing_admission_decision(AdmissionState::Released, false).is_err());
    }

    #[test]
    fn admission_owner_and_symlinked_paths_reconcile_to_exact_physical_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let record = admission_record(&paths, "request", "task");
        reserve(&paths, &record, 1).expect("reserve");
        let receipt = read_receipt(&paths, "request")
            .expect("receipt read")
            .expect("receipt");
        let linked = temp.path().join("linked-home");
        symlink(temp.path(), &linked).expect("home symlink");
        let mut equivalent = receipt.clone();
        equivalent.root_home = linked.clone();
        equivalent.owner_home = linked.clone();
        equivalent.owner_state = linked.join("state");
        assert!(same_receipt_identity(&receipt, &equivalent));

        let absolute = absolute_ps_identity(std::process::id()).expect("absolute ps identity");
        assert_eq!(absolute.pid, std::process::id());
        assert!(!absolute.marker.is_empty());
        assert_eq!(
            admission_owner_identity(std::process::id())
                .expect("admission owner")
                .pid,
            std::process::id()
        );
    }

    #[test]
    fn uncertain_admission_needs_proof_before_exact_retry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let record = admission_record(&paths, "request", "task");
        reserve(&paths, &record, 1).expect("reserve");
        admission_finish(
            &paths,
            "request",
            None,
            Some("allocation".into()),
            Some("unknown endpoint".into()),
        )
        .expect("uncertain");
        assert!(existing_admission_decision(AdmissionState::Uncertain, true).is_err());
        assert!(
            admission_retryable(&paths, "request", "task", &paths.state, "wrong", "absent")
                .is_err()
        );
        admission_retryable(
            &paths,
            "request",
            "task",
            &paths.state,
            "attempt-1",
            "verified absent",
        )
        .expect("retryable");
        assert_eq!(
            existing_admission_decision(AdmissionState::Retryable, false).expect("retry"),
            AdmissionDecision::Granted
        );
        reserve(&paths, &record, 2).expect("reserve retry");
        assert!(existing_admission_decision(AdmissionState::Reserved, false).is_err());
        assert_eq!(
            existing_admission_decision(AdmissionState::Reserved, true).expect("queue owner"),
            AdmissionDecision::Granted
        );
        let mut receipt = super::read_receipt(&paths, "request")
            .expect("receipt")
            .expect("present");
        receipt.owner = Some(multplx_core::process::ProcessIdentity {
            pid: u32::MAX,
            marker: "dead-owner".into(),
        });
        assert!(!reservation_owner_is_live(
            &receipt,
            &multplx_core::process::SystemProcessProbe::default()
        ));
    }

    #[test]
    fn reconciliation_rejects_duplicate_canonical_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let mut record = admission_record(&paths, "request", "task");
        let identity = serde_json::json!({
            "schema_version": 2,
            "task_id": "task",
            "parent_id": record.parent_task_id,
            "root_id": format!("root-home:{}", paths.root_home.display()),
            "owner_home": paths.root_home,
            "owner_state": paths.state,
            "parent_home": paths.root_home,
            "parent_state": paths.state,
            "attempt": {"id":"attempt-1","generation":1,"brief_revision":1},
            "accepted_brief_revision": 1,
            "accepted_brief_digest": "digest",
            "accepted_brief_path": "/tmp/brief",
            "project": {"project_id":"p","checkout_id":"c","common_git_identity":"g","canonical_path":"/tmp/project","starting_revision":"rev"}
        });
        record.canonical_model = Some(identity.to_string());
        std::fs::write(
            paths.state.join("task.meta"),
            format!("canonical_model={identity}\ncanonical_model={identity}\nwindow=x\n"),
        )
        .expect("metadata");
        assert!(metadata_reconciled(&record).is_err());
    }

    #[test]
    fn queue_identity_and_dependency_failures_are_closed_before_mutation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let base = admission_record(&paths, "request", "task");

        let mut cases = Vec::new();
        let mut other_root = base.clone();
        other_root.root_home = temp.path().join("other-root");
        cases.push(other_root);
        let mut relative_owner = base.clone();
        relative_owner.owner_state = PathBuf::from("relative/state");
        cases.push(relative_owner);
        let mut invalid_parent = base.clone();
        invalid_parent.parent_task_id = "bad/parent".into();
        cases.push(invalid_parent);
        let mut empty_resources = base.clone();
        empty_resources.resources.clear();
        cases.push(empty_resources);
        let mut zero_resource = base.clone();
        zero_resource.resources.insert("gpu".into(), 0);
        cases.push(zero_resource);
        let mut self_dependency = base.clone();
        self_dependency.dependencies.push(Dependency {
            task_id: self_dependency.task_id.clone(),
            owner_state: self_dependency.owner_state.clone(),
        });
        cases.push(self_dependency);
        let mut duplicate_dependency = base.clone();
        let dependency = Dependency {
            task_id: "prerequisite".into(),
            owner_state: paths.state.clone(),
        };
        duplicate_dependency.dependencies = vec![dependency.clone(), dependency];
        cases.push(duplicate_dependency);
        let mut invalid_mode = base.clone();
        invalid_mode.mode = "merge-anything".into();
        cases.push(invalid_mode);
        let mut invalid_yolo = base.clone();
        invalid_yolo.yolo = "maybe".into();
        cases.push(invalid_yolo);
        for record in cases {
            assert!(validate_record(&paths, &record).is_err(), "{record:?}");
        }

        assert!(queue_priority(&paths, "bad/id", 1).is_err());
        let unsafe_record = admission_record(&paths, "unsafe", "unsafe-task");
        std::fs::create_dir_all(paths.queue_dir()).expect("queue");
        symlink(
            temp.path().join("missing-target"),
            paths.queue_dir().join("unsafe.request"),
        )
        .expect("broken symlink");
        assert!(queue_add(&paths, &unsafe_record).is_err());

        let dependency = Dependency {
            task_id: "prerequisite".into(),
            owner_state: paths.state.clone(),
        };
        assert!(!dependency_completed(&dependency));
        std::fs::write(
            paths.state.join("prerequisite.meta"),
            "canonical_model={}\ncanonical_model={}\n",
        )
        .expect("duplicate models");
        assert!(!dependency_completed(&dependency));
        std::fs::write(
            paths.state.join("prerequisite.meta"),
            "canonical_model={bad json}\n",
        )
        .expect("invalid model");
        assert!(!dependency_completed(&dependency));
        std::fs::write(
            paths.state.join("prerequisite.meta"),
            r#"canonical_model={"schedule":{"state":"running"}}
"#,
        )
        .expect("running model");
        assert!(!dependency_completed(&dependency));
        std::fs::write(
            paths.state.join("prerequisite.meta"),
            r#"canonical_model={"schedule":{"state":"completed"}}
"#,
        )
        .expect("completed model");
        assert!(dependency_completed(&dependency));

        let mut deferred = base;
        deferred.dependencies.push(Dependency {
            task_id: "missing".into(),
            owner_state: paths.state.clone(),
        });
        assert_eq!(
            admission_try_reserve(&paths, &deferred).expect("dependency deferral"),
            AdmissionDecision::Deferred
        );
        assert!(
            read_receipt(&paths, "request")
                .expect("receipt lookup")
                .is_none()
        );
    }

    #[test]
    fn corrupt_conflicting_and_duplicate_admission_receipts_remain_retained() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        assert!(read_receipt(&paths, "bad/id").is_err());
        let record = admission_record(&paths, "request", "task");
        reserve(&paths, &record, 1).expect("reserve");
        let receipt_path = paths.admissions_dir().join("request.json");
        let original = std::fs::read(&receipt_path).expect("original receipt");

        std::fs::write(&receipt_path, b"{").expect("corrupt receipt");
        assert!(read_receipt(&paths, "request").is_err());
        std::fs::write(&receipt_path, &original).expect("restore receipt");
        let mut invalid = read_receipt(&paths, "request")
            .expect("valid read")
            .expect("receipt");
        invalid.version = 2;
        std::fs::write(&receipt_path, serde_json::to_vec(&invalid).expect("encode"))
            .expect("invalid identity");
        assert!(read_receipt(&paths, "request").is_err());
        std::fs::write(&receipt_path, &original).expect("restore receipt");

        let mut conflicting = record.clone();
        conflicting.resources.insert("gpu".into(), 1);
        assert!(reserve(&paths, &conflicting, 2).is_err());
        assert!(absolute_ps_identity(0).is_err());
        assert!(admission_finish(&paths, "missing", None, None, None).is_err());
        assert!(
            admission_retryable(
                &paths,
                "missing",
                "task",
                &paths.state,
                "attempt-1",
                "absent",
            )
            .is_err()
        );

        admission_finish(
            &paths,
            "request",
            Some("session:task".into()),
            Some("allocation".into()),
            None,
        )
        .expect("active");
        let active = read_receipt(&paths, "request")
            .expect("read active")
            .expect("active");
        assert_eq!(
            resource_use(&paths, &BTreeSet::from(["request".into()]))
                .expect("active use")
                .get("project:demo"),
            Some(&1)
        );
        assert_eq!(
            resource_use(&paths, &BTreeSet::from(["request".into()]))
                .expect("native use")
                .get("session"),
            None
        );

        let mut duplicate = active.clone();
        duplicate.request_id = "duplicate".into();
        write_receipt(&paths, &duplicate).expect("duplicate execution claim");
        assert!(
            admission_release_execution(&paths, "task", &paths.state, "attempt-1", "session:task",)
                .is_err()
        );
        duplicate.state = AdmissionState::Released;
        write_receipt(&paths, &duplicate).expect("retire duplicate");
        assert!(
            admission_release_execution(&paths, "task", &paths.state, "attempt-1", "session:task",)
                .expect("unique release")
        );
        assert!(
            resource_use(&paths, &BTreeSet::new())
                .expect("released use")
                .is_empty()
        );
    }

    #[test]
    fn public_admission_recovers_dead_reservation_and_keeps_active_custom_units() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        #[cfg(target_os = "linux")]
        let paths = HeadroomPaths {
            proc_root: PathBuf::from("/proc"),
            ..paths
        };
        std::fs::write(
            paths.config.join("admission-capacity.json"),
            r#"{"version":1,"aging_seconds":5,"resources":{"gpu":2}}"#,
        )
        .expect("capacity");
        let mut record = admission_record(&paths, "request", "task");
        record.resources = std::collections::BTreeMap::from([("gpu".into(), 2)]);

        assert_eq!(
            admission_try_reserve(&paths, &record).expect("first reservation"),
            AdmissionDecision::Granted
        );
        assert!(admission_try_reserve(&paths, &record).is_err());

        let mut abandoned = read_receipt(&paths, "request")
            .expect("read reservation")
            .expect("reservation");
        abandoned.owner = Some(multplx_core::process::ProcessIdentity {
            pid: u32::MAX,
            marker: "dead owner lifetime".into(),
        });
        write_receipt(&paths, &abandoned).expect("simulate owner crash");
        assert_eq!(
            admission_try_reserve(&paths, &record).expect("recover dead owner"),
            AdmissionDecision::Granted
        );
        assert_eq!(
            admission_resume_reserved(&paths, &record).expect("resume exact queue reservation"),
            AdmissionDecision::Granted
        );

        admission_finish(
            &paths,
            "request",
            Some("session:task".into()),
            Some("allocation".into()),
            None,
        )
        .expect("activate");
        assert_eq!(
            admission_try_reserve(&paths, &record).expect("active convergence"),
            AdmissionDecision::AlreadyActive
        );
        let mut competing = admission_record(&paths, "competing", "other-task");
        competing.resources = std::collections::BTreeMap::from([("gpu".into(), 1)]);
        assert_eq!(
            admission_try_reserve(&paths, &competing).expect("custom capacity"),
            AdmissionDecision::Deferred
        );
        assert!(
            read_receipt(&paths, "competing")
                .expect("competing receipt lookup")
                .is_none()
        );

        admission_finish(
            &paths,
            "request",
            None,
            None,
            Some("provider result was uncertain".into()),
        )
        .expect("uncertain");
        admission_retryable(
            &paths,
            "request",
            "task",
            &paths.state,
            "attempt-1",
            "endpoint proved absent",
        )
        .expect("retryable");
        assert_eq!(
            admission_try_reserve(&paths, &record).expect("retry reservation"),
            AdmissionDecision::Granted
        );
    }

    #[test]
    fn actual_headroom_counts_nested_root_and_persistent_active_receipts_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        #[cfg(target_os = "linux")]
        let paths = HeadroomPaths {
            proc_root: PathBuf::from("/proc"),
            ..paths
        };
        std::fs::write(paths.config.join("api-capacity"), "3\n").expect("global capacity");
        std::fs::write(paths.config.join("api-capacity-codex"), "2\n").expect("codex capacity");
        std::fs::write(paths.config.join("api-capacity-opaque"), "1\n").expect("opaque capacity");
        std::fs::write(
            paths.config.join("actor-dispatch.json"),
            r#"{"rules":[],"default":[{"harness":"codex"},{"harness":"opaque"}]}"#,
        )
        .expect("dispatch candidates");

        let mut nested = admission_record(&paths, "nested-request", "nested");
        nested.owner_home = temp.path().join("child-home");
        nested.owner_state = nested.owner_home.join("state");
        nested.resources = BTreeMap::from([("session".into(), 1), ("harness:codex".into(), 1)]);
        reserve(&paths, &nested, 1).expect("nested reserve");
        admission_finish(
            &paths,
            "nested-request",
            Some("session:nested".into()),
            None,
            None,
        )
        .expect("nested active");

        let mut root = admission_record(&paths, "root-request", "root-task");
        root.resources = nested.resources.clone();
        reserve(&paths, &root, 1).expect("root reserve");
        admission_finish(
            &paths,
            "root-request",
            Some("session:root".into()),
            None,
            None,
        )
        .expect("root active");
        std::fs::write(
            paths.state.join("root-task.meta"),
            "window=session:root\nharness=codex\nkind=delivery\ncanonical_model={\"attempt\":{\"id\":\"attempt-1\"}}\n",
        )
        .expect("root metadata");

        let mut coordinator = admission_record(&paths, "coordinator-request", "coordinator");
        coordinator.kind = "daemon".into();
        coordinator.owner_home = temp.path().join("coordinator-home");
        coordinator.owner_state = coordinator.owner_home.join("state");
        coordinator.resources =
            BTreeMap::from([("session".into(), 1), ("harness:opaque".into(), 1)]);
        reserve(&paths, &coordinator, 1).expect("coordinator reserve");
        admission_finish(
            &paths,
            "coordinator-request",
            Some("provider:coordinator".into()),
            None,
            None,
        )
        .expect("coordinator active");

        let headroom = evaluate(&paths).expect("actual headroom");
        assert_eq!(headroom.in_use, 3);
        assert_eq!(headroom.available, 0);
        assert!(headroom.at_limit());
        assert_eq!(headroom.candidates["codex"].in_use, 2);
        assert_eq!(headroom.candidates["opaque"].in_use, 1);

        let mut next = admission_record(&paths, "next-request", "next");
        next.resources = BTreeMap::from([("session".into(), 1), ("harness:opaque".into(), 1)]);
        assert_eq!(
            admission_try_reserve(&paths, &next).expect("root budget exhausted"),
            AdmissionDecision::Deferred
        );

        std::fs::write(
            paths.state.join("legacy-daemon.meta"),
            "window=legacy:daemon\nbackend=unknown\nharness=opaque\nkind=daemon\n",
        )
        .expect("legacy daemon metadata");
        let (total, harnesses, observed) =
            live_counts_with_probe(&paths, |_, _| true, None).expect("legacy live probe");
        assert_eq!(total, 4, "root receipt and metadata must be deduplicated");
        assert_eq!(harnesses.get("codex"), Some(&2));
        assert_eq!(harnesses.get("opaque"), Some(&2));
        assert_eq!(observed.len(), 3);
    }

    #[test]
    fn nested_active_receipt_exhausts_a_one_session_root_budget() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        #[cfg(target_os = "linux")]
        let paths = HeadroomPaths {
            proc_root: PathBuf::from("/proc"),
            ..paths
        };
        std::fs::write(paths.config.join("api-capacity"), "1\n").expect("capacity");
        let mut nested = admission_record(&paths, "nested-request", "nested");
        nested.owner_home = temp.path().join("child-home");
        nested.owner_state = nested.owner_home.join("state");
        reserve(&paths, &nested, 1).expect("nested reserve");
        admission_finish(
            &paths,
            "nested-request",
            Some("session:nested".into()),
            None,
            None,
        )
        .expect("nested active");

        let headroom = evaluate(&paths).expect("actual root headroom");
        assert_eq!((headroom.in_use, headroom.available), (1, 0));
        let (overridden_total, overridden_harnesses, observed) =
            live_counts_with_probe(&paths, |_, _| false, Some(0)).expect("zero native override");
        assert_eq!(overridden_total, 1);
        assert_eq!(overridden_harnesses.get("codex"), Some(&1));
        assert!(observed.contains("nested-request"));
        let next = admission_record(&paths, "next-request", "next");
        assert_eq!(
            admission_try_reserve(&paths, &next).expect("nested budget retained"),
            AdmissionDecision::Deferred
        );
    }

    #[test]
    fn admission_recounts_receipts_activated_after_the_headroom_snapshot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        #[cfg(target_os = "linux")]
        let paths = HeadroomPaths {
            proc_root: PathBuf::from("/proc"),
            ..paths
        };
        std::fs::write(paths.config.join("api-capacity"), "1\n").expect("capacity");
        let record = admission_record(&paths, "racing-request", "racing");
        reserve(&paths, &record, 1).expect("reserved before snapshot");
        let snapshot = evaluate(&paths).expect("headroom snapshot");
        assert!(!snapshot.observed_active_receipts.contains("racing-request"));
        admission_finish(
            &paths,
            "racing-request",
            Some("session:racing".into()),
            None,
            None,
        )
        .expect("activate after snapshot");
        let used = resource_use(&paths, &snapshot.observed_active_receipts)
            .expect("locked receipt recount");
        assert_eq!(used.get("session"), Some(&1));
        let next = admission_record(&paths, "next-request", "next");
        assert!(!fits(
            &next,
            &snapshot,
            &AdmissionConfig::default(),
            &used,
            false,
        ));
    }

    #[test]
    fn inactive_admission_reconciliation_requires_exact_metadata_and_backend() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let mut record = admission_record(&paths, "request", "task");
        record.backend = "unknown".into();
        reserve(&paths, &record, 1).expect("reserve");
        admission_finish(
            &paths,
            "request",
            Some("provider:endpoint".into()),
            None,
            None,
        )
        .expect("active");
        let canonical = serde_json::json!({
            "task_id": "task",
            "root_id": format!("root-home:{}", paths.root_home.display()),
            "owner_home": paths.root_home,
            "owner_state": paths.state,
            "attempt": {"id": "attempt-1"}
        });
        let metadata = |backend: &str, model: &serde_json::Value| {
            format!("window=provider:endpoint\nbackend={backend}\ncanonical_model={model}\n")
        };

        std::fs::write(
            paths.state.join("task.meta"),
            metadata("unknown", &canonical),
        )
        .expect("unknown backend metadata");
        assert_eq!(
            admission_reconcile_inactive(&paths).expect("opaque provider"),
            0
        );

        std::fs::write(
            paths.state.join("task.meta"),
            "window=provider:endpoint\nbackend=tmux\n",
        )
        .expect("missing identity");
        assert!(admission_reconcile_inactive(&paths).is_err());
        std::fs::write(
            paths.state.join("task.meta"),
            "window=provider:endpoint\nbackend=tmux\ncanonical_model={bad}\n",
        )
        .expect("bad identity");
        assert!(admission_reconcile_inactive(&paths).is_err());
        let mut wrong = canonical.clone();
        wrong["task_id"] = serde_json::json!("other");
        std::fs::write(paths.state.join("task.meta"), metadata("tmux", &wrong))
            .expect("wrong identity");
        assert!(admission_reconcile_inactive(&paths).is_err());
        std::fs::write(paths.state.join("task.meta"), metadata("herdr", &canonical))
            .expect("changed backend");
        assert!(admission_reconcile_inactive(&paths).is_err());

        std::fs::remove_file(paths.state.join("task.meta")).expect("remove metadata");
        assert_eq!(
            admission_reconcile_inactive(&paths).expect("missing metadata"),
            0
        );
    }

    #[test]
    fn root_queue_drain_launches_with_registered_private_owner_and_parent_context() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let owner_home = temp.path().join("registered-coordinator-home");
        let owner_state = owner_home.join("state");
        std::fs::create_dir_all(&owner_state).expect("private owner state");
        let parent_id = "coord-parent";
        let parent_model = serde_json::json!({
            "task_id": parent_id,
            "root_id": format!("root-home:{}", paths.root_home.display()),
            "owner_home": owner_home,
            "owner_state": owner_state,
            "attempt": {"id":"coord-attempt","generation":3,"brief_revision":7}
        });
        let parent_meta = owner_state.join(format!("{parent_id}.meta"));
        std::fs::write(
            &parent_meta,
            "canonical_model={\"task_id\":\"different-parent\"}\n",
        )
        .expect("untrusted parent metadata");

        let captured = temp.path().join("spawn-environment");
        let spawn = temp.path().join("fake-mx-spawn");
        std::fs::write(
            &spawn,
            format!("#!/bin/sh\nenv > '{}'\nexit 0\n", captured.display()),
        )
        .expect("fake spawn");
        std::fs::set_permissions(&spawn, std::fs::Permissions::from_mode(0o700))
            .expect("executable");
        let mut record = admission_record(&paths, "private-request", "private-worker");
        record.owner_home = owner_home.clone();
        record.owner_state = owner_state.clone();
        record.parent_task_id = parent_id.into();
        record.parent_home = owner_home.clone();
        record.parent_state = owner_state.clone();
        record.canonical_model = Some(
            serde_json::json!({
                "attempt":{"id":"worker-attempt","generation":1,"brief_revision":1},
                "task_id":"private-worker",
                "parent_id":parent_id,
                "root_id":format!("root-home:{}", paths.root_home.display()),
                "owner_home":owner_home,
                "owner_state":owner_state,
                "parent_home":owner_home,
                "parent_state":owner_state
            })
            .to_string(),
        );
        queue_add(&paths, &record).expect("queue private task for root owner");
        assert!(
            queue_drain_with_headroom(&paths, Some(&spawn), &|_, _| Some(false), &|_| Ok(
                synthetic_headroom(8)
            ),)
            .is_err()
        );
        assert!(
            read_receipt(&paths, "private-request")
                .expect("no reserved receipt")
                .is_none()
        );
        assert_eq!(
            queue_records(&paths).expect("queued task")[0].state,
            DispatchState::Queued
        );
        std::fs::write(&parent_meta, format!("canonical_model={parent_model}\n"))
            .expect("restored canonical parent metadata");
        assert!(
            queue_drain_with_headroom(&paths, Some(&spawn), &|_, _| Some(false), &|_| Ok(
                synthetic_headroom(8)
            ),)
            .expect("root drains private task")
            .contains("launched private-worker")
        );

        let environment = std::fs::read_to_string(captured).expect("captured child environment");
        for expected in [
            format!("MX_HOME={}", owner_home.display()),
            format!("MX_ROOT_HOME={}", paths.root_home.display()),
            format!("MX_STATE_OVERRIDE={}", owner_state.display()),
            format!("MX_DATA_OVERRIDE={}", owner_home.join("data").display()),
            format!(
                "MX_PROJECTS_OVERRIDE={}",
                owner_home.join("projects").display()
            ),
            format!("MX_CONFIG_OVERRIDE={}", owner_home.join("config").display()),
            format!("MX_TASK_ID={parent_id}"),
            format!("MX_REPORT_STATE_OVERRIDE={}", owner_state.display()),
            "MX_ATTEMPT_ID=coord-attempt".into(),
            "MX_ATTEMPT_GENERATION=3".into(),
            "MX_BRIEF_REVISION=7".into(),
        ] {
            assert!(
                environment.lines().any(|line| line == expected),
                "missing {expected:?} in {environment}"
            );
        }
        assert_eq!(queue_list(&paths).expect("queue after launch"), "");
    }

    #[test]
    fn root_queue_drain_recovers_only_exact_never_started_dispatches() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let mut record = admission_record(&paths, "recover-request", "recover-task");
        record.canonical_model = Some(
            serde_json::json!({
                "attempt":{"id":"attempt-1","generation":1,"brief_revision":1},
                "task_id":"recover-task",
                "parent_id":record.parent_task_id,
                "root_id":format!("root-home:{}", paths.root_home.display()),
                "owner_home":record.owner_home,
                "owner_state":record.owner_state,
                "parent_home":record.parent_home,
                "parent_state":record.parent_state
            })
            .to_string(),
        );
        record.state = DispatchState::Dispatching;
        record.dispatch_started_at = Some(10);
        std::fs::create_dir_all(paths.queue_dir()).expect("queue dir");
        std::fs::write(
            paths.queue_dir().join("recover-request.request"),
            record.render(),
        )
        .expect("dispatch intent");
        std::fs::set_permissions(
            paths.queue_dir().join("recover-request.request"),
            std::fs::Permissions::from_mode(0o600),
        )
        .expect("secure dispatch record");
        reserve(&paths, &record, 10).expect("reserved admission");
        admission_finish(
            &paths,
            "recover-request",
            None,
            None,
            Some("old caller used the wrong home".into()),
        )
        .expect("uncertain receipt");
        let mut receipt = read_receipt(&paths, "recover-request")
            .expect("read receipt")
            .expect("receipt exists");
        receipt.owner = Some(ProcessIdentity {
            pid: u32::MAX,
            marker: "exited-old-drain".into(),
        });
        write_receipt(&paths, &receipt).expect("stale owner receipt");
        assert!(dispatch_proves_never_started(&paths, &record).expect("eligible"));
        let mut allocated_record = record.clone();
        let mut allocated_model: serde_json::Value = serde_json::from_str(
            allocated_record
                .canonical_model
                .as_deref()
                .expect("canonical model"),
        )
        .expect("canonical json");
        allocated_model["allocation"] = serde_json::json!({"allocation_id":"preserve-me"});
        allocated_record.canonical_model = Some(allocated_model.to_string());
        assert!(
            !dispatch_proves_never_started(&paths, &allocated_record)
                .expect("allocation blocks retry")
        );

        let spawn_action = paths.state.join(".spawn-actions/recover-request.json");
        std::fs::create_dir_all(spawn_action.parent().expect("action dir"))
            .expect("action directory");
        for (path, contents) in [
            (spawn_action.clone(), "{}"),
            (paths.state.join(".spawn-recover-task.intent"), "{}"),
            (paths.state.join("recover-task.meta"), "bad metadata"),
        ] {
            std::fs::write(&path, contents).expect("write blocker");
            assert!(!dispatch_proves_never_started(&paths, &record).expect("blocked"));
            std::fs::remove_file(path).expect("remove blocker");
        }
        std::os::unix::fs::symlink(temp.path().join("missing-action"), &spawn_action)
            .expect("symlink blocker");
        assert!(!dispatch_proves_never_started(&paths, &record).expect("symlink blocks"));
        std::fs::remove_file(&spawn_action).expect("remove symlink");
        receipt.owner = Some(admission_owner_identity(std::process::id()).expect("live owner"));
        write_receipt(&paths, &receipt).expect("live owner receipt");
        assert!(!dispatch_proves_never_started(&paths, &record).expect("live owner blocks"));
        receipt.owner = None;
        write_receipt(&paths, &receipt).expect("unknown owner receipt");
        assert!(!dispatch_proves_never_started(&paths, &record).expect("unknown owner blocks"));
        receipt.owner = Some(ProcessIdentity {
            pid: u32::MAX,
            marker: "exited-old-drain".into(),
        });
        write_receipt(&paths, &receipt).expect("restore absent owner receipt");

        let spawn = temp.path().join("retry-mx-spawn");
        std::fs::write(&spawn, "#!/bin/sh\nexit 0\n").expect("fake spawn");
        std::fs::set_permissions(&spawn, std::fs::Permissions::from_mode(0o700))
            .expect("executable");
        assert!(
            queue_drain_with_headroom(&paths, Some(&spawn), &|_, _| Some(false), &|_| Ok(
                synthetic_headroom(8)
            ),)
            .expect("recover and launch same request")
            .contains("launched recover-task")
        );
        let final_receipt = read_receipt(&paths, "recover-request")
            .expect("receipt")
            .expect("retained receipt");
        assert_eq!(final_receipt.state, AdmissionState::Active);
        assert_eq!(final_receipt.attempt_id, "attempt-1");
        assert_eq!(queue_list(&paths).expect("queue"), "");
    }

    #[test]
    fn queue_drain_reconciles_absent_execution_without_a_queue_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(&temp);
        let record = admission_record(&paths, "request", "task");
        reserve(&paths, &record, 1).expect("reserve");
        admission_finish(
            &paths,
            "request",
            Some("broker:completed-execution".into()),
            None,
            None,
        )
        .expect("active receipt");
        let canonical = serde_json::json!({
            "task_id": "task",
            "root_id": format!("root-home:{}", paths.root_home.display()),
            "owner_home": paths.root_home,
            "owner_state": paths.state,
            "attempt": {"id": "attempt-1"}
        });
        std::fs::write(
            paths.state.join("task.meta"),
            format!(
                "window=broker:completed-execution\nbackend=tmux\ncanonical_model={canonical}\n"
            ),
        )
        .expect("canonical task metadata");
        assert!(!paths.queue_dir().exists());

        queue_drain_with(&paths, None, &|_, _| Some(false))
            .expect("explicit drain reconciles before inspecting queue");
        assert_eq!(
            read_receipt(&paths, "request")
                .expect("receipt")
                .unwrap()
                .state,
            AdmissionState::Released
        );
    }
}

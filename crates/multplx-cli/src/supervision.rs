//! Native supervision entry-point transactions.

use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use multplx_core::locks::DirectoryLock;
use multplx_core::process::{
    ProcessProbe, ProcessTerminator, SystemProcessProbe, SystemProcessTerminator,
};
use rustix::process::{Pid, Signal, kill_process_group};

#[derive(Clone, Debug, Eq, PartialEq)]
struct SignalObservation {
    path: std::path::PathBuf,
    marker: std::path::PathBuf,
    signature: String,
    maintainer_relevant: bool,
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn signal_signature(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    #[cfg(target_os = "macos")]
    let modified = format!("{}.{:09}", metadata.mtime(), metadata.mtime_nsec());
    #[cfg(not(target_os = "macos"))]
    let modified = metadata.mtime().to_string();
    Some(format!("{}:{modified}", metadata.len()))
}

fn signal_marker(state: &Path, path: &Path) -> std::path::PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .replace('.', "_");
    state.join(format!(".seen-{name}"))
}

fn scan_signals(state: &Path) -> Vec<SignalObservation> {
    let Ok(entries) = fs::read_dir(state) else {
        return Vec::new();
    };
    let override_regex = std::env::var("MX_MAINTAINER_RE").ok();
    let mut observations = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with(".status") || name.ends_with(".turn-ended"))
        })
        .filter_map(|path| {
            let signature = signal_signature(&path)?;
            let marker = signal_marker(state, &path);
            if fs::read_to_string(&marker).is_ok_and(|value| value == signature) {
                return None;
            }
            let last = multplx_core::classification::last_status_line(
                &path,
                multplx_core::classification::STATUS_READ_LIMIT,
            )
            .ok()
            .flatten()
            .unwrap_or_default();
            let maintainer_relevant = multplx_core::classification::is_maintainer_relevant(
                &last,
                override_regex.as_deref(),
                "paused",
            )
            .unwrap_or(true);
            Some(SignalObservation {
                path,
                marker,
                signature,
                maintainer_relevant,
            })
        })
        .collect::<Vec<_>>();
    observations.sort_by(|left, right| left.path.cmp(&right.path));
    observations
}

fn coalesce_signals(state: &Path, grace: Duration) -> Vec<SignalObservation> {
    let mut by_path = scan_signals(state)
        .into_iter()
        .map(|observation| (observation.path.clone(), observation))
        .collect::<std::collections::BTreeMap<_, _>>();
    if !by_path.is_empty() && !grace.is_zero() {
        std::thread::sleep(grace);
        for observation in scan_signals(state) {
            by_path.insert(observation.path.clone(), observation);
        }
    }
    by_path.into_values().collect()
}

fn publish_signal_markers(observations: &[SignalObservation]) -> Result<(), String> {
    for observation in observations {
        multplx_core::filesystem::atomic_replace(
            &observation.marker,
            observation.signature.as_bytes(),
            0o600,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn heartbeat_marker(state: &Path, task: &str) -> std::path::PathBuf {
    let key = task.replace([':', '/', '.'], "_");
    state.join(format!(".hb-surfaced-{key}"))
}

fn mark_status_surfaced(state: &Path, path: &Path) {
    let Some(task) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".status"))
    else {
        return;
    };
    let last = multplx_core::classification::last_status_line(
        path,
        multplx_core::classification::STATUS_READ_LIMIT,
    )
    .ok()
    .flatten()
    .unwrap_or_default();
    let override_regex = std::env::var("MX_MAINTAINER_RE").ok();
    if multplx_core::classification::is_maintainer_relevant(
        &last,
        override_regex.as_deref(),
        "paused",
    )
    .unwrap_or(true)
    {
        let _ = multplx_core::filesystem::atomic_replace(
            heartbeat_marker(state, task),
            last.as_bytes(),
            0o600,
        );
    }
}

fn unsurfaced_maintainer_statuses(state: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = fs::read_dir(state) else {
        return Vec::new();
    };
    let override_regex = std::env::var("MX_MAINTAINER_RE").ok();
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "status")
        })
        .filter(|path| {
            let Some(task) = path.file_stem().and_then(|name| name.to_str()) else {
                return false;
            };
            let last = multplx_core::classification::last_status_line(
                path,
                multplx_core::classification::STATUS_READ_LIMIT,
            )
            .ok()
            .flatten()
            .unwrap_or_default();
            multplx_core::classification::is_maintainer_relevant(
                &last,
                override_regex.as_deref(),
                "paused",
            )
            .unwrap_or(true)
                && fs::read_to_string(heartbeat_marker(state, task))
                    .map_or(true, |surfaced| surfaced != last)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActorAbsorbClass {
    Working,
    Paused,
    None,
}

fn observation_task(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    name.strip_suffix(".status")
        .or_else(|| name.strip_suffix(".turn-ended"))
        .filter(|task| !task.is_empty())
}

fn actor_state_line(source_root: &Path, state: &Path, task: &str) -> Option<String> {
    let executable = std::env::var_os("MX_ACTOR_STATE_BIN")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| source_root.join("bin/mx-actor-state.sh"));
    let output = Command::new(executable)
        .arg(task)
        .env("MX_STATE_OVERRIDE", state)
        .env("MX_JOURNAL_CLASSIFY", "1")
        .env("MX_JOURNAL_SOURCE", "mx-watch")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_owned)
}

fn parse_actor_absorb_class(line: &str) -> ActorAbsorbClass {
    let Some(rest) = line.strip_prefix("state: ") else {
        return ActorAbsorbClass::None;
    };
    let state_value = rest.split_whitespace().next().unwrap_or_default();
    if state_value == "paused" {
        return ActorAbsorbClass::Paused;
    }
    if state_value != "working" {
        return ActorAbsorbClass::None;
    }
    let source = line
        .split("source: ")
        .nth(1)
        .and_then(|value| value.split_whitespace().next())
        .unwrap_or_default();
    if matches!(source, "native-event" | "run-step" | "pane") {
        ActorAbsorbClass::Working
    } else {
        ActorAbsorbClass::None
    }
}

fn actor_absorb_class(source_root: &Path, state: &Path, task: &str) -> ActorAbsorbClass {
    actor_state_line(source_root, state, task)
        .as_deref()
        .map(parse_actor_absorb_class)
        .unwrap_or(ActorAbsorbClass::None)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecordedWindow {
    task: String,
    endpoint: String,
    backend: multplx_backend::facade::BackendName,
    kind: String,
}

fn metadata_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix(&format!("{key}=")))
        .next_back()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn recorded_windows(state: &Path) -> Vec<RecordedWindow> {
    let Ok(entries) = fs::read_dir(state) else {
        return Vec::new();
    };
    let mut windows = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "meta")
        })
        .filter_map(|path| {
            let task = path.file_stem()?.to_str()?.to_owned();
            let text = fs::read_to_string(path).ok()?;
            let endpoint =
                metadata_value(&text, "window").or_else(|| metadata_value(&text, "terminal"))?;
            let backend = multplx_backend::facade::BackendName::parse(
                metadata_value(&text, "backend")
                    .as_deref()
                    .unwrap_or("tmux"),
            )
            .ok()?;
            Some(RecordedWindow {
                task,
                endpoint,
                backend,
                kind: metadata_value(&text, "kind").unwrap_or_else(|| "delivery".to_owned()),
            })
        })
        .collect::<Vec<_>>();
    windows.sort_by(|left, right| left.endpoint.cmp(&right.endpoint));
    windows.dedup_by(|left, right| left.endpoint == right.endpoint);
    windows
}

fn backend_capture(window: &RecordedWindow) -> Option<(String, bool)> {
    use multplx_backend::facade::{
        BackendName, BackendTarget, CaptureRequest, NativeState, RuntimeBackend,
    };
    let target = BackendTarget::new(
        window.backend,
        window.endpoint.clone(),
        Some(format!("mx-{}", window.task)),
    )
    .ok()?;
    let request = CaptureRequest {
        target: target.clone(),
        lines: 40,
        byte_limit: 256 * 1024,
    };
    let (bytes, native) = match window.backend {
        BackendName::Tmux => {
            let mut backend = multplx_backend::tmux::TmuxBackend::system();
            let native = backend.native_state(&target).ok();
            (backend.capture(&request).ok()?, native)
        }
        BackendName::Herdr => {
            let mut backend = multplx_backend::herdr::HerdrBackend::system();
            let native = backend.native_state(&target).ok();
            (backend.capture(&request).ok()?, native)
        }
        BackendName::Cmux => {
            let mut backend = multplx_backend::cmux::CmuxBackend::system();
            let native = backend.native_state(&target).ok();
            (backend.capture(&request).ok()?, native)
        }
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let regex = std::env::var("MX_BUSY_REGEX")
        .unwrap_or_else(|_| "esc (to )?interrupt|Working(\\.\\.\\.)?|ctrl\\+c to stop".to_owned());
    let heuristic_busy = regex::RegexBuilder::new(&regex)
        .case_insensitive(true)
        .build()
        .ok()
        .is_some_and(|regex| {
            let tail = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            regex.is_match(&tail)
        });
    let busy = match native {
        Some(NativeState::Working) => true,
        Some(NativeState::Idle | NativeState::Blocked | NativeState::Done) => false,
        None => heuristic_busy,
    };
    Some((text, busy))
}

fn backend_agent_dead(window: &RecordedWindow) -> bool {
    use multplx_backend::facade::{AgentState, BackendName, BackendTarget, RuntimeBackend};
    let Ok(target) = BackendTarget::new(
        window.backend,
        window.endpoint.clone(),
        Some(format!("mx-{}", window.task)),
    ) else {
        return false;
    };
    let state = match window.backend {
        BackendName::Tmux => multplx_backend::tmux::TmuxBackend::system().agent_state(&target),
        BackendName::Herdr => multplx_backend::herdr::HerdrBackend::system().agent_state(&target),
        BackendName::Cmux => multplx_backend::cmux::CmuxBackend::system().agent_state(&target),
    };
    state == AgentState::Dead
}

fn backend_agent_alive(window: &RecordedWindow) -> bool {
    use multplx_backend::facade::{AgentState, BackendName, BackendTarget, RuntimeBackend};
    let Ok(target) = BackendTarget::new(
        window.backend,
        window.endpoint.clone(),
        Some(format!("mx-{}", window.task)),
    ) else {
        return false;
    };
    let state = match window.backend {
        BackendName::Tmux => {
            let output = Command::new("tmux")
                .args([
                    "display-message",
                    "-p",
                    "-t",
                    &window.endpoint,
                    "#{pane_current_command}",
                ])
                .output();
            if output.is_ok_and(|output| {
                output.status.success() && {
                    let command = String::from_utf8_lossy(&output.stdout);
                    let command = command.trim().trim_start_matches('-');
                    command.contains("claude") || command.contains("codex")
                }
            }) {
                AgentState::Alive
            } else {
                multplx_backend::tmux::TmuxBackend::system().agent_state(&target)
            }
        }
        BackendName::Herdr => multplx_backend::herdr::HerdrBackend::system().agent_state(&target),
        BackendName::Cmux => multplx_backend::cmux::CmuxBackend::system().agent_state(&target),
    };
    state == AgentState::Alive
}

fn window_key(window: &str) -> String {
    window.replace([':', '/', '.'], "_")
}

fn status_line(state: &Path, task: &str) -> String {
    multplx_core::classification::last_status_line(
        state.join(format!("{task}.status")),
        multplx_core::classification::STATUS_READ_LIMIT,
    )
    .ok()
    .flatten()
    .unwrap_or_default()
}

fn status_paused_or_held(line: &str) -> bool {
    matches!(
        multplx_core::classification::status_line_verb(line),
        "paused" | "maintainer-held"
    )
}

fn store_marker(path: &Path, value: &str) -> bool {
    multplx_core::filesystem::atomic_replace(path, value.as_bytes(), 0o600).is_ok()
}

fn triage_log(state: &Path, message: &str) {
    use std::io::Write;
    let path = state.join(".watch-triage.log");
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(file, "[{timestamp}] {message}");
    }
    let max = environment_u64("MX_WATCH_TRIAGE_LOG_MAX_BYTES", 262_144) as usize;
    let Ok(bytes) = fs::read(&path) else {
        return;
    };
    if bytes.len() < max {
        return;
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut bounded = text.lines().rev().take(2_000).collect::<Vec<_>>();
    bounded.reverse();
    let mut output = bounded.join("\n");
    output.push('\n');
    let _ = multplx_core::filesystem::atomic_replace(&path, output.as_bytes(), 0o600);
}

fn surface_stale(state: &Path, window: &RecordedWindow, hash: &str, reason: &str) -> bool {
    let key = window_key(&window.endpoint);
    if !append_wake(
        state,
        multplx_core::wake::WakeKind::Stale,
        &window.endpoint,
        reason,
    ) {
        return false;
    }
    if !store_marker(&state.join(format!(".stale-{key}")), hash) {
        return false;
    }
    let _ = fs::remove_file(state.join(format!(".stale-since-{key}")));
    mark_status_surfaced(state, &state.join(format!("{}.status", window.task)));
    true
}

fn clear_pause_tracking(state: &Path, key: &str) {
    for prefix in [".paused-", ".paused-rechecked-", ".paused-resurfaced-"] {
        let _ = fs::remove_file(state.join(format!("{prefix}{key}")));
    }
}

fn handle_paused_stale(
    state: &Path,
    window: &RecordedWindow,
    hash: &str,
    resurface: Duration,
) -> Option<String> {
    let key = window_key(&window.endpoint);
    let stale = state.join(format!(".stale-{key}"));
    let paused = state.join(format!(".paused-{key}"));
    if !store_marker(&stale, hash) || !store_marker(&paused, "") {
        return Some(format!("stale: {}", window.endpoint));
    }
    let _ = fs::remove_file(state.join(format!(".stale-since-{key}")));
    let _ = fs::remove_file(state.join(format!(".wedge-escalations-{key}")));
    let status_age = file_age(&state.join(format!("{}.status", window.task)));
    let resurfaced = state.join(format!(".paused-resurfaced-{key}"));
    if status_age >= resurface && file_age(&resurfaced) >= resurface {
        let reason = format!(
            "stale: {} (paused {}s, awaiting external - declared pause, rechecked on a long cadence not a wedge; confirm the wait still holds)",
            window.endpoint,
            status_age.as_secs()
        );
        if surface_stale(state, window, hash, &reason) {
            let _ = fs::write(resurfaced, b"");
            return Some(reason);
        }
        return Some(String::new());
    }
    None
}

fn pane_hash(capture: &str) -> Option<String> {
    let (program, args): (&str, &[&str]) = if program_available("md5") {
        ("md5", &["-q"])
    } else {
        ("md5sum", &[])
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    std::io::Write::write_all(
        child.stdin.as_mut()?,
        capture.trim_end_matches(['\r', '\n']).as_bytes(),
    )
    .ok()?;
    drop(child.stdin.take());
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
}

fn stale_scan(source_root: &Path, state: &Path) -> Option<String> {
    let afk = state.join(".afk").exists();
    let escalate = Duration::from_secs(environment_u64("MX_STALE_ESCALATE_SECS", 240));
    let resurface = Duration::from_secs(environment_u64("MX_PAUSE_RESURFACE_SECS", 21_600));
    let demand_count = environment_u64("MX_WEDGE_DEMAND_INSPECT_COUNT", 3);
    let override_regex = std::env::var("MX_MAINTAINER_RE").ok();
    for window in recorded_windows(state) {
        let last = status_line(state, &window.task);
        let key = window_key(&window.endpoint);
        if !status_paused_or_held(&last) && state.join(format!(".paused-{key}")).exists() {
            clear_pause_tracking(state, &key);
            let _ = fs::remove_file(state.join(format!(".stale-{key}")));
            let _ = fs::remove_file(state.join(format!(".stale-since-{key}")));
            let _ = fs::remove_file(state.join(format!(".wedge-escalations-{key}")));
        }
        if window.kind == "daemon"
            && multplx_core::classification::status_line_verb(&last) != "paused"
        {
            continue;
        }
        let Some((capture, busy)) = backend_capture(&window) else {
            continue;
        };
        let Some(hash) = pane_hash(&capture) else {
            continue;
        };
        let hash_file = state.join(format!(".hash-{key}"));
        let count_file = state.join(format!(".count-{key}"));
        let previous = fs::read_to_string(&hash_file).unwrap_or_default();
        if previous != hash {
            let _ = store_marker(&hash_file, &hash);
            let _ = store_marker(&count_file, "0\n");
            let _ = fs::remove_file(state.join(format!(".stale-since-{key}")));
            let _ = fs::remove_file(state.join(format!(".wedge-escalations-{key}")));
            if !afk && status_paused_or_held(&last) && !busy {
                if actor_absorb_class(source_root, state, &window.task) == ActorAbsorbClass::Paused
                    && let Some(reason) = handle_paused_stale(state, &window, &hash, resurface)
                {
                    return Some(reason);
                }
            } else {
                clear_pause_tracking(state, &key);
            }
            continue;
        }
        let count = fs::read_to_string(&count_file)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0)
            + 1;
        let _ = store_marker(&count_file, &format!("{count}\n"));
        let terminal = multplx_core::classification::is_maintainer_relevant(
            &last,
            override_regex.as_deref(),
            "paused",
        )
        .unwrap_or(true);
        if count < 2 || (busy && !terminal) {
            let _ = fs::remove_file(state.join(format!(".stale-since-{key}")));
            let _ = fs::remove_file(state.join(format!(".wedge-escalations-{key}")));
            continue;
        }
        let stale_file = state.join(format!(".stale-{key}"));
        let already = fs::read_to_string(&stale_file).is_ok_and(|value| value == hash);
        if afk {
            if !already {
                let reason = format!("stale: {}", window.endpoint);
                return surface_stale(state, &window, &hash, &reason).then_some(reason);
            }
            continue;
        }
        let mut class = if !already || status_paused_or_held(&last) {
            actor_absorb_class(source_root, state, &window.task)
        } else {
            ActorAbsorbClass::None
        };
        if class == ActorAbsorbClass::None
            && status_paused_or_held(&last)
            && window.kind != "daemon"
            && backend_agent_dead(&window)
        {
            class = ActorAbsorbClass::Paused;
        }
        if class == ActorAbsorbClass::Paused
            && window.kind != "daemon"
            && backend_agent_alive(&window)
        {
            if !already {
                let reason = format!("stale: {}", window.endpoint);
                if surface_stale(state, &window, &hash, &reason) {
                    let _ = store_marker(&state.join(format!(".paused-{key}")), "");
                    let _ = store_marker(&state.join(format!(".paused-rechecked-{key}")), "");
                    let _ = store_marker(&state.join(format!(".paused-resurfaced-{key}")), "");
                    return Some(reason);
                }
                return Some(String::new());
            }
            let _ = fs::remove_file(state.join(format!(".stale-since-{key}")));
            continue;
        }
        if class == ActorAbsorbClass::Working {
            clear_pause_tracking(state, &key);
            let _ = store_marker(&stale_file, &hash);
            let since_file = state.join(format!(".stale-since-{key}"));
            if fs::read_to_string(&since_file)
                .ok()
                .and_then(|value| value.trim().parse::<u64>().ok())
                .is_none()
            {
                let epoch = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let _ = store_marker(&since_file, &format!("{epoch}\n"));
                continue;
            }
            class = ActorAbsorbClass::None;
        }
        match class {
            ActorAbsorbClass::Paused => {
                if let Some(reason) = handle_paused_stale(state, &window, &hash, resurface) {
                    return Some(reason);
                }
            }
            _ if !already => {
                let reason = format!("stale: {}", window.endpoint);
                if surface_stale(state, &window, &hash, &reason) {
                    if status_paused_or_held(&last) {
                        let _ = store_marker(&state.join(format!(".paused-{key}")), "");
                        let _ = store_marker(&state.join(format!(".paused-rechecked-{key}")), "");
                        let _ = store_marker(&state.join(format!(".paused-resurfaced-{key}")), "");
                    }
                    return Some(reason);
                }
                return Some(String::new());
            }
            _ => {
                let since_file = state.join(format!(".stale-since-{key}"));
                let since = fs::read_to_string(&since_file)
                    .ok()
                    .and_then(|value| value.trim().parse::<u64>().ok());
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let Some(since) = since else {
                    let _ = store_marker(&since_file, &format!("{now}\n"));
                    continue;
                };
                if now.saturating_sub(since) < escalate.as_secs() {
                    continue;
                }
                let escalations = state.join(format!(".wedge-escalations-{key}"));
                let count = fs::read_to_string(&escalations)
                    .ok()
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .unwrap_or(0)
                    + 1;
                let _ = store_marker(&escalations, &format!("{count}\n"));
                let detail = if count >= demand_count {
                    format!(
                        ", demand-deep-inspection: same pane has wedge-escalated {count} times in a row - do not re-absorb on the run-step/pane state alone"
                    )
                } else {
                    String::new()
                };
                let reason = format!(
                    "stale: {} (idle {}s, possible wedge, escalation {count}{detail})",
                    window.endpoint,
                    now.saturating_sub(since)
                );
                if surface_stale(state, &window, &hash, &reason) {
                    return Some(reason);
                }
                return Some(String::new());
            }
        }
    }
    None
}

fn backend_event_wait(state: &Path, poll: Duration) -> Result<Option<String>, String> {
    let windows = recorded_windows(state)
        .into_iter()
        .filter(|window| {
            window.backend == multplx_backend::facade::BackendName::Herdr && window.kind != "daemon"
        })
        .collect::<Vec<_>>();
    let Some(first) = windows.first() else {
        return Ok(None);
    };
    let Some(session) = first.endpoint.split(':').next() else {
        return Ok(None);
    };
    let endpoints = windows
        .iter()
        .filter(|window| window.endpoint.split(':').next() == Some(session))
        .map(|window| window.endpoint.clone())
        .collect::<Vec<_>>();
    let mut backend = multplx_backend::herdr::HerdrBackend::system();
    let record = backend
        .wait_transition_in_state(session, poll, state, &endpoints)
        .map_err(|error| error.to_string())?;
    let Some(record) = record else {
        return Ok(None);
    };
    let window = format!("{session}:{}", record.pane_id);
    let task = windows
        .iter()
        .find(|candidate| candidate.endpoint == window)
        .map(|candidate| candidate.task.as_str())
        .unwrap_or_default();
    let last = status_line(state, task);
    let reason = if multplx_core::classification::status_line_verb(&last) == "paused" {
        format!(
            "stale: {window} (native-event={}; herdr: agent {} - native event overruled declared pause, waiting on human)",
            record.to_status, record.to_status
        )
    } else {
        format!(
            "stale: {window} (native-event={}; herdr: agent {} - waiting on human, escalated immediately, not via wedge timer)",
            record.to_status, record.to_status
        )
    };
    if append_wake(state, multplx_core::wake::WakeKind::Stale, &window, &reason)
        && multplx_backend::herdr::commit_transition(state, session, &record).is_ok()
    {
        mark_status_surfaced(state, &state.join(format!("{task}.status")));
        Ok(Some(reason))
    } else {
        Err("could not durably commit native transition wake".to_owned())
    }
}

fn event_failure_state(failures: u64, maximum: u64) -> (u64, bool) {
    let failures = failures.saturating_add(1);
    (failures, failures >= maximum.max(1))
}

fn pending_reply_observation(state: &Path, task: &str) -> &'static str {
    use multplx_backend::facade::{BackendName, BackendTarget, NativeState, RuntimeBackend};
    let Some(window) = recorded_windows(state)
        .into_iter()
        .find(|window| window.task == task)
    else {
        return "unknown";
    };
    let Ok(target) =
        BackendTarget::new(window.backend, window.endpoint, Some(format!("mx-{task}")))
    else {
        return "unknown";
    };
    let native = match window.backend {
        BackendName::Tmux => multplx_backend::tmux::TmuxBackend::system().native_state(&target),
        BackendName::Herdr => multplx_backend::herdr::HerdrBackend::system().native_state(&target),
        BackendName::Cmux => multplx_backend::cmux::CmuxBackend::system().native_state(&target),
    };
    match native {
        Ok(NativeState::Working) => "busy",
        Ok(NativeState::Idle | NativeState::Blocked | NativeState::Done) => "idle",
        Err(_) => "unknown",
    }
}

fn signal_actors_provably_working(
    source_root: &Path,
    state: &Path,
    observations: &[SignalObservation],
) -> bool {
    let tasks = observations
        .iter()
        .filter_map(|observation| observation_task(&observation.path))
        .collect::<std::collections::BTreeSet<_>>();
    !tasks.is_empty()
        && tasks
            .into_iter()
            .all(|task| actor_absorb_class(source_root, state, task) == ActorAbsorbClass::Working)
}

#[derive(Debug)]
enum AuthenticatedCheck {
    PrPoll {
        task: multplx_domain::review_delivery::OperationalTaskId,
        snapshot: Box<PrPollSnapshot>,
    },
    Custom {
        task: multplx_core::identifiers::TaskId,
        snapshot: multplx_core::checks::CheckSnapshot,
    },
    Rejected(std::path::PathBuf),
}

#[derive(Debug)]
struct PrPollSnapshot {
    registration: multplx_domain::review_delivery::PollRegistration,
    registration_identity: multplx_domain::review_delivery::FileIdentity,
    registration_digest: multplx_core::identifiers::Sha256Digest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrPollRetirement {
    task: multplx_domain::review_delivery::OperationalTaskId,
    identity: multplx_domain::review_delivery::PrIdentity,
    data_hash: multplx_core::identifiers::Sha256Digest,
    template_hash: multplx_core::identifiers::Sha256Digest,
    data_identity: multplx_domain::review_delivery::FileIdentity,
    check_identity: multplx_domain::review_delivery::FileIdentity,
    registration_hash: multplx_core::identifiers::Sha256Digest,
    registration_identity: multplx_domain::review_delivery::FileIdentity,
}

impl PrPollRetirement {
    fn from_snapshot(snapshot: &PrPollSnapshot) -> Self {
        Self {
            task: snapshot.registration.task.clone(),
            identity: snapshot.registration.identity.clone(),
            data_hash: snapshot.registration.data_hash.clone(),
            template_hash: snapshot.registration.template_hash.clone(),
            data_identity: snapshot.registration.data_identity.clone(),
            check_identity: snapshot.registration.check_identity.clone(),
            registration_hash: snapshot.registration_digest.clone(),
            registration_identity: snapshot.registration_identity.clone(),
        }
    }

    fn render(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\nmerged\n",
            multplx_domain::review_delivery::POLL_RETIREMENT_VERSION,
            self.task,
            self.identity.provider,
            self.identity.url,
            self.identity.host,
            self.identity.project_path(),
            self.identity.number,
            self.data_hash.as_str(),
            self.template_hash.as_str(),
            self.data_identity.render(),
            self.check_identity.render(),
            self.registration_hash.as_str(),
            self.registration_identity.render(),
        )
    }

    fn parse(bytes: &[u8]) -> Result<Self, String> {
        use multplx_domain::review_delivery::{
            FileIdentity, OperationalTaskId, POLL_RETIREMENT_VERSION, PrIdentity,
        };
        let text = std::str::from_utf8(bytes).map_err(|_| "retirement is not UTF-8")?;
        let lines = text.split_terminator('\n').collect::<Vec<_>>();
        if !text.ends_with('\n')
            || lines.len() != 14
            || lines[0] != POLL_RETIREMENT_VERSION
            || lines[13] != "merged"
        {
            return Err("invalid poll retirement shape".to_owned());
        }
        let task = OperationalTaskId::parse(lines[1].to_owned())?;
        let identity = PrIdentity::parse(lines[3])?;
        if lines[2] != identity.provider
            || lines[4] != identity.host
            || lines[5] != identity.project_path()
            || lines[6] != identity.number
        {
            return Err("retirement identity mismatch".to_owned());
        }
        Ok(Self {
            task,
            identity,
            data_hash: multplx_core::identifiers::Sha256Digest::parse(lines[7])
                .map_err(|error| error.to_string())?,
            template_hash: multplx_core::identifiers::Sha256Digest::parse(lines[8])
                .map_err(|error| error.to_string())?,
            data_identity: FileIdentity::parse(lines[9])?,
            check_identity: FileIdentity::parse(lines[10])?,
            registration_hash: multplx_core::identifiers::Sha256Digest::parse(lines[11])
                .map_err(|error| error.to_string())?,
            registration_identity: FileIdentity::parse(lines[12])?,
        })
    }
}

fn pr_poll_snapshot(
    state: &Path,
    source_root: &Path,
    task: &str,
) -> Result<PrPollSnapshot, String> {
    use multplx_domain::review_delivery::{
        OperationalTaskId, PollRegistration, PrIdentity, metadata_pr, read_private,
    };
    let task = OperationalTaskId::parse(task.to_owned())?;
    let state_metadata = fs::symlink_metadata(state).map_err(|error| error.to_string())?;
    if !state_metadata.is_dir() || state_metadata.file_type().is_symlink() {
        return Err("state directory is unsafe".to_owned());
    }
    let check = read_private(
        &state.join(format!("{task}.check.sh")),
        0o600,
        state_metadata.dev(),
    )?;
    let data = read_private(
        &state.join(format!("{task}.pr-poll")),
        0o600,
        state_metadata.dev(),
    )?;
    let registration = read_private(
        &state.join(format!("{task}.pr-poll-registration")),
        0o600,
        state_metadata.dev(),
    )?;
    let template =
        fs::read(source_root.join("bin/mx-pr-poll.sh")).map_err(|error| error.to_string())?;
    if check.bytes != template {
        return Err("poll check does not match the installed template".to_owned());
    }
    let identity = PrIdentity::parse_sidecar(&data.bytes)?;
    let parsed = PollRegistration::parse(&registration.bytes)?;
    if parsed.task != task
        || parsed.identity != identity
        || parsed.data_hash != data.digest
        || parsed.template_hash != check.digest
        || parsed.data_identity != data.identity
        || parsed.check_identity != check.identity
    {
        return Err("poll registration does not bind its artifacts".to_owned());
    }
    let meta_path = state.join(format!("{task}.meta"));
    let meta = fs::symlink_metadata(&meta_path).map_err(|error| error.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.nlink() != 1 {
        return Err("poll metadata is unsafe".to_owned());
    }
    if metadata_pr(&fs::read(meta_path).map_err(|error| error.to_string())?)? != identity {
        return Err("poll metadata identity mismatch".to_owned());
    }
    Ok(PrPollSnapshot {
        registration: parsed,
        registration_identity: registration.identity,
        registration_digest: registration.digest,
    })
}

fn authenticated_checks(state: &Path, source_root: &Path) -> Vec<AuthenticatedCheck> {
    let Ok(entries) = fs::read_dir(state) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with(".check.sh"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let task = name.trim_end_matches(".check.sh");
            if let Ok(snapshot) = pr_poll_snapshot(state, source_root, task) {
                return AuthenticatedCheck::PrPoll {
                    task: snapshot.registration.task.clone(),
                    snapshot: Box::new(snapshot),
                };
            }
            let Ok(task) = multplx_core::identifiers::TaskId::parse(task) else {
                return AuthenticatedCheck::Rejected(path);
            };
            match multplx_core::checks::CheckSnapshot::prepare(state, &task) {
                Ok(snapshot) => AuthenticatedCheck::Custom { task, snapshot },
                Err(_) => AuthenticatedCheck::Rejected(path),
            }
        })
        .collect()
}

fn environment_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn environment_duration(name: &str, default_secs: f64) -> Duration {
    let seconds = std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(default_secs);
    Duration::from_secs_f64(seconds)
}

fn command_payload(path: &Path, args: &[&str], payload: &str) -> Result<(i32, String), String> {
    let mut child = Command::new(path)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(payload.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((exit_status_code(output.status), text))
}

fn cursor_json(value: serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string(&value).expect("JSON value renders")
    );
}

fn cursor_deny(message: &str) {
    cursor_json(serde_json::json!({
        "permission": "deny",
        "user_message": message,
        "agent_message": message,
    }));
}

/// Translate Cursor's hook protocol onto the shared Multplx guard owners.
pub(crate) fn cursor_hook(args: &[std::ffi::OsString], payload: &str, source_root: &Path) -> i32 {
    let Some(mode) = args.first().and_then(|arg| arg.to_str()) else {
        eprintln!("usage: mx-cursor-hook.sh session-start|pre-tool|subagent-start|stop");
        return 2;
    };
    if args.len() != 1 {
        eprintln!("usage: mx-cursor-hook.sh session-start|pre-tool|subagent-start|stop");
        return 2;
    }
    let bin = source_root.join("bin");
    match mode {
        "session-start" => {
            if payload.is_empty() {
                return 1;
            }
            let context = command_payload(&bin.join("mx-sessionstart-nudge.sh"), &[], payload)
                .ok()
                .map(|(_, output)| output.trim_end().to_owned())
                .unwrap_or_default();
            if context.is_empty() {
                cursor_json(serde_json::json!({}));
            } else {
                cursor_json(serde_json::json!({"additional_context": context}));
            }
            0
        }
        "pre-tool" => {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                return 1;
            };
            if !value.is_object()
                || value
                    .get("tool_name")
                    .and_then(serde_json::Value::as_str)
                    .is_none()
                || !value
                    .get("tool_input")
                    .is_some_and(serde_json::Value::is_object)
            {
                return 1;
            }
            for guard in [
                "mx-arm-pretool-check.sh",
                "mx-cd-pretool-check.sh",
                "mx-subagent-pretool-check.sh",
            ] {
                match command_payload(&bin.join(guard), &[], payload) {
                    Ok((2, output)) => {
                        cursor_deny(output.trim_end());
                        return 0;
                    }
                    Ok((0, _)) => {}
                    _ => return 1,
                }
            }
            cursor_json(serde_json::json!({"permission": "allow"}));
            0
        }
        "subagent-start" => {
            if payload.is_empty() {
                return 1;
            }
            match command_payload(
                &bin.join("mx-subagent-pretool-check.sh"),
                &["--tool", "subagentStart"],
                "",
            ) {
                Ok((2, output)) => cursor_deny(output.trim_end()),
                Ok((0, _)) => cursor_json(serde_json::json!({"permission": "allow"})),
                _ => return 1,
            }
            0
        }
        "stop" => {
            let Ok(mut value) = serde_json::from_str::<serde_json::Value>(payload) else {
                cursor_json(serde_json::json!({}));
                return 0;
            };
            let loop_count = value
                .get("loop_count")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            if payload.is_empty() || loop_count != 0 {
                cursor_json(serde_json::json!({}));
                return 0;
            }
            let Some(object) = value.as_object_mut() else {
                cursor_json(serde_json::json!({}));
                return 0;
            };
            object.insert(
                "stop_hook_active".to_owned(),
                serde_json::Value::Bool(false),
            );
            let guard_payload = serde_json::to_string(&value).expect("JSON value renders");
            match command_payload(&bin.join("mx-turnend-guard.sh"), &[], &guard_payload) {
                Ok((2, output)) => {
                    cursor_json(serde_json::json!({"followup_message": output.trim_end()}));
                }
                _ => cursor_json(serde_json::json!({})),
            }
            0
        }
        _ => {
            eprintln!("usage: mx-cursor-hook.sh session-start|pre-tool|subagent-start|stop");
            2
        }
    }
}

fn autoarm_needed(state: &Path) -> bool {
    fs::read_dir(state).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("meta"))
    })
}

fn write_autoarm_epoch(state: &Path, outcome: &str) {
    let path = state.join(".claude-autoarm-epoch");
    let sequence = fs::read_to_string(&path)
        .ok()
        .and_then(|text| {
            text.split_whitespace()
                .find_map(|field| field.strip_prefix("epoch="))
                .and_then(|value| value.parse::<u64>().ok())
        })
        .unwrap_or(0)
        .saturating_add(1);
    let updated = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let record = format!(
        "epoch={sequence} owner_pid={} outcome={outcome} updated_at={updated}\n",
        std::process::id()
    );
    let _ = multplx_core::filesystem::atomic_replace(&path, record.as_bytes(), 0o600);
}

fn autoarm_session_owned(state: &Path) -> bool {
    let Ok(owner) = fs::read_to_string(state.join(".lock")) else {
        return false;
    };
    let Ok(owner) = owner.trim().parse::<u32>() else {
        return false;
    };
    owner == std::process::id() || autoarm_owner_pid() == owner
}

fn autoarm_harness_pid() -> Option<u32> {
    let matcher = multplx_core::session_lock::harness_regex();
    let mut pid = std::process::id();
    for _ in 0..8 {
        let output = Command::new("ps")
            .args([
                "-p",
                &pid.to_string(),
                "-o",
                "ppid=",
                "-o",
                "comm=",
                "-o",
                "args=",
            ])
            .output()
            .ok()?;
        let text = String::from_utf8(output.stdout).ok()?;
        let mut fields = text.split_whitespace();
        let parent = fields.next()?.parse::<u32>().ok()?;
        let command = fields.next()?;
        let arguments = fields.collect::<Vec<_>>().join(" ");
        let basename = PathBuf::from(command)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(command)
            .to_owned();
        let interpreter = (command.contains("node") || command.contains("python"))
            && matcher.is_match(&arguments);
        if matcher.is_match(&basename) || interpreter {
            return Some(pid);
        }
        if arguments.contains("mx-claude-stop-autoarm.sh")
            || arguments.contains("supervision mx-claude-stop-autoarm.sh")
        {
            pid = parent;
            continue;
        }
        if parent <= 1 {
            return None;
        }
        pid = parent;
    }
    None
}

fn autoarm_owner_pid() -> u32 {
    autoarm_harness_pid()
        .or_else(|| {
            SystemProcessProbe::default()
                .ancestry_row(std::process::id())
                .ok()
                .map(|row| row.parent_pid)
                .filter(|pid| *pid > 1)
        })
        .unwrap_or_else(std::process::id)
}

/// Own one Claude async-Rewake watcher cycle and translate its close.
pub(crate) fn claude_stop_autoarm(root: &Path, home: &Path, source_root: &Path) -> i32 {
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if !multplx_core::primary_scope::matches(root, &state) {
        return 0;
    }
    let processes = SystemProcessProbe::default();
    if !autoarm_session_owned(&state) {
        let status = multplx_core::session_lock::status(
            state.join(".lock"),
            &processes,
            &multplx_core::session_lock::harness_regex(),
        );
        if !matches!(
            status,
            multplx_core::session_lock::SessionLockStatus::Stale(_)
        ) || state.join(".afk").exists()
            || !autoarm_needed(&state)
        {
            return 0;
        }
        let owner = autoarm_owner_pid();
        let recovered =
            multplx_core::session_lock::SessionLock::new(state.join(".lock"), &processes)
                .acquire(owner)
                .is_ok();
        if !recovered || !autoarm_session_owned(&state) {
            return 0;
        }
    }
    if state.join(".afk").exists() || !autoarm_needed(&state) {
        return 0;
    }
    let _owner = match DirectoryLock::try_acquire(
        state.join(".claude-autoarm.lock"),
        &SystemProcessProbe::default(),
    ) {
        Ok(lock) => lock,
        Err(_) => return 0,
    };
    write_autoarm_epoch(&state, "arming");
    let output = Command::new(source_root.join("bin/mx-watch-arm.sh"))
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", &state)
        .output();
    if state.join(".afk").exists() {
        write_autoarm_epoch(&state, "afk");
        return 0;
    }
    let (code, text) = match output {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (exit_status_code(output.status), text)
        }
        Err(error) => (
            1,
            format!("watcher: FAILED - cannot run watcher arm: {error}\n"),
        ),
    };
    let actionable = actionable_output(&text).is_some();
    let failed = code != 0 || text.lines().any(|line| line.starts_with("watcher: FAILED"));
    if (!actionable && !failed) || !autoarm_needed(&state) {
        write_autoarm_epoch(&state, "clean");
        return 0;
    }
    write_autoarm_epoch(&state, "rewake");
    let selected = text
        .lines()
        .filter(|line| {
            line.starts_with("watcher:")
                || line.starts_with("signal:")
                || line.starts_with("stale:")
                || line.starts_with("check:")
                || line.starts_with("heartbeat")
        })
        .take(8)
        .collect::<Vec<_>>()
        .join("\n");
    if failed {
        eprintln!(
            "broker watcher cycle FAILED - supervision is down while this home still needs it."
        );
        if !selected.is_empty() {
            eprintln!("{selected}");
        }
        eprintln!(
            "Run bin/mx-wake-drain.sh first. Then repair supervision with bin/mx-watch-arm.sh as its own Claude Code background task (never shell &). If the failure repeats, treat it as a blocker and report it instead of ending blind."
        );
    } else {
        eprintln!("broker watcher wake - one supervision event needs a handling turn now.");
        if !selected.is_empty() {
            eprintln!("{selected}");
        }
        eprintln!(
            "Run bin/mx-wake-drain.sh first and handle the wake. This Stop hook owns watcher continuity: when the handling turn ends, the next needed cycle arms automatically - do NOT run bin/mx-watch-arm.sh after an ordinary wake."
        );
    }
    2
}

fn daemon_lock_owner(state: &Path) -> Option<PathBuf> {
    let lock = state.join(".supervise-daemon.lock");
    let metadata = fs::symlink_metadata(&lock).ok()?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&lock).ok()?;
        return Some(if target.is_absolute() {
            target
        } else {
            state.join(target)
        });
    }
    metadata.is_dir().then_some(lock)
}

fn live_daemon_pid(state: &Path, source_root: &Path) -> Option<u32> {
    let owner = daemon_lock_owner(state)?;
    let pid = fs::read_to_string(owner.join("pid"))
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    let processes = SystemProcessProbe::default();
    if let Ok(expected) = fs::read_to_string(owner.join("pid-identity"))
        && !expected.trim().is_empty()
    {
        return processes
            .identity(pid)
            .ok()
            .filter(|identity| identity.marker == expected.trim())
            .map(|_| pid);
    }
    let row = processes.ancestry_row(pid).ok()?;
    let daemon = source_root.join("bin/mx-supervise-daemon.sh");
    (row.arguments.contains(daemon.to_string_lossy().as_ref())
        || row.arguments.contains("mx-supervise-daemon.sh"))
    .then_some(pid)
}

fn clear_stale_afk_artifacts(state: &Path) {
    for name in [
        ".subsuper-escalations",
        ".subsuper-escalations.since",
        ".subsuper-inject-wedged",
    ] {
        let _ = fs::remove_file(state.join(name));
    }
}

/// Enter away mode and foreground the daemon in the harness-owned process tree.
pub(crate) fn afk_start(args: &[std::ffi::OsString], home: &Path, source_root: &Path) -> i32 {
    if args.len() == 1 && matches!(args[0].to_str(), Some("-h" | "--help")) {
        println!(
            "Enter away mode and run the sub-supervisor daemon in a harness-tracked foreground process when one is not already alive."
        );
        return 0;
    }
    if !args.is_empty() {
        eprintln!("usage: mx-afk-start.sh");
        return 2;
    }
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if fs::create_dir_all(&state).is_err() {
        return 1;
    }
    let prepared = std::env::var("MX_AFK_STATE_PREPARED").as_deref() == Ok("1");
    if prepared {
        if !state.join(".afk").is_file() {
            eprintln!("afk: launcher-prepared state is missing");
            return 1;
        }
    } else if fs::write(
        state.join(".afk"),
        format!(
            "{}\n",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ),
    )
    .is_err()
    {
        return 1;
    }
    if let Some(pid) = live_daemon_pid(&state, source_root) {
        println!("afk: daemon already running pid={pid}");
        return 0;
    }
    if let Some(owner) = daemon_lock_owner(&state) {
        if let Ok(lock) = DirectoryLock::try_acquire(
            state.join(".supervise-daemon.lock"),
            &SystemProcessProbe::default(),
        ) {
            drop(lock);
        } else if owner.exists() {
            let pid = fs::read_to_string(owner.join("pid"))
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok());
            let stale = pid.is_none_or(|pid| !SystemProcessProbe::default().is_alive(pid))
                || fs::read_to_string(owner.join("pid-identity"))
                    .ok()
                    .is_some_and(|expected| {
                        pid.and_then(|pid| SystemProcessProbe::default().identity(pid).ok())
                            .is_none_or(|identity| identity.marker != expected.trim())
                    });
            if !stale {
                return 1;
            }
            for name in ["pid", "pid-identity"] {
                let _ = fs::remove_file(owner.join(name));
            }
            let _ = fs::remove_dir(&owner);
        }
    }
    if !prepared {
        clear_stale_afk_artifacts(&state);
    }
    println!(
        "afk: starting supervise daemon in foreground; keep this command as a tracked background session"
    );
    use std::os::unix::process::CommandExt;
    let error = Command::new(source_root.join("bin/mx-supervise-daemon.sh"))
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", &state)
        .exec();
    eprintln!("afk: could not start supervise daemon: {error}");
    1
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AfkTerminalRecord {
    backend: String,
    target: String,
    extra: String,
}

fn afk_record_path(state: &Path) -> PathBuf {
    state.join(".afk-daemon-terminal")
}

fn afk_record_read(state: &Path) -> Result<Option<AfkTerminalRecord>, String> {
    let path = afk_record_path(state);
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let rows = text.lines().collect::<Vec<_>>();
    if rows.len() != 1 {
        return Err("daemon terminal record is malformed; refusing to act on it".to_owned());
    }
    let fields = rows[0].split('\t').collect::<Vec<_>>();
    if fields.len() != 3 || fields[0].is_empty() || fields[1].is_empty() {
        return Err("daemon terminal record is malformed; refusing to act on it".to_owned());
    }
    let valid = match fields[0] {
        "herdr" => fields[1].contains(':') && !fields[2].is_empty(),
        "tmux" => true,
        "none" => fields[1] == "-" && fields[2] == "native",
        _ => false,
    };
    if !valid {
        return Err("daemon terminal record is malformed; refusing to act on it".to_owned());
    }
    Ok(Some(AfkTerminalRecord {
        backend: fields[0].to_owned(),
        target: fields[1].to_owned(),
        extra: fields[2].to_owned(),
    }))
}

fn afk_record_write(state: &Path, record: &AfkTerminalRecord) -> Result<(), String> {
    let text = format!("{}\t{}\t{}\n", record.backend, record.target, record.extra);
    multplx_core::filesystem::atomic_replace(afk_record_path(state), text.as_bytes(), 0o600)
        .map_err(|error| error.to_string())
}

fn afk_terminal_absent(record: &AfkTerminalRecord) -> bool {
    match record.backend.as_str() {
        "none" => true,
        "tmux" => Command::new("tmux")
            .args(["has-session", "-t", &record.target])
            .output()
            .is_ok_and(|output| {
                output.status.code() == Some(1)
                    && String::from_utf8_lossy(&output.stderr).contains("can't find session")
            }),
        "herdr" => {
            let Some((session, pane)) = record.target.split_once(':') else {
                return false;
            };
            Command::new("herdr")
                .args(["pane", "get", pane, "--session", session])
                .env("HERDR_SESSION", session)
                .output()
                .ok()
                .filter(|output| !output.status.success())
                .and_then(|output| {
                    [&output.stdout, &output.stderr]
                        .into_iter()
                        .find_map(|bytes| {
                            let value = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
                            value.pointer("/error/code")?.as_str().map(str::to_owned)
                        })
                })
                .as_deref()
                == Some("pane_not_found")
        }
        _ => false,
    }
}

fn afk_close_recorded(state: &Path, record: &AfkTerminalRecord) -> bool {
    match record.backend.as_str() {
        "none" => {}
        "tmux" => {
            let _ = Command::new("tmux")
                .args(["kill-session", "-t", &record.target])
                .status();
        }
        "herdr" => {
            let Some((session, pane)) = record.target.split_once(':') else {
                return false;
            };
            let _ = Command::new("herdr")
                .args(["pane", "close", pane, "--session", session])
                .env("HERDR_SESSION", session)
                .status();
        }
        _ => return false,
    }
    let mut absent = false;
    for _ in 0..40 {
        if afk_terminal_absent(record) {
            absent = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !absent {
        eprintln!(
            "mx-afk-launch: recorded {} terminal {} did not disappear after close",
            record.backend, record.target
        );
        return false;
    }
    fs::remove_file(afk_record_path(state)).is_ok() || !afk_record_path(state).exists()
}

fn afk_reconcile(state: &Path, source_root: &Path) -> bool {
    if live_daemon_pid(state, source_root).is_some() {
        return true;
    }
    match afk_record_read(state) {
        Ok(Some(record)) => afk_close_recorded(state, &record),
        Ok(None) => true,
        Err(error) => {
            eprintln!("mx-afk-launch: {error}");
            false
        }
    }
}

fn afk_launch_flag(state: &Path) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    multplx_core::filesystem::atomic_replace(
        state.join(".afk"),
        format!("{now}\n").as_bytes(),
        0o600,
    )
    .is_ok()
}

fn afk_launch_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn afk_launch_entry(home: &Path, target: &str, backend: &str, source_root: &Path) -> String {
    let entry = std::env::var_os("MX_AFK_LAUNCH_ENTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| source_root.join("bin/mx-afk-start.sh"));
    format!(
        "exec env MX_AFK_STATE_PREPARED=1 MX_HOME={} MX_SUPERVISOR_TARGET={} MX_SUPERVISOR_BACKEND={} {}",
        afk_launch_quote(home.to_string_lossy().as_ref()),
        afk_launch_quote(target),
        afk_launch_quote(backend),
        afk_launch_quote(entry.to_string_lossy().as_ref())
    )
}

fn afk_launch_tmux(state: &Path, home: &Path, target: &str, source_root: &Path) -> bool {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let session = format!("mx-afk-daemon-{}-{nonce}", std::process::id());
    let record = AfkTerminalRecord {
        backend: "tmux".to_owned(),
        target: session.clone(),
        extra: String::new(),
    };
    if afk_record_write(state, &record).is_err() {
        return false;
    }
    let command = afk_launch_entry(home, target, "tmux", source_root);
    if !Command::new("tmux")
        .args(["new-session", "-d", "-s", &session, &command])
        .status()
        .is_ok_and(|status| status.success())
    {
        let _ = fs::remove_file(afk_record_path(state));
        return false;
    }
    true
}

fn afk_herdr_output(session: &str, args: &[&str]) -> Option<std::process::Output> {
    Command::new("herdr")
        .args(args)
        .args(["--session", session])
        .env("HERDR_SESSION", session)
        .output()
        .ok()
}

fn afk_herdr_recover_created(session: &str, label: &str) -> Option<(String, String)> {
    for _ in 0..20 {
        let workspaces = afk_herdr_output(session, &["workspace", "list"])?;
        let value = serde_json::from_slice::<serde_json::Value>(&workspaces.stdout).ok()?;
        let matches = value
            .pointer("/result/workspaces")?
            .as_array()?
            .iter()
            .filter(|workspace| {
                workspace.get("label").and_then(serde_json::Value::as_str) == Some(label)
            })
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            let workspace = matches[0].get("workspace_id")?.as_str()?.to_owned();
            let panes = afk_herdr_output(session, &["pane", "list", "--workspace", &workspace])?;
            let panes = serde_json::from_slice::<serde_json::Value>(&panes.stdout).ok()?;
            let panes = panes.pointer("/result/panes")?.as_array()?;
            if panes.len() == 1 {
                return Some((workspace, panes[0].get("pane_id")?.as_str()?.to_owned()));
            }
        } else if matches.len() > 1 {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn afk_launch_herdr(state: &Path, home: &Path, target: &str, source_root: &Path) -> bool {
    let Some((session, _)) = target.split_once(':') else {
        return false;
    };
    let label = std::env::var("MX_AFK_LAUNCH_LABEL").unwrap_or_else(|_| {
        format!(
            "broker-afk-daemon-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    });
    let cwd = home.to_string_lossy();
    let Some(output) = afk_herdr_output(
        session,
        &[
            "workspace",
            "create",
            "--cwd",
            &cwd,
            "--label",
            &label,
            "--no-focus",
        ],
    ) else {
        return false;
    };
    let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
    let exact = parsed.as_ref().and_then(|value| {
        Some((
            value
                .pointer("/result/workspace/workspace_id")?
                .as_str()?
                .to_owned(),
            value
                .pointer("/result/root_pane/pane_id")?
                .as_str()?
                .to_owned(),
        ))
    });
    let Some((workspace, pane)) = exact.or_else(|| afk_herdr_recover_created(session, &label))
    else {
        return false;
    };
    let record = AfkTerminalRecord {
        backend: "herdr".to_owned(),
        target: format!("{session}:{pane}"),
        extra: workspace,
    };
    if afk_record_write(state, &record).is_err() {
        let _ = afk_herdr_output(session, &["pane", "close", &pane]);
        return false;
    }
    if !output.status.success() {
        let _ = afk_close_recorded(state, &record);
        return false;
    }
    let command = afk_launch_entry(home, target, "herdr", source_root);
    if !afk_herdr_output(session, &["pane", "run", &pane, &command])
        .is_some_and(|output| output.status.success())
    {
        let _ = afk_close_recorded(state, &record);
        return false;
    }
    true
}

fn afk_launch_start_native(state: &Path, source_root: &Path) -> bool {
    if !afk_reconcile(state, source_root) {
        return false;
    }
    clear_stale_afk_artifacts(state);
    afk_launch_flag(state)
        && afk_record_write(
            state,
            &AfkTerminalRecord {
                backend: "none".to_owned(),
                target: "-".to_owned(),
                extra: "native".to_owned(),
            },
        )
        .is_ok()
}

/// Own away-mode lifecycle locking, native preparation, exact tmux launch, and stop.
pub(crate) fn afk_launch(args: &[std::ffi::OsString], home: &Path, source_root: &Path) -> i32 {
    let mode = args
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or("start");
    if args.len() > 1
        || !matches!(
            mode,
            "start" | "start-native" | "stop" | "reconcile" | "-h" | "--help" | "help"
        )
    {
        eprintln!("usage: mx-afk-launch.sh start|start-native|stop|reconcile");
        return 2;
    }
    if matches!(mode, "-h" | "--help" | "help") {
        println!("usage: mx-afk-launch.sh start|start-native|stop|reconcile");
        return 0;
    }
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if fs::create_dir_all(&state).is_err() {
        return 1;
    }
    let _lock = match DirectoryLock::acquire_wait(
        state.join(".afk-launch.lock"),
        &SystemProcessProbe::default(),
        Duration::from_secs(10),
    ) {
        Ok(lock) => lock,
        Err(_) => return 1,
    };
    if mode == "reconcile" {
        return i32::from(!afk_reconcile(&state, source_root));
    }
    if mode == "stop" {
        let record = match afk_record_read(&state) {
            Ok(record) => record,
            Err(error) => {
                eprintln!("mx-afk-launch: {error}");
                return 1;
            }
        };
        if let Some(pid) = live_daemon_pid(&state, source_root) {
            let Ok(identity) = SystemProcessProbe::default().identity(pid) else {
                return 1;
            };
            let mut terminator = SystemProcessTerminator::default();
            if terminator.terminate(&identity).is_err()
                || !terminator.wait_gone(&identity, Duration::from_secs(10))
            {
                return 1;
            }
        }
        if record
            .as_ref()
            .is_some_and(|record| !afk_close_recorded(&state, record))
        {
            return 1;
        }
        return i32::from(
            fs::remove_file(state.join(".afk")).is_err() && state.join(".afk").exists(),
        );
    }
    if state.join(".afk-return-catchup").exists() {
        eprintln!(
            "mx-afk-launch: return catch-up is still pending; run bin/mx-afk-return.sh check before re-entering away mode"
        );
        return 1;
    }
    if live_daemon_pid(&state, source_root).is_some() {
        return i32::from(afk_record_read(&state).is_err() || !afk_launch_flag(&state));
    }
    let protected = [
        ".afk",
        ".subsuper-escalations",
        ".subsuper-escalations.since",
        ".subsuper-inject-wedged",
    ];
    let backup = protected
        .iter()
        .map(|name| ((*name).to_owned(), fs::read(state.join(name)).ok()))
        .collect::<Vec<_>>();
    let restore = || {
        for (name, bytes) in &backup {
            if let Some(bytes) = bytes {
                let _ = multplx_core::filesystem::atomic_replace(state.join(name), bytes, 0o600);
            } else {
                let _ = fs::remove_file(state.join(name));
            }
        }
    };
    let success = if mode == "start-native" {
        afk_launch_start_native(&state, source_root)
    } else if !afk_reconcile(&state, source_root) {
        false
    } else {
        clear_stale_afk_artifacts(&state);
        if !afk_launch_flag(&state) {
            false
        } else {
            let backend = std::env::var("MX_SUPERVISOR_BACKEND")
                .ok()
                .or_else(|| std::env::var_os("TMUX_PANE").map(|_| "tmux".to_owned()))
                .or_else(|| {
                    (std::env::var("HERDR_ENV").as_deref() == Ok("1")).then(|| "herdr".to_owned())
                });
            let target = std::env::var("MX_SUPERVISOR_TARGET")
                .ok()
                .or_else(|| std::env::var("TMUX_PANE").ok())
                .or_else(|| {
                    std::env::var("HERDR_PANE_ID").ok().map(|pane| {
                        format!(
                            "{}:{pane}",
                            std::env::var("HERDR_SESSION").unwrap_or_else(|_| "default".to_owned())
                        )
                    })
                });
            matches!(backend.as_deref(), Some("tmux"))
                && target
                    .as_deref()
                    .is_some_and(|target| afk_launch_tmux(&state, home, target, source_root))
                || matches!(backend.as_deref(), Some("herdr"))
                    && target
                        .as_deref()
                        .is_some_and(|target| afk_launch_herdr(&state, home, target, source_root))
        }
    };
    if !success {
        restore();
        return 1;
    }
    0
}

fn afk_return_blockers(state: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(state) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for path in entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("meta"))
    {
        let Some(task) = path.file_stem().and_then(|v| v.to_str()) else {
            continue;
        };
        let Ok(status) = fs::read_to_string(state.join(format!("{task}.status"))) else {
            continue;
        };
        for item in
            multplx_core::classification::open_decisions(&status, "resolved", "maintainer-held")
        {
            if item.verb == "blocked" {
                rows.push(format!(
                    "blocker\t{task}\t{}\t{}",
                    item.key.replace(['\t', '\r', '\n'], " "),
                    item.note.replace(['\t', '\r', '\n'], " ")
                ));
            }
        }
    }
    rows.sort();
    rows
}

fn afk_return_print(text: &str) {
    for line in text.lines() {
        let f = line.splitn(4, '\t').collect::<Vec<_>>();
        match f.as_slice() {
            ["evidence", kind, value] => println!("catch-up {kind}: {value}"),
            ["blocker", task, key, summary] => {
                eprintln!("broker-actionable blocker: {task} [key={key}] {summary}")
            }
            _ => {}
        }
    }
}

pub(crate) fn afk_return(args: &[std::ffi::OsString], home: &Path, source_root: &Path) -> i32 {
    let mode = args.first().and_then(|v| v.to_str()).unwrap_or("begin");
    if args.len() > 1 || !matches!(mode, "begin" | "check" | "guard" | "-h" | "--help" | "help") {
        return 2;
    }
    if matches!(mode, "-h" | "--help" | "help") {
        println!("usage: mx-afk-return.sh [begin|check|guard]");
        return 0;
    }
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    let gate = state.join(".afk-return-catchup");
    if mode == "guard" {
        if state.join(".afk").exists() {
            eprintln!(
                "mx-afk-return: away mode is still active; run bin/mx-afk-return.sh before ordinary maintainer work"
            );
            return 3;
        }
        if let Ok(text) = fs::read_to_string(&gate) {
            eprintln!(
                "mx-afk-return: return catch-up is pending; remediate or durably reclassify every listed blocker, then run bin/mx-afk-return.sh check"
            );
            afk_return_print(&text);
            return 3;
        }
        return 0;
    }
    if fs::create_dir_all(&state).is_err() {
        return 1;
    }
    let _lock = match DirectoryLock::acquire_wait(
        state.join(".afk-return-catchup.lock"),
        &SystemProcessProbe::default(),
        Duration::from_secs(10),
    ) {
        Ok(lock) => lock,
        Err(_) => return 1,
    };
    let prior = fs::read_to_string(&gate).unwrap_or_default();
    let started = prior
        .lines()
        .find_map(|line| line.strip_prefix("started\t"))
        .unwrap_or("0")
        .to_owned();
    let mut evidence = prior
        .lines()
        .filter(|line| line.starts_with("evidence\t"))
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let pending = format!(
        "schema\tmx-afk-return.v1\nstarted\t{started}\nphase\tstopping-and-draining\n{}",
        evidence
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
    );
    if multplx_core::filesystem::atomic_replace(&gate, pending.as_bytes(), 0o600).is_err() {
        return 1;
    }
    let mut ok = true;
    if state.join(".afk").exists() || afk_record_path(&state).exists() {
        ok = Command::new(source_root.join("bin/mx-afk-launch.sh"))
            .arg("stop")
            .env("MX_HOME", home)
            .env("MX_STATE_OVERRIDE", &state)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            evidence.insert("evidence\tlifecycle\taway-mode shutdown failed; lifecycle state preserved for retry".to_owned());
        }
    }
    match Command::new(source_root.join("bin/mx-wake-drain.sh"))
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", &state)
        .output()
    {
        Ok(output) if output.status.success() => {
            for line in String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| !line.is_empty())
            {
                evidence.insert(format!(
                    "evidence\twake\t{}",
                    line.replace(['\t', '\r', '\n'], " ")
                ));
            }
        }
        _ => {
            ok = false;
            evidence.insert("evidence\tlifecycle\tdurable wake drain failed; retry catch-up before ordinary work".to_owned());
        }
    }
    for (name, kind) in [
        (".subsuper-inject-wedged", "wedge"),
        (".subsuper-escalations", "escalation"),
    ] {
        if let Ok(text) = fs::read_to_string(state.join(name)) {
            for line in text.lines().filter(|line| !line.is_empty()) {
                evidence.insert(format!(
                    "evidence\t{kind}\t{}",
                    line.replace(['\t', '\r', '\n'], " ")
                ));
            }
        }
    }
    let blockers = afk_return_blockers(&state);
    if !ok || !blockers.is_empty() {
        let text = format!(
            "schema\tmx-afk-return.v1\nstarted\t{started}\nphase\tblocked\n{}{}",
            evidence
                .iter()
                .map(|l| format!("{l}\n"))
                .collect::<String>(),
            blockers
                .iter()
                .map(|l| format!("{l}\n"))
                .collect::<String>()
        );
        if multplx_core::filesystem::atomic_replace(&gate, text.as_bytes(), 0o600).is_err() {
            return 1;
        }
        eprintln!("mx-afk-return: catch-up must finish before the maintainer request");
        afk_return_print(&text);
        return 3;
    }
    afk_return_print(&evidence.iter().cloned().collect::<Vec<_>>().join("\n"));
    let _ = fs::remove_file(gate);
    clear_stale_afk_artifacts(&state);
    println!("mx-afk-return: catch-up clear; ordinary maintainer work may proceed");
    0
}

fn file_age(path: &Path) -> Duration {
    path_age(path).unwrap_or(Duration::from_secs(999_999))
}

fn run_check_snapshot(
    path: &Path,
    timeout: Duration,
    shutdown: &std::sync::atomic::AtomicBool,
) -> String {
    run_check_command(
        "bash",
        &[path.to_string_lossy().as_ref()],
        timeout,
        Some(shutdown),
    )
}

fn run_check_command<P: AsRef<std::ffi::OsStr>>(
    program: P,
    args: &[&str],
    timeout: Duration,
    shutdown: Option<&std::sync::atomic::AtomicBool>,
) -> String {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let Ok(mut child) = command.spawn() else {
        return String::new();
    };
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut value = String::new();
        let _ = stdout.read_to_string(&mut value);
        value
    });
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                terminate_finished_group(child.id());
                break;
            }
            Ok(None)
                if shutdown
                    .is_some_and(|shutdown| shutdown.load(std::sync::atomic::Ordering::SeqCst)) =>
            {
                terminate_group(&mut child);
                break;
            }
            Ok(None) if started.elapsed() >= timeout => {
                terminate_group(&mut child);
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                terminate_group(&mut child);
                break;
            }
        }
    }
    reader.join().unwrap_or_default()
}

fn remove_exact_private(
    path: &Path,
    expected_device: u64,
    identity: &multplx_domain::review_delivery::FileIdentity,
    digest: &multplx_core::identifiers::Sha256Digest,
) -> Result<(), String> {
    let current = multplx_domain::review_delivery::read_private(path, 0o600, expected_device)?;
    if &current.identity != identity || &current.digest != digest {
        return Err("private artifact changed before retirement".to_owned());
    }
    fs::remove_file(path).map_err(|error| error.to_string())
}

fn retire_pr_poll(state: &Path, snapshot: &PrPollSnapshot) -> Result<(), String> {
    let state_device = fs::symlink_metadata(state)
        .map_err(|error| error.to_string())?
        .dev();
    let task = snapshot.registration.task.as_str();
    let registration_path = state.join(format!("{task}.pr-poll-registration"));
    let current =
        multplx_domain::review_delivery::read_private(&registration_path, 0o600, state_device)?;
    if current.identity != snapshot.registration_identity
        || current.digest != snapshot.registration_digest
        || multplx_domain::review_delivery::PollRegistration::parse(&current.bytes)?
            != snapshot.registration
    {
        return Err("poll registration changed before retirement".to_owned());
    }
    let retirement = PrPollRetirement::from_snapshot(snapshot);
    let receipt_path = state.join(format!("{task}.pr-poll-retirement"));
    if fs::symlink_metadata(&receipt_path).is_ok() {
        return Err("poll retirement receipt already exists".to_owned());
    }
    multplx_domain::review_delivery::publish_private(
        &receipt_path,
        retirement.render().as_bytes(),
    )?;
    recover_pr_poll_retirement(state, &receipt_path)
}

fn recover_pr_poll_retirement(state: &Path, receipt_path: &Path) -> Result<(), String> {
    let state_device = fs::symlink_metadata(state)
        .map_err(|error| error.to_string())?
        .dev();
    let receipt = multplx_domain::review_delivery::read_private(receipt_path, 0o600, state_device)?;
    let retirement = PrPollRetirement::parse(&receipt.bytes)?;
    let task = retirement.task.as_str();
    let metadata =
        fs::read(state.join(format!("{task}.meta"))).map_err(|error| error.to_string())?;
    if multplx_domain::review_delivery::metadata_pr(&metadata)? != retirement.identity {
        return Err("poll retirement metadata identity mismatch".to_owned());
    }
    let check = state.join(format!("{task}.check.sh"));
    let registration = state.join(format!("{task}.pr-poll-registration"));
    let data = state.join(format!("{task}.pr-poll"));
    if fs::symlink_metadata(&check).is_ok() {
        remove_exact_private(
            &check,
            state_device,
            &retirement.check_identity,
            &retirement.template_hash,
        )?;
    }
    if fs::symlink_metadata(&registration).is_ok() {
        remove_exact_private(
            &registration,
            state_device,
            &retirement.registration_identity,
            &retirement.registration_hash,
        )?;
    }
    if fs::symlink_metadata(&data).is_ok() {
        remove_exact_private(
            &data,
            state_device,
            &retirement.data_identity,
            &retirement.data_hash,
        )?;
    }
    remove_exact_private(
        receipt_path,
        state_device,
        &receipt.identity,
        &receipt.digest,
    )
}

fn recover_pr_poll_retirements(state: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = fs::read_dir(state) else {
        return Vec::new();
    };
    let mut rejected = Vec::new();
    for path in entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".pr-poll-retirement"))
        })
    {
        if recover_pr_poll_retirement(state, &path).is_err() {
            rejected.push(path);
        }
    }
    rejected.sort();
    rejected
}

fn append_wake(state: &Path, kind: multplx_core::wake::WakeKind, key: &str, reason: &str) -> bool {
    multplx_core::wake::WakeQueue::new(state.to_path_buf())
        .append(
            kind,
            key,
            reason,
            SystemTime::now(),
            &SystemProcessProbe::default(),
        )
        .is_ok()
}

fn watcher_lock_pid(state: &Path) -> Option<u32> {
    fs::read_to_string(state.join(".watch.lock/pid"))
        .ok()
        .and_then(|value| value.trim().parse().ok())
}

fn watcher_lock_is_self(state: &Path) -> bool {
    watcher_lock_pid(state) == Some(std::process::id())
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct CycleRecord<'a> {
    arm_pid: u32,
    watcher_pid: &'a str,
    origin: &'a str,
    started_at: u64,
    exit_code: i32,
    signal: &'a str,
    reason: &'a str,
    lock_before: &'a str,
    lock_after: &'a str,
    successor: &'a str,
}

#[allow(dead_code)]
fn clean_ledger_field(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            _ => character,
        })
        .take(512)
        .collect()
}

#[allow(dead_code)]
fn append_cycle_record(state: &Path, record: &CycleRecord<'_>) -> Result<(), String> {
    let path = state.join(".watch-cycle-exits.log");
    let lock_path = state.join(".watch-cycle-exits.lock");
    let _lock = DirectoryLock::acquire_wait(
        &lock_path,
        &SystemProcessProbe::default(),
        Duration::from_millis(400),
    )
    .map_err(|error| error.to_string())?;
    let ended_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let beacon_age = file_age(&state.join(".last-watcher-beat")).as_secs();
    let line = format!(
        "arm_pid={}\twatcher_pid={}\torigin={}\tstarted_at={}\tended_at={ended_at}\texit_code={}\tsignal={}\treason={}\tbeacon_age={beacon_age}\tlock_before={}\tlock_after={}\tsuccessor={}\n",
        record.arm_pid,
        clean_ledger_field(record.watcher_pid),
        clean_ledger_field(record.origin),
        record.started_at,
        record.exit_code,
        clean_ledger_field(record.signal),
        clean_ledger_field(record.reason),
        clean_ledger_field(record.lock_before),
        clean_ledger_field(record.lock_after),
        clean_ledger_field(record.successor),
    );
    multplx_core::filesystem::append_single_write(&path, line.as_bytes(), 0o600)
        .map_err(|error| error.to_string())?;
    let max = environment_u64("MX_WATCH_CYCLE_LOG_MAX_BYTES", 262_144) as usize;
    let keep = environment_u64("MX_WATCH_CYCLE_LOG_KEEP_LINES", 1_000) as usize;
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    if bytes.len() >= max {
        let text = String::from_utf8_lossy(&bytes);
        let rows = text.lines().rev().take(keep).collect::<Vec<_>>();
        let mut bounded = rows.into_iter().rev().collect::<Vec<_>>().join("\n");
        bounded.push('\n');
        while bounded.len() > max {
            let Some(index) = bounded.find('\n') else {
                break;
            };
            bounded.drain(..=index);
        }
        multplx_core::filesystem::atomic_replace(&path, bounded.as_bytes(), 0o600)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn watcher_lock_snapshot(state: &Path) -> String {
    let lock = state.join(".watch.lock");
    let pid = fs::read_to_string(lock.join("pid"))
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "none".to_owned());
    let identity = fs::read_to_string(lock.join("pid-identity"))
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "none".to_owned());
    format!("pid:{pid}|identity:{identity}")
}

fn mark_predecessor_successor(state: &Path, successor: &str) {
    let Ok(predecessor) = std::env::var("MX_WATCH_PREDECESSOR_ARM_PID") else {
        return;
    };
    if predecessor.parse::<u32>().is_err() {
        return;
    }
    let path = state.join(".watch-cycle-exits.log");
    let lock_path = state.join(".watch-cycle-exits.lock");
    let Ok(_lock) = DirectoryLock::acquire_wait(
        &lock_path,
        &SystemProcessProbe::default(),
        Duration::from_millis(400),
    ) else {
        return;
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let target = format!("arm_pid={predecessor}");
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    let index = lines.iter().rposition(|line| {
        line.split('\t').next() == Some(target.as_str()) && line.ends_with("\tsuccessor=none")
    });
    if let Some(index) = index {
        let replacement_start = lines[index].len() - "successor=none".len();
        lines[index].truncate(replacement_start);
        lines[index].push_str("successor=");
        lines[index].push_str(&clean_ledger_field(successor));
        let mut output = lines.join("\n");
        output.push('\n');
        let _ = multplx_core::filesystem::atomic_replace(&path, output.as_bytes(), 0o600);
    }
}

fn watcher_healthy_pid(state: &Path, root: &Path, home: &Path, grace: Duration) -> Option<u32> {
    multplx_core::wake::watcher_healthy(
        state,
        &root.join("bin/mx-watch.sh"),
        home,
        grace,
        SystemTime::now(),
        &SystemProcessProbe::default(),
    )
    .ok()
    .flatten()
    .map(|health| health.pid)
}

fn recorded_watcher_identity(
    state: &Path,
    root: &Path,
    home: &Path,
) -> Option<multplx_core::process::ProcessIdentity> {
    let lock = state.join(".watch.lock");
    if fs::read_to_string(lock.join("mx-home")).ok()?.trim() != home.to_string_lossy()
        || fs::read_to_string(lock.join("watcher-path")).ok()?.trim()
            != root.join("bin/mx-watch.sh").to_string_lossy()
    {
        return None;
    }
    let pid = fs::read_to_string(lock.join("pid"))
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    let marker = fs::read_to_string(lock.join("pid-identity"))
        .ok()?
        .trim()
        .to_owned();
    let current = SystemProcessProbe::default().identity(pid).ok()?;
    (current.marker == marker).then_some(current)
}

fn clear_mismatched_watcher_lock(state: &Path, root: &Path, home: &Path) {
    let lock = state.join(".watch.lock");
    let metadata = match fs::symlink_metadata(&lock) {
        Ok(metadata) => metadata,
        Err(_) => return,
    };
    let owner = if metadata.file_type().is_symlink() {
        let Ok(target) = fs::read_link(&lock) else {
            return;
        };
        if target.is_absolute() {
            target
        } else {
            state.join(target)
        }
    } else if metadata.is_dir() {
        lock.clone()
    } else {
        return;
    };
    let expected_path = root.join("bin/mx-watch.sh");
    if fs::read_to_string(owner.join("mx-home"))
        .ok()
        .as_deref()
        .map(str::trim)
        != Some(home.to_string_lossy().as_ref())
        || fs::read_to_string(owner.join("watcher-path"))
            .ok()
            .as_deref()
            .map(str::trim)
            != Some(expected_path.to_string_lossy().as_ref())
        || fs::read_to_string(owner.join("pid-identity"))
            .ok()
            .is_none_or(|value| value.trim().is_empty())
    {
        return;
    }
    if metadata.file_type().is_symlink() {
        let Ok(current) = fs::read_link(&lock) else {
            return;
        };
        let current = if current.is_absolute() {
            current
        } else {
            state.join(current)
        };
        if current != owner || fs::remove_file(&lock).is_err() {
            return;
        }
    }
    for name in ["pid", "mx-home", "pid-identity", "watcher-path"] {
        let _ = fs::remove_file(owner.join(name));
    }
    let _ = fs::remove_dir(owner);
}

fn arm_signal_flag() -> Result<std::sync::Arc<std::sync::atomic::AtomicUsize>, String> {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    let signal = Arc::new(AtomicUsize::new(0));
    for (number, code) in [
        (signal_hook::consts::SIGHUP, 129),
        (signal_hook::consts::SIGINT, 130),
        (signal_hook::consts::SIGTERM, 143),
    ] {
        signal_hook::flag::register_usize(number, Arc::clone(&signal), code)
            .map_err(|error| format!("watcher: cannot install arm signal handler: {error}"))?;
    }
    Ok(signal)
}

fn signal_name(code: i32) -> &'static str {
    match code {
        129 => "HUP",
        130 => "INT",
        143 => "TERM",
        _ => "none",
    }
}

fn actionable_output(output: &str) -> Option<&'static str> {
    output.lines().find_map(|line| {
        if line.starts_with("signal:") {
            Some("actionable-signal")
        } else if line.starts_with("stale:") {
            Some("actionable-stale")
        } else if line.starts_with("check:") {
            Some("actionable-check")
        } else if line == "heartbeat" || line.starts_with("heartbeat:") {
            Some("actionable-heartbeat")
        } else {
            None
        }
    })
}

fn exit_status_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
}

struct ArmCycle {
    watcher_pid: String,
    origin: &'static str,
    started_at: u64,
    lock_before: String,
}

impl ArmCycle {
    fn begin(state: &Path, watcher_pid: impl ToString, origin: &'static str) -> Self {
        Self {
            watcher_pid: watcher_pid.to_string(),
            origin,
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            lock_before: watcher_lock_snapshot(state),
        }
    }

    fn close(self, state: &Path, exit_code: i32, reason: &str, successor: &str) {
        let lock_after = watcher_lock_snapshot(state);
        let _ = append_cycle_record(
            state,
            &CycleRecord {
                arm_pid: std::process::id(),
                watcher_pid: &self.watcher_pid,
                origin: self.origin,
                started_at: self.started_at,
                exit_code,
                signal: signal_name(exit_code),
                reason,
                lock_before: &self.lock_before,
                lock_after: &lock_after,
                successor,
            },
        );
    }
}

fn install_watcher_signals() -> Result<
    (
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ),
    String,
> {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let shutdown = Arc::new(AtomicBool::new(false));
    let nudge = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&shutdown))
            .map_err(|error| format!("watcher: cannot install signal handler: {error}"))?;
    }
    signal_hook::flag::register(signal_hook::consts::SIGUSR1, Arc::clone(&nudge))
        .map_err(|error| format!("watcher: cannot install nudge handler: {error}"))?;
    Ok((shutdown, nudge))
}

/// Native watcher transaction under construction.
#[allow(dead_code)]
pub(crate) fn watch(_root: &Path, home: &Path, source_root: &Path) -> i32 {
    use std::sync::atomic::Ordering;
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if let Err(error) = fs::create_dir_all(&state) {
        eprintln!("watcher: cannot create state: {error}");
        return 1;
    }
    let migration = Command::new(source_root.join("bin/mx-pr-check-migrate.sh"))
        .arg("--checks-safe")
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", &state)
        .status();
    if !migration.is_ok_and(|status| status.success()) {
        eprintln!("watcher: PR check migration blocked; refusing to execute state checks");
        return 1;
    }
    let lock_path = state.join(".watch.lock");
    let lock = match DirectoryLock::try_acquire(&lock_path, &SystemProcessProbe::default()) {
        Ok(lock) => lock,
        Err(_) => {
            if let Some(pid) = watcher_lock_pid(&state) {
                let stale_grace = environment_u64(
                    "MX_WATCHER_STALE_GRACE",
                    environment_u64("MX_GUARD_GRACE", 300),
                );
                let beat = state.join(".last-watcher-beat");
                let stale = if beat.exists() {
                    file_age(&beat) >= Duration::from_secs(stale_grace)
                } else {
                    file_age(&lock_path) >= Duration::from_secs(stale_grace)
                };
                if SystemProcessProbe::default().is_alive(pid) && stale {
                    eprintln!(
                        "watcher: lock held by live pid {pid} but heartbeat is stale for {}s (>{stale_grace}s); inspect or stop that watcher before re-arming.",
                        file_age(&beat).as_secs()
                    );
                    return 1;
                }
                println!("watcher: already running pid {pid}");
            } else {
                println!("watcher: already running");
            }
            return 0;
        }
    };
    let watcher_path = source_root.join("bin/mx-watch.sh");
    let identity = match SystemProcessProbe::default().identity(std::process::id()) {
        Ok(identity) => identity,
        Err(error) => {
            eprintln!("watcher: cannot establish process identity: {error}");
            return 1;
        }
    };
    for (name, value) in [
        ("mx-home", format!("{}\n", home.display())),
        ("watcher-path", format!("{}\n", watcher_path.display())),
        ("pid-identity", format!("{}\n", identity.marker)),
    ] {
        if let Err(error) = lock.publish_metadata(name, value.as_bytes()) {
            eprintln!("watcher: cannot publish {name}: {error}");
            return 1;
        }
    }
    let (shutdown, nudge) = match install_watcher_signals() {
        Ok(flags) => flags,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let poll = environment_duration("MX_POLL", 15.0);
    let signal_grace = Duration::from_secs(environment_u64("MX_SIGNAL_GRACE", 30));
    let check_interval = Duration::from_secs(environment_u64("MX_CHECK_INTERVAL", 300));
    let check_timeout = Duration::from_secs(environment_u64("MX_CHECK_TIMEOUT", 30));
    let event_failure_max = environment_u64("MX_EVENT_CAP_FAIL_MAX", 3);
    let mut event_failures = 0;
    let mut event_path_disabled = false;
    if !state.join(".last-heartbeat").exists() {
        let _ = fs::write(state.join(".last-heartbeat"), b"");
    }
    let rejected_retirements = recover_pr_poll_retirements(&state);
    if !rejected_retirements.is_empty() {
        let paths = rejected_retirements
            .iter()
            .map(|path| format!(" {}", path.display()))
            .collect::<String>();
        let reason = format!("check: rejected unauthenticated PR poll retirement receipts:{paths}");
        if append_wake(
            &state,
            multplx_core::wake::WakeKind::Check,
            "pr-poll-retirement",
            &reason,
        ) {
            let _ = fs::write(state.join(".last-check"), b"");
            println!("{reason}");
            return 0;
        }
        return 1;
    }
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return 1;
        }
        if !watcher_lock_is_self(&state) {
            return 0;
        }
        let _ = fs::write(state.join(".last-watcher-beat"), b"");
        let headroom_paths = multplx_backend::headroom::HeadroomPaths::from_environment();
        match multplx_backend::headroom::queue_drain(&headroom_paths) {
            Ok(output) if !output.is_empty() => triage_log(&state, output.trim_end()),
            Ok(_) => {}
            Err(error) => {
                let reason = format!("check: dispatch queue: {error}");
                if !append_wake(
                    &state,
                    multplx_core::wake::WakeKind::Check,
                    "dispatch-queue",
                    &reason,
                ) {
                    return 1;
                }
                println!("{reason}");
                return 0;
            }
        }
        multplx_domain::lifecycle::pending_reply::tick(&state, source_root, |task| {
            pending_reply_observation(&state, task)
        });
        if file_age(&state.join(".last-check")) >= check_interval {
            let checks = authenticated_checks(&state, source_root);
            let rejected = checks
                .iter()
                .filter_map(|check| match check {
                    AuthenticatedCheck::Rejected(path)
                        if crate::review::quarantine_rejected_check(&state, path).is_err() =>
                    {
                        Some(path.display().to_string())
                    }
                    AuthenticatedCheck::Rejected(_)
                    | AuthenticatedCheck::Custom { .. }
                    | AuthenticatedCheck::PrPoll { .. } => None,
                })
                .collect::<Vec<_>>();
            if !rejected.is_empty() {
                let reason = format!(
                    "check: rejected unauthenticated state checks: {}",
                    rejected.join(" ")
                );
                if append_wake(
                    &state,
                    multplx_core::wake::WakeKind::Check,
                    "unauthenticated-state-checks",
                    &reason,
                ) {
                    let _ = fs::write(state.join(".last-check"), b"");
                    println!("{reason}");
                    return 0;
                }
                return 1;
            }
            for check in checks {
                if let AuthenticatedCheck::Custom { task, snapshot } = check {
                    let output = run_check_snapshot(snapshot.path(), check_timeout, &shutdown);
                    if shutdown.load(Ordering::SeqCst) {
                        return 1;
                    }
                    if !output.is_empty() {
                        let check_path = state.join(format!("{}.check.sh", task.as_str()));
                        let reason =
                            format!("check: {}: {}", check_path.display(), output.trim_end());
                        if append_wake(
                            &state,
                            multplx_core::wake::WakeKind::Check,
                            &check_path.to_string_lossy(),
                            &reason,
                        ) {
                            let _ = fs::write(state.join(".last-check"), b"");
                            println!("{reason}");
                            return 0;
                        }
                        return 1;
                    }
                } else if let AuthenticatedCheck::PrPoll { task, snapshot } = check {
                    let registration = &snapshot.registration;
                    let output = run_check_command(
                        source_root.join("bin/mx-pr-poll.sh"),
                        &[
                            "--validated",
                            registration.identity.provider,
                            &registration.identity.url,
                            registration.identity.host,
                            &registration.identity.project_path(),
                            &registration.identity.number,
                        ],
                        check_timeout,
                        Some(&shutdown),
                    );
                    if shutdown.load(Ordering::SeqCst) {
                        return 1;
                    }
                    if !output.is_empty() {
                        let check_path = state.join(format!("{task}.check.sh"));
                        let reason =
                            format!("check: {}: {}", check_path.display(), output.trim_end());
                        if !append_wake(
                            &state,
                            multplx_core::wake::WakeKind::Check,
                            &check_path.to_string_lossy(),
                            &reason,
                        ) {
                            return 1;
                        }
                        if output.trim() == "merged" {
                            let _ = retire_pr_poll(&state, &snapshot);
                        }
                        let _ = fs::write(state.join(".last-check"), b"");
                        println!("{reason}");
                        return 0;
                    }
                }
            }
            let _ = fs::write(state.join(".last-check"), b"");
        }
        let signals = coalesce_signals(&state, signal_grace);
        if !signals.is_empty() {
            let files = signals
                .iter()
                .map(|observation| format!(" {}", observation.path.display()))
                .collect::<String>();
            let reason = format!("signal:{files}");
            let actionable = state.join(".afk").exists()
                || signals
                    .iter()
                    .any(|observation| observation.maintainer_relevant)
                || !signal_actors_provably_working(source_root, &state, &signals);
            if actionable {
                for observation in &signals {
                    let key = observation
                        .path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("signal");
                    if !append_wake(&state, multplx_core::wake::WakeKind::Signal, key, &reason) {
                        return 1;
                    }
                }
                if publish_signal_markers(&signals).is_err() {
                    return 1;
                }
                for observation in &signals {
                    mark_status_surfaced(&state, &observation.path);
                }
                let _ = fs::write(state.join(".heartbeat-streak"), b"0\n");
                println!("{reason}");
                return 0;
            }
            if publish_signal_markers(&signals).is_err() {
                return 1;
            }
            triage_log(&state, &format!("absorbed benign {reason}"));
        }
        if let Some(reason) = stale_scan(source_root, &state) {
            if reason.is_empty() {
                return 1;
            }
            let _ = fs::write(state.join(".heartbeat-streak"), b"0\n");
            println!("{reason}");
            return 0;
        }
        let heartbeat = Duration::from_secs(environment_u64("MX_HEARTBEAT", 600));
        let heartbeat_max = Duration::from_secs(environment_u64("MX_HEARTBEAT_MAX", 7_200));
        let streak = fs::read_to_string(state.join(".heartbeat-streak"))
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap_or(0)
            .min(12);
        let interval = heartbeat
            .checked_mul(1_u32.checked_shl(streak).unwrap_or(u32::MAX))
            .unwrap_or(heartbeat_max)
            .min(heartbeat_max);
        if file_age(&state.join(".last-heartbeat")) >= interval {
            let statuses = unsurfaced_maintainer_statuses(&state);
            if state.join(".afk").exists() || !statuses.is_empty() {
                if !append_wake(
                    &state,
                    multplx_core::wake::WakeKind::Heartbeat,
                    "heartbeat",
                    "heartbeat",
                ) {
                    return 1;
                }
                let _ = fs::write(state.join(".last-heartbeat"), b"");
                for status in statuses {
                    mark_status_surfaced(&state, &status);
                }
                let _ = fs::write(state.join(".heartbeat-streak"), format!("{}\n", streak + 1));
                println!("heartbeat");
                return 0;
            }
            let _ = fs::write(state.join(".last-heartbeat"), b"");
            let _ = fs::write(state.join(".heartbeat-streak"), format!("{}\n", streak + 1));
        }
        nudge.store(false, Ordering::SeqCst);
        if !event_path_disabled
            && recorded_windows(&state).iter().any(|window| {
                window.backend == multplx_backend::facade::BackendName::Herdr
                    && window.kind != "daemon"
            })
        {
            match backend_event_wait(&state, poll) {
                Ok(Some(reason)) => {
                    println!("{reason}");
                    return 0;
                }
                Ok(None) => {
                    event_failures = 0;
                    continue;
                }
                Err(error) => {
                    triage_log(&state, &format!("native event wait unavailable: {error}"));
                    let state = event_failure_state(event_failures, event_failure_max);
                    event_failures = state.0;
                    event_path_disabled = state.1;
                }
            }
        }
        let deadline = std::time::Instant::now() + poll;
        while std::time::Instant::now() < deadline
            && !shutdown.load(Ordering::SeqCst)
            && !nudge.load(Ordering::SeqCst)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Run the away-mode watcher under one durable, single-instance supervisor.
pub(crate) fn supervise_daemon(
    args: &[std::ffi::OsString],
    home: &Path,
    source_root: &Path,
) -> i32 {
    use multplx_backend::facade::{BackendName, BackendTarget, RuntimeBackend, SubmitRequest};
    use multplx_core::composer::ComposerState;
    use std::sync::atomic::Ordering;
    if !args.is_empty() {
        eprintln!("usage: mx-supervise-daemon.sh");
        return 2;
    }
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if let Err(error) = fs::create_dir_all(&state) {
        eprintln!("error: cannot create supervisor state: {error}");
        return 1;
    }
    let lock = match DirectoryLock::try_acquire(
        state.join(".supervise-daemon.lock"),
        &SystemProcessProbe::default(),
    ) {
        Ok(lock) => lock,
        Err(_) => {
            eprintln!("error: another mx-supervise-daemon is already running");
            return 1;
        }
    };
    let pid = std::process::id();
    let identity = SystemProcessProbe::default().identity(pid).ok();
    let _ = lock.publish_metadata("pid", format!("{pid}\n").as_bytes());
    if let Some(identity) = identity {
        let _ = lock.publish_metadata("pid-identity", identity.marker.as_bytes());
    }
    let pidfile = state.join(".supervise-daemon.pid");
    if fs::write(&pidfile, format!("{pid}\n")).is_err() {
        return 1;
    }
    let (shutdown, _) = match install_watcher_signals() {
        Ok(flags) => flags,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("error: cannot locate native supervisor executable: {error}");
            return 1;
        }
    };
    let watcher_override = std::env::var_os("MX_SUPERVISE_WATCH_EXEC").map(PathBuf::from);
    let log = state.join(".supervise-daemon.log");
    let flush = |state: &Path| -> bool {
        if !state.join(".afk").is_file() {
            return false;
        }
        let path = state.join(".subsuper-escalations");
        let Ok(text) = fs::read_to_string(&path) else {
            return true;
        };
        let rows = text
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return true;
        }
        let body = format!(
            "Supervisor escalate ({} event(s)): {} (pre-read; re-arm not needed - watcher daemon-managed)",
            rows.len(),
            rows.join(" | ")
        );
        let Some(message) = multplx_domain::operational_input::construct(
            multplx_domain::operational_input::Kind::AwaySupervisor,
            &body,
        ) else {
            return false;
        };
        let backend = std::env::var("MX_SUPERVISOR_BACKEND").unwrap_or_else(|_| "tmux".to_owned());
        let Some(target_text) = std::env::var("MX_SUPERVISOR_TARGET")
            .ok()
            .or_else(|| std::env::var("TMUX_PANE").ok())
        else {
            return false;
        };
        let (backend_name, mut submit): (BackendName, Box<dyn RuntimeBackend>) =
            match backend.as_str() {
                "tmux" => (
                    BackendName::Tmux,
                    Box::new(multplx_backend::tmux::TmuxBackend::system()),
                ),
                "herdr" => (
                    BackendName::Herdr,
                    Box::new(multplx_backend::herdr::HerdrBackend::system()),
                ),
                _ => return false,
            };
        let Ok(target) = BackendTarget::new(backend_name, target_text, None) else {
            return false;
        };
        let retries = environment_u64("MX_INJECT_CONFIRM_RETRIES", 3) as usize;
        let delay = environment_duration("MX_INJECT_CONFIRM_SLEEP", 0.5);
        let result = submit.send_submit(
            &target,
            SubmitRequest {
                text: &message,
                retries,
                enter_delay: delay,
                settle: delay,
            },
        );
        if matches!(result, Ok(ComposerState::Empty)) {
            let _ = fs::write(&path, b"");
            let _ = fs::remove_file(state.join(".subsuper-escalations.since"));
            return true;
        }
        false
    };
    while !shutdown.load(Ordering::SeqCst) {
        let mut watcher = Command::new(watcher_override.as_deref().unwrap_or(&executable));
        if watcher_override.is_none() {
            watcher.arg("watch");
        }
        let output = watcher
            .env("MX_HOME", home)
            .env("MX_STATE_OVERRIDE", &state)
            .env("MX_RUST_SOURCE_ROOT", source_root)
            .output();
        match output {
            Ok(output) => {
                let reason = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                let line = format!(
                    "[{}] watcher rc={:?} {}\n",
                    now_epoch(),
                    output.status.code(),
                    reason
                );
                let _ = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log)
                    .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
                if reason.starts_with("signal:")
                    || reason.starts_with("check:")
                    || reason.starts_with("stale:")
                {
                    let escalation = state.join(".subsuper-escalations");
                    let _ = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(escalation)
                        .and_then(|mut file| {
                            std::io::Write::write_all(
                                &mut file,
                                format!("{}\t{}\n", now_epoch(), reason).as_bytes(),
                            )
                        });
                }
                let _ = flush(&state);
            }
            Err(error) => {
                eprintln!("supervisor: watcher launch failed: {error}");
            }
        }
        let _ = flush(&state);
        for _ in 0..10 {
            if shutdown.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let _ = fs::remove_file(pidfile);
    0
}

fn wait_for_successor(
    state: &Path,
    root: &Path,
    home: &Path,
    grace: Duration,
    timeout: Duration,
) -> Option<u32> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(pid) = watcher_healthy_pid(state, root, home, grace) {
            return Some(pid);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct ArmAttachContext<'a> {
    state: &'a Path,
    root: &'a Path,
    home: &'a Path,
    grace: Duration,
    confirm: Duration,
    poll: Duration,
    signal: &'a std::sync::atomic::AtomicUsize,
}

fn attach_to_watcher(mut pid: u32, context: &ArmAttachContext<'_>) -> i32 {
    use std::sync::atomic::Ordering;
    let mut cycle = ArmCycle::begin(context.state, pid, "attached");
    loop {
        let interrupted = context.signal.load(Ordering::SeqCst) as i32;
        if interrupted != 0 {
            cycle.close(context.state, interrupted, "arm-interrupted", "none");
            return interrupted;
        }
        match watcher_healthy_pid(context.state, context.root, context.home, context.grace) {
            Some(current) if current == pid => std::thread::sleep(context.poll),
            Some(current) => {
                let successor = format!("attached:{current}");
                cycle.close(context.state, 0, "lock-replaced", &successor);
                pid = current;
                println!(
                    "watcher: attached pid={pid} (beacon {}s)",
                    file_age(&context.state.join(".last-watcher-beat")).as_secs()
                );
                cycle = ArmCycle::begin(context.state, pid, "attached");
            }
            None => {
                if let Some(current) = wait_for_successor(
                    context.state,
                    context.root,
                    context.home,
                    context.grace,
                    context.confirm,
                ) {
                    let successor = format!("attached:{current}");
                    cycle.close(context.state, 0, "attached-cycle-ended", &successor);
                    pid = current;
                    println!(
                        "watcher: attached pid={pid} (beacon {}s)",
                        file_age(&context.state.join(".last-watcher-beat")).as_secs()
                    );
                    cycle = ArmCycle::begin(context.state, pid, "attached");
                } else {
                    cycle.close(context.state, 1, "attached-cycle-ended", "none");
                    println!("watcher: FAILED - cycle ended without an actionable reason");
                    return 1;
                }
            }
        }
    }
}

fn finish_owned_watcher(
    status: std::process::ExitStatus,
    output: &str,
    cycle: ArmCycle,
    context: &ArmAttachContext<'_>,
) -> i32 {
    let code = exit_status_code(status);
    if code == 0 {
        if let Some(reason) = actionable_output(output) {
            cycle.close(context.state, code, reason, "none");
            print!("{output}");
            return 0;
        }
        if let Some(successor_pid) = wait_for_successor(
            context.state,
            context.root,
            context.home,
            context.grace,
            context.confirm,
        ) {
            let successor = format!("attached:{successor_pid}");
            cycle.close(context.state, code, "unexpected-clean-exit", &successor);
            print!("{output}");
            mark_predecessor_successor(context.state, &successor);
            println!(
                "watcher: attached pid={successor_pid} (beacon {}s)",
                file_age(&context.state.join(".last-watcher-beat")).as_secs()
            );
            return attach_to_watcher(successor_pid, context);
        }
        cycle.close(context.state, code, "unexpected-clean-exit", "none");
        print!("{output}");
        println!("watcher: FAILED - cycle ended without an actionable reason");
        return 1;
    }
    let reason = if code > 128 {
        "signal-exit"
    } else {
        "nonzero-exit"
    };
    cycle.close(context.state, code, reason, "none");
    print!("{output}");
    if !output
        .lines()
        .any(|line| line.starts_with("watcher: FAILED"))
    {
        println!("watcher: FAILED - watcher cycle exited {code} without an actionable reason");
    }
    code.max(1)
}

/// Run the native watcher arm lifecycle without detaching its owned watcher.
#[allow(dead_code)]
pub(crate) fn watch_arm(
    args: &[std::ffi::OsString],
    root: &Path,
    home: &Path,
    source_root: &Path,
) -> i32 {
    use std::sync::atomic::Ordering;
    let restart = match args {
        [] => false,
        [argument] if argument == "arm" || argument == "--arm" => false,
        [argument] if argument == "--restart" => true,
        _ => {
            eprintln!("usage: mx-watch-arm.sh [--restart]");
            return 2;
        }
    };
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if let Err(error) = fs::create_dir_all(&state) {
        eprintln!("watcher: FAILED - cannot create state: {error}");
        return 1;
    }
    let grace = Duration::from_secs(environment_u64("MX_GUARD_GRACE", 300));
    let confirm = Duration::from_secs(environment_u64("MX_ARM_CONFIRM_TIMEOUT", 10));
    let attach_poll = std::env::var("MX_ARM_ATTACH_POLL")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(Duration::from_secs_f64)
        .unwrap_or(Duration::from_millis(500));
    let signal = match arm_signal_flag() {
        Ok(signal) => signal,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if restart {
        if let Some(identity) = recorded_watcher_identity(&state, source_root, home) {
            let mut terminator = SystemProcessTerminator::default();
            if terminator.terminate(&identity).is_ok() {
                let _ = terminator.wait_gone(&identity, Duration::from_secs(5));
            }
        } else {
            clear_mismatched_watcher_lock(&state, source_root, home);
        }
    } else if let Some(pid) = watcher_healthy_pid(&state, source_root, home, grace) {
        let successor = format!("attached:{pid}");
        mark_predecessor_successor(&state, &successor);
        println!(
            "watcher: attached pid={pid} (beacon {}s)",
            file_age(&state.join(".last-watcher-beat")).as_secs()
        );
        return attach_to_watcher(
            pid,
            &ArmAttachContext {
                state: &state,
                root: source_root,
                home,
                grace,
                confirm,
                poll: attach_poll,
                signal: &signal,
            },
        );
    }

    let output_file = match tempfile::Builder::new()
        .prefix(".watch-arm-output.")
        .tempfile_in(&state)
    {
        Ok(output) => output,
        Err(_) => {
            println!("watcher: FAILED - no live watcher with a fresh beacon");
            return 1;
        }
    };
    let stdout = match output_file.as_file().try_clone() {
        Ok(file) => file,
        Err(_) => {
            println!("watcher: FAILED - no live watcher with a fresh beacon");
            return 1;
        }
    };
    let mut child = match Command::new(source_root.join("bin/mx-watch.sh"))
        .env("MX_ROOT_OVERRIDE", root)
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", &state)
        .stdout(stdout)
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("watcher: FAILED - cannot start watcher: {error}");
            return 1;
        }
    };
    let child_pid = child.id();
    let mut cycle = Some(ArmCycle::begin(&state, child_pid, "started"));
    let deadline = std::time::Instant::now() + confirm;
    let mut confirmed = false;
    loop {
        let interrupted = signal.load(Ordering::SeqCst) as i32;
        if interrupted != 0 {
            terminate_group(&mut child);
            cycle.take().expect("active cycle").close(
                &state,
                interrupted,
                "arm-interrupted",
                "none",
            );
            return interrupted;
        }
        if let Some(pid) = watcher_healthy_pid(&state, source_root, home, grace)
            && pid == child_pid
            && !confirmed
        {
            cycle.as_mut().expect("active cycle").lock_before = watcher_lock_snapshot(&state);
            mark_predecessor_successor(&state, &format!("started:{pid}"));
            println!("watcher: started pid={pid} (beacon fresh)");
            confirmed = true;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = fs::read_to_string(output_file.path()).unwrap_or_default();
                return finish_owned_watcher(
                    status,
                    &output,
                    cycle.take().expect("active cycle"),
                    &ArmAttachContext {
                        state: &state,
                        root: source_root,
                        home,
                        grace,
                        confirm,
                        poll: attach_poll,
                        signal: &signal,
                    },
                );
            }
            Ok(None) if confirmed || std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                terminate_group(&mut child);
                let output = fs::read_to_string(output_file.path()).unwrap_or_default();
                print!("{output}");
                cycle.take().expect("active cycle").close(
                    &state,
                    1,
                    "confirmation-timeout",
                    "none",
                );
                println!("watcher: FAILED - no live watcher with a fresh beacon");
                return 1;
            }
            Err(error) => {
                eprintln!("watcher: FAILED - cannot inspect watcher: {error}");
                return 1;
            }
        }
    }
}

fn bool_environment(name: &str) -> bool {
    std::env::var(name)
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn program_available(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(name).is_file())
    })
}

fn program_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(name))
            .find(|path| path.is_file())
    })
}

fn grace() -> Duration {
    Duration::from_secs(
        std::env::var("MX_GUARD_GRACE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(300),
    )
}

fn episode_key(state: &Path) -> String {
    let beat = state.join(".last-watcher-beat");
    fs::metadata(&beat)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or_else(
            || {
                if beat.exists() {
                    "beat:unknown".to_owned()
                } else {
                    "beat:absent".to_owned()
                }
            },
            |modified| format!("beat:{}", modified.as_secs()),
        )
}

fn marker_matches(marker: &Path, key: &str) -> bool {
    fs::read_to_string(marker).is_ok_and(|value| value.trim_end_matches('\n') == key)
}

fn claim_episode(state: &Path, key: &str) -> bool {
    let marker = state.join(".guard-watcher-stale-banner");
    if marker_matches(&marker, key) {
        return false;
    }
    let lock_path = state.join(".guard-watcher-stale-banner.lock");
    let processes = SystemProcessProbe::default();
    let Ok(_lock) = DirectoryLock::acquire_wait(&lock_path, &processes, Duration::from_secs(1))
    else {
        return true;
    };
    if marker_matches(&marker, key) {
        return false;
    }
    multplx_core::filesystem::atomic_replace(&marker, format!("{key}\n").as_bytes(), 0o600).is_ok()
}

fn render_tangle(root: &Path, read_only: bool) -> String {
    let Ok(Some(branch)) = multplx_core::tangle::primary_tangle_branch(root) else {
        return String::new();
    };
    let default = multplx_core::tangle::default_branch(root)
        .ok()
        .flatten()
        .unwrap_or_else(|| "main".to_owned());
    let rule = "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";
    let recovery = if read_only {
        "●  This read-only session must leave restore work to a session with verified system-lock ownership.\n".to_owned()
    } else {
        format!(
            "●  Restore the primary to '{default}':\n●      git -C {} checkout {default}\n●  then re-validate '{branch}' in a proper isolated worktree.\n",
            root.display()
        )
    };
    format!(
        "●{rule}\n●  WORKTREE TANGLE - PRIMARY CHECKOUT IS ON A FEATURE BRANCH\n●  {} is on '{branch}', not its default branch '{default}'.\n●  an actor likely branched/committed in the primary instead of its own worktree.\n●  The work is SAFE on the '{branch}' ref.\n{recovery}●{rule}\n",
        root.display()
    )
}

fn repair_line(
    source_root: &Path,
    logical_root: &Path,
    detected_harness: &str,
    read_only: bool,
    afk: bool,
    queue_pending: bool,
) -> String {
    let args = vec![
        "--read-only".to_owned(),
        usize::from(read_only).to_string(),
        "--afk".to_owned(),
        usize::from(afk).to_string(),
        "--queue-pending".to_owned(),
        usize::from(queue_pending).to_string(),
        "--repair-line".to_owned(),
    ];
    let result = multplx_domain::session::supervision_instructions(
        &args,
        detected_harness,
        source_root,
        logical_root,
    );
    if result.status == 0 && !result.stdout.trim().is_empty() {
        result.stdout.trim().to_owned()
    } else {
        "Repair missing watcher supervision according to the session-start operating block."
            .to_owned()
    }
}

/// Run the native watcher/tangle warning transaction.
pub(crate) fn guard(root: &Path, home: &Path, source_root: &Path, detected_harness: &str) -> i32 {
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    let read_only = bool_environment("MX_GUARD_READ_ONLY");
    let mut stderr = render_tangle(root, read_only);
    let status = multplx_core::supervision::inspect(&state, grace(), SystemTime::now());
    let marker = state.join(".guard-watcher-stale-banner");
    if status.in_flight == 0 {
        if !read_only {
            let _ = fs::remove_file(marker);
        }
        eprint!("{stderr}");
        return 0;
    }
    if !status.watcher_fresh {
        let key = episode_key(&state);
        let full = if read_only {
            !marker_matches(&marker, &key)
        } else {
            claim_episode(&state, &key)
        };
        if full {
            let afk = state.join(".afk").exists();
            let fix = repair_line(
                source_root,
                root,
                detected_harness,
                read_only,
                afk,
                status.queue_pending,
            );
            let rule = "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";
            let ownership = if read_only {
                "●  This read-only session should report the lapse, not repair it.\n"
            } else {
                "●  Trust the emitted supervision protocol for this harness; do not use shell & for watcher repair.\n"
            };
            let continuation = std::env::var("MX_GUARD_CONTINUE_LINE").unwrap_or_else(|_| {
                "This is a supervision warning only; the guarded operation WILL still run."
                    .to_owned()
            });
            stderr.push_str(&format!(
                "●{rule}\n●  WATCHER DOWN - SUPERVISION IS OFF\n●  {} task(s) in flight, but no watcher has a fresh beacon (last beat: {}, grace {}s).\n{ownership}●  {continuation}\n●  {fix}\n●{rule}\n",
                status.in_flight,
                status.beacon_description,
                grace().as_secs(),
            ));
        } else {
            stderr.push_str(&format!(
                "WARNING: watcher still down (same stale episode; last beat: {}, grace {}s) - full banner already printed this episode.\n",
                status.beacon_description,
                grace().as_secs(),
            ));
        }
    } else if !read_only {
        let _ = fs::remove_file(marker);
    }
    if status.queue_pending {
        if read_only {
            stderr.push_str("WARNING: queued wakes pending - left untouched because this session lacks verified system-lock ownership.\n");
        } else {
            stderr.push_str("WARNING: queued wakes pending - drain them with bin/mx-wake-drain.sh before anything else.\n");
        }
    }
    eprint!("{stderr}");
    0
}

fn watcher_healthy(state: &Path, root: &Path, home: &Path, grace: Duration) -> bool {
    let result = multplx_core::wake::watcher_healthy(
        state,
        &root.join("bin/mx-watch.sh"),
        home,
        grace,
        SystemTime::now(),
        &SystemProcessProbe::default(),
    );
    result.is_ok_and(|health| health.is_some())
}

fn path_age(path: &Path) -> Option<Duration> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
}

fn autoarm_owns_recovery(state: &Path, root: &Path, home: &Path, grace: Duration) -> bool {
    if watcher_healthy(state, root, home, grace) {
        return true;
    }
    let process = SystemProcessProbe::default();
    if fs::read_to_string(state.join(".claude-autoarm.lock/pid"))
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .is_some_and(|pid| process.is_alive(pid))
    {
        return true;
    }
    let epoch = state.join(".claude-autoarm-epoch");
    let rewake = fs::read_to_string(&epoch).is_ok_and(|value| {
        value
            .split_whitespace()
            .any(|field| field == "outcome=rewake")
    });
    let fresh = std::env::var("MX_CLAUDE_AUTOARM_EPOCH_FRESH")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(15);
    rewake && path_age(&epoch).is_some_and(|age| age < Duration::from_secs(fresh))
}

fn turnend_banner(
    status: &multplx_core::supervision::SupervisionStatus,
    source_root: &Path,
    logical_root: &Path,
    detected_harness: &str,
    claude: bool,
    state: &Path,
) -> String {
    let fix = repair_line(
        source_root,
        logical_root,
        detected_harness,
        false,
        state.join(".afk").exists(),
        status.queue_pending,
    );
    let rule = "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";
    let autoarm = if claude {
        "●  The Stop-owned auto-arm did not claim this home either, so recovery is NOT already under way.\n"
    } else {
        ""
    };
    format!(
        "●{rule}\n●  TURN WOULD END BLIND - SUPERVISION IS OFF\n●  {} task(s) in flight, but no live watcher holds this home lock (last beat: {}).\n{autoarm}●  {fix}\n●{rule}\n",
        status.in_flight, status.beacon_description,
    )
}

fn reset_budget(path: &Path, claude: bool) {
    if claude {
        let _ = fs::remove_file(path);
    }
}

/// Run the native primary turn-end decision.
pub(crate) fn turnend_guard(
    args: &[std::ffi::OsString],
    payload: &str,
    root: &Path,
    home: &Path,
    source_root: &Path,
    detected_harness: &str,
) -> i32 {
    let mut claude = false;
    for argument in args {
        if argument == "--claude" {
            claude = true;
        } else {
            eprintln!("usage: mx-turnend-guard.sh [--claude]");
            return 2;
        }
    }
    if payload.is_empty() || !program_available("jq") {
        return 0;
    }
    let Ok(document) = serde_json::from_str::<serde_json::Value>(payload) else {
        return 0;
    };
    if !claude
        && document
            .get("stop_hook_active")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    {
        return 0;
    }
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if !multplx_core::primary_scope::matches(root, &state) {
        return 0;
    }
    let status = multplx_core::supervision::inspect(&state, grace(), SystemTime::now());
    let budget = state.join(".turnend-claude-blocks");
    if !status.needed || watcher_healthy(&state, root, home, grace()) {
        reset_budget(&budget, claude);
        return 0;
    }
    if !claude {
        eprint!(
            "{}",
            turnend_banner(&status, source_root, root, detected_harness, false, &state)
        );
        return 2;
    }
    let sync_wait = std::env::var("MX_CLAUDE_AUTOARM_SYNC_WAIT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(800);
    let deadline = std::time::Instant::now() + Duration::from_millis(sync_wait);
    while std::time::Instant::now() < deadline {
        if autoarm_owns_recovery(&state, root, home, grace()) {
            reset_budget(&budget, true);
            return 0;
        }
        std::thread::sleep(
            Duration::from_millis(100)
                .min(deadline.saturating_duration_since(std::time::Instant::now())),
        );
    }
    if autoarm_owns_recovery(&state, root, home, grace()) {
        reset_budget(&budget, true);
        return 0;
    }
    let session = document
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let (old_session, old_count) = fs::read_to_string(&budget)
        .ok()
        .map(|value| {
            let mut lines = value.lines();
            let session = lines
                .next()
                .and_then(|line| line.strip_prefix("session="))
                .unwrap_or("");
            let count = lines
                .next()
                .and_then(|line| line.strip_prefix("count="))
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            (session.to_owned(), count)
        })
        .unwrap_or_default();
    let count = if old_session == session {
        old_count + 1
    } else {
        1
    };
    let limit = std::env::var("MX_CLAUDE_TURNEND_BLOCK_BUDGET")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(3);
    if count > limit {
        reset_budget(&budget, true);
        let message = format!(
            "broker turn-end guard: {} task(s) in flight with no live watcher and no Stop auto-arm claim; block budget exhausted, allowing this stop. Repair supervision (bin/mx-watch-arm.sh as a Claude Code background task) or investigate why bin/mx-claude-stop-autoarm.sh is not claiming this home.",
            status.in_flight
        );
        println!("{}", serde_json::json!({"systemMessage": message}));
        return 0;
    }
    let _ = multplx_core::filesystem::atomic_replace(
        &budget,
        format!("session={session}\ncount={count}\n").as_bytes(),
        0o600,
    );
    eprint!(
        "{}",
        turnend_banner(&status, source_root, root, detected_harness, true, &state)
    );
    2
}

const CHECKPOINT_USAGE: &str = "Usage: mx-watch-checkpoint.sh [--seconds <n>]\n\nRun bin/mx-watch.sh in the foreground for a bounded checkpoint.\nOn an actionable watcher wake, pass through the watcher output and exit 0.\nOn a quiet checkpoint, print \"checkpoint: no actionable wake within <n>s\" and exit 124.\n";

fn checkpoint_seconds(args: &[std::ffi::OsString]) -> Result<Option<u64>, String> {
    let mut seconds = std::env::var("MX_CODEX_WATCH_CHECKPOINT")
        .ok()
        .unwrap_or_else(|| "180".to_owned());
    let mut index = 0;
    while index < args.len() {
        let value = args[index].to_string_lossy();
        match value.as_ref() {
            "-h" | "--help" => return Ok(None),
            "--seconds" => {
                index += 1;
                seconds = args
                    .get(index)
                    .ok_or("error: --seconds requires a value")?
                    .to_string_lossy()
                    .into_owned();
            }
            value if value.starts_with("--seconds=") => seconds = value[10..].to_owned(),
            _ => return Err(format!("error: unknown argument: {value}")),
        }
        index += 1;
    }
    seconds
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .map(Some)
        .ok_or_else(|| {
            if seconds == "0" {
                "error: --seconds must be greater than zero".to_owned()
            } else {
                "error: --seconds must be a positive integer".to_owned()
            }
        })
}

fn terminate_group(child: &mut std::process::Child) {
    if let Some(pid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process_group(pid, Signal::TERM);
    }
    std::thread::sleep(Duration::from_millis(200));
    if let Some(pid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process_group(pid, Signal::KILL);
    }
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn terminate_finished_group(child_pid: u32) {
    let Some(pid) = Pid::from_raw(child_pid as i32) else {
        return;
    };
    let _ = kill_process_group(pid, Signal::TERM);
    std::thread::sleep(Duration::from_millis(200));
    let _ = kill_process_group(pid, Signal::KILL);
}

fn cleanup_checkpoint_lock(state: &Path) -> bool {
    let path = state.join(".watch.lock");
    for _ in 0..60 {
        if fs::symlink_metadata(&path).is_err() {
            return true;
        }
        if let Ok(lock) = DirectoryLock::try_acquire(&path, &SystemProcessProbe::default()) {
            drop(lock);
        }
        if fs::symlink_metadata(&path).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Run one bounded foreground watcher checkpoint.
pub(crate) fn watch_checkpoint(
    args: &[std::ffi::OsString],
    root: &Path,
    home: &Path,
    source_root: &Path,
) -> i32 {
    let seconds = match checkpoint_seconds(args) {
        Ok(Some(seconds)) => seconds,
        Ok(None) => {
            print!("{CHECKPOINT_USAGE}");
            return 0;
        }
        Err(error) => {
            eprintln!("{error}");
            eprint!("{CHECKPOINT_USAGE}");
            return 2;
        }
    };
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    let external_timeout = program_path("timeout").or_else(|| program_path("gtimeout"));
    let mut command = if let Some(timeout) = &external_timeout {
        let mut command = Command::new(timeout);
        command
            .arg(seconds.to_string())
            .arg(source_root.join("bin/mx-watch.sh"));
        command
    } else {
        Command::new(source_root.join("bin/mx-watch.sh"))
    };
    command
        .env("MX_ROOT_OVERRIDE", root)
        .env("MX_HOME", home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let Ok(mut child) = command.spawn() else {
        eprintln!("checkpoint: could not start watcher");
        return 1;
    };
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let stdout_reader = std::thread::spawn(move || {
        let mut value = String::new();
        let _ = stdout.read_to_string(&mut value);
        value
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut value = String::new();
        let _ = stderr.read_to_string(&mut value);
        value
    });
    let started = std::time::Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= Duration::from_secs(seconds) => {
                timed_out = true;
                terminate_group(&mut child);
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                eprintln!("checkpoint: cannot wait for watcher: {error}");
                terminate_group(&mut child);
                break None;
            }
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let actionable = stdout.lines().any(|line| {
        line.starts_with("signal:")
            || line.starts_with("stale:")
            || line.starts_with("check:")
            || line == "heartbeat"
            || line.starts_with("heartbeat:")
    });
    if actionable {
        print!("{stdout}");
        eprint!("{stderr}");
        return 0;
    }
    if stdout.contains("watcher: already running") || stderr.contains("watcher: already running") {
        print!("{stdout}");
        eprint!("{stderr}");
        eprintln!("checkpoint: watcher is already running outside this foreground checkpoint");
        return 1;
    }
    if timed_out || status.as_ref().and_then(std::process::ExitStatus::code) == Some(124) {
        if !cleanup_checkpoint_lock(&state) {
            let pid = fs::read_to_string(state.join(".watch.lock/pid"))
                .unwrap_or_else(|_| "unknown".to_owned());
            eprintln!(
                "checkpoint: timed-out watcher lock did not clean up (pid={})",
                pid.trim()
            );
            return 1;
        }
        println!("checkpoint: no actionable wake within {seconds}s");
        return 124;
    }
    print!("{stdout}");
    eprint!("{stderr}");
    status.and_then(|status| status.code()).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use sha2::{Digest, Sha256};

    use super::{
        ActorAbsorbClass, AuthenticatedCheck, CycleRecord, append_cycle_record,
        authenticated_checks, coalesce_signals, event_failure_state, parse_actor_absorb_class,
        publish_signal_markers, scan_signals,
    };

    #[test]
    fn actor_absorption_requires_a_verified_working_source() {
        assert_eq!(
            parse_actor_absorb_class("state: working · source: native-event · running"),
            ActorAbsorbClass::Working
        );
        assert_eq!(
            parse_actor_absorb_class("state: working · source: status · stale"),
            ActorAbsorbClass::None
        );
        assert_eq!(
            parse_actor_absorb_class("state: paused · source: status · external"),
            ActorAbsorbClass::Paused
        );
    }

    #[test]
    fn event_failures_disable_the_fast_path_at_the_bounded_threshold() {
        assert_eq!(event_failure_state(0, 3), (1, false));
        assert_eq!(event_failure_state(1, 3), (2, false));
        assert_eq!(event_failure_state(2, 3), (3, true));
        assert_eq!(event_failure_state(u64::MAX, 3), (u64::MAX, true));
        assert_eq!(event_failure_state(0, 0), (1, true));
    }

    #[test]
    fn signal_scan_is_transactional_and_last_write_wins() {
        let temp = tempfile::tempdir().expect("tempdir");
        let status = temp.path().join("task.status");
        fs::write(&status, "working: first\n").expect("status");
        let first = scan_signals(temp.path());
        assert_eq!(first.len(), 1);
        assert!(!first[0].maintainer_relevant);
        fs::write(&status, "working: first\ndone: ready\n").expect("status update");
        let coalesced = coalesce_signals(temp.path(), Duration::ZERO);
        assert_eq!(coalesced.len(), 1);
        assert!(coalesced[0].maintainer_relevant);
        publish_signal_markers(&coalesced).expect("publish markers");
        assert!(scan_signals(temp.path()).is_empty());
    }

    #[test]
    fn authenticated_check_discovery_snapshots_only_exact_trust_bindings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bytes = b"#!/bin/sh\necho actionable\n";
        let check = temp.path().join("safe.check.sh");
        fs::write(&check, bytes).expect("check");
        fs::set_permissions(&check, fs::Permissions::from_mode(0o700)).expect("check mode");
        let digest =
            multplx_core::identifiers::Sha256Digest::parse(format!("{:x}", Sha256::digest(bytes)))
                .expect("digest");
        let trust = temp.path().join("safe.check-trust");
        fs::write(&trust, multplx_core::checks::render_trust(&digest)).expect("trust");
        fs::set_permissions(&trust, fs::Permissions::from_mode(0o600)).expect("trust mode");
        let unsafe_check = temp.path().join("unsafe.check.sh");
        fs::write(&unsafe_check, bytes).expect("unsafe check");
        fs::set_permissions(&unsafe_check, fs::Permissions::from_mode(0o700)).expect("unsafe mode");
        let checks = authenticated_checks(temp.path(), temp.path());
        assert_eq!(checks.len(), 2);
        assert!(checks.iter().any(|check| match check {
            AuthenticatedCheck::Custom { task, snapshot } => {
                task.as_str() == "safe"
                    && fs::read(snapshot.path()).is_ok_and(|value| value == bytes)
            }
            AuthenticatedCheck::PrPoll { .. } | AuthenticatedCheck::Rejected(_) => false,
        }));
        assert!(checks.iter().any(|check| matches!(
            check,
            AuthenticatedCheck::Rejected(path) if path == &unsafe_check
        )));
    }

    #[test]
    fn cycle_ledger_scrubs_fields_and_stays_bounded() {
        let temp = tempfile::tempdir().expect("tempdir");
        for index in 0..1_200 {
            append_cycle_record(
                temp.path(),
                &CycleRecord {
                    arm_pid: 7,
                    watcher_pid: "8",
                    origin: "started",
                    started_at: index,
                    exit_code: 0,
                    signal: "none",
                    reason: "signal\twith\nseparators",
                    lock_before: "pid:none",
                    lock_after: "pid:none",
                    successor: "none",
                },
            )
            .expect("ledger append");
        }
        let ledger =
            fs::read_to_string(temp.path().join(".watch-cycle-exits.log")).expect("ledger");
        assert!(ledger.len() <= 262_144);
        assert!(ledger.lines().all(|line| line.split('\t').count() == 12));
    }

    #[test]
    fn afk_terminal_record_parser_rejects_partial_or_ambiguous_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert_eq!(super::afk_record_read(temp.path()).expect("absent"), None);
        fs::write(temp.path().join(".afk-daemon-terminal"), "tmux\texact\t\n")
            .expect("tmux record");
        assert_eq!(
            super::afk_record_read(temp.path()).expect("tmux"),
            Some(super::AfkTerminalRecord {
                backend: "tmux".to_owned(),
                target: "exact".to_owned(),
                extra: String::new(),
            })
        );
        fs::write(
            temp.path().join(".afk-daemon-terminal"),
            "herdr\tmissing\tws\n",
        )
        .expect("bad herdr");
        assert!(super::afk_record_read(temp.path()).is_err());
        fs::write(temp.path().join(".afk-daemon-terminal"), "none\t-\twrong\n")
            .expect("bad native");
        assert!(super::afk_record_read(temp.path()).is_err());
        fs::write(
            temp.path().join(".afk-daemon-terminal"),
            "tmux\tone\t\ntmux\ttwo\t\n",
        )
        .expect("multiple records");
        assert!(super::afk_record_read(temp.path()).is_err());
    }

    #[test]
    fn afk_native_start_publishes_private_state_and_clears_only_stale_delivery_artifacts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        for name in [
            ".subsuper-escalations",
            ".subsuper-escalations.since",
            ".subsuper-inject-wedged",
        ] {
            fs::write(state.join(name), "stale\n").expect("stale artifact");
        }
        fs::write(state.join(".wake-queue"), "durable\n").expect("wake queue");

        assert!(super::afk_launch_start_native(&state, temp.path()));
        assert!(state.join(".afk").is_file());
        assert_eq!(
            super::afk_record_read(&state).expect("record"),
            Some(super::AfkTerminalRecord {
                backend: "none".to_owned(),
                target: "-".to_owned(),
                extra: "native".to_owned(),
            })
        );
        assert_eq!(
            fs::metadata(state.join(".afk-daemon-terminal"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(state.join(".wake-queue").is_file());
        for name in [
            ".subsuper-escalations",
            ".subsuper-escalations.since",
            ".subsuper-inject-wedged",
        ] {
            assert!(!state.join(name).exists());
        }
    }

    #[test]
    fn afk_record_publication_refuses_a_symlink_without_changing_its_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let outside = temp.path().join("outside");
        fs::write(&outside, "protected\n").expect("outside");
        std::os::unix::fs::symlink(&outside, temp.path().join(".afk-daemon-terminal"))
            .expect("symlink");
        let record = super::AfkTerminalRecord {
            backend: "tmux".to_owned(),
            target: "exact-session".to_owned(),
            extra: String::new(),
        };
        assert!(super::afk_record_write(temp.path(), &record).is_err());
        assert_eq!(
            fs::read_to_string(outside).expect("outside bytes"),
            "protected\n"
        );
    }

    #[test]
    fn afk_launch_command_quotes_every_environment_value_as_literal_data() {
        let home = std::path::Path::new("/tmp/home with ' quote");
        let command = super::afk_launch_entry(
            home,
            "session:pane; touch /tmp/nope",
            "herdr",
            std::path::Path::new("/tmp/source root"),
        );
        assert!(command.starts_with("exec env MX_AFK_STATE_PREPARED=1 "));
        assert!(command.contains("MX_HOME='/tmp/home with '\\'' quote'"));
        assert!(command.contains("MX_SUPERVISOR_TARGET='session:pane; touch /tmp/nope'"));
        assert!(command.contains("MX_SUPERVISOR_BACKEND='herdr'"));
        let entry = std::env::var_os("MX_AFK_LAUNCH_ENTRY")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/source root/bin/mx-afk-start.sh"));
        assert!(command.ends_with(&super::afk_launch_quote(entry.to_string_lossy().as_ref())));
    }
}

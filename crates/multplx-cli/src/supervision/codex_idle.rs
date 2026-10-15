//! Codex turn-ended supervision: an owned watcher queues exact-thread input.
use super::*;
use multplx_core::process::ProcessIdentity;
use multplx_core::wake::{WakeQueue, WakeRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
mod queue;

const TARGET: &str = ".codex-idle-target.json";
const READY: &str = ".codex-idle-hook-ready.json";
const FAILURE: &str = ".codex-idle-failure";
const RECEIPTS: &str = ".codex-idle-receipts.json";
const UNCERTAIN: &str = ".codex-idle-uncertain.json";
const SUBMITTED: &str = ".codex-idle-submission.json";
const MAX_GROUP_WAKES: usize = 512;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Submitted {
    thread: String,
    codex_home: PathBuf,
    submission_id: String,
    wakes: BTreeSet<String>,
}
struct Watcher(std::process::Child);
impl Drop for Watcher {
    fn drop(&mut self) {
        terminate_group(&mut self.0);
    }
}
const REPAIR: &str = "Codex idle supervision failed; durable wakes remain unfinished. Inspect state/.codex-idle-failure, then run bin/mx-codex-idle.sh --retry from this same Codex session. Unsupported Codex CLI versions require an explicit foreground checkpoint fallback: bin/mx-watch-checkpoint.sh --seconds 180.";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Target {
    thread: String,
    codex_home: PathBuf,
    executable: PathBuf,
    owner: ProcessIdentity,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    multplx_core::filesystem::atomic_replace(
        path,
        &serde_json::to_vec(value).map_err(|error| error.to_string())?,
        0o600,
    )
    .map_err(|error| error.to_string())
}

fn owner_matches(state: &Path, target: &Target, probe: &impl ProcessProbe) -> bool {
    fs::read_to_string(state.join(".lock"))
        .is_ok_and(|text| text.trim() == target.owner.pid.to_string())
        && probe
            .identity(target.owner.pid)
            .is_ok_and(|owner| owner == target.owner)
}

fn hook_owner(probe: &impl ProcessProbe) -> Result<u32, String> {
    hook_owner_from(std::process::id(), probe)
}

fn hook_owner_from(mut pid: u32, probe: &impl ProcessProbe) -> Result<u32, String> {
    let matcher =
        regex::Regex::new(r"^(codex|codex-cli|codex\.js|codex\.py)$").expect("Codex owner matcher");
    for _ in 0..12 {
        let row = probe.ancestry_row(pid).map_err(|error| error.to_string())?;
        if multplx_core::session_lock::harness_command_matches(
            &row.command,
            &row.arguments,
            &matcher,
        ) {
            return Ok(pid);
        }
        if row.parent_pid <= 1 {
            break;
        }
        pid = row.parent_pid;
    }
    Err("SessionStart could not identify its owning CLI harness".into())
}

fn payload_thread(payload: &serde_json::Value) -> Result<String, String> {
    let thread = payload
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .ok_or("Stop hook did not supply session_id")?;
    let valid = thread.len() == 36
        && thread.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    valid
        .then(|| thread.to_owned())
        .ok_or_else(|| "Stop hook session_id is not an exact UUID".to_owned())
}

fn bind(state: &Path, target: &Target, probe: &impl ProcessProbe) -> Result<(), String> {
    bind_with_cancel(state, target, probe, queue::cancel)
}

fn bind_with_cancel(
    state: &Path,
    target: &Target,
    probe: &impl ProcessProbe,
    cancel: impl FnOnce(&Target, &str) -> Result<(), String>,
) -> Result<(), String> {
    let path = state.join(TARGET);
    if path.exists() {
        let old: Target = read_json(&path)?;
        if old == *target {
            return Ok(());
        }
        let prior_identity = probe.identity(old.owner.pid);
        if prior_identity
            .as_ref()
            .is_ok_and(|owner| owner == &old.owner)
        {
            return Err("this home is already bound to another Codex thread or CODEX_HOME for the live lock owner; stop that session before replacing its binding".into());
        }
        // The prior exact process lifetime must be known to have ended before
        // its native item can be cancelled for a replacement receiver.
        if let Some(submitted) = read_submitted(state, &old)? {
            if let Err(error) = prior_identity {
                let absent = i32::try_from(old.owner.pid)
                    .ok()
                    .filter(|pid| *pid > 0)
                    .and_then(rustix::process::Pid::from_raw)
                    .is_some_and(|pid| {
                        rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH)
                    });
                if !absent {
                    return Err(format!("prior native owner lifetime is uncertain: {error}"));
                }
            }
            // Delete is idempotent across crashes. An already-started item is
            // never interrupted, and its canonical wake remains unfinished.
            cancel(&old, &submitted.submission_id)?;
            fs::remove_file(state.join(SUBMITTED)).map_err(|error| error.to_string())?;
        }
    }
    write_json(&path, target)
}

/// A transport receipt is not a wake disposition. Read without claiming/draining.
fn pending(state: &Path) -> Result<BTreeSet<String>, String> {
    let mut keys = BTreeSet::new();
    let items = WakeQueue::new(state)
        .observe_inbox_items()
        .map_err(|error| error.to_string())?;
    let known = items
        .iter()
        .map(|item| item.record.sequence)
        .collect::<BTreeSet<_>>();
    for item in items {
        if item.acknowledged_at.is_none() {
            keys.insert(format!(
                "{}:{}",
                item.record.sequence,
                item.disposition_history.len()
            ));
        }
    }
    match fs::read_to_string(state.join(".wake-queue")) {
        Ok(rows) => {
            for line in rows.lines().filter(|line| !line.is_empty()) {
                let record = WakeRecord::parse(line).map_err(|error| error.to_string())?;
                if !known.contains(&record.sequence) {
                    keys.insert(format!("{}:0", record.sequence));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    Ok(keys)
}

// Disposition updates do not acknowledge an event. Keep the submitted group
// until the canonical sequence is settled, across its disposition revisions.
fn receipt_outstanding(receipt: &str, thread: &str, outstanding: &BTreeSet<String>) -> bool {
    let Some((sequence, revision)) = receipt
        .strip_prefix(&format!("{thread}:"))
        .and_then(|key| key.split_once(':'))
    else {
        return false;
    };
    if sequence.parse::<u64>().is_err() || revision.parse::<usize>().is_err() {
        return false;
    }
    outstanding.iter().any(|key| {
        key.split_once(':')
            .is_some_and(|(current, _)| current == sequence)
    })
}

fn queue_supported(executable: &Path) -> bool {
    let mut command = Command::new(executable);
    command.args(["queue", "--help"]);
    matches!(run_bounded_command_result(&mut command, Duration::from_secs(3), None, None, true), BoundedCommandResult::Completed { success: true, output } if output.contains("--thread") && output.contains("--message"))
}

fn fail(state: &Path, error: &str) -> Result<(), String> {
    multplx_core::filesystem::atomic_replace(state.join(FAILURE), error.as_bytes(), 0o600)
        .map_err(|error| error.to_string())
}

fn notify(state: &Path, target: &Target) -> Result<(), String> {
    notify_with_transport(
        state,
        target,
        |target, message| {
            let mut command = Command::new(&target.executable);
            command.env("CODEX_HOME", &target.codex_home).args([
                "queue",
                "--thread",
                &target.thread,
                "--message",
                message,
            ]);
            run_bounded_command_result(&mut command, Duration::from_secs(15), None, None, true)
        },
        queue::cancel,
    )
}

#[cfg(test)]
fn notify_with(
    state: &Path,
    target: &Target,
    send: impl FnOnce(&Target, &str) -> BoundedCommandResult,
) -> Result<(), String> {
    notify_with_transport(state, target, send, |_, _| {
        Err("fixture has no native cancellation".into())
    })
}

fn read_submitted(state: &Path, target: &Target) -> Result<Option<Submitted>, String> {
    let submission_path = state.join(SUBMITTED);
    let submission_present = match fs::symlink_metadata(&submission_path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("cannot inspect native submission receipt: {error}")),
    };
    if !submission_present {
        return Ok(None);
    }
    let submitted: Submitted = serde_json::from_slice(
        &multplx_core::filesystem::read_bounded_regular(&submission_path, 64 * 1024)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if submitted.thread != target.thread
        || submitted.codex_home != target.codex_home
        || submitted.wakes.is_empty()
        || queue::submission_id(
            &format!(
                "Queued message {} for thread {}.",
                submitted.submission_id, submitted.thread
            ),
            &target.thread,
        )
        .is_none()
        || submitted.wakes.iter().any(|key| {
            !key.strip_prefix(&format!("{}:", target.thread))
                .and_then(|key| key.split_once(':'))
                .is_some_and(|(sequence, revision)| {
                    sequence.parse::<u64>().is_ok() && revision.parse::<usize>().is_ok()
                })
        })
    {
        return Err("native submission receipt conflicts with exact-session binding".into());
    }
    Ok(Some(submitted))
}

fn notify_with_transport(
    state: &Path,
    target: &Target,
    send: impl FnOnce(&Target, &str) -> BoundedCommandResult,
    cancel: impl FnOnce(&Target, &str) -> Result<(), String>,
) -> Result<(), String> {
    let receipts_path = state.join(RECEIPTS);
    let existed = receipts_path.exists();
    let mut receipts: BTreeSet<String> = if existed {
        read_json(&receipts_path)?
    } else {
        BTreeSet::new()
    };
    let mut outstanding = pending(state)?;
    let before = receipts.clone();
    receipts.retain(|receipt| receipt_outstanding(receipt, &target.thread, &outstanding));
    if !existed || before != receipts {
        write_json(&receipts_path, &receipts)?;
    }
    let submission_path = state.join(SUBMITTED);
    if let Some(submitted) = read_submitted(state, target)? {
        if submitted
            .wakes
            .iter()
            .any(|receipt| receipt_outstanding(receipt, &target.thread, &outstanding))
        {
            return Ok(());
        }
        // The model may handle these facts while their FIFO input is still
        // staged. Remove only that exact native item, never a running turn.
        // Retain the receipt on failure so restart cannot submit another group.
        cancel(target, &submitted.submission_id)?;
        fs::remove_file(&submission_path).map_err(|error| error.to_string())?;
        outstanding = pending(state)?;
    }
    // One accepted native input can cover everything the receiver drains.
    // New arrivals remain durable and unmarked until this receipt group is
    // acknowledged. Never enqueue one stale future model turn per report.
    if !receipts.is_empty() {
        return Ok(());
    }
    let unseen = outstanding
        .into_iter()
        .filter(|key| !receipts.contains(&format!("{}:{key}", target.thread)))
        .take(MAX_GROUP_WAKES)
        .collect::<Vec<_>>();
    if unseen.is_empty() {
        return Ok(());
    }
    // Write before external enqueue: an interrupted external result is unknown.
    // Never blindly enqueue a duplicate; --retry is an explicit recovery.
    for key in &unseen {
        receipts.insert(format!("{}:{key}", target.thread));
    }
    write_json(&state.join(UNCERTAIN), &unseen)?;
    write_json(&receipts_path, &receipts)?;
    fail(
        state,
        "queue delivery is pending or uncertain; reconcile before explicit retry",
    )?;
    let body = format!(
        "Durable wake for home {}. Internal sequence:revision keys {} require reconciliation. Run bin/mx-wake-drain.sh and `mx wake list --unfinished` to obtain stable event_id values for `mx wake disposition` and `mx wake ack`; the internal keys are not CLI event IDs. Inspect canonical tasks, then record each durable disposition and acknowledgement. Transport enqueue does not acknowledge handling. When all inputs for unfinished accepted work are available, handling includes its already-authorized follow-up and delivery; continue those steps in this turn, delegating as required by your assignment. End this turn when handling is complete; the Stop hook owns subsequent watcher cycles.",
        state.display(),
        unseen.join(", ")
    );
    let message = multplx_domain::operational_input::construct(
        multplx_domain::operational_input::Kind::Watcher,
        &body,
    )
    .ok_or("cannot construct watcher input")?;
    match send(target, &message) {
        BoundedCommandResult::Completed {
            success: true,
            output,
        } => {
            if let Some(submission_id) = queue::submission_id(&output, &target.thread) {
                write_json(
                    &submission_path,
                    &Submitted {
                        thread: target.thread.clone(),
                        codex_home: target.codex_home.clone(),
                        submission_id,
                        wakes: receipts,
                    },
                )?;
            }
            fs::remove_file(state.join(UNCERTAIN)).map_err(|error| error.to_string())?;
            fs::remove_file(state.join(FAILURE)).map_err(|error| error.to_string())
        }
        result => {
            let error = format!("Codex queue failed or delivery is uncertain: {result:?}");
            fail(state, &error)?;
            Err(error)
        }
    }
}

fn run_bridge(root: &Path, home: &Path, source_root: &Path, state: &Path) -> Result<(), String> {
    let probe = SystemProcessProbe::default();
    let _lock = DirectoryLock::try_acquire(state.join(".codex-idle.lock"), &probe)
        .map_err(|error| error.to_string())?;
    let target: Target = read_json(&state.join(TARGET))?;
    let identity = probe
        .identity(std::process::id())
        .map_err(|error| error.to_string())?;
    write_json(&state.join(".codex-idle-process.json"), &identity)?;
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handlers = [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT]
        .into_iter()
        .map(|signal| signal_hook::flag::register(signal, shutdown.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let result = (|| {
        while !shutdown.load(std::sync::atomic::Ordering::SeqCst)
            && owner_matches(state, &target, &probe)
            && !state.join(".afk").exists()
        {
            if state.join(FAILURE).exists() || state.join(UNCERTAIN).exists() {
                return Err(REPAIR.into());
            }
            notify(state, &target)?;
            // Task completion is committed before its report wake is appended.
            // Keep the exact-session bridge through that no-work window; another
            // Stop callback cannot occur until the later wake reaches the model.
            // An idle home needs no watcher child or model input.
            if !autoarm_needed(state) {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            let child = Command::new(source_root.join("bin/mx-watch.sh"))
                .env("MX_ROOT_OVERRIDE", root)
                .env("MX_HOME", home)
                .env("MX_STATE_OVERRIDE", state)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .map_err(|error| error.to_string())?;
            let mut child = Watcher(child);
            loop {
                if shutdown.load(std::sync::atomic::Ordering::SeqCst)
                    || !owner_matches(state, &target, &probe)
                    || state.join(".afk").exists()
                {
                    return Ok(());
                }
                if !autoarm_needed(state) {
                    break;
                }
                match child.0.try_wait().map_err(|error| error.to_string())? {
                    Some(status) if status.success() => break,
                    Some(status) => return Err(format!("owned watcher exited {status}")),
                    None => {
                        // Report publication can happen independently of status
                        // scans, including after a watcher already marked the
                        // status endpoint. Queue receipts deduplicate delivery;
                        // observing does not claim or settle the model's wake.
                        notify(state, &target)?;
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
            notify(state, &target)?;
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();
    for handler in handlers {
        signal_hook::low_level::unregister(handler);
    }
    if let Err(error) = &result {
        if !state.join(UNCERTAIN).exists() && owner_matches(state, &target, &probe) {
            // A watcher failure is actionable even after the model ended its
            // turn. Publish once and use the working queue transport to wake it.
            let key = format!("codex-idle-failure-{}-{}", target.thread, identity.pid);
            let message = format!(
                "Codex owned watcher failed; inspect {} and repair before retrying.",
                state.join(FAILURE).display()
            );
            if WakeQueue::new(state)
                .append_once(
                    multplx_core::wake::WakeKind::Check,
                    &key,
                    &message,
                    SystemTime::now(),
                    &probe,
                )
                .is_ok()
            {
                let _ = notify(state, &target);
            }
        }
        let _ = fail(state, error);
    }
    result
}

pub(crate) fn entry(
    args: &[std::ffi::OsString],
    payload: &str,
    root: &Path,
    home: &Path,
    source_root: &Path,
) -> i32 {
    if args.iter().any(|value| value == "--help" || value == "-h") {
        println!(
            "Usage: mx-codex-idle.sh [--status|--register|--retry|--end|--run]\n--status is read-only and reports exact-session readiness, queue capability and degraded reasons. Stop hook owns detached exact-thread Codex queue supervision. Manual --retry and --end require CODEX_THREAD_ID matching this live owned session. --retry explicitly permits possible duplicate input after uncertain queue acceptance; durable wakes remain unacknowledged. --end stops only this thread's owned bridge. --register captures native SessionStart readiness; --register and --run are internal lifecycle handlers, not model supervision commands. Unsupported queue support requires an explicit foreground checkpoint fallback."
        );
        return 0;
    }
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if args == [std::ffi::OsString::from("--status")] {
        let active = std::env::var("MX_CODEX_IDLE_CLI").as_deref() == Ok("1");
        let readiness = multplx_domain::session::codex_hook_readiness(&state);
        let ready = readiness.is_ok();
        let probe = SystemProcessProbe::default();
        let target = read_json::<Target>(&state.join(READY)).ok();
        let primary_scope = multplx_core::primary_scope::matches(root, &state);
        let reason = if !active {
            Some(
                "CLI activation absent; direct launches require explicit MX_CODEX_IDLE_CLI=1 opt-in",
            )
        } else if !primary_scope {
            Some(
                "primary scope refused; verify the installed runtime binding and this home's exact live launcher connection before native hook recovery",
            )
        } else if let Err(reason) = readiness {
            Some(reason)
        } else if state.join(UNCERTAIN).exists() {
            Some("queue delivery is uncertain; reconcile before explicit retry")
        } else if state.join(FAILURE).exists() {
            Some(
                "retained delivery or watcher failure; inspect state/.codex-idle-failure before recovery",
            )
        } else if !target
            .as_ref()
            .is_some_and(|target| queue_supported(&target.executable))
        {
            Some(
                "installed queue command lacks --thread and --message support; bounded checkpoint recovery required",
            )
        } else {
            None
        };
        let bridge_live = read_json::<ProcessIdentity>(&state.join(".codex-idle-process.json"))
            .is_ok_and(|identity| {
                probe
                    .identity(identity.pid)
                    .is_ok_and(|current| current == identity)
            });
        let bound_target = read_json::<Target>(&state.join(TARGET)).ok();
        let bridge_live = bridge_live && target.is_some() && bound_target == target;
        let outstanding_delivery = target.as_ref().map_or_else(
            || Ok(BTreeSet::new()),
            |target| {
                let receipts = if state.join(RECEIPTS).exists() {
                    read_json::<BTreeSet<String>>(&state.join(RECEIPTS))?
                } else {
                    BTreeSet::new()
                };
                let outstanding = pending(&state)?;
                Ok::<_, String>(
                    receipts
                        .into_iter()
                        .filter(|receipt| {
                            receipt_outstanding(receipt, &target.thread, &outstanding)
                        })
                        .collect::<BTreeSet<_>>(),
                )
            },
        );
        let awaiting_reconciliation = outstanding_delivery
            .as_ref()
            .is_ok_and(|keys| !keys.is_empty());
        let reason = reason.or_else(|| outstanding_delivery.as_ref().err().map(|_| "delivery receipts or durable wakes cannot be read; inspect retained state before recovery"));
        let watcher_fresh =
            multplx_core::supervision::inspect(&state, grace(), SystemTime::now()).watcher_fresh;
        let needed = autoarm_needed(&state);
        let away = state.join(".afk").exists();
        let mode = if away {
            "away"
        } else if reason.is_some() {
            "degraded-checkpoint"
        } else if awaiting_reconciliation {
            "awaiting-wake-reconciliation"
        } else if !needed {
            "idle-no-work"
        } else if bridge_live && watcher_fresh {
            "healthy-idle"
        } else if bridge_live {
            "starting-or-degraded-watcher"
        } else {
            "ready-for-turn-end"
        };
        let reason = reason.or_else(|| awaiting_reconciliation.then_some("one exact-thread native wake was submitted but remains unacknowledged; reconcile its durable wake before another input is submitted; new arrivals remain durable and unmarked"));
        println!(
            "{}",
            serde_json::json!({"mode":mode,"reason":reason,"needed":needed,"bridge_live":bridge_live,"watcher_fresh":watcher_fresh,"native_hook_ready":ready,"primary_scope":primary_scope,"outstanding_delivery":outstanding_delivery.ok(),"activation":active,"away":away})
        );
        return 0;
    }
    if args == [std::ffi::OsString::from("--run")] {
        return match run_bridge(root, home, source_root, &state) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        };
    }
    if args.len() > 1
        || args
            .first()
            .is_some_and(|arg| arg != "--retry" && arg != "--end" && arg != "--register")
    {
        eprintln!("invalid Codex idle arguments");
        return 2;
    }
    if std::env::var("MX_CODEX_IDLE_CLI").as_deref() != Ok("1") {
        if !args.is_empty() {
            println!(
                "{}",
                serde_json::json!({"systemMessage":"Codex idle supervision is enabled only for a managed CLI launch or explicit MX_CODEX_IDLE_CLI=1 CLI opt-in; Desktop is unverified."})
            );
        }
        return 0;
    }
    let registering = args.first().is_some_and(|arg| arg == "--register");
    let stale_lock = fs::read_to_string(state.join(".lock"))
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .is_some_and(|pid| pid > 1 && !SystemProcessProbe::default().is_alive(pid));
    if !multplx_core::primary_scope::matches(root, &state)
        || (!registering || state.join(".lock").exists() && !stale_lock)
            && !autoarm_session_owned(&state)
    {
        return 0;
    }
    let mut bound = false;
    let outcome = (|| -> Result<(), String> {
        let document: serde_json::Value = if payload.is_empty() && !args.is_empty() {
            let thread = std::env::var("CODEX_THREAD_ID").map_err(
                |_| "manual recovery requires CODEX_THREAD_ID identifying this Codex session",
            )?;
            serde_json::json!({"session_id": thread})
        } else {
            serde_json::from_str(payload).map_err(|error| error.to_string())?
        };
        let thread = payload_thread(&document)?;
        let probe = SystemProcessProbe::default();
        let pid = if registering {
            hook_owner(&probe)?
        } else {
            fs::read_to_string(state.join(".lock"))
                .map_err(|error| error.to_string())?
                .trim()
                .parse()
                .map_err(|_| "invalid system lock")?
        };
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
            .ok_or("CODEX_HOME is unavailable")?;
        let target = Target {
            thread,
            codex_home: fs::canonicalize(codex_home).map_err(|error| error.to_string())?,
            executable: if let Some(path) = std::env::var_os("MX_REAL_CODEX").map(PathBuf::from) {
                if !path.is_absolute() {
                    return Err("MX_REAL_CODEX is not an absolute executable path".into());
                }
                fs::canonicalize(path).map_err(|error| error.to_string())?
            } else {
                if std::env::var_os("MX_SHIM_DIR").is_some() {
                    return Err(
                        "MX_REAL_CODEX is missing while primary harness shims are active".into(),
                    );
                }
                program_path("codex")
                    .and_then(|path| fs::canonicalize(path).ok())
                    .ok_or("Codex executable is unavailable")?
            },
            owner: probe.identity(pid).map_err(|error| error.to_string())?,
        };
        let _arm = DirectoryLock::acquire_wait(
            state.join(".codex-idle-arm.lock"),
            &probe,
            Duration::from_secs(2),
        )
        .map_err(|error| error.to_string())?;
        if registering {
            if let Ok(old) = read_json::<Target>(&state.join(READY))
                && old != target
                && probe
                    .identity(old.owner.pid)
                    .is_ok_and(|owner| owner == old.owner)
            {
                return Err("this home already registered another live Codex thread".into());
            }
            write_json(&state.join(READY), &target)?;
            return Ok(());
        }
        bind(&state, &target, &probe)?;
        bound = true;
        if !args.first().is_some_and(|arg| arg == "--end")
            && (!multplx_domain::session::codex_hook_ready_for_thread(&state, &target.thread)
                || !read_json::<Target>(&state.join(READY)).is_ok_and(|ready| ready == target))
        {
            return Err("Codex SessionStart hook readiness is missing for this exact thread and owner. Enable hooks and review native trust for the owned Multplx hooks, then restart this CLI session. Until ready, use bin/mx-watch-checkpoint.sh --seconds 180.".into());
        }
        if args.is_empty()
            && let Ok(identity) =
                read_json::<ProcessIdentity>(&state.join(".codex-idle-process.json"))
            && probe
                .identity(identity.pid)
                .is_ok_and(|current| current == identity)
        {
            return Ok(());
        }
        if args.first().is_some_and(|arg| arg == "--end") {
            if let Ok(identity) =
                read_json::<ProcessIdentity>(&state.join(".codex-idle-process.json"))
            {
                let mut terminator = SystemProcessTerminator::default();
                if !probe
                    .identity(identity.pid)
                    .is_ok_and(|current| current == identity)
                {
                    return Ok(());
                }
                terminator
                    .terminate(&identity)
                    .map_err(|error| error.to_string())?;
                if !terminator.wait_gone(&identity, Duration::from_secs(2)) {
                    return Err("bridge did not stop within two seconds".into());
                }
            }
            return Ok(());
        }
        if args.first().is_some_and(|arg| arg == "--retry") {
            let _ = fs::remove_file(state.join(FAILURE));
            if state.join(UNCERTAIN).exists() {
                let uncertain: Vec<String> = read_json(&state.join(UNCERTAIN))?;
                let mut receipts: BTreeSet<String> = read_json(&state.join(RECEIPTS))?;
                for key in uncertain {
                    receipts.remove(&format!("{}:{key}", target.thread));
                }
                write_json(&state.join(RECEIPTS), &receipts)?;
                fs::remove_file(state.join(UNCERTAIN)).map_err(|error| error.to_string())?;
            }
        }
        if state.join(FAILURE).exists() || state.join(UNCERTAIN).exists() {
            return Err(format!(
                "{}: {REPAIR}",
                fs::read_to_string(state.join(FAILURE)).unwrap_or_default()
            ));
        }
        if !autoarm_needed(&state) || state.join(".afk").exists() {
            return Ok(());
        }
        if !queue_supported(&target.executable) {
            return Err(format!(
                "installed Codex CLI does not support queue --thread --message. {REPAIR}"
            ));
        }
        if let Ok(identity) = read_json::<ProcessIdentity>(&state.join(".codex-idle-process.json"))
            && probe
                .identity(identity.pid)
                .is_ok_and(|current| current == identity)
        {
            return Ok(());
        }
        let mut child = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
            .args(["supervision", "mx-codex-idle.sh", "--run"])
            .env("MX_ROOT_OVERRIDE", root)
            .env("MX_HOME", home)
            .env("MX_STATE_OVERRIDE", &state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|error| error.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if let Ok(identity) =
                read_json::<ProcessIdentity>(&state.join(".codex-idle-process.json"))
                && identity.pid == child.id()
                && probe
                    .identity(identity.pid)
                    .is_ok_and(|current| current == identity)
            {
                return Ok(());
            }
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("Codex idle bridge failed to start: {status}"))
                };
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        terminate_group(&mut child);
        Err("Codex idle bridge did not publish readiness within two seconds".into())
    })();
    match outcome {
        Ok(()) => {
            println!("{{}}");
            0
        }
        Err(error) => {
            if bound {
                let _ = fail(&state, &error);
            }
            println!("{}", serde_json::json!({"systemMessage":error}));
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multplx_core::wake::{WakeDisposition, WakeDispositionKind, WakeKind};

    const THREAD: &str = "123e4567-e89b-12d3-a456-426614174000";

    pub(super) fn target(path: &Path) -> Target {
        Target {
            thread: THREAD.into(),
            codex_home: path.into(),
            executable: "/bin/false".into(),
            owner: SystemProcessProbe::default()
                .identity(std::process::id())
                .unwrap(),
        }
    }

    #[test]
    fn hook_owner_skips_interpreter_payloads_and_other_providers() {
        use multplx_core::process::AncestryRow;
        struct Probe {
            args: &'static str,
        }
        impl ProcessProbe for Probe {
            fn is_alive(&self, _: u32) -> bool {
                true
            }
            fn identity(&self, pid: u32) -> multplx_core::error::Result<ProcessIdentity> {
                Ok(ProcessIdentity {
                    pid,
                    marker: "fixture".into(),
                })
            }
            fn ancestry_row(&self, pid: u32) -> multplx_core::error::Result<AncestryRow> {
                Ok(if pid == 2 {
                    AncestryRow {
                        parent_pid: 3,
                        command: "python3".into(),
                        arguments: self.args.into(),
                    }
                } else {
                    AncestryRow {
                        parent_pid: 1,
                        command: "codex".into(),
                        arguments: "codex app-server".into(),
                    }
                })
            }
        }
        for args in [
            "python3 /tmp/mx-codex-fixture/handle.py",
            "python3 handle.py /tmp/codex",
            "python3 -X codex handle.py",
            "python3 /tmp/claude.py",
        ] {
            assert_eq!(hook_owner_from(2, &Probe { args }).unwrap(), 3, "{args}");
        }
        assert_eq!(
            hook_owner_from(
                2,
                &Probe {
                    args: "python3 /tmp/codex.py"
                }
            )
            .unwrap(),
            2
        );
    }

    fn queued() -> BoundedCommandResult {
        BoundedCommandResult::Completed {
            success: true,
            output: "queued".into(),
        }
    }

    fn accepted(target: &Target, id: &str) -> BoundedCommandResult {
        BoundedCommandResult::Completed {
            success: true,
            output: format!("Queued message {id} for thread {}.\n", target.thread),
        }
    }

    #[test]
    fn exact_native_submission_is_cancelled_after_ack_and_before_next_group() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let target = target(state);
        let probe = SystemProcessProbe::default();
        let queue = WakeQueue::new(state);
        let id = "123e4567-e89b-12d3-a456-426614174001";
        for key in ["first", "second"] {
            queue
                .append(WakeKind::Signal, key, key, SystemTime::now(), &probe)
                .unwrap();
        }
        notify_with_transport(
            state,
            &target,
            |target, text| {
                assert!(text.contains("Internal sequence:revision keys 1:0, 2:0"));
                assert!(text.contains("`mx wake list --unfinished`"));
                assert!(text.contains("stable event_id"));
                accepted(target, id)
            },
            |_, _| panic!("no previous group"),
        )
        .unwrap();
        let recovered: Submitted = read_json(&state.join(SUBMITTED)).unwrap();
        assert_eq!(recovered.submission_id, id);
        let claims = queue
            .claim_available(&target.owner, SystemTime::now(), 64, &probe)
            .unwrap();
        acknowledge(&queue, &target, &claims[0]);
        queue
            .append(WakeKind::Signal, "new", "new", SystemTime::now(), &probe)
            .unwrap();
        notify_with_transport(
            state,
            &target,
            |_, _| panic!("partial acknowledgement"),
            |_, _| panic!("partial acknowledgement cannot cancel"),
        )
        .unwrap();
        acknowledge(&queue, &target, &claims[1]);
        let error = notify_with_transport(
            state,
            &target,
            |_, _| panic!("failed cancellation gates next input"),
            |_, actual| {
                assert_eq!(actual, id);
                Err("disconnected native proxy".into())
            },
        )
        .unwrap_err();
        assert!(error.contains("disconnected"));
        assert!(state.join(SUBMITTED).exists());
        assert_eq!(pending(state).unwrap(), BTreeSet::from(["3:0".into()]));
        let cancelled = std::cell::Cell::new(false);
        notify_with_transport(
            state,
            &target,
            |_, text| {
                assert!(cancelled.get(), "new group preceded cancellation receipt");
                assert!(text.contains("keys 3:0 require"));
                queued()
            },
            |actual_target, actual| {
                assert_eq!(actual_target.thread, target.thread);
                assert_eq!(actual, id);
                cancelled.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(!state.join(SUBMITTED).exists());
        assert_eq!(queue.observe_unfinished_count().unwrap(), 1);
    }

    #[test]
    fn acknowledgement_during_enqueue_retains_id_for_cancellation_without_fabricating_ack() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let target = target(state);
        let probe = SystemProcessProbe::default();
        let queue = WakeQueue::new(state);
        queue
            .append(
                WakeKind::Signal,
                "first",
                "report",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        notify_with_transport(
            state,
            &target,
            |actual, _| {
                let items = queue
                    .claim_available(&target.owner, SystemTime::now(), 64, &probe)
                    .unwrap();
                acknowledge(&queue, &target, &items[0]);
                accepted(actual, "123e4567-e89b-12d3-a456-426614174001")
            },
            |_, _| panic!("no previous input"),
        )
        .unwrap();
        let recovered: Target =
            serde_json::from_value(serde_json::to_value(&target).unwrap()).unwrap();
        notify_with_transport(
            state,
            &recovered,
            |_, _| panic!("no unfinished input"),
            |_, _| Ok(()),
        )
        .unwrap();
        assert_eq!(queue.observe_unfinished_count().unwrap(), 0);
        assert!(!state.join(SUBMITTED).exists());
        // A successful external send followed by a local persistence failure
        // stays ambiguous, rather than clearing the pre-send journal.
        queue
            .append(
                WakeKind::Signal,
                "second",
                "report",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        assert!(
            notify_with_transport(
                state,
                &target,
                |actual, _| {
                    fs::create_dir(state.join(SUBMITTED)).unwrap();
                    accepted(actual, "123e4567-e89b-12d3-a456-426614174001")
                },
                |_, _| Ok(())
            )
            .is_err()
        );
        assert!(state.join(UNCERTAIN).exists());
        assert!(state.join(FAILURE).exists());
        assert_eq!(pending(state).unwrap().len(), 1);
    }

    #[test]
    fn malformed_or_foreign_native_receipts_cannot_authorize_cancellation_or_submission() {
        for mode in [
            "thread",
            "home",
            "uuid",
            "key",
            "empty",
            "malformed",
            "oversized",
            "symlink",
            "dangling",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let state = temp.path();
            let target = target(state);
            let path = state.join(SUBMITTED);
            let mut receipt = serde_json::json!({"thread":target.thread,"codex_home":target.codex_home,"submission_id":"123e4567-e89b-12d3-a456-426614174001","wakes":[format!("{}:1:0",target.thread)]});
            match mode {
                "thread" => receipt["thread"] = serde_json::json!("other"),
                "home" => receipt["codex_home"] = serde_json::json!("/foreign"),
                "uuid" => receipt["submission_id"] = serde_json::json!("invalid"),
                "key" => receipt["wakes"] = serde_json::json!(["foreign:1:0"]),
                "empty" => receipt["wakes"] = serde_json::json!([]),
                _ => {}
            }
            let bytes = match mode {
                "malformed" => b"broken".to_vec(),
                "oversized" => vec![b'x'; 64 * 1024 + 1],
                _ => serde_json::to_vec(&receipt).unwrap(),
            };
            if matches!(mode, "symlink" | "dangling") {
                let foreign = state.join("foreign.json");
                if mode == "symlink" {
                    fs::write(&foreign, &bytes).unwrap();
                }
                std::os::unix::fs::symlink(foreign, &path).unwrap();
            } else {
                fs::write(&path, &bytes).unwrap();
            }
            assert!(
                notify_with_transport(
                    state,
                    &target,
                    |_, _| panic!("foreign receipt cannot send"),
                    |_, _| panic!("foreign receipt cannot cancel")
                )
                .is_err(),
                "{mode}"
            );
            assert!(
                fs::symlink_metadata(&path).is_ok(),
                "receipt lost for {mode}"
            );
        }
    }

    fn acknowledge(queue: &WakeQueue, target: &Target, item: &multplx_core::wake::WakeInboxItem) {
        let probe = SystemProcessProbe::default();
        queue
            .record_disposition(
                &item.event_id,
                &target.owner,
                WakeDisposition {
                    kind: WakeDispositionKind::Handled,
                    detail: "handled canonical event".into(),
                    recorded_at: 1,
                    condition: None,
                    resume_trigger: None,
                    recheck_after_epoch: None,
                    follow_up_id: None,
                },
                &probe,
            )
            .unwrap();
        queue
            .acknowledge(&item.event_id, &target.owner, SystemTime::now(), &probe)
            .unwrap();
    }

    #[test]
    fn burst_coalescing_waits_for_full_group_and_preserves_new_arrivals_across_restart() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let probe = SystemProcessProbe::default();
        let target = target(state);
        let queue = WakeQueue::new(state);
        for key in ["first", "second"] {
            queue
                .append(WakeKind::Signal, key, "report", SystemTime::now(), &probe)
                .unwrap();
        }
        notify_with(state, &target, |_, input| {
            assert!(input.contains("1:0, 2:0"));
            queued()
        })
        .unwrap();
        queue
            .append(
                WakeKind::Signal,
                "new",
                "later report",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        notify_with(state, &target, |_, _| {
            panic!("one input already covers receiver reconciliation")
        })
        .unwrap();
        let mut items = queue
            .claim_available(&target.owner, SystemTime::now(), 10, &probe)
            .unwrap();
        acknowledge(&queue, &target, &items.remove(0));
        notify_with(state, &target, |_, _| {
            panic!("partial acknowledgement cannot release group")
        })
        .unwrap();
        write_json(&state.join(TARGET), &target).unwrap();
        let recovered: Target = read_json(&state.join(TARGET)).unwrap();
        notify_with(state, &recovered, |_, _| {
            panic!("restart cannot submit a duplicate")
        })
        .unwrap();
        assert_eq!(
            read_json::<BTreeSet<String>>(&state.join(RECEIPTS)).unwrap(),
            BTreeSet::from([format!("{}:2:0", target.thread)])
        );
        let last_group_item = items.remove(0);
        queue
            .record_disposition(
                &last_group_item.event_id,
                &target.owner,
                WakeDisposition {
                    kind: WakeDispositionKind::Handled,
                    detail: "handled but not yet acknowledged".into(),
                    recorded_at: 1,
                    condition: None,
                    resume_trigger: None,
                    recheck_after_epoch: None,
                    follow_up_id: None,
                },
                &probe,
            )
            .unwrap();
        notify_with(state, &recovered, |_, _| {
            panic!("a disposition revision is not acknowledgement")
        })
        .unwrap();
        assert_eq!(
            read_json::<BTreeSet<String>>(&state.join(RECEIPTS)).unwrap(),
            BTreeSet::from([format!("{}:2:0", target.thread)])
        );
        queue
            .acknowledge(
                &last_group_item.event_id,
                &target.owner,
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        notify_with(state, &recovered, |_, input| {
            assert!(input.contains("Internal sequence:revision keys 3:0 require"));
            queued()
        })
        .unwrap();
        assert_eq!(pending(state).unwrap(), BTreeSet::from(["3:0".into()]));
        // Later work handled in the same turn needs no already-stale input.
        queue
            .append(
                WakeKind::Signal,
                "fourth",
                "same turn",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        acknowledge(&queue, &target, &items.remove(0));
        let later = queue
            .claim_available(&target.owner, SystemTime::now(), 10, &probe)
            .unwrap();
        for item in later {
            acknowledge(&queue, &target, &item);
        }
        notify_with(state, &recovered, |_, _| {
            panic!("all canonical work already handled")
        })
        .unwrap();
        assert!(pending(state).unwrap().is_empty());
    }

    #[test]
    fn acknowledgement_during_enqueue_does_not_starve_arrivals_while_receiver_is_busy() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let probe = SystemProcessProbe::default();
        let target = target(state);
        let queue = WakeQueue::new(state);
        queue
            .append(
                WakeKind::Signal,
                "first",
                "report",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        for sequence in 1..=3 {
            notify_with(state, &target, |_, input| {
                assert!(input.contains(&format!(
                    "Internal sequence:revision keys {sequence}:0 require"
                )));
                let items = queue
                    .claim_available(&target.owner, SystemTime::now(), 10, &probe)
                    .unwrap();
                assert_eq!(items.len(), 1);
                acknowledge(&queue, &target, &items[0]);
                if sequence < 3 {
                    queue
                        .append(
                            WakeKind::Signal,
                            &format!("arrival-{sequence}"),
                            "new report",
                            SystemTime::now(),
                            &probe,
                        )
                        .unwrap();
                }
                queued()
            })
            .unwrap();
        }
        notify_with(state, &target, |_, _| panic!("no future work")).unwrap();
        assert!(pending(state).unwrap().is_empty());
        assert!(!state.join(UNCERTAIN).exists());
    }

    #[test]
    fn independent_threads_and_uncertain_groups_keep_unsubmitted_arrivals_durable() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let probe = SystemProcessProbe::default();
        let left_target = target(left.path());
        let mut right_target = target(right.path());
        right_target.thread = "123e4567-e89b-12d3-a456-426614174001".into();
        let left_queue = WakeQueue::new(left.path());
        let right_queue = WakeQueue::new(right.path());
        left_queue
            .append(
                WakeKind::Signal,
                "left",
                "report",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        notify_with(left.path(), &left_target, |_, _| {
            BoundedCommandResult::TimedOut
        })
        .unwrap_err();
        left_queue
            .append(
                WakeKind::Signal,
                "left-new",
                "new report",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        notify_with(left.path(), &left_target, |_, _| {
            panic!("uncertain transport must never retry blindly")
        })
        .unwrap();
        assert_eq!(pending(left.path()).unwrap().len(), 2);
        assert_eq!(
            read_json::<Vec<String>>(&left.path().join(UNCERTAIN)).unwrap(),
            vec!["1:0"]
        );
        assert_eq!(
            read_json::<BTreeSet<String>>(&left.path().join(RECEIPTS)).unwrap(),
            BTreeSet::from([format!("{}:1:0", left_target.thread)])
        );
        right_queue
            .append(
                WakeKind::Signal,
                "right",
                "report",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        notify_with(right.path(), &right_target, |actual, _| {
            assert_eq!(actual.thread, right_target.thread);
            queued()
        })
        .unwrap();
        assert!(left.path().join(FAILURE).exists());
        assert!(!right.path().join(FAILURE).exists());
    }

    #[test]
    fn quiet_notification_observation_does_not_rewrite_receipts() {
        use std::os::unix::fs::MetadataExt;
        let temp = tempfile::tempdir().unwrap();
        let target = target(temp.path());
        let call = || notify_with(temp.path(), &target, |_, _| panic!("quiet transport send"));
        call().unwrap();
        let path = temp.path().join(RECEIPTS);
        let before = fs::metadata(&path).unwrap();
        for _ in 0..4 {
            call().unwrap();
        }
        let after = fs::metadata(path).unwrap();
        assert_eq!(before.ino(), after.ino());
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    }

    #[test]
    fn exact_thread_binding_refuses_retarget_and_pid_reuse() {
        let temp = tempfile::tempdir().unwrap();
        let probe = SystemProcessProbe::default();
        let original = target(temp.path());
        fs::write(temp.path().join(".lock"), original.owner.pid.to_string()).unwrap();
        assert!(owner_matches(temp.path(), &original, &probe));
        bind(temp.path(), &original, &probe).unwrap();
        bind(temp.path(), &original, &probe).unwrap();
        let mut other = original.clone();
        other.thread = "123e4567-e89b-12d3-a456-426614174001".into();
        assert!(bind(temp.path(), &other, &probe).is_err());
        other = original.clone();
        other.codex_home = temp.path().join("other");
        assert!(bind(temp.path(), &other, &probe).is_err());
        other = original.clone();
        other.owner.marker = "old lifetime".into();
        assert!(!owner_matches(temp.path(), &other, &probe));
        write_json(&temp.path().join(TARGET), &other).unwrap();
        bind(temp.path(), &original, &probe).unwrap();
        fs::write(temp.path().join(".lock"), "wrong").unwrap();
        assert!(!owner_matches(temp.path(), &original, &probe));
        fs::remove_file(temp.path().join(".lock")).unwrap();
        assert!(!owner_matches(temp.path(), &original, &probe));
        fs::write(temp.path().join(TARGET), "broken").unwrap();
        assert!(bind(temp.path(), &original, &probe).is_err());
        assert!(read_json::<Target>(&temp.path().join("absent")).is_err());
        assert!(write_json(&temp.path().join("missing/record"), &original).is_err());
    }

    #[test]
    fn dead_owner_replacement_reconciles_only_prior_submission_before_binding() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let probe = SystemProcessProbe::default();
        let current = target(state);
        let mut old = current.clone();
        old.thread = "123e4567-e89b-12d3-a456-426614174001".into();
        old.owner.marker = "confirmed prior lifetime".into();
        write_json(&state.join(TARGET), &old).unwrap();
        let receipt = Submitted {
            thread: old.thread.clone(),
            codex_home: old.codex_home.clone(),
            submission_id: "123e4567-e89b-12d3-a456-426614174002".into(),
            wakes: BTreeSet::from([format!("{}:1:0", old.thread)]),
        };
        write_json(&state.join(SUBMITTED), &receipt).unwrap();
        write_json(&state.join(RECEIPTS), &receipt.wakes).unwrap();
        let mut live = old.clone();
        live.owner = current.owner.clone();
        write_json(&state.join(TARGET), &live).unwrap();
        assert!(
            bind_with_cancel(state, &current, &probe, |_, _| panic!(
                "live prior owner cannot be cancelled"
            ))
            .is_err()
        );
        assert_eq!(read_json::<Target>(&state.join(TARGET)).unwrap(), live);
        write_json(&state.join(TARGET), &old).unwrap();
        let queue = WakeQueue::new(state);
        queue
            .append(
                WakeKind::Signal,
                "unfinished",
                "still needs a receiver",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        queue
            .claim_available(&current.owner, SystemTime::now(), 64, &probe)
            .unwrap();
        let cancelled = std::cell::Cell::new(false);
        bind_with_cancel(state, &current, &probe, |prior, id| {
            assert_eq!(prior, &old);
            assert_eq!(id, receipt.submission_id);
            assert_eq!(
                read_json::<Target>(&state.join(TARGET)).unwrap(),
                old,
                "binding changed before external reconciliation"
            );
            cancelled.set(true);
            Err("native cancellation not yet confirmed".into())
        })
        .unwrap_err();
        assert!(cancelled.get());
        assert_eq!(read_json::<Target>(&state.join(TARGET)).unwrap(), old);
        assert!(state.join(SUBMITTED).exists());
        // Retry is idempotent even if the old native item was already started
        // or deletion committed before a crash. The canonical wake stays live.
        bind_with_cancel(state, &current, &probe, |prior, id| {
            assert_eq!(prior, &old);
            assert_eq!(id, receipt.submission_id);
            Ok(())
        })
        .unwrap();
        assert_eq!(read_json::<Target>(&state.join(TARGET)).unwrap(), current);
        assert!(!state.join(SUBMITTED).exists());
        assert_eq!(pending(state).unwrap(), BTreeSet::from(["1:0".into()]));
        notify_with(state, &current, |actual, _| {
            assert_eq!(actual, &current);
            queued()
        })
        .unwrap();
        assert_eq!(queue.observe_unfinished_count().unwrap(), 1);
        assert_eq!(pending(state).unwrap(), BTreeSet::from(["1:0".into()]));
        assert_eq!(
            read_json::<BTreeSet<String>>(&state.join(RECEIPTS)).unwrap(),
            BTreeSet::from([format!("{}:1:0", current.thread)])
        );
    }

    #[test]
    fn replacement_does_not_treat_failed_identity_observation_as_process_death() {
        struct Uncertain;
        impl ProcessProbe for Uncertain {
            fn is_alive(&self, _: u32) -> bool {
                true
            }
            fn identity(&self, _: u32) -> multplx_core::error::Result<ProcessIdentity> {
                Err(multplx_core::error::CoreError::Command {
                    command: "ps".into(),
                    reason: "fixture observation denied".into(),
                })
            }
            fn ancestry_row(
                &self,
                _: u32,
            ) -> multplx_core::error::Result<multplx_core::process::AncestryRow> {
                unreachable!()
            }
        }
        for uncertain in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let state = temp.path();
            let current = target(state);
            let mut old = current.clone();
            old.thread = "123e4567-e89b-12d3-a456-426614174001".into();
            if !uncertain {
                let mut process = Command::new("/bin/sleep").arg("60").spawn().unwrap();
                old.owner = SystemProcessProbe::default()
                    .identity(process.id())
                    .unwrap();
                process.kill().unwrap();
                process.wait().unwrap();
            }
            write_json(&state.join(TARGET), &old).unwrap();
            let receipt = Submitted {
                thread: old.thread.clone(),
                codex_home: old.codex_home.clone(),
                submission_id: "123e4567-e89b-12d3-a456-426614174002".into(),
                wakes: BTreeSet::from([format!("{}:1:0", old.thread)]),
            };
            write_json(&state.join(SUBMITTED), &receipt).unwrap();
            if uncertain {
                assert!(
                    bind_with_cancel(state, &current, &Uncertain, |_, _| panic!(
                        "uncertain live owner cannot authorize native deletion"
                    ))
                    .is_err()
                );
                assert_eq!(read_json::<Target>(&state.join(TARGET)).unwrap(), old);
                assert!(state.join(SUBMITTED).exists());
            } else {
                bind_with_cancel(
                    state,
                    &current,
                    &SystemProcessProbe::default(),
                    |prior, _| {
                        assert_eq!(prior, &old);
                        Ok(())
                    },
                )
                .unwrap();
                assert_eq!(read_json::<Target>(&state.join(TARGET)).unwrap(), current);
                assert!(!state.join(SUBMITTED).exists());
            }
        }
    }

    #[test]
    fn stop_payload_requires_uuid_without_alias_or_shell_text() {
        assert_eq!(
            payload_thread(&serde_json::json!({"session_id": THREAD})).unwrap(),
            THREAD
        );
        for value in [
            serde_json::json!({}),
            serde_json::json!({"session_id": "name"}),
            serde_json::json!({"session_id": "123e4567-e89b-12d3-a456-42661417400z"}),
            serde_json::json!({"session_id": "123e4567_e89b-12d3-a456-426614174000"}),
        ] {
            assert!(payload_thread(&value).is_err());
        }
    }

    #[test]
    fn receipts_preserve_durable_wakes_dedupe_and_prune_only_handled() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let probe = SystemProcessProbe::default();
        let target = target(state);
        let queue = WakeQueue::new(state);
        assert!(pending(state).unwrap().is_empty());
        let record = queue
            .append(WakeKind::Signal, "child", "done", SystemTime::now(), &probe)
            .unwrap();
        notify_with(state, &target, |_, input| {
            assert!(input.starts_with(multplx_domain::operational_input::PREFIX));
            assert!(input.contains(&format!("{}:0", record.sequence)));
            BoundedCommandResult::Completed {
                success: true,
                output: "queued".into(),
            }
        })
        .unwrap();
        assert!(state.join(".wake-queue").exists());
        assert!(!state.join(FAILURE).exists());
        assert!(!state.join(UNCERTAIN).exists());
        notify_with(state, &target, |_, _| panic!("duplicate transport enqueue")).unwrap();
        let item = queue
            .claim_available(&target.owner, SystemTime::now(), 10, &probe)
            .unwrap()
            .remove(0);
        notify_with(state, &target, |_, _| panic!("claim is not a new wake")).unwrap();
        queue
            .record_disposition(
                &item.event_id,
                &target.owner,
                WakeDisposition {
                    kind: WakeDispositionKind::Handled,
                    detail: "checked canonical task".into(),
                    recorded_at: 1,
                    condition: None,
                    resume_trigger: None,
                    recheck_after_epoch: None,
                    follow_up_id: None,
                },
                &probe,
            )
            .unwrap();
        assert!(!pending(state).unwrap().is_empty());
        queue
            .acknowledge(&item.event_id, &target.owner, SystemTime::now(), &probe)
            .unwrap();
        notify_with(state, &target, |_, _| panic!("acknowledged wake")).unwrap();
        assert!(
            read_json::<BTreeSet<String>>(&state.join(RECEIPTS))
                .unwrap()
                .is_empty()
        );
        for kind in [WakeKind::Check, WakeKind::Heartbeat] {
            queue
                .append(kind, "poller", "actionable", SystemTime::now(), &probe)
                .unwrap();
        }
        assert_eq!(pending(state).unwrap().len(), 2);
        notify_with(state, &target, |_, _| BoundedCommandResult::TimedOut).unwrap_err();
        assert!(state.join(FAILURE).exists());
        assert_eq!(pending(state).unwrap().len(), 2);
        assert_eq!(
            read_json::<Vec<String>>(&state.join(UNCERTAIN))
                .unwrap()
                .len(),
            2
        );
        notify_with(state, &target, |_, _| {
            panic!("ambiguous send must not blindly retry")
        })
        .unwrap();
        fs::write(state.join(".wake-queue"), "invalid").unwrap();
        assert!(pending(state).is_err());
        fs::remove_file(state.join(".wake-queue")).unwrap();
        fs::create_dir(state.join(".wake-queue")).unwrap();
        assert!(pending(state).is_err());
        fs::write(state.join(RECEIPTS), "broken").unwrap();
        assert!(notify_with(state, &target, |_, _| panic!()).is_err());
    }

    #[test]
    fn bridge_releases_owned_lock_on_bad_binding_or_away_home() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        assert!(run_bridge(state, state, state, state).is_err());
        let original = target(state);
        write_json(&state.join(TARGET), &original).unwrap();
        fs::write(state.join(".lock"), original.owner.pid.to_string()).unwrap();
        fs::write(state.join(".afk"), "away").unwrap();
        run_bridge(state, state, state, state).unwrap();
        fs::remove_file(state.join(".afk")).unwrap();
        assert!(state.join(".codex-idle-process.json").exists());
        assert!(!state.join(".codex-idle.lock").exists());
        fs::write(
            state.join(".wake-queue"),
            WakeRecord::new(1, 1, WakeKind::Check, "test", "check").render(),
        )
        .unwrap();
        fail(state, "retained failure").unwrap();
        assert!(run_bridge(state, state, state, state).is_err());
        assert!(!state.join(".codex-idle.lock").exists());
        assert_eq!(entry(&["--help".into()], "", state, state, state), 0);
        assert_eq!(entry(&["--bad".into()], "", state, state, state), 2);
        assert_eq!(entry(&[], "", state, state, state), 0);
    }
}

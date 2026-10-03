//! Codex turn-ended supervision: an owned watcher queues exact-thread input.
use super::*;
use multplx_core::process::ProcessIdentity;
use multplx_core::wake::{WakeQueue, WakeRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const TARGET: &str = ".codex-idle-target.json";
const READY: &str = ".codex-idle-hook-ready.json";
const FAILURE: &str = ".codex-idle-failure";
const RECEIPTS: &str = ".codex-idle-receipts.json";
const UNCERTAIN: &str = ".codex-idle-uncertain.json";
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
    let matcher = multplx_core::session_lock::harness_regex();
    let mut pid = std::process::id();
    for _ in 0..12 {
        let row = probe.ancestry_row(pid).map_err(|error| error.to_string())?;
        let name = Path::new(&row.command)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name == "codex" || name == "codex-cli" {
            return Ok(pid);
        }
        let interpreter = row.command.to_ascii_lowercase();
        if (interpreter.contains("python") || interpreter.contains("node"))
            && row.arguments.split_whitespace().skip(1).any(|argument| {
                Path::new(argument)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| matcher.is_match(name))
            })
        {
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
    let path = state.join(TARGET);
    if path.exists() {
        let old: Target = read_json(&path)?;
        if old == *target {
            return Ok(());
        }
        if probe
            .identity(old.owner.pid)
            .is_ok_and(|owner| owner == old.owner)
        {
            return Err("this home is already bound to another Codex thread or CODEX_HOME for the live lock owner; stop that session before replacing its binding".into());
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
    notify_with(state, target, |target, message| {
        let mut command = Command::new(&target.executable);
        command.env("CODEX_HOME", &target.codex_home).args([
            "queue",
            "--thread",
            &target.thread,
            "--message",
            message,
        ]);
        run_bounded_command_result(&mut command, Duration::from_secs(15), None, None, true)
    })
}

fn notify_with(
    state: &Path,
    target: &Target,
    send: impl FnOnce(&Target, &str) -> BoundedCommandResult,
) -> Result<(), String> {
    let receipts_path = state.join(RECEIPTS);
    let mut receipts: BTreeSet<String> = if receipts_path.exists() {
        read_json(&receipts_path)?
    } else {
        BTreeSet::new()
    };
    let outstanding = pending(state)?;
    receipts.retain(|receipt| {
        outstanding
            .iter()
            .any(|key| receipt == &format!("{}:{key}", target.thread))
    });
    write_json(&receipts_path, &receipts)?;
    let unseen = outstanding
        .into_iter()
        .filter(|key| !receipts.contains(&format!("{}:{key}", target.thread)))
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
        "Durable wake for home {}. Wake IDs {} require reconciliation. Run bin/mx-wake-drain.sh, inspect canonical tasks, then record each durable disposition and acknowledgement. Transport enqueue does not acknowledge handling. End this turn when handling is complete; the Stop hook owns subsequent watcher cycles.",
        state.display(),
        unseen.join(", ")
    );
    let message = multplx_domain::operational_input::construct(
        multplx_domain::operational_input::Kind::Watcher,
        &body,
    )
    .ok_or("cannot construct watcher input")?;
    match send(target, &message) {
        BoundedCommandResult::Completed { success: true, .. } => {
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
            && autoarm_needed(state)
        {
            if state.join(FAILURE).exists() || state.join(UNCERTAIN).exists() {
                return Err(REPAIR.into());
            }
            notify(state, &target)?;
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
                    || !autoarm_needed(state)
                {
                    return Ok(());
                }
                match child.0.try_wait().map_err(|error| error.to_string())? {
                    Some(status) if status.success() => break,
                    Some(status) => return Err(format!("owned watcher exited {status}")),
                    None => std::thread::sleep(Duration::from_millis(100)),
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
            "Usage: mx-codex-idle.sh [--register|--retry|--end|--run]\nStop hook owns detached exact-thread Codex queue supervision. Manual --retry and --end require CODEX_THREAD_ID matching this live owned session. --retry explicitly permits possible duplicate input after uncertain queue acceptance; durable wakes remain unacknowledged. --end stops only this thread's owned bridge. --register captures native SessionStart readiness; --register and --run are internal lifecycle handlers, not model supervision commands. Unsupported queue support requires an explicit foreground checkpoint fallback."
        );
        return 0;
    }
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
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

    fn target(path: &Path) -> Target {
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
    fn bridge_releases_owned_lock_on_bad_binding_or_idle_home() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        assert!(run_bridge(state, state, state, state).is_err());
        let original = target(state);
        write_json(&state.join(TARGET), &original).unwrap();
        fs::write(state.join(".lock"), original.owner.pid.to_string()).unwrap();
        run_bridge(state, state, state, state).unwrap();
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

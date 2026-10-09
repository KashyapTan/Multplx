//! Exact managed Codex SessionStart identity, without transcript discovery scans.
use multplx_core::process::{ProcessProbe, SystemProcessProbe};
use multplx_domain::lifecycle::subagent_model::{Attempt, TaskRecord};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::Duration,
};

const HELP: &str = "Usage: multplx task-session inspect TASK\n       multplx task-session history TASK\nRead the exact current managed Codex SessionStart receipt. UUID is hook verified; transcript is available only when the hook supplied a validated path. This does not prove assignment acceptance or authorize resume. Other harnesses and old launches may have no receipt. The multplx command selects the configured operational home from any cwd. Low-level mx task-session uses MX_STATE_OVERRIDE, MX_HOME, then the current home. History lists retained identities without asserting live execution and reports invalid individual receipts with retained paths. Receipts preserve full process identity under a shared 1 MiB writer/read bound.\n";
// Full process markers can include provider argv and must remain exact.
const MAX_RECEIPT_BYTES: usize = 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct Receipt {
    task_id: String,
    owner_state: PathBuf,
    attempt: Attempt,
    provider: String,
    backend: String,
    provider_owner: multplx_core::process::ProcessIdentity,
    session_id: String,
    transcript_path: Option<PathBuf>,
}
fn receipt_bytes(receipt: &Receipt) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(receipt).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_RECEIPT_BYTES {
        return Err(format!(
            "provider receipt exceeds {MAX_RECEIPT_BYTES} bytes; exact process identity retained, receipt not written"
        ));
    }
    Ok(bytes)
}
fn record(state: &Path, id: &str) -> Result<TaskRecord, String> {
    multplx_core::identifiers::TaskId::parse(id).map_err(|e| e.to_string())?;
    let bytes = multplx_core::filesystem::read_bounded_regular(
        state.join(format!("{id}.meta")),
        multplx_domain::lifecycle::subagent_model::MAX_TASK_METADATA_BYTES,
    )
    .map_err(|e| e.to_string())?;
    multplx_domain::lifecycle::subagent_model::read_meta(
        id,
        std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
    )
}
fn checkout(state: &Path, id: &str) -> Result<PathBuf, String> {
    let bytes = multplx_core::filesystem::read_bounded_regular(
        state.join(format!("{id}.meta")),
        multplx_domain::lifecycle::subagent_model::MAX_TASK_METADATA_BYTES,
    )
    .map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let mut paths = text
        .lines()
        .filter_map(|line| line.strip_prefix("worktree="));
    let path = PathBuf::from(paths.next().ok_or("task checkout missing")?);
    if !path.is_absolute() || paths.next().is_some() {
        return Err("task checkout ambiguous".into());
    }
    Ok(path)
}
fn harness(state: &Path, id: &str) -> Result<String, String> {
    let bytes = multplx_core::filesystem::read_bounded_regular(
        state.join(format!("{id}.meta")),
        multplx_domain::lifecycle::subagent_model::MAX_TASK_METADATA_BYTES,
    )
    .map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let mut values = text
        .lines()
        .filter_map(|line| line.strip_prefix("harness="));
    let value = values.next().ok_or("task harness missing")?;
    if values.next().is_some() {
        return Err("task harness ambiguous".into());
    }
    Ok(value.into())
}
fn receipt_path(state: &Path, id: &str, attempt: &Attempt) -> PathBuf {
    let identity = serde_json::to_string(attempt).expect("attempt identity");
    state.join(format!(
        "{id}.provider-session-{}.json",
        multplx_domain::maintainer_override::sha256_text(&identity)
    ))
}
fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn transcript(path: &Path, session: &str, cwd: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    if !path.is_absolute() {
        return Err("transcript_path must be absolute".into());
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.uid() != rustix::process::geteuid().as_raw() {
        return Err("transcript_path must be an owned regular file".into());
    }
    let mut first = Vec::new();
    use std::io::Read;
    BufReader::new(file)
        .take(64 * 1024)
        .read_until(b'\n', &mut first)
        .map_err(|e| e.to_string())?;
    if first.len() >= 64 * 1024 || first.last() != Some(&b'\n') {
        return Err("transcript header missing or too large".into());
    }
    let header: serde_json::Value = serde_json::from_slice(&first).map_err(|e| e.to_string())?;
    if header["type"] != "session_meta" || header["payload"]["id"].as_str() != Some(session) {
        return Err("transcript session UUID differs from native hook".into());
    }
    let actual = header["payload"]["cwd"]
        .as_str()
        .ok_or("transcript header cwd missing")?;
    if fs::canonicalize(actual).map_err(|e| e.to_string())?
        != fs::canonicalize(cwd).map_err(|e| e.to_string())?
    {
        return Err("transcript cwd differs from task checkout".into());
    }
    Ok(path.to_owned())
}
fn register(
    state: &Path,
    id: &str,
    payload: &serde_json::Value,
    expected: (&str, u64, u64),
    provider_pid: u32,
) -> Result<(), String> {
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        state.join(format!(".{id}.identity.lock")),
        &SystemProcessProbe::default(),
        Duration::from_secs(2),
    )
    .map_err(|e| e.to_string())?;
    let current = record(state, id)?;
    if harness(state, id)? != "codex" {
        return Err("task runtime provider is not Codex".into());
    }
    let attempt = current.attempt.as_ref().ok_or("task attempt missing")?;
    if attempt.id != expected.0
        || attempt.generation != expected.1
        || attempt.brief_revision != expected.2
        || current.accepted_brief_revision != Some(attempt.brief_revision)
    {
        return Err("SessionStart task attempt or brief was superseded".into());
    }
    if fs::canonicalize(current.owner_state.as_deref().ok_or("task owner missing")?)
        .map_err(|e| e.to_string())?
        != fs::canonicalize(state).map_err(|e| e.to_string())?
    {
        return Err("SessionStart owner state differs".into());
    }
    let session = payload["session_id"]
        .as_str()
        .filter(|s| uuid(s))
        .ok_or("native SessionStart session_id must be an exact UUID")?;
    let checkout = checkout(state, id)?;
    let hook_cwd = payload["cwd"]
        .as_str()
        .ok_or("native SessionStart cwd missing")?;
    if fs::canonicalize(hook_cwd).map_err(|e| e.to_string())?
        != fs::canonicalize(&checkout).map_err(|e| e.to_string())?
    {
        return Err("native SessionStart cwd differs from task checkout".into());
    }
    if payload
        .get("transcript_path")
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err("native SessionStart transcript_path must be a string or null".into());
    }
    let path = payload
        .get("transcript_path")
        .and_then(serde_json::Value::as_str)
        .map(|path| transcript(Path::new(path), session, &checkout))
        .transpose()?;
    let mut receipt = Receipt {
        task_id: id.into(),
        owner_state: fs::canonicalize(state).map_err(|e| e.to_string())?,
        attempt: attempt.clone(),
        provider: "codex".into(),
        backend: current.runtime.provider.clone(),
        provider_owner: SystemProcessProbe::default()
            .identity(provider_pid)
            .map_err(|e| e.to_string())?,
        session_id: session.into(),
        transcript_path: path,
    };
    let destination = receipt_path(state, id, attempt);
    if destination.exists() {
        let bytes = multplx_core::filesystem::read_bounded_regular(&destination, MAX_RECEIPT_BYTES)
            .map_err(|e| e.to_string())?;
        let old: Receipt = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if old.task_id != receipt.task_id
            || old.owner_state != receipt.owner_state
            || old.provider != receipt.provider
            || old.backend != receipt.backend
            || old.attempt != receipt.attempt
            || old.provider_owner != receipt.provider_owner
            || old.session_id != receipt.session_id
        {
            return Err("conflicting provider receipt already binds this exact task attempt; reconcile before replacing".into());
        }
        if let Some(previous) = old.transcript_path {
            if receipt
                .transcript_path
                .as_ref()
                .is_some_and(|path| path != &previous)
            {
                return Err("conflicting transcript path already binds this exact session".into());
            }
            receipt.transcript_path = Some(previous);
        }
    }
    multplx_core::filesystem::atomic_replace(destination, &receipt_bytes(&receipt)?, 0o600)
        .map_err(|e| e.to_string())
}
/// Called only by the existing managed native SessionStart observer.
pub(super) fn observe(args: &[String], payload: &str) -> Result<(), String> {
    if args != ["--provider", "codex", "--event", "reconcile"] {
        return Ok(());
    }
    let Ok(id) = std::env::var("MX_TASK_ID") else {
        return Ok(());
    };
    let Ok(attempt) = std::env::var("MX_ATTEMPT_ID") else {
        return Ok(());
    };
    let generation = std::env::var("MX_ATTEMPT_GENERATION")
        .map_err(|e| e.to_string())?
        .parse()
        .map_err(|_| "invalid attempt generation")?;
    let revision = std::env::var("MX_BRIEF_REVISION")
        .map_err(|e| e.to_string())?
        .parse()
        .map_err(|_| "invalid brief revision")?;
    let state = std::env::var_os("MX_REPORT_STATE_OVERRIDE")
        .map(PathBuf::from)
        .ok_or("SessionStart owner state missing")?;
    let probe = SystemProcessProbe::default();
    let mut pid = std::process::id();
    let mut owned = false;
    for _ in 0..12 {
        let row = probe.ancestry_row(pid).map_err(|e| e.to_string())?;
        if super::harness_process_row_matches(
            &row,
            "codex",
            super::harness_executable("codex").as_deref(),
        ) {
            owned = true;
            break;
        }
        if row.parent_pid <= 1 {
            break;
        }
        pid = row.parent_pid;
    }
    if !owned {
        return Err("SessionStart has no owning native Codex process".into());
    }
    let current = record(&state, &id)?;
    let tmp = multplx_domain::lifecycle::spawn::task_temp_path_for_record(&current)?;
    let bytes =
        multplx_core::filesystem::read_bounded_regular(tmp.join("launch-started"), 16 * 1024)
            .map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let mut lines = text.lines();
    let launch: serde_json::Value =
        serde_json::from_str(lines.next().ok_or("launch receipt missing")?)
            .map_err(|e| e.to_string())?;
    if lines.next().and_then(|text| text.parse::<u32>().ok()) != Some(pid)
        || lines.next().is_some()
        || launch["task"].as_str() != Some(&id)
        || launch["attempt"] != serde_json::to_value(&current.attempt).map_err(|e| e.to_string())?
        || launch["endpoint"].as_str() != current.runtime.endpoint.as_deref()
        || launch["brief_revision"].as_u64() != current.accepted_brief_revision
    {
        return Err(
            "native SessionStart does not belong to this task's exact launched provider process"
                .into(),
        );
    }
    register(
        &state,
        &id,
        &serde_json::from_str(payload).map_err(|e| e.to_string())?,
        (&attempt, generation, revision),
        pid,
    )
}
fn history(state: &Path, id: &str) -> Result<serde_json::Value, String> {
    let current = record(state, id).ok();
    let owner = fs::canonicalize(state).map_err(|e| e.to_string())?;
    let mut receipts = Vec::new();
    let mut errors = Vec::new();
    for entry in fs::read_dir(state).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(&format!("{id}.provider-session-")) || !name.ends_with(".json") {
            continue;
        }
        let loaded = (|| -> Result<Receipt, String> {
            let bytes = multplx_core::filesystem::read_bounded_regular(&path, MAX_RECEIPT_BYTES)
                .map_err(|e| e.to_string())?;
            let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if receipt.task_id != id
                || receipt.owner_state != owner
                || receipt_path(state, id, &receipt.attempt) != path
            {
                return Err("historical provider receipt has conflicting identity".into());
            }
            Ok(receipt)
        })();
        let receipt = match loaded {
            Ok(receipt) => receipt,
            Err(reason) => {
                errors.push(serde_json::json!({"retained_path":path,"reason":reason}));
                continue;
            }
        };
        let bound = current.as_ref().is_some_and(|task| {
            task.attempt.as_ref() == Some(&receipt.attempt)
                && task.runtime.endpoint.is_some()
                && task.runtime.provider == receipt.backend
        });
        receipts.push(serde_json::json!({"provider_session":receipt,"current_attempt_bound":bound,"binding":if bound {"current-task-attempt"} else {"retained-historical-or-retired"},"live_execution_proven":false,"resumability_proven":false}));
    }
    receipts.sort_by_key(|value| value["provider_session"]["attempt"]["generation"].as_u64());
    Ok(
        serde_json::json!({"task_id":id,"retained_sessions":receipts,"complete":errors.is_empty(),"invalid_retained_sessions":errors}),
    )
}
pub(super) fn run(args: &[OsString], state: &Path) -> i32 {
    if args.len() == 1 && matches!(args[0].to_str(), Some("--help" | "-h" | "help")) {
        print!("{HELP}");
        return 0;
    }
    let outcome = (|| -> Result<serde_json::Value, String> {
        if args.len() == 2 && args[0] == "history" {
            let id = args[1].to_str().ok_or("task id is not UTF-8")?;
            multplx_core::identifiers::TaskId::parse(id).map_err(|e| e.to_string())?;
            return history(state, id);
        }
        if args.len() != 2 || args[0] != "inspect" {
            return Err(HELP.into());
        }
        let id = args[1].to_str().ok_or("task id is not UTF-8")?;
        let current = record(state, id)?;
        let Some(attempt) = current.attempt.as_ref() else {
            return Err("task attempt missing".into());
        };
        let path = receipt_path(state, id, attempt);
        if !path.exists() {
            return Ok(
                serde_json::json!({"task_id":id,"provider_session":null,"reason":"no exact managed SessionStart receipt"}),
            );
        }
        let bytes = multplx_core::filesystem::read_bounded_regular(&path, MAX_RECEIPT_BYTES)
            .map_err(|e| e.to_string())?;
        let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if receipt.task_id != id
            || current.attempt.as_ref() != Some(&receipt.attempt)
            || receipt.owner_state != fs::canonicalize(state).map_err(|e| e.to_string())?
        {
            return Err(
                "provider session receipt belongs to a superseded task attempt or owner".into(),
            );
        }
        let available = receipt.transcript_path.as_ref().is_some_and(|path| {
            checkout(state, id).is_ok_and(|cwd| transcript(path, &receipt.session_id, &cwd).is_ok())
        });
        Ok(
            serde_json::json!({"task_id":id,"provider_session":receipt,"transcript_available":available,"model_acceptance_proven":false,"resumability_proven":false,"binding":if current.runtime.endpoint.is_some() && current.runtime.provider == receipt.backend {"current-task-attempt"} else {"retained-after-retirement"},"retained_receipt":path}),
        )
    })();
    match outcome {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
            0
        }
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multplx_domain::lifecycle::subagent_model::{ArtifactKind, AssignmentRole, write_meta};
    const SESSION: &str = "12345678-1234-1234-1234-123456789abc";
    fn fixture() -> (tempfile::TempDir, PathBuf, TaskRecord) {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        fs::create_dir(&state).unwrap();
        let mut task = TaskRecord::new(
            "worker".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            "primary".into(),
            "primary".into(),
            root.path().to_str().unwrap().into(),
        );
        task.briefs[0].scope = "fixture".into();
        task.runtime.provider = "herdr".into();
        let meta = write_meta(
            &format!("harness=codex\nworktree={}\n", root.path().display()),
            &task,
        )
        .unwrap();
        fs::write(state.join("worker.meta"), meta).unwrap();
        (root, state, task)
    }
    #[test]
    fn task_session_registers_actual_large_process_command_identity() {
        let (root, state, task) = fixture();
        let ready = root.path().join("process-ready");
        let mut command = std::process::Command::new("python3");
        command.args([
            "-c",
            "import pathlib, sys, time; pathlib.Path(sys.argv[2]).touch(); time.sleep(60)",
            &"provider-argv".repeat(2000),
            ready.to_str().unwrap(),
        ]);
        let child = multplx_core::process::OwnedChild::spawn(&mut command).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !ready.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists());
        let identity = SystemProcessProbe::default().identity(child.id()).unwrap();
        assert!(identity.marker.len() > 16 * 1024);
        let attempt = task.attempt.as_ref().unwrap();
        register(
            &state,
            "worker",
            &serde_json::json!({"cwd":root.path(),"session_id":SESSION}),
            (&attempt.id, attempt.generation, attempt.brief_revision),
            child.id(),
        )
        .unwrap();
        let path = receipt_path(&state, "worker", attempt);
        let bytes =
            multplx_core::filesystem::read_bounded_regular(&path, MAX_RECEIPT_BYTES).unwrap();
        assert!(bytes.len() > 16 * 1024);
        let receipt: Receipt = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(receipt.provider_owner, identity);
        assert_eq!(history(&state, "worker").unwrap()["complete"], true);
        // OwnedChild reaps only this synthetic process, including on failures.
    }

    #[test]
    fn task_session_large_markers_share_write_read_bound_and_history_retains_invalid_paths() {
        let (root, state, task) = fixture();
        let attempt = task.attempt.as_ref().unwrap();
        register(
            &state,
            "worker",
            &serde_json::json!({"cwd":root.path(),"session_id":SESSION}),
            (&attempt.id, attempt.generation, attempt.brief_revision),
            std::process::id(),
        )
        .unwrap();
        let path = receipt_path(&state, "worker", attempt);
        let mut receipt: Receipt = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        receipt.provider_owner.marker =
            format!("exact-start-time: codex {}", "a".repeat(32 * 1024));
        let bytes = receipt_bytes(&receipt).unwrap();
        assert!(bytes.len() > 16 * 1024);
        fs::write(&path, &bytes).unwrap();
        let read =
            multplx_core::filesystem::read_bounded_regular(&path, MAX_RECEIPT_BYTES).unwrap();
        assert_eq!(
            serde_json::from_slice::<Receipt>(&read)
                .unwrap()
                .provider_owner,
            receipt.provider_owner
        );
        let valid = history(&state, "worker").unwrap();
        assert_eq!(valid["complete"], true);
        assert_eq!(valid["retained_sessions"].as_array().unwrap().len(), 1);
        let malformed = state.join("worker.provider-session-malformed.json");
        fs::write(&malformed, b"broken").unwrap();
        let partial = history(&state, "worker").unwrap();
        assert_eq!(partial["complete"], false);
        assert_eq!(partial["retained_sessions"].as_array().unwrap().len(), 1);
        assert_eq!(
            partial["invalid_retained_sessions"][0]["retained_path"],
            serde_json::to_value(&malformed).unwrap()
        );
        receipt.provider_owner.marker.clear();
        let overhead = receipt_bytes(&receipt).unwrap().len();
        receipt.provider_owner.marker = "x".repeat(MAX_RECEIPT_BYTES - overhead);
        let boundary = receipt_bytes(&receipt).unwrap();
        assert_eq!(boundary.len(), MAX_RECEIPT_BYTES);
        let boundary_path = state.join("boundary.json");
        fs::write(&boundary_path, &boundary).unwrap();
        assert_eq!(
            multplx_core::filesystem::read_bounded_regular(&boundary_path, MAX_RECEIPT_BYTES)
                .unwrap()
                .len(),
            MAX_RECEIPT_BYTES
        );
        receipt.provider_owner.marker.push('x');
        assert!(receipt_bytes(&receipt).is_err());
        receipt.provider_owner.marker = "x".repeat(MAX_RECEIPT_BYTES);
        assert!(receipt_bytes(&receipt).unwrap_err().contains("exceeds"));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        receipt.provider_owner.marker = "owner-marker".into();
        receipt.owner_state = root.path().join("foreign");
        fs::write(&path, receipt_bytes(&receipt).unwrap()).unwrap();
        assert_eq!(
            history(&state, "worker").unwrap()["retained_sessions"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn task_session_registration_rejects_stale_attempt_and_preserves_historical_uuid() {
        let (root, state, mut task) = fixture();
        let attempt = task.attempt.clone().unwrap();
        let expected = (
            attempt.id.as_str(),
            attempt.generation,
            attempt.brief_revision,
        );
        let payload = serde_json::json!({"cwd":root.path(),"session_id":SESSION});
        register(&state, "worker", &payload, expected, std::process::id()).unwrap();
        let original = receipt_path(&state, "worker", &attempt);
        assert!(original.exists());
        assert!(
            register(
                &state,
                "worker",
                &payload,
                ("old-attempt", 1, 1),
                std::process::id()
            )
            .is_err()
        );
        assert!(
            register(
                &state,
                "worker",
                &serde_json::json!({"cwd":root.path(),"session_id":"22345678-1234-1234-1234-123456789abc"}),
                expected, std::process::id()
            )
            .is_err()
        );
        assert!(
            register(
                &state,
                "worker",
                &serde_json::json!({"cwd":state,"session_id":SESSION}),
                expected,
                std::process::id()
            )
            .is_err()
        );
        let before = fs::read(&original).unwrap();
        task.replace_attempt(&attempt.id, true).unwrap();
        let meta = write_meta(
            &format!("harness=codex\nworktree={}\n", root.path().display()),
            &task,
        )
        .unwrap();
        fs::write(state.join("worker.meta"), meta).unwrap();
        assert!(register(&state, "worker", &payload, expected, std::process::id()).is_err());
        let next = task.attempt.as_ref().unwrap();
        register(
            &state,
            "worker",
            &payload,
            (&next.id, next.generation, next.brief_revision),
            std::process::id(),
        )
        .unwrap();
        assert_eq!(fs::read(original).unwrap(), before);
        assert!(receipt_path(&state, "worker", next).exists());
    }
    #[test]
    fn task_session_transcript_header_is_exact_and_never_follows_symlinks() {
        let (root, state, task) = fixture();
        let path = root.path().join("rollout.jsonl");
        let header = |session: &str, cwd: &Path| {
            format!(
                "{}\n",
                serde_json::json!({"type":"session_meta","payload":{"id":session,"cwd":cwd}})
            )
        };
        fs::write(&path, header(SESSION, root.path())).unwrap();
        assert_eq!(transcript(&path, SESSION, root.path()).unwrap(), path);
        assert!(transcript(&path, "22345678-1234-1234-1234-123456789abc", root.path()).is_err());
        assert!(transcript(&path, SESSION, &state).is_err());
        let alias = root.path().join("alias.jsonl");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(transcript(&alias, SESSION, root.path()).is_err());
        let attempt = task.attempt.unwrap();
        register(
            &state,
            "worker",
            &serde_json::json!({"cwd":root.path(),"session_id":SESSION,"transcript_path":path}),
            (&attempt.id, attempt.generation, attempt.brief_revision),
            std::process::id(),
        )
        .unwrap();
        register(
            &state,
            "worker",
            &serde_json::json!({"cwd":root.path(),"session_id":SESSION}),
            (&attempt.id, attempt.generation, attempt.brief_revision),
            std::process::id(),
        )
        .unwrap();
        let retained: Receipt =
            serde_json::from_slice(&fs::read(receipt_path(&state, "worker", &attempt)).unwrap())
                .unwrap();
        assert_eq!(retained.transcript_path, Some(path.clone()));
        assert!(
            register(
                &state,
                "worker",
                &serde_json::json!({"cwd":root.path(),"session_id":SESSION,"transcript_path":7}),
                (&attempt.id, attempt.generation, attempt.brief_revision),
                std::process::id()
            )
            .is_err()
        );
        fs::write(&path, header("different", root.path())).unwrap();
        assert!(transcript(&path, SESSION, root.path()).is_err());
    }
}

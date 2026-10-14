//! Administrative quarantine of exact, unsubmitted successors of retired homes.
//! This owner never resumes a launch or changes task/worktree completion truth.
use multplx_core::{
    filesystem::{atomic_replace, read_bounded_regular},
    identifiers::TaskId,
    locks::DirectoryLock,
    process::SystemProcessProbe,
    session_lock::MaintenanceGuard,
};
use multplx_domain::lifecycle::{
    home_seed, spawn, subagent_model, upgrade_cleanup::private_directory,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Proof {
    task: String,
    intent: Vec<u8>,
    snapshots: BTreeMap<PathBuf, Vec<u8>>,
    archive: PathBuf,
    archive_identity: (u64, u64),
}
fn owned(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.nlink() != 1
        || meta.mode() & 0o022 != 0
    {
        return Err(format!(
            "unsafe retired-intent evidence {}; retained",
            path.display()
        ));
    }
    read_bounded_regular(path, 16 * 1024 * 1024).map_err(|e| e.to_string())
}
fn absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(format!(
            "retired task path exists or is uncertain {}; retained",
            path.display()
        )),
    }
}

/// Read-only proof. Absence of metadata alone never authorizes quarantine.
pub(crate) fn inspect(root: &Path, home: &Path, task: &str) -> Result<Proof, String> {
    TaskId::parse(task).map_err(|e| e.to_string())?;
    for dir in [
        root.to_owned(),
        home.to_owned(),
        home.join("state"),
        home.join("data"),
    ] {
        private_directory(&dir, false)?;
    }
    if fs::canonicalize(root).map_err(|e| e.to_string())? != root
        || fs::canonicalize(home).map_err(|e| e.to_string())? != home
    {
        return Err("retired-intent owner paths must be canonical; retained".into());
    }
    let state = home.join("state");
    absent(&state.join(format!("{task}.meta")))?;
    absent(&state.join(format!(".teardown.transaction.{task}")))?;
    let intent = owned(&state.join(format!(".spawn-{task}.intent")))?;
    let reserved: subagent_model::TaskRecord =
        serde_json::from_slice(&intent).map_err(|e| e.to_string())?;
    reserved.validate()?;
    if reserved.task_id != task
        || !reserved.private_home
        || !reserved.persistent
        || reserved.owner_home.as_deref() != home.to_str()
        || reserved.owner_state.as_deref() != state.to_str()
        || reserved.parent_home.as_deref() != home.to_str()
        || reserved.parent_state.as_deref() != state.to_str()
        || reserved.root_id.as_deref() != Some(&format!("root-home:{}", home.display()))
    {
        return Err("retired intent is not an exact root-owned standing task; retained".into());
    }
    let lease_path = home.join(format!("data/.home-allocation-{task}.json"));
    let lease_bytes = owned(&lease_path)?;
    let allocation: home_seed::HomeAllocation =
        serde_json::from_slice(&lease_bytes).map_err(|e| e.to_string())?;
    if allocation.binding
        != *reserved
            .home_allocation
            .as_ref()
            .ok_or("retired intent lacks home lease")?
        || allocation.runtime_root != root
        || allocation.binding.owner_home != home
        || allocation.state != "retired"
        || allocation.git_allocation.is_some()
        || reserved.persistent_home.as_deref() != allocation.binding.path.to_str()
    {
        return Err("retired intent lease/runtime identity differs; retained".into());
    }
    absent(&allocation.binding.path)?;
    home_seed::verify_retired_home(&allocation)?;
    let archive = allocation
        .retained_path
        .clone()
        .ok_or("retired archive missing")?;
    private_directory(&archive, false)?;
    private_directory(&archive.join("state"), false)?;
    crate::launcher::verify_cleanup_home_quiescent(root, home, &archive, task)?;
    let archive_meta = fs::symlink_metadata(&archive).map_err(|e| e.to_string())?;
    let mut snapshots = BTreeMap::from([(lease_path, lease_bytes)]);
    let directory = state.join(".spawn-actions");
    private_directory(&directory, false)?;
    let context = spawn::Context {
        root: root.into(),
        home: home.into(),
        state: state.clone(),
        data: home.join("data"),
        projects: home.join("projects"),
    };
    let mut actions = Vec::new();
    for (index, entry) in fs::read_dir(&directory)
        .map_err(|e| e.to_string())?
        .enumerate()
    {
        if index >= 4096 {
            return Err("spawn action evidence exceeds bounded scan; retained".into());
        }
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            return Err("unknown spawn action entry; retained".into());
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("invalid action filename")?;
        let bytes = owned(&path)?;
        let action = spawn::read_action(&context, id)?.ok_or("spawn action disappeared")?;
        if serde_json::from_slice::<spawn::LaunchAction>(&bytes).map_err(|e| e.to_string())?
            != action
        {
            return Err("spawn action changed during preview; retained".into());
        }
        if action.task_id == task {
            actions.push(action);
        }
        snapshots.insert(path, bytes);
    }
    let previous = reserved
        .prior_attempts
        .last()
        .ok_or("retired intent predecessor missing")?;
    let predecessors: Vec<_> = actions
        .iter()
        .filter(|a| {
            a.stage == spawn::LaunchStage::Running && a.binding.attempt.as_ref() == Some(previous)
        })
        .collect();
    if predecessors.len() != 1 {
        return Err("retired intent lacks unique Running predecessor; retained".into());
    }
    let predecessor = predecessors[0];
    if !spawn::inspect_retired_launch(&context, predecessor)? {
        return Err("retired predecessor lease proof differs; retained".into());
    }
    let mut current = predecessor.binding.clone();
    if current.runtime.provider != predecessor.backend
        || current
            .runtime
            .endpoint
            .as_ref()
            .is_some_and(|endpoint| Some(endpoint) != predecessor.endpoint.as_ref())
        || predecessor.endpoint.is_none()
        || current.allocation.as_ref().map(|a| Path::new(&a.path))
            != predecessor.worktree.as_deref()
    {
        return Err("retired predecessor execution receipt differs; retained".into());
    }
    // Running action bindings freeze the prelaunch record; endpoint publication
    // is separately authoritative on the exact same immutable action receipt.
    current.runtime.endpoint = predecessor.endpoint.clone();
    if spawn::inspect_unsubmitted_replacement_intent(&context, &current, &actions)?.as_deref()
        != Some(intent.as_slice())
    {
        return Err("retired successor proof differs; retained".into());
    }
    // A reserved successor has no endpoint or allocation. Its prior execution
    // still requires backend ownership reconciliation, performed by the CLI.
    Ok(Proof {
        task: task.into(),
        intent,
        snapshots,
        archive,
        archive_identity: (archive_meta.dev(), archive_meta.ino()),
    })
}

pub(crate) fn archive(proof: &Proof) -> &Path {
    &proof.archive
}
pub(crate) fn snapshots(proof: &Proof) -> &BTreeMap<PathBuf, Vec<u8>> {
    &proof.snapshots
}

/// Called only under root maintenance and exact task/action lifecycle locks.
pub(crate) fn quarantine(root: &Path, home: &Path, approved: &Proof) -> Result<(), String> {
    quarantine_inner(root, home, approved, |_| Ok(()))
}

fn quarantine_inner(
    root: &Path,
    home: &Path,
    approved: &Proof,
    mut checkpoint: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let state = home.join("state");
    let intent_path = state.join(format!(".spawn-{}.intent", approved.task));
    let bytes = serde_json::to_vec(approved).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("retired intent proof exceeds retry-readable budget; retained".into());
    }
    let directory = state.join(".retired-spawn-intents");
    private_directory(&directory, true)?;
    let audit = directory.join(format!("{}-{:x}", approved.task, Sha256::digest(&bytes)));
    private_directory(&audit, true)?;
    if fs::symlink_metadata(&intent_path).is_err() {
        absent(&intent_path)?;
        if owned(&audit.join("proof.json"))? != bytes {
            return Err("retired intent recovery proof differs; retained".into());
        }
        // An interrupted unlink is recoverable only while retirement and every
        // launch receipt remain identical, with no new task or active home.
        recheck_without_intent(root, home, approved)?;
    } else {
        if inspect(root, home, &approved.task)? != *approved {
            return Err("retired intent changed after preview; retained".into());
        }
        if audit.join("proof.json").exists() && owned(&audit.join("proof.json"))? != bytes {
            return Err("retired intent archive conflict; retained".into());
        }
        atomic_replace(audit.join("proof.json"), &bytes, 0o600).map_err(|e| e.to_string())?;
        checkpoint("backup")?;
        // Evidence is durable before the only removed artifact. Task records,
        // archived homes, actions, worktrees and completion truth stay intact.
        if owned(&intent_path)? != approved.intent {
            return Err("retired intent changed before quarantine; retained".into());
        }
        fs::remove_file(&intent_path).map_err(|e| e.to_string())?;
        fs::File::open(&state)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
        checkpoint("unlink")?;
    }
    atomic_replace(audit.join("complete"), b"quarantined\n", 0o600).map_err(|e| e.to_string())?;
    eprintln!(
        "Quarantined retired spawn intent {}; retained exact evidence: {}",
        approved.task,
        audit.display()
    );
    Ok(())
}
fn recheck_without_intent(root: &Path, home: &Path, proof: &Proof) -> Result<(), String> {
    absent(&home.join(format!("state/{}.meta", proof.task)))?;
    absent(&home.join(format!("state/.teardown.transaction.{}", proof.task)))?;
    for (path, bytes) in &proof.snapshots {
        if owned(path)? != *bytes {
            return Err("retired intent recovery evidence changed; retained".into());
        }
    }
    let allocation = home_seed::read_home_allocation(&home.join("data"), &proof.task)?
        .ok_or("retirement allocation missing")?;
    if allocation.state != "retired"
        || allocation.runtime_root != root
        || allocation.binding.owner_home != home
        || allocation.retained_path.as_ref() != Some(&proof.archive)
    {
        return Err("retirement changed; retained".into());
    }
    absent(&allocation.binding.path)?;
    home_seed::verify_retired_home(&allocation)?;
    let meta = fs::symlink_metadata(&proof.archive).map_err(|e| e.to_string())?;
    if (meta.dev(), meta.ino()) != proof.archive_identity {
        return Err("retired archive identity changed; retained".into());
    }
    let actual: std::collections::BTreeSet<_> = fs::read_dir(home.join("state/.spawn-actions"))
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.path()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let expected: std::collections::BTreeSet<_> = proof
        .snapshots
        .keys()
        .filter(|p| p.parent() == Some(home.join("state/.spawn-actions").as_path()))
        .cloned()
        .collect();
    if actual != expected {
        return Err("new launch evidence appeared; retained".into());
    }
    Ok(())
}

pub(crate) fn run(args: &[OsString]) -> i32 {
    match run_inner(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("mx upgrade-cleanup: {e}");
            2
        }
    }
}
fn run_inner(args: &[OsString]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: mx upgrade-cleanup retired-intent inspect|apply --root PATH --home PATH --task ID\nAdministrative recovery of an exact unsubmitted successor of a verified retired private home.\nInspect is read-only. Apply quarantines only the intent, preserving task/action/home/worktree evidence.\nApply refuses live or uncertain runtime users and requires lifecycle maintenance exclusion; it never starts or stops sessions."
        );
        return Ok(());
    }
    if args.len() != 8
        || args[0] != "retired-intent"
        || !matches!(args[1].to_str(), Some("inspect" | "apply"))
    {
        return Err("use --help for the exact administrative recovery form".into());
    }
    let mut root = None;
    let mut home = None;
    let mut task_id = None;
    for pair in args[2..].chunks_exact(2) {
        match pair[0].to_str() {
            Some("--root") if root.is_none() => root = Some(PathBuf::from(&pair[1])),
            Some("--home") if home.is_none() => home = Some(PathBuf::from(&pair[1])),
            Some("--task") if task_id.is_none() => task_id = pair[1].to_str().map(str::to_owned),
            _ => return Err("invalid or duplicate recovery option".into()),
        }
    }
    let (root, home, task_id) = (
        root.ok_or("--root required")?,
        home.ok_or("--home required")?,
        task_id.ok_or("--task required")?,
    );
    TaskId::parse(&task_id).map_err(|e| e.to_string())?;
    if args[1] == "inspect" {
        let proof = inspect(&root, &home, &task_id)?;
        crate::launcher::verify_retired_intent_users(&root, &home, &proof.archive, &task_id)?;
        println!(
            "{}",
            serde_json::to_string(&proof).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let processes = SystemProcessProbe::default();
    let state = home.join("state");
    let _guard = MaintenanceGuard::acquire(&state, &processes).map_err(|e| e.to_string())?;
    let mut locks = Vec::new();
    for name in [
        ".watch.lock".to_owned(),
        ".workspace-launch.lock".to_owned(),
        format!(".teardown.{task_id}.lock"),
        format!(".{task_id}.identity.lock"),
        format!(".spawn-{task_id}.lock"),
    ] {
        locks.push(
            DirectoryLock::try_acquire(state.join(name), &processes).map_err(|e| e.to_string())?,
        );
    }
    let proof = if state.join(format!(".spawn-{task_id}.intent")).exists() {
        inspect(&root, &home, &task_id)?
    } else {
        let directory = state.join(".retired-spawn-intents");
        private_directory(&directory, false)?;
        let mut matches = Vec::new();
        for (index, entry) in fs::read_dir(&directory)
            .map_err(|e| e.to_string())?
            .enumerate()
        {
            if index >= 4096 {
                return Err("recovery archives exceed bounded scan; retained".into());
            }
            let path = entry.map_err(|e| e.to_string())?.path();
            if path
                .file_name()
                .and_then(|p| p.to_str())
                .is_some_and(|p| p.starts_with(&format!("{task_id}-")))
            {
                private_directory(&path, false)?;
                let proof: Proof = serde_json::from_slice(&owned(&path.join("proof.json"))?)
                    .map_err(|e| e.to_string())?;
                let digest = format!(
                    "{}-{:x}",
                    proof.task,
                    Sha256::digest(serde_json::to_vec(&proof).map_err(|e| e.to_string())?)
                );
                if path.file_name().and_then(|p| p.to_str()) != Some(&digest) {
                    return Err("retired intent recovery archive digest differs; retained".into());
                }
                if proof.task != task_id {
                    return Err("archive task identity differs".into());
                }
                matches.push(proof);
            }
        }
        if matches.len() != 1 {
            return Err("missing or ambiguous retired intent recovery archive; retained".into());
        }
        matches.remove(0)
    };
    for path in proof
        .snapshots
        .keys()
        .filter(|p| p.parent() == Some(state.join(".spawn-actions").as_path()))
    {
        let request = path
            .file_stem()
            .and_then(|p| p.to_str())
            .ok_or("invalid action id")?;
        locks.push(
            DirectoryLock::try_acquire(
                state.join(format!(".spawn-action-{request}.lock")),
                &processes,
            )
            .map_err(|e| e.to_string())?,
        );
    }
    let _child =
        MaintenanceGuard::acquire_anchored(&proof.archive.join("state"), &state, &processes)
            .map_err(|e| e.to_string())?;
    for name in [".watch.lock", ".workspace-launch.lock"] {
        locks.push(
            DirectoryLock::try_acquire_anchored(
                &proof.archive.join("state").join(name),
                &state,
                &processes,
            )
            .map_err(|e| e.to_string())?,
        );
    }
    crate::launcher::verify_retired_intent_users(&root, &home, &proof.archive, &task_id)?;
    quarantine(&root, &home, &proof)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use subagent_model::{ArtifactKind, AssignmentRole, TaskRecord};
    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        home: PathBuf,
        archive: PathBuf,
        action: spawn::LaunchAction,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let base = fs::canonicalize(temp.path()).unwrap();
            let root = base.join("runtime");
            let home = base.join("home");
            let archive = base.join("archive");
            for p in [
                &root,
                &home.join("state/.spawn-actions"),
                &home.join("data"),
                &archive.join("state"),
                &archive.join("data"),
            ] {
                fs::create_dir_all(p).unwrap();
            }
            fs::write(archive.join(multplx_core::agent_home::MARKER), "task\n").unwrap();
            fs::write(archive.join("private.txt"), "preserve me").unwrap();
            let token = home_seed::HomeBinding {
                id: "task".into(),
                owner_home: home.clone(),
                path: base.join("old-home"),
                lease_id: "exact-lease".into(),
                generation: 1,
            };
            let owner = format!("root-home:{}", home.display());
            let mut record = TaskRecord::new(
                "task".into(),
                AssignmentRole::Implementer,
                ArtifactKind::Implementation,
                true,
                owner.clone(),
                owner,
                home.to_string_lossy().into(),
            );
            record.home_allocation = Some(token.clone());
            record.persistent_home = Some(token.path.to_string_lossy().into());
            record.runtime.provider = "tmux".into();
            record.runtime.endpoint = Some("never-existed:mx-task".into());
            let action = spawn::LaunchAction {
                version: 1,
                request_id: "old-launch".into(),
                task_id: "task".into(),
                backend: "tmux".into(),
                endpoint: record.runtime.endpoint.clone(),
                worktree: None,
                stage: spawn::LaunchStage::Running,
                detail: None,
                binding: record.clone(),
            };
            let previous = record.attempt.as_ref().unwrap().id.clone();
            record.replace_attempt(&previous, true).unwrap();
            let metadata = fs::metadata(&archive).unwrap();
            let allocation = home_seed::HomeAllocation {
                version: 1,
                binding: token,
                runtime_root: root.clone(),
                state: "retired".into(),
                retained_path: Some(archive.clone()),
                directory_identity: Some((metadata.dev(), metadata.ino())),
                git_allocation: None,
            };
            fs::write(
                home.join("data/.home-allocation-task.json"),
                serde_json::to_vec(&allocation).unwrap(),
            )
            .unwrap();
            fs::write(
                home.join("state/.spawn-task.intent"),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
            fs::write(
                home.join("state/.spawn-actions/old-launch.json"),
                serde_json::to_vec(&action).unwrap(),
            )
            .unwrap();
            Self {
                _temp: temp,
                root,
                home,
                archive,
                action,
            }
        }
        fn intent(&self) -> PathBuf {
            self.home.join("state/.spawn-task.intent")
        }
        fn edit_intent(&self, f: impl FnOnce(&mut TaskRecord)) {
            let mut record: TaskRecord =
                serde_json::from_slice(&fs::read(self.intent()).unwrap()).unwrap();
            f(&mut record);
            fs::write(self.intent(), serde_json::to_vec(&record).unwrap()).unwrap();
        }
    }
    #[test]
    fn exact_retired_successor_archives_bytes_without_launch_or_false_completion() {
        let f = Fixture::new();
        let bytes = fs::read(f.intent()).unwrap();
        let proof = inspect(&f.root, &f.home, "task").unwrap();
        assert_eq!(proof.intent, bytes);
        assert!(f.intent().exists());
        quarantine(&f.root, &f.home, &proof).unwrap();
        assert!(!f.intent().exists());
        quarantine(&f.root, &f.home, &proof).unwrap();
        assert_eq!(
            fs::read_to_string(f.archive.join("private.txt")).unwrap(),
            "preserve me"
        );
        assert!(!f.home.join("state/task.meta").exists());
        assert_eq!(
            fs::read(f.home.join("state/.spawn-actions/old-launch.json")).unwrap(),
            serde_json::to_vec(&f.action).unwrap()
        );
        let audits: Vec<_> = fs::read_dir(f.home.join("state/.retired-spawn-intents"))
            .unwrap()
            .collect();
        assert_eq!(audits.len(), 1);
        let saved: Proof = serde_json::from_slice(
            &fs::read(audits[0].as_ref().unwrap().path().join("proof.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(saved, proof);
    }
    #[test]
    fn frozen_prelaunch_binding_uses_exact_running_action_endpoint() {
        let f = Fixture::new();
        let mut action = f.action.clone();
        action.binding.runtime.endpoint = None;
        fs::write(
            f.home.join("state/.spawn-actions/old-launch.json"),
            serde_json::to_vec(&action).unwrap(),
        )
        .unwrap();
        inspect(&f.root, &f.home, "task").unwrap();
        action.endpoint = Some("different:mx-task".into());
        fs::write(
            f.home.join("state/.spawn-actions/old-launch.json"),
            serde_json::to_vec(&action).unwrap(),
        )
        .unwrap();
        assert!(inspect(&f.root, &f.home, "task").is_err());
        action.endpoint = f.action.endpoint.clone();
        action.binding.runtime.provider = "herdr".into();
        fs::write(
            f.home.join("state/.spawn-actions/old-launch.json"),
            serde_json::to_vec(&action).unwrap(),
        )
        .unwrap();
        assert!(inspect(&f.root, &f.home, "task").is_err());
        action.binding.runtime.provider = f.action.backend.clone();
        action.binding.runtime.endpoint = Some("conflicting:mx-task".into());
        fs::write(
            f.home.join("state/.spawn-actions/old-launch.json"),
            serde_json::to_vec(&action).unwrap(),
        )
        .unwrap();
        assert!(inspect(&f.root, &f.home, "task").is_err());
    }
    #[test]
    fn uncertain_or_active_identity_is_never_quarantined() {
        for case in [
            "current-meta",
            "meta-link",
            "home-reoccupied",
            "home-link",
            "archive-missing",
            "archive-replaced",
            "partial-retirement",
            "lease-mismatch",
            "unknown-action",
            "advanced-successor",
            "wrong-successor",
            "intent-link",
            "unfinished-journal",
            "intent-hardlink",
            "action-hardlink",
            "lease-hardlink",
        ] {
            let f = Fixture::new();
            let bytes = fs::read(f.intent()).unwrap();
            match case {
                "intent-hardlink" => {
                    fs::hard_link(f.intent(), f._temp.path().join("intent-link")).unwrap()
                }
                "action-hardlink" => fs::hard_link(
                    f.home.join("state/.spawn-actions/old-launch.json"),
                    f._temp.path().join("action-link"),
                )
                .unwrap(),
                "lease-hardlink" => fs::hard_link(
                    f.home.join("data/.home-allocation-task.json"),
                    f._temp.path().join("lease-link"),
                )
                .unwrap(),
                "current-meta" => fs::write(f.home.join("state/task.meta"), "current").unwrap(),
                "meta-link" => symlink("missing", f.home.join("state/task.meta")).unwrap(),
                "home-reoccupied" => fs::create_dir(f._temp.path().join("old-home")).unwrap(),
                "home-link" => symlink("missing", f._temp.path().join("old-home")).unwrap(),
                "archive-missing" => fs::remove_dir_all(&f.archive).unwrap(),
                "archive-replaced" => {
                    fs::rename(&f.archive, f._temp.path().join("saved")).unwrap();
                    fs::create_dir_all(f.archive.join("state")).unwrap();
                    fs::write(f.archive.join(multplx_core::agent_home::MARKER), "task\n").unwrap();
                }
                "partial-retirement" | "lease-mismatch" => {
                    let path = f.home.join("data/.home-allocation-task.json");
                    let mut a: home_seed::HomeAllocation =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    if case == "partial-retirement" {
                        a.state = "archiving".into();
                    } else {
                        a.binding.lease_id = "another".into();
                    }
                    fs::write(path, serde_json::to_vec(&a).unwrap()).unwrap();
                }
                "unknown-action" => {
                    fs::write(f.home.join("state/.spawn-actions/unknown"), "unknown").unwrap();
                }
                "advanced-successor" => {
                    let mut a = f.action.clone();
                    a.request_id = "successor".into();
                    a.binding = serde_json::from_slice(&bytes).unwrap();
                    a.stage = spawn::LaunchStage::Submitted;
                    a.endpoint = Some("other:mx-task".into());
                    fs::write(
                        f.home.join("state/.spawn-actions/successor.json"),
                        serde_json::to_vec(&a).unwrap(),
                    )
                    .unwrap();
                }
                "wrong-successor" => f.edit_intent(|r| r.attempt.as_mut().unwrap().generation += 1),
                "intent-link" => {
                    fs::remove_file(f.intent()).unwrap();
                    symlink("missing", f.intent()).unwrap();
                }
                "unfinished-journal" => {
                    fs::write(f.home.join("state/.teardown.transaction.task"), "prepared").unwrap();
                }
                _ => unreachable!(),
            }
            assert!(inspect(&f.root, &f.home, "task").is_err(), "{case}");
            assert!(
                !f.home.join("state/.retired-spawn-intents").exists(),
                "{case}"
            );
        }
    }
    #[test]
    fn changed_proof_and_new_launch_after_preview_fail_closed() {
        for case in ["intent", "lease", "action", "new-action"] {
            let f = Fixture::new();
            let proof = inspect(&f.root, &f.home, "task").unwrap();
            match case {
                "intent" => f.edit_intent(|r| r.schedule.priority += 1),
                "lease" => {
                    fs::write(f.home.join("data/.home-allocation-task.json"), "changed").unwrap();
                }
                "action" => {
                    fs::write(
                        f.home.join("state/.spawn-actions/old-launch.json"),
                        "changed",
                    )
                    .unwrap();
                }
                "new-action" => {
                    let mut a = f.action.clone();
                    a.request_id = "new".into();
                    fs::write(
                        f.home.join("state/.spawn-actions/new.json"),
                        serde_json::to_vec(&a).unwrap(),
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            };
            assert!(quarantine(&f.root, &f.home, &proof).is_err(), "{case}");
            assert!(f.intent().exists());
        }
    }
    #[test]
    fn interrupted_unlink_recovers_only_exact_preserved_proof() {
        let f = Fixture::new();
        let proof = inspect(&f.root, &f.home, "task").unwrap();
        quarantine(&f.root, &f.home, &proof).unwrap();
        let dir = fs::read_dir(f.home.join("state/.retired-spawn-intents"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::remove_file(dir.join("complete")).unwrap();
        quarantine(&f.root, &f.home, &proof).unwrap();
        assert!(dir.join("complete").exists());
        fs::write(
            f.home.join("state/.spawn-actions/new.json"),
            serde_json::to_vec(&f.action).unwrap(),
        )
        .unwrap();
        assert!(quarantine(&f.root, &f.home, &proof).is_err());
    }
    #[test]
    fn interruptions_preserve_evidence_before_and_after_unlink() {
        for stage in ["backup", "unlink"] {
            let f = Fixture::new();
            let original = fs::read(f.intent()).unwrap();
            let proof = inspect(&f.root, &f.home, "task").unwrap();
            assert!(
                quarantine_inner(&f.root, &f.home, &proof, |checkpoint| {
                    if checkpoint == stage {
                        Err("injected interruption".into())
                    } else {
                        Ok(())
                    }
                })
                .is_err()
            );
            assert_eq!(f.intent().exists(), stage == "backup");
            if stage == "backup" {
                assert_eq!(fs::read(f.intent()).unwrap(), original);
            }
            let audit = fs::read_dir(f.home.join("state/.retired-spawn-intents"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            assert!(audit.join("proof.json").exists());
            assert!(!audit.join("complete").exists());
            quarantine(&f.root, &f.home, &proof).unwrap();
            assert!(audit.join("complete").exists());
        }
    }
    #[test]
    fn unsafe_cli_task_fails_before_creating_paths() {
        let f = Fixture::new();
        let args: Vec<OsString> = [
            "retired-intent",
            "apply",
            "--root",
            f.root.to_str().unwrap(),
            "--home",
            f.home.to_str().unwrap(),
            "--task",
            "../escape",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        assert!(run_inner(&args).is_err());
        assert!(!f.home.join("state/.lock.acquire").exists());
    }
    #[test]
    fn upgrade_cleanup_inspects_and_applies_retired_intents_without_task_completion() {
        let f = Fixture::new();
        let before = fs::read(f.intent()).unwrap();
        let plan = crate::upgrade_cleanup::inspect(&f.root, &f.home).unwrap();
        assert!(plan.actionable());
        assert_eq!(fs::read(f.intent()).unwrap(), before);
        crate::upgrade_cleanup::apply(&f.root, &f.home, &plan).unwrap();
        assert!(!f.intent().exists());
        let next = crate::upgrade_cleanup::inspect(&f.root, &f.home).unwrap();
        assert!(!next.actionable());
        assert!(!f.home.join("state/.upgrade-retirements/task.json").exists());
        assert!(f.archive.join("private.txt").is_file());
    }
    #[test]
    fn standalone_owner_inspects_refuses_live_then_applies_and_repeats() {
        let f = Fixture::new();
        let arguments = |operation: &str| -> Vec<OsString> {
            [
                "retired-intent",
                operation,
                "--root",
                f.root.to_str().unwrap(),
                "--home",
                f.home.to_str().unwrap(),
                "--task",
                "task",
            ]
            .into_iter()
            .map(OsString::from)
            .collect()
        };
        assert_eq!(run(&[OsString::from("--help")]), 0);
        assert_eq!(run(&[OsString::from("invalid")]), 2);
        let original = fs::read(f.intent()).unwrap();
        run_inner(&arguments("inspect")).unwrap();
        assert_eq!(fs::read(f.intent()).unwrap(), original);
        for state in [f.home.join("state"), f.archive.join("state")] {
            fs::write(state.join(".lock"), std::process::id().to_string()).unwrap();
            assert!(run_inner(&arguments("apply")).is_err());
            assert_eq!(fs::read(f.intent()).unwrap(), original);
            fs::remove_file(state.join(".lock")).unwrap();
        }
        run_inner(&arguments("apply")).unwrap();
        run_inner(&arguments("apply")).unwrap();
        assert!(!f.intent().exists());
        let directory = f.home.join("state/.retired-spawn-intents");
        let audit = fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::rename(&audit, directory.join("task-wrong-digest")).unwrap();
        assert!(run_inner(&arguments("apply")).is_err());
    }
    #[test]
    fn recovery_refuses_changed_retirement_and_unreadable_backup() {
        for case in ["lease", "archive", "proof-link", "proof-corrupt"] {
            let f = Fixture::new();
            let proof = inspect(&f.root, &f.home, "task").unwrap();
            quarantine(&f.root, &f.home, &proof).unwrap();
            let audit = fs::read_dir(f.home.join("state/.retired-spawn-intents"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            fs::remove_file(audit.join("complete")).unwrap();
            match case {
                "lease" => {
                    fs::write(f.home.join("data/.home-allocation-task.json"), "changed").unwrap()
                }
                "archive" => {
                    fs::rename(&f.archive, f._temp.path().join("saved-archive")).unwrap();
                    fs::create_dir_all(f.archive.join("state")).unwrap();
                    fs::write(f.archive.join(multplx_core::agent_home::MARKER), "task\n").unwrap();
                }
                "proof-link" => {
                    fs::rename(audit.join("proof.json"), f._temp.path().join("saved-proof"))
                        .unwrap();
                    symlink(f._temp.path().join("saved-proof"), audit.join("proof.json")).unwrap();
                }
                "proof-corrupt" => fs::write(audit.join("proof.json"), "changed").unwrap(),
                _ => unreachable!(),
            }
            assert!(quarantine(&f.root, &f.home, &proof).is_err(), "{case}");
            assert!(!audit.join("complete").exists());
        }
    }
    #[test]
    fn live_primary_and_archive_owner_block_maintenance() {
        let f = Fixture::new();
        for state in [f.home.join("state"), f.archive.join("state")] {
            fs::write(state.join(".lock"), std::process::id().to_string()).unwrap();
            assert!(MaintenanceGuard::acquire(&state, &SystemProcessProbe::default()).is_err());
            fs::remove_file(state.join(".lock")).unwrap();
        }
    }
}

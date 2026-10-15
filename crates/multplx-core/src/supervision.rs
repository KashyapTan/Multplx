//! Missing-supervision predicate from `bin/mx-supervision-lib.sh`.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::filesystem::MAX_TASK_METADATA_BYTES;

/// Exclude only a well-formed, current canonical assignment that is
/// explicitly complete. Every legacy, malformed, oversized, or
/// incomplete record remains in-flight conservatively.
fn completed_assignment_record(path: &Path) -> bool {
    let Some(task_id) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    let Ok(bytes) = crate::filesystem::read_bounded_regular(path, MAX_TASK_METADATA_BYTES) else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return false;
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut schema_version = None;
    let mut canonical_model = None;
    let mut kind = None;
    for line in text.lines().filter(|line| !line.is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        if !seen.insert(key) {
            return false;
        }
        match key {
            "schema_version" => schema_version = Some(value),
            "canonical_model" => canonical_model = Some(value),
            "kind" => kind = Some(value),
            _ => {}
        }
    }
    if schema_version != Some("2") {
        return false;
    }
    let Some(model) =
        canonical_model.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
    else {
        return false;
    };
    model
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        == Some(2)
        && model.get("task_id").and_then(serde_json::Value::as_str) == Some(task_id)
        && matches!(
            model.get("role").and_then(serde_json::Value::as_str),
            Some("researcher" | "implementer" | "reviewer" | "sub-orchestrator")
        )
        && matches!(
            model.get("artifact").and_then(serde_json::Value::as_str),
            Some("report" | "implementation" | "coordination")
        )
        && kind
            == if model.get("persistent").and_then(serde_json::Value::as_bool) == Some(true)
                || model
                    .get("private_home")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            {
                Some("daemon")
            } else {
                match model.get("artifact").and_then(serde_json::Value::as_str) {
                    Some("report") => Some("scout"),
                    Some("implementation" | "coordination") => Some("delivery"),
                    _ => None,
                }
            }
        && model
            .get("persistent")
            .and_then(serde_json::Value::as_bool)
            .is_some()
        && model
            .get("private_home")
            .and_then(serde_json::Value::as_bool)
            .is_some()
        && model
            .get("legacy_unknown")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        && model
            .pointer("/attempt/id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| !id.is_empty())
        && model
            .pointer("/attempt/generation")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|generation| generation > 0)
        && model
            .pointer("/attempt/brief_revision")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|revision| {
                revision > 0
                    && model
                        .get("accepted_brief_revision")
                        .and_then(serde_json::Value::as_u64)
                        == Some(revision)
            })
        && model
            .pointer("/schedule/state")
            .and_then(serde_json::Value::as_str)
            == Some("completed")
}

/// Shared location of the root-owned admission queue.
pub const DISPATCH_QUEUE_DIRECTORY: &str = ".dispatch-queue";

// Presence is an obligation, not validation or dispatch authority. The queue
// owner interprets records and rejects malformed/foreign bindings. Do not read
// record contents or follow a directory symlink merely to decide supervision.
fn dispatch_pending(state: &Path) -> bool {
    retained_queue_entries(&state.join(DISPATCH_QUEUE_DIRECTORY))
}

// Queue presence keeps its owning background consumer eligible even when all
// task assignments and model wakes have settled. Contents remain exclusively
// the queue owner's responsibility, including invalid or blocked records.
fn retained_queue_entries(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => true,
        Ok(_) => fs::read_dir(path).map_or(true, |mut entries| entries.next().is_some()),
    }
}

/// One deterministic supervision observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupervisionStatus {
    /// Count of active, legacy, or otherwise unclassified task records.
    pub in_flight: usize,
    /// Whether active/queued work, durable wakes, or explicit checks need a watcher.
    pub needed: bool,
    /// Whether the watcher beacon is younger than the grace interval.
    pub watcher_fresh: bool,
    /// Human-compatible beacon description.
    pub beacon_description: String,
    /// Whether the durable wake queue has unread bytes.
    pub queue_pending: bool,
    /// Whether this state owns retained or unreadable dispatch-queue entries.
    pub dispatch_pending: bool,
    /// Whether this state retains inbox/outbox work for its parent relay.
    pub parent_channel_pending: bool,
}

impl SupervisionStatus {
    /// The dangerous state: work exists and no fresh watcher beacon does.
    #[must_use]
    pub fn unhealthy(&self) -> bool {
        self.needed && !self.watcher_fresh
    }
}

/// Inspect one state directory using an injected current time.
pub fn inspect(state: impl AsRef<Path>, grace: Duration, now: SystemTime) -> SupervisionStatus {
    let state = state.as_ref();
    let in_flight = fs::read_dir(state)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(std::result::Result::ok))
        .filter(|entry| {
            let path = entry.path();
            path.extension().and_then(|extension| extension.to_str()) == Some("meta")
                && !completed_assignment_record(&path)
        })
        .count();
    let beacon = state.join(".last-watcher-beat");
    let (watcher_fresh, beacon_description) = match fs::metadata(&beacon)
        .and_then(|metadata| metadata.modified())
    {
        Ok(modified) => {
            let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
            (age < grace, format!("{}s ago", age.as_secs()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (false, "never".to_owned()),
        Err(_) => (false, "unknown".to_owned()),
    };
    let queue_pending = fs::metadata(state.join(".wake-queue"))
        .is_ok_and(|metadata| metadata.len() > 0)
        || crate::wake::WakeQueue::new(state.to_path_buf())
            .observe_unfinished_count()
            .map_or(true, |count| count > 0);
    let explicit_checks = fs::read_dir(state).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".check.sh"))
        })
    });
    let dispatch_pending = dispatch_pending(state);
    let parent_channel_pending = ["parent-inbox", "parent-outbox"]
        .iter()
        .any(|name| retained_queue_entries(&state.join(name)));
    SupervisionStatus {
        in_flight,
        needed: in_flight > 0
            || queue_pending
            || dispatch_pending
            || parent_channel_pending
            || explicit_checks,
        watcher_fresh,
        beacon_description,
        queue_pending,
        dispatch_pending,
        parent_channel_pending,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::inspect;

    #[test]
    fn queued_dispatch_needs_supervision_without_task_metadata_and_stays_local() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child/state");
        fs::create_dir_all(&child).unwrap();
        let queue = root.path().join(super::DISPATCH_QUEUE_DIRECTORY);
        fs::create_dir(&queue).unwrap();
        let observe =
            |state: &std::path::Path| inspect(state, Duration::from_secs(300), SystemTime::now());
        assert!(!observe(root.path()).needed);
        let request = queue.join("nested.request");
        // Malformed retained bytes remain an obligation for the admission owner;
        // supervision must not parse, claim, repair, or overwrite their binding.
        fs::write(&request, "owner_state=child/state\nmalformed\n").unwrap();
        let status = observe(root.path());
        assert!(status.needed && status.dispatch_pending && status.unhealthy());
        assert_eq!(status.in_flight, 0);
        assert!(!status.queue_pending);
        assert!(!observe(&child).needed);
        assert_eq!(
            fs::read_to_string(&request).unwrap(),
            "owner_state=child/state\nmalformed\n"
        );
        fs::remove_file(request).unwrap();
        assert!(!observe(root.path()).needed);
        fs::remove_dir(&queue).unwrap();
        fs::write(&queue, "invalid queue directory").unwrap();
        assert!(observe(root.path()).dispatch_pending);
        fs::remove_file(&queue).unwrap();
        std::os::unix::fs::symlink(&child, &queue).unwrap();
        assert!(observe(root.path()).dispatch_pending);
        assert!(fs::read_dir(&child).unwrap().next().is_none());
    }

    #[test]
    fn in_flight_without_beacon_is_unhealthy() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(!inspect(temp.path(), Duration::from_secs(300), SystemTime::now()).unhealthy());
        fs::write(temp.path().join("task.meta"), b"id=task\n").expect("meta");
        assert!(inspect(temp.path(), Duration::from_secs(300), SystemTime::now()).unhealthy());
    }

    #[test]
    fn retained_parent_channel_work_needs_supervision_without_tasks_or_wakes() {
        let temp = tempfile::tempdir().unwrap();
        let observe = || inspect(temp.path(), Duration::from_secs(300), SystemTime::now());
        for name in ["parent-inbox", "parent-outbox"] {
            let directory = temp.path().join(name);
            fs::create_dir(&directory).unwrap();
            assert!(!observe().needed);
            let path = directory.join("outcome.json");
            fs::write(&path, "retained owner work").unwrap();
            assert!(observe().needed, "{name} was stranded without supervision");
            assert_eq!(observe().in_flight, 0);
            assert!(!observe().queue_pending);
            assert_eq!(fs::read_to_string(&path).unwrap(), "retained owner work");
            fs::remove_file(path).unwrap();
            assert!(!observe().needed);
            fs::remove_dir(&directory).unwrap();
            fs::write(&directory, "malformed queue").unwrap();
            assert!(observe().needed);
            fs::remove_file(&directory).unwrap();
            std::os::unix::fs::symlink(temp.path().join("absent"), &directory).unwrap();
            assert!(observe().needed);
            fs::remove_file(directory).unwrap();
        }
    }

    #[test]
    fn completed_standing_assignment_retains_wakes_checks_and_reopened_work() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("standing.meta");
        let completed = r#"schema_version=2
kind=daemon
canonical_model={"schema_version":2,"task_id":"standing","role":"sub-orchestrator","artifact":"coordination","persistent":true,"private_home":true,"legacy_unknown":false,"accepted_brief_revision":1,"attempt":{"id":"attempt-1","generation":1,"brief_revision":1},"schedule":{"state":"completed"}}
"#;
        fs::write(&path, completed).unwrap();
        let observe = || inspect(temp.path(), Duration::from_secs(300), SystemTime::now());
        assert!(!observe().needed);
        assert!(!temp.path().join(".wake-queue.lock").exists());
        assert!(!temp.path().join("wake-inbox").exists());
        fs::write(temp.path().join(".wake-queue"), "done wake\n").unwrap();
        assert_eq!(observe().in_flight, 0);
        assert!(observe().needed);
        assert!(observe().unhealthy());
        fs::remove_file(temp.path().join(".wake-queue")).unwrap();
        fs::write(temp.path().join("standing.check.sh"), "check").unwrap();
        assert!(observe().needed);
        fs::remove_file(temp.path().join("standing.check.sh")).unwrap();
        assert!(!observe().needed);
        fs::write(&path, completed.replace("completed", "running")).unwrap();
        assert_eq!(observe().in_flight, 1);
        fs::write(
            &path,
            completed.replace("accepted_brief_revision\":1", "accepted_brief_revision\":2"),
        )
        .unwrap();
        assert_eq!(observe().in_flight, 1);
        fs::write(
            &path,
            completed.replace("\"task_id\":\"standing\"", "\"task_id\":\"other\""),
        )
        .unwrap();
        assert_eq!(observe().in_flight, 1);
    }

    #[test]
    fn unfinished_claim_keeps_supervision_without_observer_mutation() {
        use crate::process::ProcessProbe;
        let temp = tempfile::tempdir().expect("tempdir");
        let queue = crate::wake::WakeQueue::new(temp.path());
        let probe = crate::process::SystemProcessProbe::default();
        let owner = probe.identity(std::process::id()).unwrap();
        queue
            .append(
                crate::wake::WakeKind::Signal,
                "result.status",
                "done",
                SystemTime::now(),
                &probe,
            )
            .unwrap();
        let claimed = queue
            .claim_available(&owner, SystemTime::now(), 1, &probe)
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(
            fs::metadata(temp.path().join(".wake-queue")).unwrap().len(),
            0
        );
        let inbox_path = temp.path().join("wake-inbox");
        let before = fs::read_dir(&inbox_path)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (path.clone(), fs::read(path).unwrap())
            })
            .collect::<Vec<_>>();
        let lock_path = temp.path().join(".wake-queue.lock");
        let lock = crate::locks::DirectoryLock::try_acquire(&lock_path, &probe).unwrap();
        let started = std::time::Instant::now();
        let status = inspect(temp.path(), Duration::from_secs(300), SystemTime::now());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(lock_path.exists());
        drop(lock);
        assert_eq!(status.in_flight, 0);
        assert!(status.queue_pending && status.needed);
        assert!(!temp.path().join(".wake-queue.lock").exists());
        for (path, bytes) in before {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn only_valid_completed_assignment_records_stop_requiring_supervision() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ordinary = |id: &str, state: &str| {
            format!(
                "schema_version=2\nkind=delivery\ncanonical_model={{\"schema_version\":2,\"task_id\":\"{id}\",\"role\":\"implementer\",\"artifact\":\"implementation\",\"persistent\":false,\"private_home\":false,\"legacy_unknown\":false,\"accepted_brief_revision\":1,\"attempt\":{{\"id\":\"attempt-1\",\"generation\":1,\"brief_revision\":1}},\"schedule\":{{\"state\":\"{state}\"}}}}\n"
            )
        };
        let now = SystemTime::now();
        fs::write(
            temp.path().join("ordinary.meta"),
            ordinary("ordinary", "completed"),
        )
        .expect("completed metadata");
        assert_eq!(
            inspect(temp.path(), Duration::from_secs(300), now).in_flight,
            0
        );

        fs::write(
            temp.path().join("ordinary.meta"),
            ordinary("ordinary", "running"),
        )
        .expect("reopened metadata");
        assert_eq!(
            inspect(temp.path(), Duration::from_secs(300), now).in_flight,
            1
        );

        fs::write(
            temp.path().join("standing.meta"),
            "schema_version=2\nkind=daemon\ncanonical_model={\"schema_version\":2,\"task_id\":\"standing\",\"role\":\"sub-orchestrator\",\"artifact\":\"coordination\",\"persistent\":true,\"private_home\":true,\"legacy_unknown\":false,\"accepted_brief_revision\":1,\"attempt\":{\"id\":\"attempt-1\",\"generation\":1,\"brief_revision\":1},\"schedule\":{\"state\":\"completed\"}}\n",
        )
        .expect("standing coordinator metadata");
        fs::write(
            temp.path().join("persistent-worker.meta"),
            "schema_version=2\nkind=daemon\ncanonical_model={\"schema_version\":2,\"task_id\":\"persistent-worker\",\"role\":\"implementer\",\"artifact\":\"implementation\",\"persistent\":true,\"private_home\":true,\"legacy_unknown\":false,\"accepted_brief_revision\":1,\"attempt\":{\"id\":\"attempt-1\",\"generation\":1,\"brief_revision\":1},\"schedule\":{\"state\":\"completed\"}}\n",
        )
        .expect("persistent worker metadata");
        fs::write(temp.path().join("legacy.meta"), b"id=legacy\n").expect("legacy metadata");
        fs::write(
            temp.path().join("malformed.meta"),
            b"schema_version=2\nkind=delivery\ncanonical_model={bad}\n",
        )
        .expect("malformed metadata");
        fs::write(temp.path().join(".wake-queue"), b"pending\n").expect("pending wake");
        let status = inspect(temp.path(), Duration::from_secs(300), now);
        assert_eq!(status.in_flight, 3);
        assert!(status.unhealthy());
        assert!(status.queue_pending);
    }
}

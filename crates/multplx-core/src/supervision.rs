//! Missing-supervision predicate from `bin/mx-supervision-lib.sh`.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

const META_LIMIT: usize = 4 * 1024 * 1024;

/// Exclude only a well-formed, current canonical ordinary assignment that is
/// explicitly complete. Every legacy, malformed, oversized, persistent, or
/// coordinator record remains in-flight conservatively.
fn completed_ordinary_record(path: &Path) -> bool {
    let Some(task_id) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    let Ok(bytes) = crate::filesystem::read_bounded_regular(path, META_LIMIT) else {
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
            Some("researcher" | "implementer" | "reviewer")
        )
        && matches!(
            model.get("artifact").and_then(serde_json::Value::as_str),
            Some("report" | "implementation")
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
                    Some("implementation") => Some("delivery"),
                    _ => None,
                }
            }
        && model.get("persistent").and_then(serde_json::Value::as_bool) == Some(false)
        && model
            .get("private_home")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
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

/// One deterministic supervision observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupervisionStatus {
    /// Count of active, legacy, persistent, or otherwise unclassified task records.
    pub in_flight: usize,
    /// Whether in-flight work needs a watcher.
    pub needed: bool,
    /// Whether the watcher beacon is younger than the grace interval.
    pub watcher_fresh: bool,
    /// Human-compatible beacon description.
    pub beacon_description: String,
    /// Whether the durable wake queue has unread bytes.
    pub queue_pending: bool,
}

impl SupervisionStatus {
    /// The dangerous state: work exists and no fresh watcher beacon does.
    #[must_use]
    pub fn unhealthy(&self) -> bool {
        self.in_flight > 0 && !self.watcher_fresh
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
                && !completed_ordinary_record(&path)
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
    let queue_pending =
        fs::metadata(state.join(".wake-queue")).is_ok_and(|metadata| metadata.len() > 0);
    SupervisionStatus {
        in_flight,
        needed: in_flight > 0,
        watcher_fresh,
        beacon_description,
        queue_pending,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::inspect;

    #[test]
    fn in_flight_without_beacon_is_unhealthy() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(!inspect(temp.path(), Duration::from_secs(300), SystemTime::now()).unhealthy());
        fs::write(temp.path().join("task.meta"), b"id=task\n").expect("meta");
        assert!(inspect(temp.path(), Duration::from_secs(300), SystemTime::now()).unhealthy());
    }

    #[test]
    fn only_valid_completed_ordinary_records_stop_requiring_supervision() {
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
            "schema_version=2\nkind=daemon\ncanonical_model={\"schema_version\":2,\"task_id\":\"standing\",\"role\":\"sub-orchestrator\",\"artifact\":\"coordination\",\"persistent\":true,\"private_home\":true,\"legacy_unknown\":false,\"attempt\":{\"id\":\"attempt-1\",\"generation\":1,\"brief_revision\":1},\"schedule\":{\"state\":\"completed\"}}\n",
        )
        .expect("standing coordinator metadata");
        fs::write(
            temp.path().join("persistent-worker.meta"),
            "schema_version=2\nkind=daemon\ncanonical_model={\"schema_version\":2,\"task_id\":\"persistent-worker\",\"role\":\"implementer\",\"artifact\":\"implementation\",\"persistent\":true,\"private_home\":true,\"legacy_unknown\":false,\"attempt\":{\"id\":\"attempt-1\",\"generation\":1,\"brief_revision\":1},\"schedule\":{\"state\":\"completed\"}}\n",
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
        assert_eq!(status.in_flight, 5);
        assert!(status.unhealthy());
        assert!(status.queue_pending);
    }
}

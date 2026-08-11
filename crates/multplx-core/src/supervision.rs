//! Missing-supervision predicate from `bin/mx-supervision-lib.sh`.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// One deterministic supervision observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupervisionStatus {
    /// Count of `state/*.meta` records.
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
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("meta")
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
}

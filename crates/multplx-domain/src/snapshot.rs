//! Typed read model and deterministic human renderer for system snapshots.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde::Deserialize;

use crate::supervision::CommandResult;

pub const SYSTEM_VIEW_USAGE: &str = "usage: mx-system-view.sh [--json]\n\nRender a human system view from mx-system-snapshot.sh.\nUse --json to print the underlying snapshot.\n";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SystemSnapshot {
    pub schema: String,
    pub mx_home: String,
    pub roots: SnapshotRoots,
    pub backlog: Backlog,
    pub tasks: Vec<Task>,
    pub main_inventory: MainInventory,
    pub scout_reports: Vec<ArtifactPointer>,
    pub watcher: Watcher,
    pub wake_queue: WakeQueue,
    pub dispatch_queue: DispatchQueue,
    #[serde(default)]
    pub headroom: Option<serde_json::Value>,
    #[serde(default)]
    pub headroom_reason: Option<String>,
    pub vplan_reviews: ArtifactFeed,
    pub later_feeds: LaterFeeds,
    pub daemon_current: DaemonCurrent,
    pub daemon_landed: DaemonLanded,
    pub daemon_guidance: DaemonGuidance,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SnapshotRoots {
    pub mx_root: String,
    pub state: String,
    pub data: String,
    pub config: String,
    pub projects: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct MainInventory {
    pub valid: bool,
    #[serde(default)]
    pub reason: Option<String>,
    pub orphan_in_flight: Vec<String>,
    pub unstructured_current_count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ArtifactPointer {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub artifact: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Watcher {
    pub lock_present: bool,
    #[serde(default)]
    pub pid: Option<u32>,
    pub identity_verified: bool,
    pub alive: bool,
    #[serde(default)]
    pub beacon_age_secs: Option<u64>,
    pub stale: bool,
    pub afk: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct WakeQueue {
    pub depth: u64,
    #[serde(default)]
    pub oldest_age_secs: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DispatchQueue {
    pub depth: u64,
    pub records: Vec<serde_json::Value>,
    pub available: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ArtifactFeed {
    pub records: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LaterFeeds {
    pub gate_runs: LifecycleFeed,
    pub workflow_runs: LifecycleFeed,
    pub deliveries: LifecycleFeed,
    pub upstream_drift: serde_json::Value,
    pub doctor: Availability,
    pub timeline: Availability,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LifecycleFeed {
    pub supported: bool,
    pub available: bool,
    pub records: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Availability {
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DaemonCurrent {
    pub registry: serde_json::Value,
    pub records: Vec<DaemonRecord>,
    pub total_registered: u64,
    pub total: u64,
    pub shown: u64,
    pub truncated: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DaemonRecord {
    pub id: String,
    #[serde(default)]
    pub home: Option<String>,
    pub current: serde_json::Value,
    pub provenance: serde_json::Value,
    pub freshness: serde_json::Value,
    pub active_children: Vec<serde_json::Value>,
    pub decisions_open: Vec<serde_json::Value>,
    pub holds: Vec<serde_json::Value>,
    pub queued: Vec<serde_json::Value>,
    pub landed: Vec<serde_json::Value>,
    pub endpoints: Vec<serde_json::Value>,
    pub counts: serde_json::Value,
    pub omitted: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DaemonLanded {
    pub records: Vec<serde_json::Value>,
    pub truncated: Vec<String>,
    pub unreadable: Vec<String>,
    pub partial: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Backlog {
    pub path: String,
    pub present: bool,
    pub records: Vec<BacklogRecord>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct BacklogRecord {
    pub state: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub raw: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub blocked_by: Option<String>,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub report_path: Option<String>,
    #[serde(default)]
    pub local_note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Task {
    pub id: String,
    pub kind: String,
    pub project: String,
    pub backend: String,
    pub current_state: CurrentState,
    pub endpoint: Endpoint,
    pub pr: PullRequest,
    pub paths: TaskPaths,
    pub actions: Actions,
    #[serde(default)]
    pub backlog: Option<BacklogRecord>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CurrentState {
    pub state: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Endpoint {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub exists: Option<bool>,
    pub agent_alive: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct PullRequest {
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct TaskPaths {
    pub home: ObservedPath,
    pub worktree: ObservedPath,
    pub report: ObservedPath,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObservedPath {
    #[serde(default)]
    pub path: Option<String>,
    pub present: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Actions {
    #[serde(default)]
    pub watch: Option<String>,
    #[serde(default)]
    pub send: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DaemonGuidance {
    pub note: String,
}

fn dash(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("-")
}

fn endpoint(task: &Task) -> String {
    let presence = match task.endpoint.exists {
        Some(true) => "present",
        Some(false) => "absent",
        None => "unknown",
    };
    if task.kind == "daemon" {
        format!("{presence} / {}", task.endpoint.agent_alive)
    } else {
        presence.to_owned()
    }
}

fn artifact(task: &Task) -> &str {
    task.pr
        .url
        .as_deref()
        .or_else(|| {
            task.paths
                .report
                .present
                .then_some(task.paths.report.path.as_deref())
                .flatten()
        })
        .unwrap_or("-")
}

fn task_path(task: &Task) -> String {
    if task.paths.home.present {
        return dash(task.paths.home.path.as_deref()).to_owned();
    }
    if let Some(path) = task.paths.home.path.as_deref() {
        return format!("{path} (absent)");
    }
    if task.paths.worktree.present {
        return dash(task.paths.worktree.path.as_deref()).to_owned();
    }
    if let Some(path) = task.paths.worktree.path.as_deref() {
        return format!("{path} (absent)");
    }
    "-".to_owned()
}

fn action(task: &Task) -> String {
    if task.kind == "daemon" {
        format!(
            "{} - {}",
            dash(task.actions.send.as_deref()),
            dash(task.actions.watch.as_deref())
        )
    } else {
        dash(task.actions.watch.as_deref()).to_owned()
    }
}

fn backlog_artifact(record: &BacklogRecord) -> &str {
    record
        .pr_url
        .as_deref()
        .or(record.report_path.as_deref())
        .or(record.local_note.as_deref())
        .unwrap_or("-")
}

fn blocker(record: &BacklogRecord) -> String {
    match (
        record
            .blocked_by
            .as_deref()
            .filter(|value| !value.is_empty()),
        record
            .blocked_reason
            .as_deref()
            .filter(|value| !value.is_empty()),
    ) {
        (None, _) => "-".to_owned(),
        (Some(blocker), None) => blocker.to_owned(),
        (Some(blocker), Some(reason)) => format!("{blocker} - {reason}"),
    }
}

fn backlog_row(record: &BacklogRecord) -> String {
    format!(
        "| {} | {} | {} | {} | {} | {} |",
        dash(record.id.as_deref()),
        dash(record.title.as_deref().or(record.raw.as_deref())),
        dash(record.repo.as_deref()),
        dash(record.kind.as_deref()),
        blocker(record),
        backlog_artifact(record)
    )
}

/// Parse the canonical JSON into the typed, read-only snapshot model.
pub fn parse_system_snapshot(bytes: &[u8]) -> Result<SystemSnapshot, CommandResult> {
    let snapshot: SystemSnapshot = serde_json::from_slice(bytes).map_err(|_| CommandResult {
        status: 1,
        stdout: String::new(),
        stderr: "mx-system-view: invalid canonical snapshot\n".to_owned(),
    })?;
    if snapshot.schema != "mx-system-snapshot.v1" {
        return Err(CommandResult {
            status: 1,
            stdout: String::new(),
            stderr: "mx-system-view: invalid canonical snapshot\n".to_owned(),
        });
    }
    Ok(snapshot)
}

/// Render the stable human Markdown view without rereading system state.
#[must_use]
pub fn render_system_view(snapshot: &SystemSnapshot) -> String {
    let mut output = format!(
        "# System View\n\nSchema: {}\nHome: {}\n\n## Under Way\n",
        snapshot.schema, snapshot.mx_home
    );
    if snapshot.tasks.is_empty() {
        output.push_str("No live task metadata found.\n");
    } else {
        output.push_str("| ID | Current | Kind | Repo/Project | Backend | Endpoint | Artifact | Path | Watch / return channel |\n");
        output.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
        for task in &snapshot.tasks {
            let repo = task
                .backlog
                .as_ref()
                .and_then(|record| record.repo.as_deref())
                .unwrap_or(&task.project);
            output.push_str(&format!(
                "| {} | {} / {} | {} | {} | {} | {} | {} | {} | {} |\n",
                task.id,
                task.current_state.state,
                task.current_state.source,
                task.kind,
                dash(Some(repo)),
                task.backend,
                endpoint(task),
                artifact(task),
                task_path(task),
                action(task)
            ));
        }
    }
    for (heading, state, empty) in [
        ("Queued", "queued", "No queued backlog records found."),
        ("Done", "done", "No done backlog records found."),
    ] {
        output.push_str(&format!("\n## {heading}\n"));
        let records = snapshot
            .backlog
            .records
            .iter()
            .filter(|record| record.state == state)
            .collect::<Vec<_>>();
        if records.is_empty() {
            output.push_str(empty);
            output.push('\n');
        } else {
            output.push_str("| ID | Title | Repo | Kind | Blocked By | Artifact |\n");
            output.push_str("| --- | --- | --- | --- | --- | --- |\n");
            for record in records {
                output.push_str(&backlog_row(record));
                output.push('\n');
            }
        }
    }
    output.push_str("\n## Daemons\n");
    output.push_str(&snapshot.daemon_guidance.note);
    output.push('\n');
    output
}

/// Check whether one executable name is present on PATH.
#[must_use]
pub fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| {
            fs::metadata(Path::new(&directory).join(name)).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_view_rejects_the_wrong_schema() {
        let bytes = br#"{"schema":"wrong"}"#;
        assert!(parse_system_snapshot(bytes).is_err());
    }
}

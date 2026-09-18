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
    /// Task-first shared projection consumed by status, workspace clients and MX Viz.
    /// Older fixtures may omit it, but native snapshots always publish it.
    #[serde(default)]
    pub portfolio: Option<Portfolio>,
    pub main_inventory: MainInventory,
    pub scout_reports: Vec<ArtifactPointer>,
    pub watcher: Watcher,
    pub wake_queue: WakeQueue,
    pub dispatch_queue: DispatchQueue,
    #[serde(default)]
    pub headroom: Option<serde_json::Value>,
    #[serde(default)]
    pub headroom_reason: Option<String>,
    #[serde(default)]
    pub domains: DomainProjection,
    pub vplan_reviews: ArtifactFeed,
    pub later_feeds: LaterFeeds,
    pub daemon_current: DaemonCurrent,
    pub daemon_landed: DaemonLanded,
    pub daemon_guidance: DaemonGuidance,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Portfolio {
    #[serde(deserialize_with = "portfolio_schema")]
    pub schema: String,
    pub generated: String,
    pub observed_at: String,
    pub freshness: PortfolioFreshness,
    pub counts: PortfolioCounts,
    pub projects: Vec<serde_json::Value>,
    pub tasks: Vec<PortfolioTask>,
}

fn portfolio_schema<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let schema = String::deserialize(deserializer)?;
    if schema != "mx-portfolio.v1" {
        return Err(serde::de::Error::custom(
            "unsupported canonical portfolio schema",
        ));
    }
    Ok(schema)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct PortfolioFreshness {
    /// One of `fresh`, `partial`, `stale`, or `unknown`.
    pub status: String,
    #[serde(default)]
    pub age_seconds: Option<u64>,
    pub partial: bool,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct PortfolioCounts {
    pub tasks: u64,
    #[serde(default)]
    pub coordinators: u64,
    #[serde(default)]
    pub records: u64,
    #[serde(default)]
    pub shown: u64,
    #[serde(default)]
    pub truncated: u64,
    pub sessions: u64,
    pub attempts: u64,
    pub projects: u64,
    pub domains: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct PortfolioTask {
    /// Home-qualified stable identity used by deep links and cross-home edges.
    pub key: String,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub root_id: Option<String>,
    pub children: Vec<String>,
    pub project: serde_json::Value,
    pub state: String,
    pub priority: i64,
    #[serde(default)]
    pub role: Option<String>,
    pub owner: serde_json::Value,
    pub attempt: serde_json::Value,
    pub prior_attempts: Vec<serde_json::Value>,
    pub brief: serde_json::Value,
    pub workflow: serde_json::Value,
    pub dependencies: Vec<serde_json::Value>,
    pub decisions: Vec<serde_json::Value>,
    pub evidence: serde_json::Value,
    pub allocation: serde_json::Value,
    pub sessions: Vec<serde_json::Value>,
    pub native_observations: Vec<serde_json::Value>,
    pub latest_change: serde_json::Value,
    pub freshness: PortfolioFreshness,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct DomainProjection {
    pub records: Vec<DomainRecord>,
    pub total: u64,
    pub shown: u64,
    pub truncated: u64,
    pub complete: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DomainRecord {
    #[serde(default)]
    pub domain_id: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    pub projects: Vec<String>,
    #[serde(default)]
    pub idea_id: Option<String>,
    #[serde(default)]
    pub scope_revision: Option<u64>,
    #[serde(default)]
    pub assignment_generation: Option<u64>,
    pub coordinator: serde_json::Value,
    pub channel: serde_json::Value,
    pub observation: serde_json::Value,
    pub counts: serde_json::Value,
    pub children: Vec<serde_json::Value>,
    /// Bounded canonical child task rows from the coordinator home.
    #[serde(default)]
    pub tasks: Vec<serde_json::Value>,
    #[serde(default)]
    pub workflow_runs: Vec<serde_json::Value>,
    #[serde(default)]
    pub portfolio: Option<Portfolio>,
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
    #[serde(default)]
    pub records: Vec<serde_json::Value>,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub reason: Option<String>,
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
    #[serde(default)]
    pub valid: bool,
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

fn coordination_schema<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    let version = Option::<u32>::deserialize(deserializer)?;
    if version.is_some_and(|v| v != 2) {
        return Err(serde::de::Error::custom(
            "unsupported coordination projection schema",
        ));
    }
    Ok(version)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Task {
    #[serde(default, deserialize_with = "coordination_schema")]
    pub coordination_schema: Option<u32>,
    #[serde(default)]
    pub coordination: Option<crate::lifecycle::subagent_model::TaskRecord>,
    #[serde(default)]
    pub coordination_error: Option<String>,
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
    if snapshot
        .portfolio
        .as_ref()
        .is_some_and(|portfolio| portfolio.schema != "mx-portfolio.v1")
    {
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
        "# System View\n\nSchema: {}\nHome: {}\n",
        snapshot.schema, snapshot.mx_home
    );
    if let Some(portfolio) = &snapshot.portfolio {
        output.push_str(&format!(
            "\n## Portfolio\nUseful tasks: {} · coordinator records: {} · sessions: {} · attempts: {} · freshness: {}\n",
            portfolio.counts.tasks,
            portfolio.counts.coordinators,
            portfolio.counts.sessions,
            portfolio.counts.attempts,
            portfolio.freshness.status
        ));
        if portfolio.tasks.is_empty() {
            output.push_str("No accepted task records found.\n");
        } else {
            output.push_str(
                "| Task | State | Role | Project | Brief | Workflow stage | Freshness |\n",
            );
            output.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
            for task in &portfolio.tasks {
                output.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} | {} |\n",
                    task.id,
                    task.state,
                    dash(task.role.as_deref()),
                    dash(
                        task.project
                            .get("display_name")
                            .and_then(serde_json::Value::as_str)
                    ),
                    task.brief
                        .get("revision")
                        .and_then(serde_json::Value::as_u64)
                        .map_or_else(|| "unknown".into(), |value| value.to_string()),
                    dash(
                        task.workflow
                            .get("current_stage")
                            .and_then(serde_json::Value::as_str)
                    ),
                    task.freshness.status
                ));
            }
        }
    }
    output.push_str("\n## Under Way\n");
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
    output.push_str("\n## Domains\n");
    if snapshot.domains.records.is_empty() {
        output.push_str("No scoped coordinator domains found.\n");
    } else {
        output.push_str("| Domain | Coordinator | Scope revision | Useful tasks | Coordinator sessions | Undelivered | Health |\n");
        output.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
        for domain in &snapshot.domains.records {
            let count = |key: &str| domain.counts.get(key).and_then(serde_json::Value::as_u64);
            output.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                dash(domain.domain_id.as_deref()),
                dash(
                    domain
                        .coordinator
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                ),
                domain
                    .scope_revision
                    .map_or_else(|| "-".into(), |value| value.to_string()),
                count("useful_tasks").map_or_else(|| "unknown".into(), |value| value.to_string()),
                count("coordinator_sessions")
                    .map_or_else(|| "unknown".into(), |value| value.to_string()),
                count("undelivered_outcomes")
                    .map_or_else(|| "unknown".into(), |value| value.to_string()),
                if domain.observation.get("partial") == Some(&serde_json::Value::Bool(false)) {
                    "current"
                } else {
                    "partial"
                }
            ));
        }
        if snapshot.domains.truncated > 0 {
            output.push_str(&format!(
                "\n{} additional domain(s) omitted by the snapshot bound.\n",
                snapshot.domains.truncated
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

    #[test]
    fn typed_view_rejects_an_unsupported_portfolio_schema() {
        let error = serde_json::from_value::<Portfolio>(serde_json::json!({"schema":"future"}))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported canonical portfolio")
        );
    }
}

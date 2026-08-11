//! Conservative startup retirement of stale Herdr presentation children.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use multplx_core::identifiers::TaskId;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use serde_json::Value;

use crate::facade::BackendError;
use crate::herdr::{HerdrBackend, PaneAgentState};
use crate::herdr_presentation::{
    JOURNAL_SUFFIX, ProjectionJournal, home_identity, projection_workspace_label, read_journal,
};

const MAX_PS_OUTPUT: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct JournalMatch {
    path: PathBuf,
    task_id: String,
    journal: ProjectionJournal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Endpoint {
    tab_id: String,
    pane_id: String,
}

/// Run startup cleanup. Every discovery or proof failure preserves state and returns success.
pub fn run_session_cleanup() -> i32 {
    if let Err(error) = cleanup() {
        warn(&error.to_string());
    }
    0
}

fn cleanup() -> Result<(), BackendError> {
    let root = std::env::var_os("MX_ROOT_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let home = std::env::var_os("MX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.clone());
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if !real_directory(&state) || journal_paths(&state).is_empty() || !executable_available() {
        return Ok(());
    }
    let canonical_home = home_identity(&home)?;
    let mut backend = HerdrBackend::system();
    let session = backend.session().to_owned();
    let workspaces = match backend.json_scoped(&session, ["workspace", "list"]) {
        Ok(value) => array(&value, "/result/workspaces")?.to_vec(),
        Err(_) => {
            warn(&format!(
                "session '{session}' workspace discovery failed; preserving every candidate"
            ));
            return Ok(());
        }
    };
    for workspace in workspaces {
        let Some(workspace_id) = text(&workspace, "/workspace_id") else {
            continue;
        };
        let Some(title) = text(&workspace, "/label") else {
            continue;
        };
        cleanup_one(
            &mut backend,
            &state,
            &canonical_home,
            &session,
            workspace_id,
            title,
        );
    }
    Ok(())
}

fn cleanup_one(
    backend: &mut HerdrBackend,
    state: &Path,
    home: &Path,
    session: &str,
    workspace_id: &str,
    title: &str,
) {
    let Some(token) = title_token(title) else {
        return;
    };
    let Some(found) = unique_match(state, title, session, home) else {
        return;
    };
    if found.journal.projection_id() != token {
        return;
    }
    let task_lock_path = state.join(format!(".spawn-{}.lock", found.task_id));
    let processes = SystemProcessProbe::default();
    let _task_lock = match DirectoryLock::try_acquire(&task_lock_path, &processes) {
        Ok(lock) => lock,
        Err(_) => {
            warn(&format!(
                "{} skipped because its task lock is busy",
                found.task_id
            ));
            return;
        }
    };
    let presentation_path = match backend.presentation_session_lock_path(session) {
        Ok(path) => path,
        Err(_) => {
            warn(&format!(
                "{} skipped because the shared presentation lock is unavailable",
                found.task_id
            ));
            return;
        }
    };
    let _presentation_lock = match DirectoryLock::try_acquire(&presentation_path, &processes) {
        Ok(lock) => lock,
        Err(_) => {
            warn(&format!(
                "{} skipped because the shared presentation lock is busy",
                found.task_id
            ));
            return;
        }
    };
    if path_present(&state.join(format!("{}.meta", found.task_id))) {
        return;
    }
    let endpoint = match locked_snapshot_candidate(
        backend,
        session,
        workspace_id,
        title,
        token,
        &found.journal,
    ) {
        Some(endpoint) => endpoint,
        None => {
            warn(&format!(
                "{} preserved because its locked candidate snapshot was ambiguous",
                found.task_id
            ));
            return;
        }
    };
    if backend.pane_agent_state(session, &endpoint.pane_id) != PaneAgentState::NoAgent
        || !process_is_idle_shell(backend, session, &endpoint.pane_id)
    {
        warn(&format!(
            "{} preserved because its pane is not a provably idle childless shell",
            found.task_id
        ));
        return;
    }
    if !revalidate(
        backend,
        state,
        home,
        session,
        workspace_id,
        title,
        token,
        &found,
        &endpoint,
    ) {
        warn(&format!(
            "{} preserved because immediate revalidation changed or was unreadable",
            found.task_id
        ));
        return;
    }
    let close = backend.close_pane_focus_preserving(
        session,
        &endpoint.pane_id,
        Some(PaneAgentState::NoAgent),
    );
    if backend.pane_agent_state(session, &endpoint.pane_id) != PaneAgentState::Dead {
        warn(&format!(
            "{} preserved because exact pane closure was refused or unconfirmed",
            found.task_id
        ));
        return;
    }
    let unchanged = !path_present(&state.join(format!("{}.meta", found.task_id)))
        && unique_match(state, title, session, home).as_ref() == Some(&found);
    if unchanged {
        if let Err(error) = fs::remove_file(&found.path) {
            warn(&format!(
                "{} pane closed but its journal could not be retired: {error}",
                found.task_id
            ));
        }
    } else {
        warn(&format!(
            "{} pane closed but its journal changed and was preserved",
            found.task_id
        ));
    }
    if close.is_err() {
        warn(&format!(
            "{} exact pane close reported a failure after disappearance",
            found.task_id
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn revalidate(
    backend: &mut HerdrBackend,
    state: &Path,
    home: &Path,
    session: &str,
    workspace_id: &str,
    title: &str,
    token: &str,
    found: &JournalMatch,
    endpoint: &Endpoint,
) -> bool {
    if path_present(&state.join(format!("{}.meta", found.task_id)))
        || unique_match(state, title, session, home).as_ref() != Some(found)
    {
        return false;
    }
    let Ok(workspaces) = backend.json_scoped(session, ["workspace", "list"]) else {
        return false;
    };
    let Ok(all_workspaces) = array(&workspaces, "/result/workspaces") else {
        return false;
    };
    if all_workspaces
        .iter()
        .filter(|workspace| {
            text(workspace, "/workspace_id") == Some(workspace_id)
                && text(workspace, "/label") == Some(title)
        })
        .count()
        != 1
        || token_occurrences(all_workspaces, token) != 1
    {
        return false;
    }
    let Ok(workspace) = backend.json_scoped(session, ["workspace", "get", workspace_id]) else {
        return false;
    };
    if text(&workspace, "/result/workspace/workspace_id") != Some(workspace_id)
        || text(&workspace, "/result/workspace/label") != Some(title)
        || number(&workspace, "/result/workspace/tab_count") != Some(1)
        || number(&workspace, "/result/workspace/pane_count") != Some(1)
    {
        return false;
    }
    let Ok(tabs) = backend.json_scoped(session, ["tab", "list", "--workspace", workspace_id])
    else {
        return false;
    };
    let Ok(tabs) = array(&tabs, "/result/tabs") else {
        return false;
    };
    if tabs.len() != 1
        || text(&tabs[0], "/workspace_id") != Some(workspace_id)
        || text(&tabs[0], "/tab_id") != Some(endpoint.tab_id.as_str())
    {
        return false;
    }
    let Ok(panes) = backend.json_scoped(session, ["pane", "list", "--workspace", workspace_id])
    else {
        return false;
    };
    let Ok(panes) = array(&panes, "/result/panes") else {
        return false;
    };
    if panes.len() != 1
        || text(&panes[0], "/workspace_id") != Some(workspace_id)
        || text(&panes[0], "/tab_id") != Some(endpoint.tab_id.as_str())
        || text(&panes[0], "/pane_id") != Some(endpoint.pane_id.as_str())
    {
        return false;
    }
    backend.pane_agent_state(session, &endpoint.pane_id) == PaneAgentState::NoAgent
        && process_is_idle_shell(backend, session, &endpoint.pane_id)
        && backend
            .focus_snapshot(session)
            .is_ok_and(|focus| focus.tab_id != endpoint.tab_id)
}

fn locked_snapshot_candidate(
    backend: &mut HerdrBackend,
    session: &str,
    workspace_id: &str,
    title: &str,
    token: &str,
    journal: &ProjectionJournal,
) -> Option<Endpoint> {
    let snapshot = backend.json_scoped(session, ["api", "snapshot"]).ok()?;
    let root = snapshot.pointer("/result/snapshot")?;
    let workspaces = array(root, "/workspaces").ok()?;
    let tabs = array(root, "/tabs").ok()?;
    let panes = array(root, "/panes").ok()?;
    let matching_workspaces = workspaces
        .iter()
        .filter(|workspace| text(workspace, "/workspace_id") == Some(workspace_id))
        .collect::<Vec<_>>();
    let matching_tabs = tabs
        .iter()
        .filter(|tab| text(tab, "/workspace_id") == Some(workspace_id))
        .collect::<Vec<_>>();
    let matching_panes = panes
        .iter()
        .filter(|pane| text(pane, "/workspace_id") == Some(workspace_id))
        .collect::<Vec<_>>();
    if matching_workspaces.len() != 1
        || matching_tabs.len() != 1
        || matching_panes.len() != 1
        || text(matching_workspaces[0], "/label") != Some(title)
        || number(matching_workspaces[0], "/tab_count") != Some(1)
        || number(matching_workspaces[0], "/pane_count") != Some(1)
        || token_occurrences(workspaces, token) != 1
        || text(root, "/focused_workspace_id").is_none()
        || text(root, "/focused_tab_id").is_none()
        || text(root, "/focused_pane_id").is_none()
    {
        return None;
    }
    let tab_id = text(matching_tabs[0], "/tab_id")?;
    let pane_id = text(matching_panes[0], "/pane_id")?;
    if text(matching_panes[0], "/tab_id") != Some(tab_id)
        || text(root, "/focused_tab_id") == Some(tab_id)
    {
        return None;
    }
    if let ProjectionJournal::V2(binding) = journal
        && (binding.workspace_id != workspace_id
            || binding.tab_id != tab_id
            || binding.pane_id != pane_id)
    {
        return None;
    }
    Some(Endpoint {
        tab_id: tab_id.to_owned(),
        pane_id: pane_id.to_owned(),
    })
}

fn process_is_idle_shell(backend: &mut HerdrBackend, session: &str, pane_id: &str) -> bool {
    let Ok(info) = backend.json_scoped(session, ["pane", "process-info", "--pane", pane_id]) else {
        return false;
    };
    if text(&info, "/result/type") != Some("pane_process_info")
        || text(&info, "/result/process_info/pane_id") != Some(pane_id)
    {
        return false;
    }
    let Some(shell_pid) = number(&info, "/result/process_info/shell_pid") else {
        return false;
    };
    let Some(foreground_pgid) = number(&info, "/result/process_info/foreground_process_group_id")
    else {
        return false;
    };
    if shell_pid <= 1 || shell_pid != foreground_pgid {
        return false;
    }
    let Ok(processes) = array(&info, "/result/process_info/foreground_processes") else {
        return false;
    };
    if processes.len() != 1 || number(&processes[0], "/pid") != Some(shell_pid) {
        return false;
    }
    let Some(name) = text(&processes[0], "/name") else {
        return false;
    };
    let argv0 = text(&processes[0], "/argv0")
        .or_else(|| processes[0].pointer("/argv/0").and_then(Value::as_str));
    let Some(argv0) = argv0 else {
        return false;
    };
    let shell_name = Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(name);
    let argv_name = Path::new(argv0.trim_start_matches('-'))
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(argv0);
    if shell_name != argv_name
        || !matches!(shell_name, "sh" | "bash" | "zsh" | "dash" | "ksh" | "fish")
    {
        return false;
    }
    ps_proves_idle(shell_pid)
}

fn ps_proves_idle(shell_pid: u64) -> bool {
    let program = std::env::var_os("MX_HERDR_PS_BIN").unwrap_or_else(|| OsString::from("ps"));
    let Ok(rows) = Command::new(&program)
        .args(["-axo", "pid=,ppid="])
        .env("LC_ALL", "C")
        .output()
    else {
        return false;
    };
    if !rows.status.success() || rows.stdout.len() > MAX_PS_OUTPUT {
        return false;
    }
    let Ok(rows) = std::str::from_utf8(&rows.stdout) else {
        return false;
    };
    let mut found = 0;
    let mut child = 0;
    for row in rows.lines() {
        let fields = row.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 2 {
            continue;
        }
        if fields[0].parse::<u64>().ok() == Some(shell_pid) {
            found += 1;
        }
        if fields[1].parse::<u64>().ok() == Some(shell_pid) {
            child += 1;
        }
    }
    if found != 1 || child != 0 {
        return false;
    }
    let Ok(stat) = Command::new(program)
        .args(["-p", &shell_pid.to_string(), "-o", "stat="])
        .env("LC_ALL", "C")
        .output()
    else {
        return false;
    };
    stat.status.success()
        && stat.stdout.len() <= MAX_PS_OUTPUT
        && std::str::from_utf8(&stat.stdout)
            .ok()
            .and_then(|value| value.trim().chars().next())
            .is_some_and(|state| matches!(state, 'S' | 'I'))
}

fn unique_match(state: &Path, title: &str, session: &str, home: &Path) -> Option<JournalMatch> {
    let mut matches = Vec::new();
    for path in journal_paths(state) {
        let filename = path.file_name()?.to_str()?;
        let task_id = filename.strip_suffix(JOURNAL_SUFFIX)?.to_owned();
        if TaskId::parse(&task_id).is_err() {
            continue;
        }
        let Ok(journal) = read_journal(&path, &task_id) else {
            continue;
        };
        if let ProjectionJournal::V2(binding) = &journal
            && (home_identity(&binding.home).ok().as_deref() != Some(home)
                || binding.session != session)
        {
            continue;
        }
        if projection_workspace_label(&task_id, journal.projection_id()) == title {
            matches.push(JournalMatch {
                path,
                task_id,
                journal,
            });
        }
    }
    (matches.len() == 1).then(|| matches.remove(0))
}

fn journal_paths(state: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(state) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(JOURNAL_SUFFIX))
                && fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        })
        .collect()
}

fn title_token(title: &str) -> Option<&str> {
    let (_, token) = title.strip_prefix("└ ")?.rsplit_once(" · p:")?;
    if token.len() == 22
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && title.matches("p:").count() == 1
    {
        Some(token)
    } else {
        None
    }
}

fn token_occurrences(workspaces: &[Value], token: &str) -> usize {
    let needle = format!("p:{token}");
    workspaces
        .iter()
        .filter_map(|workspace| text(workspace, "/label"))
        .map(|label| label.matches(&needle).count())
        .sum()
}

fn array<'a>(value: &'a Value, pointer: &str) -> Result<&'a [Value], BackendError> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| BackendError::Malformed(format!("missing array at {pointer}")))
}

fn text<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn number(value: &Value, pointer: &str) -> Option<u64> {
    value.pointer(pointer).and_then(Value::as_u64)
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn path_present(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn executable_available() -> bool {
    let executable = std::env::var_os("MX_HERDR_BIN").unwrap_or_else(|| OsString::from("herdr"));
    let executable = PathBuf::from(executable);
    if executable.components().count() > 1 {
        return executable.is_file();
    }
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join(&executable).is_file())
    })
}

fn warn(message: &str) {
    eprintln!("warning: herdr session-start projection cleanup: {message}");
}

#[cfg(test)]
mod tests {
    use super::{title_token, token_occurrences};
    use serde_json::json;

    #[test]
    fn title_grammar_is_exact() {
        let token = "Abcdefghijklmnopqrstu_";
        assert_eq!(title_token(&format!("└ task · p:{token}")), Some(token));
        assert_eq!(title_token(&format!("task · p:{token}")), None);
        assert_eq!(title_token(&format!("└ task p:x · p:{token}")), None);
    }

    #[test]
    fn token_count_spans_every_workspace_label() {
        let values = vec![json!({"label": "a p:token"}), json!({"label": "b p:token"})];
        assert_eq!(token_occurrences(&values, "token"), 2);
    }
}

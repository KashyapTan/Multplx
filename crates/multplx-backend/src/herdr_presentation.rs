//! Herdr presentation projection identity, focus, ordering, and journal safety.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use multplx_core::filesystem::{atomic_replace, read_bounded_regular};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::command::{CommandRequest, CommandRunner, SystemCommandRunner};
use crate::facade::{BackendError, BackendName, RuntimeBackend};
use crate::herdr::{HerdrBackend, PaneAgentState};
use crate::herdr_wire;

/// Durable projection-journal suffix.
pub const JOURNAL_SUFFIX: &str = ".herdr-presentation";

/// One exact focus identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusSnapshot {
    /// Focused workspace id.
    pub workspace_id: String,
    /// Focused tab id.
    pub tab_id: String,
}

impl FocusSnapshot {
    /// Render the legacy tab-separated record.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}\t{}", self.workspace_id, self.tab_id)
    }
}

/// Versioned, non-authoritative projection binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectionJournal {
    /// Attempt identity published before create.
    V1 {
        /// Task id.
        task_id: String,
        /// Random 128-bit base64url projection id.
        projection_id: String,
    },
    /// Exact successfully created projection identity.
    V2(Box<ProjectionBinding>),
}

/// Version-2 exact projection binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionBinding {
    /// Task id.
    pub task_id: String,
    /// Random visual correlator.
    pub projection_id: String,
    /// Canonical physical Multplx home.
    pub home: PathBuf,
    /// Exact named Herdr session.
    pub session: String,
    /// Exact child workspace id.
    pub workspace_id: String,
    /// Exact task tab id.
    pub tab_id: String,
    /// Exact task pane id.
    pub pane_id: String,
    /// Exact parent workspace id.
    pub parent_workspace_id: String,
    /// Immutable expected parent label.
    pub parent_label: String,
    /// Immutable expected projection label.
    pub workspace_label: String,
    /// Immutable expected task label.
    pub task_label: String,
}

impl ProjectionJournal {
    /// Return the projection token.
    #[must_use]
    pub fn projection_id(&self) -> &str {
        match self {
            Self::V1 { projection_id, .. } => projection_id,
            Self::V2(binding) => &binding.projection_id,
        }
    }

    /// Return the task id.
    #[must_use]
    pub fn task_id(&self) -> &str {
        match self {
            Self::V1 { task_id, .. } => task_id,
            Self::V2(binding) => &binding.task_id,
        }
    }

    /// Render exact line-oriented journal bytes.
    #[must_use]
    pub fn render(&self) -> Vec<u8> {
        match self {
            Self::V1 {
                task_id,
                projection_id,
            } => format!("version=1\ntask_id={task_id}\nprojection_id={projection_id}\n")
                .into_bytes(),
            Self::V2(binding) => format!(
                concat!(
                    "version=2\n",
                    "task_id={}\n",
                    "projection_id={}\n",
                    "home={}\n",
                    "session={}\n",
                    "workspace_id={}\n",
                    "tab_id={}\n",
                    "pane_id={}\n",
                    "parent_workspace_id={}\n",
                    "parent_label={}\n",
                    "workspace_label={}\n",
                    "task_label={}\n"
                ),
                binding.task_id,
                binding.projection_id,
                binding.home.display(),
                binding.session,
                binding.workspace_id,
                binding.tab_id,
                binding.pane_id,
                binding.parent_workspace_id,
                binding.parent_label,
                binding.workspace_label,
                binding.task_label
            )
            .into_bytes(),
        }
    }
}

/// Full exact response-derived projection create result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionEndpoint {
    /// Named session.
    pub session: String,
    /// Disposable workspace id.
    pub workspace_id: String,
    /// Auto-seeded tab id.
    pub seeded_tab_id: String,
    /// Auto-seeded pane id.
    pub seeded_pane_id: String,
    /// Task tab id.
    pub tab_id: String,
    /// Task pane id.
    pub pane_id: String,
}

/// Result of attempting to replace an exact restored projection husk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReclaimOutcome {
    /// Exact replacement succeeded and the journal now binds this endpoint.
    Reclaimed { tab_id: String, pane_id: String },
    /// No live agent was found and a non-mutating flat fallback is safe.
    Flat,
    /// Mutation safety became uncertain or a live/unknown agent was observed.
    Refuse,
}

/// Return one journal's path.
#[must_use]
pub fn journal_path(state: &Path, task_id: &str) -> PathBuf {
    state.join(format!("{task_id}{JOURNAL_SUFFIX}"))
}

/// Generate a compact 128-bit base64url projection correlator.
pub fn projection_id() -> Result<String, BackendError> {
    use std::io::Read;

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut input = [0_u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut input))
        .map_err(|error| BackendError::Metadata(error.to_string()))?;
    let mut output = String::with_capacity(22);
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for byte in input {
        accumulator = (accumulator << 8) | u32::from(byte);
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            output.push(ALPHABET[((accumulator >> bits) & 0x3f) as usize] as char);
        }
    }
    if bits > 0 {
        output.push(ALPHABET[((accumulator << (6 - bits)) & 0x3f) as usize] as char);
    }
    if valid_token(&output) {
        Ok(output)
    } else {
        Err(BackendError::Metadata(
            "could not generate projection id".to_owned(),
        ))
    }
}

/// Validate a task id before using it as a journal filename.
fn valid_task_id(task_id: &str) -> bool {
    !task_id.is_empty()
        && !task_id.starts_with('.')
        && task_id.len() <= 64
        && task_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Create one version-1 journal with create-if-absent semantics and mode 0600.
pub fn create_journal(
    state: &Path,
    task_id: &str,
    projection_id: &str,
) -> Result<PathBuf, BackendError> {
    if !valid_task_id(task_id) || !valid_token(projection_id) {
        return Err(BackendError::Metadata(
            "invalid Herdr presentation journal identity".to_owned(),
        ));
    }
    fs::create_dir_all(state).map_err(|error| BackendError::Metadata(error.to_string()))?;
    let path = journal_path(state, task_id);
    let journal = ProjectionJournal::V1 {
        task_id: task_id.to_owned(),
        projection_id: projection_id.to_owned(),
    };
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| BackendError::Metadata(format!("{}: {error}", path.display())))?;
    file.write_all(&journal.render())
        .and_then(|()| file.sync_all())
        .map_err(|error| BackendError::Metadata(error.to_string()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|error| BackendError::Metadata(error.to_string()))?;
    Ok(path)
}

/// Parse and validate exactly one versioned journal.
pub fn read_journal(path: &Path, expected_task: &str) -> Result<ProjectionJournal, BackendError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| BackendError::Metadata(error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(BackendError::Metadata(
            "presentation journal is not a regular file".to_owned(),
        ));
    }
    let bytes = read_bounded_regular(path, 64 * 1024)
        .map_err(|error| BackendError::Metadata(error.to_string()))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| BackendError::Metadata("journal is not UTF-8".to_owned()))?;
    let mut fields = BTreeMap::<&str, &str>::new();
    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| BackendError::Metadata("malformed journal row".to_owned()))?;
        if fields.insert(key, value).is_some() {
            return Err(BackendError::Metadata("duplicate journal field".to_owned()));
        }
    }
    let exact = |key: &str| {
        fields
            .get(key)
            .copied()
            .ok_or_else(|| BackendError::Metadata(format!("missing journal field {key}")))
    };
    let task_id = exact("task_id")?;
    let token = exact("projection_id")?;
    if task_id != expected_task || !valid_task_id(task_id) || !valid_token(token) {
        return Err(BackendError::Metadata(
            "journal identity does not match".to_owned(),
        ));
    }
    match (exact("version")?, fields.len()) {
        ("1", 3) => Ok(ProjectionJournal::V1 {
            task_id: task_id.to_owned(),
            projection_id: token.to_owned(),
        }),
        ("2", 12) => {
            let home = PathBuf::from(exact("home")?);
            let binding = ProjectionBinding {
                task_id: task_id.to_owned(),
                projection_id: token.to_owned(),
                home,
                session: exact_field(&exact, "session")?,
                workspace_id: exact_field(&exact, "workspace_id")?,
                tab_id: exact_field(&exact, "tab_id")?,
                pane_id: exact_field(&exact, "pane_id")?,
                parent_workspace_id: exact_field(&exact, "parent_workspace_id")?,
                parent_label: exact_field(&exact, "parent_label")?,
                workspace_label: exact_field(&exact, "workspace_label")?,
                task_label: exact_field(&exact, "task_label")?,
            };
            if !binding.home.is_absolute()
                || projection_workspace_label(task_id, token) != binding.workspace_label
                || format!("mx-{task_id}") != binding.task_label
            {
                return Err(BackendError::Metadata(
                    "version 2 journal binding is inconsistent".to_owned(),
                ));
            }
            Ok(ProjectionJournal::V2(Box::new(binding)))
        }
        _ => Err(BackendError::Metadata(
            "unsupported journal version or field count".to_owned(),
        )),
    }
}

fn exact_field<'a>(
    exact: &impl Fn(&str) -> Result<&'a str, BackendError>,
    key: &str,
) -> Result<String, BackendError> {
    let value = exact(key)?;
    if value.is_empty() || value.chars().any(char::is_control) {
        Err(BackendError::Metadata(format!(
            "invalid journal field {key}"
        )))
    } else {
        Ok(value.to_owned())
    }
}

/// Atomically upgrade an exact version-1 attempt to a version-2 binding.
pub fn bind_journal(path: &Path, binding: ProjectionBinding) -> Result<(), BackendError> {
    match read_journal(path, &binding.task_id)? {
        ProjectionJournal::V1 { projection_id, .. } if projection_id == binding.projection_id => {}
        _ => {
            return Err(BackendError::Metadata(
                "journal is not the exact version 1 attempt".to_owned(),
            ));
        }
    }
    write_journal_v2(path, binding)
}

/// Atomically publish a fully validated version-2 journal over an existing regular file.
pub fn write_journal_v2(path: &Path, binding: ProjectionBinding) -> Result<(), BackendError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| BackendError::Metadata(error.to_string()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || !valid_task_id(&binding.task_id)
        || !valid_token(&binding.projection_id)
        || !binding.home.is_absolute()
        || projection_workspace_label(&binding.task_id, &binding.projection_id)
            != binding.workspace_label
        || format!("mx-{}", binding.task_id) != binding.task_label
    {
        return Err(BackendError::Metadata(
            "invalid version 2 presentation binding".to_owned(),
        ));
    }
    for value in [
        &binding.session,
        &binding.workspace_id,
        &binding.tab_id,
        &binding.pane_id,
        &binding.parent_workspace_id,
        &binding.parent_label,
        &binding.workspace_label,
        &binding.task_label,
    ] {
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(BackendError::Metadata(
                "invalid version 2 presentation field".to_owned(),
            ));
        }
    }
    for value in [
        &binding.session,
        &binding.workspace_id,
        &binding.tab_id,
        &binding.pane_id,
        &binding.parent_workspace_id,
    ] {
        checked_identity(value)?;
    }
    atomic_replace(
        path,
        &ProjectionJournal::V2(Box::new(binding)).render(),
        0o600,
    )
    .map_err(|error| BackendError::Metadata(error.to_string()))
}

/// Atomically advance only the exact currently-bound endpoint.
pub fn replace_journal_endpoint(
    path: &Path,
    task_id: &str,
    old_tab: &str,
    old_pane: &str,
    new_tab: &str,
    new_pane: &str,
) -> Result<(), BackendError> {
    let ProjectionJournal::V2(mut binding) = read_journal(path, task_id)? else {
        return Err(BackendError::Metadata(
            "journal is not version 2".to_owned(),
        ));
    };
    if binding.tab_id != old_tab || binding.pane_id != old_pane {
        return Err(BackendError::Metadata(
            "journal endpoint changed concurrently".to_owned(),
        ));
    }
    binding.tab_id = checked_identity(new_tab)?;
    binding.pane_id = checked_identity(new_pane)?;
    write_journal_v2(path, *binding)
}

fn checked_identity(value: &str) -> Result<String, BackendError> {
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character == '\0')
    {
        Err(BackendError::Metadata("invalid exact identity".to_owned()))
    } else {
        Ok(value.to_owned())
    }
}

/// Strip redundant owner prefixes for the presentation-only label.
#[must_use]
pub fn concise_task_label(task_id: &str) -> &str {
    let task = task_id
        .strip_prefix("broker/")
        .or_else(|| {
            task_id
                .strip_prefix("daemon-")
                .and_then(|rest| rest.split_once('/').map(|(_, task)| task))
        })
        .unwrap_or(task_id);
    task.strip_prefix("mx-").unwrap_or(task)
}

/// Construct the exact current projection label.
#[must_use]
pub fn projection_workspace_label(task_id: &str, projection_id: &str) -> String {
    format!("└ {} · p:{}", concise_task_label(task_id), projection_id)
}

fn valid_token(token: &str) -> bool {
    token.len() == 22
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn child_label_for_parent(label: Option<&str>, parent_label: &str) -> bool {
    let Some(label) = label else {
        return false;
    };
    let new_child = label
        .strip_prefix("└ ")
        .and_then(|rest| rest.rsplit_once(" · p:"))
        .is_some_and(|(task, token)| !task.is_empty() && valid_token(token));
    let legacy_child = label
        .strip_prefix(parent_label)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.rsplit_once(" · p:"))
        .is_some_and(|(task, token)| !task.is_empty() && valid_token(token));
    new_child || legacy_child
}

/// Canonicalize one physical home identity.
pub fn home_identity(home: &Path) -> Result<PathBuf, BackendError> {
    let metadata =
        fs::symlink_metadata(home).map_err(|error| BackendError::Metadata(error.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BackendError::Metadata(
            "home must be a real directory".to_owned(),
        ));
    }
    fs::canonicalize(home).map_err(|error| BackendError::Metadata(error.to_string()))
}

impl<R: CommandRunner> HerdrBackend<R> {
    /// Verify the exact bound projection topology and contiguous child position.
    pub fn projection_live_binding_matches(
        &mut self,
        session: &str,
        binding: &ProjectionBinding,
    ) -> bool {
        if binding.session != session {
            return false;
        }
        let Ok(list) = self.json_scoped(session, ["workspace", "list"]) else {
            return false;
        };
        let Ok(spaces) = array(&list, "/result/workspaces") else {
            return false;
        };
        let workspace_matches = spaces
            .iter()
            .filter(|space| string_at(space, "/workspace_id") == Some(&binding.workspace_id))
            .collect::<Vec<_>>();
        let parent_matches = spaces
            .iter()
            .filter(|space| string_at(space, "/label") == Some(&binding.parent_label))
            .collect::<Vec<_>>();
        let token_suffix = format!(" · p:{}", binding.projection_id);
        let token_matches = spaces
            .iter()
            .filter(|space| {
                string_at(space, "/label").is_some_and(|label| label.ends_with(&token_suffix))
            })
            .collect::<Vec<_>>();
        if workspace_matches.len() != 1
            || string_at(workspace_matches[0], "/label") != Some(&binding.workspace_label)
            || parent_matches.len() != 1
            || string_at(parent_matches[0], "/workspace_id") != Some(&binding.parent_workspace_id)
            || token_matches.len() != 1
            || string_at(token_matches[0], "/workspace_id") != Some(&binding.workspace_id)
        {
            return false;
        }
        let Some(parent_index) = spaces.iter().position(|space| {
            string_at(space, "/workspace_id") == Some(&binding.parent_workspace_id)
        }) else {
            return false;
        };
        let Some(child_index) = spaces
            .iter()
            .position(|space| string_at(space, "/workspace_id") == Some(&binding.workspace_id))
        else {
            return false;
        };
        if child_index <= parent_index
            || !spaces[parent_index + 1..child_index].iter().all(|space| {
                child_label_for_parent(string_at(space, "/label"), &binding.parent_label)
            })
        {
            return false;
        }
        let Ok(tabs) = self.json_scoped(
            session,
            ["tab", "list", "--workspace", &binding.workspace_id],
        ) else {
            return false;
        };
        let Ok(tabs) = array(&tabs, "/result/tabs") else {
            return false;
        };
        if tabs.len() != 1
            || string_at(&tabs[0], "/tab_id") != Some(&binding.tab_id)
            || string_at(&tabs[0], "/label") != Some(&binding.task_label)
        {
            return false;
        }
        let Ok(panes) = self.json_scoped(
            session,
            ["pane", "list", "--workspace", &binding.workspace_id],
        ) else {
            return false;
        };
        let Ok(panes) = array(&panes, "/result/panes") else {
            return false;
        };
        panes.len() == 1
            && string_at(&panes[0], "/pane_id") == Some(&binding.pane_id)
            && string_at(&panes[0], "/tab_id") == Some(&binding.tab_id)
    }

    /// Allow a flat fallback only when every exact token match is dead or agent-free.
    pub fn projection_recovery_allows_flat(
        &mut self,
        session: &str,
        journal: &Path,
        task_id: &str,
    ) -> bool {
        let Ok(journal) = read_journal(journal, task_id) else {
            return false;
        };
        if self.server_ensure(session).is_err() {
            return false;
        }
        let Ok(list) = self.json_scoped(session, ["workspace", "list"]) else {
            return false;
        };
        let Ok(spaces) = array(&list, "/result/workspaces") else {
            return false;
        };
        let suffix = format!(" · p:{}", journal.projection_id());
        let workspace_ids = spaces
            .iter()
            .filter(|space| {
                string_at(space, "/label").is_some_and(|label| label.ends_with(&suffix))
            })
            .filter_map(|space| string_at(space, "/workspace_id"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for workspace in workspace_ids {
            let Ok(panes) =
                self.json_scoped(session, ["pane", "list", "--workspace", workspace.as_str()])
            else {
                return false;
            };
            let Ok(panes) = array(&panes, "/result/panes") else {
                return false;
            };
            if panes
                .iter()
                .filter_map(|pane| string_at(pane, "/pane_id"))
                .any(|pane| {
                    !matches!(
                        self.pane_agent_state(session, pane),
                        PaneAgentState::Dead | PaneAgentState::NoAgent
                    )
                })
            {
                return false;
            }
        }
        true
    }

    /// Correlate an endpoint to exactly one token-bearing workspace without mutation.
    pub fn projection_endpoint_matches_journal(
        &mut self,
        session: &str,
        workspace_id: &str,
        journal: &Path,
        task_id: &str,
    ) -> bool {
        let Ok(journal) = read_journal(journal, task_id) else {
            return false;
        };
        let Ok(list) = self.json_scoped(session, ["workspace", "list"]) else {
            return false;
        };
        let Ok(spaces) = array(&list, "/result/workspaces") else {
            return false;
        };
        let suffix = format!(" · p:{}", journal.projection_id());
        let matches = spaces
            .iter()
            .filter(|space| {
                string_at(space, "/label").is_some_and(|label| label.ends_with(&suffix))
            })
            .filter_map(|space| string_at(space, "/workspace_id"))
            .collect::<Vec<_>>();
        matches == [workspace_id]
    }

    /// Replace one exact agent-free restored husk, rolling back any new pane on refusal.
    #[allow(clippy::too_many_arguments)]
    pub fn projection_reclaim_task(
        &mut self,
        session: &str,
        journal_path: &Path,
        task_id: &str,
        home: &Path,
        meta_workspace: &str,
        meta_tab: &str,
        meta_pane: &str,
        parent_label: &str,
        task_label: &str,
        cwd: &Path,
    ) -> ReclaimOutcome {
        let Ok(ProjectionJournal::V2(binding)) = read_journal(journal_path, task_id) else {
            return ReclaimOutcome::Flat;
        };
        let Ok(canonical_home) = home_identity(home) else {
            return ReclaimOutcome::Flat;
        };
        if binding.home != canonical_home
            || binding.session != session
            || binding.workspace_id != meta_workspace
            || binding.tab_id != meta_tab
            || binding.pane_id != meta_pane
            || binding.parent_label != parent_label
            || binding.task_label != task_label
            || !self.projection_live_binding_matches(session, &binding)
        {
            return ReclaimOutcome::Flat;
        }
        match self.pane_agent_state(session, meta_pane) {
            PaneAgentState::NoAgent => {}
            PaneAgentState::Dead => return ReclaimOutcome::Flat,
            PaneAgentState::Live | PaneAgentState::Unknown => return ReclaimOutcome::Refuse,
        }
        let Ok(focus) = self.focus_snapshot(session) else {
            return ReclaimOutcome::Flat;
        };
        if focus.tab_id == meta_tab {
            return ReclaimOutcome::Flat;
        }
        let created = self.json_scoped(
            session,
            [
                OsString::from("tab"),
                OsString::from("create"),
                OsString::from("--workspace"),
                OsString::from(meta_workspace),
                OsString::from("--cwd"),
                cwd.as_os_str().to_owned(),
                OsString::from("--label"),
                OsString::from(task_label),
                OsString::from("--no-focus"),
            ],
        );
        let Ok(created) = created else {
            return if self.focus_restore(session, &focus).is_ok() {
                ReclaimOutcome::Flat
            } else {
                ReclaimOutcome::Refuse
            };
        };
        let Some(new_tab) = string_at(&created, "/result/tab/tab_id").map(str::to_owned) else {
            return if self.focus_restore(session, &focus).is_ok() {
                ReclaimOutcome::Flat
            } else {
                ReclaimOutcome::Refuse
            };
        };
        let Some(new_pane) = string_at(&created, "/result/root_pane/pane_id").map(str::to_owned)
        else {
            return if self.focus_restore(session, &focus).is_ok() {
                ReclaimOutcome::Flat
            } else {
                ReclaimOutcome::Refuse
            };
        };
        if self.focus_restore(session, &focus).is_err() {
            return ReclaimOutcome::Refuse;
        }
        let valid_tab = self
            .json_scoped(session, ["tab", "get", &new_tab])
            .is_ok_and(|value| {
                string_at(&value, "/result/tab/tab_id") == Some(&new_tab)
                    && string_at(&value, "/result/tab/workspace_id") == Some(meta_workspace)
            });
        let valid_pane = self
            .json_scoped(session, ["pane", "get", &new_pane])
            .is_ok_and(|value| {
                string_at(&value, "/result/pane/pane_id") == Some(&new_pane)
                    && string_at(&value, "/result/pane/tab_id") == Some(&new_tab)
                    && string_at(&value, "/result/pane/workspace_id") == Some(meta_workspace)
            });
        if !valid_tab || !valid_pane {
            return self.rollback_reclaim(session, &new_pane);
        }
        match self.pane_agent_state(session, meta_pane) {
            PaneAgentState::NoAgent => {}
            PaneAgentState::Dead => return self.rollback_reclaim(session, &new_pane),
            PaneAgentState::Live | PaneAgentState::Unknown => {
                let _ = self.rollback_reclaim(session, &new_pane);
                return ReclaimOutcome::Refuse;
            }
        }
        if self
            .close_pane_focus_preserving(session, meta_pane, Some(PaneAgentState::NoAgent))
            .is_err()
        {
            let old_state = self.pane_agent_state(session, meta_pane);
            let rollback = self.rollback_reclaim(session, &new_pane);
            return if matches!(old_state, PaneAgentState::Live | PaneAgentState::Unknown)
                || rollback == ReclaimOutcome::Refuse
            {
                ReclaimOutcome::Refuse
            } else {
                ReclaimOutcome::Flat
            };
        }
        if self.pane_agent_state(session, meta_pane) != PaneAgentState::Dead {
            let _ = self.rollback_reclaim(session, &new_pane);
            return ReclaimOutcome::Refuse;
        }
        let mut replacement = binding.clone();
        replacement.tab_id = new_tab.clone();
        replacement.pane_id = new_pane.clone();
        if !self.projection_live_binding_matches(session, &replacement)
            || replace_journal_endpoint(
                journal_path,
                task_id,
                meta_tab,
                meta_pane,
                &new_tab,
                &new_pane,
            )
            .is_err()
        {
            return self.rollback_reclaim(session, &new_pane);
        }
        ReclaimOutcome::Reclaimed {
            tab_id: new_tab,
            pane_id: new_pane,
        }
    }

    fn rollback_reclaim(&mut self, session: &str, pane: &str) -> ReclaimOutcome {
        match self.pane_agent_state(session, pane) {
            PaneAgentState::Dead => ReclaimOutcome::Flat,
            PaneAgentState::NoAgent => {
                if self
                    .close_pane_focus_preserving(session, pane, Some(PaneAgentState::NoAgent))
                    .is_ok()
                    && self.pane_agent_state(session, pane) == PaneAgentState::Dead
                {
                    ReclaimOutcome::Flat
                } else {
                    ReclaimOutcome::Refuse
                }
            }
            PaneAgentState::Live | PaneAgentState::Unknown => ReclaimOutcome::Refuse,
        }
    }

    /// Capture and verify the exact active workspace and tab.
    pub fn focus_snapshot(&mut self, session: &str) -> Result<FocusSnapshot, BackendError> {
        let value = self.json_scoped(session, ["workspace", "list"])?;
        let focused = array(&value, "/result/workspaces")?
            .iter()
            .filter(|workspace| bool_at(workspace, "/focused") == Some(true))
            .collect::<Vec<_>>();
        if focused.len() != 1 {
            return Err(BackendError::Malformed(
                "expected one focused Herdr workspace".to_owned(),
            ));
        }
        let snapshot = FocusSnapshot {
            workspace_id: required(focused[0], "/workspace_id")?,
            tab_id: required(focused[0], "/active_tab_id")?,
        };
        let tabs = self.json_scoped(
            session,
            ["tab", "list", "--workspace", &snapshot.workspace_id],
        )?;
        let focused_tabs = array(&tabs, "/result/tabs")?
            .iter()
            .filter(|tab| bool_at(tab, "/focused") == Some(true))
            .collect::<Vec<_>>();
        if focused_tabs.len() != 1
            || string_at(focused_tabs[0], "/tab_id") != Some(snapshot.tab_id.as_str())
        {
            return Err(BackendError::Malformed(
                "focused workspace and tab disagree".to_owned(),
            ));
        }
        Ok(snapshot)
    }

    /// Restore the exact pre-operation focus if Herdr moved it.
    pub fn focus_restore(
        &mut self,
        session: &str,
        before: &FocusSnapshot,
    ) -> Result<(), BackendError> {
        if self.focus_snapshot(session).ok().as_ref() == Some(before) {
            return Ok(());
        }
        let tab = self.json_scoped(session, ["tab", "get", &before.tab_id])?;
        if string_at(&tab, "/result/tab/workspace_id") != Some(before.workspace_id.as_str())
            || string_at(&tab, "/result/tab/tab_id") != Some(before.tab_id.as_str())
        {
            return Err(BackendError::Malformed(
                "exact prior tab could not be verified".to_owned(),
            ));
        }
        self.success_scoped(session, ["tab", "focus", &before.tab_id])?;
        if self.focus_snapshot(session).ok().as_ref() == Some(before) {
            Ok(())
        } else {
            Err(BackendError::Command(
                "exact prior workspace and tab were not restored".to_owned(),
            ))
        }
    }

    /// Close an exact non-active pane and restore the prior exact focus.
    pub fn close_pane_focus_preserving(
        &mut self,
        session: &str,
        pane_id: &str,
        required_state: Option<PaneAgentState>,
    ) -> Result<(), BackendError> {
        let before = self.focus_snapshot(session)?;
        let pane = self.json_any_status(session, ["pane", "get", pane_id])?;
        if string_at(&pane, "/error/code") == Some("pane_not_found") {
            return Ok(());
        }
        if string_at(&pane, "/result/pane/pane_id") != Some(pane_id) {
            return Err(BackendError::Malformed(
                "exact pane response did not round trip".to_owned(),
            ));
        }
        let tab = required(&pane, "/result/pane/tab_id")?;
        if tab == before.tab_id {
            return Err(BackendError::Command(
                "herdr presentation cleanup target is the maintainer's active tab; refusing a close that cannot preserve focus".to_owned(),
            ));
        }
        if required_state
            .is_some_and(|required| self.pane_agent_state(session, pane_id) != required)
        {
            return Err(BackendError::Command(
                "pane agent state changed at close boundary".to_owned(),
            ));
        }
        let close = self.run_scoped(session, ["pane", "close", pane_id])?;
        let restore = self.focus_restore(session, &before);
        if !close.status.success() {
            return Err(BackendError::Command("Herdr pane close failed".to_owned()));
        }
        restore
    }

    /// Create a disposable projection and converge it to exactly one task pane.
    pub fn projection_create_task(
        &mut self,
        cwd: &Path,
        workspace_label: &str,
        task_label: &str,
    ) -> Result<ProjectionEndpoint, BackendError> {
        self.version_check()?;
        let session = self.session().to_owned();
        self.server_ensure(&session)?;
        let before = self.focus_snapshot(&session)?;
        let workspace = self.json_scoped(
            &session,
            [
                std::ffi::OsString::from("workspace"),
                std::ffi::OsString::from("create"),
                std::ffi::OsString::from("--cwd"),
                cwd.as_os_str().to_owned(),
                std::ffi::OsString::from("--label"),
                std::ffi::OsString::from(workspace_label),
                std::ffi::OsString::from("--no-focus"),
            ],
        )?;
        self.focus_restore(&session, &before)?;
        let workspace_id = required(&workspace, "/result/workspace/workspace_id")?;
        let seeded_tab_id = required(&workspace, "/result/tab/tab_id")?;
        let seeded_pane_id = required(&workspace, "/result/root_pane/pane_id")?;
        let before = self.focus_snapshot(&session)?;
        let task = self.json_scoped(
            &session,
            [
                std::ffi::OsString::from("tab"),
                std::ffi::OsString::from("create"),
                std::ffi::OsString::from("--workspace"),
                std::ffi::OsString::from(&workspace_id),
                std::ffi::OsString::from("--cwd"),
                cwd.as_os_str().to_owned(),
                std::ffi::OsString::from("--label"),
                std::ffi::OsString::from(task_label),
                std::ffi::OsString::from("--no-focus"),
            ],
        )?;
        self.focus_restore(&session, &before)?;
        let tab_id = required(&task, "/result/tab/tab_id")?;
        let pane_id = required(&task, "/result/root_pane/pane_id")?;
        if let Err(error) = self.close_pane_focus_preserving(&session, &seeded_pane_id, None) {
            let _ = self.close_pane_focus_preserving(&session, &pane_id, None);
            return Err(error);
        }
        let tabs = self.json_scoped(&session, ["tab", "list", "--workspace", &workspace_id])?;
        let panes = self.json_scoped(&session, ["pane", "list", "--workspace", &workspace_id])?;
        let exact_tab = array(&tabs, "/result/tabs")?;
        let exact_pane = array(&panes, "/result/panes")?;
        if exact_tab.len() != 1
            || exact_pane.len() != 1
            || string_at(&exact_tab[0], "/tab_id") != Some(tab_id.as_str())
            || string_at(&exact_pane[0], "/pane_id") != Some(pane_id.as_str())
            || string_at(&exact_pane[0], "/tab_id") != Some(tab_id.as_str())
        {
            let _ = self.close_pane_focus_preserving(&session, &pane_id, None);
            return Err(BackendError::Malformed(
                "projection did not converge to one exact task pane".to_owned(),
            ));
        }
        Ok(ProjectionEndpoint {
            session,
            workspace_id,
            seeded_tab_id,
            seeded_pane_id,
            tab_id,
            pane_id,
        })
    }

    /// Resolve an exact unique parent workspace by immutable expected label.
    pub fn parent_workspace_exact(
        &mut self,
        session: &str,
        label: &str,
    ) -> Result<String, BackendError> {
        let value = self.json_scoped(session, ["workspace", "list"])?;
        let matching = array(&value, "/result/workspaces")?
            .iter()
            .filter(|workspace| string_at(workspace, "/label") == Some(label))
            .collect::<Vec<_>>();
        if matching.len() == 1 {
            required(matching[0], "/workspace_id")
        } else {
            Err(BackendError::Malformed(
                "parent workspace label is not unique".to_owned(),
            ))
        }
    }

    /// Verify a bound projection's unique workspace, topology, parent, and placement.
    pub fn live_binding_matches(&mut self, binding: &ProjectionBinding) -> bool {
        self.live_binding_matches_inner(binding).unwrap_or(false)
    }

    fn live_binding_matches_inner(
        &mut self,
        binding: &ProjectionBinding,
    ) -> Result<bool, BackendError> {
        let value = self.json_scoped(&binding.session, ["workspace", "list"])?;
        let spaces = array(&value, "/result/workspaces")?;
        let child_matches = spaces
            .iter()
            .enumerate()
            .filter(|(_, workspace)| {
                string_at(workspace, "/workspace_id") == Some(binding.workspace_id.as_str())
                    && string_at(workspace, "/label") == Some(binding.workspace_label.as_str())
            })
            .collect::<Vec<_>>();
        let parent_matches = spaces
            .iter()
            .enumerate()
            .filter(|(_, workspace)| {
                string_at(workspace, "/workspace_id") == Some(binding.parent_workspace_id.as_str())
                    && string_at(workspace, "/label") == Some(binding.parent_label.as_str())
            })
            .collect::<Vec<_>>();
        let token_matches = spaces
            .iter()
            .filter(|workspace| {
                string_at(workspace, "/label").is_some_and(|label| {
                    label.ends_with(&format!(" · p:{}", binding.projection_id))
                })
            })
            .count();
        if child_matches.len() != 1 || parent_matches.len() != 1 || token_matches != 1 {
            return Ok(false);
        }
        let child_index = child_matches[0].0;
        let parent_index = parent_matches[0].0;
        if child_index <= parent_index
            || spaces[parent_index + 1..child_index]
                .iter()
                .any(|workspace| {
                    !string_at(workspace, "/label")
                        .is_some_and(|label| is_child_label(label, &binding.parent_label))
                })
        {
            return Ok(false);
        }
        let tabs = self.json_scoped(
            &binding.session,
            ["tab", "list", "--workspace", &binding.workspace_id],
        )?;
        let panes = self.json_scoped(
            &binding.session,
            ["pane", "list", "--workspace", &binding.workspace_id],
        )?;
        let tabs = array(&tabs, "/result/tabs")?;
        let panes = array(&panes, "/result/panes")?;
        Ok(tabs.len() == 1
            && panes.len() == 1
            && string_at(&tabs[0], "/tab_id") == Some(binding.tab_id.as_str())
            && string_at(&tabs[0], "/label") == Some(binding.task_label.as_str())
            && string_at(&panes[0], "/pane_id") == Some(binding.pane_id.as_str())
            && string_at(&panes[0], "/tab_id") == Some(binding.tab_id.as_str()))
    }

    /// Best-effort move one response-derived new workspace into its parent's contiguous block.
    pub fn order_projection_best_effort(
        &mut self,
        session: &str,
        created_workspace: &str,
        parent_label: &str,
    ) -> Result<(), BackendError> {
        let value = self.json_scoped(session, ["workspace", "list"])?;
        let analysis = analyze_order(&value, created_workspace, parent_label)?;
        if analysis.current == analysis.desired {
            return Ok(());
        }
        let status = self.json_scoped(session, ["status", "--json"])?;
        if status
            .pointer("/client/protocol")
            .and_then(Value::as_u64)
            .is_none_or(|protocol| protocol < 16)
        {
            return Err(BackendError::Unsupported {
                backend: BackendName::Herdr,
                capability: "workspace.move",
            });
        }
        let schema = self.json_scoped(session, ["api", "schema", "--json"])?;
        let schema_text = schema.to_string();
        if !schema_text.contains("workspace.move")
            || !schema_text.contains("workspace_id")
            || !schema_text.contains("insert_index")
        {
            return Err(BackendError::Unsupported {
                backend: BackendName::Herdr,
                capability: "workspace.move schema",
            });
        }
        let socket = self.presentation_session_socket_path(session)?;
        let focus = self.focus_snapshot(session)?;
        let response = move_workspace(&socket, created_workspace, analysis.desired as u64);
        let restore = self.focus_restore(session, &focus);
        let response = response?;
        restore?;
        let spaces = array(&response, "/result/workspaces")?;
        if spaces
            .get(analysis.desired)
            .and_then(|workspace| string_at(workspace, "/workspace_id"))
            != Some(created_workspace)
        {
            return Err(BackendError::Malformed(
                "workspace move returned an unverifiable order".to_owned(),
            ));
        }
        let existing = spaces
            .iter()
            .filter_map(|workspace| string_at(workspace, "/workspace_id"))
            .filter(|id| *id != created_workspace)
            .collect::<Vec<_>>();
        if existing != analysis.existing {
            return Err(BackendError::Malformed(
                "workspace move changed pre-existing relative order".to_owned(),
            ));
        }
        Ok(())
    }

    /// Resolve one exact running named-session socket, canonicalizing its parent.
    pub fn presentation_session_socket_path(
        &mut self,
        session: &str,
    ) -> Result<PathBuf, BackendError> {
        let output = self.run_scoped(session, ["session", "list", "--json"])?;
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| BackendError::Malformed(error.to_string()))?;
        let matches = array(&value, "/sessions")?
            .iter()
            .filter(|entry| {
                string_at(entry, "/name") == Some(session)
                    && bool_at(entry, "/running") == Some(true)
            })
            .filter_map(|entry| string_at(entry, "/socket_path"))
            .collect::<Vec<_>>();
        if matches.len() != 1 || !Path::new(matches[0]).is_absolute() {
            return Err(BackendError::Malformed(
                "named-session socket is ambiguous".to_owned(),
            ));
        }
        canonicalize_parent(Path::new(matches[0]))
    }

    /// Compute the machine-private lock path bound to session name and exact socket.
    pub fn presentation_session_lock_path(
        &mut self,
        session: &str,
    ) -> Result<PathBuf, BackendError> {
        let socket = self.presentation_session_socket_path(session)?;
        let namespace = PathBuf::from("/tmp/broker-herdr-presentation");
        ensure_private_namespace(&namespace)?;
        let mut digest = Sha256::new();
        digest.update(session.as_bytes());
        digest.update([0]);
        digest.update(socket.as_os_str().as_encoded_bytes());
        let key = format!("{:x}", digest.finalize());
        Ok(namespace.join(format!("order-{}.lock", &key[..32])))
    }
}

fn move_workspace(socket: &Path, workspace: &str, index: u64) -> Result<Value, BackendError> {
    let Some(mover) = std::env::var_os("MX_BACKEND_HERDR_WORKSPACE_MOVER") else {
        return herdr_wire::workspace_move(socket, workspace, index)
            .map_err(|error| BackendError::Command(error.to_string()));
    };
    let mut request = CommandRequest::new(
        mover,
        [
            socket.as_os_str().to_owned(),
            OsString::from(workspace),
            OsString::from(index.to_string()),
        ],
    );
    request.timeout = std::time::Duration::from_secs(6);
    request.output_limit = 4 * 1024 * 1024;
    let output = SystemCommandRunner
        .run(&request)
        .map_err(|error| BackendError::Command(error.to_string()))?;
    if !output.status.success() {
        return Err(BackendError::Command(
            "custom workspace mover failed".to_owned(),
        ));
    }
    let response: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| BackendError::Malformed(error.to_string()))?;
    if response.get("id").and_then(Value::as_str) != Some("mx-workspace-move")
        || response.get("error").is_some_and(|error| !error.is_null())
        || response.pointer("/result/type").and_then(Value::as_str) != Some("workspace_list")
        || !response
            .pointer("/result/workspaces")
            .is_some_and(Value::is_array)
    {
        return Err(BackendError::Malformed(
            "custom workspace mover returned a mismatched response".to_owned(),
        ));
    }
    Ok(response)
}

fn ensure_private_namespace(path: &Path) -> Result<(), BackendError> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| BackendError::Metadata(error.to_string()))?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(BackendError::Metadata(error.to_string())),
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|error| BackendError::Metadata(error.to_string()))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(BackendError::Metadata(
            "presentation lock namespace is not private".to_owned(),
        ));
    }
    Ok(())
}

fn canonicalize_parent(path: &Path) -> Result<PathBuf, BackendError> {
    let parent = path
        .parent()
        .ok_or_else(|| BackendError::Metadata("socket has no parent".to_owned()))?;
    let name = path
        .file_name()
        .ok_or_else(|| BackendError::Metadata("socket has no filename".to_owned()))?;
    if parent.is_dir() {
        fs::canonicalize(parent)
            .map(|parent| parent.join(name))
            .map_err(|error| BackendError::Metadata(error.to_string()))
    } else {
        Ok(path.to_owned())
    }
}

#[derive(Debug)]
struct OrderAnalysis<'a> {
    current: usize,
    desired: usize,
    existing: Vec<&'a str>,
}

fn analyze_order<'a>(
    value: &'a Value,
    created: &str,
    parent: &str,
) -> Result<OrderAnalysis<'a>, BackendError> {
    let spaces = array(value, "/result/workspaces")?;
    let created_matches = spaces
        .iter()
        .enumerate()
        .filter(|(_, workspace)| string_at(workspace, "/workspace_id") == Some(created))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let parent_matches = spaces
        .iter()
        .enumerate()
        .filter(|(_, workspace)| string_at(workspace, "/label") == Some(parent))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if created_matches.len() != 1
        || parent_matches.len() != 1
        || created_matches[0] + 1 != spaces.len()
        || parent_matches[0] >= created_matches[0]
    {
        return Err(BackendError::Malformed(
            "ambiguous presentation workspace layout".to_owned(),
        ));
    }
    let current = created_matches[0];
    let parent_index = parent_matches[0];
    let mut block = 0;
    for workspace in &spaces[parent_index + 1..current] {
        let Some(label) = string_at(workspace, "/label") else {
            break;
        };
        if is_child_label(label, parent) {
            block += 1;
        } else {
            break;
        }
    }
    validate_remainder(&spaces[parent_index + 1 + block..current])?;
    Ok(OrderAnalysis {
        current,
        desired: parent_index + 1 + block,
        existing: spaces
            .iter()
            .filter_map(|workspace| string_at(workspace, "/workspace_id"))
            .filter(|id| *id != created)
            .collect(),
    })
}

fn validate_remainder(spaces: &[Value]) -> Result<(), BackendError> {
    let mut active_parent: Option<&str> = None;
    for workspace in spaces {
        let label = string_at(workspace, "/label").unwrap_or_default();
        if is_parent_label(label) {
            active_parent = Some(label);
        } else if is_new_child_label(label) {
            if active_parent.is_none() {
                return Err(BackendError::Malformed(
                    "detached presentation child".to_owned(),
                ));
            }
        } else if is_legacy_child_label(label) {
            if active_parent.is_none_or(|parent| !label.starts_with(&format!("{parent}/"))) {
                return Err(BackendError::Malformed(
                    "foreign legacy presentation child".to_owned(),
                ));
            }
        } else {
            active_parent = None;
        }
    }
    Ok(())
}

fn is_parent_label(label: &str) -> bool {
    label == "broker"
        || label
            .strip_prefix("daemon-")
            .is_some_and(|suffix| !suffix.is_empty() && !suffix.contains('/'))
}

fn is_new_child_label(label: &str) -> bool {
    label
        .strip_prefix("└ ")
        .and_then(|rest| rest.rsplit_once(" · p:"))
        .is_some_and(|(task, token)| !task.is_empty() && valid_token(token))
}

fn is_legacy_child_label(label: &str) -> bool {
    let Some((owner, rest)) = label.split_once('/') else {
        return false;
    };
    is_parent_label(owner)
        && rest
            .rsplit_once(" · p:")
            .is_some_and(|(task, token)| !task.is_empty() && valid_token(token))
}

fn is_child_label(label: &str, parent: &str) -> bool {
    is_new_child_label(label)
        || (is_legacy_child_label(label) && label.starts_with(&format!("{parent}/")))
}

fn string_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn bool_at(value: &Value, pointer: &str) -> Option<bool> {
    value.pointer(pointer).and_then(Value::as_bool)
}

fn required(value: &Value, pointer: &str) -> Result<String, BackendError> {
    string_at(value, pointer)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BackendError::Malformed(format!("missing string at {pointer}")))
}

fn array<'a>(value: &'a Value, pointer: &str) -> Result<&'a [Value], BackendError> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| BackendError::Malformed(format!("missing array at {pointer}")))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::ExitStatus;

    use serde_json::json;

    use super::{
        FocusSnapshot, ProjectionBinding, ProjectionJournal, analyze_order, bind_journal,
        concise_task_label, create_journal, home_identity, journal_path, projection_id,
        projection_workspace_label, read_journal, replace_journal_endpoint,
    };
    use crate::command::{CommandError, CommandOutput, CommandRequest, CommandRunner};
    use crate::herdr::HerdrBackend;

    const TOKEN: &str = "abcdefghijklmnopqrstuv";

    #[derive(Debug)]
    struct PresentationRunner {
        socket: PathBuf,
        calls: Vec<CommandRequest>,
    }

    fn output(stdout: impl Into<Vec<u8>>) -> CommandOutput {
        CommandOutput {
            status: ExitStatus::from_raw(0),
            stdout: stdout.into(),
            stderr: Vec::new(),
        }
    }

    impl CommandRunner for PresentationRunner {
        fn run(&mut self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
            self.calls.push(request.clone());
            let args = request
                .args
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>();
            let workspace = args
                .iter()
                .position(|arg| arg == "--workspace")
                .and_then(|index| args.get(index + 1))
                .map(|arg| arg.as_ref());
            let body = match args.first().map(|arg| arg.as_ref()) {
                Some("--version") => b"herdr 0.7.4\n".to_vec(),
                Some("status") => br#"{"client":{"version":"0.7.4","protocol":16},"server":{"running":true}}"#.to_vec(),
                Some("workspace") if args.get(1).is_some_and(|arg| arg == "list") => format!(
                    r#"{{"result":{{"workspaces":[{{"workspace_id":"parent","label":"broker","focused":true,"active_tab_id":"parent:t1"}},{{"workspace_id":"child","label":"└ task · p:{TOKEN}","focused":false,"active_tab_id":"child:t1"}}]}}}}"#
                )
                .into_bytes(),
                Some("workspace") if args.get(1).is_some_and(|arg| arg == "create") => br#"{"result":{"workspace":{"workspace_id":"new"},"tab":{"tab_id":"new:seed"},"root_pane":{"pane_id":"new:seed-pane"}}}"#.to_vec(),
                Some("tab") if args.get(1).is_some_and(|arg| arg == "list") => match workspace {
                    Some("parent") => br#"{"result":{"tabs":[{"tab_id":"parent:t1","workspace_id":"parent","label":"maintainer","focused":true}]}}"#.to_vec(),
                    Some("new") => br#"{"result":{"tabs":[{"tab_id":"new:t2","workspace_id":"new","label":"mx-task","focused":false}]}}"#.to_vec(),
                    _ => br#"{"result":{"tabs":[{"tab_id":"child:t1","workspace_id":"child","label":"mx-task","focused":false}]}}"#.to_vec(),
                },
                Some("tab") if args.get(1).is_some_and(|arg| arg == "create") => br#"{"result":{"tab":{"tab_id":"new:t2"},"root_pane":{"pane_id":"new:p2"}}}"#.to_vec(),
                Some("tab") if args.get(1).is_some_and(|arg| arg == "get") => {
                    let tab = args.get(2).map_or("parent:t1", |arg| arg.as_ref());
                    let workspace = if tab.starts_with("parent") { "parent" } else { "new" };
                    format!(r#"{{"result":{{"tab":{{"tab_id":"{tab}","workspace_id":"{workspace}"}}}}}}"#).into_bytes()
                }
                Some("pane") if args.get(1).is_some_and(|arg| arg == "list") => match workspace {
                    Some("new") => br#"{"result":{"panes":[{"pane_id":"new:p2","tab_id":"new:t2","workspace_id":"new"}]}}"#.to_vec(),
                    _ => br#"{"result":{"panes":[{"pane_id":"child:p1","tab_id":"child:t1","workspace_id":"child"}]}}"#.to_vec(),
                },
                Some("pane") if args.get(1).is_some_and(|arg| arg == "get") => {
                    let pane = args.get(2).map_or("child:p1", |arg| arg.as_ref());
                    let (tab, workspace) = if pane.starts_with("new") {
                        ("new:t2", "new")
                    } else {
                        ("child:t1", "child")
                    };
                    format!(r#"{{"result":{{"pane":{{"pane_id":"{pane}","tab_id":"{tab}","workspace_id":"{workspace}"}}}}}}"#).into_bytes()
                }
                Some("agent") => br#"{"error":{"code":"agent_not_found"}}"#.to_vec(),
                Some("session") => format!(
                    r#"{{"sessions":[{{"name":"named","running":true,"socket_path":"{}"}}]}}"#,
                    self.socket.display()
                )
                .into_bytes(),
                Some("api") => br#"{"methods":{"workspace.move":{"params":["workspace_id","insert_index"]}}}"#.to_vec(),
                _ => Vec::new(),
            };
            Ok(output(body))
        }
    }

    fn binding(home: &std::path::Path) -> ProjectionBinding {
        ProjectionBinding {
            task_id: "task".to_owned(),
            projection_id: TOKEN.to_owned(),
            home: home.to_owned(),
            session: "named".to_owned(),
            workspace_id: "child".to_owned(),
            tab_id: "child:t1".to_owned(),
            pane_id: "child:p1".to_owned(),
            parent_workspace_id: "parent".to_owned(),
            parent_label: "broker".to_owned(),
            workspace_label: projection_workspace_label("task", TOKEN),
            task_label: "mx-task".to_owned(),
        }
    }

    #[test]
    fn journal_versions_are_exact_private_and_atomic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = create_journal(temp.path(), "task", TOKEN).expect("create");
        assert_eq!(path, journal_path(temp.path(), "task"));
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(create_journal(temp.path(), "task", TOKEN).is_err());
        assert_eq!(
            read_journal(&path, "task").expect("v1"),
            ProjectionJournal::V1 {
                task_id: "task".to_owned(),
                projection_id: TOKEN.to_owned()
            }
        );
        let binding = ProjectionBinding {
            task_id: "task".to_owned(),
            projection_id: TOKEN.to_owned(),
            home: temp.path().to_owned(),
            session: "named".to_owned(),
            workspace_id: "w1".to_owned(),
            tab_id: "w1:t1".to_owned(),
            pane_id: "w1:p1".to_owned(),
            parent_workspace_id: "parent".to_owned(),
            parent_label: "broker".to_owned(),
            workspace_label: projection_workspace_label("task", TOKEN),
            task_label: "mx-task".to_owned(),
        };
        bind_journal(&path, binding.clone()).expect("bind");
        assert_eq!(
            read_journal(&path, "task").expect("v2"),
            ProjectionJournal::V2(Box::new(binding))
        );
        replace_journal_endpoint(&path, "task", "w1:t1", "w1:p1", "w1:t2", "w1:p2")
            .expect("replace");
        let ProjectionJournal::V2(updated) = read_journal(&path, "task").expect("updated") else {
            panic!("expected v2")
        };
        assert_eq!(
            (updated.tab_id.as_str(), updated.pane_id.as_str()),
            ("w1:t2", "w1:p2")
        );
    }

    #[test]
    fn journals_refuse_symlinks_duplicates_and_malformed_bindings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let real = temp.path().join("real");
        std::fs::write(
            &real,
            format!("version=1\ntask_id=task\nprojection_id={TOKEN}\n"),
        )
        .expect("real");
        let link = journal_path(temp.path(), "task");
        symlink(&real, &link).expect("link");
        assert!(read_journal(&link, "task").is_err());
        assert!(create_journal(temp.path(), "../escape", TOKEN).is_err());
    }

    #[test]
    fn labels_preserve_current_unicode_and_concise_owner_shape() {
        assert_eq!(concise_task_label("broker/mx-one"), "one");
        assert_eq!(concise_task_label("daemon-wheelhouse/mx-two"), "two");
        assert_eq!(
            projection_workspace_label("broker/mx-one", TOKEN),
            format!("└ one · p:{TOKEN}")
        );
    }

    #[test]
    fn order_analysis_preserves_existing_sequence_and_rejects_ambiguity() {
        let value = json!({"result":{"workspaces":[
            {"workspace_id":"parent","label":"broker"},
            {"workspace_id":"old","label":format!("└ old · p:{TOKEN}")},
            {"workspace_id":"other","label":"daemon-a"},
            {"workspace_id":"new","label":format!("└ new · p:{TOKEN}")}
        ]}});
        let analysis = analyze_order(&value, "new", "broker").expect("analysis");
        assert_eq!(analysis.current, 3);
        assert_eq!(analysis.desired, 2);
        assert_eq!(analysis.existing, ["parent", "old", "other"]);
        assert!(analyze_order(&value, "missing", "broker").is_err());
    }

    #[test]
    fn projection_runtime_reads_exact_topology_focus_and_session_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("named.sock");
        let mut backend = HerdrBackend::new(
            PresentationRunner {
                socket: socket.clone(),
                calls: Vec::new(),
            },
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        let binding = binding(temp.path());
        let focus = FocusSnapshot {
            workspace_id: "parent".to_owned(),
            tab_id: "parent:t1".to_owned(),
        };
        assert_eq!(backend.focus_snapshot("named").expect("focus"), focus);
        backend.focus_restore("named", &focus).expect("restore");
        assert_eq!(
            backend
                .parent_workspace_exact("named", "broker")
                .expect("parent"),
            "parent"
        );
        assert!(backend.projection_live_binding_matches("named", &binding));
        assert!(backend.live_binding_matches(&binding));
        let journal = create_journal(temp.path(), "task", TOKEN).expect("journal");
        assert!(backend.projection_endpoint_matches_journal("named", "child", &journal, "task"));
        assert!(backend.projection_recovery_allows_flat("named", &journal, "task"));
        backend
            .close_pane_focus_preserving(
                "named",
                "child:p1",
                Some(crate::herdr::PaneAgentState::NoAgent),
            )
            .expect("close");
        assert_eq!(
            backend
                .presentation_session_socket_path("named")
                .expect("socket"),
            std::fs::canonicalize(socket.parent().expect("socket parent"))
                .expect("canonical parent")
                .join("named.sock")
        );
        assert!(
            backend
                .presentation_session_lock_path("named")
                .expect("lock")
                .starts_with("/tmp/broker-herdr-presentation")
        );
        assert_eq!(
            home_identity(temp.path()).expect("home"),
            std::fs::canonicalize(temp.path()).expect("canonical home")
        );
        let token = projection_id().expect("token");
        assert_eq!(token.len(), 22);
    }

    #[test]
    fn projection_create_converges_to_one_task_and_preserves_focus() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut backend = HerdrBackend::new(
            PresentationRunner {
                socket: temp.path().join("named.sock"),
                calls: Vec::new(),
            },
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        let endpoint = backend
            .projection_create_task(temp.path(), "projection", "mx-task")
            .expect("projection");
        assert_eq!(endpoint.session, "named");
        assert_eq!(endpoint.workspace_id, "new");
        assert_eq!(endpoint.seeded_tab_id, "new:seed");
        assert_eq!(endpoint.seeded_pane_id, "new:seed-pane");
        assert_eq!(endpoint.tab_id, "new:t2");
        assert_eq!(endpoint.pane_id, "new:p2");
    }
}

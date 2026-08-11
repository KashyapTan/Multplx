//! Typed runtime-backend identities, capabilities, and selector resolution.

use std::fmt;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use multplx_core::composer::ComposerState;
use multplx_core::identifiers::TaskId;

/// Supported runtime backend names.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackendName {
    /// Reference tmux backend.
    Tmux,
    /// Herdr compatibility backend, ported in Portion 05.
    Herdr,
    /// cmux compatibility backend, ported in Portion 06.
    Cmux,
}

impl BackendName {
    /// Parse the exact current backend vocabulary.
    pub fn parse(value: &str) -> Result<Self, BackendError> {
        match value {
            "tmux" => Ok(Self::Tmux),
            "herdr" => Ok(Self::Herdr),
            "cmux" => Ok(Self::Cmux),
            _ => Err(BackendError::UnknownBackend(value.to_owned())),
        }
    }

    /// Return the stable wire token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tmux => "tmux",
            Self::Herdr => "herdr",
            Self::Cmux => "cmux",
        }
    }

    /// Return the backend-specific tool delta in stable order.
    #[must_use]
    pub fn required_tools(self) -> &'static [&'static str] {
        match self {
            Self::Tmux => &["tmux"],
            Self::Herdr => &["herdr"],
            Self::Cmux => &["cmux", "jq"],
        }
    }
}

impl fmt::Display for BackendName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One backend operation capability.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Capability {
    /// Native semantic task state.
    NativeState,
    /// Native transition event stream.
    TransitionEvents,
    /// Structured composer state.
    ComposerState,
    /// Recovery-grade agent liveness.
    AgentState,
}

/// Validated endpoint text tied to its backend and optional expected label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendTarget {
    backend: BackendName,
    endpoint: String,
    expected_label: Option<String>,
}

impl BackendTarget {
    /// Construct a non-empty endpoint without control bytes.
    pub fn new(
        backend: BackendName,
        endpoint: impl Into<String>,
        expected_label: Option<String>,
    ) -> Result<Self, BackendError> {
        let endpoint = endpoint.into();
        if endpoint.is_empty()
            || endpoint
                .bytes()
                .any(|byte| byte == 0 || byte.is_ascii_control())
        {
            return Err(BackendError::InvalidTarget(endpoint));
        }
        if expected_label.as_ref().is_some_and(|label| {
            label.is_empty()
                || label
                    .bytes()
                    .any(|byte| byte == 0 || byte.is_ascii_control())
        }) {
            return Err(BackendError::InvalidTarget(
                expected_label.unwrap_or_default(),
            ));
        }
        Ok(Self {
            backend,
            endpoint,
            expected_label,
        })
    }

    /// Return the bound backend.
    #[must_use]
    pub fn backend(&self) -> BackendName {
        self.backend
    }

    /// Return literal endpoint text.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Return the optional expected presentation label.
    #[must_use]
    pub fn expected_label(&self) -> Option<&str> {
        self.expected_label.as_deref()
    }
}

/// Validated container or session name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerId {
    backend: BackendName,
    value: String,
}

impl ContainerId {
    /// Parse one non-empty container name.
    pub fn parse(value: impl Into<String>) -> Result<Self, BackendError> {
        Self::for_backend(BackendName::Tmux, value)
    }

    /// Parse a container identity using its backend-specific shape.
    pub fn for_backend(
        backend: BackendName,
        value: impl Into<String>,
    ) -> Result<Self, BackendError> {
        let value = value.into();
        if value.is_empty()
            || value
                .bytes()
                .any(|byte| byte == 0 || byte.is_ascii_control())
            || (backend != BackendName::Herdr && value.contains(':'))
            || (backend == BackendName::Herdr
                && value
                    .split_once(':')
                    .is_none_or(|(session, workspace)| session.is_empty() || workspace.is_empty()))
        {
            return Err(BackendError::InvalidContainer(value));
        }
        Ok(Self { backend, value })
    }

    /// Return the bound backend.
    #[must_use]
    pub fn backend(&self) -> BackendName {
        self.backend
    }

    /// Return literal container text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

/// Task creation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskSpec {
    /// Expected backend label.
    pub label: String,
    /// Existing starting directory.
    pub working_directory: PathBuf,
}

/// Bounded capture request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureRequest {
    /// Bound target identity.
    pub target: BackendTarget,
    /// Requested terminal rows.
    pub lines: u32,
    /// Maximum captured bytes.
    pub byte_limit: usize,
}

/// Verified-submit policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubmitRequest<'a> {
    /// Literal text typed exactly once.
    pub text: &'a str,
    /// Enter attempt budget, with zero retaining the legacy one-attempt floor.
    pub retries: usize,
    /// Delay after each Enter.
    pub enter_delay: Duration,
    /// Delay between typing and the first Enter.
    pub settle: Duration,
}

/// Recovery-grade endpoint state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentState {
    /// Verified harness process is present.
    Alive,
    /// Endpoint exists with a known bare shell.
    Dead,
    /// Exact endpoint is authoritatively absent.
    Missing,
    /// Endpoint process cannot be attributed safely.
    Ambiguous,
    /// Inventory or endpoint observation failed.
    Unreadable,
    /// Backend has no recovery classifier.
    Unverified,
}

impl AgentState {
    /// Return the exact compatibility token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::Dead => "dead",
            Self::Missing => "missing",
            Self::Ambiguous => "ambiguous",
            Self::Unreadable => "unreadable",
            Self::Unverified => "unverified",
        }
    }

    /// Collapse to the compatibility three-state view.
    #[must_use]
    pub fn alive_token(self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::Dead | Self::Missing => "dead",
            Self::Ambiguous | Self::Unreadable | Self::Unverified => "unknown",
        }
    }
}

/// Semantic native task state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeState {
    /// Backend reports idle.
    Idle,
    /// Backend reports active work.
    Working,
    /// Backend reports a blocker.
    Blocked,
    /// Backend reports completion.
    Done,
}

/// One live endpoint inventory item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveTarget {
    /// Full backend target.
    pub target: BackendTarget,
    /// Backend presentation label.
    pub label: String,
}

/// Verified kill observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KillOutcome {
    /// Target is absent after the request.
    Gone,
    /// Target is still present.
    StillPresent,
    /// Post-kill state could not be read.
    Unknown,
}

/// Typed backend failure classes.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    /// Unknown backend token.
    #[error("unknown backend '{0}'")]
    UnknownBackend(String),
    /// Invalid target identity.
    #[error("invalid backend target '{0}'")]
    InvalidTarget(String),
    /// Invalid container identity.
    #[error("invalid backend container '{0}'")]
    InvalidContainer(String),
    /// Explicit unsupported capability.
    #[error("backend {backend} does not support {capability}")]
    Unsupported {
        /// Backend name.
        backend: BackendName,
        /// Capability name.
        capability: &'static str,
    },
    /// Backend command failed.
    #[error("backend command failed: {0}")]
    Command(String),
    /// Backend response was unusable.
    #[error("malformed backend response: {0}")]
    Malformed(String),
    /// Local metadata or directory read failed.
    #[error("backend metadata error: {0}")]
    Metadata(String),
}

/// Complete runtime backend interface.
pub trait RuntimeBackend {
    /// Backend identity.
    fn name(&self) -> BackendName;
    /// Whether this implementation supports one optional capability.
    fn supports(&self, capability: Capability) -> bool;
    /// Verify the provider CLI is executable.
    fn tool_check(&mut self) -> Result<(), BackendError>;
    /// Verify the provider version command.
    fn version_check(&mut self) -> Result<String, BackendError>;
    /// Resolve or create the task container.
    fn container_ensure(&mut self) -> Result<ContainerId, BackendError>;
    /// Create one task endpoint.
    fn task_create(
        &mut self,
        container: &ContainerId,
        task: &TaskSpec,
    ) -> Result<BackendTarget, BackendError>;
    /// Verify exact endpoint readiness.
    fn target_ready(&mut self, target: &BackendTarget) -> Result<(), BackendError>;
    /// Read the current endpoint path.
    fn current_path(&mut self, target: &BackendTarget) -> Result<PathBuf, BackendError>;
    /// Capture bounded plain endpoint text.
    fn capture(&mut self, request: &CaptureRequest) -> Result<Vec<u8>, BackendError>;
    /// Classify pending composer input.
    fn composer_state(&mut self, target: &BackendTarget) -> Result<ComposerState, BackendError>;
    /// Type literal text without submission.
    fn send_literal(&mut self, target: &BackendTarget, text: &str) -> Result<(), BackendError>;
    /// Send one named key.
    fn send_key(&mut self, target: &BackendTarget, key: &str) -> Result<(), BackendError>;
    /// Type once and verify submission.
    fn send_submit(
        &mut self,
        target: &BackendTarget,
        request: SubmitRequest<'_>,
    ) -> Result<ComposerState, BackendError>;
    /// Send one unverified line followed by Enter.
    fn send_text_line(&mut self, target: &BackendTarget, text: &str) -> Result<(), BackendError>;
    /// Return a semantic native state or an explicit unsupported error.
    fn native_state(&mut self, target: &BackendTarget) -> Result<NativeState, BackendError>;
    /// Return recovery-grade liveness.
    fn agent_state(&mut self, target: &BackendTarget) -> AgentState;
    /// Kill the exact endpoint and report the postcondition.
    fn kill_verified(&mut self, target: &BackendTarget) -> KillOutcome;
    /// List live endpoints.
    fn list_live(
        &mut self,
        container: Option<&ContainerId>,
    ) -> Result<Vec<LiveTarget>, BackendError>;
    /// Wait for a native transition or return an explicit unsupported error.
    fn wait_transition(
        &mut self,
        container: &ContainerId,
        targets: &[BackendTarget],
        timeout: Duration,
    ) -> Result<Option<String>, BackendError>;
}

/// Narrow inventory dependency used by selector resolution.
pub trait LiveInventory {
    /// List live endpoints for selector matching.
    fn list_live(
        &mut self,
        container: Option<&ContainerId>,
    ) -> Result<Vec<LiveTarget>, BackendError>;
}

/// Last `key=` value in one bounded metadata file.
pub fn meta_get(path: &Path, key: &str) -> Result<Option<String>, BackendError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(BackendError::Metadata(error.to_string())),
    };
    if bytes.len() > 1024 * 1024 {
        return Err(BackendError::Metadata(
            "metadata exceeds 1048576 bytes".to_owned(),
        ));
    }
    let prefix = format!("{key}=");
    Ok(String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix))
        .next_back()
        .map(str::to_owned))
}

/// Backend recorded by metadata, defaulting an absent field to tmux.
pub fn backend_of_meta(path: &Path) -> Result<BackendName, BackendError> {
    match meta_get(path, "backend")? {
        Some(value) if !value.is_empty() => BackendName::parse(&value),
        _ => Ok(BackendName::Tmux),
    }
}

/// Recorded endpoint in metadata.
pub fn target_of_meta(path: &Path) -> Result<Option<String>, BackendError> {
    meta_get(path, "window")
}

fn metadata_paths(state: &Path) -> Result<Vec<PathBuf>, BackendError> {
    let mut paths = fs::read_dir(state)
        .map_err(|error| BackendError::Metadata(error.to_string()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "meta")
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| {
        left.as_os_str()
            .as_bytes()
            .cmp(right.as_os_str().as_bytes())
    });
    Ok(paths)
}

/// Find the first metadata file whose last `window=` equals `target`.
pub fn meta_for_target(state: &Path, target: &str) -> Result<Option<PathBuf>, BackendError> {
    if !state.is_dir() {
        return Ok(None);
    }
    for path in metadata_paths(state)? {
        if target_of_meta(&path)?.as_deref() == Some(target) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Resolve exact-id and legacy `mx-<id>` selectors to a metadata path.
pub fn meta_for_selector(
    state: &Path,
    raw: &str,
) -> Result<Option<(TaskId, PathBuf)>, BackendError> {
    if raw.contains(':') {
        return Ok(None);
    }
    if let Ok(id) = TaskId::parse(raw) {
        let path = state.join(format!("{id}.meta"));
        if path.is_file() {
            return Ok(Some((id, path)));
        }
    }
    if let Some(legacy) = raw.strip_prefix("mx-") {
        let Ok(id) = TaskId::parse(legacy) else {
            return Ok(None);
        };
        let path = state.join(format!("{id}.meta"));
        if path.is_file() {
            return Ok(Some((id, path)));
        }
    }
    Ok(None)
}

/// Resolve a selector without weakening the legacy metadata precedence.
pub fn resolve_selector(
    raw: &str,
    state: &Path,
    tmux: &mut impl LiveInventory,
) -> Result<BackendTarget, BackendError> {
    if raw.contains(':') {
        let backend = meta_for_target(state, raw)?
            .as_deref()
            .map(backend_of_meta)
            .transpose()?
            .unwrap_or(BackendName::Tmux);
        return BackendTarget::new(backend, raw, None);
    }
    if let Some((id, meta)) = meta_for_selector(state, raw)? {
        let endpoint = target_of_meta(&meta)?.ok_or_else(|| {
            BackendError::Metadata(format!("no backend target recorded in {}", meta.display()))
        })?;
        return BackendTarget::new(backend_of_meta(&meta)?, endpoint, Some(format!("mx-{id}")));
    }
    if raw.starts_with("mx-") {
        return Err(BackendError::Metadata(format!(
            "no metadata for {raw} in {}",
            state.display()
        )));
    }
    if let Some(meta) = meta_for_target(state, raw)? {
        let endpoint = target_of_meta(&meta)?.ok_or_else(|| {
            BackendError::Metadata(format!("no backend target recorded in {}", meta.display()))
        })?;
        return BackendTarget::new(backend_of_meta(&meta)?, endpoint, None);
    }
    TaskId::parse(raw).map_err(|_| BackendError::InvalidTarget(raw.to_owned()))?;
    let matches = tmux
        .list_live(None)?
        .into_iter()
        .filter(|item| item.label == raw)
        .collect::<Vec<_>>();
    matches
        .into_iter()
        .next()
        .map(|item| item.target)
        .ok_or_else(|| BackendError::Metadata(format!("no window named {raw}")))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        AgentState, BackendError, BackendName, BackendTarget, ContainerId, LiveInventory,
        LiveTarget, backend_of_meta, meta_for_selector, meta_for_target, meta_get,
        resolve_selector, target_of_meta,
    };

    struct Inventory(Vec<LiveTarget>);

    impl LiveInventory for Inventory {
        fn list_live(&mut self, _: Option<&ContainerId>) -> Result<Vec<LiveTarget>, BackendError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn backend_vocabulary_capabilities_and_targets_are_typed() {
        for (text, backend, tools) in [
            ("tmux", BackendName::Tmux, &["tmux"][..]),
            ("herdr", BackendName::Herdr, &["herdr"][..]),
            ("cmux", BackendName::Cmux, &["cmux", "jq"][..]),
        ] {
            assert_eq!(BackendName::parse(text).expect("backend"), backend);
            assert_eq!(backend.to_string(), text);
            assert_eq!(backend.required_tools(), tools);
        }
        assert!(BackendName::parse("codex-app").is_err());
        assert!(BackendTarget::new(BackendName::Tmux, "", None).is_err());
        let target = BackendTarget::new(
            BackendName::Tmux,
            "broker:mx-one",
            Some("mx-one".to_owned()),
        )
        .expect("target");
        assert_eq!(target.backend(), BackendName::Tmux);
        assert_eq!(target.endpoint(), "broker:mx-one");
        assert_eq!(target.expected_label(), Some("mx-one"));
        assert!(BackendTarget::new(BackendName::Tmux, "pane", Some(String::new())).is_err());
        assert!(ContainerId::parse("").is_err());
        assert!(ContainerId::parse("bad:name").is_err());
        for state in [
            AgentState::Alive,
            AgentState::Dead,
            AgentState::Missing,
            AgentState::Ambiguous,
            AgentState::Unreadable,
            AgentState::Unverified,
        ] {
            assert!(!state.as_str().is_empty());
            assert!(!state.alive_token().is_empty());
        }
    }

    #[test]
    fn metadata_and_selector_precedence_match_the_legacy_facade() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path();
        std::fs::write(
            state.join("task.meta"),
            "window=old\nwindow=broker:mx-task\n",
        )
        .expect("meta");
        std::fs::write(
            state.join("other.meta"),
            "window=default:w:p\nbackend=herdr\n",
        )
        .expect("meta");
        assert_eq!(
            meta_get(&state.join("task.meta"), "window")
                .expect("get")
                .as_deref(),
            Some("broker:mx-task")
        );
        assert_eq!(
            backend_of_meta(&state.join("task.meta")).expect("backend"),
            BackendName::Tmux
        );
        assert_eq!(
            target_of_meta(&state.join("other.meta"))
                .expect("target")
                .as_deref(),
            Some("default:w:p")
        );
        assert_eq!(
            meta_for_target(state, "default:w:p").expect("lookup"),
            Some(state.join("other.meta"))
        );
        assert_eq!(
            meta_for_selector(state, "mx-task")
                .expect("selector")
                .expect("found")
                .0
                .as_str(),
            "task"
        );
        assert!(
            meta_for_selector(state, "session:window")
                .expect("explicit")
                .is_none()
        );
        assert!(
            meta_for_selector(state, "mx-../bad")
                .expect("malformed legacy")
                .is_none()
        );
        assert!(
            meta_for_target(&state.join("missing"), "target")
                .expect("missing state")
                .is_none()
        );
        std::fs::write(state.join("empty.meta"), "worktree=/tmp\n").expect("empty meta");

        let live = LiveTarget {
            target: BackendTarget::new(BackendName::Tmux, "broker:adhoc", None).expect("target"),
            label: "adhoc".to_owned(),
        };
        let mut inventory = Inventory(vec![live]);
        assert_eq!(
            resolve_selector("task", state, &mut inventory)
                .expect("task")
                .endpoint(),
            "broker:mx-task"
        );
        assert_eq!(
            resolve_selector("mx-task", state, &mut inventory)
                .expect("legacy")
                .expected_label(),
            Some("mx-task")
        );
        assert_eq!(
            resolve_selector("default:w:p", state, &mut inventory)
                .expect("explicit")
                .backend(),
            BackendName::Herdr
        );
        assert_eq!(
            resolve_selector("adhoc", state, &mut inventory)
                .expect("adhoc")
                .endpoint(),
            "broker:adhoc"
        );
        assert!(resolve_selector("mx-missing", state, &mut inventory).is_err());
        assert!(resolve_selector("empty", state, &mut inventory).is_err());
        assert!(resolve_selector("../escape", state, &mut inventory).is_err());
    }

    #[test]
    fn metadata_reads_are_bounded_and_missing_is_empty() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            meta_get(&temp.path().join("missing"), "window").expect("missing"),
            None
        );
        std::fs::write(temp.path().join("huge"), vec![b'x'; 1024 * 1024 + 1]).expect("huge");
        assert!(meta_get(&temp.path().join("huge"), "window").is_err());
        let invalid = Path::new("not-used");
        let _ = invalid;
    }
}

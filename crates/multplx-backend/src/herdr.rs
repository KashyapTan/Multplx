//! Typed Herdr runtime backend.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use multplx_core::composer::{ComposerState, classify_content, strip_ansi, strip_ghost};
use multplx_core::transition::{TransitionAction, TransitionRecord, policy};
use serde_json::Value;

use crate::command::{CommandOutput, CommandRequest, CommandRunner, SystemCommandRunner};
use crate::facade::{
    AgentState, BackendError, BackendName, BackendTarget, Capability, CaptureRequest, ContainerId,
    KillOutcome, LiveInventory, LiveTarget, NativeState, RuntimeBackend, SubmitRequest, TaskSpec,
};
use crate::herdr_wire;

/// Oldest protocol accepted for ordinary Herdr runtime operations.
pub const MIN_PROTOCOL: u64 = 14;

/// Oldest protocol accepted for push events and workspace movement.
pub const MIN_EVENTS_PROTOCOL: u64 = 16;

const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const CAPTURE_LIMIT: usize = 256 * 1024;

/// Exact result from task-tab creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HerdrTaskEndpoint {
    /// Response-derived tab id.
    pub tab_id: String,
    /// Response-derived pane id.
    pub pane_id: String,
    /// Session-bound facade target.
    pub target: BackendTarget,
}

/// Herdr pane registration state used by restart and cleanup decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneAgentState {
    /// Pane is authoritatively absent.
    Dead,
    /// Pane exists but no agent is registered.
    NoAgent,
    /// A supported native agent state is registered.
    Live,
    /// Response was contradictory or unreadable.
    Unknown,
}

impl PaneAgentState {
    /// Exact legacy token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dead => "dead",
            Self::NoAgent => "no-agent",
            Self::Live => "live",
            Self::Unknown => "unknown",
        }
    }
}

/// Herdr backend with an injectable bounded command runner.
#[derive(Debug)]
pub struct HerdrBackend<R = SystemCommandRunner> {
    runner: R,
    executable: OsString,
    session: String,
    home: PathBuf,
    seeded_tab_id: Option<String>,
    composer_lines: u32,
    pi_composer_max_lines: usize,
    ghost_luma_max: u16,
}

impl HerdrBackend<SystemCommandRunner> {
    /// Construct the real backend from the current environment.
    #[must_use]
    pub fn system() -> Self {
        let root = std::env::var_os("MX_ROOT_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
        let home = std::env::var_os("MX_HOME")
            .map(PathBuf::from)
            .unwrap_or(root);
        Self::new(
            SystemCommandRunner,
            std::env::var_os("MX_HERDR_BIN").unwrap_or_else(|| OsString::from("herdr")),
            std::env::var("HERDR_SESSION").unwrap_or_else(|_| "default".to_owned()),
            home,
        )
    }
}

impl<R: CommandRunner> HerdrBackend<R> {
    /// Construct an injectable Herdr backend.
    #[must_use]
    pub fn new(
        runner: R,
        executable: impl Into<OsString>,
        session: impl Into<String>,
        home: PathBuf,
    ) -> Self {
        Self {
            runner,
            executable: executable.into(),
            session: session.into(),
            home,
            seeded_tab_id: None,
            composer_lines: environment_number("MX_BACKEND_HERDR_COMPOSER_LINES", 20),
            pi_composer_max_lines: environment_number("MX_BACKEND_HERDR_PI_COMPOSER_MAX_LINES", 8),
            ghost_luma_max: environment_number("MX_COMPOSER_GHOST_LUMA_MAX", 128),
        }
    }

    /// Return the selected named session.
    #[must_use]
    pub fn session(&self) -> &str {
        &self.session
    }

    /// Return the response-derived seeded tab from the last ensure call.
    #[must_use]
    pub fn seeded_tab_id(&self) -> Option<&str> {
        self.seeded_tab_id.as_deref()
    }

    fn request(&self, args: impl IntoIterator<Item = impl Into<OsString>>) -> CommandRequest {
        let mut request = CommandRequest::new(self.executable.clone(), args);
        request.output_limit = OUTPUT_LIMIT;
        request
    }

    fn scoped_request(
        &self,
        session: &str,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> CommandRequest {
        let mut args = args.into_iter().map(Into::into).collect::<Vec<_>>();
        args.push(OsString::from("--session"));
        args.push(OsString::from(session));
        let mut request = self.request(args);
        request
            .env
            .push((OsString::from("HERDR_SESSION"), OsString::from(session)));
        request
    }

    fn run_request(&mut self, request: &CommandRequest) -> Result<CommandOutput, BackendError> {
        self.runner
            .run(request)
            .map_err(|error| BackendError::Command(error.to_string()))
    }

    pub(crate) fn run_scoped(
        &mut self,
        session: &str,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<CommandOutput, BackendError> {
        let request = self.scoped_request(session, args);
        self.run_request(&request)
    }

    pub(crate) fn success_scoped(
        &mut self,
        session: &str,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<CommandOutput, BackendError> {
        let output = self.run_scoped(session, args)?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(command_failure("herdr", &output))
        }
    }

    pub(crate) fn json_scoped(
        &mut self,
        session: &str,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Value, BackendError> {
        let output = self.success_scoped(session, args)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|error| BackendError::Malformed(format!("Herdr JSON: {error}")))
    }

    pub(crate) fn json_any_status(
        &mut self,
        session: &str,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Value, BackendError> {
        let output = self.run_scoped(session, args)?;
        let bytes = if output.stdout.is_empty() {
            &output.stderr
        } else {
            &output.stdout
        };
        serde_json::from_slice(bytes)
            .map_err(|error| BackendError::Malformed(format!("Herdr JSON: {error}")))
    }

    fn ensure_target<'a>(
        &self,
        target: &'a BackendTarget,
    ) -> Result<(&'a str, &'a str), BackendError> {
        if target.backend() != BackendName::Herdr {
            return Err(BackendError::InvalidTarget(target.endpoint().to_owned()));
        }
        parse_target(target.endpoint())
            .ok_or_else(|| BackendError::InvalidTarget(target.endpoint().to_owned()))
    }

    /// Execute an arbitrary already-validated Herdr CLI call with redundant named-session scope.
    pub fn scoped_cli(
        &mut self,
        session: &str,
        args: &[OsString],
    ) -> Result<CommandOutput, BackendError> {
        self.run_scoped(session, args.iter().cloned())
    }

    /// Derive the persistent workspace label for this Multplx home.
    #[must_use]
    pub fn workspace_label(&self) -> String {
        let marker = self.home.join(".mx-daemon-home");
        fs::read_to_string(marker)
            .ok()
            .map(|value| {
                value
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect()
            })
            .filter(|value: &String| !value.is_empty())
            .map_or_else(|| "broker".to_owned(), |id| format!("daemon-{id}"))
    }

    /// Start and poll the exact named server without touching an ambient session.
    pub fn server_ensure(&mut self, session: &str) -> Result<(), BackendError> {
        if self.server_running(session) {
            return Ok(());
        }
        let executable = self.executable.clone();
        let session_owned = session.to_owned();
        let mut child = Command::new(executable)
            .args([
                OsStr::new("server"),
                OsStr::new("--session"),
                OsStr::new(session),
            ])
            .env("HERDR_SESSION", session)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| BackendError::Command(format!("could not start Herdr: {error}")))?;
        thread::spawn(move || {
            let _ = child.wait();
            drop(session_owned);
        });
        for _ in 0..20 {
            if self.server_running(session) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(500));
        }
        Err(BackendError::Command(format!(
            "herdr server for session '{session}' did not report running within 10s"
        )))
    }

    fn server_running(&mut self, session: &str) -> bool {
        self.json_scoped(session, ["status", "--json"])
            .ok()
            .and_then(|value| value.pointer("/server/running").and_then(Value::as_bool))
            == Some(true)
    }

    /// Find the first home-label workspace in the exact named session.
    pub fn workspace_find(&mut self, session: &str) -> Option<String> {
        let label = self.workspace_label();
        self.json_scoped(session, ["workspace", "list"])
            .ok()
            .and_then(|value| {
                array_at(&value, "/result/workspaces")?
                    .iter()
                    .find(|workspace| string_at(workspace, "/label") == Some(label.as_str()))
                    .and_then(|workspace| string_at(workspace, "/workspace_id"))
                    .map(str::to_owned)
            })
    }

    /// Ensure the persistent home workspace and preserve create-vs-adopt seed authority.
    pub fn workspace_ensure(&mut self, session: &str, cwd: &Path) -> Result<String, BackendError> {
        self.seeded_tab_id = None;
        if let Some(workspace) = self.workspace_find(session) {
            return Ok(workspace);
        }
        let label = self.workspace_label();
        let value = self.json_scoped(
            session,
            [
                OsString::from("workspace"),
                OsString::from("create"),
                OsString::from("--cwd"),
                cwd.as_os_str().to_owned(),
                OsString::from("--label"),
                OsString::from(label),
                OsString::from("--no-focus"),
            ],
        )?;
        let workspace = required_string(&value, "/result/workspace/workspace_id")?;
        self.seeded_tab_id = string_at(&value, "/result/tab/tab_id").map(str::to_owned);
        Ok(workspace)
    }

    /// Create one task tab, replacing only positively identified restored husks.
    pub fn create_task_full(
        &mut self,
        container: &ContainerId,
        task: &TaskSpec,
        seeded_tab_id: Option<&str>,
    ) -> Result<HerdrTaskEndpoint, BackendError> {
        if container.backend() != BackendName::Herdr {
            return Err(BackendError::InvalidContainer(
                container.as_str().to_owned(),
            ));
        }
        let (session, workspace) = parse_target(container.as_str())
            .ok_or_else(|| BackendError::InvalidContainer(container.as_str().to_owned()))?;
        let tabs = self.json_scoped(session, ["tab", "list", "--workspace", workspace])?;
        let tabs = array_at(&tabs, "/result/tabs").ok_or_else(|| {
            BackendError::Malformed(format!(
                "could not parse herdr tab list output for workspace {workspace} (session {session})"
            ))
        })?;
        let duplicates = tabs
            .iter()
            .filter(|tab| string_at(tab, "/label") == Some(task.label.as_str()))
            .filter_map(|tab| string_at(tab, "/tab_id").map(str::to_owned))
            .collect::<Vec<_>>();
        for duplicate in &duplicates {
            let pane = self.pane_for_tab(session, workspace, duplicate).ok_or_else(|| {
                BackendError::Command(format!(
                    "herdr tab '{}' already exists in workspace {workspace} (session {session})",
                    task.label
                ))
            })?;
            if !matches!(
                self.pane_agent_state(session, &pane),
                PaneAgentState::Dead | PaneAgentState::NoAgent
            ) {
                return Err(BackendError::Command(format!(
                    "herdr tab '{}' already exists in workspace {workspace} (session {session})",
                    task.label
                )));
            }
        }
        let value = self.json_scoped(
            session,
            [
                OsString::from("tab"),
                OsString::from("create"),
                OsString::from("--workspace"),
                OsString::from(workspace),
                OsString::from("--cwd"),
                task.working_directory.as_os_str().to_owned(),
                OsString::from("--label"),
                OsString::from(&task.label),
                OsString::from("--no-focus"),
            ],
        )?;
        let tab_id = required_string(&value, "/result/tab/tab_id")?;
        let pane_id = required_string(&value, "/result/root_pane/pane_id")?;
        if let Some(seed) = seeded_tab_id.filter(|seed| !seed.is_empty()) {
            self.prune_seeded_tab(session, workspace, seed);
        }
        for duplicate in &duplicates {
            let _ = self.run_scoped(session, ["tab", "close", duplicate]);
        }
        if !duplicates.is_empty() {
            let verify = self.json_scoped(session, ["tab", "list", "--workspace", workspace])?;
            let remaining = array_at(&verify, "/result/tabs")
                .ok_or_else(|| BackendError::Malformed("missing result.tabs".to_owned()))?
                .iter()
                .any(|tab| {
                    string_at(tab, "/label") == Some(task.label.as_str())
                        && string_at(tab, "/tab_id") != Some(tab_id.as_str())
                });
            if remaining {
                return Err(BackendError::Command(format!(
                    "failed to remove preexisting herdr tabs for label '{}'",
                    task.label
                )));
            }
        }
        let target = BackendTarget::new(
            BackendName::Herdr,
            format!("{session}:{pane_id}"),
            Some(task.label.clone()),
        )?;
        Ok(HerdrTaskEndpoint {
            tab_id,
            pane_id,
            target,
        })
    }

    fn prune_seeded_tab(&mut self, session: &str, workspace: &str, seed: &str) {
        let Ok(tabs) = self.json_scoped(session, ["tab", "list", "--workspace", workspace]) else {
            return;
        };
        let Some(tabs) = array_at(&tabs, "/result/tabs") else {
            return;
        };
        if tabs.len() <= 1 {
            return;
        }
        if tabs
            .iter()
            .find(|tab| string_at(tab, "/tab_id") == Some(seed))
            .and_then(|tab| string_at(tab, "/label"))
            != Some("1")
        {
            return;
        }
        let Some(pane) = self.pane_for_tab(session, workspace, seed) else {
            return;
        };
        if self.agent_status_raw(session, &pane).as_deref() == Some("working") {
            return;
        }
        let _ = self.run_scoped(session, ["pane", "close", &pane]);
    }

    /// Classify a pane from exact response bodies rather than command status.
    pub fn pane_agent_state(&mut self, session: &str, pane: &str) -> PaneAgentState {
        let Ok(pane_value) = self.json_any_status(session, ["pane", "get", pane]) else {
            return PaneAgentState::Unknown;
        };
        if string_at(&pane_value, "/error/code") == Some("pane_not_found") {
            return PaneAgentState::Dead;
        }
        if string_at(&pane_value, "/result/pane/pane_id") != Some(pane) {
            return PaneAgentState::Unknown;
        }
        let Ok(agent) = self.json_any_status(session, ["agent", "get", pane]) else {
            return PaneAgentState::Unknown;
        };
        if string_at(&agent, "/error/code") == Some("agent_not_found") {
            return PaneAgentState::NoAgent;
        }
        match string_at(&agent, "/result/agent/agent_status") {
            Some("working" | "idle" | "done" | "blocked") => PaneAgentState::Live,
            _ => PaneAgentState::Unknown,
        }
    }

    /// Return the root pane for one exact tab.
    pub fn pane_for_tab(&mut self, session: &str, workspace: &str, tab: &str) -> Option<String> {
        self.json_scoped(session, ["pane", "list", "--workspace", workspace])
            .ok()
            .and_then(|value| {
                array_at(&value, "/result/panes")?
                    .iter()
                    .find(|pane| string_at(pane, "/tab_id") == Some(tab))
                    .and_then(|pane| string_at(pane, "/pane_id"))
                    .map(str::to_owned)
            })
    }

    /// Return the raw native status when readable.
    pub fn agent_status_raw(&mut self, session: &str, pane: &str) -> Option<String> {
        self.json_scoped(session, ["agent", "get", pane])
            .ok()
            .and_then(|value| string_at(&value, "/result/agent/agent_status").map(str::to_owned))
    }

    /// Return the event socket for one exact running session only.
    pub fn socket_path(&mut self, session: &str) -> Option<PathBuf> {
        let request = self.request(["session", "list", "--json"]);
        let output = self.run_request(&request).ok()?;
        let value: Value = serde_json::from_slice(&output.stdout).ok()?;
        array_at(&value, "/sessions")?
            .iter()
            .find(|entry| string_at(entry, "/name") == Some(session))
            .and_then(|entry| string_at(entry, "/socket_path"))
            .filter(|path| Path::new(path).is_absolute())
            .map(PathBuf::from)
    }

    /// Verify protocol and schema support for native events.
    pub fn events_capable(&mut self, session: &str) -> bool {
        match std::env::var("MX_BACKEND_HERDR_EVENTS_FORCE").as_deref() {
            Ok("1") => return true,
            Ok("0") => return false,
            _ => {}
        }
        let Ok(status) = self.version_status() else {
            return false;
        };
        if status
            .pointer("/client/protocol")
            .and_then(Value::as_u64)
            .is_none_or(|protocol| protocol < MIN_EVENTS_PROTOCOL)
        {
            return false;
        }
        let Ok(schema) = self.json_scoped(session, ["api", "schema", "--json"]) else {
            return false;
        };
        let rendered = schema.to_string();
        rendered.contains("events.subscribe") && rendered.contains("pane.agent_status_changed")
    }

    fn version_status(&mut self) -> Result<Value, BackendError> {
        let request = self.request(["status", "--json"]);
        let output = self.run_request(&request)?;
        if !output.status.success() {
            return Err(command_failure("herdr", &output));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|error| BackendError::Malformed(error.to_string()))
    }

    /// Wait on native events, reconcile levels, and maintain committed dedupe markers.
    pub fn wait_transition_in_state(
        &mut self,
        session: &str,
        timeout: Duration,
        state: &Path,
        windows: &[String],
    ) -> Result<Option<TransitionRecord>, BackendError> {
        if windows.is_empty() || !self.events_capable(session) {
            return Err(BackendError::Unsupported {
                backend: BackendName::Herdr,
                capability: "native transition events",
            });
        }
        let panes = windows
            .iter()
            .filter_map(|window| parse_target(window).map(|(_, pane)| pane.to_owned()))
            .collect::<Vec<_>>();
        if panes.is_empty() {
            return Err(BackendError::InvalidTarget("no Herdr panes".to_owned()));
        }
        let socket = self
            .socket_path(session)
            .ok_or_else(|| BackendError::Command("could not resolve Herdr socket".to_owned()))?;
        // Subscription must be active before level reconciliation. The wire reader emits the
        // acknowledgement synchronously, so collect the bounded stream in a child thread while
        // this process performs the initial level read.
        let panes_for_thread = panes.clone();
        let socket_for_thread = socket.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let thread_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            herdr_wire::event_wait_cancelled(
                &socket_for_thread,
                timeout,
                &panes_for_thread,
                &thread_cancelled,
                |line| {
                    sender.send(line.to_owned()).map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "cancelled")
                    })
                },
            )
        });
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(line) if line == "@subscribed" => {}
            _ => {
                cancelled.store(true, Ordering::Release);
                drop(receiver);
                let _ = handle.join();
                return Err(BackendError::Command(
                    "Herdr event subscription failed".to_owned(),
                ));
            }
        }
        for pane in &panes {
            if let Some(status) = self.agent_status_raw(session, pane) {
                let record = TransitionRecord::new(pane, "", "", &status, "");
                if apply_transition(state, session, &record)? {
                    cancelled.store(true, Ordering::Release);
                    drop(receiver);
                    let _ = handle.join();
                    return Ok(Some(record));
                }
            }
        }
        while let Ok(line) = receiver.recv() {
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() != 4 || fields[0].is_empty() {
                continue;
            }
            let record = TransitionRecord::new(fields[0], fields[1], "", fields[2], fields[3]);
            if apply_transition(state, session, &record)? {
                cancelled.store(true, Ordering::Release);
                drop(receiver);
                let _ = handle.join();
                return Ok(Some(record));
            }
        }
        match handle.join() {
            Ok(Ok(())) => Ok(None),
            _ => Err(BackendError::Command(
                "Herdr event reader failed".to_owned(),
            )),
        }
    }

    fn capture_with_format(
        &mut self,
        target: &BackendTarget,
        lines: u32,
        ansi: bool,
    ) -> Result<Vec<u8>, BackendError> {
        let (session, pane) = self.ensure_target(target)?;
        self.server_ensure(session)?;
        let requested = if lines == 0 { 200 } else { lines };
        let fetch = requested.max(200);
        let mut args = vec![
            OsString::from("pane"),
            OsString::from("read"),
            OsString::from(pane),
            OsString::from("--source"),
            OsString::from("recent"),
            OsString::from("--lines"),
            OsString::from(fetch.to_string()),
        ];
        if ansi {
            args.extend([OsString::from("--format"), OsString::from("ansi")]);
        }
        let output = self.success_scoped(session, args)?;
        Ok(tail_lines(&output.stdout, requested as usize))
    }

    /// Capture a bounded ANSI pane tail for compatibility presentation callers.
    pub fn capture_ansi(
        &mut self,
        target: &BackendTarget,
        lines: u32,
    ) -> Result<Vec<u8>, BackendError> {
        self.capture_with_format(target, lines, true)
    }

    fn composer_from_capture(
        &mut self,
        session: &str,
        pane: &str,
        capture: &[u8],
    ) -> ComposerState {
        let rows = capture.split(|byte| *byte == b'\n').collect::<Vec<_>>();
        let mut generic: Option<(usize, bool, Vec<u8>)> = None;
        for (index, row) in rows.iter().enumerate() {
            let plain = String::from_utf8_lossy(&strip_ansi(row)).trim().to_owned();
            if plain.is_empty() {
                continue;
            }
            let bordered = ((plain.starts_with('│') && plain.ends_with('│'))
                || (plain.starts_with('┃') && plain.ends_with('┃'))
                || (plain.starts_with('|') && plain.ends_with('|')))
                && plain.chars().count() >= 2;
            let bare = plain.starts_with('❯') || plain.starts_with('›');
            if bordered || bare {
                generic = Some((index, bordered, (*row).to_vec()));
            }
        }
        let pi = bottom_pi_pair(&rows, self.pi_composer_max_lines);
        let selected = match (generic, pi) {
            (generic, Some((open, close, content)))
                if generic.as_ref().is_none_or(|(row, _, _)| *row < open) =>
            {
                let allowed = self
                    .json_scoped(session, ["agent", "get", pane])
                    .ok()
                    .map(|value| {
                        string_at(&value, "/result/agent/agent") == Some("pi")
                            && matches!(
                                string_at(&value, "/result/agent/agent_status"),
                                Some("idle" | "done" | "blocked")
                            )
                    })
                    .unwrap_or(false);
                if allowed && close > open {
                    Some((true, content))
                } else {
                    None
                }
            }
            (Some((_, bordered, row)), _) => Some((bordered, row)),
            _ => None,
        };
        let Some((bordered, raw)) = selected else {
            return ComposerState::Unknown;
        };
        let plain = String::from_utf8_lossy(&strip_ansi(&raw)).trim().to_owned();
        let mut content = String::from_utf8_lossy(&strip_ghost(&raw, self.ghost_luma_max))
            .trim()
            .to_owned();
        if bordered {
            content = content.replace(['│', '┃', '|'], "").trim().to_owned();
        }
        classify_content(
            bordered,
            &content,
            Some(r"^Type a message\.\.\.$"),
            false,
            Some(&plain),
        )
        .unwrap_or(ComposerState::Unknown)
    }

    fn post_kill(&mut self, session: &str, pane: &str) -> KillOutcome {
        match self.pane_agent_state(session, pane) {
            PaneAgentState::Dead => KillOutcome::Gone,
            PaneAgentState::NoAgent | PaneAgentState::Live => KillOutcome::StillPresent,
            PaneAgentState::Unknown => KillOutcome::Unknown,
        }
    }
}

impl<R: CommandRunner> RuntimeBackend for HerdrBackend<R> {
    fn name(&self) -> BackendName {
        BackendName::Herdr
    }

    fn supports(&self, capability: Capability) -> bool {
        matches!(
            capability,
            Capability::NativeState
                | Capability::TransitionEvents
                | Capability::ComposerState
                | Capability::AgentState
        )
    }

    fn tool_check(&mut self) -> Result<(), BackendError> {
        let request = self.request(["--version"]);
        let output = self.run_request(&request)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(command_failure("herdr", &output))
        }
    }

    fn version_check(&mut self) -> Result<String, BackendError> {
        self.tool_check()?;
        let status = self.version_status()?;
        let protocol = status
            .pointer("/client/protocol")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                BackendError::Malformed(
                    "could not read herdr client protocol from 'herdr status --json'".to_owned(),
                )
            })?;
        let version = status
            .pointer("/client/version")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if protocol < MIN_PROTOCOL {
            return Err(BackendError::Command(format!(
                "herdr protocol {protocol} (version {version}) is older than the verified minimum {MIN_PROTOCOL}"
            )));
        }
        Ok(version.to_owned())
    }

    fn container_ensure(&mut self) -> Result<ContainerId, BackendError> {
        self.version_check()?;
        let session = self.session.clone();
        self.server_ensure(&session)?;
        let cwd =
            std::env::current_dir().map_err(|error| BackendError::Metadata(error.to_string()))?;
        let workspace = self.workspace_ensure(&session, &cwd)?;
        ContainerId::for_backend(BackendName::Herdr, format!("{session}:{workspace}"))
    }

    fn task_create(
        &mut self,
        container: &ContainerId,
        task: &TaskSpec,
    ) -> Result<BackendTarget, BackendError> {
        let seed = self.seeded_tab_id.clone();
        self.create_task_full(container, task, seed.as_deref())
            .map(|endpoint| endpoint.target)
    }

    fn target_ready(&mut self, target: &BackendTarget) -> Result<(), BackendError> {
        let (session, _) = self.ensure_target(target)?;
        self.server_ensure(session)
    }

    fn current_path(&mut self, target: &BackendTarget) -> Result<PathBuf, BackendError> {
        let (session, pane) = self.ensure_target(target)?;
        self.server_ensure(session)?;
        let value = self.json_scoped(session, ["pane", "get", pane])?;
        required_string(&value, "/result/pane/foreground_cwd").map(PathBuf::from)
    }

    fn capture(&mut self, request: &CaptureRequest) -> Result<Vec<u8>, BackendError> {
        let mut output = self.capture_with_format(&request.target, request.lines, false)?;
        if output.len() > request.byte_limit.min(CAPTURE_LIMIT) {
            return Err(BackendError::Malformed(
                "Herdr capture exceeded byte limit".to_owned(),
            ));
        }
        output.shrink_to_fit();
        Ok(output)
    }

    fn composer_state(&mut self, target: &BackendTarget) -> Result<ComposerState, BackendError> {
        let (session, pane) = self.ensure_target(target)?;
        let capture = self
            .capture_with_format(target, self.composer_lines, true)
            .or_else(|_| self.capture_with_format(target, self.composer_lines, false))?;
        Ok(self.composer_from_capture(session, pane, &capture))
    }

    fn send_literal(&mut self, target: &BackendTarget, text: &str) -> Result<(), BackendError> {
        let (session, pane) = self.ensure_target(target)?;
        self.server_ensure(session)?;
        self.success_scoped(session, ["pane", "send-text", pane, text])
            .map(|_| ())
    }

    fn send_key(&mut self, target: &BackendTarget, key: &str) -> Result<(), BackendError> {
        let (session, pane) = self.ensure_target(target)?;
        self.server_ensure(session)?;
        self.success_scoped(session, ["pane", "send-keys", pane, normalize_key(key)])
            .map(|_| ())
    }

    fn send_submit(
        &mut self,
        target: &BackendTarget,
        request: SubmitRequest<'_>,
    ) -> Result<ComposerState, BackendError> {
        let (session, pane) = self.ensure_target(target)?;
        let session = session.to_owned();
        let pane = pane.to_owned();
        self.send_literal(target, request.text)?;
        crate::facade::compatibility_sleep(request.settle);
        let baseline = classify_submit(self.agent_status_raw(&session, &pane).as_deref());
        let attempts = request.retries.max(1);
        let budget = request.enter_delay.max(Duration::from_millis(600));
        for _ in 0..attempts {
            let _ = self.send_key(target, "Enter");
            let verdict = if baseline == SubmitState::Idle {
                self.wait_for_working(&session, &pane, budget, 6)
            } else {
                crate::facade::compatibility_sleep(request.enter_delay);
                match self.composer_state(target)? {
                    ComposerState::Empty => SubmitState::Busy,
                    ComposerState::Pending => SubmitState::Idle,
                    ComposerState::Unknown => SubmitState::Unknown,
                }
            };
            match verdict {
                SubmitState::Busy => return Ok(ComposerState::Empty),
                SubmitState::Unknown => return Ok(ComposerState::Unknown),
                SubmitState::Idle => {}
            }
        }
        Ok(ComposerState::Pending)
    }

    fn send_text_line(&mut self, target: &BackendTarget, text: &str) -> Result<(), BackendError> {
        let (session, pane) = self.ensure_target(target)?;
        self.server_ensure(session)?;
        self.success_scoped(session, ["pane", "run", pane, text])
            .map(|_| ())
    }

    fn native_state(&mut self, target: &BackendTarget) -> Result<NativeState, BackendError> {
        let (session, pane) = self.ensure_target(target)?;
        self.server_ensure(session)?;
        match self.agent_status_raw(session, pane).as_deref() {
            Some("idle") => Ok(NativeState::Idle),
            Some("working") => Ok(NativeState::Working),
            Some("blocked") => Ok(NativeState::Blocked),
            Some("done") => Ok(NativeState::Done),
            _ => Err(BackendError::Malformed(
                "unknown Herdr native state".to_owned(),
            )),
        }
    }

    fn agent_state(&mut self, target: &BackendTarget) -> AgentState {
        let Ok((session, pane)) = self.ensure_target(target) else {
            return AgentState::Unreadable;
        };
        match self.pane_agent_state(session, pane) {
            PaneAgentState::Dead => AgentState::Missing,
            PaneAgentState::NoAgent => AgentState::Dead,
            PaneAgentState::Live => AgentState::Alive,
            PaneAgentState::Unknown => AgentState::Unreadable,
        }
    }

    fn kill_verified(&mut self, target: &BackendTarget) -> KillOutcome {
        let Ok((session, pane)) = self.ensure_target(target) else {
            return KillOutcome::Unknown;
        };
        let session = session.to_owned();
        let pane = pane.to_owned();
        let _ = self.run_scoped(&session, ["pane", "close", &pane]);
        self.post_kill(&session, &pane)
    }

    fn list_live(
        &mut self,
        container: Option<&ContainerId>,
    ) -> Result<Vec<LiveTarget>, BackendError> {
        let (session, workspace) = if let Some(container) = container {
            if container.backend() != BackendName::Herdr {
                return Err(BackendError::InvalidContainer(
                    container.as_str().to_owned(),
                ));
            }
            let (session, workspace) = parse_target(container.as_str())
                .ok_or_else(|| BackendError::InvalidContainer(container.as_str().to_owned()))?;
            (session.to_owned(), Some(workspace.to_owned()))
        } else {
            let session = self.session.clone();
            let workspace = self.workspace_find(&session);
            (session, workspace)
        };
        let Some(workspace) = workspace else {
            return Ok(Vec::new());
        };
        let value = self.json_scoped(&session, ["tab", "list", "--workspace", &workspace])?;
        let mut live = Vec::new();
        for tab in array_at(&value, "/result/tabs").unwrap_or(&[]) {
            let Some(label) = string_at(tab, "/label").filter(|label| label.starts_with("mx-"))
            else {
                continue;
            };
            let Some(tab_id) = string_at(tab, "/tab_id") else {
                continue;
            };
            let Some(pane) = self.pane_for_tab(&session, &workspace, tab_id) else {
                continue;
            };
            live.push(LiveTarget {
                target: BackendTarget::new(
                    BackendName::Herdr,
                    format!("{session}:{pane}"),
                    Some(label.to_owned()),
                )?,
                label: label.to_owned(),
            });
        }
        Ok(live)
    }

    fn wait_transition(
        &mut self,
        container: &ContainerId,
        targets: &[BackendTarget],
        timeout: Duration,
    ) -> Result<Option<String>, BackendError> {
        let (session, _) = parse_target(container.as_str())
            .ok_or_else(|| BackendError::InvalidContainer(container.as_str().to_owned()))?;
        let state = std::env::var_os("MX_STATE_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join("state"));
        let windows = targets
            .iter()
            .map(|target| target.endpoint().to_owned())
            .collect::<Vec<_>>();
        self.wait_transition_in_state(session, timeout, &state, &windows)
            .map(|record| record.map(|record| record.render()))
    }
}

impl<R: CommandRunner> LiveInventory for HerdrBackend<R> {
    fn list_live(
        &mut self,
        container: Option<&ContainerId>,
    ) -> Result<Vec<LiveTarget>, BackendError> {
        RuntimeBackend::list_live(self, container)
    }
}

impl<R: CommandRunner> HerdrBackend<R> {
    fn wait_for_working(
        &mut self,
        session: &str,
        pane: &str,
        budget: Duration,
        polls: usize,
    ) -> SubmitState {
        let polls = polls.max(1);
        let interval = budget.div_f64((polls.saturating_sub(1).max(1)) as f64);
        let mut saw_idle = false;
        for index in 0..polls {
            if polls == 1 || index > 0 {
                thread::sleep(interval);
            }
            match classify_submit(self.agent_status_raw(session, pane).as_deref()) {
                SubmitState::Busy => return SubmitState::Busy,
                SubmitState::Idle => saw_idle = true,
                SubmitState::Unknown => {}
            }
        }
        if saw_idle {
            SubmitState::Idle
        } else {
            SubmitState::Unknown
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmitState {
    Busy,
    Idle,
    Unknown,
}

fn classify_submit(value: Option<&str>) -> SubmitState {
    match value {
        Some("working" | "blocked") => SubmitState::Busy,
        Some("idle" | "done") => SubmitState::Idle,
        _ => SubmitState::Unknown,
    }
}

fn normalize_key(key: &str) -> &str {
    match key {
        "Enter" | "enter" => "enter",
        "Escape" | "escape" | "Esc" | "esc" => "escape",
        "C-c" | "c-c" | "ctrl+c" | "Ctrl+C" => "ctrl+c",
        other => other,
    }
}

/// Split `<session>:<pane-or-workspace>` on the first colon only.
#[must_use]
pub fn parse_target(target: &str) -> Option<(&str, &str)> {
    let (session, endpoint) = target.split_once(':')?;
    if session.is_empty() || endpoint.is_empty() {
        None
    } else {
        Some((session, endpoint))
    }
}

fn command_failure(program: &str, output: &CommandOutput) -> BackendError {
    BackendError::Command(format!(
        "{program} exited {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn string_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn required_string(value: &Value, pointer: &str) -> Result<String, BackendError> {
    string_at(value, pointer)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BackendError::Malformed(format!("missing string at {pointer}")))
}

fn array_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a [Value]> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

fn tail_lines(bytes: &[u8], lines: usize) -> Vec<u8> {
    if lines == 0 || bytes.is_empty() {
        return Vec::new();
    }
    let content_end = bytes.len() - usize::from(bytes.ends_with(b"\n"));
    let separators = bytes
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'\n' && index < content_end).then_some(index))
        .collect::<Vec<_>>();
    let start = if separators.len() < lines {
        0
    } else {
        separators[separators.len() - lines] + 1
    };
    bytes[start..].to_vec()
}

fn separator(row: &[u8]) -> bool {
    let plain = String::from_utf8_lossy(&strip_ansi(row)).trim().to_owned();
    plain.chars().count() >= 8 && plain.chars().all(|character| character == '─')
}

fn bottom_pi_pair(rows: &[&[u8]], maximum: usize) -> Option<(usize, usize, Vec<u8>)> {
    let separators = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| separator(row).then_some(index))
        .collect::<Vec<_>>();
    let close = *separators.last()?;
    let open = *separators.iter().rev().nth(1)?;
    if close.saturating_sub(open + 1) > maximum {
        return None;
    }
    let content = rows[open + 1..close].join(&b'\n');
    Some((open, close, content))
}

fn escalation_marker(state: &Path, session: &str, pane: &str) -> PathBuf {
    let key = format!("{session}:{pane}")
        .chars()
        .map(|character| match character {
            ':' | '/' | '.' => '_',
            other => other,
        })
        .collect::<String>();
    state.join(format!(".herdr-escalated-{key}"))
}

fn apply_transition(
    state: &Path,
    session: &str,
    record: &TransitionRecord,
) -> Result<bool, BackendError> {
    let marker = escalation_marker(state, session, &record.pane_id);
    match policy(&record.to_status) {
        TransitionAction::Actionable => Ok(!marker.exists()),
        TransitionAction::Absorb => {
            match fs::remove_file(marker) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(BackendError::Metadata(error.to_string())),
            }
            Ok(false)
        }
        TransitionAction::Defer | TransitionAction::Fallback => Ok(false),
    }
}

/// Commit one handled actionable transition.
pub fn commit_transition(
    state: &Path,
    session: &str,
    record: &TransitionRecord,
) -> Result<(), BackendError> {
    fs::write(escalation_marker(state, session, &record.pane_id), [])
        .map_err(|error| BackendError::Metadata(error.to_string()))
}

/// Clear one endpoint's transition marker.
pub fn clear_transition(state: &Path, window: &str) -> Result<(), BackendError> {
    let (session, pane) =
        parse_target(window).ok_or_else(|| BackendError::InvalidTarget(window.to_owned()))?;
    match fs::remove_file(escalation_marker(state, session, pane)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(BackendError::Metadata(error.to_string())),
    }
}

fn environment_number<T>(name: &str, default: T) -> T
where
    T: std::str::FromStr + Copy,
{
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::ffi::OsString;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::ExitStatus;
    use std::time::Duration;

    use multplx_core::composer::ComposerState;
    use multplx_core::transition::TransitionRecord;

    use super::{
        HerdrBackend, PaneAgentState, SubmitState, apply_transition, bottom_pi_pair,
        clear_transition, commit_transition, normalize_key, parse_target, tail_lines,
    };
    use crate::command::{CommandError, CommandOutput, CommandRequest, CommandRunner};
    use crate::facade::{
        AgentState, BackendName, BackendTarget, Capability, CaptureRequest, ContainerId,
        KillOutcome, LiveInventory, NativeState, RuntimeBackend, SubmitRequest, TaskSpec,
    };

    #[derive(Debug, Default)]
    struct NeverRunner;

    impl CommandRunner for NeverRunner {
        fn run(&mut self, _: &CommandRequest) -> Result<CommandOutput, CommandError> {
            panic!("unexpected command")
        }
    }

    #[derive(Debug, Default)]
    struct SmartRunner {
        calls: Vec<CommandRequest>,
    }

    #[derive(Debug)]
    struct SequenceRunner {
        outputs: VecDeque<CommandOutput>,
    }

    impl SequenceRunner {
        fn new(outputs: impl IntoIterator<Item = CommandOutput>) -> Self {
            Self {
                outputs: outputs.into_iter().collect(),
            }
        }
    }

    impl CommandRunner for SequenceRunner {
        fn run(&mut self, _: &CommandRequest) -> Result<CommandOutput, CommandError> {
            Ok(self.outputs.pop_front().expect("queued command output"))
        }
    }

    fn success(stdout: impl Into<Vec<u8>>) -> CommandOutput {
        CommandOutput {
            status: ExitStatus::from_raw(0),
            stdout: stdout.into(),
            stderr: Vec::new(),
        }
    }

    fn failure(stderr: impl Into<Vec<u8>>) -> CommandOutput {
        CommandOutput {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: stderr.into(),
        }
    }

    impl CommandRunner for SmartRunner {
        fn run(&mut self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
            self.calls.push(request.clone());
            let args = request
                .args
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>();
            let body = match args.first().map(|arg| arg.as_ref()) {
                Some("--version") => b"herdr 0.7.4\n".to_vec(),
                Some("status") => br#"{"client":{"version":"0.7.4","protocol":16},"server":{"running":true}}"#.to_vec(),
                Some("workspace") if args.get(1).is_some_and(|arg| arg == "list") => br#"{"result":{"workspaces":[{"workspace_id":"w1","label":"broker","is_active":true}]}}"#.to_vec(),
                Some("workspace") if args.get(1).is_some_and(|arg| arg == "create") => br#"{"result":{"workspace":{"workspace_id":"w1"},"tab":{"tab_id":"w1:t1"},"root_pane":{"pane_id":"w1:p1"}}}"#.to_vec(),
                Some("tab") if args.get(1).is_some_and(|arg| arg == "list") => br#"{"result":{"tabs":[{"tab_id":"w1:t2","workspace_id":"w1","label":"mx-task","is_active":true}]}}"#.to_vec(),
                Some("tab") if args.get(1).is_some_and(|arg| arg == "create") => br#"{"result":{"tab":{"tab_id":"w1:t2"},"root_pane":{"pane_id":"w1:p2"}}}"#.to_vec(),
                Some("pane") if args.get(1).is_some_and(|arg| arg == "list") => br#"{"result":{"panes":[{"pane_id":"w1:p2","tab_id":"w1:t2","workspace_id":"w1","is_active":true}]}}"#.to_vec(),
                Some("pane") if args.get(1).is_some_and(|arg| arg == "get") => {
                    let pane = args.get(2).map_or("w1:p2", |arg| arg.as_ref());
                    format!(r#"{{"result":{{"pane":{{"pane_id":"{pane}","tab_id":"w1:t2","workspace_id":"w1","foreground_cwd":"/tmp/work","shell_pid":4242}}}}}}"#).into_bytes()
                }
                Some("pane") if args.get(1).is_some_and(|arg| arg == "read") => "│ typed │\n".as_bytes().to_vec(),
                Some("agent") if args.get(1).is_some_and(|arg| arg == "get") => br#"{"result":{"agent":{"agent":"codex","agent_status":"working"}}}"#.to_vec(),
                Some("api") => br#"{"methods":["events.subscribe"],"events":["pane.agent_status_changed"]}"#.to_vec(),
                Some("session") => br#"{"sessions":[{"name":"named","running":true,"socket_path":"/tmp/herdr.sock"}]}"#.to_vec(),
                _ => Vec::new(),
            };
            Ok(success(body))
        }
    }

    fn herdr_target(value: &str) -> BackendTarget {
        BackendTarget::new(BackendName::Herdr, value, Some("mx-task".to_owned())).expect("target")
    }

    #[test]
    fn target_key_and_workspace_contracts_are_exact() {
        assert_eq!(parse_target("named:w1:p2"), Some(("named", "w1:p2")));
        assert_eq!(parse_target("missing"), None);
        assert_eq!(normalize_key("Enter"), "enter");
        assert_eq!(normalize_key("C-c"), "ctrl+c");
        assert_eq!(normalize_key("F5"), "F5");
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join(".mx-daemon-home"), "wheelhouse\n").expect("marker");
        let backend = HerdrBackend::new(NeverRunner, "herdr", "named", temp.path().to_owned());
        assert_eq!(backend.workspace_label(), "daemon-wheelhouse");
        let primary = HerdrBackend::new(NeverRunner, "herdr", "named", PathBuf::from("/missing"));
        assert_eq!(primary.workspace_label(), "broker");
    }

    #[test]
    fn pane_state_tokens_and_capture_tail_match_legacy_shapes() {
        assert_eq!(PaneAgentState::Dead.as_str(), "dead");
        assert_eq!(PaneAgentState::NoAgent.as_str(), "no-agent");
        assert_eq!(PaneAgentState::Live.as_str(), "live");
        assert_eq!(PaneAgentState::Unknown.as_str(), "unknown");
        assert_eq!(tail_lines(b"one\ntwo\nthree", 2), b"two\nthree");
        assert_eq!(tail_lines(b"one\ntwo\nthree\n", 2), b"two\nthree\n");
        assert_eq!(tail_lines(b"one\n", 200), b"one\n");
        assert_eq!(tail_lines(b"one", 200), b"one");
    }

    #[test]
    fn blocked_markers_commit_only_after_handling_and_working_clears() {
        let temp = tempfile::tempdir().expect("tempdir");
        let blocked = TransitionRecord::new("w:p", "w", "", "blocked", "claude");
        assert!(apply_transition(temp.path(), "default", &blocked).expect("apply"));
        commit_transition(temp.path(), "default", &blocked).expect("commit");
        assert!(!apply_transition(temp.path(), "default", &blocked).expect("dedupe"));
        let working = TransitionRecord::new("w:p", "w", "", "working", "claude");
        assert!(!apply_transition(temp.path(), "default", &working).expect("working"));
        assert!(apply_transition(temp.path(), "default", &blocked).expect("reblock"));
        commit_transition(temp.path(), "default", &blocked).expect("commit");
        clear_transition(temp.path(), "default:w:p").expect("clear");
        assert!(apply_transition(temp.path(), "default", &blocked).expect("fresh"));
    }

    #[test]
    fn complete_runtime_surface_uses_typed_scoped_responses() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut backend = HerdrBackend::new(
            SmartRunner::default(),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert_eq!(backend.name(), BackendName::Herdr);
        assert_eq!(backend.session(), "named");
        for capability in [
            Capability::NativeState,
            Capability::TransitionEvents,
            Capability::ComposerState,
            Capability::AgentState,
        ] {
            assert!(backend.supports(capability));
        }
        backend.tool_check().expect("tool");
        assert_eq!(backend.version_check().expect("version"), "0.7.4");
        backend.server_ensure("named").expect("server");
        assert_eq!(backend.workspace_find("named").as_deref(), Some("w1"));
        assert_eq!(
            backend
                .workspace_ensure("named", temp.path())
                .expect("workspace"),
            "w1"
        );
        assert_eq!(backend.seeded_tab_id(), None);
        let container = backend.container_ensure().expect("container");
        assert_eq!(container.as_str(), "named:w1");
        let endpoint = backend
            .create_task_full(
                &container,
                &TaskSpec {
                    label: "new-task".to_owned(),
                    working_directory: temp.path().to_owned(),
                },
                None,
            )
            .expect("task");
        assert_eq!(endpoint.tab_id, "w1:t2");
        assert_eq!(endpoint.pane_id, "w1:p2");
        let target = herdr_target("named:w1:p2");
        backend.target_ready(&target).expect("ready");
        assert_eq!(
            backend.current_path(&target).expect("path"),
            PathBuf::from("/tmp/work")
        );
        assert_eq!(
            backend
                .capture(&CaptureRequest {
                    target: target.clone(),
                    lines: 5,
                    byte_limit: 4096,
                })
                .expect("capture"),
            "│ typed │\n".as_bytes()
        );
        assert_eq!(
            backend.capture_ansi(&target, 0).expect("ansi"),
            "│ typed │\n".as_bytes()
        );
        assert_eq!(
            backend.composer_state(&target).expect("composer"),
            ComposerState::Pending
        );
        backend.send_literal(&target, "literal").expect("literal");
        backend.send_key(&target, "Escape").expect("key");
        backend.send_text_line(&target, "line").expect("line");
        assert_eq!(
            backend
                .send_submit(
                    &target,
                    SubmitRequest {
                        text: "submit",
                        retries: 1,
                        enter_delay: Duration::ZERO,
                        settle: Duration::ZERO,
                    },
                )
                .expect("submit"),
            ComposerState::Pending
        );
        assert_eq!(
            backend.native_state(&target).expect("native"),
            NativeState::Working
        );
        assert_eq!(backend.agent_state(&target), AgentState::Alive);
        assert_eq!(backend.kill_verified(&target), KillOutcome::StillPresent);
        let live = RuntimeBackend::list_live(&mut backend, Some(&container)).expect("live");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].target.endpoint(), "named:w1:p2");
        assert_eq!(
            backend.socket_path("named"),
            Some(PathBuf::from("/tmp/herdr.sock"))
        );
        assert!(backend.events_capable("named"));
        assert!(
            backend
                .runner
                .calls
                .iter()
                .filter(|call| call
                    .args
                    .first()
                    .is_some_and(|arg| arg != "--version" && arg != "status" && arg != "session"))
                .all(|call| call
                    .args
                    .ends_with(&[OsString::from("--session"), OsString::from("named")]))
        );
    }

    #[test]
    fn malformed_and_cross_backend_inputs_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut backend = HerdrBackend::new(
            SmartRunner::default(),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        let tmux_target = BackendTarget::new(BackendName::Tmux, "s:w", None).expect("tmux");
        assert!(backend.target_ready(&tmux_target).is_err());
        assert_eq!(backend.agent_state(&tmux_target), AgentState::Unreadable);
        assert_eq!(backend.kill_verified(&tmux_target), KillOutcome::Unknown);
        assert!(
            backend
                .create_task_full(
                    &ContainerId::for_backend(BackendName::Tmux, "broker").expect("container"),
                    &TaskSpec {
                        label: "mx-task".to_owned(),
                        working_directory: temp.path().to_owned(),
                    },
                    None,
                )
                .is_err()
        );
        assert!(
            RuntimeBackend::list_live(
                &mut backend,
                Some(&ContainerId::for_backend(BackendName::Tmux, "broker").expect("container"))
            )
            .is_err()
        );
        assert!(
            backend
                .capture(&CaptureRequest {
                    target: herdr_target("named:w1:p2"),
                    lines: 1,
                    byte_limit: 2,
                })
                .is_err()
        );
        assert!(
            backend
                .wait_transition(
                    &ContainerId::for_backend(BackendName::Herdr, "named:w1").expect("container"),
                    &[],
                    Duration::ZERO,
                )
                .is_err()
        );
    }

    #[test]
    fn pane_agent_state_distinguishes_authoritative_and_ambiguous_responses() {
        let temp = tempfile::tempdir().expect("tempdir");
        for (outputs, expected) in [
            (
                vec![success(br#"{"error":{"code":"pane_not_found"}}"#)],
                PaneAgentState::Dead,
            ),
            (
                vec![
                    success(br#"{"result":{"pane":{"pane_id":"w:p"}}}"#),
                    success(br#"{"error":{"code":"agent_not_found"}}"#),
                ],
                PaneAgentState::NoAgent,
            ),
            (
                vec![
                    success(br#"{"result":{"pane":{"pane_id":"w:p"}}}"#),
                    success(br#"{"result":{"agent":{"agent_status":"idle"}}}"#),
                ],
                PaneAgentState::Live,
            ),
            (
                vec![success(br#"{"result":{"pane":{"pane_id":"other"}}}"#)],
                PaneAgentState::Unknown,
            ),
            (vec![success(b"not json")], PaneAgentState::Unknown),
        ] {
            let mut backend = HerdrBackend::new(
                SequenceRunner::new(outputs),
                "herdr",
                "named",
                temp.path().to_owned(),
            );
            assert_eq!(backend.pane_agent_state("named", "w:p"), expected);
        }
    }

    #[test]
    fn workspace_creation_preserves_response_derived_seed_authority() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut backend = HerdrBackend::new(
            SequenceRunner::new([
                success(br#"{"result":{"workspaces":[]}}"#),
                success(
                    br#"{"result":{"workspace":{"workspace_id":"w2"},"tab":{"tab_id":"w2:seed"},"root_pane":{"pane_id":"w2:p1"}}}"#,
                ),
            ]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert_eq!(
            backend
                .workspace_ensure("named", temp.path())
                .expect("create workspace"),
            "w2"
        );
        assert_eq!(backend.seeded_tab_id(), Some("w2:seed"));
    }

    #[derive(Debug, Default)]
    struct DuplicateRunner {
        tab_lists: usize,
        pane_lists: usize,
    }

    impl CommandRunner for DuplicateRunner {
        fn run(&mut self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
            let args = request
                .args
                .iter()
                .map(|argument| argument.to_string_lossy())
                .collect::<Vec<_>>();
            let output = match args.first().map(|argument| argument.as_ref()) {
                Some("tab") if args.get(1).is_some_and(|argument| argument == "list") => {
                    self.tab_lists += 1;
                    match self.tab_lists {
                        1 => br#"{"result":{"tabs":[{"tab_id":"w:dup","label":"mx-task"},{"tab_id":"w:seed","label":"1"}]}}"#.to_vec(),
                        2 => br#"{"result":{"tabs":[{"tab_id":"w:dup","label":"mx-task"},{"tab_id":"w:seed","label":"1"},{"tab_id":"w:new","label":"mx-task"}]}}"#.to_vec(),
                        _ => br#"{"result":{"tabs":[{"tab_id":"w:new","label":"mx-task"}]}}"#.to_vec(),
                    }
                }
                Some("tab") if args.get(1).is_some_and(|argument| argument == "create") => {
                    br#"{"result":{"tab":{"tab_id":"w:new"},"root_pane":{"pane_id":"w:new-pane"}}}"#
                        .to_vec()
                }
                Some("pane") if args.get(1).is_some_and(|argument| argument == "list") => {
                    self.pane_lists += 1;
                    if self.pane_lists == 1 {
                        br#"{"result":{"panes":[{"tab_id":"w:dup","pane_id":"w:dup-pane"}]}}"#
                            .to_vec()
                    } else {
                        br#"{"result":{"panes":[{"tab_id":"w:seed","pane_id":"w:seed-pane"}]}}"#
                            .to_vec()
                    }
                }
                Some("pane") if args.get(1).is_some_and(|argument| argument == "get") => {
                    br#"{"result":{"pane":{"pane_id":"w:dup-pane"}}}"#.to_vec()
                }
                Some("agent") if args.get(1).is_some_and(|argument| argument == "get") => {
                    if args.get(2).is_some_and(|argument| argument == "w:dup-pane") {
                        br#"{"error":{"code":"agent_not_found"}}"#.to_vec()
                    } else {
                        br#"{"result":{"agent":{"agent_status":"idle"}}}"#.to_vec()
                    }
                }
                Some("pane") | Some("tab") => Vec::new(),
                _ => panic!("unexpected duplicate command: {args:?}"),
            };
            Ok(success(output))
        }
    }

    #[test]
    fn task_creation_replaces_only_verified_husks_and_prunes_idle_seed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut backend = HerdrBackend::new(
            DuplicateRunner::default(),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        backend.seeded_tab_id = Some("w:seed".to_owned());
        let target = RuntimeBackend::task_create(
            &mut backend,
            &ContainerId::for_backend(BackendName::Herdr, "named:w").expect("container"),
            &TaskSpec {
                label: "mx-task".to_owned(),
                working_directory: temp.path().to_owned(),
            },
        )
        .expect("replace husk");
        assert_eq!(target.endpoint(), "named:w:new-pane");
        assert_eq!(backend.runner.tab_lists, 3);
        assert_eq!(backend.runner.pane_lists, 2);
    }

    #[test]
    fn composer_pi_and_generic_shapes_fail_closed_by_agent_authority() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pi_capture = "history\n────────\nhello\n────────\n".as_bytes();
        let mut pi = HerdrBackend::new(
            SequenceRunner::new([success(
                br#"{"result":{"agent":{"agent":"pi","agent_status":"idle"}}}"#,
            )]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert_eq!(
            pi.composer_from_capture("named", "w:p", pi_capture),
            ComposerState::Pending
        );
        let mut denied = HerdrBackend::new(
            SequenceRunner::new([success(
                br#"{"result":{"agent":{"agent":"codex","agent_status":"idle"}}}"#,
            )]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert_eq!(
            denied.composer_from_capture("named", "w:p", pi_capture),
            ComposerState::Unknown
        );
        let mut generic = HerdrBackend::new(NeverRunner, "herdr", "named", temp.path().to_owned());
        assert_eq!(
            generic.composer_from_capture("named", "w:p", "┃ Type a message... ┃\n".as_bytes()),
            ComposerState::Empty
        );
        assert_eq!(
            generic.composer_from_capture("named", "w:p", b"no composer here\n"),
            ComposerState::Unknown
        );
    }

    #[test]
    fn version_and_facade_state_failures_are_typed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut unavailable = HerdrBackend::new(
            SequenceRunner::new([failure(b"missing")]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert!(unavailable.tool_check().is_err());
        for status in [
            br#"{"client":{"version":"0.1"}}"#.as_slice(),
            br#"{"client":{"version":"0.1","protocol":13}}"#.as_slice(),
        ] {
            let mut backend = HerdrBackend::new(
                SequenceRunner::new([success(b"herdr 0.1\n"), success(status)]),
                "herdr",
                "named",
                temp.path().to_owned(),
            );
            assert!(backend.version_check().is_err());
        }
        for (outputs, expected) in [
            (
                vec![success(br#"{"error":{"code":"pane_not_found"}}"#)],
                AgentState::Missing,
            ),
            (
                vec![
                    success(br#"{"result":{"pane":{"pane_id":"w:p"}}}"#),
                    success(br#"{"error":{"code":"agent_not_found"}}"#),
                ],
                AgentState::Dead,
            ),
        ] {
            let mut backend = HerdrBackend::new(
                SequenceRunner::new(outputs),
                "herdr",
                "named",
                temp.path().to_owned(),
            );
            assert_eq!(backend.agent_state(&herdr_target("named:w:p")), expected);
        }
    }

    #[test]
    fn server_start_poll_and_submit_classifier_edges_are_observable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut server = HerdrBackend::new(
            SequenceRunner::new([
                success(br#"{"server":{"running":false}}"#),
                success(br#"{"server":{"running":true}}"#),
            ]),
            "/usr/bin/true",
            "named",
            temp.path().to_owned(),
        );
        server.server_ensure("named").expect("start and poll");
        for (status, expected) in [
            ("working", SubmitState::Busy),
            ("idle", SubmitState::Idle),
            ("mystery", SubmitState::Unknown),
        ] {
            let response = format!(r#"{{"result":{{"agent":{{"agent_status":"{status}"}}}}}}"#);
            let mut backend = HerdrBackend::new(
                SequenceRunner::new([success(response)]),
                "herdr",
                "named",
                temp.path().to_owned(),
            );
            assert_eq!(
                backend.wait_for_working("named", "w:p", Duration::ZERO, 1),
                expected
            );
        }
    }

    #[test]
    fn empty_inventory_trait_forwarding_and_kill_outcomes_are_typed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut empty = HerdrBackend::new(
            SequenceRunner::new([success(br#"{"result":{"workspaces":[]}}"#)]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert!(
            RuntimeBackend::list_live(&mut empty, None)
                .expect("empty inventory")
                .is_empty()
        );
        let container =
            ContainerId::for_backend(BackendName::Herdr, "named:w1").expect("container");
        let mut live = HerdrBackend::new(
            SmartRunner::default(),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert_eq!(
            LiveInventory::list_live(&mut live, Some(&container))
                .expect("live")
                .len(),
            1
        );
        for (pane_response, expected) in [
            (
                br#"{"error":{"code":"pane_not_found"}}"#.as_slice(),
                KillOutcome::Gone,
            ),
            (b"not json".as_slice(), KillOutcome::Unknown),
        ] {
            let mut backend = HerdrBackend::new(
                SequenceRunner::new([success(Vec::new()), success(pane_response)]),
                "herdr",
                "named",
                temp.path().to_owned(),
            );
            assert_eq!(backend.kill_verified(&herdr_target("named:w:p")), expected);
        }
    }

    #[test]
    fn seeded_tab_pruning_refuses_every_ambiguous_shape() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cases = [
            vec![success(b"not json")],
            vec![success(br#"{"result":{}}"#)],
            vec![success(br#"{"result":{"tabs":[{"tab_id":"w:seed","label":"1"}]}}"#)],
            vec![success(br#"{"result":{"tabs":[{"tab_id":"w:seed","label":"named"},{"tab_id":"w:new","label":"mx-task"}]}}"#)],
            vec![
                success(br#"{"result":{"tabs":[{"tab_id":"w:seed","label":"1"},{"tab_id":"w:new","label":"mx-task"}]}}"#),
                success(br#"{"result":{"panes":[]}}"#),
            ],
            vec![
                success(br#"{"result":{"tabs":[{"tab_id":"w:seed","label":"1"},{"tab_id":"w:new","label":"mx-task"}]}}"#),
                success(br#"{"result":{"panes":[{"tab_id":"w:seed","pane_id":"w:p"}]}}"#),
                success(br#"{"result":{"agent":{"agent_status":"working"}}}"#),
            ],
        ];
        for outputs in cases {
            let mut backend = HerdrBackend::new(
                SequenceRunner::new(outputs),
                "herdr",
                "named",
                temp.path().to_owned(),
            );
            backend.prune_seeded_tab("named", "w", "w:seed");
        }
    }

    #[test]
    fn bounded_helpers_and_capability_checks_cover_refusal_edges() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut failed = HerdrBackend::new(
            SequenceRunner::new([failure(b"failed")]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert!(failed.success_scoped("named", ["status"]).is_err());
        let mut stderr_json = HerdrBackend::new(
            SequenceRunner::new([failure(br#"{"error":{"code":"pane_not_found"}}"#)]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert!(
            stderr_json
                .json_any_status("named", ["pane", "get", "w:p"])
                .is_ok()
        );
        for outputs in [
            vec![success(br#"{"client":{"protocol":15}}"#)],
            vec![
                success(br#"{"client":{"protocol":16}}"#),
                success(br#"{"methods":[],"events":[]}"#),
            ],
            vec![failure(b"status failed")],
        ] {
            let mut backend = HerdrBackend::new(
                SequenceRunner::new(outputs),
                "herdr",
                "named",
                temp.path().to_owned(),
            );
            assert!(!backend.events_capable("named"));
        }
        assert_eq!(parse_target(":pane"), None);
        assert_eq!(parse_target("session:"), None);
        assert!(tail_lines(b"some bytes", 0).is_empty());
        let rows = [
            "────────".as_bytes(),
            b"one",
            b"two",
            b"three",
            "────────".as_bytes(),
        ];
        assert_eq!(bottom_pi_pair(&rows, 2), None);
        for status in ["idle", "unrecognized"] {
            let record = TransitionRecord::new("w:p", "w", "", status, "");
            assert!(!apply_transition(temp.path(), "named", &record).expect("transition"));
        }
    }

    #[test]
    fn submit_inventory_and_creation_refusals_keep_typed_outcomes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = herdr_target("named:w:p");
        for (capture, expected) in [
            ("┃ Type a message... ┃\n", ComposerState::Empty),
            ("no composer\n", ComposerState::Unknown),
        ] {
            let mut backend = HerdrBackend::new(
                SequenceRunner::new([
                    success(br#"{"server":{"running":true}}"#),
                    success(Vec::new()),
                    success(br#"{"result":{"agent":{"agent_status":"mystery"}}}"#),
                    success(br#"{"server":{"running":true}}"#),
                    success(Vec::new()),
                    success(br#"{"server":{"running":true}}"#),
                    success(capture),
                ]),
                "herdr",
                "named",
                temp.path().to_owned(),
            );
            assert_eq!(
                backend
                    .send_submit(
                        &target,
                        SubmitRequest {
                            text: "submit",
                            retries: 1,
                            enter_delay: Duration::ZERO,
                            settle: Duration::ZERO,
                        },
                    )
                    .expect("submit outcome"),
                expected
            );
        }
        let mut inventory = HerdrBackend::new(
            SequenceRunner::new([
                success(br#"{"result":{"tabs":[{"tab_id":"w:other","label":"other"},{"label":"mx-missing"},{"tab_id":"w:live","label":"mx-live"}]}}"#),
                success(br#"{"result":{"panes":[{"tab_id":"w:live","pane_id":"w:p"}]}}"#),
            ]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        let container = ContainerId::for_backend(BackendName::Herdr, "named:w").expect("container");
        assert_eq!(
            RuntimeBackend::list_live(&mut inventory, Some(&container))
                .expect("inventory")
                .len(),
            1
        );
        let mut malformed = HerdrBackend::new(
            SequenceRunner::new([success(br#"{"result":{}}"#)]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert!(
            malformed
                .create_task_full(
                    &container,
                    &TaskSpec {
                        label: "mx-task".to_owned(),
                        working_directory: temp.path().to_owned(),
                    },
                    None,
                )
                .is_err()
        );
        let mut duplicate_without_pane = HerdrBackend::new(
            SequenceRunner::new([
                success(br#"{"result":{"tabs":[{"tab_id":"w:dup","label":"mx-task"}]}}"#),
                success(br#"{"result":{"panes":[]}}"#),
            ]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert!(
            duplicate_without_pane
                .create_task_full(
                    &container,
                    &TaskSpec {
                        label: "mx-task".to_owned(),
                        working_directory: temp.path().to_owned(),
                    },
                    None,
                )
                .is_err()
        );
        let mut unknown = HerdrBackend::new(
            SequenceRunner::new([success(b"not json")]),
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        assert_eq!(unknown.agent_state(&target), AgentState::Unreadable);
    }

    #[derive(Debug)]
    struct EventRunner {
        socket: PathBuf,
    }

    impl CommandRunner for EventRunner {
        fn run(&mut self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
            let args = request
                .args
                .iter()
                .map(|argument| argument.to_string_lossy())
                .collect::<Vec<_>>();
            let output = match args.first().map(|argument| argument.as_ref()) {
                Some("status") => {
                    br#"{"client":{"protocol":16},"server":{"running":true}}"#.to_vec()
                }
                Some("api") => {
                    br#"{"methods":["events.subscribe"],"events":["pane.agent_status_changed"]}"#
                        .to_vec()
                }
                Some("session") => format!(
                    r#"{{"sessions":[{{"name":"named","running":true,"socket_path":"{}"}}]}}"#,
                    self.socket.display()
                )
                .into_bytes(),
                Some("agent") => br#"{"result":{"agent":{"agent_status":"working"}}}"#.to_vec(),
                _ => panic!("unexpected event command: {args:?}"),
            };
            Ok(success(output))
        }
    }

    #[test]
    fn native_event_wait_reconciles_then_returns_actionable_transition() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("herdr.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = String::new();
            BufReader::new(stream.try_clone().expect("clone"))
                .read_line(&mut request)
                .expect("request");
            assert!(request.contains("\"method\":\"events.subscribe\""));
            stream
                .write_all(
                    b"{\"id\":\"mx-eventwait\",\"result\":{\"type\":\"subscription_started\"}}\n",
                )
                .expect("ack");
            stream
                .write_all(b"{\"event\":\"pane.agent_status_changed\",\"data\":{\"pane_id\":\"w:p\",\"workspace_id\":\"w\",\"agent_status\":\"blocked\",\"agent\":\"claude\"}}\n")
                .expect("event");
        });
        let mut backend = HerdrBackend::new(
            EventRunner {
                socket: socket.clone(),
            },
            "herdr",
            "named",
            temp.path().to_owned(),
        );
        let transition = backend
            .wait_transition_in_state(
                "named",
                Duration::from_secs(1),
                temp.path(),
                &["named:w:p".to_owned()],
            )
            .expect("wait")
            .expect("actionable transition");
        assert_eq!(transition.pane_id, "w:p");
        assert_eq!(transition.to_status, "blocked");
        server.join().expect("server");
    }
}

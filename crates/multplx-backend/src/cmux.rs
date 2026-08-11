//! cmux runtime backend with fresh socket authentication and scoped identity.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use multplx_core::backend_hometag::home_tag;
use multplx_core::composer::{ComposerState, classify_content};
use serde_json::Value;

use crate::command::{CommandOutput, CommandRequest, CommandRunner, SystemCommandRunner};
use crate::facade::{
    AgentState, BackendError, BackendName, BackendTarget, Capability, CaptureRequest, ContainerId,
    KillOutcome, LiveInventory, LiveTarget, NativeState, RuntimeBackend, SubmitRequest, TaskSpec,
};

const MIN_MAJOR: u64 = 0;
const MIN_MINOR: u64 = 64;
const OUTPUT_LIMIT: usize = 256 * 1024;
const DEFAULT_BUNDLE_BIN: &str = "/Applications/cmux.app/Contents/Resources/bin/cmux";
const CWD_MARKER_BEGIN: &str = "__MX_CMUX_CWD_BEGIN__";
const CWD_MARKER_END: &str = "__MX_CMUX_CWD_END__";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PingState {
    Ok,
    Denied,
    Unauthenticated,
    Down,
    Error,
}

impl PingState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Denied => "denied",
            Self::Unauthenticated => "unauth",
            Self::Down => "down",
            Self::Error => "error",
        }
    }
}

#[derive(Debug)]
pub struct CmuxBackend<R = SystemCommandRunner> {
    runner: R,
    executable: OsString,
    root: PathBuf,
    home: PathBuf,
    config: PathBuf,
    idle_pattern: String,
    composer_lines: u32,
    system_tools: bool,
    executable_available: bool,
}

impl CmuxBackend<SystemCommandRunner> {
    #[must_use]
    pub fn system() -> Self {
        let root = std::env::var_os("MX_ROOT_OVERRIDE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let home = std::env::var_os("MX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone());
        let config = std::env::var_os("MX_CONFIG_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("config"));
        let resolved = std::env::var_os("MX_BACKEND_CMUX_BIN")
            .filter(|value| is_executable(Path::new(value)))
            .or_else(resolve_system_executable);
        let executable_available = resolved.is_some();
        let executable = resolved.unwrap_or_else(|| OsString::from("cmux"));
        let mut backend = Self::new(SystemCommandRunner, executable, root, home, config);
        backend.system_tools = true;
        backend.executable_available = executable_available;
        backend
    }
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn resolve_path_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable(candidate))
}

fn resolve_system_executable() -> Option<OsString> {
    if let Some(candidate) = resolve_path_executable("cmux") {
        return Some(candidate.into_os_string());
    }
    let bundle = std::env::var_os("MX_BACKEND_CMUX_BUNDLE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_BUNDLE_BIN));
    is_executable(&bundle).then(|| bundle.into_os_string())
}

impl<R: CommandRunner> CmuxBackend<R> {
    #[must_use]
    pub fn new(
        runner: R,
        executable: impl Into<OsString>,
        root: impl Into<PathBuf>,
        home: impl Into<PathBuf>,
        config: impl Into<PathBuf>,
    ) -> Self {
        Self {
            runner,
            executable: executable.into(),
            root: root.into(),
            home: home.into(),
            config: config.into(),
            idle_pattern: std::env::var("MX_BACKEND_CMUX_IDLE_RE")
                .unwrap_or_else(|_| r"^Type a message\.\.\.$".to_owned()),
            composer_lines: std::env::var("MX_BACKEND_CMUX_COMPOSER_LINES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(20),
            system_tools: false,
            executable_available: true,
        }
    }

    #[must_use]
    pub fn executable(&self) -> &OsStr {
        &self.executable
    }

    #[must_use]
    pub fn executable_available(&self) -> bool {
        self.executable_available
    }

    pub fn socket_password(&self) -> Option<String> {
        let text = fs::read_to_string(self.config.join("cmux-socket-password")).ok()?;
        text.lines()
            .find(|line| !line.is_empty())
            .map(str::to_owned)
    }

    fn request(&self, args: impl IntoIterator<Item = impl Into<OsString>>) -> CommandRequest {
        let mut request = CommandRequest::new(self.executable.clone(), args);
        request.output_limit = OUTPUT_LIMIT;
        request
            .env
            .push((OsString::from("CMUX_QUIET"), OsString::from("1")));
        if let Some(password) = self.socket_password() {
            request.env.push((
                OsString::from("CMUX_SOCKET_PASSWORD"),
                OsString::from(password),
            ));
        }
        request
    }

    pub fn cli(
        &mut self,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<CommandOutput, BackendError> {
        self.runner
            .run(&self.request(args))
            .map_err(|error| BackendError::Command(error.to_string()))
    }

    fn success(
        &mut self,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<CommandOutput, BackendError> {
        let output = self.cli(args)?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(BackendError::Command(format!(
                "cmux exited {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    fn json(
        &mut self,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Value, BackendError> {
        let output = self.success(args)?;
        serde_json::from_slice(&output.stdout)
            .map_err(|error| BackendError::Malformed(format!("cmux JSON: {error}")))
    }

    fn ensure_target(&self, target: &BackendTarget) -> Result<(String, String), BackendError> {
        if target.backend() != BackendName::Cmux {
            return Err(BackendError::InvalidTarget(target.endpoint().to_owned()));
        }
        parse_target(target.endpoint())
    }

    pub fn home_label(&self) -> Result<String, BackendError> {
        home_tag(&self.root, &self.home).map_err(|error| BackendError::Metadata(error.to_string()))
    }

    pub fn scoped_title(&self, label: &str) -> Result<String, BackendError> {
        let rest = label.strip_prefix("mx-").unwrap_or(label);
        Ok(format!("mx-{}-{rest}", self.home_label()?))
    }

    pub fn ping_state(&mut self) -> PingState {
        let Ok(output) = self.cli(["ping"]) else {
            return PingState::Error;
        };
        let mut bytes = output.stdout;
        bytes.extend_from_slice(&output.stderr);
        let text = String::from_utf8_lossy(&bytes);
        if text.trim() == "PONG" {
            PingState::Ok
        } else if text.contains("only processes started inside cmux can connect") {
            PingState::Denied
        } else if text.contains("Password mode is enabled but no socket password")
            || text.contains("Authentication required")
            || text.contains("Invalid password")
        {
            PingState::Unauthenticated
        } else if text.contains("Socket not found") {
            PingState::Down
        } else {
            PingState::Error
        }
    }

    pub fn ensure_running(&mut self) -> Result<(), BackendError> {
        match self.ping_state() {
            PingState::Ok => return Ok(()),
            PingState::Denied => return Err(BackendError::Command(denied_message().to_owned())),
            PingState::Unauthenticated => {
                return Err(BackendError::Command(unauthenticated_message().to_owned()));
            }
            PingState::Down | PingState::Error => {}
        }
        let mut request = CommandRequest::new("open", ["-a", "cmux"]);
        request.output_limit = OUTPUT_LIMIT;
        let output = self
            .runner
            .run(&request)
            .map_err(|error| BackendError::Command(error.to_string()))?;
        if !output.status.success() {
            return Err(BackendError::Command(
                "failed to launch cmux ('open -a cmux' failed)".to_owned(),
            ));
        }
        for _ in 0..20 {
            match self.ping_state() {
                PingState::Ok => return Ok(()),
                PingState::Denied => {
                    return Err(BackendError::Command(denied_message().to_owned()));
                }
                PingState::Unauthenticated => {
                    return Err(BackendError::Command(unauthenticated_message().to_owned()));
                }
                PingState::Down | PingState::Error => thread::sleep(Duration::from_millis(500)),
            }
        }
        Err(BackendError::Command("cmux did not become reachable within 10s of launch. If the app is already running, its Socket Control Mode may be 'Off' (no control socket at all) - set it to 'Automation mode' (recommended) in Settings > Automation, see docs/cmux-backend.md 'Setup'.".to_owned()))
    }

    pub fn workspace_id_for_label(&mut self, label: &str) -> Result<Option<String>, BackendError> {
        let value = self.json(["workspace", "list", "--json", "--id-format", "uuids"])?;
        Ok(value
            .get("workspaces")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|workspace| workspace.get("title").and_then(Value::as_str) == Some(label))
            .and_then(|workspace| workspace.get("id").and_then(Value::as_str))
            .map(str::to_owned))
    }

    pub fn surface_id_for_workspace(
        &mut self,
        workspace: &str,
    ) -> Result<Option<String>, BackendError> {
        let value = self.json([
            "list-panes",
            "--workspace",
            workspace,
            "--json",
            "--id-format",
            "uuids",
        ])?;
        let pane = value
            .get("panes")
            .and_then(Value::as_array)
            .and_then(|panes| panes.first());
        Ok(pane
            .and_then(|pane| pane.get("selected_surface_id").and_then(Value::as_str))
            .or_else(|| {
                pane.and_then(|pane| pane.get("surface_ids"))
                    .and_then(Value::as_array)
                    .and_then(|surfaces| surfaces.first())
                    .and_then(Value::as_str)
            })
            .map(str::to_owned))
    }

    pub fn surface_exists(&mut self, workspace: &str, surface: &str) -> Result<bool, BackendError> {
        let value = self.json([
            "list-panes",
            "--workspace",
            workspace,
            "--json",
            "--id-format",
            "uuids",
        ])?;
        Ok(value
            .get("panes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|pane| {
                pane.get("surface_ids")
                    .and_then(Value::as_array)
                    .is_some_and(|surfaces| {
                        surfaces.iter().any(|value| value.as_str() == Some(surface))
                    })
            }))
    }

    fn workspace_title(&mut self, workspace: &str) -> Result<Option<String>, BackendError> {
        let value = self.json(["workspace", "list", "--json", "--id-format", "uuids"])?;
        Ok(value
            .get("workspaces")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(workspace))
            .and_then(|item| item.get("title").and_then(Value::as_str))
            .map(str::to_owned))
    }

    fn refreshed_target(
        &mut self,
        target: &BackendTarget,
    ) -> Result<(String, String), BackendError> {
        let (workspace, surface) = self.ensure_target(target)?;
        let Some(label) = target.expected_label() else {
            return self
                .surface_exists(&workspace, &surface)?
                .then_some((workspace, surface))
                .ok_or_else(|| BackendError::Command("cmux target is absent".to_owned()));
        };
        let expected = self.scoped_title(label)?;
        match self.workspace_title(&workspace)? {
            Some(title) if title == expected => {
                if self.surface_exists(&workspace, &surface)? {
                    return Ok((workspace, surface));
                }
                let refreshed = self
                    .surface_id_for_workspace(&workspace)?
                    .ok_or_else(|| BackendError::Command("cmux surface is absent".to_owned()))?;
                Ok((workspace, refreshed))
            }
            Some(_) => Err(BackendError::Command(
                "cmux target label does not match".to_owned(),
            )),
            None => {
                let workspace = self
                    .workspace_id_for_label(&expected)?
                    .ok_or_else(|| BackendError::Command("cmux workspace is absent".to_owned()))?;
                let surface = self
                    .surface_id_for_workspace(&workspace)?
                    .ok_or_else(|| BackendError::Command("cmux surface is absent".to_owned()))?;
                Ok((workspace, surface))
            }
        }
    }

    pub fn window_of_workspace(
        &mut self,
        workspace: &str,
    ) -> Result<Option<(String, usize)>, BackendError> {
        let windows = match self.json(["list-windows", "--json", "--id-format", "uuids"]) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        for window in windows.as_array().into_iter().flatten() {
            let Some(id) = window.get("id").and_then(Value::as_str) else {
                continue;
            };
            let listing = match self.json([
                "workspace",
                "list",
                "--json",
                "--id-format",
                "uuids",
                "--window",
                id,
            ]) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let workspaces = listing
                .get("workspaces")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if workspaces
                .iter()
                .any(|item| item.get("id").and_then(Value::as_str) == Some(workspace))
            {
                return Ok(Some((id.to_owned(), workspaces.len())));
            }
        }
        Ok(None)
    }

    /// Preserve the public shell adapter's best-effort teardown contract.
    pub fn kill_best_effort(&mut self, target: &BackendTarget) {
        let resolved = if target.expected_label().is_some() {
            self.refreshed_target(target)
        } else {
            self.ensure_target(target)
        };
        let Ok((workspace, _)) = resolved else {
            return;
        };
        if let Ok(Some((window, count))) = self.window_of_workspace(&workspace)
            && count == 1
        {
            let _ = self.cli([
                "new-workspace",
                "--window",
                &window,
                "--focus",
                "false",
                "--id-format",
                "uuids",
            ]);
        }
        let _ = self.cli(["close-workspace", "--workspace", &workspace]);
    }
}

fn denied_message() -> &'static str {
    "backend=cmux socket rejected the connection (automation.socketControlMode is cmuxOnly, the default, which never admits an external CLI like broker). In cmux Settings > Automation set Socket Control Mode to 'Automation mode' (recommended - same-user external clients, no password), or 'Password mode' plus config/cmux-socket-password/CMUX_SOCKET_PASSWORD, or 'Full open access' (NOT recommended - admits every local user) - see docs/cmux-backend.md 'Setup' - or set config/backend to tmux (or pass --backend tmux) if you did not mean to use cmux."
}

fn unauthenticated_message() -> &'static str {
    "backend=cmux socket requires a password (automation.socketControlMode=password) but none is configured for this caller, or the configured one was rejected. Set config/cmux-socket-password or export CMUX_SOCKET_PASSWORD to the password from cmux Settings > Automation, or switch Socket Control Mode to 'Automation mode' (recommended - no password needed) - see docs/cmux-backend.md 'Setup' - or set config/backend to tmux (or pass --backend tmux) if you did not mean to use cmux."
}

pub fn parse_target(target: &str) -> Result<(String, String), BackendError> {
    let (workspace, surface) = target
        .split_once(':')
        .ok_or_else(|| BackendError::InvalidTarget(target.to_owned()))?;
    if workspace.is_empty() || surface.is_empty() {
        return Err(BackendError::InvalidTarget(target.to_owned()));
    }
    Ok((workspace.to_owned(), surface.to_owned()))
}

#[must_use]
pub fn normalize_key(key: &str) -> &str {
    match key {
        "Enter" | "enter" => "enter",
        "Escape" | "escape" | "Esc" | "esc" => "escape",
        "C-c" | "c-c" | "ctrl+c" | "Ctrl+c" | "Ctrl+C" | "ctrl-c" => "ctrl-c",
        value => value,
    }
}

fn tail_lines(text: &str, lines: usize) -> String {
    let rows = text.lines().collect::<Vec<_>>();
    rows[rows.len().saturating_sub(lines)..].join("\n")
}

impl<R: CommandRunner> RuntimeBackend for CmuxBackend<R> {
    fn name(&self) -> BackendName {
        BackendName::Cmux
    }

    fn supports(&self, capability: Capability) -> bool {
        capability == Capability::ComposerState
    }

    fn tool_check(&mut self) -> Result<(), BackendError> {
        if self.system_tools && !self.executable_available {
            let bundle = std::env::var_os("MX_BACKEND_CMUX_BUNDLE_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_BUNDLE_BIN));
            return Err(BackendError::Command(format!(
                "backend=cmux selected but the 'cmux' CLI was not found on PATH or at {} (https://cmux.com)",
                bundle.display()
            )));
        }
        if self.system_tools && resolve_path_executable("jq").is_none() {
            return Err(BackendError::Command(
                "backend=cmux selected but 'jq' is not installed (required to parse cmux's JSON output)"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn version_check(&mut self) -> Result<String, BackendError> {
        self.tool_check()?;
        let output = self.success(["version"])?;
        let raw = String::from_utf8(output.stdout)
            .map_err(|_| BackendError::Malformed("cmux version is not UTF-8".to_owned()))?;
        let version = raw.split_whitespace().nth(1).ok_or_else(|| {
            BackendError::Malformed(format!(
                "could not parse a cmux version from '{}'",
                raw.trim()
            ))
        })?;
        if !version
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
        {
            return Err(BackendError::Malformed(format!(
                "could not parse a cmux version from '{}'",
                raw.trim()
            )));
        }
        let mut parts = version.split('.');
        let major = parts
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let minor = parts
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        if major == MIN_MAJOR && minor < MIN_MINOR {
            return Err(BackendError::Command(format!(
                "cmux {version} is older than the verified minimum {MIN_MAJOR}.{MIN_MINOR}; update cmux before using backend=cmux"
            )));
        }
        Ok(version.to_owned())
    }

    fn container_ensure(&mut self) -> Result<ContainerId, BackendError> {
        self.version_check()?;
        self.ensure_running()?;
        ContainerId::for_backend(BackendName::Cmux, "cmux")
    }

    fn task_create(
        &mut self,
        _: &ContainerId,
        task: &TaskSpec,
    ) -> Result<BackendTarget, BackendError> {
        let title = self.scoped_title(&task.label)?;
        if self.workspace_id_for_label(&title)?.is_some() {
            return Err(BackendError::Command(format!(
                "cmux workspace '{title}' already exists"
            )));
        }
        let cwd = task.working_directory.as_os_str().to_owned();
        let output = self.cli([
            OsString::from("new-workspace"),
            OsString::from("--name"),
            OsString::from(&title),
            OsString::from("--cwd"),
            cwd,
            OsString::from("--focus"),
            OsString::from("false"),
            OsString::from("--id-format"),
            OsString::from("uuids"),
        ])?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(BackendError::Command(format!(
                "cmux new-workspace failed for '{title}': {}",
                detail.trim()
            )));
        }
        let workspace = self.workspace_id_for_label(&title)?.ok_or_else(|| {
            BackendError::Malformed(format!(
                "could not resolve a cmux workspace id for '{title}' after creation"
            ))
        })?;
        let surface = self.surface_id_for_workspace(&workspace)?.ok_or_else(|| {
            BackendError::Malformed(format!(
                "could not resolve the default surface for cmux workspace '{title}' ({workspace})"
            ))
        })?;
        BackendTarget::new(
            BackendName::Cmux,
            format!("{workspace}:{surface}"),
            Some(task.label.clone()),
        )
    }

    fn target_ready(&mut self, target: &BackendTarget) -> Result<(), BackendError> {
        self.refreshed_target(target).map(|_| ())
    }

    fn current_path(&mut self, target: &BackendTarget) -> Result<PathBuf, BackendError> {
        self.target_ready(target)?;
        let command =
            format!("printf '%s\\n' '{CWD_MARKER_BEGIN}'; pwd; printf '%s\\n' '{CWD_MARKER_END}'");
        self.send_text_line(target, &command)?;
        thread::sleep(Duration::from_millis(300));
        let capture = String::from_utf8(self.capture(&CaptureRequest {
            target: target.clone(),
            lines: 200,
            byte_limit: OUTPUT_LIMIT,
        })?)
        .map_err(|_| BackendError::Malformed("cmux capture is not UTF-8".to_owned()))?;
        let mut inside = false;
        let mut chunk = String::new();
        let mut last = None;
        for line in capture.lines() {
            if line == CWD_MARKER_BEGIN {
                inside = true;
                chunk.clear();
                continue;
            }
            if line == CWD_MARKER_END {
                if chunk.starts_with('/') {
                    last = Some(chunk.clone());
                }
                inside = false;
                continue;
            }
            if inside {
                chunk.push_str(line);
            }
        }
        last.map(PathBuf::from).ok_or_else(|| {
            BackendError::Malformed("cmux current-path marker was absent".to_owned())
        })
    }

    fn capture(&mut self, request: &CaptureRequest) -> Result<Vec<u8>, BackendError> {
        let (workspace, surface) = self.refreshed_target(&request.target)?;
        let lines = request.lines.max(1);
        let fetch = lines.max(200).to_string();
        let output = self.success([
            "read-screen",
            "--workspace",
            &workspace,
            "--surface",
            &surface,
            "--scrollback",
            "--lines",
            &fetch,
            "--json",
        ])?;
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| BackendError::Malformed(format!("cmux JSON: {error}")))?;
        let text = value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let capture = tail_lines(text, lines as usize).into_bytes();
        if capture.len() > request.byte_limit {
            return Err(BackendError::Command(format!(
                "cmux capture exceeded the {}-byte limit",
                request.byte_limit
            )));
        }
        Ok(capture)
    }

    fn composer_state(&mut self, target: &BackendTarget) -> Result<ComposerState, BackendError> {
        let capture = String::from_utf8(self.capture(&CaptureRequest {
            target: target.clone(),
            lines: self.composer_lines,
            byte_limit: OUTPUT_LIMIT,
        })?)
        .map_err(|_| BackendError::Malformed("cmux capture is not UTF-8".to_owned()))?;
        let mut selected = None;
        for line in capture.lines() {
            let trimmed = line.trim();
            let bordered = [('│', '│'), ('┃', '┃'), ('|', '|')]
                .iter()
                .any(|(start, end)| trimmed.starts_with(*start) && trimmed.ends_with(*end));
            if bordered {
                selected = Some(trimmed.to_owned());
            }
        }
        let Some(mut content) = selected else {
            return Ok(ComposerState::Unknown);
        };
        content.retain(|character| !matches!(character, '│' | '┃' | '|'));
        classify_content(true, content.trim(), Some(&self.idle_pattern), false, None)
            .map_err(|error| BackendError::Malformed(error.to_string()))
    }

    fn send_literal(&mut self, target: &BackendTarget, text: &str) -> Result<(), BackendError> {
        let (workspace, surface) = self.refreshed_target(target)?;
        self.success([
            "send",
            "--workspace",
            &workspace,
            "--surface",
            &surface,
            "--",
            text,
        ])
        .map(|_| ())
    }

    fn send_key(&mut self, target: &BackendTarget, key: &str) -> Result<(), BackendError> {
        let (workspace, surface) = self.refreshed_target(target)?;
        self.success([
            "send-key",
            "--workspace",
            &workspace,
            "--surface",
            &surface,
            normalize_key(key),
        ])
        .map(|_| ())
    }

    fn send_submit(
        &mut self,
        target: &BackendTarget,
        request: SubmitRequest<'_>,
    ) -> Result<ComposerState, BackendError> {
        self.send_literal(target, request.text)?;
        thread::sleep(request.settle);
        for _ in 0..request.retries.max(1) {
            let _ = self.send_key(target, "Enter");
            thread::sleep(request.enter_delay);
            let state = self
                .composer_state(target)
                .unwrap_or(ComposerState::Unknown);
            if state != ComposerState::Pending {
                return Ok(state);
            }
        }
        Ok(ComposerState::Pending)
    }

    fn send_text_line(&mut self, target: &BackendTarget, text: &str) -> Result<(), BackendError> {
        self.send_literal(target, text)?;
        self.send_key(target, "Enter")
    }

    fn native_state(&mut self, _: &BackendTarget) -> Result<NativeState, BackendError> {
        Err(BackendError::Unsupported {
            backend: BackendName::Cmux,
            capability: "native-state",
        })
    }

    fn agent_state(&mut self, _: &BackendTarget) -> AgentState {
        AgentState::Unverified
    }

    fn kill_verified(&mut self, target: &BackendTarget) -> KillOutcome {
        let Ok((workspace, _)) = self.ensure_target(target) else {
            return KillOutcome::Gone;
        };
        self.kill_best_effort(target);
        match self.workspace_title(&workspace) {
            Ok(None) => KillOutcome::Gone,
            Ok(Some(_)) => KillOutcome::StillPresent,
            Err(_) => KillOutcome::Unknown,
        }
    }

    fn list_live(&mut self, _: Option<&ContainerId>) -> Result<Vec<LiveTarget>, BackendError> {
        let prefix = format!("mx-{}-", self.home_label()?);
        let value = match self.json(["workspace", "list", "--json", "--id-format", "uuids"]) {
            Ok(value) => value,
            Err(_) => return Ok(Vec::new()),
        };
        let mut result = Vec::new();
        for workspace in value
            .get("workspaces")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = workspace.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(title) = workspace.get("title").and_then(Value::as_str) else {
                continue;
            };
            let Some(plain) = title
                .strip_prefix(&prefix)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Ok(Some(surface)) = self.surface_id_for_workspace(id) else {
                continue;
            };
            result.push(LiveTarget {
                target: BackendTarget::new(
                    BackendName::Cmux,
                    format!("{id}:{surface}"),
                    Some(format!("mx-{plain}")),
                )?,
                label: format!("mx-{plain}"),
            });
        }
        Ok(result)
    }

    fn wait_transition(
        &mut self,
        _: &ContainerId,
        _: &[BackendTarget],
        _: Duration,
    ) -> Result<Option<String>, BackendError> {
        Err(BackendError::Unsupported {
            backend: BackendName::Cmux,
            capability: "transition-events",
        })
    }
}

impl<R: CommandRunner> LiveInventory for CmuxBackend<R> {
    fn list_live(
        &mut self,
        container: Option<&ContainerId>,
    ) -> Result<Vec<LiveTarget>, BackendError> {
        RuntimeBackend::list_live(self, container)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::time::Duration;

    use crate::command::{CommandError, CommandOutput, CommandRequest, CommandRunner};
    use crate::facade::{
        AgentState, BackendName, BackendTarget, Capability, CaptureRequest, ContainerId,
        KillOutcome, RuntimeBackend, SubmitRequest, TaskSpec,
    };

    use super::{CmuxBackend, PingState, normalize_key, parse_target};

    #[derive(Debug, Default)]
    struct FakeRunner {
        calls: Vec<CommandRequest>,
        outputs: VecDeque<Result<CommandOutput, CommandError>>,
    }

    impl CommandRunner for FakeRunner {
        fn run(&mut self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
            self.calls.push(request.clone());
            self.outputs.pop_front().unwrap_or_else(|| ok(b""))
        }
    }

    fn ok(stdout: &'static [u8]) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: ExitStatus::from_raw(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        })
    }

    fn failed(stderr: &'static [u8]) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: stderr.to_vec(),
        })
    }

    fn backend(outputs: Vec<Result<CommandOutput, CommandError>>) -> CmuxBackend<FakeRunner> {
        CmuxBackend::new(
            FakeRunner {
                calls: Vec::new(),
                outputs: outputs.into(),
            },
            "cmux",
            "/tmp/root",
            "/tmp/home",
            "/tmp/config",
        )
    }

    #[test]
    fn target_and_key_vocabularies_are_exact() {
        assert_eq!(
            parse_target("workspace:surface").expect("target"),
            ("workspace".to_owned(), "surface".to_owned())
        );
        assert!(parse_target("missing").is_err());
        assert_eq!(normalize_key("Escape"), "escape");
        assert_eq!(normalize_key("C-c"), "ctrl-c");
    }

    #[test]
    fn password_is_fresh_and_never_an_argument() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = temp.path().join("config");
        std::fs::create_dir(&config).expect("config");
        std::fs::write(config.join("cmux-socket-password"), "secret\n").expect("password");
        let mut backend = CmuxBackend::new(
            FakeRunner::default(),
            "cmux",
            temp.path(),
            temp.path(),
            &config,
        );
        backend.cli(["ping"]).expect("ping");
        let call = &backend.runner.calls[0];
        assert!(!call.args.iter().any(|value| value == "secret"));
        assert!(
            call.env
                .iter()
                .any(|(name, value)| name == "CMUX_SOCKET_PASSWORD" && value == "secret")
        );
        std::fs::write(config.join("cmux-socket-password"), "changed\n").expect("change");
        backend.cli(["ping"]).expect("ping");
        assert!(
            backend.runner.calls[1]
                .env
                .iter()
                .any(|(_, value)| value == "changed")
        );
    }

    #[test]
    fn ping_and_capture_parse_structured_responses() {
        let mut fixture = backend(vec![
            ok(b"PONG\n"),
            ok(br#"{"panes":[{"surface_ids":["s"]}]}"#),
            ok(br#"{"text":"one\ntwo\nthree"}"#),
        ]);
        assert_eq!(fixture.ping_state(), PingState::Ok);
        let target = BackendTarget::new(BackendName::Cmux, "w:s", None).expect("target");
        assert_eq!(
            fixture
                .capture(&CaptureRequest {
                    target,
                    lines: 2,
                    byte_limit: 1024
                })
                .expect("capture"),
            b"two\nthree"
        );
    }

    #[test]
    fn ping_version_and_startup_failures_are_typed() {
        for (text, expected) in [
            (
                "only processes started inside cmux can connect",
                PingState::Denied,
            ),
            ("Authentication required", PingState::Unauthenticated),
            ("Invalid password", PingState::Unauthenticated),
            ("Socket not found", PingState::Down),
            ("unexpected", PingState::Error),
        ] {
            let mut backend = CmuxBackend::new(
                FakeRunner {
                    calls: Vec::new(),
                    outputs: vec![Ok(CommandOutput {
                        status: ExitStatus::from_raw(0),
                        stdout: text.as_bytes().to_vec(),
                        stderr: Vec::new(),
                    })]
                    .into(),
                },
                "cmux",
                "/tmp/root",
                "/tmp/home",
                "/tmp/config",
            );
            assert_eq!(backend.ping_state(), expected);
            assert_eq!(expected.as_str(), expected.as_str());
        }

        let mut denied = backend(vec![ok(b"only processes started inside cmux can connect")]);
        assert!(
            denied
                .ensure_running()
                .unwrap_err()
                .to_string()
                .contains("cmuxOnly")
        );
        let mut unauth = backend(vec![ok(b"Password mode is enabled but no socket password")]);
        assert!(
            unauth
                .ensure_running()
                .unwrap_err()
                .to_string()
                .contains("requires a password")
        );
        let mut launch_failure = backend(vec![ok(b"Socket not found"), failed(b"no app")]);
        assert!(launch_failure.ensure_running().is_err());
        let mut launched = backend(vec![ok(b"Socket not found"), ok(b""), ok(b"PONG\n")]);
        launched.ensure_running().expect("started");

        for bytes in [&b"cmux\n"[..], &b"cmux bad\n"[..], &b"cmux 0.63.9\n"[..]] {
            let mut backend = backend(vec![ok(bytes)]);
            assert!(backend.version_check().is_err());
        }
        assert_eq!(
            backend(vec![ok(b"cmux 1.2.3\n")])
                .version_check()
                .expect("version"),
            "1.2.3"
        );
    }

    #[test]
    fn workspace_target_and_runtime_outcomes_cover_refresh_edges() {
        let mut fixture = backend(vec![
            ok(br#"{"workspaces":[{"id":"w","title":"wanted"}]}"#),
            ok(br#"{"panes":[{"surface_ids":["s"]}]}"#),
            ok(br#"{"panes":[{"selected_surface_id":"selected"}]}"#),
            ok(br#"[{"id":"win"}]"#),
            ok(br#"{"workspaces":[{"id":"w"},{"id":"other"}]}"#),
        ]);
        assert_eq!(
            fixture.workspace_id_for_label("wanted").unwrap().as_deref(),
            Some("w")
        );
        assert!(fixture.surface_exists("w", "s").unwrap());
        assert_eq!(
            fixture.surface_id_for_workspace("w").unwrap().as_deref(),
            Some("selected")
        );
        assert_eq!(
            fixture.window_of_workspace("w").unwrap(),
            Some(("win".to_owned(), 2))
        );

        let target = BackendTarget::new(BackendName::Cmux, "w:s", None).unwrap();
        let mut absent = backend(vec![ok(br#"{"panes":[]}"#)]);
        assert!(absent.target_ready(&target).is_err());
        let wrong = BackendTarget::new(BackendName::Tmux, "w:s", None).unwrap();
        assert!(backend(vec![]).target_ready(&wrong).is_err());

        let mut runtime = backend(vec![]);
        assert_eq!(runtime.name(), BackendName::Cmux);
        assert!(runtime.supports(Capability::ComposerState));
        assert!(!runtime.supports(Capability::NativeState));
        assert_eq!(runtime.agent_state(&target), AgentState::Unverified);
        assert!(runtime.native_state(&target).is_err());
        let container = ContainerId::for_backend(BackendName::Cmux, "cmux").unwrap();
        assert!(
            runtime
                .wait_transition(&container, &[], Duration::ZERO)
                .is_err()
        );
        assert_eq!(runtime.kill_verified(&target), KillOutcome::Unknown);
    }

    #[test]
    fn capture_composer_submit_and_inventory_cover_public_contracts() {
        let container = ContainerId::for_backend(BackendName::Cmux, "cmux").unwrap();
        let task = TaskSpec {
            label: "mx-task".to_owned(),
            working_directory: "/tmp".into(),
        };
        let mut duplicate = backend(vec![ok(
            br#"{"workspaces":[{"id":"w","title":"mx-home-task"}]}"#,
        )]);
        assert!(duplicate.task_create(&container, &task).is_err());

        let target = BackendTarget::new(BackendName::Cmux, "w:s", None).unwrap();
        let mut capture_too_large = backend(vec![
            ok(br#"{"panes":[{"surface_ids":["s"]}]}"#),
            ok(br#"{"text":"oversized"}"#),
        ]);
        assert!(
            capture_too_large
                .capture(&CaptureRequest {
                    target: target.clone(),
                    lines: 1,
                    byte_limit: 2,
                })
                .is_err()
        );

        let mut unknown = backend(vec![
            ok(br#"{"panes":[{"surface_ids":["s"]}]}"#),
            ok(br#"{"text":"plain output"}"#),
        ]);
        assert_eq!(unknown.composer_state(&target).unwrap().as_str(), "unknown");

        let mut submit = backend(vec![
            ok(br#"{"panes":[{"surface_ids":["s"]}]}"#),
            ok(b""),
            ok(br#"{"panes":[{"surface_ids":["s"]}]}"#),
            ok(b""),
            ok(br#"{"panes":[{"surface_ids":["s"]}]}"#),
            ok(br#"{"text":"| Type a message... |"}"#),
        ]);
        let state = submit
            .send_submit(
                &target,
                SubmitRequest {
                    text: "hello",
                    retries: 1,
                    enter_delay: Duration::ZERO,
                    settle: Duration::ZERO,
                },
            )
            .expect("submit");
        assert_eq!(state.as_str(), "empty");

        let mut inventory = backend(vec![]);
        let title = inventory.scoped_title("mx-live").expect("title");
        inventory.runner.outputs.push_back(Ok(CommandOutput {
            status: ExitStatus::from_raw(0),
            stdout: format!(
                r#"{{"workspaces":[{{"id":"w","title":"{title}"}},{{"id":"x","title":"other"}}]}}"#
            )
            .into_bytes(),
            stderr: Vec::new(),
        }));
        inventory
            .runner
            .outputs
            .push_back(ok(br#"{"panes":[{"surface_ids":["s"]}]}"#));
        let live = inventory.list_live(None).expect("inventory");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].target.endpoint(), "w:s");
    }
}

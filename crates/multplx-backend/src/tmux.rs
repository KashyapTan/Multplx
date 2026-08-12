//! Reference tmux runtime backend.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use multplx_core::composer::ComposerState;
use multplx_core::tmux::{BUSY_REGEX_DEFAULT, classify_row};
use regex::RegexBuilder;

use crate::command::{CommandOutput, CommandRequest, CommandRunner, SystemCommandRunner};
use crate::facade::{
    AgentState, BackendError, BackendName, BackendTarget, Capability, CaptureRequest, ContainerId,
    KillOutcome, LiveInventory, LiveTarget, NativeState, RuntimeBackend, SubmitRequest, TaskSpec,
};

const TMUX_OUTPUT_LIMIT: usize = 256 * 1024;

/// tmux backend with an injectable bounded command runner.
#[derive(Debug)]
pub struct TmuxBackend<R = SystemCommandRunner> {
    runner: R,
    executable: OsString,
    inside_tmux: bool,
    busy_pattern: String,
    idle_pattern: Option<String>,
    ghost_luma_max: u16,
}

impl Default for TmuxBackend<SystemCommandRunner> {
    fn default() -> Self {
        Self::system()
    }
}

impl TmuxBackend<SystemCommandRunner> {
    /// Construct the real tmux backend from the current process environment.
    #[must_use]
    pub fn system() -> Self {
        Self::new(
            SystemCommandRunner,
            std::env::var_os("MX_TMUX_BIN").unwrap_or_else(|| OsString::from("tmux")),
            std::env::var_os("TMUX").is_some_and(|value| !value.is_empty()),
        )
    }
}

impl<R: CommandRunner> TmuxBackend<R> {
    /// Construct an injectable tmux backend.
    #[must_use]
    pub fn new(runner: R, executable: impl Into<OsString>, inside_tmux: bool) -> Self {
        Self {
            runner,
            executable: executable.into(),
            inside_tmux,
            busy_pattern: std::env::var("MX_BUSY_REGEX")
                .unwrap_or_else(|_| BUSY_REGEX_DEFAULT.to_owned()),
            idle_pattern: std::env::var("MX_COMPOSER_IDLE_RE").ok(),
            ghost_luma_max: std::env::var("MX_COMPOSER_GHOST_LUMA_MAX")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(128),
        }
    }

    fn request(&self, args: impl IntoIterator<Item = impl Into<OsString>>) -> CommandRequest {
        let mut request = CommandRequest::new(self.executable.clone(), args);
        request.output_limit = TMUX_OUTPUT_LIMIT;
        request
    }

    fn run(
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
        let output = self.run(args)?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(BackendError::Command(format!(
                "tmux exited {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    fn text(
        &mut self,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<String, BackendError> {
        let output = self.success(args)?;
        String::from_utf8(output.stdout)
            .map_err(|_| BackendError::Malformed("tmux stdout is not UTF-8".to_owned()))
    }

    fn ensure_tmux_target(&self, target: &BackendTarget) -> Result<(), BackendError> {
        if target.backend() != BackendName::Tmux {
            return Err(BackendError::InvalidTarget(target.endpoint().to_owned()));
        }
        Ok(())
    }

    fn display(&mut self, target: &BackendTarget, format: &str) -> Result<String, BackendError> {
        self.ensure_tmux_target(target)?;
        self.text(["display-message", "-p", "-t", target.endpoint(), format])
    }

    /// Read tmux's foreground command field for the exact target.
    pub fn current_command(&mut self, target: &BackendTarget) -> Result<String, BackendError> {
        self.display(target, "#{pane_current_command}")
            .map(|command| command.trim().to_owned())
    }

    /// Preserve the legacy adapter's best-effort, single-command kill behavior.
    /// Callers that require a checked postcondition use [`RuntimeBackend::kill_verified`].
    pub fn kill_best_effort(&mut self, target: &BackendTarget) {
        let _ = self.run(["kill-window", "-t", target.endpoint()]);
    }

    fn cursor_row(&mut self, target: &BackendTarget) -> Result<u32, BackendError> {
        self.display(target, "#{cursor_y}")?
            .trim()
            .parse()
            .map_err(|_| BackendError::Malformed("tmux cursor row is not numeric".to_owned()))
    }

    fn styled_row(&mut self, target: &BackendTarget, row: u32) -> Result<Vec<u8>, BackendError> {
        let row = row.to_string();
        self.success([
            "capture-pane",
            "-e",
            "-p",
            "-t",
            target.endpoint(),
            "-S",
            &row,
            "-E",
            &row,
        ])
        .map(|output| output.stdout)
    }

    fn pane_is_busy(&mut self, target: &BackendTarget) -> bool {
        let request = CaptureRequest {
            target: target.clone(),
            lines: 40,
            byte_limit: TMUX_OUTPUT_LIMIT,
        };
        let Ok(bytes) = self.capture(&request) else {
            return false;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return false;
        };
        let Ok(regex) = RegexBuilder::new(&self.busy_pattern)
            .case_insensitive(true)
            .build()
        else {
            return false;
        };
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .rev()
            .take(6)
            .any(|line| regex.is_match(line))
    }

    fn list_windows_raw(&mut self, session: Option<&str>) -> Result<CommandOutput, BackendError> {
        match session {
            Some(session) => self.run(["list-windows", "-t", session, "-F", "#{window_name}"]),
            None => self.run(["list-windows", "-a", "-F", "#{session_name}:#{window_name}"]),
        }
    }

    fn missing_inventory(stderr: &[u8]) -> bool {
        let text = String::from_utf8_lossy(stderr);
        text.contains("can't find session:")
            || text.contains("no server running on ")
            || (text.contains("error connecting to ")
                && (text.contains(" (No such file or directory)")
                    || text.contains(" (Connection refused)")))
    }

    fn split_named_target(target: &str) -> Option<(&str, &str)> {
        let (session, window) = target.split_once(':')?;
        if session.is_empty() || window.is_empty() || window.contains(':') {
            None
        } else {
            Some((session, window))
        }
    }

    fn post_kill_state(&mut self, target: &BackendTarget) -> KillOutcome {
        let Some((session, window)) = Self::split_named_target(target.endpoint()) else {
            return KillOutcome::Unknown;
        };
        match self.list_windows_raw(Some(session)) {
            Ok(output) if output.status.success() => {
                if String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| line == window)
                {
                    KillOutcome::StillPresent
                } else {
                    KillOutcome::Gone
                }
            }
            Ok(output) if Self::missing_inventory(&output.stderr) => KillOutcome::Gone,
            _ => KillOutcome::Unknown,
        }
    }
}

impl<R: CommandRunner> RuntimeBackend for TmuxBackend<R> {
    fn name(&self) -> BackendName {
        BackendName::Tmux
    }

    fn supports(&self, capability: Capability) -> bool {
        matches!(
            capability,
            Capability::ComposerState | Capability::AgentState
        )
    }

    fn tool_check(&mut self) -> Result<(), BackendError> {
        self.success(["-V"]).map(|_| ())
    }

    fn version_check(&mut self) -> Result<String, BackendError> {
        self.text(["-V"]).map(|version| version.trim().to_owned())
    }

    fn container_ensure(&mut self) -> Result<ContainerId, BackendError> {
        if self.inside_tmux {
            return ContainerId::parse(self.text(["display-message", "-p", "#S"])?.trim());
        }
        let has_session = self.run(["has-session", "-t", "broker"])?;
        if !has_session.status.success() {
            self.success(["new-session", "-d", "-s", "broker"])?;
        }
        ContainerId::parse("broker")
    }

    fn task_create(
        &mut self,
        container: &ContainerId,
        task: &TaskSpec,
    ) -> Result<BackendTarget, BackendError> {
        if container.backend() != BackendName::Tmux {
            return Err(BackendError::InvalidContainer(
                container.as_str().to_owned(),
            ));
        }
        if task.label.is_empty()
            || task
                .label
                .bytes()
                .any(|byte| byte == 0 || byte.is_ascii_control() || byte == b':')
        {
            return Err(BackendError::InvalidTarget(task.label.clone()));
        }
        let inventory = self.success([
            "list-windows",
            "-t",
            container.as_str(),
            "-F",
            "#{window_name}",
        ])?;
        if String::from_utf8_lossy(&inventory.stdout)
            .lines()
            .any(|line| line == task.label)
        {
            return Err(BackendError::Command(format!(
                "error: window {}:{} already exists",
                container.as_str(),
                task.label
            )));
        }
        let destination = format!("{}:", container.as_str());
        let cwd = task.working_directory.as_os_str().to_owned();
        let created = self.success([
            OsString::from("new-window"),
            OsString::from("-dP"),
            OsString::from("-F"),
            OsString::from("#{window_id}"),
            OsString::from("-t"),
            OsString::from(destination),
            OsString::from("-n"),
            OsString::from(&task.label),
            OsString::from("-c"),
            cwd,
        ])?;
        let window_id = String::from_utf8(created.stdout)
            .map_err(|_| BackendError::Malformed("tmux window id is not UTF-8".to_owned()))?;
        let window_id = window_id.trim();
        if window_id.is_empty() {
            return Err(BackendError::Malformed(
                "tmux returned an empty window id".to_owned(),
            ));
        }
        let _ = self.run([
            "set-window-option",
            "-t",
            window_id,
            "automatic-rename",
            "off",
        ]);
        let _ = self.run(["set-window-option", "-t", window_id, "allow-rename", "off"]);
        BackendTarget::new(BackendName::Tmux, window_id, Some(task.label.clone()))
    }

    fn target_ready(&mut self, target: &BackendTarget) -> Result<(), BackendError> {
        self.display(target, "#{pane_id}").map(|_| ())
    }

    fn current_path(&mut self, target: &BackendTarget) -> Result<PathBuf, BackendError> {
        self.display(target, "#{pane_current_path}")
            .map(|path| PathBuf::from(path.trim_end_matches(['\r', '\n'])))
    }

    fn capture(&mut self, request: &CaptureRequest) -> Result<Vec<u8>, BackendError> {
        self.ensure_tmux_target(&request.target)?;
        let start = format!("-{}", request.lines);
        let mut command = self.request([
            "capture-pane",
            "-p",
            "-t",
            request.target.endpoint(),
            "-S",
            &start,
        ]);
        command.output_limit = request.byte_limit;
        let output = self
            .runner
            .run(&command)
            .map_err(|error| BackendError::Command(error.to_string()))?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(BackendError::Command(format!(
                "tmux exited {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    fn composer_state(&mut self, target: &BackendTarget) -> Result<ComposerState, BackendError> {
        let row = self.cursor_row(target)?;
        let styled = self.styled_row(target, row)?;
        classify_row(
            &styled,
            self.idle_pattern.as_deref(),
            &self.busy_pattern,
            self.ghost_luma_max,
        )
        .map_err(|error| BackendError::Malformed(error.to_string()))
    }

    fn send_literal(&mut self, target: &BackendTarget, text: &str) -> Result<(), BackendError> {
        self.ensure_tmux_target(target)?;
        self.success(["send-keys", "-t", target.endpoint(), "-l", text])
            .map(|_| ())
    }

    fn send_key(&mut self, target: &BackendTarget, key: &str) -> Result<(), BackendError> {
        self.target_ready(target)?;
        self.success(["send-keys", "-t", target.endpoint(), key])
            .map(|_| ())
    }

    fn send_submit(
        &mut self,
        target: &BackendTarget,
        request: SubmitRequest<'_>,
    ) -> Result<ComposerState, BackendError> {
        self.send_literal(target, request.text)?;
        crate::facade::compatibility_sleep(request.settle);
        for _ in 0..request.retries.max(1) {
            let _ = self.success(["send-keys", "-t", target.endpoint(), "Enter"]);
            crate::facade::compatibility_sleep(request.enter_delay);
            let state = self
                .composer_state(target)
                .unwrap_or(ComposerState::Unknown);
            if state != ComposerState::Pending {
                return Ok(state);
            }
        }
        if self.pane_is_busy(target) {
            Ok(ComposerState::Empty)
        } else {
            Ok(ComposerState::Pending)
        }
    }

    fn send_text_line(&mut self, target: &BackendTarget, text: &str) -> Result<(), BackendError> {
        self.ensure_tmux_target(target)?;
        self.success(["send-keys", "-t", target.endpoint(), text, "Enter"])
            .map(|_| ())
    }

    fn native_state(&mut self, _: &BackendTarget) -> Result<NativeState, BackendError> {
        Err(BackendError::Unsupported {
            backend: BackendName::Tmux,
            capability: "native-state",
        })
    }

    fn agent_state(&mut self, target: &BackendTarget) -> AgentState {
        let Some((session, window)) = Self::split_named_target(target.endpoint()) else {
            return AgentState::Unreadable;
        };
        let inventory = match self.list_windows_raw(Some(session)) {
            Ok(output) => output,
            Err(_) => return AgentState::Unreadable,
        };
        if !inventory.status.success() {
            return if Self::missing_inventory(&inventory.stderr) {
                AgentState::Missing
            } else {
                AgentState::Unreadable
            };
        }
        if !String::from_utf8_lossy(&inventory.stdout)
            .lines()
            .any(|line| line == window)
        {
            return AgentState::Missing;
        }
        let command = match self.display(target, "#{pane_current_command}") {
            Ok(command) => command,
            Err(_) => return AgentState::Unreadable,
        };
        let command = command.trim().trim_start_matches('-');
        if command.is_empty() {
            AgentState::Unreadable
        } else if command.contains("claude") || command.contains("codex") {
            AgentState::Alive
        } else if matches!(
            command,
            "zsh" | "bash" | "sh" | "dash" | "ash" | "ksh" | "mksh" | "tcsh" | "csh" | "fish"
        ) {
            AgentState::Dead
        } else {
            AgentState::Ambiguous
        }
    }

    fn kill_verified(&mut self, target: &BackendTarget) -> KillOutcome {
        if self.ensure_tmux_target(target).is_err() {
            return KillOutcome::Unknown;
        }
        let _ = self.run(["kill-window", "-t", target.endpoint()]);
        self.post_kill_state(target)
    }

    fn list_live(
        &mut self,
        container: Option<&ContainerId>,
    ) -> Result<Vec<LiveTarget>, BackendError> {
        let output = self.list_windows_raw(container.map(ContainerId::as_str))?;
        if !output.status.success() {
            return Err(BackendError::Command(format!(
                "tmux exited {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| BackendError::Malformed("tmux inventory is not UTF-8".to_owned()))?;
        let mut live = Vec::new();
        for line in text.lines().filter(|line| !line.is_empty()) {
            if let Some(container) = container {
                let session = container.as_str();
                live.push(LiveTarget {
                    target: BackendTarget::new(
                        BackendName::Tmux,
                        format!("{session}:{line}"),
                        None,
                    )?,
                    label: line.to_owned(),
                });
            } else if let Some((_, label)) = line.split_once(':') {
                live.push(LiveTarget {
                    target: BackendTarget::new(BackendName::Tmux, line, None)?,
                    label: label.to_owned(),
                });
            }
        }
        Ok(live)
    }

    fn wait_transition(
        &mut self,
        _: &ContainerId,
        _: &[BackendTarget],
        _: Duration,
    ) -> Result<Option<String>, BackendError> {
        Err(BackendError::Unsupported {
            backend: BackendName::Tmux,
            capability: "transition-events",
        })
    }
}

impl<R: CommandRunner> LiveInventory for TmuxBackend<R> {
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
    use std::ffi::OsString;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::ExitStatus;
    use std::time::Duration;

    use multplx_core::composer::ComposerState;

    use crate::command::{CommandError, CommandOutput, CommandRequest, CommandRunner};
    use crate::facade::{
        AgentState, BackendName, BackendTarget, Capability, CaptureRequest, ContainerId,
        KillOutcome, RuntimeBackend, SubmitRequest, TaskSpec,
    };

    use super::TmuxBackend;

    #[derive(Default, Debug)]
    struct FakeRunner {
        calls: Vec<CommandRequest>,
        outputs: VecDeque<Result<CommandOutput, CommandError>>,
    }

    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    fn output(
        code: i32,
        stdout: impl Into<Vec<u8>>,
        stderr: impl Into<Vec<u8>>,
    ) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            status: status(code),
            stdout: stdout.into(),
            stderr: stderr.into(),
        })
    }

    impl CommandRunner for FakeRunner {
        fn run(&mut self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
            self.calls.push(request.clone());
            self.outputs
                .pop_front()
                .unwrap_or_else(|| output(0, Vec::new(), Vec::new()))
        }
    }

    fn target(value: &str) -> BackendTarget {
        BackendTarget::new(BackendName::Tmux, value, None).expect("target")
    }

    #[test]
    fn lifecycle_command_arrays_preserve_order_and_outputs() {
        let runner = FakeRunner {
            outputs: VecDeque::from([
                output(1, b"", b"missing"),
                output(0, b"", b""),
                output(0, b"", b""),
                output(0, b"@9\n", b""),
                output(0, b"", b""),
                output(0, b"", b""),
            ]),
            ..FakeRunner::default()
        };
        let mut backend = TmuxBackend::new(runner, "tmux", false);
        assert_eq!(
            backend.container_ensure().expect("container").as_str(),
            "broker"
        );
        let created = backend
            .task_create(
                &ContainerId::parse("broker").expect("container"),
                &TaskSpec {
                    label: "mx-one".to_owned(),
                    working_directory: PathBuf::from("/tmp/project"),
                },
            )
            .expect("create");
        assert_eq!(created.endpoint(), "@9");
        let calls = &backend.runner.calls;
        assert_eq!(
            calls[0].args,
            ["has-session", "-t", "broker"].map(OsString::from)
        );
        assert_eq!(
            calls[1].args,
            ["new-session", "-d", "-s", "broker"].map(OsString::from)
        );
        assert_eq!(
            calls[2].args,
            ["list-windows", "-t", "broker", "-F", "#{window_name}"].map(OsString::from)
        );
        assert_eq!(calls[3].args[0], "new-window");
        assert_eq!(calls[3].args.last().expect("cwd"), "/tmp/project");
        assert_eq!(
            calls[4].args,
            ["set-window-option", "-t", "@9", "automatic-rename", "off"].map(OsString::from)
        );
        assert_eq!(
            calls[5].args,
            ["set-window-option", "-t", "@9", "allow-rename", "off"].map(OsString::from)
        );
    }

    #[test]
    fn lifecycle_refuses_duplicates_and_handles_inside_tmux() {
        let runner = FakeRunner {
            outputs: VecDeque::from([output(0, b"active\n", b""), output(0, b"mx-one\n", b"")]),
            ..FakeRunner::default()
        };
        let mut backend = TmuxBackend::new(runner, "tmux", true);
        assert_eq!(
            backend.container_ensure().expect("container").as_str(),
            "active"
        );
        assert!(
            backend
                .task_create(
                    &ContainerId::parse("active").expect("container"),
                    &TaskSpec {
                        label: "mx-one".to_owned(),
                        working_directory: PathBuf::from("/tmp")
                    },
                )
                .is_err()
        );
    }

    #[test]
    fn read_send_and_composer_commands_are_exact() {
        let runner = FakeRunner {
            outputs: VecDeque::from([
                output(0, b"%1\n", b""),
                output(0, b"/tmp/wt\n", b""),
                output(0, b"tail\n", b""),
                output(0, b"12\n", b""),
                output(0, "│ ❯ │\n".as_bytes(), b""),
                output(0, b"", b""),
                output(0, b"%1\n", b""),
                output(0, b"", b""),
                output(0, b"", b""),
            ]),
            ..FakeRunner::default()
        };
        let mut backend = TmuxBackend::new(runner, "tmux", false);
        let pane = target("broker:mx-one");
        backend.target_ready(&pane).expect("ready");
        assert_eq!(
            backend.current_path(&pane).expect("path"),
            PathBuf::from("/tmp/wt")
        );
        assert_eq!(
            backend
                .capture(&CaptureRequest {
                    target: pane.clone(),
                    lines: 7,
                    byte_limit: 1024
                })
                .expect("capture"),
            b"tail\n"
        );
        assert_eq!(
            backend.composer_state(&pane).expect("composer"),
            ComposerState::Empty
        );
        backend
            .send_literal(&pane, "literal $text")
            .expect("literal");
        backend.send_key(&pane, "Escape").expect("key");
        backend
            .send_text_line(&pane, "treehouse get")
            .expect("line");
        let calls = &backend.runner.calls;
        assert_eq!(
            calls[2].args,
            ["capture-pane", "-p", "-t", "broker:mx-one", "-S", "-7"].map(OsString::from)
        );
        assert_eq!(
            calls[4].args,
            [
                "capture-pane",
                "-e",
                "-p",
                "-t",
                "broker:mx-one",
                "-S",
                "12",
                "-E",
                "12"
            ]
            .map(OsString::from)
        );
        assert_eq!(
            calls[5].args,
            ["send-keys", "-t", "broker:mx-one", "-l", "literal $text"].map(OsString::from)
        );
        assert_eq!(
            calls[8].args,
            ["send-keys", "-t", "broker:mx-one", "treehouse get", "Enter"].map(OsString::from)
        );
    }

    #[test]
    fn submit_retries_enter_without_retyping_and_accepts_busy_queue() {
        let mut outputs = VecDeque::new();
        outputs.push_back(output(0, b"", b""));
        for _ in 0..2 {
            outputs.push_back(output(0, b"", b""));
            outputs.push_back(output(0, b"0\n", b""));
            outputs.push_back(output(0, "│ typed │\n".as_bytes(), b""));
        }
        outputs.push_back(output(0, b"Working...\n", b""));
        let mut backend = TmuxBackend::new(
            FakeRunner {
                calls: Vec::new(),
                outputs,
            },
            "tmux",
            false,
        );
        let state = backend
            .send_submit(
                &target("broker:mx-one"),
                SubmitRequest {
                    text: "hello",
                    retries: 2,
                    enter_delay: Duration::ZERO,
                    settle: Duration::ZERO,
                },
            )
            .expect("submit");
        assert_eq!(state, ComposerState::Empty);
        assert_eq!(
            backend
                .runner
                .calls
                .iter()
                .filter(|call| call.args.iter().any(|arg| arg == "-l"))
                .count(),
            1
        );
        assert_eq!(
            backend
                .runner
                .calls
                .iter()
                .filter(|call| call.args.last().is_some_and(|arg| arg == "Enter"))
                .count(),
            2
        );
    }

    #[test]
    fn liveness_matrix_requires_exact_inventory_membership() {
        for (inventory_code, inventory_out, inventory_err, command_out, expected) in [
            (
                0,
                b"mx-one\n".as_slice(),
                b"".as_slice(),
                b"codex\n".as_slice(),
                AgentState::Alive,
            ),
            (0, b"mx-one\n", b"", b"-zsh\n", AgentState::Dead),
            (0, b"other\n", b"", b"codex\n", AgentState::Missing),
            (0, b"mx-one\n", b"", b"node\n", AgentState::Ambiguous),
            (
                1,
                b"",
                b"can't find session: broker",
                b"",
                AgentState::Missing,
            ),
            (1, b"", b"permission denied", b"", AgentState::Unreadable),
        ] {
            let mut outputs =
                VecDeque::from([output(inventory_code, inventory_out, inventory_err)]);
            if inventory_code == 0 && inventory_out == b"mx-one\n" {
                outputs.push_back(output(0, command_out, b""));
            }
            let mut backend = TmuxBackend::new(
                FakeRunner {
                    calls: Vec::new(),
                    outputs,
                },
                "tmux",
                false,
            );
            assert_eq!(backend.agent_state(&target("broker:mx-one")), expected);
        }
        let mut backend = TmuxBackend::new(FakeRunner::default(), "tmux", false);
        assert_eq!(
            backend.agent_state(&target("malformed")),
            AgentState::Unreadable
        );
    }

    #[test]
    fn inventory_kill_capabilities_and_failures_are_explicit() {
        let runner = FakeRunner {
            outputs: VecDeque::from([
                output(0, b"one:a\ntwo:b\n", b""),
                output(0, b"", b""),
                output(0, b"other\n", b""),
                output(0, b"tmux 3.6a\n", b""),
                output(0, b"tmux 3.6a\n", b""),
            ]),
            ..FakeRunner::default()
        };
        let mut backend = TmuxBackend::new(runner, "tmux", false);
        let live = backend.list_live(None).expect("inventory");
        assert_eq!(
            live.iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(backend.kill_verified(&target("one:a")), KillOutcome::Gone);
        assert!(backend.supports(Capability::ComposerState));
        assert!(!backend.supports(Capability::NativeState));
        assert!(backend.native_state(&target("one:a")).is_err());
        assert!(
            backend
                .wait_transition(
                    &ContainerId::parse("one").expect("container"),
                    &[],
                    Duration::ZERO
                )
                .is_err()
        );
        assert_eq!(backend.version_check().expect("version"), "tmux 3.6a");
        backend.tool_check().expect("tool");
    }
}

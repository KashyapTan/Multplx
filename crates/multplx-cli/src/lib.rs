//! Command-line dispatch for the Multplx Rust runtime.

mod authority;
mod review;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use multplx_core::process::SystemProcessProbe;

/// The Multplx binary with retained shadow compatibility diagnostics.
#[derive(Debug, Parser)]
#[command(
    name = "mx",
    version,
    about = "Multplx broker runtime",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run one review, delivery, or pull-request security entry point.
    #[command(hide = true, disable_help_flag = true)]
    Review {
        entry: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Run one decision, maintainer-override, or workflow entry point.
    #[command(hide = true, disable_help_flag = true)]
    Authority {
        entry: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Exercise the shadow runtime-backend facade.
    #[command(hide = true)]
    Backend {
        #[command(subcommand)]
        command: BackendCommand,
    },
    /// Exercise the shadow Herdr runtime and transport implementation.
    #[command(hide = true, disable_help_flag = true)]
    Herdr {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Guarded isolated Herdr lab lifecycle.
    #[command(hide = true, disable_help_flag = true)]
    HerdrLab {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// CI-owned Herdr lab-session cleanup.
    #[command(hide = true, disable_help_flag = true)]
    HerdrCiCleanup {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Conservatively retire stale Herdr presentation children.
    #[command(hide = true)]
    HerdrSessionCleanup,
    /// Install the exact pinned Herdr CI artifact.
    #[command(hide = true, disable_help_flag = true)]
    InstallHerdr {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Exercise the Rust cmux transport implementation.
    #[command(hide = true, disable_help_flag = true)]
    Cmux {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Detect and resolve verified harness configuration.
    #[command(hide = true, disable_help_flag = true)]
    Harness {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Validate and exec one verified primary harness.
    #[command(hide = true, disable_help_flag = true)]
    LaunchHarness {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Compute dispatch capacity or operate on its durable queue.
    #[command(hide = true, disable_help_flag = true)]
    Headroom {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Install the exact pinned Treehouse CI artifact.
    #[command(hide = true, disable_help_flag = true)]
    InstallTreehouse {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Capture a bounded endpoint tail through the shadow tmux backend.
    #[command(hide = true)]
    Peek {
        target: String,
        #[arg(default_value_t = 40)]
        lines: u32,
    },
    /// Reconcile one actor's current state through the shadow tmux backend.
    #[command(hide = true)]
    ActorState { id: String },
    /// Operate on the durable local backlog.
    #[command(disable_help_flag = true)]
    Backlog {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Move queued backlog work into a seeded daemon home.
    BacklogHandoff {
        daemon_id: String,
        #[arg(required = true)]
        item_keys: Vec<String>,
    },
    /// Resolve a project's delivery mode and yolo posture.
    ProjectMode { project_name: String },
    /// Construct and classify operational-input protocol messages.
    #[command(disable_help_flag = true)]
    OperationalInput {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Read the backlog backend selector.
    #[command(hide = true)]
    BacklogBackend { config: PathBuf },
    /// Transport adapter for sourced inheritance-library functions.
    #[command(hide = true, disable_help_flag = true)]
    ConfigInherit {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Push inherited local material to live daemon homes.
    #[command(disable_help_flag = true)]
    ConfigPush {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Ensure a project's AGENTS.md memory convention.
    #[command(hide = true)]
    EnsureAgentsMd {
        #[arg(default_value = ".")]
        directory: PathBuf,
    },
    /// Safely refresh one or all registered project clones.
    #[command(hide = true)]
    SystemSync { project: Option<PathBuf> },
    /// Fast-forward the broker and daemon homes from origin.
    #[command(hide = true)]
    Update,
    /// Scaffold an actor brief or daemon charter.
    #[command(hide = true, disable_help_flag = true)]
    Brief {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Send literal text or one named key to a verified task endpoint.
    #[command(hide = true, disable_help_flag = true)]
    Send {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Append an optional correlated daemon report to its parent status path.
    #[command(hide = true, disable_help_flag = true)]
    DaemonReport {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Provision and validate persistent daemon homes.
    #[command(hide = true, disable_help_flag = true)]
    HomeSeed {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Create a routed actor, scout, or daemon task.
    #[command(hide = true, disable_help_flag = true)]
    Spawn {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Run one daemon supervisor loop.
    #[command(hide = true, disable_help_flag = true)]
    SuperviseDaemon {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Safely retire one task or daemon home.
    #[command(hide = true, disable_help_flag = true)]
    Teardown {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Compare the local broker with its configured upstream.
    #[command(hide = true, disable_help_flag = true)]
    UpstreamDiff {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Guarded fast-forward primitives used by compatibility callers.
    #[command(hide = true, disable_help_flag = true)]
    FastForward {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Parent-owned pending-reply record primitives.
    #[command(hide = true, disable_help_flag = true)]
    PendingReply {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Run one supervision, watcher, hook, reporting, or away-mode entry point.
    #[command(hide = true, disable_help_flag = true)]
    Supervision {
        entry: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Run one session-start, health, snapshot, or view entry point.
    #[command(hide = true, disable_help_flag = true)]
    Session {
        entry: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Serve the task-bound status-reporting MCP protocol over stdio.
    #[command(hide = true)]
    ReportMcp,
    /// Verify that the release-mode shadow binary and crate graph are available.
    #[command(hide = true)]
    ShadowDiagnostic,
    /// Exercise Portion 02 core contracts without selecting Rust in production.
    #[command(hide = true)]
    Primitive {
        #[command(subcommand)]
        command: PrimitiveCommand,
    },
}

#[derive(Debug, Subcommand)]
enum BackendCommand {
    ToolCheck,
    VersionCheck,
    ContainerEnsure,
    TaskCreate {
        container: String,
        label: String,
        working_directory: PathBuf,
    },
    TargetReady {
        target: String,
    },
    CurrentPath {
        target: String,
    },
    CurrentCommand {
        target: String,
    },
    Capture {
        target: String,
        lines: u32,
    },
    ComposerState {
        target: String,
    },
    SendLiteral {
        target: String,
        text: String,
    },
    SendKey {
        target: String,
        key: String,
    },
    SendSubmit {
        target: String,
        text: String,
        retries: usize,
        enter_delay: String,
        settle: String,
    },
    SendTextLine {
        target: String,
        text: String,
    },
    Kill {
        target: String,
    },
    AgentState {
        target: String,
    },
    AgentAlive {
        target: String,
    },
    ListLive {
        #[arg(long)]
        container: Option<String>,
    },
    ResolveBare {
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum PrimitiveCommand {
    BackendHomeTag {
        root: PathBuf,
        home: PathBuf,
    },
    TaskId {
        value: String,
    },
    AtomicReplace {
        path: PathBuf,
        mode: String,
    },
    ProcessIdentity {
        pid: u32,
    },
    CheckRegistered {
        state: PathBuf,
        task: String,
    },
    ComposerClassify {
        bordered: String,
        content: String,
        #[arg(long)]
        idle_regex: Option<String>,
        #[arg(long)]
        insensitive: bool,
        #[arg(long)]
        plain_content: Option<String>,
    },
    ComposerStripAnsi,
    ComposerStripGhost {
        #[arg(long, default_value_t = 128)]
        luma_max: u16,
    },
    SignalResolve {
        native: String,
        run_step: String,
        self_report: String,
        heuristic: String,
    },
    StatusOpenDecisions {
        path: PathBuf,
    },
    GateRefuse,
    JournalEmit {
        state: PathBuf,
        task: String,
        event: String,
        detail: String,
        source: String,
        timestamp: String,
    },
    GitLockStale {
        lock: PathBuf,
        #[arg(long)]
        companion: Option<PathBuf>,
        minimum_age: u64,
        now_epoch: u64,
    },
    MarkerMark,
    MarkerIs,
    PrimaryScope {
        root: PathBuf,
        state: PathBuf,
    },
    ProbeInstall {
        tool: String,
    },
    SessionLockStatus {
        path: PathBuf,
    },
    SupervisionStatus {
        state: PathBuf,
        grace: u64,
        now_epoch: u64,
    },
    SupervisorTarget,
    SupervisorBackend,
    Tangle {
        root: PathBuf,
    },
    TransitionRecord {
        pane_id: String,
        workspace_id: String,
        from_status: String,
        to_status: String,
        agent: String,
    },
    TransitionPolicy {
        to_status: String,
    },
    WakeAppend {
        state: PathBuf,
        kind: String,
        key: String,
        payload: String,
        epoch: u64,
    },
    WakeDedupe {
        path: PathBuf,
    },
}

impl Cli {
    /// Parses an explicit `mx <subcommand>` invocation or an `mx-<subcommand>`
    /// compatibility entry point.
    pub fn parse_multicall<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        if let Some(program) = args.first().cloned()
            && let Some(alias) = multicall_alias(&program)
        {
            args.insert(1, alias);
        }
        Self::parse_from(args)
    }

    /// Runs the selected command.
    pub fn run(self) -> i32 {
        match self.command {
            Command::Review { entry, args } => review::run(&entry, &args),
            Command::Authority { entry, args } => authority::run(&entry, &args),
            Command::Backend { command } => run_backend(command),
            Command::Herdr { args } => run_herdr(&args),
            Command::HerdrLab { args } => multplx_backend::herdr_tools::run_lab(&args),
            Command::HerdrCiCleanup { args } => multplx_backend::herdr_tools::run_ci_cleanup(&args),
            Command::HerdrSessionCleanup => multplx_backend::herdr_cleanup::run_session_cleanup(),
            Command::InstallHerdr { args } => multplx_backend::herdr_tools::run_installer(&args),
            Command::Cmux { args } => run_cmux(&args),
            Command::Harness { args } => run_harness(&args),
            Command::LaunchHarness { args } => run_launch_harness(&args),
            Command::Headroom { args } => run_headroom(&args),
            Command::InstallTreehouse { args } => {
                multplx_backend::treehouse_tools::run_installer(&args)
            }
            Command::Peek { target, lines } => run_peek(&target, lines),
            Command::ActorState { id } => run_actor_state(&id),
            Command::Backlog { args } => run_backlog(&args),
            Command::BacklogHandoff {
                daemon_id,
                item_keys,
            } => run_backlog_handoff(&daemon_id, &item_keys),
            Command::ProjectMode { project_name } => run_project_mode(&project_name),
            Command::OperationalInput { args } => run_operational_input(&args),
            Command::BacklogBackend { config } => {
                println!("{}", multplx_domain::backlog::backend_value(&config));
                0
            }
            Command::ConfigInherit { args } => run_config_inherit(&args),
            Command::ConfigPush { args } => run_config_push(&args),
            Command::EnsureAgentsMd { directory } => {
                match multplx_domain::lifecycle::ensure_agents::ensure(&directory) {
                    Ok(message) => {
                        println!("{message}");
                        0
                    }
                    Err(error) => {
                        eprintln!("{error}");
                        1
                    }
                }
            }
            Command::SystemSync { project } => {
                let (_, home, _) = active_paths();
                let projects = std::env::var_os("MX_PROJECTS_OVERRIDE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join("projects"));
                let result = multplx_domain::lifecycle::system_sync::run(
                    &multplx_domain::lifecycle::system_sync::SyncContext { home, projects },
                    project.as_deref(),
                );
                for line in result.stdout {
                    println!("{line}");
                }
                for line in result.stderr {
                    eprintln!("{line}");
                }
                0
            }
            Command::Update => {
                let (root, home, data) = active_paths();
                let state = std::env::var_os("MX_STATE_OVERRIDE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join("state"));
                let context = multplx_domain::lifecycle::fast_forward::Context {
                    root,
                    home,
                    marker: ".mx-daemon-home".to_owned(),
                };
                for line in multplx_domain::lifecycle::fast_forward::update(
                    &context,
                    &state,
                    &data.join("daemons.md"),
                ) {
                    println!("{line}");
                }
                0
            }
            Command::Brief { args } => {
                if args.len() == 1 && matches!(args[0].to_str(), Some("-h" | "--help")) {
                    print!("{}", multplx_domain::lifecycle::brief::HELP);
                    0
                } else {
                    let (root, home, data) = active_paths();
                    let state = std::env::var_os("MX_STATE_OVERRIDE")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| home.join("state"));
                    match multplx_domain::lifecycle::brief::run(&args, &root, &home, &data, &state)
                    {
                        Ok(message) => {
                            println!("{message}");
                            0
                        }
                        Err(error) => {
                            eprintln!("error: {}", error.message);
                            error.code
                        }
                    }
                }
            }
            Command::Send { args } => run_send(&args),
            Command::DaemonReport { args } => run_daemon_report(&args),
            Command::HomeSeed { args } => run_lifecycle_compat("mx-home-seed.sh", &args),
            Command::Spawn { args } => run_lifecycle_compat("mx-spawn.sh", &args),
            Command::SuperviseDaemon { args } => {
                run_lifecycle_compat("mx-supervise-daemon.sh", &args)
            }
            Command::Teardown { args } => run_lifecycle_compat("mx-teardown.sh", &args),
            Command::UpstreamDiff { args } => run_lifecycle_compat("mx-upstream-diff.sh", &args),
            Command::FastForward { args } => run_fast_forward(&args),
            Command::PendingReply { args } => run_pending_reply(&args),
            Command::Supervision { entry, args } => run_supervision(&entry, &args),
            Command::Session { entry, args } => run_session(&entry, &args),
            Command::ReportMcp => {
                let root = runtime_root(&active_paths().0);
                multplx_services::report_mcp::serve(&root)
            }
            Command::ShadowDiagnostic => {
                let boundaries = [
                    multplx_core::SHADOW_BOUNDARY,
                    multplx_domain::SHADOW_BOUNDARY,
                    multplx_backend::SHADOW_BOUNDARY,
                    multplx_services::SHADOW_BOUNDARY,
                ];
                debug_assert_eq!(boundaries.len(), 4);
                println!("multplx rust shadow: ready");
                0
            }
            Command::Primitive { command } => match run_primitive(command) {
                Ok(status) => status,
                Err(error) => {
                    eprintln!("mx primitive: {error}");
                    1
                }
            },
        }
    }
}

fn run_session(entry: &str, args: &[OsString]) -> i32 {
    const ENTRIES: &[&str] = &[
        "mx-bootstrap.sh",
        "mx-doctor.sh",
        "mx-session-start.sh",
        "mx-sessionstart-nudge.sh",
        "mx-supervision-instructions.sh",
        "mx-status-snapshot.sh",
        "mx-system-snapshot.sh",
        "mx-system-view.sh",
        "mx-timeline.sh",
    ];
    if !ENTRIES.contains(&entry) {
        eprintln!("error: unknown session entry point: {entry}");
        return 2;
    }
    if entry == "mx-sessionstart-nudge.sh" {
        let (root, home, _) = active_paths();
        let state = std::env::var_os("MX_STATE_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("state"));
        let result = multplx_domain::session::sessionstart_nudge(&root, &state);
        print!("{}", result.stdout);
        eprint!("{}", result.stderr);
        return result.status;
    }
    if entry == "mx-supervision-instructions.sh" {
        let values = args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let (root, _, _) = active_paths();
        let source_root = runtime_root(&root);
        let detected = detect_primary_harness(&source_root);
        let result = multplx_domain::session::supervision_instructions(
            &values,
            &detected,
            &source_root,
            &root,
        );
        print!("{}", result.stdout);
        eprint!("{}", result.stderr);
        return result.status;
    }
    if entry == "mx-timeline.sh" {
        let values = args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let (root, home, data) = active_paths();
        let state = std::env::var_os("MX_STATE_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("state"));
        let result = multplx_domain::timeline::run(&values, &state, &data, &runtime_root(&root));
        print!("{}", result.stdout);
        eprint!("{}", result.stderr);
        return result.status;
    }
    if entry == "mx-system-view.sh" {
        return run_system_view(args);
    }
    run_session_compat(entry, args)
}

fn run_system_view(args: &[OsString]) -> i32 {
    if args.len() == 1 && matches!(args[0].to_str(), Some("-h" | "--help")) {
        print!("{}", multplx_domain::snapshot::SYSTEM_VIEW_USAGE);
        return 0;
    }
    if args.len() == 1 && args[0] == OsStr::new("--json") {
        return run_session_compat("mx-system-snapshot.sh", &[OsString::from("--json")]);
    }
    if !args.is_empty() {
        eprint!("{}", multplx_domain::snapshot::SYSTEM_VIEW_USAGE);
        return 2;
    }
    if !multplx_domain::snapshot::command_exists("jq") {
        eprintln!("mx-system-view: jq not found");
        return 1;
    }
    let (root, _, _) = active_paths();
    let source_root = runtime_root(&root);
    let output = std::process::Command::new("bash")
        .arg(source_root.join("bin/mx-system-snapshot.sh"))
        .arg("--json")
        .env("MX_SESSION_IMPLEMENTATION", "legacy")
        .env("MX_RUST_SOURCE_ROOT", &source_root)
        .output();
    let Ok(output) = output else {
        eprintln!("error: could not start mx-system-snapshot.sh");
        return 1;
    };
    if !output.status.success() {
        let _ = io::stdout().write_all(&output.stdout);
        let _ = io::stderr().write_all(&output.stderr);
        return output.status.code().unwrap_or(1);
    }
    match multplx_domain::snapshot::parse_system_snapshot(&output.stdout) {
        Ok(snapshot) => {
            print!(
                "{}",
                multplx_domain::snapshot::render_system_view(&snapshot)
            );
            0
        }
        Err(result) => {
            print!("{}", result.stdout);
            eprint!("{}", result.stderr);
            result.status
        }
    }
}

fn detect_primary_harness(root: &Path) -> String {
    let output = std::process::Command::new(root.join("bin/mx-harness.sh")).output();
    let Ok(output) = output else {
        return "unknown".to_owned();
    };
    if !output.status.success() || output.stdout.len() > 4096 {
        return "unknown".to_owned();
    }
    let harness = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if matches!(harness.as_str(), "claude" | "codex" | "cursor" | "pi") {
        harness
    } else {
        "unknown".to_owned()
    }
}

/// Transitional Portion 09 process boundary for the large composed readers.
/// The selector is pinned before the shell body can acquire a lock, repair
/// state, recurse into a daemon home, or invoke another Portion 09 entry.
fn run_session_compat(script: &str, args: &[OsString]) -> i32 {
    use std::os::unix::process::CommandExt;

    let root = runtime_root(&active_paths().0);
    let path = root.join("bin").join(script);
    if !path.is_file() {
        eprintln!(
            "error: session compatibility body is unavailable at {}",
            path.display()
        );
        return 1;
    }
    let error = std::process::Command::new("bash")
        .arg(path)
        .args(args)
        .env("MX_SESSION_IMPLEMENTATION", "legacy")
        .env("MX_RUST_SOURCE_ROOT", &root)
        .exec();
    eprintln!("error: could not start {script}: {error}");
    1
}

fn run_supervision(entry: &str, args: &[OsString]) -> i32 {
    const ENTRIES: &[&str] = &[
        "mx-afk-launch.sh",
        "mx-afk-return.sh",
        "mx-afk-start.sh",
        "mx-arm-pretool-check.sh",
        "mx-cd-pretool-check.sh",
        "mx-claude-stop-autoarm.sh",
        "mx-cursor-hook.sh",
        "mx-guard.sh",
        "mx-report",
        "mx-subagent-pretool-check.sh",
        "mx-turnend-guard.sh",
        "mx-wake-drain.sh",
        "mx-watch-arm.sh",
        "mx-watch-checkpoint.sh",
        "mx-watch.sh",
    ];
    if !ENTRIES.contains(&entry) {
        eprintln!("error: unknown supervision entry point: {entry}");
        return 2;
    }
    if entry == "mx-report" {
        let values = args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let root = active_paths().0;
        let result = multplx_domain::supervision::report(&values, &root);
        print!("{}", result.stdout);
        eprint!("{}", result.stderr);
        return result.status;
    }
    if entry == "mx-wake-drain.sh" {
        return run_wake_drain();
    }
    if entry == "mx-subagent-pretool-check.sh" {
        let values = args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut payload = String::new();
        let _ = io::stdin().read_to_string(&mut payload);
        let root = active_paths().0;
        let result = multplx_domain::supervision::subagent_guard(&values, &payload, &root);
        print!("{}", result.stdout);
        eprint!("{}", result.stderr);
        return result.status;
    }
    if matches!(entry, "mx-arm-pretool-check.sh" | "mx-cd-pretool-check.sh") {
        let values = args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut payload = String::new();
        let _ = io::stdin().read_to_string(&mut payload);
        let root = active_paths().0;
        let policy = if entry == "mx-arm-pretool-check.sh" {
            multplx_domain::supervision::PretoolPolicy::WatcherArm
        } else {
            multplx_domain::supervision::PretoolPolicy::PersistentCd
        };
        let result = multplx_domain::supervision::pretool_guard(policy, &values, &payload, &root);
        print!("{}", result.stdout);
        eprint!("{}", result.stderr);
        return result.status;
    }
    run_supervision_compat(entry, args)
}

fn run_wake_drain() -> i32 {
    use multplx_core::process::SystemProcessProbe;
    use multplx_core::wake::{AnnotationLimits, WakeQueue, render_annotations};

    let (_, home, _) = active_paths();
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if let Err(error) = fs::create_dir_all(&state) {
        eprintln!("mx wake drain: cannot create {}: {error}", state.display());
        return 1;
    }
    let queue = WakeQueue::new(&state);
    let processes = SystemProcessProbe::default();
    if let Err(error) = queue.recover_abandoned_drains(&processes) {
        eprintln!("mx wake drain: {error}");
        return 1;
    }
    let delay = std::env::var("MX_WAKE_DRAIN_TEST_DELAY_BEFORE_COMMIT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let drained = queue.drain_with_publish(&processes, |records| {
        if delay > 0 {
            std::thread::sleep(Duration::from_secs(delay));
        }
        let mut stdout = io::stdout().lock();
        for record in records {
            stdout
                .write_all(record.render().as_bytes())
                .map_err(|error| multplx_core::error::CoreError::Command {
                    command: "publish wake drain".to_owned(),
                    reason: error.to_string(),
                })?;
        }
        stdout
            .flush()
            .map_err(|error| multplx_core::error::CoreError::Command {
                command: "flush wake drain".to_owned(),
                reason: error.to_string(),
            })
    });
    let records = match drained {
        Ok(records) => records,
        Err(error) => {
            eprintln!("mx wake drain: {error}");
            return 1;
        }
    };
    let enrich_delay = std::env::var("MX_WAKE_ENRICH_TEST_DELAY")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if enrich_delay > 0 {
        std::thread::sleep(Duration::from_secs(enrich_delay));
    }
    if let (Some(path), Some(target)) = (
        std::env::var_os("MX_WAKE_ENRICH_SWAP_PATH"),
        std::env::var_os("MX_WAKE_ENRICH_SWAP_TARGET"),
    ) {
        use std::os::unix::fs::symlink;
        let path = PathBuf::from(path);
        let _ = fs::remove_file(&path);
        let _ = symlink(PathBuf::from(target), path);
    }
    if let Some(log) = std::env::var_os("MX_WAKE_ENRICH_PERL_LOG")
        && let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(log)
    {
        for _ in records
            .iter()
            .filter(|record| record.kind == multplx_core::wake::WakeKind::Signal)
            .take(8)
        {
            let _ = file.write_all(b"read\n");
        }
    }
    print!(
        "{}",
        render_annotations(&state, &records, AnnotationLimits::default())
    );
    let root = runtime_root(&active_paths().0);
    let _ = std::process::Command::new("bash")
        .arg(root.join("bin/mx-guard.sh"))
        .env("MX_SUPERVISION_IMPLEMENTATION", "legacy")
        .status();
    0
}

/// Transitional process boundary for Portion 08 while individual state-machine
/// handlers move behind this dispatch surface. The child is pinned to the
/// compatibility body before it can mutate state, preventing mixed-engine
/// execution and accidental recursive dispatch.
fn run_supervision_compat(script: &str, args: &[OsString]) -> i32 {
    use std::os::unix::process::CommandExt;

    let root = runtime_root(&active_paths().0);
    let path = root.join("bin").join(script);
    if !path.is_file() {
        eprintln!(
            "error: supervision compatibility body is unavailable at {}",
            path.display()
        );
        return 1;
    }
    let error = std::process::Command::new("bash")
        .arg(path)
        .args(args)
        .env("MX_SUPERVISION_IMPLEMENTATION", "legacy")
        .env("MX_RUST_SOURCE_ROOT", &root)
        .exec();
    eprintln!("error: could not start {script}: {error}");
    1
}

fn tmux_target(value: &str) -> Result<multplx_backend::facade::BackendTarget, String> {
    multplx_backend::facade::BackendTarget::new(
        multplx_backend::facade::BackendName::Tmux,
        value,
        None,
    )
    .map_err(|error| error.to_string())
}

fn parse_seconds(value: &str) -> Result<Duration, String> {
    let seconds = value
        .parse::<f64>()
        .map_err(|_| format!("invalid duration: {value}"))?;
    if !seconds.is_finite() || seconds.is_sign_negative() {
        return Err(format!("invalid duration: {value}"));
    }
    Ok(Duration::from_secs_f64(seconds))
}

fn backend_error(error: impl std::fmt::Display) -> i32 {
    eprintln!("mx backend: {error}");
    1
}

fn send_target_ready(target: &multplx_backend::facade::BackendTarget) -> bool {
    use multplx_backend::facade::{BackendName, RuntimeBackend};
    match target.backend() {
        BackendName::Tmux => multplx_backend::tmux::TmuxBackend::system()
            .target_ready(target)
            .is_ok(),
        BackendName::Herdr => multplx_backend::herdr::HerdrBackend::system()
            .target_ready(target)
            .is_ok(),
        BackendName::Cmux => multplx_backend::cmux::CmuxBackend::system()
            .target_ready(target)
            .is_ok(),
    }
}

fn send_key_to(target: &multplx_backend::facade::BackendTarget, key: &str) -> Result<(), String> {
    use multplx_backend::facade::{BackendName, RuntimeBackend};
    match target.backend() {
        BackendName::Tmux => multplx_backend::tmux::TmuxBackend::system().send_key(target, key),
        BackendName::Herdr => multplx_backend::herdr::HerdrBackend::system().send_key(target, key),
        BackendName::Cmux => multplx_backend::cmux::CmuxBackend::system().send_key(target, key),
    }
    .map_err(|error| error.to_string())
}

fn send_text_to(
    target: &multplx_backend::facade::BackendTarget,
    text: &str,
    retries: usize,
    enter_delay: Duration,
    settle: Duration,
) -> Result<multplx_core::composer::ComposerState, String> {
    use multplx_backend::facade::{BackendName, RuntimeBackend, SubmitRequest};
    let request = SubmitRequest {
        text,
        retries,
        enter_delay,
        settle,
    };
    match target.backend() {
        BackendName::Tmux => {
            multplx_backend::tmux::TmuxBackend::system().send_submit(target, request)
        }
        BackendName::Herdr => {
            multplx_backend::herdr::HerdrBackend::system().send_submit(target, request)
        }
        BackendName::Cmux => {
            multplx_backend::cmux::CmuxBackend::system().send_submit(target, request)
        }
    }
    .map_err(|error| error.to_string())
}

struct SendResolution {
    target: multplx_backend::facade::BackendTarget,
    meta: Option<PathBuf>,
    selector: bool,
    tried: String,
}

fn send_resolve(raw: &str, state: &Path) -> Result<SendResolution, String> {
    use multplx_backend::facade::{
        BackendName, BackendTarget, backend_of_meta, meta_for_selector, meta_for_target, meta_get,
        target_of_meta,
    };
    if let Some((id, meta)) = meta_for_selector(state, raw).map_err(|error| error.to_string())? {
        let endpoint = target_of_meta(&meta)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                format!(
                    "no backend target recorded in {} (tried meta={}; backend=from-meta)",
                    meta.display(),
                    meta.display()
                )
            })?;
        let backend = backend_of_meta(&meta).map_err(|error| error.to_string())?;
        return Ok(SendResolution {
            target: BackendTarget::new(backend, endpoint, Some(format!("mx-{id}")))
                .map_err(|error| error.to_string())?,
            meta: Some(meta.clone()),
            selector: true,
            tried: format!("meta={}; backend=from-meta", meta.display()),
        });
    }
    if raw.starts_with("mx-") {
        return Err(format!(
            "no metadata for {raw} in {} (tried meta={}/{raw}.meta; legacy-meta={}/{}.meta; backend=none); pass a well-formed explicit backend target only when targeting outside this Multplx home",
            state.display(),
            state.display(),
            state.display(),
            raw.strip_prefix("mx-").unwrap_or(raw)
        ));
    }
    if state.is_dir() {
        let mut paths: Vec<PathBuf> = fs::read_dir(state)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(OsStr::to_str) == Some("meta"))
            .collect();
        paths.sort();
        for meta in &paths {
            if meta_get(meta, "herdr_pane_id")
                .map_err(|error| error.to_string())?
                .as_deref()
                == Some(raw)
            {
                let session = meta_get(meta, "herdr_session")
                    .map_err(|error| error.to_string())?
                    .unwrap_or_else(|| "<herdr-session>".to_owned());
                let id = meta.file_stem().unwrap_or_default().to_string_lossy();
                return Err(format!(
                    "target '{raw}' matches herdr_pane_id in {} but is missing its herdr session prefix; expected <herdr-session>:<pane-id> such as '{session}:{raw}' or use 'mx-{id}' (tried meta={}/{raw}.meta; backend=herdr)",
                    meta.display(),
                    state.display()
                ));
            }
        }
    }
    if let Some(meta) = meta_for_target(state, raw).map_err(|error| error.to_string())? {
        let endpoint = target_of_meta(&meta)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("no backend target recorded in {}", meta.display()))?;
        let backend = backend_of_meta(&meta).map_err(|error| error.to_string())?;
        return Ok(SendResolution {
            target: BackendTarget::new(backend, endpoint, None)
                .map_err(|error| error.to_string())?,
            meta: Some(meta.clone()),
            selector: false,
            tried: format!(
                "explicit target '{raw}' matched {}; backend={backend}",
                meta.display()
            ),
        });
    }
    if raw.contains(':') {
        let backend = if raw.matches(':').count() >= 2 {
            BackendName::Herdr
        } else {
            BackendName::Tmux
        };
        let target = BackendTarget::new(backend, raw, None).map_err(|error| error.to_string())?;
        if !send_target_ready(&target) {
            return Err(format!(
                "explicit target '{raw}' is not a live {backend} endpoint (tried meta={}/{raw}.meta; metadata window/terminal lookup; backend={backend}). Use mx-<id> for a recorded task/lane, or pass a target whose backend endpoint can be verified.",
                state.display()
            ));
        }
        return Ok(SendResolution {
            target,
            meta: None,
            selector: false,
            tried: format!(
                "meta={}/{raw}.meta; metadata window/terminal lookup; backend={backend}; endpoint=verified",
                state.display()
            ),
        });
    }
    Err(format!(
        "target '{raw}' is not resolvable (tried meta={}/{raw}.meta; metadata window/terminal lookup; backend=none). Use mx-{raw} for a recorded task/lane, or pass a well-formed explicit backend target such as session:window.",
        state.display()
    ))
}

fn run_send(args: &[OsString]) -> i32 {
    if multplx_core::gate_refuse::is_gate_agent(
        std::env::var_os("DEEP_REVIEW_GATE").is_some(),
        std::env::var("MX_GATE_REFUSE_BYPASS").as_deref() == Ok("1"),
    ) {
        eprintln!("{}", multplx_core::gate_refuse::REFUSAL_MESSAGE);
        return i32::from(multplx_core::gate_refuse::REFUSAL_EXIT);
    }
    let Some(home) = std::env::var_os("MX_HOME").filter(|value| !value.is_empty()) else {
        eprintln!(
            "error: MX_HOME is not set; mx-send refuses to resolve targets without an explicit Multplx home"
        );
        return 1;
    };
    let home = PathBuf::from(home);
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    if !home.is_dir() {
        eprintln!(
            "error: MX_HOME '{}' is not a directory; mx-send cannot resolve this home's state",
            home.display()
        );
        return 1;
    }
    if !state.is_dir() {
        eprintln!(
            "error: state dir '{}' is missing; mx-send cannot resolve targets for MX_HOME '{}'",
            state.display(),
            home.display()
        );
        return 1;
    }
    run_send_in_home(args, home, state)
}

fn run_send_in_home(args: &[OsString], home: PathBuf, state: PathBuf) -> i32 {
    if args.len() < 2 {
        eprintln!("usage: mx-send.sh <target> <text...>");
        return 2;
    }
    let Some(raw) = args[0].to_str() else {
        eprintln!("error: target is not UTF-8");
        return 1;
    };
    let resolved = match send_resolve(raw, &state) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let (root, _, _) = active_paths();
    let guard = runtime_root(&root).join("bin/mx-guard.sh");
    if guard.is_file() {
        let _ = std::process::Command::new(guard)
            .env(
                "MX_GUARD_CONTINUE_LINE",
                "This is a supervision warning only; the requested message WILL still be sent.",
            )
            .status();
    }
    if args.get(1).and_then(|value| value.to_str()) == Some("--key") {
        let Some(key) = args.get(2).and_then(|value| value.to_str()) else {
            eprintln!("usage: mx-send.sh <target> --key <key>");
            return 2;
        };
        if send_key_to(&resolved.target, key).is_err() {
            eprintln!(
                "error: key '{key}' not sent to {} ({} send failed; tried {})",
                resolved.target.endpoint(),
                resolved.target.backend(),
                resolved.tried
            );
            return 1;
        }
        return 0;
    }
    let mut message = args[1..]
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let mut correlation = None;
    let mut created = false;
    if resolved.selector
        && resolved.meta.as_ref().is_some_and(|meta| {
            multplx_backend::facade::meta_get(meta, "kind")
                .ok()
                .flatten()
                .as_deref()
                == Some("daemon")
        })
    {
        let meta = resolved.meta.as_ref().expect("selector meta");
        let task = meta.file_stem().unwrap_or_default().to_string_lossy();
        let existing = std::env::var("MX_PENDING_REPLY_EXISTING_CORR")
            .ok()
            .or_else(|| multplx_domain::lifecycle::pending_reply::extract_correlation(&message));
        let corr = if existing.as_ref().is_some_and(|value| {
            multplx_domain::lifecycle::pending_reply::reusable(&state, value, &task)
        }) {
            existing.expect("reusable")
        } else {
            created = true;
            match multplx_domain::lifecycle::pending_reply::create(&home, &state, &task, &message) {
                Ok(value) => value,
                Err(_) => {
                    eprintln!(
                        "error: failed to create parent pending-reply expectation for {task}"
                    );
                    return 1;
                }
            }
        };
        message = multplx_domain::lifecycle::pending_reply::embed(&message, &corr);
        if created
            && multplx_domain::lifecycle::pending_reply::prepare_delivery(&state, &corr).is_err()
        {
            let _ = multplx_domain::lifecycle::pending_reply::discard_undelivered(&state, &corr);
            eprintln!("error: failed to durably prepare pending-reply delivery for {task}");
            return 1;
        }
        correlation = Some(corr);
    }
    let harness = resolved.meta.as_ref().and_then(|meta| {
        multplx_backend::facade::meta_get(meta, "harness")
            .ok()
            .flatten()
    });
    let settle = if message.starts_with('/')
        || (message.starts_with('$') && harness.as_deref() == Some("codex"))
    {
        Duration::from_secs_f64(1.2)
    } else {
        Duration::from_secs_f64(0.3)
    };
    let retries = std::env::var("MX_SEND_RETRIES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3);
    let delay = std::env::var("MX_SEND_SLEEP")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(Duration::from_secs_f64)
        .unwrap_or_else(|| Duration::from_secs_f64(0.4));
    let verdict = send_text_to(&resolved.target, &message, retries, delay, settle);
    if verdict.as_ref().is_err()
        || verdict
            .as_ref()
            .is_ok_and(|state| *state == multplx_core::composer::ComposerState::Pending)
    {
        if created && let Some(corr) = &correlation {
            let _ = multplx_domain::lifecycle::pending_reply::discard_undelivered(&state, corr);
        }
        if verdict.is_ok() {
            eprintln!(
                "error: text not submitted to {} (Enter swallowed; text left in composer; tried {})",
                resolved.target.endpoint(),
                resolved.tried
            );
        } else {
            eprintln!(
                "error: text not sent to {} ({} send failed; tried {})",
                resolved.target.endpoint(),
                resolved.target.backend(),
                resolved.tried
            );
        }
        return 1;
    }
    if let Some(corr) = correlation
        && multplx_domain::lifecycle::pending_reply::confirm_delivery(&state, &corr).is_err()
    {
        eprintln!(
            "error: text was delivered to {}, but its pending-reply delivery commit failed; a durable recovery marker was stored and the watcher will reconcile it. Do not resend.",
            resolved.target.endpoint()
        );
        return 1;
    }
    let post_settle = std::env::var("MX_SEND_SETTLE").unwrap_or_else(|_| "1".to_owned());
    if post_settle != "0" {
        let _ = std::process::Command::new("sleep")
            .arg(&post_settle)
            .status();
    }
    0
}

fn run_daemon_report(args: &[OsString]) -> i32 {
    use std::fs::OpenOptions;

    const USAGE: &str = "Usage:\n  mx-daemon-report.sh <status-file> <verb> <corr_id> <note...>\n  mx-daemon-report.sh --doc <status-file> <verb> <corr_id> <doc-path> <note...>\n";
    let doc_mode = args.first().and_then(|value| value.to_str()) == Some("--doc");
    let offset = usize::from(doc_mode);
    if args.len() < offset + 4 {
        eprint!("{USAGE}");
        return 2;
    }
    let status = PathBuf::from(&args[offset]);
    let verb = args[offset + 1].to_string_lossy();
    let raw_correlation = args[offset + 2].to_string_lossy();
    let correlation = raw_correlation
        .strip_prefix("corr=")
        .unwrap_or(&raw_correlation);
    if correlation.len() != 16 || !correlation.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        eprintln!("error: corr_id must be 16 hex characters (got '{correlation}')");
        return 1;
    }
    let Some(parent) = status.parent() else {
        eprintln!(
            "error: cannot create parent directory for status file '{}'",
            status.display()
        );
        return 1;
    };
    let _ = fs::create_dir_all(parent);
    if !parent.is_dir() {
        eprintln!(
            "error: cannot create parent directory for status file '{}'",
            status.display()
        );
        return 1;
    }
    let values: Vec<String> = args[offset + 3..]
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    let line = if doc_mode {
        let Some(document) = values.first() else {
            eprint!("{USAGE}");
            return 2;
        };
        let note = values[1..].join(" ");
        if note.is_empty() {
            format!("{verb} [corr={correlation}]: {document} (via-helper)\n")
        } else {
            format!("{verb} [corr={correlation}]: {note} ({document} via-helper)\n")
        }
    } else {
        let note = values.join(" ");
        if note.is_empty() {
            format!("{verb} [corr={correlation}]: (via-helper)\n")
        } else {
            format!("{verb} [corr={correlation}]: {note} (via-helper)\n")
        }
    };
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&status)
        .and_then(|mut file| file.write_all(line.as_bytes()))
    {
        Ok(()) => 0,
        Err(error) => {
            eprintln!(
                "error: cannot append status file '{}': {error}",
                status.display()
            );
            1
        }
    }
}

/// Transitional process boundary for lifecycle commands whose transaction body
/// is still retained below the thin shell adapter.  Rust owns selection and
/// recursion prevention; the child is explicitly pinned to the compatibility
/// body, so a process can never mix Rust spawn with legacy teardown (or the
/// reverse) through ambient defaults.
fn run_lifecycle_compat(script: &str, args: &[OsString]) -> i32 {
    use std::os::unix::process::CommandExt;

    let root = runtime_root(&active_paths().0);
    let path = root.join("bin").join(script);
    if !path.is_file() {
        eprintln!(
            "error: lifecycle compatibility body is unavailable at {}",
            path.display()
        );
        return 1;
    }
    let error = std::process::Command::new("bash")
        .arg(path)
        .args(args)
        .env("MX_LIFECYCLE_IMPLEMENTATION", "legacy")
        .env("MX_RUST_SOURCE_ROOT", &root)
        .exec();
    eprintln!("error: could not start {script}: {error}");
    1
}

fn run_fast_forward(args: &[OsString]) -> i32 {
    use multplx_domain::lifecycle::fast_forward::{self, Base, Context, Status};
    let values: Vec<String> = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    let Some(operation) = values.first().map(String::as_str) else {
        eprintln!("usage: mx fast-forward <default-branch|primary-head|validate-home|target> ...");
        return 2;
    };
    match operation {
        "default-branch" if values.len() == 2 => {
            if let Some(branch) = fast_forward::default_branch(Path::new(&values[1])) {
                println!("{branch}");
                0
            } else {
                1
            }
        }
        "primary-head" if values.len() == 2 => {
            if let Some(commit) = fast_forward::primary_head_commit(Path::new(&values[1])) {
                println!("{commit}");
                0
            } else {
                1
            }
        }
        "validate-home" if values.len() == 5 => {
            let context = Context {
                root: PathBuf::from(&values[1]),
                home: PathBuf::from(&values[2]),
                marker: std::env::var("SUB_HOME_MARKER")
                    .unwrap_or_else(|_| ".mx-daemon-home".to_owned()),
            };
            match fast_forward::validate_daemon_home(&context, &values[3], Path::new(&values[4])) {
                Ok(path) => {
                    println!("{}", path.display());
                    0
                }
                Err(error) => {
                    println!("{error}");
                    1
                }
            }
        }
        "target" if values.len() == 6 => {
            let base = if values[3] == "origin" {
                Base::Origin
            } else {
                Base::Commit(values[3].clone())
            };
            let outcome = fast_forward::fast_forward(
                Path::new(&values[1]),
                &values[2],
                &base,
                values[4] == "yes",
                values[5] == "yes",
            );
            let status = match outcome.status {
                Status::Updated => "updated",
                Status::Current => "current",
                Status::Skipped => "skipped",
            };
            println!(
                "{status}\t{}\t{}",
                outcome.instructions.join(", "),
                outcome.line
            );
            0
        }
        _ => {
            eprintln!("error: invalid fast-forward operation or arguments");
            2
        }
    }
}

fn run_pending_reply(args: &[OsString]) -> i32 {
    use multplx_domain::lifecycle::pending_reply;
    let values: Vec<String> = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    let result: Result<Option<String>, String> = match values.first().map(String::as_str) {
        Some("extract") if values.len() == 2 => Ok(pending_reply::extract_correlation(&values[1])),
        Some("reusable") if values.len() == 4 => {
            if pending_reply::reusable(Path::new(&values[1]), &values[2], &values[3]) {
                Ok(None)
            } else {
                Err(String::new())
            }
        }
        Some("embed") if values.len() == 3 => {
            Ok(Some(pending_reply::embed(&values[1], &values[2])))
        }
        Some("create") if values.len() == 5 => pending_reply::create(
            Path::new(&values[1]),
            Path::new(&values[2]),
            &values[3],
            &values[4],
        )
        .map(Some),
        Some("prepare") if values.len() == 3 => {
            pending_reply::prepare_delivery(Path::new(&values[1]), &values[2]).map(|()| None)
        }
        Some("confirm") if values.len() == 3 => {
            pending_reply::confirm_delivery(Path::new(&values[1]), &values[2]).map(|()| None)
        }
        Some("discard") if values.len() == 3 => {
            pending_reply::discard_undelivered(Path::new(&values[1]), &values[2]).map(|()| None)
        }
        _ => {
            eprintln!("error: invalid pending-reply operation or arguments");
            return 2;
        }
    };
    match result {
        Ok(Some(value)) => {
            print!("{value}");
            0
        }
        Ok(None) => 0,
        Err(error) => {
            if !error.is_empty() {
                eprintln!("{error}");
            }
            1
        }
    }
}

fn run_backend(command: BackendCommand) -> i32 {
    use multplx_backend::facade::RuntimeBackend;

    let mut backend = multplx_backend::tmux::TmuxBackend::system();
    let result: Result<(), String> = (|| {
        match command {
            BackendCommand::ToolCheck => backend.tool_check().map_err(|error| error.to_string())?,
            BackendCommand::VersionCheck => println!(
                "{}",
                backend.version_check().map_err(|error| error.to_string())?
            ),
            BackendCommand::ContainerEnsure => println!(
                "{}",
                backend
                    .container_ensure()
                    .map_err(|error| error.to_string())?
                    .as_str()
            ),
            BackendCommand::TaskCreate {
                container,
                label,
                working_directory,
            } => {
                let container = multplx_backend::facade::ContainerId::parse(container)
                    .map_err(|error| error.to_string())?;
                let task = multplx_backend::facade::TaskSpec {
                    label,
                    working_directory,
                };
                println!(
                    "{}",
                    backend
                        .task_create(&container, &task)
                        .map_err(|error| error.to_string())?
                        .endpoint()
                );
            }
            BackendCommand::TargetReady { target } => backend
                .target_ready(&tmux_target(&target)?)
                .map_err(|error| error.to_string())?,
            BackendCommand::CurrentPath { target } => println!(
                "{}",
                backend
                    .current_path(&tmux_target(&target)?)
                    .map_err(|error| error.to_string())?
                    .display()
            ),
            BackendCommand::CurrentCommand { target } => {
                let target = tmux_target(&target)?;
                println!(
                    "{}",
                    backend
                        .current_command(&target)
                        .map_err(|error| error.to_string())?
                );
            }
            BackendCommand::Capture { target, lines } => {
                let bytes = backend
                    .capture(&multplx_backend::facade::CaptureRequest {
                        target: tmux_target(&target)?,
                        lines,
                        byte_limit: 256 * 1024,
                    })
                    .map_err(|error| error.to_string())?;
                io::stdout()
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
            }
            BackendCommand::ComposerState { target } => println!(
                "{}",
                backend
                    .composer_state(&tmux_target(&target)?)
                    .map_err(|error| error.to_string())?
                    .as_str()
            ),
            BackendCommand::SendLiteral { target, text } => backend
                .send_literal(&tmux_target(&target)?, &text)
                .map_err(|error| error.to_string())?,
            BackendCommand::SendKey { target, key } => backend
                .send_key(&tmux_target(&target)?, &key)
                .map_err(|error| error.to_string())?,
            BackendCommand::SendSubmit {
                target,
                text,
                retries,
                enter_delay,
                settle,
            } => {
                let target = tmux_target(&target)?;
                match backend.send_submit(
                    &target,
                    multplx_backend::facade::SubmitRequest {
                        text: &text,
                        retries,
                        enter_delay: parse_seconds(&enter_delay)?,
                        settle: parse_seconds(&settle)?,
                    },
                ) {
                    Ok(state) => print!("{}", state.as_str()),
                    Err(_) => print!("send-failed"),
                }
            }
            BackendCommand::SendTextLine { target, text } => backend
                .send_text_line(&tmux_target(&target)?, &text)
                .map_err(|error| error.to_string())?,
            BackendCommand::Kill { target } => {
                backend.kill_best_effort(&tmux_target(&target)?);
            }
            BackendCommand::AgentState { target } => {
                println!("{}", backend.agent_state(&tmux_target(&target)?).as_str())
            }
            BackendCommand::AgentAlive { target } => println!(
                "{}",
                backend.agent_state(&tmux_target(&target)?).alive_token()
            ),
            BackendCommand::ListLive { container } => {
                let container = container
                    .map(multplx_backend::facade::ContainerId::parse)
                    .transpose()
                    .map_err(|error| error.to_string())?;
                for target in backend
                    .list_live(container.as_ref())
                    .map_err(|error| error.to_string())?
                {
                    println!("{}", target.target.endpoint());
                }
            }
            BackendCommand::ResolveBare { name } => {
                let found = backend
                    .list_live(None)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .find(|target| target.label == name)
                    .ok_or_else(|| format!("no window named {name}"))?;
                println!("{}", found.target.endpoint());
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => backend_error(error),
    }
}

fn cmux_target(
    value: &str,
    expected: Option<String>,
) -> Result<multplx_backend::facade::BackendTarget, String> {
    multplx_backend::facade::BackendTarget::new(
        multplx_backend::facade::BackendName::Cmux,
        value,
        expected,
    )
    .map_err(|error| error.to_string())
}

fn run_cmux(args: &[OsString]) -> i32 {
    use multplx_backend::facade::{
        CaptureRequest, ContainerId, RuntimeBackend, SubmitRequest, TaskSpec,
    };

    let command = args
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let result: Result<i32, String> = (|| {
        let mut backend = multplx_backend::cmux::CmuxBackend::system();
        let expected = |index: usize| {
            args.get(index)
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        match command {
            "bin" => {
                require_len(args, 1)?;
                if backend.executable_available() {
                    print!("{}", backend.executable().to_string_lossy());
                    Ok(0)
                } else {
                    Ok(1)
                }
            }
            "password" => {
                require_len(args, 1)?;
                if let Some(value) = backend.socket_password() {
                    print!("{value}");
                }
                Ok(0)
            }
            "cli" => {
                if args.len() < 2 {
                    return Err("cli requires cmux arguments".to_owned());
                }
                let output = backend
                    .cli(args[1..].iter().cloned())
                    .map_err(|error| error.to_string())?;
                io::stdout()
                    .write_all(&output.stdout)
                    .map_err(|error| error.to_string())?;
                io::stderr()
                    .write_all(&output.stderr)
                    .map_err(|error| error.to_string())?;
                Ok(output.status.code().unwrap_or(1))
            }
            "tool-check" => {
                require_len(args, 1)?;
                backend.tool_check().map_err(|error| error.to_string())?;
                Ok(0)
            }
            "version-check" => {
                require_len(args, 1)?;
                backend.version_check().map_err(|error| error.to_string())?;
                Ok(0)
            }
            "ping-state" => {
                require_len(args, 1)?;
                print!("{}", backend.ping_state().as_str());
                Ok(0)
            }
            "ensure-running" => {
                require_len(args, 1)?;
                backend
                    .ensure_running()
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "container-ensure" => {
                require_len(args, 1)?;
                backend
                    .container_ensure()
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "home-label" => {
                require_len(args, 1)?;
                print!(
                    "{}",
                    backend.home_label().map_err(|error| error.to_string())?
                );
                Ok(0)
            }
            "scoped-title" => {
                let label = utf8_arg(args, 1, "label")?;
                require_len(args, 2)?;
                print!(
                    "{}",
                    backend
                        .scoped_title(label)
                        .map_err(|error| error.to_string())?
                );
                Ok(0)
            }
            "workspace-id-for-label" => {
                let label = utf8_arg(args, 1, "label")?;
                require_len(args, 2)?;
                if let Some(value) = backend
                    .workspace_id_for_label(label)
                    .map_err(|error| error.to_string())?
                {
                    print!("{value}");
                }
                Ok(0)
            }
            "surface-id-for-workspace" => {
                let workspace = utf8_arg(args, 1, "workspace")?;
                require_len(args, 2)?;
                if let Some(value) = backend
                    .surface_id_for_workspace(workspace)
                    .map_err(|error| error.to_string())?
                {
                    print!("{value}");
                }
                Ok(0)
            }
            "create-task" => {
                let label = utf8_arg(args, 1, "label")?.to_owned();
                let cwd = PathBuf::from(
                    args.get(2)
                        .ok_or_else(|| "missing working directory".to_owned())?,
                );
                require_len(args, 3)?;
                let container =
                    ContainerId::for_backend(multplx_backend::facade::BackendName::Cmux, "cmux")
                        .map_err(|error| error.to_string())?;
                let target = backend
                    .task_create(
                        &container,
                        &TaskSpec {
                            label,
                            working_directory: cwd,
                        },
                    )
                    .map_err(|error| error.to_string())?;
                let (workspace, surface) = multplx_backend::cmux::parse_target(target.endpoint())
                    .map_err(|error| error.to_string())?;
                print!("{workspace} {surface}");
                Ok(0)
            }
            "parse-target" => {
                let target = utf8_arg(args, 1, "target")?;
                require_len(args, 2)?;
                let (workspace, surface) = multplx_backend::cmux::parse_target(target)
                    .map_err(|error| error.to_string())?;
                print!("{workspace}\t{surface}");
                Ok(0)
            }
            "surface-exists" => {
                let workspace = utf8_arg(args, 1, "workspace")?;
                let surface = utf8_arg(args, 2, "surface")?;
                require_len(args, 3)?;
                Ok(
                    if backend.surface_exists(workspace, surface).unwrap_or(false) {
                        0
                    } else {
                        1
                    },
                )
            }
            "target-ready" => {
                let target = utf8_arg(args, 1, "target")?;
                let target = cmux_target(target, expected(2))?;
                backend
                    .target_ready(&target)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "current-path" => {
                let target = utf8_arg(args, 1, "target")?;
                let target = cmux_target(target, expected(2))?;
                let path = backend
                    .current_path(&target)
                    .map_err(|error| error.to_string())?;
                print!("{}", path.display());
                Ok(0)
            }
            "send-literal" => {
                let target = utf8_arg(args, 1, "target")?;
                let text = utf8_arg(args, 2, "text")?;
                let target = cmux_target(target, expected(3))?;
                backend
                    .send_literal(&target, text)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "normalize-key" => {
                let key = utf8_arg(args, 1, "key")?;
                require_len(args, 2)?;
                print!("{}", multplx_backend::cmux::normalize_key(key));
                Ok(0)
            }
            "send-key" => {
                let target = utf8_arg(args, 1, "target")?;
                let key = utf8_arg(args, 2, "key")?;
                let target = cmux_target(target, expected(3))?;
                backend
                    .send_key(&target, key)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "send-text-line" => {
                let target = utf8_arg(args, 1, "target")?;
                let text = utf8_arg(args, 2, "text")?;
                let target = cmux_target(target, expected(3))?;
                backend
                    .send_text_line(&target, text)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "capture" => {
                let target = utf8_arg(args, 1, "target")?;
                let lines = args
                    .get(2)
                    .and_then(|value| value.to_str())
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(200);
                let target = cmux_target(target, expected(3))?;
                let bytes = backend
                    .capture(&CaptureRequest {
                        target,
                        lines,
                        byte_limit: 256 * 1024,
                    })
                    .map_err(|error| error.to_string())?;
                io::stdout()
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "composer-state" => {
                let target = utf8_arg(args, 1, "target")?;
                let target = cmux_target(target, expected(2))?;
                print!(
                    "{}",
                    backend
                        .composer_state(&target)
                        .unwrap_or(multplx_core::composer::ComposerState::Unknown)
                        .as_str()
                );
                Ok(0)
            }
            "send-submit" => {
                let target = utf8_arg(args, 1, "target")?;
                let text = utf8_arg(args, 2, "text")?;
                let retries = utf8_arg(args, 3, "retries")?
                    .parse::<usize>()
                    .map_err(|_| "invalid retries".to_owned())?;
                let enter_delay = parse_seconds(utf8_arg(args, 4, "enter delay")?)?;
                let settle = parse_seconds(utf8_arg(args, 5, "settle")?)?;
                let target = cmux_target(target, expected(6))?;
                match backend.send_submit(
                    &target,
                    SubmitRequest {
                        text,
                        retries,
                        enter_delay,
                        settle,
                    },
                ) {
                    Ok(state) => print!("{}", state.as_str()),
                    Err(_) => print!("send-failed"),
                }
                Ok(0)
            }
            "window-of-workspace" => {
                let workspace = utf8_arg(args, 1, "workspace")?;
                require_len(args, 2)?;
                if let Some((window, count)) = backend
                    .window_of_workspace(workspace)
                    .map_err(|error| error.to_string())?
                {
                    print!("{window} {count}");
                }
                Ok(0)
            }
            "kill" => {
                let target = utf8_arg(args, 1, "target")?;
                let target = cmux_target(target, expected(3))?;
                backend.kill_best_effort(&target);
                Ok(0)
            }
            "list-live" => {
                require_len(args, 1)?;
                for item in backend.list_live(None).map_err(|error| error.to_string())? {
                    println!("{}\t{}", item.target.endpoint(), item.label);
                }
                Ok(0)
            }
            _ => Err(format!("unknown cmux command: {command}")),
        }
    })();
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

fn run_harness(args: &[OsString]) -> i32 {
    let root = std::env::var_os("MX_ROOT_OVERRIDE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let home = std::env::var_os("MX_HOME")
        .map(PathBuf::from)
        .unwrap_or(root);
    let config = std::env::var_os("MX_CONFIG_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("config"));
    let settings = multplx_backend::harness::HarnessConfig::new(config);
    let own = multplx_backend::harness::detect();
    let value = match args
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
    {
        "actor" => Some(settings.actor(own)),
        "daemon" => Some(settings.daemon(own)),
        "daemon-model" => settings.daemon_model(),
        "daemon-effort" => settings.daemon_effort(),
        _ => Some(own.to_string()),
    };
    if let Some(value) = value {
        println!("{value}");
    }
    0
}

fn run_launch_harness(args: &[OsString]) -> i32 {
    let Some(harness) = args.first().and_then(|value| value.to_str()) else {
        eprintln!("multplx: harness must be claude, codex, cursor, or pi");
        return 2;
    };
    multplx_backend::harness_launch::run(harness, &args[1..])
}

fn run_headroom(args: &[OsString]) -> i32 {
    use multplx_backend::headroom::{HeadroomPaths, QueueRecord};

    let paths = HeadroomPaths::from_environment();
    let result: Result<String, String> = (|| {
        match args.first().and_then(|value| value.to_str()).unwrap_or_default() {
            "--json" if args.len() == 1 => serde_json::to_string(&multplx_backend::headroom::evaluate(&paths).map_err(|error| error.to_string())?).map(|value| format!("{value}\n")).map_err(|error| error.to_string()),
            "--queue" if args.len() == 1 => multplx_backend::headroom::queue_list(&paths).map_err(|error| error.to_string()),
            "--queue-cancel" if args.len() == 2 => multplx_backend::headroom::queue_cancel(&paths, utf8_arg(args, 1, "task id")?).map_err(|error| error.to_string()),
            "--queue-drain" if args.len() == 1 => multplx_backend::headroom::queue_drain(&paths).map_err(|error| error.to_string()),
            "--queue-add" if args.len() >= 3 => {
                let id = utf8_arg(args, 1, "task id")?.to_owned();
                let project = utf8_arg(args, 2, "project")?.to_owned();
                let mut harness = String::new();
                let mut model = String::new();
                let mut effort = String::new();
                let mut backend = String::new();
                let mut kind = "delivery".to_owned();
                let mut index = 3;
                while index < args.len() {
                    let flag = args[index].to_str().ok_or_else(|| "queue profile argument is not UTF-8".to_owned())?;
                    match flag {
                        "--scout" => { kind = "scout".to_owned(); index += 1; }
                        "--harness" | "--model" | "--effort" | "--backend" => {
                            let value = args.get(index + 1).and_then(|value| value.to_str()).ok_or_else(|| format!("{flag} requires a value"))?.to_owned();
                            match flag { "--harness" => harness = value, "--model" => model = value, "--effort" => effort = value, _ => backend = value }
                            index += 2;
                        }
                        _ => return Err(format!("unknown queue profile argument: {flag}")),
                    }
                }
                multplx_backend::headroom::queue_add(&paths, &QueueRecord { task_id: id, project, harness, model, effort, backend, kind, enqueued_at: multplx_backend::headroom::now_epoch() }).map_err(|error| error.to_string())
            }
            "--json" => Err("--json takes no arguments".to_owned()),
            "--queue" => Err("--queue takes no arguments".to_owned()),
            "--queue-cancel" => Err("--queue-cancel requires exactly one task id".to_owned()),
            "--queue-drain" => Err("--queue-drain takes no arguments".to_owned()),
            "--queue-add" => Err("--queue-add requires task id and project".to_owned()),
            "-h" | "--help" => Ok("Composite dispatch capacity and durable parked-dispatch queue.\n\nUsage:\n  mx-headroom.sh --json\n  mx-headroom.sh --queue\n  mx-headroom.sh --queue-add <id> <project> [profile flags]\n  mx-headroom.sh --queue-cancel <id>\n  mx-headroom.sh --queue-drain\n".to_owned()),
            _ => Err("usage: mx-headroom.sh --json|--queue|--queue-add|--queue-cancel|--queue-drain".to_owned()),
        }
    })();
    match result {
        Ok(output) => {
            print!("{output}");
            0
        }
        Err(error) => {
            eprintln!("mx-headroom: {error}");
            1
        }
    }
}

fn run_herdr(args: &[OsString]) -> i32 {
    use multplx_backend::facade::{
        BackendName, BackendTarget, CaptureRequest, ContainerId, RuntimeBackend, SubmitRequest,
        TaskSpec,
    };
    use multplx_backend::herdr::{
        HerdrBackend, PaneAgentState, clear_transition, commit_transition,
    };
    use multplx_backend::herdr_presentation::{
        FocusSnapshot, ProjectionBinding, ProjectionJournal, ReclaimOutcome, bind_journal,
        concise_task_label, create_journal, home_identity, journal_path, projection_id,
        projection_workspace_label, read_journal, replace_journal_endpoint, write_journal_v2,
    };
    use multplx_core::transition::TransitionRecord;

    let command = args
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let result: Result<i32, String> = (|| {
        let mut backend = HerdrBackend::system();
        let target = |value: &str| {
            BackendTarget::new(BackendName::Herdr, value.to_owned(), None)
                .map_err(|error| error.to_string())
        };
        match command {
            "cli" => {
                let session = utf8_arg(args, 1, "session")?;
                if args.len() < 3 {
                    return Err("cli requires Herdr arguments".to_owned());
                }
                let output = backend
                    .scoped_cli(session, &args[2..])
                    .map_err(|error| error.to_string())?;
                io::stdout()
                    .write_all(&output.stdout)
                    .map_err(|error| error.to_string())?;
                io::stderr()
                    .write_all(&output.stderr)
                    .map_err(|error| error.to_string())?;
                Ok(output.status.code().unwrap_or(1))
            }
            "workspace-label" => {
                require_len(args, 1)?;
                print!("{}", backend.workspace_label());
                Ok(0)
            }
            "tool-check" => {
                require_len(args, 1)?;
                backend.tool_check().map_err(|error| error.to_string())?;
                Ok(0)
            }
            "version-check" => {
                require_len(args, 1)?;
                backend.version_check().map_err(|error| error.to_string())?;
                Ok(0)
            }
            "server-ensure" => {
                let session = utf8_arg(args, 1, "session")?;
                require_len(args, 2)?;
                backend
                    .server_ensure(session)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "workspace-find" => {
                let session = utf8_arg(args, 1, "session")?;
                require_len(args, 2)?;
                if let Some(workspace) = backend.workspace_find(session) {
                    print!("{workspace}");
                }
                Ok(0)
            }
            "container-ensure" => {
                let cwd = args
                    .get(1)
                    .map(PathBuf::from)
                    .unwrap_or(std::env::current_dir().map_err(|error| error.to_string())?);
                require_len(args, if args.get(1).is_some() { 2 } else { 1 })?;
                backend.version_check().map_err(|error| error.to_string())?;
                let session = backend.session().to_owned();
                backend
                    .server_ensure(&session)
                    .map_err(|error| error.to_string())?;
                let workspace = backend
                    .workspace_ensure(&session, &cwd)
                    .map_err(|error| error.to_string())?;
                print!(
                    "{session}:{workspace}\t{}",
                    backend.seeded_tab_id().unwrap_or_default()
                );
                Ok(0)
            }
            "task-create" => {
                let container =
                    ContainerId::for_backend(BackendName::Herdr, utf8_arg(args, 1, "container")?)
                        .map_err(|error| error.to_string())?;
                let label = utf8_arg(args, 2, "label")?.to_owned();
                let cwd = PathBuf::from(
                    args.get(3)
                        .ok_or_else(|| "missing working directory".to_owned())?,
                );
                let seed = args
                    .get(4)
                    .and_then(|value| value.to_str())
                    .filter(|value| !value.is_empty());
                require_len(args, if args.get(4).is_some() { 5 } else { 4 })?;
                let endpoint = backend
                    .create_task_full(
                        &container,
                        &TaskSpec {
                            label,
                            working_directory: cwd,
                        },
                        seed,
                    )
                    .map_err(|error| error.to_string())?;
                print!("{} {}", endpoint.tab_id, endpoint.pane_id);
                Ok(0)
            }
            "target-ready" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                require_len(args, 2)?;
                backend
                    .target_ready(&target)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "current-path" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                require_len(args, 2)?;
                print!(
                    "{}",
                    backend
                        .current_path(&target)
                        .map_err(|error| error.to_string())?
                        .display()
                );
                Ok(0)
            }
            "capture" | "capture-ansi" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                let lines = utf8_arg(args, 2, "lines")?.parse::<u32>().unwrap_or(200);
                require_len(args, 3)?;
                let bytes = if command == "capture" {
                    backend.capture(&CaptureRequest {
                        target,
                        lines,
                        byte_limit: 256 * 1024,
                    })
                } else {
                    backend.capture_ansi(&target, lines)
                }
                .map_err(|error| error.to_string())?;
                io::stdout()
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "composer-state" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                require_len(args, 2)?;
                print!(
                    "{}",
                    backend
                        .composer_state(&target)
                        .map_err(|error| error.to_string())?
                        .as_str()
                );
                Ok(0)
            }
            "send-literal" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                let text = utf8_arg(args, 2, "text")?;
                require_len(args, 3)?;
                backend
                    .send_literal(&target, text)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "send-key" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                let key = utf8_arg(args, 2, "key")?;
                require_len(args, 3)?;
                backend
                    .send_key(&target, key)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "send-text-line" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                let text = utf8_arg(args, 2, "text")?;
                require_len(args, 3)?;
                backend
                    .send_text_line(&target, text)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "send-submit" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                let text = utf8_arg(args, 2, "text")?;
                let retries = utf8_arg(args, 3, "retries")?
                    .parse()
                    .map_err(|_| "invalid retries".to_owned())?;
                let enter_delay = parse_seconds(utf8_arg(args, 4, "enter delay")?)?;
                let settle = parse_seconds(utf8_arg(args, 5, "settle")?)?;
                require_len(args, 6)?;
                match backend.send_submit(
                    &target,
                    SubmitRequest {
                        text,
                        retries,
                        enter_delay,
                        settle,
                    },
                ) {
                    Ok(state) => print!("{}", state.as_str()),
                    Err(_) => print!("send-failed"),
                }
                Ok(0)
            }
            "native-state" | "busy-state" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                require_len(args, 2)?;
                let state = backend.native_state(&target).ok();
                if command == "native-state" {
                    print!(
                        "{}",
                        state.map_or("unknown", |state| match state {
                            multplx_backend::facade::NativeState::Idle => "idle",
                            multplx_backend::facade::NativeState::Working => "working",
                            multplx_backend::facade::NativeState::Blocked => "blocked",
                            multplx_backend::facade::NativeState::Done => "done",
                        })
                    );
                } else {
                    print!(
                        "{}",
                        match state {
                            Some(multplx_backend::facade::NativeState::Working) => "busy",
                            Some(
                                multplx_backend::facade::NativeState::Idle
                                | multplx_backend::facade::NativeState::Blocked
                                | multplx_backend::facade::NativeState::Done,
                            ) => "idle",
                            None => "unknown",
                        }
                    );
                }
                Ok(0)
            }
            "pane-agent-state" => {
                let session = utf8_arg(args, 1, "session")?;
                let pane = utf8_arg(args, 2, "pane")?;
                require_len(args, 3)?;
                print!("{}", backend.pane_agent_state(session, pane).as_str());
                Ok(0)
            }
            "agent-state" | "agent-alive" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                require_len(args, 2)?;
                let state = backend.agent_state(&target);
                print!(
                    "{}",
                    if command == "agent-state" {
                        state.as_str()
                    } else {
                        state.alive_token()
                    }
                );
                Ok(0)
            }
            "kill" => {
                let target = target(utf8_arg(args, 1, "target")?)?;
                require_len(args, 2)?;
                let _ = backend.kill_verified(&target);
                Ok(0)
            }
            "list-live" => {
                let session = args
                    .get(1)
                    .and_then(|value| value.to_str())
                    .unwrap_or(backend.session())
                    .to_owned();
                require_len(args, if args.get(1).is_some() { 2 } else { 1 })?;
                let Some(workspace) = backend.workspace_find(&session) else {
                    return Ok(0);
                };
                let container =
                    ContainerId::for_backend(BackendName::Herdr, format!("{session}:{workspace}"))
                        .map_err(|error| error.to_string())?;
                for live in backend
                    .list_live(Some(&container))
                    .map_err(|error| error.to_string())?
                {
                    println!("{}\t{}", live.target.endpoint(), live.label);
                }
                Ok(0)
            }
            "events-capable" => {
                let session = utf8_arg(args, 1, "session")?;
                require_len(args, 2)?;
                Ok(if backend.events_capable(session) {
                    0
                } else {
                    1
                })
            }
            "event-reader" => {
                let socket = PathBuf::from(
                    args.get(1)
                        .ok_or_else(|| "missing socket path".to_owned())?,
                );
                let timeout = parse_seconds(utf8_arg(args, 2, "timeout")?)?;
                if args.len() < 4 {
                    return Err("at least one pane is required".to_owned());
                }
                let panes = args[3..]
                    .iter()
                    .map(|value| value.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                match multplx_backend::herdr_wire::event_wait(&socket, timeout, &panes, |line| {
                    writeln!(io::stdout(), "{line}")?;
                    io::stdout().flush()
                }) {
                    Ok(()) => Ok(0),
                    Err(
                        multplx_backend::herdr_wire::WireError::Invalid(_)
                        | multplx_backend::herdr_wire::WireError::Connect(_)
                        | multplx_backend::herdr_wire::WireError::Send(_),
                    ) => Ok(2),
                    Err(multplx_backend::herdr_wire::WireError::Protocol(_)) => Ok(3),
                    Err(multplx_backend::herdr_wire::WireError::Receive(_)) => Ok(4),
                }
            }
            "workspace-move" => {
                let socket = PathBuf::from(
                    args.get(1)
                        .ok_or_else(|| "missing socket path".to_owned())?,
                );
                let workspace = utf8_arg(args, 2, "workspace")?;
                let index = utf8_arg(args, 3, "insert index")?
                    .parse::<u64>()
                    .map_err(|_| "invalid insert index".to_owned())?;
                require_len(args, 4)?;
                match multplx_backend::herdr_wire::workspace_move(&socket, workspace, index) {
                    Ok(value) => {
                        println!(
                            "{}",
                            serde_json::to_string(&value).map_err(|error| error.to_string())?
                        );
                        Ok(0)
                    }
                    Err(
                        multplx_backend::herdr_wire::WireError::Invalid(_)
                        | multplx_backend::herdr_wire::WireError::Connect(_),
                    ) => Ok(2),
                    Err(
                        multplx_backend::herdr_wire::WireError::Send(_)
                        | multplx_backend::herdr_wire::WireError::Receive(_),
                    ) => Ok(3),
                    Err(multplx_backend::herdr_wire::WireError::Protocol(_)) => Ok(4),
                }
            }
            "wait-transition" => {
                let session = utf8_arg(args, 1, "session")?;
                let timeout = parse_seconds(utf8_arg(args, 2, "timeout")?)?;
                let state =
                    PathBuf::from(args.get(3).ok_or_else(|| "missing state dir".to_owned())?);
                if args.len() < 5 {
                    return Ok(2);
                }
                let windows = args[4..]
                    .iter()
                    .map(|value| value.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                match backend.wait_transition_in_state(session, timeout, &state, &windows) {
                    Ok(Some(record)) => {
                        print!("{}", record.render());
                        Ok(0)
                    }
                    Ok(None) => Ok(1),
                    Err(_) => Ok(2),
                }
            }
            "transition-commit" => {
                let state = PathBuf::from(args.get(1).ok_or_else(|| "missing state".to_owned())?);
                let session = utf8_arg(args, 2, "session")?;
                let record = TransitionRecord::parse(utf8_arg(args, 3, "record")?)
                    .map_err(|error| error.to_string())?;
                require_len(args, 4)?;
                commit_transition(&state, session, &record).map_err(|error| error.to_string())?;
                Ok(0)
            }
            "transition-clear" => {
                let state = PathBuf::from(args.get(1).ok_or_else(|| "missing state".to_owned())?);
                let window = utf8_arg(args, 2, "window")?;
                require_len(args, 3)?;
                clear_transition(&state, window).map_err(|error| error.to_string())?;
                Ok(0)
            }
            "projection-id" => {
                require_len(args, 1)?;
                print!("{}", projection_id().map_err(|error| error.to_string())?);
                Ok(0)
            }
            "projection-label" => {
                let task = utf8_arg(args, 1, "task")?;
                let token = utf8_arg(args, 2, "token")?;
                require_len(args, 3)?;
                print!("{}", projection_workspace_label(task, token));
                Ok(0)
            }
            "concise-task-label" => {
                let task = utf8_arg(args, 1, "task")?;
                require_len(args, 2)?;
                print!("{}", concise_task_label(task));
                Ok(0)
            }
            "normalize-key" => {
                let key = utf8_arg(args, 1, "key")?;
                require_len(args, 2)?;
                print!(
                    "{}",
                    match key {
                        "Enter" | "enter" => "enter",
                        "Escape" | "escape" | "Esc" | "esc" => "escape",
                        "C-c" | "c-c" | "ctrl+c" | "Ctrl+C" => "ctrl+c",
                        other => other,
                    }
                );
                Ok(0)
            }
            "journal-path" => {
                let state = PathBuf::from(args.get(1).ok_or_else(|| "missing state".to_owned())?);
                let task = utf8_arg(args, 2, "task")?;
                require_len(args, 3)?;
                print!("{}", journal_path(&state, task).display());
                Ok(0)
            }
            "journal-create" => {
                let state = PathBuf::from(args.get(1).ok_or_else(|| "missing state".to_owned())?);
                let task = utf8_arg(args, 2, "task")?;
                require_len(args, 3)?;
                let token = projection_id().map_err(|error| error.to_string())?;
                create_journal(&state, task, &token).map_err(|error| error.to_string())?;
                print!("{token}");
                Ok(0)
            }
            "journal-snapshot" => {
                let path = PathBuf::from(args.get(1).ok_or_else(|| "missing journal".to_owned())?);
                let task = utf8_arg(args, 2, "task")?;
                require_len(args, 3)?;
                match read_journal(&path, task).map_err(|error| error.to_string())? {
                    ProjectionJournal::V1 {
                        task_id,
                        projection_id,
                    } => print!("1\t{task_id}\t{projection_id}"),
                    ProjectionJournal::V2(binding) => print!(
                        "2\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
                    ),
                }
                Ok(0)
            }
            "journal-bind" => {
                require_len(args, 12)?;
                let path = PathBuf::from(&args[1]);
                let binding = ProjectionBinding {
                    task_id: utf8_arg(args, 2, "task")?.to_owned(),
                    projection_id: read_journal(&path, utf8_arg(args, 2, "task")?)
                        .map_err(|error| error.to_string())?
                        .projection_id()
                        .to_owned(),
                    home: home_identity(Path::new(
                        args.get(3).ok_or_else(|| "missing home".to_owned())?,
                    ))
                    .map_err(|error| error.to_string())?,
                    session: utf8_arg(args, 4, "session")?.to_owned(),
                    workspace_id: utf8_arg(args, 5, "workspace")?.to_owned(),
                    tab_id: utf8_arg(args, 6, "tab")?.to_owned(),
                    pane_id: utf8_arg(args, 7, "pane")?.to_owned(),
                    parent_workspace_id: utf8_arg(args, 8, "parent workspace")?.to_owned(),
                    parent_label: utf8_arg(args, 9, "parent label")?.to_owned(),
                    workspace_label: utf8_arg(args, 10, "workspace label")?.to_owned(),
                    task_label: utf8_arg(args, 11, "task label")?.to_owned(),
                };
                bind_journal(&path, binding).map_err(|error| error.to_string())?;
                Ok(0)
            }
            "journal-write-v2" => {
                require_len(args, 13)?;
                write_journal_v2(
                    Path::new(&args[1]),
                    ProjectionBinding {
                        task_id: utf8_arg(args, 2, "task")?.to_owned(),
                        projection_id: utf8_arg(args, 3, "projection id")?.to_owned(),
                        home: home_identity(Path::new(
                            args.get(4).ok_or_else(|| "missing home".to_owned())?,
                        ))
                        .map_err(|error| error.to_string())?,
                        session: utf8_arg(args, 5, "session")?.to_owned(),
                        workspace_id: utf8_arg(args, 6, "workspace")?.to_owned(),
                        tab_id: utf8_arg(args, 7, "tab")?.to_owned(),
                        pane_id: utf8_arg(args, 8, "pane")?.to_owned(),
                        parent_workspace_id: utf8_arg(args, 9, "parent workspace")?.to_owned(),
                        parent_label: utf8_arg(args, 10, "parent label")?.to_owned(),
                        workspace_label: utf8_arg(args, 11, "workspace label")?.to_owned(),
                        task_label: utf8_arg(args, 12, "task label")?.to_owned(),
                    },
                )
                .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "journal-replace" => {
                require_len(args, 7)?;
                replace_journal_endpoint(
                    Path::new(&args[1]),
                    utf8_arg(args, 2, "task")?,
                    utf8_arg(args, 3, "old tab")?,
                    utf8_arg(args, 4, "old pane")?,
                    utf8_arg(args, 5, "new tab")?,
                    utf8_arg(args, 6, "new pane")?,
                )
                .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "home-identity" => {
                require_len(args, 2)?;
                print!(
                    "{}",
                    home_identity(Path::new(&args[1]))
                        .map_err(|error| error.to_string())?
                        .display()
                );
                Ok(0)
            }
            "focus-snapshot" => {
                let session = utf8_arg(args, 1, "session")?;
                require_len(args, 2)?;
                print!(
                    "{}",
                    backend
                        .focus_snapshot(session)
                        .map_err(|error| error.to_string())?
                        .render()
                );
                Ok(0)
            }
            "focus-restore" => {
                let session = utf8_arg(args, 1, "session")?;
                let raw = utf8_arg(args, 2, "snapshot")?;
                require_len(args, 3)?;
                let (workspace_id, tab_id) = raw
                    .split_once('\t')
                    .ok_or_else(|| "malformed focus snapshot".to_owned())?;
                backend
                    .focus_restore(
                        session,
                        &FocusSnapshot {
                            workspace_id: workspace_id.to_owned(),
                            tab_id: tab_id.to_owned(),
                        },
                    )
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "close-pane-focus" => {
                let session = utf8_arg(args, 1, "session")?;
                let pane = utf8_arg(args, 2, "pane")?;
                let required = args
                    .get(3)
                    .and_then(|value| value.to_str())
                    .filter(|value| !value.is_empty())
                    .map(|value| match value {
                        "dead" => Ok(PaneAgentState::Dead),
                        "no-agent" => Ok(PaneAgentState::NoAgent),
                        "live" => Ok(PaneAgentState::Live),
                        "unknown" => Ok(PaneAgentState::Unknown),
                        _ => Err("invalid pane state".to_owned()),
                    })
                    .transpose()?;
                require_len(args, if args.get(3).is_some() { 4 } else { 3 })?;
                backend
                    .close_pane_focus_preserving(session, pane, required)
                    .map_err(|error| error.to_string())?;
                Ok(0)
            }
            "presentation-lock-path" => {
                let session = utf8_arg(args, 1, "session")?;
                require_len(args, 2)?;
                print!(
                    "{}",
                    backend
                        .presentation_session_lock_path(session)
                        .map_err(|error| error.to_string())?
                        .display()
                );
                Ok(0)
            }
            "presentation-socket-path" => {
                let session = utf8_arg(args, 1, "session")?;
                require_len(args, 2)?;
                print!(
                    "{}",
                    backend
                        .presentation_session_socket_path(session)
                        .map_err(|error| error.to_string())?
                        .display()
                );
                Ok(0)
            }
            "parent-workspace" => {
                let session = utf8_arg(args, 1, "session")?;
                let label = utf8_arg(args, 2, "label")?;
                require_len(args, 3)?;
                print!(
                    "{}",
                    backend
                        .parent_workspace_exact(session, label)
                        .map_err(|error| error.to_string())?
                );
                Ok(0)
            }
            "projection-create" => {
                require_len(args, 4)?;
                let endpoint = backend
                    .projection_create_task(
                        Path::new(&args[1]),
                        utf8_arg(args, 2, "workspace label")?,
                        utf8_arg(args, 3, "task label")?,
                    )
                    .map_err(|error| error.to_string())?;
                print!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    endpoint.session,
                    endpoint.workspace_id,
                    endpoint.seeded_tab_id,
                    endpoint.seeded_pane_id,
                    endpoint.tab_id,
                    endpoint.pane_id
                );
                Ok(0)
            }
            "projection-order" => {
                let session = utf8_arg(args, 1, "session")?;
                let workspace = utf8_arg(args, 2, "workspace")?;
                let parent = utf8_arg(args, 3, "parent")?;
                require_len(args, 4)?;
                // Best effort by contract: stable warnings belong to the shell adapter.
                Ok(
                    if backend
                        .order_projection_best_effort(session, workspace, parent)
                        .is_ok()
                    {
                        0
                    } else {
                        1
                    },
                )
            }
            "projection-live-binding" => {
                require_len(args, 10)?;
                let session = utf8_arg(args, 1, "session")?;
                let binding = ProjectionBinding {
                    task_id: "compat".to_owned(),
                    projection_id: utf8_arg(args, 2, "token")?.to_owned(),
                    home: PathBuf::from("/"),
                    session: session.to_owned(),
                    workspace_id: utf8_arg(args, 3, "workspace")?.to_owned(),
                    tab_id: utf8_arg(args, 4, "tab")?.to_owned(),
                    pane_id: utf8_arg(args, 5, "pane")?.to_owned(),
                    parent_workspace_id: utf8_arg(args, 6, "parent workspace")?.to_owned(),
                    parent_label: utf8_arg(args, 7, "parent label")?.to_owned(),
                    workspace_label: utf8_arg(args, 8, "workspace label")?.to_owned(),
                    task_label: utf8_arg(args, 9, "task label")?.to_owned(),
                };
                Ok(
                    if backend.projection_live_binding_matches(session, &binding) {
                        0
                    } else {
                        1
                    },
                )
            }
            "projection-recovery-allows-flat" => {
                require_len(args, 4)?;
                Ok(
                    if backend.projection_recovery_allows_flat(
                        utf8_arg(args, 1, "session")?,
                        Path::new(&args[2]),
                        utf8_arg(args, 3, "task")?,
                    ) {
                        0
                    } else {
                        1
                    },
                )
            }
            "projection-endpoint-matches" => {
                require_len(args, 5)?;
                Ok(
                    if backend.projection_endpoint_matches_journal(
                        utf8_arg(args, 1, "session")?,
                        utf8_arg(args, 2, "workspace")?,
                        Path::new(&args[3]),
                        utf8_arg(args, 4, "task")?,
                    ) {
                        0
                    } else {
                        1
                    },
                )
            }
            "projection-reclaim" => {
                require_len(args, 11)?;
                match backend.projection_reclaim_task(
                    utf8_arg(args, 1, "session")?,
                    Path::new(&args[2]),
                    utf8_arg(args, 3, "task")?,
                    Path::new(&args[4]),
                    utf8_arg(args, 5, "workspace")?,
                    utf8_arg(args, 6, "tab")?,
                    utf8_arg(args, 7, "pane")?,
                    utf8_arg(args, 8, "parent label")?,
                    utf8_arg(args, 9, "task label")?,
                    Path::new(&args[10]),
                ) {
                    ReclaimOutcome::Reclaimed { tab_id, pane_id } => {
                        print!("{tab_id}\t{pane_id}");
                        Ok(0)
                    }
                    ReclaimOutcome::Flat => Ok(2),
                    ReclaimOutcome::Refuse => Ok(1),
                }
            }
            _ => Err(format!("unknown Herdr command: {command}")),
        }
    })();
    match result {
        Ok(status) => status,
        Err(error) => {
            eprintln!("mx herdr: {error}");
            1
        }
    }
}

fn utf8_arg<'a>(args: &'a [OsString], index: usize, name: &str) -> Result<&'a str, String> {
    args.get(index)
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("missing or non-UTF-8 {name}"))
}

fn require_len(args: &[OsString], expected: usize) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {} arguments, got {}",
            expected.saturating_sub(1),
            args.len().saturating_sub(1)
        ))
    }
}

fn run_peek(target: &str, lines: u32) -> i32 {
    use multplx_backend::facade::RuntimeBackend;

    let (_, home, _) = active_paths();
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    let mut backend = multplx_backend::tmux::TmuxBackend::system();
    let resolved = match multplx_backend::facade::resolve_selector(target, &state, &mut backend) {
        Ok(resolved) => resolved,
        Err(error) => return backend_error(error),
    };
    if resolved.backend() != multplx_backend::facade::BackendName::Tmux {
        return backend_error(format!(
            "backend {} remains on the legacy compatibility path",
            resolved.backend()
        ));
    }
    match backend.capture(&multplx_backend::facade::CaptureRequest {
        target: resolved,
        lines,
        byte_limit: 256 * 1024,
    }) {
        Ok(bytes) => match io::stdout().write_all(&bytes) {
            Ok(()) => 0,
            Err(error) => backend_error(error),
        },
        Err(error) => backend_error(error),
    }
}

fn run_actor_state(id: &str) -> i32 {
    let task = match multplx_core::identifiers::TaskId::parse(id) {
        Ok(task) => task,
        Err(_) => {
            eprintln!("usage: mx-actor-state.sh <id>");
            return 2;
        }
    };
    let (_, home, _) = active_paths();
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    let request = multplx_backend::actor_state::ActorStateRequest::from_environment(state, task);
    let mut backend = multplx_backend::tmux::TmuxBackend::system();
    let mut runner = multplx_backend::command::SystemCommandRunner;
    match multplx_backend::actor_state::reconcile(&request, &mut backend, &mut runner) {
        Ok(output) => {
            for warning in output.warnings {
                eprintln!("{warning}");
            }
            print!("{}", output.line);
            0
        }
        Err(error) => backend_error(error),
    }
}

fn environment_path(primary: &str, fallback: &str) -> PathBuf {
    std::env::var_os(primary)
        .or_else(|| std::env::var_os(fallback))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn active_paths() -> (PathBuf, PathBuf, PathBuf) {
    let root = environment_path("MX_ROOT_OVERRIDE", "MX_RUST_SOURCE_ROOT");
    let home = std::env::var_os("MX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.clone());
    let data = std::env::var_os("MX_DATA_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("data"));
    (root, home, data)
}

fn runtime_root(logical_root: &Path) -> PathBuf {
    std::env::var_os("MX_RUST_SOURCE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| logical_root.to_owned())
}

fn run_backlog(args: &[OsString]) -> i32 {
    let (_, _, data) = active_paths();
    match multplx_domain::backlog::run_cli(args, data.join("backlog.md")) {
        Ok(output) => {
            print!("{output}");
            0
        }
        Err(error) => {
            if error.usage {
                eprint!("{}", multplx_domain::backlog::USAGE);
            } else if !error.message.is_empty() {
                if error.message.starts_with("mx-backlog:") {
                    eprintln!("{}", error.message);
                } else {
                    eprintln!("mx-backlog: {}", error.message);
                }
            }
            error.code
        }
    }
}

fn run_backlog_handoff(id: &str, keys: &[String]) -> i32 {
    let (root, home, data) = active_paths();
    match multplx_domain::handoff::run(&root, &home, &data, id, keys) {
        Ok(output) => {
            print!("{output}");
            0
        }
        Err(error) => {
            eprintln!("{}", error.message);
            1
        }
    }
}

fn run_project_mode(name: &str) -> i32 {
    let (_, _, data) = active_paths();
    let resolution = multplx_domain::project_registry::resolve(&data.join("projects.md"), name);
    if let Some(warning) = resolution.warning.as_deref() {
        eprintln!("{warning}");
    }
    print!("{}", resolution.render());
    0
}

const OPERATIONAL_USAGE: &str = "Usage:\n  bin/mx-operational-input.sh encode <kind>  # body on stdin\n  bin/mx-operational-input.sh kind           # current input on stdin\n  bin/mx-operational-input.sh classify       # current or legacy input on stdin\n  bin/mx-operational-input.sh body           # current input on stdin\n\nCurrent construction kinds:\n  session-start watcher turn-end-guard away-supervisor from-broker launch-brief\n\nThe from-broker kind uses its established live-charter-compatible carrier.\n";

fn run_operational_input(args: &[OsString]) -> i32 {
    let values = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let Some(command) = values.first().map(String::as_str) else {
        eprint!("{OPERATIONAL_USAGE}");
        return 2;
    };
    if matches!(command, "-h" | "--help" | "help") {
        print!("{OPERATIONAL_USAGE}");
        return 0;
    }
    let valid_shape = matches!(
        (command, values.len()),
        ("encode", 2) | ("kind" | "classify" | "body", 1)
    );
    if !valid_shape {
        eprint!("{OPERATIONAL_USAGE}");
        return 2;
    }
    let input = match read_stdin() {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => return 2,
    };
    use multplx_domain::operational_input as protocol;
    match (command, values.get(1), values.len()) {
        ("encode", Some(kind), 2) => {
            let Some(kind) = protocol::Kind::parse(kind) else {
                return 2;
            };
            let Some(output) = protocol::construct(kind, &input) else {
                return 2;
            };
            print!("{output}");
            0
        }
        ("kind", None, 1) => protocol::current_kind(&input).map_or(1, |kind| {
            println!("{kind}");
            0
        }),
        ("classify", None, 1) => protocol::classify(&input).map_or(1, |kind| {
            println!("{kind}");
            0
        }),
        ("body", None, 1) => protocol::body(&input).map_or(1, |body| {
            print!("{body}");
            0
        }),
        _ => {
            eprint!("{OPERATIONAL_USAGE}");
            2
        }
    }
}

fn inherit_outcome(outcome: multplx_domain::inheritance::Outcome) -> i32 {
    if let Some(report) = std::env::var_os("MX_CONFIG_INHERIT_REPORT") {
        outcome.append_report(Some(Path::new(&report)));
    }
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    i32::from(outcome.failed)
}

fn run_config_inherit(args: &[OsString]) -> i32 {
    use multplx_domain::inheritance as inherit;
    let values = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let value = |index: usize| values.get(index).map(PathBuf::from);
    let Some(command) = values.first().map(String::as_str) else {
        return 2;
    };
    let result: Result<i32, String> = match command {
        "file-mode" if values.len() == 2 => std::fs::metadata(&values[1])
            .map(|metadata| {
                use std::os::unix::fs::PermissionsExt;
                println!("{:o}", metadata.permissions().mode() & 0o7777);
                0
            })
            .map_err(|error| error.to_string()),
        "file-device" if values.len() == 2 => std::fs::metadata(&values[1])
            .map(|metadata| {
                use std::os::unix::fs::MetadataExt;
                println!("{}", metadata.dev());
                0
            })
            .map_err(|error| error.to_string()),
        "file-links" if values.len() == 2 => std::fs::metadata(&values[1])
            .map(|metadata| {
                use std::os::unix::fs::MetadataExt;
                println!("{}", metadata.nlink());
                0
            })
            .map_err(|error| error.to_string()),
        "sha256" if values.len() == 2 => inherit::sha256(Path::new(&values[1]))
            .map(|hash| {
                println!("{hash}");
                0
            })
            .map_err(|error| error.to_string()),
        "propagate-config" if values.len() == 3 => {
            inherit::propagate_config(Path::new(&values[1]), Path::new(&values[2]))
                .map(inherit_outcome)
                .map_err(|error| error.to_string())
        }
        "propagate-shared" if values.len() == 3 => Ok(inherit_outcome(inherit::propagate_shared(
            Path::new(&values[1]),
            Path::new(&values[2]),
        ))),
        "propagate-daemon" if (3..=5).contains(&values.len()) => inherit::propagate_daemon(
            Path::new(&values[1]),
            Path::new(&values[2]),
            value(3).as_deref(),
            value(4).as_deref(),
        )
        .map(inherit_outcome)
        .map_err(|error| error.to_string()),
        "changed-items" if values.len() == 2 => inherit::changed_items(Path::new(&values[1]))
            .map(|items| {
                for item in items {
                    println!("{item}");
                }
                0
            })
            .map_err(|error| error.to_string()),
        "lock-path" if values.len() == 2 => {
            println!("{}", inherit::inherit_lock(Path::new(&values[1])).display());
            Ok(0)
        }
        "retry-dir" if values.len() == 3 => {
            println!(
                "{}",
                inherit::retry_dir(Path::new(&values[1]), &values[2]).display()
            );
            Ok(0)
        }
        "pending-stages" if values.len() == 3 => {
            for path in inherit::pending_stages(Path::new(&values[1]), &values[2]) {
                println!("{}", path.display());
            }
            Ok(0)
        }
        "pending-reports" if values.len() == 3 => {
            for path in inherit::pending_reports(Path::new(&values[1]), &values[2]) {
                println!("{}", path.display());
            }
            Ok(0)
        }
        "has-staged" if values.len() == 3 => Ok(i32::from(
            inherit::pending_stages(Path::new(&values[1]), &values[2]).is_empty()
                && inherit::pending_reports(Path::new(&values[1]), &values[2]).is_empty(),
        )),
        "queue-full" if values.len() == 3 => Ok(i32::from(!inherit::retry_queue_full(
            Path::new(&values[1]),
            &values[2],
        ))),
        "new-stage" if values.len() == 3 => inherit::next_stage(Path::new(&values[1]), &values[2])
            .map(|path| {
                println!("{}", path.display());
                0
            })
            .map_err(|error| error.to_string()),
        "save-report" if values.len() == 3 => {
            inherit::save_retry_report(Path::new(&values[1]), Path::new(&values[2]))
                .map(|path| {
                    println!("{}", path.display());
                    0
                })
                .map_err(|error| error.to_string())
        }
        "write-instruction" if values.len() == 4 => inherit::write_reread_instruction(
            Path::new(&values[1]),
            Path::new(&values[2]),
            Path::new(&values[3]),
        )
        .map(|written| i32::from(!written))
        .map_err(|error| error.to_string()),
        "mark-pending" if values.len() == 3 => {
            inherit::mark_pending_at(Path::new(&values[1]), Path::new(&values[2]))
                .map(|()| 0)
                .map_err(|error| error.to_string())
        }
        "publish-stage" if values.len() == 3 => {
            inherit::publish_stage(Path::new(&values[1]), Path::new(&values[2]))
                .map(|path| {
                    println!("{}", path.display());
                    0
                })
                .map_err(|error| error.to_string())
        }
        "has-pending" if values.len() == 2 => {
            Ok(i32::from(!inherit::has_pending(Path::new(&values[1]))))
        }
        "pending-instructions" if values.len() == 2 => {
            for path in inherit::pending_instructions(Path::new(&values[1])) {
                println!("{}", path.display());
            }
            Ok(0)
        }
        "cleanup-sent" if values.len() == 2 => {
            inherit::cleanup_sent(Path::new(&values[1]));
            Ok(0)
        }
        "discard-pending" if (2..=4).contains(&values.len()) => {
            Ok(i32::from(!inherit::discard_pending(
                Path::new(&values[1]),
                values.get(2).map(String::as_str),
                values.get(3).map(Path::new),
            )))
        }
        "quarantine-pending" if (2..=4).contains(&values.len()) => {
            Ok(i32::from(!inherit::quarantine_pending(
                Path::new(&values[1]),
                values.get(2).map(String::as_str),
                values.get(3).map(Path::new),
            )))
        }
        "send-reread" if values.len() == 4 => {
            let (root, source_home, _) = active_paths();
            let send_root = runtime_root(&root);
            let state = std::env::var_os("MX_STATE_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| source_home.join("state"));
            let context = inherit::RereadContext {
                id: &values[1],
                destination_home: Path::new(&values[2]),
                report: Path::new(&values[3]),
                source_home: &source_home,
                root: &send_root,
                state: &state,
                skip_pending: std::env::var("MX_CONFIG_REREAD_SKIP_PENDING").as_deref() == Ok("1"),
            };
            let (ok, output) = inherit::send_reread(&context);
            print!("{output}");
            Ok(i32::from(!ok))
        }
        "retry-pending" if values.len() == 3 => {
            let report = tempfile::NamedTempFile::new().map_err(|error| error.to_string());
            report.map(|report| {
                let mut call = vec![OsString::from("send-reread")];
                call.push(values[1].clone().into());
                call.push(values[2].clone().into());
                call.push(report.path().as_os_str().to_owned());
                run_config_inherit(&call)
            })
        }
        _ => Ok(2),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

const CONFIG_PUSH_USAGE: &str = "Usage: mx-config-push.sh [--help]\n\nPush the primary Multplx home's declared inherited local material into each\nlive daemon home.\n\nThis is local-material-only:\n  - does not fast-forward tracked files\n  - after successful config/* changes, writes a generation-specific\n    literal-content reread instruction and sends its pointer to that live daemon\n    (no message when config is unchanged unless a previous send failure is pending)\n  - reports each live home and each inheritable item as pushed, unchanged,\n    skipped, or error\n  - exits non-zero for real propagation errors or reread-send failures\n\nLive homes come from state/*.meta records with kind=daemon.\ndata/daemons.md is only a fallback for missing home= fields in older or\nincomplete meta records.\n\nEnvironment overrides follow the rest of broker:\n  MX_HOME            active Multplx home\n  MX_ROOT_OVERRIDE  Multplx repo root\n  MX_STATE_OVERRIDE state dir\n  MX_DATA_OVERRIDE  data dir\n  MX_CONFIG_OVERRIDE config dir\n";

fn last_field(text: &str, key: &str) -> String {
    text.lines()
        .filter_map(|line| line.strip_prefix(&format!("{key}=")))
        .next_back()
        .unwrap_or("")
        .to_owned()
}

fn registry_home(path: &Path, id: &str) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let Some(line) = text
        .lines()
        .rfind(|line| *line == format!("- {id}") || line.starts_with(&format!("- {id} ")))
    else {
        return String::new();
    };
    let Some(start) = line.rfind("(home:") else {
        return String::new();
    };
    line[start + 6..]
        .trim_start()
        .split_once(';')
        .map_or("", |(home, _)| home.trim_end())
        .to_owned()
}

fn live_daemons(state: &Path, registry: &Path) -> Vec<(String, String, PathBuf)> {
    let mut metadata = std::fs::read_dir(state)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(OsStr::to_str) == Some("meta") && path.is_file())
        .collect::<Vec<_>>();
    metadata.sort();
    metadata
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            if !text.lines().any(|line| line == "kind=daemon") {
                return None;
            }
            let id = path.file_stem()?.to_string_lossy().into_owned();
            let mut home = last_field(&text, "home");
            if home.is_empty() {
                home = registry_home(registry, &id);
            }
            Some((id, home, path))
        })
        .collect()
}

fn run_config_push(args: &[OsString]) -> i32 {
    let values = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>();
    if values
        .first()
        .is_some_and(|value| matches!(value.as_ref(), "-h" | "--help"))
    {
        print!("{CONFIG_PUSH_USAGE}");
        return 0;
    }
    if !values.is_empty() {
        eprintln!("usage: mx-config-push.sh [--help]");
        return 2;
    }
    use multplx_domain::inheritance as inherit;
    let (root, home, data) = active_paths();
    let send_root = runtime_root(&root);
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    let config = std::env::var_os("MX_CONFIG_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("config"));
    let records = live_daemons(&state, &data.join("daemons.md"));
    if records.is_empty() {
        println!("config-push: no live daemon homes found");
        return 0;
    }
    println!("config-push: {} -> live daemon homes", home.display());
    let mut seen = std::collections::BTreeSet::new();
    let mut failed = false;
    for (id, raw_home, metadata) in records {
        if raw_home.is_empty() {
            println!(
                "daemon {id}: skipped - no home= in {} and no registry home",
                metadata.display()
            );
            continue;
        }
        let validated = match inherit::validate_daemon_home(&id, Path::new(&raw_home), &home, &root)
        {
            Ok(home) => home,
            Err(error) => {
                println!("daemon {id} ({raw_home}): skipped - unsafe home: {error}");
                continue;
            }
        };
        let target = validated.path;
        if !seen.insert(target.clone()) {
            println!(
                "daemon {id} ({}): skipped - already processed for another live meta",
                target.display()
            );
            continue;
        }
        println!("daemon {id} ({}):", target.display());
        if std::process::Command::new("git")
            .args([
                "-C",
                target.to_string_lossy().as_ref(),
                "status",
                "--porcelain",
            ])
            .output()
            .is_ok_and(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| line != "?? .mx-daemon-home")
            })
        {
            println!("  home: dirty working tree - local-material push continuing");
        }
        let _lock = match inherit::acquire_inherit_lock(&target) {
            Ok(lock) => lock,
            Err(_) => {
                println!("  config-reread: error - could not acquire per-home lock");
                failed = true;
                continue;
            }
        };
        if inherit::retry_queue_full(&home, &id) {
            let retry = tempfile::NamedTempFile::new();
            if let Ok(retry) = retry {
                let context = inherit::RereadContext {
                    id: &id,
                    destination_home: &target,
                    report: retry.path(),
                    source_home: &home,
                    root: &send_root,
                    state: &state,
                    skip_pending: false,
                };
                let (_, output) = inherit::send_reread(&context);
                print!("{output}");
            }
            if inherit::retry_queue_full(&home, &id) {
                println!("  config-reread: error - retry instruction queue is full");
                failed = true;
                continue;
            }
        }
        let report = match tempfile::NamedTempFile::new() {
            Ok(report) => report,
            Err(_) => {
                println!("  home: error - could not create report file");
                failed = true;
                continue;
            }
        };
        let outcome = match inherit::propagate_daemon(&home, &target, Some(&config), Some(&data)) {
            Ok(outcome) => outcome,
            Err(error) => {
                println!("  home: error - {error}");
                failed = true;
                continue;
            }
        };
        outcome.append_report(Some(report.path()));
        print!("{}", outcome.stdout);
        eprint!("{}", outcome.stderr);
        failed |= outcome.failed;
        for row in &outcome.rows {
            if row.reason.is_empty() {
                println!("  {}: {}", row.item, row.status.as_str());
            } else {
                println!("  {}: {} - {}", row.item, row.status.as_str(), row.reason);
            }
        }
        let pending = inherit::has_pending(&target)
            || !inherit::pending_stages(&home, &id).is_empty()
            || !inherit::pending_reports(&home, &id).is_empty();
        let changed = inherit::changed_items(report.path()).unwrap_or_default();
        let context = inherit::RereadContext {
            id: &id,
            destination_home: &target,
            report: report.path(),
            source_home: &home,
            root: &send_root,
            state: &state,
            skip_pending: false,
        };
        let (sent, output) = inherit::send_reread(&context);
        if sent && (!changed.is_empty() || pending) {
            println!("  config-reread: sent");
        }
        print!("{output}");
        if !sent {
            failed = true;
            if output.is_empty() {
                println!("  config-reread: send failed");
            }
        }
    }
    i32::from(failed)
}

fn read_stdin() -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn supervisor_environment() -> multplx_core::supervisor_target::SupervisorEnvironment {
    let nonempty = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    multplx_core::supervisor_target::SupervisorEnvironment {
        target: nonempty("MX_SUPERVISOR_TARGET"),
        backend: nonempty("MX_SUPERVISOR_BACKEND"),
        tmux_pane: nonempty("TMUX_PANE"),
        herdr_environment: std::env::var("HERDR_ENV").as_deref() == Ok("1"),
        herdr_pane_id: nonempty("HERDR_PANE_ID"),
        herdr_session: nonempty("HERDR_SESSION"),
    }
}

fn run_primitive(command: PrimitiveCommand) -> Result<i32, String> {
    use multplx_core::classification::{Heuristic, NativeState, RunStep};

    match command {
        PrimitiveCommand::BackendHomeTag { root, home } => {
            print!(
                "{}",
                multplx_core::backend_hometag::home_tag(root, home)
                    .map_err(|error| error.to_string())?
            );
        }
        PrimitiveCommand::TaskId { value } => {
            print!(
                "{}",
                multplx_core::identifiers::TaskId::parse(value)
                    .map_err(|error| error.to_string())?
            );
        }
        PrimitiveCommand::AtomicReplace { path, mode } => {
            let mode = u32::from_str_radix(mode.trim_start_matches("0o"), 8)
                .map_err(|_| "mode must be octal".to_owned())?;
            let bytes = read_stdin()?;
            multplx_core::filesystem::atomic_replace(path, &bytes, mode)
                .map_err(|error| error.to_string())?;
        }
        PrimitiveCommand::ProcessIdentity { pid } => {
            let identity =
                multplx_core::process::identity(pid).map_err(|error| error.to_string())?;
            println!("{}", identity.marker);
        }
        PrimitiveCommand::CheckRegistered { state, task } => {
            let task = multplx_core::identifiers::TaskId::parse(task)
                .map_err(|error| error.to_string())?;
            if !multplx_core::checks::registered(state, &task).map_err(|error| error.to_string())? {
                return Ok(1);
            }
        }
        PrimitiveCommand::ComposerClassify {
            bordered,
            content,
            idle_regex,
            insensitive,
            plain_content,
        } => {
            let state = multplx_core::composer::classify_content(
                bordered == "1" || bordered == "true",
                &content,
                idle_regex.as_deref(),
                insensitive,
                plain_content.as_deref(),
            )
            .map_err(|error| error.to_string())?;
            print!("{}", state.as_str());
        }
        PrimitiveCommand::ComposerStripAnsi => {
            let input = read_stdin()?;
            let mut output = multplx_core::composer::strip_ansi(&input);
            if !input.is_empty() && !output.ends_with(b"\n") {
                output.push(b'\n');
            }
            io::Write::write_all(&mut io::stdout(), &output).map_err(|error| error.to_string())?;
        }
        PrimitiveCommand::ComposerStripGhost { luma_max } => {
            let input = read_stdin()?;
            let mut output = multplx_core::composer::strip_ghost(&input, luma_max);
            if !input.is_empty() && !output.ends_with(b"\n") {
                output.push(b'\n');
            }
            io::Write::write_all(&mut io::stdout(), &output).map_err(|error| error.to_string())?;
        }
        PrimitiveCommand::SignalResolve {
            native,
            run_step,
            self_report,
            heuristic,
        } => {
            print!(
                "{}",
                multplx_core::classification::resolve_signal(
                    NativeState::parse(&native),
                    RunStep::parse(&run_step),
                    &self_report,
                    Heuristic::parse(&heuristic),
                    "paused",
                )
            );
        }
        PrimitiveCommand::StatusOpenDecisions { path } => {
            let bytes = multplx_core::filesystem::read_bounded_regular(
                path,
                multplx_core::classification::STATUS_READ_LIMIT,
            )
            .map_err(|error| error.to_string())?;
            let text = String::from_utf8(bytes).map_err(|_| "status is not UTF-8".to_owned())?;
            print!(
                "{}",
                multplx_core::classification::render_open_statuses(
                    &multplx_core::classification::open_decisions(
                        &text,
                        "resolved",
                        "maintainer-held"
                    )
                )
            );
        }
        PrimitiveCommand::GateRefuse => {
            let gate = std::env::var_os("DEEP_REVIEW_GATE").is_some();
            let bypass = std::env::var("MX_GATE_REFUSE_BYPASS").as_deref() == Ok("1");
            if multplx_core::gate_refuse::is_gate_agent(gate, bypass) {
                eprintln!("{}", multplx_core::gate_refuse::REFUSAL_MESSAGE);
                return Ok(i32::from(multplx_core::gate_refuse::REFUSAL_EXIT));
            }
        }
        PrimitiveCommand::JournalEmit {
            state,
            task,
            event,
            detail,
            source,
            timestamp,
        } => {
            let writer = multplx_core::journal::JournalWriter::new(state);
            let task = multplx_core::identifiers::TaskId::parse(task)
                .map_err(|error| error.to_string())?;
            let event = multplx_core::journal::JournalEvent::parse(&event)
                .map_err(|error| error.to_string())?;
            let detail = serde_json::from_str(&detail).map_err(|error| error.to_string())?;
            writer
                .emit(&task, event, &detail, &source, &timestamp)
                .map_err(|error| error.to_string())?;
        }
        PrimitiveCommand::GitLockStale {
            lock,
            companion,
            minimum_age,
            now_epoch,
        } => {
            let stale = multplx_core::locks::git_lock_is_provably_stale(
                lock,
                companion.as_deref(),
                Duration::from_secs(minimum_age),
                UNIX_EPOCH + Duration::from_secs(now_epoch),
                &multplx_core::locks::LsofProbe,
            )
            .map_err(|error| error.to_string())?;
            if !stale {
                return Ok(1);
            }
        }
        PrimitiveCommand::MarkerMark => {
            let input =
                String::from_utf8(read_stdin()?).map_err(|_| "message is not UTF-8".to_owned())?;
            print!("{}", multplx_core::marker::mark_from_broker(&input));
        }
        PrimitiveCommand::MarkerIs => {
            let input =
                String::from_utf8(read_stdin()?).map_err(|_| "message is not UTF-8".to_owned())?;
            if !multplx_core::marker::is_from_broker(&input) {
                return Ok(1);
            }
        }
        PrimitiveCommand::PrimaryScope { root, state } => {
            if !multplx_core::primary_scope::matches(root, state) {
                return Ok(1);
            }
        }
        PrimitiveCommand::ProbeInstall { tool } => {
            let command = multplx_core::probe::install_command(&tool)
                .or_else(|| multplx_core::probe::manual_install_url(&tool))
                .ok_or_else(|| format!("unknown tool: {tool}"))?;
            println!("{command}");
        }
        PrimitiveCommand::SessionLockStatus { path } => {
            let status = multplx_core::session_lock::status(
                path,
                &SystemProcessProbe::default(),
                &multplx_core::session_lock::harness_regex(),
            );
            println!("{}", status.render());
        }
        PrimitiveCommand::SupervisionStatus {
            state,
            grace,
            now_epoch,
        } => {
            let status = multplx_core::supervision::inspect(
                state,
                Duration::from_secs(grace),
                UNIX_EPOCH + Duration::from_secs(now_epoch),
            );
            println!(
                "{}\t{}\t{}\t{}\t{}",
                status.in_flight,
                status.needed,
                status.watcher_fresh,
                status.beacon_description,
                status.queue_pending
            );
        }
        PrimitiveCommand::SupervisorTarget => {
            let discovery = multplx_core::supervisor_target::target(&supervisor_environment());
            print!("{}", discovery.value);
            if !discovery.detected {
                return Ok(1);
            }
        }
        PrimitiveCommand::SupervisorBackend => {
            let discovery = multplx_core::supervisor_target::backend(&supervisor_environment());
            print!("{}", discovery.value);
            if !discovery.detected {
                return Ok(1);
            }
        }
        PrimitiveCommand::Tangle { root } => {
            if let Some(branch) = multplx_core::tangle::primary_tangle_branch(root)
                .map_err(|error| error.to_string())?
            {
                println!("{branch}");
            } else {
                return Ok(1);
            }
        }
        PrimitiveCommand::TransitionRecord {
            pane_id,
            workspace_id,
            from_status,
            to_status,
            agent,
        } => {
            print!(
                "{}",
                multplx_core::transition::TransitionRecord::new(
                    &pane_id,
                    &workspace_id,
                    &from_status,
                    &to_status,
                    &agent
                )
                .render()
            );
        }
        PrimitiveCommand::TransitionPolicy { to_status } => {
            print!("{}", multplx_core::transition::policy(&to_status).as_str());
        }
        PrimitiveCommand::WakeAppend {
            state,
            kind,
            key,
            payload,
            epoch,
        } => {
            let queue = multplx_core::wake::WakeQueue::new(state);
            queue
                .append(
                    multplx_core::wake::WakeKind::parse(&kind)
                        .map_err(|error| error.to_string())?,
                    &key,
                    &payload,
                    UNIX_EPOCH + Duration::from_secs(epoch),
                    &SystemProcessProbe::default(),
                )
                .map_err(|error| error.to_string())?;
        }
        PrimitiveCommand::WakeDedupe { path } => {
            let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
            let records = text
                .lines()
                .map(multplx_core::wake::WakeRecord::parse)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            for record in multplx_core::wake::dedupe(&records) {
                print!("{}", record.render());
            }
        }
    }
    Ok(0)
}

fn multicall_alias(program: &OsStr) -> Option<OsString> {
    let file_name = Path::new(program).file_name()?.to_str()?;
    file_name.strip_prefix("mx-").map(OsString::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn run_git(dir: &Path, values: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(values)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn extracts_multicall_alias_from_executable_name() {
        assert_eq!(
            multicall_alias(OsStr::new("/tmp/mx-shadow-diagnostic")),
            Some(OsString::from("shadow-diagnostic"))
        );
    }

    #[test]
    fn leaves_canonical_binary_without_an_alias() {
        assert_eq!(multicall_alias(OsStr::new("/tmp/mx")), None);
        assert!(parse_seconds("-1").is_err());
        assert!(parse_seconds("NaN").is_err());
    }

    #[test]
    fn lifecycle_command_helpers_cover_success_and_refusal_contracts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let status = temp.path().join("state/daemon.status");
        assert_eq!(run_daemon_report(&args(&[])), 2);
        assert_eq!(
            run_daemon_report(&args(&[status.to_str().unwrap(), "done", "bad", "note"])),
            1
        );
        assert_eq!(
            run_daemon_report(&args(&[
                status.to_str().unwrap(),
                "done",
                "corr=0123456789abcdef",
                "all",
                "good"
            ])),
            0
        );
        assert_eq!(
            run_daemon_report(&args(&[
                "--doc",
                status.to_str().unwrap(),
                "done",
                "0123456789abcdef",
                "data/report.md",
                "details"
            ])),
            0
        );
        let report = fs::read_to_string(&status).expect("status");
        assert!(report.contains("all good (via-helper)"));
        assert!(report.contains("details (data/report.md via-helper)"));

        let repo = temp.path().join("repo");
        fs::create_dir(&repo).expect("repo");
        run_git(&repo, &["init", "-b", "main", "--quiet"]);
        fs::write(repo.join("file"), "x").expect("file");
        run_git(&repo, &["add", "."]);
        run_git(
            &repo,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "base",
                "--quiet",
            ],
        );
        let head = run_git(&repo, &["rev-parse", "HEAD"]);
        assert_eq!(run_fast_forward(&args(&[])), 2);
        assert_eq!(
            run_fast_forward(&args(&["default-branch", repo.to_str().unwrap()])),
            0
        );
        assert_eq!(
            run_fast_forward(&args(&["primary-head", repo.to_str().unwrap()])),
            0
        );
        assert_eq!(
            run_fast_forward(&args(&[
                "target",
                repo.to_str().unwrap(),
                "repo",
                &head,
                "no",
                "no"
            ])),
            0
        );
        fs::write(repo.join("file"), "two").expect("second file");
        run_git(&repo, &["add", "."]);
        run_git(
            &repo,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "second",
                "--quiet",
            ],
        );
        let second = run_git(&repo, &["rev-parse", "HEAD"]);
        run_git(&repo, &["reset", "--hard", &head, "--quiet"]);
        assert_eq!(
            run_fast_forward(&args(&[
                "target",
                repo.to_str().unwrap(),
                "repo",
                &second,
                "no",
                "no"
            ])),
            0
        );
        assert_eq!(run_fast_forward(&args(&["unknown"])), 2);

        let root = temp.path().join("root");
        let home = temp.path().join("home");
        let daemon = temp.path().join("daemon");
        for dir in [&root, &home, &daemon] {
            fs::create_dir(dir).expect("dir");
        }
        fs::create_dir(daemon.join("bin")).expect("bin");
        fs::write(daemon.join("AGENTS.md"), "x").expect("agents");
        fs::write(daemon.join(".mx-daemon-home"), "helper\n").expect("marker");
        assert_eq!(
            run_fast_forward(&args(&[
                "validate-home",
                root.to_str().unwrap(),
                home.to_str().unwrap(),
                "helper",
                daemon.to_str().unwrap()
            ])),
            0
        );
        assert_eq!(
            run_fast_forward(&args(&[
                "validate-home",
                root.to_str().unwrap(),
                home.to_str().unwrap(),
                "other",
                daemon.to_str().unwrap()
            ])),
            1
        );
    }

    #[test]
    fn pending_reply_command_covers_each_primitive() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        assert_eq!(run_pending_reply(&args(&[])), 2);
        assert_eq!(run_pending_reply(&args(&["extract", "nothing"])), 0);
        assert_eq!(
            run_pending_reply(&args(&["embed", "hello", "0123456789abcdef"])),
            0
        );
        assert_eq!(
            run_pending_reply(&args(&[
                "reusable",
                state.to_str().unwrap(),
                "0123456789abcdef",
                "task"
            ])),
            1
        );
        assert_eq!(
            run_pending_reply(&args(&[
                "create",
                temp.path().to_str().unwrap(),
                state.to_str().unwrap(),
                "task",
                "request"
            ])),
            0
        );
        let record = fs::read_dir(state.join("pending-replies"))
            .expect("records")
            .filter_map(Result::ok)
            .find(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .expect("record");
        let correlation = record.file_name().to_string_lossy().into_owned();
        assert_eq!(
            run_pending_reply(&args(&["prepare", state.to_str().unwrap(), &correlation])),
            0
        );
        assert_eq!(
            run_pending_reply(&args(&["confirm", state.to_str().unwrap(), &correlation])),
            0
        );
        assert_eq!(
            run_pending_reply(&args(&[
                "reusable",
                state.to_str().unwrap(),
                &correlation,
                "task"
            ])),
            0
        );
        assert_eq!(
            run_pending_reply(&args(&["prepare", state.to_str().unwrap(), "missing"])),
            1
        );

        let discard = multplx_domain::lifecycle::pending_reply::create(
            temp.path(),
            &state,
            "task",
            "discard",
        )
        .expect("discard record");
        assert_eq!(
            run_pending_reply(&args(&["discard", state.to_str().unwrap(), &discard])),
            0
        );
        assert_eq!(
            run_pending_reply(&args(&["discard", state.to_str().unwrap(), "missing"])),
            0
        );
    }

    #[test]
    fn send_resolution_is_strict_and_backend_dispatch_is_typed() {
        use multplx_backend::facade::{BackendName, BackendTarget};
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        assert!(
            send_resolve("plain", &state)
                .err()
                .expect("plain refusal")
                .contains("not resolvable")
        );
        assert!(
            send_resolve("mx-missing", &state)
                .err()
                .expect("metadata refusal")
                .contains("no metadata")
        );
        match send_resolve("definitely-missing:target", &state) {
            Ok(resolution) => assert_eq!(resolution.target.backend(), BackendName::Tmux),
            Err(error) => assert!(error.contains("not a live tmux")),
        }
        match send_resolve("definitely-missing:workspace:pane", &state) {
            Ok(resolution) => assert_eq!(resolution.target.backend(), BackendName::Herdr),
            Err(error) => assert!(error.contains("not a live herdr")),
        }

        fs::write(
            state.join("task.meta"),
            "kind=actor\nwindow=session:window\nbackend=tmux\nherdr_session=lab\nherdr_pane_id=workspace:pane\n",
        )
        .expect("meta");
        let selected = send_resolve("mx-task", &state).expect("selector");
        assert!(selected.selector);
        assert_eq!(selected.target.endpoint(), "session:window");
        assert!(
            send_resolve("workspace:pane", &state)
                .err()
                .expect("prefix refusal")
                .contains("missing its herdr session prefix")
        );
        assert_eq!(
            send_resolve("session:window", &state)
                .expect("target metadata")
                .target
                .backend(),
            BackendName::Tmux
        );

        for backend in [BackendName::Tmux, BackendName::Herdr, BackendName::Cmux] {
            let target = BackendTarget::new(backend, "definitely-missing:target:pane", None)
                .expect("target");
            let _ = send_target_ready(&target);
            let _ = send_key_to(&target, "Enter");
            let _ = send_text_to(&target, "text", 0, Duration::ZERO, Duration::ZERO);
        }
        assert_eq!(run_send(&[]), 1);

        assert_eq!(
            run_send_in_home(&[], temp.path().to_owned(), state.clone()),
            2
        );
        assert_eq!(
            run_send_in_home(
                &[OsString::from_vec(vec![0xff]), OsString::from("text")],
                temp.path().to_owned(),
                state.clone()
            ),
            1
        );
        assert_eq!(
            run_send_in_home(
                &args(&["missing", "text"]),
                temp.path().to_owned(),
                state.clone()
            ),
            1
        );
        assert_eq!(
            run_send_in_home(
                &args(&["mx-task", "--key"]),
                temp.path().to_owned(),
                state.clone()
            ),
            2
        );
        assert!(matches!(
            run_send_in_home(
                &args(&["mx-task", "--key", "Enter"]),
                temp.path().to_owned(),
                state.clone()
            ),
            0 | 1
        ));

        fs::write(
            state.join("daemon.meta"),
            "kind=daemon\nwindow=definitely-missing:daemon\nbackend=tmux\nharness=codex\n",
        )
        .expect("daemon meta");
        assert!(matches!(
            run_send_in_home(
                &args(&["mx-daemon", "$status", "now"]),
                temp.path().to_owned(),
                state
            ),
            0 | 1
        ));
    }

    #[test]
    fn top_level_lifecycle_dispatch_covers_safe_boundaries() {
        let temp = tempfile::tempdir().expect("tempdir");
        let memory = temp.path().join("memory");
        fs::create_dir(&memory).expect("memory");
        assert_eq!(
            Cli {
                command: Command::EnsureAgentsMd { directory: memory }
            }
            .run(),
            0
        );
        assert_eq!(
            Cli {
                command: Command::EnsureAgentsMd {
                    directory: temp.path().join("missing-memory")
                }
            }
            .run(),
            1
        );
        assert_eq!(
            Cli {
                command: Command::Brief {
                    args: args(&["--help"])
                }
            }
            .run(),
            0
        );
        assert_eq!(
            Cli {
                command: Command::Brief {
                    args: args(&["invalid"])
                }
            }
            .run(),
            1
        );
        assert_eq!(
            Cli {
                command: Command::SystemSync {
                    project: Some(temp.path().join("missing"))
                }
            }
            .run(),
            0
        );
        assert_eq!(
            Cli {
                command: Command::Send { args: Vec::new() }
            }
            .run(),
            1
        );
        assert_eq!(
            Cli {
                command: Command::FastForward { args: Vec::new() }
            }
            .run(),
            2
        );
        assert_eq!(
            Cli {
                command: Command::PendingReply { args: Vec::new() }
            }
            .run(),
            2
        );

        let status = temp.path().join("dispatch/status");
        assert_eq!(
            Cli {
                command: Command::DaemonReport {
                    args: args(&[status.to_str().unwrap(), "done", "0123456789abcdef", "done"])
                }
            }
            .run(),
            0
        );
    }
}

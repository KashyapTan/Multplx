//! Command-line dispatch for the Multplx Rust runtime.

mod authority;
mod bootstrap;
mod deep_review;
mod doctor;
mod launcher;
mod review;
mod session_start;
mod status_snapshot;
mod supervision;
mod system_snapshot;
mod tooling;
mod workflow_runtime;

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
    /// Activate or operate one globally configured Multplx control plane.
    #[command(disable_help_flag = true)]
    Launcher {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Install, upgrade, or uninstall the global Multplx binary.
    #[command(disable_help_flag = true)]
    LauncherInstall {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Run the resource-aware behavior-test scheduler.
    #[command(disable_help_flag = true)]
    TestRun {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Run the repeated behavior-test isolation proof.
    #[command(disable_help_flag = true)]
    TestIsolationProof {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Validate maintained documentation classification and local links.
    #[command(disable_help_flag = true)]
    DocAudienceCheck {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
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
    /// Run one Rust-owned local viz or vplan lifecycle or service entry point.
    #[command(hide = true, disable_help_flag = true)]
    Services {
        entry: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
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
        if std::env::var("MX_MULTICALL_EXPLICIT").as_deref() != Ok("1")
            && let Some(program) = args.first().cloned()
            && let Some(alias) = multicall_alias(&program)
        {
            args.insert(1, alias);
        }
        Self::parse_from(args)
    }

    /// Runs the selected command.
    pub fn run(self) -> i32 {
        match self.command {
            Command::Launcher { args } => launcher::run(&args),
            Command::LauncherInstall { args } => launcher::run_installer(&args),
            Command::TestRun { args } => tooling::run_tests(&args),
            Command::TestIsolationProof { args } => tooling::run_isolation_proof(&args),
            Command::DocAudienceCheck { args } => tooling::run_documentation_check(&args),
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
                let report = multplx_domain::lifecycle::fast_forward::update(
                    &context,
                    &state,
                    &data.join("daemons.md"),
                );
                let broker_status = report.broker_status;
                for line in report.lines {
                    println!("{line}");
                }
                let pending_launcher_update = match launcher::registered_update_pending() {
                    Ok(pending) => pending,
                    Err(error) => {
                        eprintln!("mx-update: {error}");
                        return 1;
                    }
                };
                if broker_status == multplx_domain::lifecycle::fast_forward::Status::Updated
                    || pending_launcher_update
                {
                    match launcher::upgrade_registered_after_update(&context.root, &context.home) {
                        Ok(Some(line)) => println!("{line}"),
                        Ok(None) => {}
                        Err(error) => {
                            eprintln!("mx-update: {error}");
                            return 1;
                        }
                    }
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
            Command::HomeSeed { args } => run_home_seed(&args),
            Command::Spawn { args } => run_spawn(&args),
            Command::SuperviseDaemon { args } => {
                let (root, home, _) = active_paths();
                supervision::supervise_daemon(&args, &home, &runtime_root(&root))
            }
            Command::Teardown { args } => run_teardown(&args),
            Command::UpstreamDiff { args } => run_upstream_diff(&args),
            Command::FastForward { args } => run_fast_forward(&args),
            Command::PendingReply { args } => run_pending_reply(&args),
            Command::Supervision { entry, args } => run_supervision(&entry, &args),
            Command::Session { entry, args } => run_session(&entry, &args),
            Command::ReportMcp => {
                let root = runtime_root(&active_paths().0);
                multplx_services::report_mcp::serve(&root)
            }
            Command::Services { entry, args } => {
                let root = runtime_root(&active_paths().0);
                multplx_services::local_services::run(&entry, &args, &root)
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
    if entry == "mx-session-start.sh" {
        if !args.is_empty() {
            eprintln!("error: mx-session-start.sh does not accept arguments");
            return 2;
        }
        let (root, home, data) = active_paths();
        let source_root = runtime_root(&root);
        let state = std::env::var_os("MX_STATE_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("state"));
        let config = std::env::var_os("MX_CONFIG_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("config"));
        let harness = detect_primary_harness(&source_root);
        print!(
            "{}",
            session_start::run(
                &session_start::Paths {
                    root,
                    home,
                    data,
                    state,
                    config,
                    source_root,
                },
                &harness
            )
        );
        return 0;
    }
    if entry == "mx-bootstrap.sh" {
        let values = args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let (root, home, data) = active_paths();
        let source_root = runtime_root(&root);
        let paths = bootstrap::Paths {
            projects: std::env::var_os("MX_PROJECTS_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("projects")),
            config: std::env::var_os("MX_CONFIG_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("config")),
            state: std::env::var_os("MX_STATE_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("state")),
            root,
            home,
            data,
            source_root,
        };
        let (status, stdout, stderr) = bootstrap::run(&values, &paths);
        print!("{stdout}");
        eprint!("{stderr}");
        return status;
    }
    if entry == "mx-doctor.sh" {
        let values = args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let (root, home, data) = active_paths();
        let paths = doctor::Paths {
            root,
            state: std::env::var_os("MX_STATE_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("state")),
            data,
        };
        let (status, stdout, stderr) = doctor::run(&values, &paths);
        print!("{stdout}");
        eprint!("{stderr}");
        return status;
    }
    if entry == "mx-status-snapshot.sh" {
        let values = args
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let (root, home, _) = active_paths();
        let (status, stdout, stderr) = status_snapshot::run(&values, &runtime_root(&root), &home);
        print!("{stdout}");
        eprint!("{stderr}");
        return status;
    }
    if entry == "mx-system-snapshot.sh" {
        let values = args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let (root, home, data) = active_paths();
        let source_root = runtime_root(&root);
        let paths = system_snapshot::Paths {
            projects: std::env::var_os("MX_PROJECTS_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("projects")),
            config: std::env::var_os("MX_CONFIG_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("config")),
            state: std::env::var_os("MX_STATE_OVERRIDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("state")),
            root,
            home,
            data,
            source_root,
        };
        let (status, stdout, stderr) = system_snapshot::run(&values, &paths);
        print!("{stdout}");
        eprint!("{stderr}");
        return status;
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
    eprintln!("error: unhandled session entry point: {entry}");
    2
}

fn run_system_view(args: &[OsString]) -> i32 {
    if args.len() == 1 && matches!(args[0].to_str(), Some("-h" | "--help")) {
        print!("{}", multplx_domain::snapshot::SYSTEM_VIEW_USAGE);
        return 0;
    }
    if args.len() == 1 && args[0] == OsStr::new("--json") {
        return run_session("mx-system-snapshot.sh", &[OsString::from("--json")]);
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
    let (_, home, data) = active_paths();
    let paths = system_snapshot::Paths {
        projects: std::env::var_os("MX_PROJECTS_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("projects")),
        config: std::env::var_os("MX_CONFIG_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("config")),
        state: std::env::var_os("MX_STATE_OVERRIDE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("state")),
        root,
        home,
        data,
        source_root,
    };
    let (status, stdout, stderr) = system_snapshot::run(&["--json".into()], &paths);
    if status != 0 {
        print!("{stdout}");
        eprint!("{stderr}");
        return status;
    }
    match multplx_domain::snapshot::parse_system_snapshot(stdout.as_bytes()) {
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
    if entry == "mx-cursor-hook.sh" {
        let mut payload = String::new();
        let _ = io::stdin().read_to_string(&mut payload);
        let (root, _, _) = active_paths();
        let source_root = runtime_root(&root);
        return supervision::cursor_hook(args, &payload, &source_root);
    }
    if entry == "mx-claude-stop-autoarm.sh" {
        let mut payload = String::new();
        let _ = io::stdin().read_to_string(&mut payload);
        let (root, home, _) = active_paths();
        let source_root = runtime_root(&root);
        let logical_root = std::env::var_os("MX_ROOT_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.clone());
        return supervision::claude_stop_autoarm(&logical_root, &home, &source_root);
    }
    if entry == "mx-afk-start.sh" {
        let (root, home, _) = active_paths();
        let source_root = runtime_root(&root);
        return supervision::afk_start(args, &home, &source_root);
    }
    if entry == "mx-afk-launch.sh" {
        let (root, home, _) = active_paths();
        let source_root = runtime_root(&root);
        return supervision::afk_launch(args, &home, &source_root);
    }
    if entry == "mx-afk-return.sh" {
        let (root, home, _) = active_paths();
        let source_root = runtime_root(&root);
        return supervision::afk_return(args, &home, &source_root);
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
    if entry == "mx-guard.sh" {
        let (root, home, _) = active_paths();
        let source_root = runtime_root(&root);
        let detected = detect_primary_harness(&source_root);
        return supervision::guard(&root, &home, &source_root, &detected);
    }
    if entry == "mx-turnend-guard.sh" {
        let mut payload = String::new();
        let _ = io::stdin().read_to_string(&mut payload);
        let (root, home, _) = active_paths();
        let source_root = runtime_root(&root);
        let detected = detect_primary_harness(&source_root);
        return supervision::turnend_guard(args, &payload, &root, &home, &source_root, &detected);
    }
    if entry == "mx-watch-checkpoint.sh" {
        let (root, home, _) = active_paths();
        let source_root = runtime_root(&root);
        return supervision::watch_checkpoint(args, &root, &home, &source_root);
    }
    if matches!(entry, "mx-watch.sh" | "mx-watch-arm.sh") {
        let (root, home, _) = active_paths();
        let source_root = runtime_root(&root);
        if entry == "mx-watch.sh" {
            return supervision::watch(&root, &home, &source_root);
        }
        if entry == "mx-watch-arm.sh" {
            return supervision::watch_arm(args, &root, &home, &source_root);
        }
    }
    eprintln!("error: unhandled supervision entry point: {entry}");
    2
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
    let (root, home, _) = active_paths();
    let source_root = runtime_root(&root);
    let detected = detect_primary_harness(&source_root);
    let _ = supervision::guard(&root, &home, &source_root, &detected);
    0
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

fn run_upstream_diff(args: &[OsString]) -> i32 {
    let values = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let root = runtime_root(&active_paths().0);
    let record = std::env::var_os("MX_UPSTREAM_RECORD_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("docs/upstream.md"));
    let output = multplx_domain::lifecycle::upstream_diff::run(&values, &record);
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    output.status
}

fn run_home_seed(args: &[OsString]) -> i32 {
    let (root, home, data) = active_paths();
    let context = multplx_domain::lifecycle::home_seed::Context {
        root,
        projects: std::env::var_os("MX_PROJECTS_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("projects")),
        state: std::env::var_os("MX_STATE_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("state")),
        home,
        data,
    };
    let output = multplx_domain::lifecycle::home_seed::run(args, &context);
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    output.status
}

fn park_spawn_if_at_limit(
    args: &[OsString],
    single_checkout_request: Option<&str>,
) -> Result<Option<String>, String> {
    use multplx_backend::headroom::{HeadroomPaths, QueueRecord};

    if args.iter().any(|value| value == "--daemon")
        || std::env::var("MX_HEADROOM_SKIP_QUEUE").as_deref() == Ok("1")
    {
        return Ok(None);
    }
    let mut positional = Vec::new();
    let mut harness = String::new();
    let mut model = String::new();
    let mut effort = String::new();
    let mut backend = "tmux".to_owned();
    let mut kind = "delivery".to_owned();
    let mut index = 0_usize;
    while index < args.len() {
        let value = args[index]
            .to_str()
            .ok_or("spawn argument is not valid UTF-8")?;
        match value {
            "--scout" => kind = "scout".to_owned(),
            "--harness" | "--model" | "--effort" | "--backend" => {
                let next = args
                    .get(index + 1)
                    .and_then(|next| next.to_str())
                    .ok_or_else(|| format!("{value} requires a value"))?
                    .to_owned();
                match value {
                    "--harness" => harness = next,
                    "--model" => model = next,
                    "--effort" => effort = next,
                    _ => backend = next,
                }
                index += 1;
            }
            value if value.starts_with("--") => {
                return Err(format!("unsupported native daemon spawn option: {value}"));
            }
            _ => positional.push(value.to_owned()),
        }
        index += 1;
    }
    let id = positional.first().ok_or("invalid spawn request")?;
    multplx_core::identifiers::TaskId::parse(id).map_err(|_| "invalid spawn request")?;
    let project = positional.get(1).ok_or("invalid spawn request")?;
    if harness.is_empty()
        && let Some(positional_harness) = positional.get(2)
    {
        harness.clone_from(positional_harness);
    }
    if positional.len() > 3 {
        return Err("invalid spawn request".to_owned());
    }
    multplx_backend::facade::BackendName::parse(&backend)
        .map_err(|_| format!("unsupported backend: {backend}"))?;
    let paths = HeadroomPaths::from_environment();
    let headroom = multplx_backend::headroom::evaluate(&paths).map_err(|error| {
        format!("dispatch capacity could not be established; refusing to spawn {id}: {error}")
    })?;
    if !headroom.at_limit() {
        return Ok(None);
    }
    if single_checkout_request.is_some() {
        return Err(
            "an exact single-checkout grant cannot be queued; retry it after capacity is available"
                .to_owned(),
        );
    }
    multplx_backend::headroom::queue_add(
        &paths,
        &QueueRecord {
            task_id: id.clone(),
            project: project.clone(),
            harness,
            model,
            effort,
            backend,
            kind,
            enqueued_at: multplx_backend::headroom::now_epoch(),
        },
    )
    .map(Some)
    .map_err(|error| error.to_string())
}

fn resolve_spawn_backend(config: &Path) -> (String, Option<String>) {
    if let Some(backend) = std::env::var_os("MX_BACKEND").filter(|value| !value.is_empty()) {
        return (backend.to_string_lossy().into_owned(), None);
    }
    if let Ok(contents) = fs::read_to_string(config.join("backend"))
        && let Some(backend) = contents
            .lines()
            .map(|line| {
                line.chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>()
            })
            .find(|line| !line.is_empty())
    {
        return (backend, None);
    }
    if std::env::var_os("TMUX").is_some_and(|value| !value.is_empty()) {
        return ("tmux".to_owned(), None);
    }
    if std::env::var("HERDR_ENV").as_deref() == Ok("1") {
        return (
            "herdr".to_owned(),
            Some("NOTICE: auto-detected herdr runtime (HERDR_ENV=1) - spawning into the EXPERIMENTAL herdr backend. Set config/backend or pass --backend tmux to opt out.".to_owned()),
        );
    }
    if std::env::var_os("CMUX_WORKSPACE_ID").is_some_and(|value| !value.is_empty()) {
        return (
            "cmux".to_owned(),
            Some("NOTICE: auto-detected cmux runtime (CMUX_WORKSPACE_ID) - spawning into the EXPERIMENTAL cmux backend. Set config/backend or pass --backend tmux to opt out.".to_owned()),
        );
    }
    if cfg!(target_os = "macos")
        && std::env::var("__CFBundleIdentifier").as_deref() == Ok("com.cmuxterm.app")
    {
        return (
            "cmux".to_owned(),
            Some("NOTICE: auto-detected cmux runtime (FALLBACK signal __CFBundleIdentifier=com.cmuxterm.app; CMUX_WORKSPACE_ID absent, stripped by cmux's bundled claude wrapper) - spawning into the EXPERIMENTAL cmux backend. Set config/backend or pass --backend tmux to opt out.".to_owned()),
        );
    }
    if cfg!(target_os = "macos") && cmux_app_is_ancestor() {
        return (
            "cmux".to_owned(),
            Some("NOTICE: auto-detected cmux runtime (FALLBACK signal process-ancestry reaching the running cmux app; CMUX_WORKSPACE_ID absent, stripped by cmux's bundled claude wrapper) - spawning into the EXPERIMENTAL cmux backend. Set config/backend or pass --backend tmux to opt out.".to_owned()),
        );
    }
    ("tmux".to_owned(), None)
}

fn cmux_app_is_ancestor() -> bool {
    let app_pid = std::process::Command::new("lsappinfo")
        .args(["info", "-only", "pid", "-app", "com.cmuxterm.app"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| output.rsplit_once('=').map(|(_, value)| value.to_owned()))
        .and_then(|value| {
            value
                .trim_matches(|character: char| character.is_whitespace() || character == '"')
                .parse::<u32>()
                .ok()
        });
    let mut pid = std::process::id();
    for _ in 0..32 {
        if app_pid == Some(pid) {
            return true;
        }
        let command = std::process::Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .unwrap_or_default();
        if command
            .trim_end()
            .ends_with("/cmux.app/Contents/MacOS/cmux")
        {
            return true;
        }
        let Some(parent) = std::process::Command::new("ps")
            .args(["-o", "ppid=", "-p", &pid.to_string()])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|output| output.trim().parse::<u32>().ok())
            .filter(|parent| *parent > 1)
        else {
            return false;
        };
        pid = parent;
    }
    false
}

fn run_spawn(args: &[OsString]) -> i32 {
    use multplx_backend::facade::{BackendName, RuntimeBackend, TaskSpec};
    if multplx_core::gate_refuse::is_gate_agent(
        std::env::var_os("DEEP_REVIEW_GATE").is_some(),
        std::env::var("MX_GATE_REFUSE_BYPASS").as_deref() == Ok("1"),
    ) {
        eprintln!("{}", multplx_core::gate_refuse::REFUSAL_MESSAGE);
        return i32::from(multplx_core::gate_refuse::REFUSAL_EXIT);
    }
    if args
        .first()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            value
                .split_once('=')
                .is_some_and(|(id, _)| !id.contains('/'))
        })
    {
        let mut pairs = Vec::new();
        let mut shared = Vec::new();
        let mut index = 0_usize;
        while index < args.len() {
            let Some(value) = args[index].to_str() else {
                eprintln!("error: spawn argument is not valid UTF-8");
                return 1;
            };
            if value.starts_with("--") {
                shared.push(args[index].clone());
                if matches!(value, "--harness" | "--model" | "--effort" | "--backend") {
                    let Some(next) = args.get(index + 1) else {
                        eprintln!("error: {value} requires a value");
                        return 1;
                    };
                    shared.push(next.clone());
                    index += 1;
                }
            } else if let Some((id, project)) = value.split_once('=') {
                if id.is_empty() || project.is_empty() {
                    eprintln!("error: invalid batch pair '{value}'");
                    return 1;
                }
                pairs.push((OsString::from(id), OsString::from(project)));
            } else {
                eprintln!("batch: batch dispatch expects every argument as id=repo; got '{value}'");
                return 1;
            }
            index += 1;
        }
        if pairs.is_empty() {
            eprintln!("error: invalid spawn request");
            return 1;
        }
        let mut failed = false;
        for (id, project) in pairs {
            let id_text = id.to_string_lossy().into_owned();
            let project_text = project.to_string_lossy().into_owned();
            let mut request = vec![id, project];
            request.extend(shared.iter().cloned());
            let status = run_spawn(&request);
            if status != 0 {
                eprintln!("batch: FAILED to spawn {id_text} ({project_text})");
                failed = true;
            }
        }
        return i32::from(failed);
    }
    let mut parse_args = Vec::new();
    let mut single_checkout_request = None;
    let mut index = 0_usize;
    while index < args.len() {
        let Some(value) = args[index].to_str() else {
            eprintln!("error: spawn argument is not valid UTF-8");
            return 1;
        };
        if value == "--single-checkout" {
            let Some(request) = args.get(index + 1).and_then(|value| value.to_str()) else {
                eprintln!("error: --single-checkout requires a valid request id");
                return 1;
            };
            single_checkout_request = Some(request.to_owned());
            index += 2;
            continue;
        }
        if let Some(request) = value.strip_prefix("--single-checkout=") {
            single_checkout_request = Some(request.to_owned());
        } else {
            parse_args.push(args[index].clone());
        }
        index += 1;
    }
    let (root, home, data) = active_paths();
    let logical_home = home.clone();
    let source_root = runtime_root(&root);
    let context = multplx_domain::lifecycle::spawn::Context {
        root: fs::canonicalize(&root).unwrap_or(root),
        state: std::env::var_os("MX_STATE_OVERRIDE")
            .map(PathBuf::from)
            .and_then(|path| fs::canonicalize(&path).ok().or(Some(path)))
            .unwrap_or_else(|| home.join("state")),
        home: fs::canonicalize(&home).unwrap_or(home),
        data: if data.as_os_str().is_empty() {
            logical_home.join("data")
        } else {
            fs::canonicalize(&data).unwrap_or(data)
        },
        projects: std::env::var_os("MX_PROJECTS_OVERRIDE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| logical_home.join("projects")),
    };
    if let Err(error_value) = fs::create_dir_all(&context.state) {
        eprintln!("error: cannot create state directory: {error_value}");
        return 1;
    }
    let config = std::env::var_os("MX_CONFIG_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| context.home.join("config"));
    let settings = multplx_backend::harness::HarnessConfig::new(config.clone());
    if !parse_args.iter().any(|value| value == "--backend") {
        let (backend, notice) = resolve_spawn_backend(&config);
        parse_args.push(OsString::from("--backend"));
        parse_args.push(OsString::from(backend));
        if let Some(notice) = notice {
            eprintln!("{notice}");
        }
    }
    match park_spawn_if_at_limit(&parse_args, single_checkout_request.as_deref()) {
        Ok(Some(output)) => {
            print!("{output}");
            return 0;
        }
        Ok(None) => {}
        Err(error_value) => {
            eprintln!("error: {error_value}");
            return 1;
        }
    }
    let default_harness = if args.iter().any(|value| value == "--daemon") {
        settings.daemon(multplx_backend::harness::detect())
    } else {
        settings.actor(multplx_backend::harness::detect())
    };
    let request = match multplx_domain::lifecycle::spawn::parse(
        &parse_args,
        &context,
        default_harness.as_str(),
    ) {
        Ok(request) => request,
        Err(error_value) => {
            eprintln!("error: {error_value}");
            return 1;
        }
    };
    let mut request = request;
    if single_checkout_request.is_some() && request.kind != "delivery" {
        eprintln!("error: --single-checkout is supported only for one delivery task");
        return 1;
    }
    let mut spawn_positionals = 0_usize;
    let mut explicit_harness = false;
    let mut explicit_model = false;
    let mut explicit_effort = false;
    let mut skip_value = false;
    for argument in &parse_args {
        if skip_value {
            skip_value = false;
            continue;
        }
        let Some(argument) = argument.to_str() else {
            eprintln!("error: spawn argument is not valid UTF-8");
            return 1;
        };
        match argument {
            "--harness" => {
                explicit_harness = true;
                skip_value = true;
            }
            "--model" => {
                explicit_model = true;
                skip_value = true;
            }
            "--effort" => {
                explicit_effort = true;
                skip_value = true;
            }
            "--backend" => skip_value = true,
            value if value.starts_with("--") => {}
            _ => spawn_positionals += 1,
        }
    }
    explicit_harness |= spawn_positionals >= 3;
    if request.kind == "daemon" && !explicit_harness {
        if !explicit_model && let Some(model) = settings.daemon_model() {
            request.model = model;
        }
        if !explicit_effort && let Some(effort) = settings.daemon_effort() {
            request.effort = effort;
        }
    }
    if !matches!(
        request.harness.as_str(),
        "codex" | "claude" | "pi" | "cursor"
    ) && !request.harness.contains(' ')
    {
        eprintln!(
            "error: no launch template for harness '{}'{}",
            request.harness,
            if request.kind == "daemon" {
                " (check config/daemon-harness or the explicit selection)"
            } else {
                ""
            }
        );
        return 1;
    }
    if request.kind != "daemon" && config.join("actor-dispatch.json").is_file() && !explicit_harness
    {
        eprintln!(
            "error: config/actor-dispatch.json is active - pass an explicit harness resolved from the dispatch rules"
        );
        return 1;
    }
    let mut single_checkout_store = None;
    if let Some(request_id) = single_checkout_request.as_deref() {
        let git_value = |arguments: &[&str]| -> Result<String, String> {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&request.project)
                .args(arguments)
                .output()
                .map_err(|error_value| error_value.to_string())?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
            }
            String::from_utf8(output.stdout)
                .map(|value| value.trim().to_owned())
                .map_err(|_| "git output is not valid UTF-8".to_owned())
        };
        if !git_value(&["status", "--porcelain=v1", "--untracked-files=all"])
            .is_ok_and(|value| value.is_empty())
        {
            eprintln!(
                "error: single-checkout mode requires a clean checkout so task material remains attributable"
            );
            return 1;
        }
        let base_head = match git_value(&["rev-parse", "HEAD"]) {
            Ok(value) => value,
            Err(error_value) => {
                eprintln!("error: {error_value}");
                return 1;
            }
        };
        let base_branch = match git_value(&["symbolic-ref", "--quiet", "--short", "HEAD"]) {
            Ok(value) => value,
            Err(_) => {
                eprintln!("error: single-checkout mode requires an attached base branch");
                return 1;
            }
        };
        let claim = match multplx_core::locks::DirectoryLock::acquire_wait(
            context.state.join(".single-checkout.acquire"),
            &SystemProcessProbe::default(),
            Duration::from_secs(5),
        ) {
            Ok(claim) => claim,
            Err(error_value) => {
                eprintln!("error: could not serialize single-checkout reservation: {error_value}");
                return 1;
            }
        };
        let Some(project_text) = request.project.to_str() else {
            eprintln!("error: project path is not valid UTF-8");
            return 1;
        };
        let record = context.state.join(format!(
            ".single-checkout-{}.json",
            multplx_domain::maintainer_override::sha256_text(project_text)
        ));
        if record.exists() || fs::symlink_metadata(&record).is_ok() {
            eprintln!("error: checkout already has a single-checkout reservation");
            return 1;
        }
        let binding_value = match authority::single_checkout_binding(&request.id, &request.project)
        {
            Ok(value) => value,
            Err(error_value) => {
                eprintln!("error: {error_value}");
                return 1;
            }
        };
        let field = |name: &str| {
            binding_value
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        };
        let binding = multplx_domain::maintainer_override::Binding {
            boundary: field("boundary"),
            task: field("task"),
            project: field("project"),
            operation: field("operation"),
            target: field("target"),
            expected_state_digest: field("expected_state_digest"),
        };
        let store = multplx_domain::maintainer_override::OverrideStore::new(&context.state);
        if let Err(error_value) = store.consume(request_id, &binding) {
            eprintln!("error: {error_value}");
            return 1;
        }
        let record_value = serde_json::json!({
            "version": 1,
            "task_id": request.id,
            "target_identity": request.project,
            "request_id": request_id,
            "base_head": base_head,
            "base_branch": base_branch,
        });
        if let Err(error_value) = multplx_core::filesystem::atomic_replace(
            &record,
            serde_json::to_string(&record_value)
                .unwrap_or_default()
                .as_bytes(),
            0o600,
        ) {
            let _ = store.result(
                request_id,
                false,
                &format!("single-checkout reservation failed: {error_value}"),
            );
            eprintln!("error: {error_value}");
            return 1;
        }
        drop(claim);
        request.single_checkout_override = Some(request_id.to_owned());
        request.single_checkout_record = Some(record);
        request.single_checkout_base_head = Some(base_head);
        request.single_checkout_base_branch = Some(base_branch);
        single_checkout_store = Some(store);
    }
    let lock_path = context.state.join(format!(".spawn-{}.lock", request.id));
    let lock = match multplx_core::locks::DirectoryLock::acquire_wait(
        lock_path,
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    ) {
        Ok(lock) => lock,
        Err(error_value) => {
            eprintln!("error: cannot acquire spawn lock: {error_value}");
            return 1;
        }
    };
    let recovering_daemon = request.kind == "daemon"
        && std::env::var("MX_SPAWN_RECOVERY").as_deref() == Ok("1")
        && context.state.join(format!("{}.meta", request.id)).is_file();
    let presentation_enabled = request.backend == "herdr"
        && request.kind != "daemon"
        && config.join("herdr-presentation-spaces").is_file();
    let presentation_journal =
        multplx_backend::herdr_presentation::journal_path(&context.state, &request.id);
    let recovering_projection = presentation_enabled
        && (presentation_journal.exists() || fs::symlink_metadata(&presentation_journal).is_ok());
    if context.state.join(format!("{}.meta", request.id)).exists()
        && !recovering_daemon
        && !recovering_projection
    {
        eprintln!("error: metadata for {} already exists", request.id);
        return 1;
    }
    if request.kind == "daemon" && !recovering_daemon {
        if let Some(commit) =
            multplx_domain::lifecycle::fast_forward::primary_head_commit(&context.root)
        {
            let outcome = multplx_domain::lifecycle::fast_forward::fast_forward(
                &request.home,
                &format!("daemon {}", request.id),
                &multplx_domain::lifecycle::fast_forward::Base::Commit(commit),
                true,
                true,
            );
            if outcome.status == multplx_domain::lifecycle::fast_forward::Status::Skipped {
                let reason = outcome
                    .line
                    .split_once(": skipped: ")
                    .map_or(outcome.line.as_str(), |(_, reason)| reason);
                eprintln!(
                    "warning: daemon {} sync skipped before launch: {reason}",
                    request.id
                );
            }
        } else {
            eprintln!(
                "warning: daemon {} sync skipped before launch: primary default-branch commit cannot be resolved",
                request.id
            );
        }
    }
    let _inherit_lock = if request.kind == "daemon" && !recovering_daemon {
        let inherit_lock = match multplx_domain::inheritance::acquire_inherit_lock(&request.home) {
            Ok(lock) => lock,
            Err(error_value) => {
                eprintln!(
                    "error: could not acquire daemon inheritance lock for {}: {error_value}",
                    request.home.display()
                );
                return 1;
            }
        };
        match multplx_domain::inheritance::propagate_daemon(
            &context.home,
            &request.home,
            Some(&config),
            Some(&context.data),
        ) {
            Ok(outcome) => {
                print!("{}", outcome.stdout);
                eprint!("{}", outcome.stderr);
                if outcome.failed {
                    eprintln!(
                        "warning: daemon {} inheritance failed for {}",
                        request.id,
                        request.home.display()
                    );
                }
            }
            Err(error_value) => eprintln!(
                "warning: daemon {} inheritance failed for {}: {error_value}",
                request.id,
                request.home.display()
            ),
        }
        Some(inherit_lock)
    } else {
        None
    };
    let mut created_target = None;
    let mut herdr_endpoint = None;
    let mut projected_endpoint = None;
    let mut presentation_lock = None;
    let herdr_backend = || {
        multplx_backend::herdr::HerdrBackend::new(
            multplx_backend::command::SystemCommandRunner,
            std::env::var_os("MX_HERDR_BIN").unwrap_or_else(|| OsString::from("herdr")),
            std::env::var("HERDR_SESSION").unwrap_or_else(|_| "default".to_owned()),
            request.home.clone(),
        )
    };
    let result: Result<_, String> = (|| {
        let spec = TaskSpec {
            label: format!("mx-{}", request.id),
            working_directory: request.project.clone(),
        };
        let (target, named_endpoint) = match request.backend.as_str() {
            "tmux" => {
                let mut backend = multplx_backend::tmux::TmuxBackend::system();
                let container = backend
                    .container_ensure()
                    .map_err(|error_value| error_value.to_string())?;
                let target = backend
                    .task_create(&container, &spec)
                    .map_err(|error_value| error_value.to_string())?;
                (target, format!("{}:mx-{}", container.as_str(), request.id))
            }
            "herdr" => 'herdr: {
                let mut backend = herdr_backend();
                if presentation_enabled {
                    use multplx_backend::herdr_presentation::{
                        ProjectionSpawnOutcome, ProjectionSpawnRequest, spawn_projection,
                    };
                    let projection = spawn_projection(
                        &mut backend,
                        &ProjectionSpawnRequest {
                            state: &context.state,
                            task_id: &request.id,
                            home: &request.home,
                            cwd: &request.project,
                            task_label: &spec.label,
                            recovering: recovering_projection,
                        },
                    )
                    .map_err(|error_value| error_value.to_string())?;
                    match projection {
                        ProjectionSpawnOutcome::Projected {
                            target,
                            endpoint,
                            lock,
                            warnings,
                        } => {
                            for warning in warnings {
                                eprintln!("warning: {warning}");
                            }
                            presentation_lock = Some(lock);
                            let named_endpoint = target.endpoint().to_owned();
                            herdr_endpoint = Some((
                                endpoint.session,
                                endpoint.workspace_id,
                                endpoint.tab_id,
                                endpoint.pane_id,
                            ));
                            projected_endpoint = herdr_endpoint
                                .as_ref()
                                .map(|(session, _, _, pane)| (session.clone(), pane.clone()));
                            break 'herdr (target, named_endpoint);
                        }
                        ProjectionSpawnOutcome::Flat { warning } => eprintln!("warning: {warning}"),
                    }
                }
                let container = if recovering_daemon {
                    let prior =
                        fs::read_to_string(context.state.join(format!("{}.meta", request.id)))
                            .unwrap_or_default();
                    let session = prior
                        .lines()
                        .rev()
                        .find_map(|line| line.strip_prefix("herdr_session="))
                        .filter(|value| !value.is_empty())
                        .unwrap_or("default");
                    let workspace = prior
                        .lines()
                        .rev()
                        .find_map(|line| line.strip_prefix("herdr_workspace_id="))
                        .ok_or("recovering Herdr daemon has no recorded workspace")?;
                    multplx_backend::facade::ContainerId::for_backend(
                        BackendName::Herdr,
                        format!("{session}:{workspace}"),
                    )
                    .map_err(|error_value| error_value.to_string())?
                } else {
                    backend
                        .container_ensure()
                        .map_err(|error_value| error_value.to_string())?
                };
                let seeded_tab = backend.seeded_tab_id().map(str::to_owned);
                let endpoint = backend
                    .create_task_full(&container, &spec, seeded_tab.as_deref())
                    .map_err(|error_value| error_value.to_string())?;
                let named_endpoint = endpoint.target.endpoint().to_owned();
                let (session, workspace) = container
                    .as_str()
                    .split_once(':')
                    .ok_or("Herdr container is missing its session scope")?;
                herdr_endpoint = Some((
                    session.to_owned(),
                    workspace.to_owned(),
                    endpoint.tab_id,
                    endpoint.pane_id,
                ));
                (endpoint.target, named_endpoint)
            }
            "cmux" => {
                let mut backend = multplx_backend::cmux::CmuxBackend::system();
                let container = backend
                    .container_ensure()
                    .map_err(|error_value| error_value.to_string())?;
                let target = backend
                    .task_create(&container, &spec)
                    .map_err(|error_value| error_value.to_string())?;
                let endpoint = target.endpoint().to_owned();
                (target, endpoint)
            }
            _ => unreachable!(),
        };
        created_target = Some(target.clone());
        let actor_worktree = if request.kind == "daemon" {
            request.home.clone()
        } else if request.single_checkout_override.is_some() {
            request.project.clone()
        } else {
            match target.backend() {
                BackendName::Tmux => {
                    let mut backend = multplx_backend::tmux::TmuxBackend::system();
                    backend
                        .send_literal(&target, "treehouse get")
                        .and_then(|()| backend.send_key(&target, "Enter"))
                        .map_err(|error_value| error_value.to_string())?;
                }
                BackendName::Herdr => {
                    let mut backend = herdr_backend();
                    backend
                        .send_literal(&target, "treehouse get")
                        .and_then(|()| backend.send_key(&target, "Enter"))
                        .map_err(|error_value| error_value.to_string())?;
                }
                BackendName::Cmux => {
                    let mut backend = multplx_backend::cmux::CmuxBackend::system();
                    backend
                        .send_literal(&target, "treehouse get")
                        .and_then(|()| backend.send_key(&target, "Enter"))
                        .map_err(|error_value| error_value.to_string())?;
                }
            }
            let project = fs::canonicalize(&request.project)
                .map_err(|error_value| format!("cannot resolve project: {error_value}"))?;
            let mut candidate = None;
            let mut settled = None;
            for _ in 0..60 {
                let current = match target.backend() {
                    BackendName::Tmux => {
                        multplx_backend::tmux::TmuxBackend::system().current_path(&target)
                    }
                    BackendName::Herdr => herdr_backend().current_path(&target),
                    BackendName::Cmux => {
                        multplx_backend::cmux::CmuxBackend::system().current_path(&target)
                    }
                };
                if let Ok(path) = current {
                    let observed = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                    if observed != project {
                        if candidate.as_ref() == Some(&observed) {
                            settled = Some(path);
                            break;
                        }
                        candidate = Some(observed);
                    } else {
                        candidate = None;
                    }
                } else {
                    candidate = None;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            let worktree = settled.ok_or_else(|| {
                format!(
                    "treehouse get did not enter a worktree within 60s; inspect window {named_endpoint}"
                )
            })?;
            if !worktree.is_dir() {
                return Err(format!(
                    "treehouse get did not yield an isolated worktree: {}",
                    worktree.display()
                ));
            }
            let output = std::process::Command::new("git")
                .args([
                    "-C",
                    worktree
                        .to_str()
                        .ok_or("worktree path is not valid UTF-8")?,
                    "rev-parse",
                    "--show-toplevel",
                ])
                .output()
                .map_err(|error_value| error_value.to_string())?;
            if !output.status.success() {
                return Err("treehouse get did not yield an isolated worktree".to_owned());
            }
            let top = PathBuf::from(
                String::from_utf8(output.stdout)
                    .map_err(|_| "git worktree path is not UTF-8")?
                    .trim(),
            );
            if fs::canonicalize(top).ok() != fs::canonicalize(&worktree).ok() {
                return Err("treehouse get did not yield an isolated worktree".to_owned());
            }
            worktree
        };
        multplx_domain::lifecycle::spawn::publish_meta_for_worktree(
            &context,
            &request,
            &named_endpoint,
            &actor_worktree,
        )?;
        if let Some((session, workspace, tab, pane)) = herdr_endpoint.as_ref() {
            let meta_path = context.state.join(format!("{}.meta", request.id));
            let mut meta = fs::read_to_string(&meta_path)
                .map_err(|error_value| format!("cannot read published metadata: {error_value}"))?;
            meta.push_str(&format!(
                "herdr_session={session}\nherdr_workspace_id={workspace}\nherdr_tab_id={tab}\nherdr_pane_id={pane}\n"
            ));
            multplx_core::filesystem::atomic_replace(&meta_path, meta.as_bytes(), 0o600)
                .map_err(|error_value| error_value.to_string())?;
        }
        let brief = if request.kind == "daemon" {
            request.home.join("data/charter.md")
        } else {
            context.data.join(&request.id).join("brief.md")
        };
        let report_server = source_root.join("bin/mx-report-mcp");
        let task_tmp = PathBuf::from(format!("/tmp/mx-{}", request.id));
        fs::create_dir_all(task_tmp.join("gotmp"))
            .map_err(|error_value| error_value.to_string())?;
        let cursor_plugin = task_tmp.join("cursor-turnend-plugin");
        if request.harness == "cursor" && request.kind != "daemon" {
            fs::create_dir_all(cursor_plugin.join(".cursor-plugin"))
                .and_then(|()| fs::create_dir_all(cursor_plugin.join("hooks")))
                .map_err(|error_value| error_value.to_string())?;
            multplx_core::filesystem::atomic_replace(
                cursor_plugin.join(".cursor-plugin/plugin.json"),
                serde_json::to_string(&serde_json::json!({
                    "name": format!("multplx-turnend-{}", request.id),
                    "version": "1.0.0",
                    "description": "Private Multplx actor turn-end signal.",
                    "hooks": "./hooks/hooks.json"
                }))
                .map_err(|error_value| error_value.to_string())?
                .as_bytes(),
                0o600,
            )
            .map_err(|error_value| error_value.to_string())?;
            multplx_core::filesystem::atomic_replace(
                cursor_plugin.join("hooks/hooks.json"),
                br#"{"version":1,"hooks":{"stop":[{"command":"${CURSOR_PLUGIN_ROOT}/hooks/stop.sh","loop_limit":1}]}}"#,
                0o600,
            )
            .map_err(|error_value| error_value.to_string())?;
            let stop = format!(
                "#!/usr/bin/env bash\nset -eu\ncat >/dev/null\ntouch '{}'\nprintf '%s\\n' '{{}}'\n",
                context
                    .state
                    .join(format!("{}.turn-ended", request.id))
                    .display()
            );
            multplx_core::filesystem::atomic_replace(
                cursor_plugin.join("hooks/stop.sh"),
                stop.as_bytes(),
                0o700,
            )
            .map_err(|error_value| error_value.to_string())?;
        }
        let mcp_config = task_tmp.join("report-mcp.json");
        let report_home = if request.kind == "daemon" {
            request.home.clone()
        } else {
            logical_home.clone()
        };
        let mcp_json = serde_json::json!({"mcpServers":{"multplx_status":{"type":"stdio","command":report_server,"args":[],"env":{"MX_TASK_ID":request.id,"MX_HOME":report_home,"MX_REPORT_STATE_OVERRIDE":context.state}}}});
        multplx_core::filesystem::atomic_replace(
            &mcp_config,
            serde_json::to_string(&mcp_json)
                .map_err(|error_value| error_value.to_string())?
                .as_bytes(),
            0o600,
        )
        .map_err(|error_value| error_value.to_string())?;
        let model = if request.model == "default" {
            String::new()
        } else {
            format!("--model '{}' ", request.model)
        };
        let effort = if request.effort == "default" {
            String::new()
        } else {
            format!("--effort '{}' ", request.effort)
        };
        let codex_effort = if request.effort == "default" || request.effort == "max" {
            String::new()
        } else {
            format!("-c 'model_reasoning_effort=\"{}\"' ", request.effort)
        };
        let codex_mcp = format!(
            "-c 'mcp_servers.multplx_status={{command=\"{}\",args=[],env={{MX_TASK_ID=\"{}\",MX_HOME=\"{}\",MX_REPORT_STATE_OVERRIDE=\"{}\"}}}}' ",
            report_server.display(),
            request.id,
            request.home.display(),
            context.state.display()
        );
        let launch = match request.harness.as_str() {
            "codex" => format!(
                "MX_HOME='{}' MX_TASK_ID='{}' MX_REPORT_STATE_OVERRIDE='{}' codex {codex_mcp}{model}{codex_effort}--dangerously-bypass-approvals-and-sandbox \"$('{}' encode launch-brief < '{}')\"",
                request.home.display(),
                request.id,
                context.state.display(),
                source_root.join("bin/mx-operational-input.sh").display(),
                brief.display()
            ),
            "claude" => format!(
                "MX_HOME='{}' MX_TASK_ID='{}' MX_REPORT_STATE_OVERRIDE='{}' CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false claude --dangerously-skip-permissions --mcp-config '{}' {model}{effort}\"$('{}' encode launch-brief < '{}')\"",
                request.home.display(),
                request.id,
                context.state.display(),
                mcp_config.display(),
                source_root.join("bin/mx-operational-input.sh").display(),
                brief.display()
            ),
            "pi" => format!(
                "MX_HOME='{}' MX_TASK_ID='{}' MX_REPORT_STATE_OVERRIDE='{}' pi {model}{}{}\"$('{}' encode launch-brief < '{}')\"",
                request.home.display(),
                request.id,
                context.state.display(),
                if request.effort != "default" {
                    format!("--thinking '{}' ", request.effort)
                } else {
                    String::new()
                },
                if request.kind == "daemon" {
                    format!(
                        "-e '{}' -e '{}' ",
                        request
                            .home
                            .join(".pi/extensions/mx-primary-turnend-guard.ts")
                            .display(),
                        request
                            .home
                            .join(".pi/extensions/mx-primary-pi-watch.ts")
                            .display()
                    )
                } else {
                    format!(
                        "-e '{}.pi-ext.ts' ",
                        context.state.join(&request.id).display()
                    )
                },
                source_root.join("bin/mx-operational-input.sh").display(),
                brief.display()
            ),
            "cursor" => {
                let cursor_model = if request.model == "default" {
                    String::new()
                } else if request.effort == "default" || request.model.contains('[') {
                    format!("--model '{}' ", request.model)
                } else {
                    format!("--model '{}[effort={}]' ", request.model, request.effort)
                };
                format!(
                    "MX_HOME='{}' MX_TASK_ID='{}' MX_REPORT_STATE_OVERRIDE='{}' agent --sandbox enabled --trust '{}' {cursor_model}\"$('{}' encode launch-brief < '{}')\"",
                    request.home.display(),
                    request.id,
                    context.state.display(),
                    cursor_plugin.display(),
                    source_root.join("bin/mx-operational-input.sh").display(),
                    brief.display()
                )
            }
            other if other.contains(' ') => format!(
                "MX_HOME='{}' MX_TASK_ID='{}' MX_REPORT_STATE_OVERRIDE='{}' {other}",
                request.home.display(),
                request.id,
                context.state.display()
            ),
            other => return Err(format!("unknown harness '{other}'")),
        };
        let launch = if request.kind == "daemon" {
            format!(
                "MX_ROOT_OVERRIDE= MX_STATE_OVERRIDE= MX_DATA_OVERRIDE= MX_PROJECTS_OVERRIDE= MX_CONFIG_OVERRIDE= {launch}"
            )
        } else {
            launch
        };
        let launch = format!(
            "env -u GH_TOKEN -u GITHUB_TOKEN -u GH_ENTERPRISE_TOKEN -u GITHUB_ENTERPRISE_TOKEN -u GH_CONFIG_DIR -u SSH_AUTH_SOCK -u MX_DELIVERY_GH_TOKEN -u MX_DELIVERY_GH_CONFIG_DIR GH_PROMPT_DISABLED=1 GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/usr/bin/false SSH_ASKPASS=/usr/bin/false GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=credential.helper GIT_CONFIG_VALUE_0= GIT_CONFIG_KEY_1=remote.origin.pushurl GIT_CONFIG_VALUE_1=/dev/null/multplx-agent-no-push GIT_SSH_COMMAND='ssh -o BatchMode=yes -o IdentityAgent=none -o IdentitiesOnly=yes -o IdentityFile=/dev/null' {launch}"
        );
        match target.backend() {
            BackendName::Tmux => {
                let mut backend = multplx_backend::tmux::TmuxBackend::system();
                backend
                    .send_literal(&target, &launch)
                    .and_then(|()| backend.send_key(&target, "Enter"))
            }
            BackendName::Herdr => {
                let mut backend = herdr_backend();
                backend
                    .send_literal(&target, &launch)
                    .and_then(|()| backend.send_key(&target, "Enter"))
            }
            BackendName::Cmux => {
                let mut backend = multplx_backend::cmux::CmuxBackend::system();
                backend
                    .send_literal(&target, &launch)
                    .and_then(|()| backend.send_key(&target, "Enter"))
            }
        }
        .map_err(|error_value| error_value.to_string())?;
        let task = multplx_core::identifiers::TaskId::parse(&request.id)
            .map_err(|error_value| error_value.to_string())?;
        let timestamp = {
            let now = time::OffsetDateTime::now_utc();
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                now.year(),
                u8::from(now.month()),
                now.day(),
                now.hour(),
                now.minute(),
                now.second()
            )
        };
        let detail = serde_json::json!({
            "kind": request.kind,
            "backend": request.backend,
            "worktree": actor_worktree,
            "branch": request.single_checkout_base_branch.as_deref().unwrap_or("")
        });
        if let Some(warning) = multplx_core::journal::JournalWriter::new(&context.state).try_emit(
            &task,
            multplx_core::journal::JournalEvent::TaskSpawned,
            &detail,
            "mx-spawn",
            &timestamp,
        ) {
            eprintln!("{warning}");
        }
        if request.kind == "daemon"
            && !multplx_domain::inheritance::discard_pending(
                &request.home,
                Some(&request.id),
                Some(&context.home),
            )
        {
            if multplx_domain::inheritance::quarantine_pending(
                &request.home,
                Some(&request.id),
                Some(&context.home),
            ) {
                eprintln!(
                    "CONFIG_REREAD: daemon {}: quarantined pre-relaunch generations after cleanup failure",
                    request.id
                );
            } else {
                eprintln!(
                    "CONFIG_REREAD: daemon {}: cleanup failed; pre-relaunch generations were force-cleared where possible",
                    request.id
                );
            }
        }
        Ok(named_endpoint)
    })();
    drop(lock);
    match result {
        Ok(endpoint) => {
            drop(presentation_lock);
            if let (Some(store), Some(request_id)) = (
                single_checkout_store.as_ref(),
                single_checkout_request.as_deref(),
            ) && let Err(error_value) = store.result(
                request_id,
                true,
                &format!(
                    "single-checkout task {} launched in {}",
                    request.id,
                    request.project.display()
                ),
            ) {
                eprintln!("error: could not record single-checkout result: {error_value}");
                return 1;
            }
            let reported_worktree =
                fs::read_to_string(context.state.join(format!("{}.meta", request.id)))
                    .ok()
                    .and_then(|text| {
                        text.lines()
                            .find_map(|line| line.strip_prefix("worktree="))
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| request.project.display().to_string());
            println!(
                "spawned {} harness={} kind={} mode={} yolo=off window={endpoint} worktree={}",
                request.id,
                request.harness,
                request.kind,
                if request.kind == "daemon" {
                    "daemon"
                } else {
                    "deep-review"
                },
                reported_worktree
            );
            0
        }
        Err(error_value) => {
            if let Some((session, pane)) = projected_endpoint.as_ref() {
                let _ = herdr_backend().close_pane_focus_preserving(session, pane, None);
            }
            drop(presentation_lock);
            if projected_endpoint.is_none()
                && let Some(target) = created_target.as_ref()
            {
                match target.backend() {
                    BackendName::Tmux => {
                        let _ = multplx_backend::tmux::TmuxBackend::system().kill_verified(target);
                    }
                    BackendName::Herdr => {
                        let _ = herdr_backend().kill_verified(target);
                    }
                    BackendName::Cmux => {
                        let _ = multplx_backend::cmux::CmuxBackend::system().kill_verified(target);
                    }
                }
            }
            let _ = fs::remove_file(context.state.join(format!("{}.meta", request.id)));
            if let Some(record) = request.single_checkout_record.as_deref() {
                let _ = fs::remove_file(record);
            }
            if let (Some(store), Some(request_id)) = (
                single_checkout_store.as_ref(),
                single_checkout_request.as_deref(),
            ) {
                let _ = store.result(
                    request_id,
                    false,
                    &format!("single-checkout spawn failed: {error_value}"),
                );
            }
            eprintln!("error: {error_value}");
            1
        }
    }
}

fn run_teardown(args: &[OsString]) -> i32 {
    if multplx_core::gate_refuse::is_gate_agent(
        std::env::var_os("DEEP_REVIEW_GATE").is_some(),
        std::env::var("MX_GATE_REFUSE_BYPASS").as_deref() == Ok("1"),
    ) {
        eprintln!("{}", multplx_core::gate_refuse::REFUSAL_MESSAGE);
        return i32::from(multplx_core::gate_refuse::REFUSAL_EXIT);
    }
    let (root, home, data) = active_paths();
    let context = multplx_domain::lifecycle::teardown::Context {
        root,
        state: std::env::var_os("MX_STATE_OVERRIDE")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("state")),
        home,
        data,
    };
    let output = if let [raw_id, flag, raw_request] = args
        && flag == "--override"
    {
        let Some(id) = raw_id.to_str() else {
            eprintln!("error: task id is not valid UTF-8");
            return 2;
        };
        let Some(request) = raw_request.to_str() else {
            eprintln!("error: override request id is not valid UTF-8");
            return 2;
        };
        let binding_value = match authority::cleanup_binding(id) {
            Ok(value) => value,
            Err(error_value) => {
                eprintln!("error: {error_value}");
                return 1;
            }
        };
        let field = |name: &str| {
            binding_value
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        };
        let binding = multplx_domain::maintainer_override::Binding {
            boundary: field("boundary"),
            task: field("task"),
            project: field("project"),
            operation: field("operation"),
            target: field("target"),
            expected_state_digest: field("expected_state_digest"),
        };
        let store = multplx_domain::maintainer_override::OverrideStore::new(&context.state);
        if let Err(error_value) = store.consume(request, &binding) {
            eprintln!("error: {error_value}");
            return 1;
        }
        let output =
            multplx_domain::lifecycle::teardown::run_override(id, &context, kill_teardown_endpoint);
        let detail = format!(
            "cleanup.discard-unlanded teardown exited with status {}",
            output.status
        );
        if let Err(error_value) = store.result(request, output.status == 0, &detail) {
            eprintln!("warning: could not record teardown override outcome: {error_value}");
            if output.status == 0 {
                return 1;
            }
        }
        output
    } else if args.iter().any(|value| value == "--force") {
        multplx_domain::lifecycle::teardown::Output {
            status: 2,
            stdout: String::new(),
            stderr: "REFUSED: --force is not an authority source; request and consume an exact cleanup.discard-unlanded grant.\n".to_owned(),
        }
    } else {
        let scout_preflight = (|| -> Result<(), String> {
            let [raw_id] = args else {
                return Ok(());
            };
            let Some(id) = raw_id.to_str() else {
                return Ok(());
            };
            let meta = context.state.join(format!("{id}.meta"));
            let raw = match fs::read_to_string(&meta) {
                Ok(raw) => raw,
                Err(_) => return Ok(()),
            };
            let is_scout = raw.lines().rev().any(|line| line == "kind=scout");
            if !is_scout {
                return Ok(());
            }
            let report = context.data.join(id).join("report.md");
            if !report.is_file() {
                return Err(format!(
                    "REFUSED: scout task {id} has no report at {}.",
                    report.display()
                ));
            }
            authority::verify_decision_completion(id).map_err(|_| {
                format!(
                    "REFUSED: scout task {id} has not passed the unresolved-decision completion gate."
                )
            })
        })();
        match scout_preflight {
            Ok(()) => {
                multplx_domain::lifecycle::teardown::run(args, &context, kill_teardown_endpoint)
            }
            Err(message) => multplx_domain::lifecycle::teardown::Output {
                status: 1,
                stdout: String::new(),
                stderr: format!("{message}\n"),
            },
        }
    };
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    output.status
}

fn kill_teardown_endpoint(meta: &Path) -> Result<(), String> {
    use multplx_backend::facade::{
        BackendName, BackendTarget, KillOutcome, RuntimeBackend, backend_of_meta, target_of_meta,
    };
    if !meta.exists() {
        return Ok(());
    }
    let backend = backend_of_meta(meta).map_err(|error_value| error_value.to_string())?;
    let Some(endpoint) = target_of_meta(meta).map_err(|error_value| error_value.to_string())?
    else {
        return Ok(());
    };
    let id = meta
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("task metadata path is not valid UTF-8: {}", meta.display()))?;
    let state = meta
        .parent()
        .ok_or("task metadata has no state directory")?;
    if backend == BackendName::Herdr {
        let raw = fs::read_to_string(meta).unwrap_or_default();
        let value = |key: &str| {
            raw.lines()
                .rev()
                .find_map(|line| line.strip_prefix(&format!("{key}=")))
                .unwrap_or_default()
        };
        let journal = state.join(format!("{id}.herdr-presentation"));
        let session = value("herdr_session");
        let workspace = value("herdr_workspace_id");
        let pane = value("herdr_pane_id");
        if journal.exists()
            && !session.is_empty()
            && !workspace.is_empty()
            && !pane.is_empty()
            && endpoint == format!("{session}:{pane}")
        {
            let mut herdr = multplx_backend::herdr::HerdrBackend::system();
            if herdr.projection_endpoint_matches_journal(session, workspace, &journal, id) {
                let _ = herdr.close_pane_focus_preserving(session, pane, None);
                if herdr.pane_agent_state(session, pane)
                    == multplx_backend::herdr::PaneAgentState::Dead
                {
                    let _ = fs::remove_file(&journal);
                } else {
                    eprintln!(
                        "warning: exact herdr task-pane close could not be confirmed for {id}; retaining the presentation journal and attempting no workspace cleanup"
                    );
                }
                let _ = multplx_backend::herdr::clear_transition(state, &endpoint);
                return Ok(());
            }
        }
    }
    let target = BackendTarget::new(backend, endpoint, Some(format!("mx-{id}")))
        .map_err(|error_value| error_value.to_string())?;
    let outcome = match backend {
        BackendName::Tmux => multplx_backend::tmux::TmuxBackend::system().kill_verified(&target),
        BackendName::Herdr => multplx_backend::herdr::HerdrBackend::system().kill_verified(&target),
        BackendName::Cmux => multplx_backend::cmux::CmuxBackend::system().kill_verified(&target),
    };
    if backend == BackendName::Herdr {
        let _ = multplx_backend::herdr::clear_transition(state, target.endpoint());
    }
    match outcome {
        KillOutcome::Gone => Ok(()),
        KillOutcome::StillPresent => Err(format!(
            "runtime endpoint {} is still present after teardown kill",
            target.endpoint()
        )),
        KillOutcome::Unknown => {
            eprintln!(
                "warning: runtime endpoint {} post-kill state could not be verified",
                target.endpoint()
            );
            Ok(())
        }
    }
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
    let mut resolver = multplx_backend::tmux::TmuxBackend::system();
    let resolved = match multplx_backend::facade::resolve_selector(target, &state, &mut resolver) {
        Ok(resolved) => resolved,
        Err(error) => return backend_error(error),
    };
    let request = multplx_backend::facade::CaptureRequest {
        target: resolved,
        lines,
        byte_limit: 256 * 1024,
    };
    let result = match request.target.backend() {
        multplx_backend::facade::BackendName::Tmux => resolver.capture(&request),
        multplx_backend::facade::BackendName::Herdr => {
            multplx_backend::herdr::HerdrBackend::system().capture(&request)
        }
        multplx_backend::facade::BackendName::Cmux => {
            multplx_backend::cmux::CmuxBackend::system().capture(&request)
        }
    };
    match result {
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
    if file_name == "multplx" {
        return Some(OsString::from("launcher"));
    }
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
    fn spawn_headroom_parser_rejects_malformed_requests_before_runtime_mutation() {
        use std::os::unix::ffi::OsStringExt;

        assert_eq!(
            park_spawn_if_at_limit(&args(&["--daemon"]), None).expect("daemon"),
            None
        );
        assert!(
            park_spawn_if_at_limit(&[OsString::from_vec(vec![0xff])], None)
                .expect_err("utf8")
                .contains("not valid UTF-8")
        );
        assert!(
            park_spawn_if_at_limit(&args(&["--harness"]), None)
                .expect_err("value")
                .contains("requires a value")
        );
        assert!(
            park_spawn_if_at_limit(&args(&["--unknown"]), None)
                .expect_err("unknown")
                .contains("unsupported native daemon spawn option")
        );
        assert!(
            park_spawn_if_at_limit(&args(&[]), None)
                .expect_err("missing")
                .contains("invalid spawn request")
        );
        assert!(
            park_spawn_if_at_limit(&args(&["../bad", "/tmp/project"]), None)
                .expect_err("id")
                .contains("invalid spawn request")
        );
        assert!(
            park_spawn_if_at_limit(&args(&["task"]), None)
                .expect_err("project")
                .contains("invalid spawn request")
        );
        assert!(
            park_spawn_if_at_limit(&args(&["task", "/tmp/project", "pi", "extra"]), None)
                .expect_err("positionals")
                .contains("invalid spawn request")
        );
    }

    #[test]
    fn daemon_report_formats_empty_notes_and_rejects_unusable_parent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let status = temp.path().join("state/task.status");
        assert_eq!(
            run_daemon_report(&args(&[
                status.to_str().unwrap(),
                "working",
                "0123456789abcdef",
                ""
            ])),
            0
        );
        assert_eq!(
            run_daemon_report(&args(&[
                "--doc",
                status.to_str().unwrap(),
                "done",
                "0123456789abcdef",
                "data/report.md"
            ])),
            0
        );
        let raw = fs::read_to_string(&status).expect("status");
        assert!(raw.contains("working [corr=0123456789abcdef]: (via-helper)"));
        assert!(raw.contains("done [corr=0123456789abcdef]: data/report.md (via-helper)"));
        let blocker = temp.path().join("not-a-directory");
        fs::write(&blocker, "x").expect("blocker");
        assert_eq!(
            run_daemon_report(&args(&[
                blocker.join("status").to_str().unwrap(),
                "done",
                "0123456789abcdef",
                "note"
            ])),
            1
        );
    }

    #[test]
    fn helper_parsers_cover_config_registry_and_runtime_path_fallbacks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("daemons.md");
        fs::write(
            &registry,
            "- first - live (home: /tmp/first; harness: pi)\n- first - live (home: /tmp/latest; harness: codex)\n- malformed\n",
        )
        .expect("registry");
        assert_eq!(last_field("key=old\nother=x\nkey=new\n", "key"), "new");
        assert_eq!(last_field("other=x\n", "key"), "");
        assert_eq!(registry_home(&registry, "first"), "/tmp/latest");
        assert_eq!(registry_home(&registry, "missing"), "");
        assert_eq!(registry_home(&temp.path().join("missing"), "first"), "");

        let state = temp.path().join("state");
        fs::create_dir(&state).expect("state");
        fs::write(state.join("actor.meta"), "kind=delivery\nhome=/tmp/no\n").expect("actor");
        fs::write(
            state.join("daemon.meta"),
            "kind=daemon\nhome=/tmp/daemon\nhome=/tmp/latest\n",
        )
        .expect("daemon");
        fs::write(state.join("fallback.meta"), "kind=daemon\n").expect("fallback");
        fs::write(
            &registry,
            "- fallback - live (home: /tmp/fallback; harness: pi)\n",
        )
        .expect("registry");
        let records = live_daemons(&state, &registry);
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .any(|(id, home, _)| id == "daemon" && home == "/tmp/latest")
        );
        assert!(
            records
                .iter()
                .any(|(id, home, _)| id == "fallback" && home == "/tmp/fallback")
        );
        assert_eq!(runtime_root(temp.path()), temp.path());
    }

    #[test]
    fn run_helpers_reject_unknown_backend_shapes_without_external_execution() {
        use std::os::unix::ffi::OsStringExt;

        assert_eq!(run_launch_harness(&[]), 2);
        assert_eq!(run_launch_harness(&[OsString::from_vec(vec![0xff])]), 2);
        assert_eq!(run_cmux(&args(&["parse-target", "bad"])), 1);
        assert_eq!(run_cmux(&args(&["normalize-key", "C-c"])), 0);
        assert_eq!(run_cmux(&args(&["scoped-title"])), 1);
        assert_eq!(run_cmux(&args(&["unknown"])), 1);
        assert_eq!(run_herdr(&args(&["unknown"])), 1);
        assert_eq!(run_headroom(&args(&["--json", "extra"])), 1);
        assert_eq!(run_headroom(&args(&["--queue", "extra"])), 1);
        assert_eq!(run_headroom(&args(&["--queue-cancel"])), 1);
        assert_eq!(run_headroom(&args(&["--queue-drain", "extra"])), 1);
        assert_eq!(run_headroom(&args(&["--queue-add", "task"])), 1);
        assert_eq!(
            run_headroom(&args(&["--queue-add", "task", "/tmp", "--bad"])),
            1
        );
        assert_eq!(run_headroom(&args(&["unknown"])), 1);
    }

    #[test]
    fn spawn_batch_parser_rejects_partial_pairs_and_missing_shared_values() {
        assert_eq!(run_spawn(&args(&["task="])), 1);
        assert_eq!(run_spawn(&args(&["=project"])), 1);
        assert_eq!(run_spawn(&args(&["task=/tmp", "stray"])), 1);
        assert_eq!(run_spawn(&args(&["task=/tmp", "--model"])), 1);
        assert_eq!(run_spawn(&args(&["task=/tmp", "--backend"])), 1);
    }

    #[test]
    fn spawn_single_request_preflight_rejects_before_backend_creation() {
        assert_eq!(run_spawn(&[]), 1);
        assert_eq!(run_spawn(&args(&["--single-checkout"])), 1);
        assert_eq!(run_spawn(&args(&["task", "/missing", "--daemon"])), 1);
        assert_eq!(
            run_spawn(&args(&[
                "task",
                "/missing",
                "--daemon",
                "--backend",
                "cmux"
            ])),
            1
        );
        assert_eq!(
            run_spawn(&args(&["task", "/missing", "--backend", "bad"])),
            1
        );
    }

    #[test]
    fn session_and_system_view_dispatch_refuse_unknown_or_extra_arguments() {
        assert_eq!(run_session("not-an-entry", &[]), 2);
        assert_eq!(run_session("mx-bootstrap.sh", &args(&["extra"])), 2);
        assert_eq!(run_session("mx-doctor.sh", &args(&["extra"])), 2);
        assert_eq!(run_session("mx-status-snapshot.sh", &args(&["extra"])), 2);
        assert_eq!(run_session("mx-system-snapshot.sh", &args(&["extra"])), 2);
        assert_eq!(run_system_view(&args(&["--help"])), 0);
        assert_eq!(run_system_view(&args(&["extra"])), 2);
    }

    #[test]
    fn teardown_endpoint_metadata_errors_are_typed_and_missing_is_idempotent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let missing = temp.path().join("missing.meta");
        assert!(kill_teardown_endpoint(&missing).is_ok());
        let no_endpoint = temp.path().join("no-endpoint.meta");
        fs::write(&no_endpoint, "kind=delivery\nbackend=tmux\n").expect("meta");
        assert!(kill_teardown_endpoint(&no_endpoint).is_ok());
        let invalid_backend = temp.path().join("invalid.meta");
        fs::write(
            &invalid_backend,
            "kind=delivery\nbackend=spaceship\nwindow=target\n",
        )
        .expect("meta");
        assert!(
            kill_teardown_endpoint(&invalid_backend)
                .expect_err("backend")
                .contains("unknown backend")
        );
        assert_eq!(run_teardown(&args(&["task", "--force"])), 2);
    }

    #[test]
    fn harness_detection_accepts_only_verified_bounded_output() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).expect("bin");
        let script = bin.join("mx-harness.sh");
        let write = |body: &str| {
            fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("script");
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("mode");
        };
        write("printf 'codex\\n'");
        assert_eq!(detect_primary_harness(temp.path()), "codex");
        write("printf 'spaceship\\n'");
        assert_eq!(detect_primary_harness(temp.path()), "unknown");
        write("exit 1");
        assert_eq!(detect_primary_harness(temp.path()), "unknown");
        fs::remove_file(script).expect("remove");
        assert_eq!(detect_primary_harness(temp.path()), "unknown");
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

//! Command-line dispatch for the Multplx Rust runtime.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
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

fn environment_path(primary: &str, fallback: &str) -> PathBuf {
    std::env::var_os(primary)
        .or_else(|| std::env::var_os(fallback))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn active_paths() -> (PathBuf, PathBuf, PathBuf) {
    let root = environment_path("MX_ROOT_OVERRIDE", "MX_HOME");
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
    use super::multicall_alias;
    use std::ffi::{OsStr, OsString};

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
    }
}

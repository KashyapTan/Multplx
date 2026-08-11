//! Command-line dispatch for the shadow Rust runtime.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use multplx_core::process::SystemProcessProbe;

/// The Portion 01 binary with Portion 02 shadow compatibility surfaces.
#[derive(Debug, Parser)]
#[command(
    name = "mx",
    version,
    about = "Multplx Rust shadow runtime (no production commands enabled)",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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

    /// Runs the selected shadow-only command.
    pub fn run(self) -> i32 {
        let result = match self.command {
            Command::ShadowDiagnostic => {
                let boundaries = [
                    multplx_core::SHADOW_BOUNDARY,
                    multplx_domain::SHADOW_BOUNDARY,
                    multplx_backend::SHADOW_BOUNDARY,
                    multplx_services::SHADOW_BOUNDARY,
                ];
                debug_assert_eq!(boundaries.len(), 4);
                println!("multplx rust shadow: ready");
                Ok(0)
            }
            Command::Primitive { command } => run_primitive(command),
        };
        match result {
            Ok(status) => status,
            Err(error) => {
                eprintln!("mx primitive: {error}");
                1
            }
        }
    }
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

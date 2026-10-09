//! Exact execution proof for the installer's consent boundary.
use crate::command::{CommandRequest, CommandRunner, SystemCommandRunner};
use crate::facade::{BackendName, BackendTarget, KillOutcome, RuntimeBackend};
use multplx_core::locks::DirectoryLock;
use multplx_core::process::{ProcessIdentity, ProcessProbe, SystemProcessProbe};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedEndpoint {
    pub task: String,
    pub home: PathBuf,
    pub backend_home: PathBuf,
    pub endpoint: String,
    pub backend: String,
    root: PathBuf,
    cwd: PathBuf,
    proof: Proof,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
enum Proof {
    Tmux {
        window: String,
        pane: String,
        server: ProcessIdentity,
        process: ProcessIdentity,
    },
    HerdrProjection(
        Box<crate::herdr_presentation::ProjectionBinding>,
        HerdrTerminal,
    ),
    HerdrFlat(HerdrTerminal),
    Scoped,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
struct HerdrTerminal {
    terminal: String,
    pane: String,
    tab: String,
    workspace: String,
    process: Option<ProcessIdentity>,
}
fn tmux(args: &[&str]) -> Result<String, String> {
    let request = CommandRequest::new("tmux", args.iter().map(OsString::from));
    let output = SystemCommandRunner
        .run(&request)
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("tmux exact execution probe failed".into());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}
fn scoped_backend(
    name: BackendName,
    root: &Path,
    home: &Path,
    endpoint: &str,
) -> Box<dyn RuntimeBackend> {
    match name {
        BackendName::Herdr => Box::new(crate::herdr::HerdrBackend::new(
            SystemCommandRunner,
            std::env::var_os("MX_HERDR_BIN").unwrap_or_else(|| "herdr".into()),
            endpoint.split(':').next().unwrap_or("default"),
            home.to_path_buf(),
        )),
        BackendName::Cmux => Box::new(crate::cmux::CmuxBackend::system_for_home(
            root.into(),
            home.into(),
            home.join("config"),
        )),
        BackendName::Tmux => Box::new(crate::tmux::TmuxBackend::system()),
    }
}
fn herdr_process(home: &Path, endpoint: &str, cwd: &Path) -> Result<HerdrTerminal, String> {
    let (session, pane) = endpoint.split_once(':').ok_or("invalid Herdr endpoint")?;
    let mut backend = crate::herdr::HerdrBackend::new(
        SystemCommandRunner,
        std::env::var_os("MX_HERDR_BIN").unwrap_or_else(|| "herdr".into()),
        session,
        home.to_path_buf(),
    );
    let output = backend
        .scoped_cli(session, &["pane".into(), "get".into(), pane.into()])
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Herdr exact pane identity probe failed".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    if value
        .pointer("/result/pane/pane_id")
        .and_then(serde_json::Value::as_str)
        != Some(pane)
        || std::fs::canonicalize(
            value
                .pointer("/result/pane/foreground_cwd")
                .and_then(serde_json::Value::as_str)
                .ok_or("Herdr pane cwd is unavailable")?,
        )
        .ok()
        .as_ref()
            != Some(&cwd.to_path_buf())
    {
        return Err("Herdr pane identity/directory does not match its recorded execution".into());
    }
    let field = |key: &str| -> Result<String, String> {
        value
            .pointer(&format!("/result/pane/{key}"))
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("Herdr pane lacks stable {key}; retained"))
    };
    // Current Herdr exposes native process ownership through process-info,
    // independently of the terminal's display/agent-status response.
    let process_info = backend
        .scoped_cli(
            session,
            &[
                "pane".into(),
                "process-info".into(),
                "--pane".into(),
                pane.into(),
            ],
        )
        .map_err(|e| e.to_string())?;
    if !process_info.status.success() {
        return Err("Herdr pane process identity is unavailable; retained".into());
    }
    let process_info: serde_json::Value =
        serde_json::from_slice(&process_info.stdout).map_err(|e| e.to_string())?;
    if process_info
        .pointer("/result/process_info/pane_id")
        .and_then(serde_json::Value::as_str)
        != Some(pane)
    {
        return Err("Herdr process-info pane binding changed; retained".into());
    }
    let pid = process_info
        .pointer("/result/process_info/shell_pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("Herdr pane has no stable native shell identity; retained")?;
    let process = Some(
        SystemProcessProbe::default()
            .identity(pid)
            .map_err(|e| e.to_string())?,
    );
    Ok(HerdrTerminal {
        terminal: field("terminal_id")?,
        pane: field("pane_id")?,
        tab: field("tab_id")?,
        workspace: field("workspace_id")?,
        process,
    })
}
/// Bind a live execution to its recorded task, exact allocation and physical home.
/// A mere matching endpoint name is insufficient authority to stop it.
pub fn inspect(
    root: &Path,
    home: &Path,
    backend_home: &Path,
    task: &str,
    backend: &str,
    endpoint: &str,
    cwd: &Path,
) -> Result<VerifiedEndpoint, String> {
    let name = BackendName::parse(backend).map_err(|e| e.to_string())?;
    let label = format!("mx-{task}");
    let target =
        BackendTarget::new(name, endpoint, Some(label.clone())).map_err(|e| e.to_string())?;
    let cwd = std::fs::canonicalize(cwd)
        .map_err(|e| format!("recorded execution directory cannot be verified: {e}"))?;
    let proof = if name == BackendName::Tmux {
        let endpoint = if endpoint.contains(':') || endpoint.starts_with('@') {
            endpoint.to_owned()
        } else {
            format!("broker:{endpoint}")
        };
        let row = tmux(&[
            "display-message",
            "-p",
            "-t",
            &endpoint,
            // tmux converts control separators to underscores under LC_ALL=C.
            // An ambiguous directory containing this printable separator refuses proof.
            "#{window_id}|mx-upgrade|#{pane_id}|mx-upgrade|#{window_name}|mx-upgrade|#{pane_current_path}|mx-upgrade|#{pid}|mx-upgrade|#{pane_pid}|mx-upgrade|#{window_panes}",
        ])?;
        let fields: Vec<_> = row.trim_end().split("|mx-upgrade|").collect();
        if fields.len() != 7
            || !fields[0].starts_with('@')
            || !fields[1].starts_with('%')
            || fields[2] != label
            || fields[6] != "1"
            || std::fs::canonicalize(fields[3]).ok().as_ref() != Some(&cwd)
        {
            return Err(format!(
                "{backend} endpoint {endpoint} lacks an exact single-pane task/allocation binding; retained"
            ));
        }
        let probe = SystemProcessProbe::default();
        let process = |s: &str| -> Result<ProcessIdentity, String> {
            probe
                .identity(s.parse().map_err(|_| "invalid backend PID")?)
                .map_err(|e| e.to_string())
        };
        Proof::Tmux {
            window: fields[0].into(),
            pane: fields[1].into(),
            server: process(fields[4])?,
            process: process(fields[5])?,
        }
    } else {
        let mut provider = scoped_backend(name, root, backend_home, endpoint);
        let journal = home
            .join("state")
            .join(format!("{task}.herdr-presentation"));
        let matching_journal = if name == BackendName::Herdr && journal.exists() {
            match crate::herdr_presentation::read_journal(&journal, task)
                .map_err(|e| e.to_string())?
            {
                crate::herdr_presentation::ProjectionJournal::V2(binding) => {
                    format!("{}:{}", binding.session, binding.pane_id) == endpoint
                }
                _ => return Err("Herdr projection has no exact execution binding; retained".into()),
            }
        } else {
            false
        };
        if matching_journal {
            let crate::herdr_presentation::ProjectionJournal::V2(binding) =
                crate::herdr_presentation::read_journal(&journal, task)
                    .map_err(|e| e.to_string())?
            else {
                return Err("Herdr projection has no exact execution binding; retained".into());
            };
            let mut herdr = crate::herdr::HerdrBackend::new(
                SystemCommandRunner,
                std::env::var_os("MX_HERDR_BIN").unwrap_or_else(|| "herdr".into()),
                &binding.session,
                backend_home.to_path_buf(),
            );
            if binding.home != backend_home
                || format!("{}:{}", binding.session, binding.pane_id) != endpoint
                || !herdr.projection_live_binding_matches(&binding.session, &binding)
            {
                return Err("Herdr projection execution/home binding changed; retained".into());
            }
            if herdr
                .focus_snapshot(&binding.session)
                .map_err(|e| e.to_string())?
                .tab_id
                == binding.tab_id
            {
                return Err("Herdr projected task is the active tab; switch to another tab before retrying upgrade so its presentation owner can preserve focus".into());
            }
            Proof::HerdrProjection(binding, herdr_process(backend_home, endpoint, &cwd)?)
        } else {
            let matches = provider
                .list_live(None)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|entry| entry.target.endpoint() == endpoint && entry.label == label)
                .count();
            if matches != 1 {
                return Err(format!(
                    "{backend} endpoint {endpoint} lacks a unique managed home/task binding; retained"
                ));
            }
            if name == BackendName::Cmux {
                let mut cmux = crate::cmux::CmuxBackend::system_for_home(
                    root.into(),
                    backend_home.into(),
                    backend_home.join("config"),
                );
                if !cmux
                    .single_surface_execution(&target)
                    .map_err(|e| e.to_string())?
                {
                    return Err(
                        "cmux workspace contains other or uncertain surfaces; retained".into(),
                    );
                }
            }
            if name == BackendName::Herdr {
                Proof::HerdrFlat(herdr_process(backend_home, endpoint, &cwd)?)
            } else {
                Proof::Scoped
            }
        }
    };
    if name == BackendName::Cmux {
        let mut provider = scoped_backend(name, root, backend_home, endpoint);
        if std::fs::canonicalize(provider.current_path(&target).map_err(|e| e.to_string())?)
            .ok()
            .as_ref()
            != Some(&cwd)
        {
            return Err(
                "backend execution directory differs from the recorded allocation; retained".into(),
            );
        }
    }
    Ok(VerifiedEndpoint {
        task: task.into(),
        home: home.into(),
        backend_home: backend_home.into(),
        endpoint: endpoint.into(),
        backend: backend.into(),
        root: root.into(),
        cwd,
        proof,
    })
}
/// Privacy-preserving execution identity for durable stop evidence.
pub fn execution_fingerprint(target: &VerifiedEndpoint) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(
        &target.root,
        &target.home,
        &target.backend_home,
        &target.task,
        &target.backend,
        &target.endpoint,
        &target.cwd,
        &target.proof,
    ))
    .map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
/// Recheck the entire approved execution proof, stop only its immutable endpoint,
/// and require exact disappearance. No records or homes are retired.
pub fn stop(approved: &VerifiedEndpoint) -> Result<(), String> {
    let current = inspect(
        &approved.root,
        &approved.home,
        &approved.backend_home,
        &approved.task,
        &approved.backend,
        &approved.endpoint,
        &approved.cwd,
    )?;
    if &current != approved {
        return Err(
            "approved runtime execution changed; rerun upgrade to review current users".into(),
        );
    }
    if let Proof::Tmux { window, .. } = &approved.proof {
        tmux(&["kill-window", "-t", window])?;
        let windows = tmux(&["list-windows", "-a", "-F", "#{window_id}"]);
        match windows {
            Ok(windows) if !windows.lines().any(|id| id == window) => Ok(()),
            _ => {
                // The final window may close the server. Its exact process lifetime
                // must also disappear before an unreadable inventory counts as gone.
                if let Proof::Tmux { server, .. } = &approved.proof {
                    let probe = SystemProcessProbe::default();
                    if !probe.is_alive(server.pid)
                        || probe.identity(server.pid).is_ok_and(|now| now != *server)
                    {
                        return Ok(());
                    }
                }
                Err("exact tmux window disappearance could not be verified; runtime was not replaced".into())
            }
        }
    } else {
        let name = BackendName::parse(&approved.backend).map_err(|e| e.to_string())?;
        let target = BackendTarget::new(
            name,
            &approved.endpoint,
            Some(format!("mx-{}", approved.task)),
        )
        .map_err(|e| e.to_string())?;
        let outcome = if let Proof::HerdrProjection(binding, _) = &approved.proof {
            let mut herdr = crate::herdr::HerdrBackend::new(
                SystemCommandRunner,
                std::env::var_os("MX_HERDR_BIN").unwrap_or_else(|| "herdr".into()),
                &binding.session,
                approved.backend_home.clone(),
            );
            let lock_path = herdr
                .presentation_session_lock_path(&binding.session)
                .map_err(|e| e.to_string())?;
            let _presentation_lock =
                DirectoryLock::try_acquire(&lock_path, &SystemProcessProbe::default()).map_err(
                    |e| format!("Herdr presentation session is busy; retry upgrade: {e}"),
                )?;
            let current = inspect(
                &approved.root,
                &approved.home,
                &approved.backend_home,
                &approved.task,
                &approved.backend,
                &approved.endpoint,
                &approved.cwd,
            )?;
            if &current != approved {
                return Err("approved Herdr presentation execution changed; rerun upgrade to review current users".into());
            }
            herdr
                .close_pane_focus_preserving(&binding.session, &binding.pane_id, None)
                .map_err(|e| e.to_string())?;
            match herdr.observe_target(&target) {
                Err(crate::facade::BackendError::Missing(_)) => KillOutcome::Gone,
                _ => KillOutcome::Unknown,
            }
        } else {
            scoped_backend(
                name,
                &approved.root,
                &approved.backend_home,
                &approved.endpoint,
            )
            .kill_verified(&target)
        };
        if outcome == KillOutcome::Gone {
            Ok(())
        } else {
            Err("endpoint stop is incomplete or uncertain; rerun upgrade after inspecting this exact endpoint; runtime was not replaced".into())
        }
    }
}

/// Check that a home lock belongs to the exact tmux execution, including its child harness.
pub fn owns_process(target: &VerifiedEndpoint, pid: u32) -> bool {
    let process = match &target.proof {
        Proof::Tmux { process, .. } => process,
        Proof::HerdrProjection(_, terminal) | Proof::HerdrFlat(terminal) => match &terminal.process
        {
            Some(process) => process,
            None => return false,
        },
        _ => return false,
    };
    let probe = SystemProcessProbe::default();
    if probe.identity(process.pid).ok().as_ref() != Some(process) {
        return false;
    }
    let mut current = pid;
    for _ in 0..64 {
        if current == process.pid {
            return true;
        }
        let Ok(row) = probe.ancestry_row(current) else {
            return false;
        };
        if row.parent_pid == 0 || row.parent_pid == current {
            return false;
        }
        current = row.parent_pid;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_lock_requires_exact_live_execution_ancestry() {
        let probe = SystemProcessProbe::default();
        let identity = probe.identity(std::process::id()).unwrap();
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("30");
        let child = multplx_core::process::OwnedChild::spawn(&mut command).unwrap();
        let mut target = VerifiedEndpoint {
            task: "owned-lock".into(),
            home: "/unused-unit-home".into(),
            backend_home: "/unused-unit-home".into(),
            endpoint: "primary:mx-owned-lock".into(),
            backend: "tmux".into(),
            root: "/unused-unit-root".into(),
            cwd: "/unused-unit-cwd".into(),
            proof: Proof::Tmux {
                window: "@unit".into(),
                pane: "%unit".into(),
                server: identity.clone(),
                process: identity.clone(),
            },
        };
        assert!(owns_process(&target, identity.pid));
        assert!(owns_process(&target, child.id()));
        assert!(!owns_process(&target, 1));
        let mut stale = identity.clone();
        stale.marker.push_str("-different-lifetime");
        if let Proof::Tmux { process, .. } = &mut target.proof {
            *process = stale;
        }
        assert!(!owns_process(&target, child.id()));
        target.proof = Proof::HerdrFlat(HerdrTerminal {
            terminal: "owned-terminal".into(),
            pane: "owned-pane".into(),
            tab: "owned-tab".into(),
            workspace: "owned-workspace".into(),
            process: Some(identity),
        });
        assert!(owns_process(&target, child.id()));
        if let Proof::HerdrFlat(terminal) = &mut target.proof {
            terminal.process = None;
        }
        assert!(!owns_process(&target, child.id()));
        target.proof = Proof::Scoped;
        assert!(!owns_process(&target, child.id()));
    }
}

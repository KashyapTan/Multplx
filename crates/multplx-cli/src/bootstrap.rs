//! Native bootstrap detection and bounded maintenance orchestration.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) struct Paths {
    pub(crate) root: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) projects: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) source_root: PathBuf,
}

const COMMON: &[&str] = &["git", "gh", "jq", "treehouse"];

fn executable(tool: &str) -> bool {
    Command::new("bash")
        .args([
            "-c",
            "command -v \"$1\" >/dev/null 2>&1",
            "mx-bootstrap",
            tool,
        ])
        .status()
        .is_ok_and(|status| status.success())
}

fn install_command(tool: &str) -> Option<&'static str> {
    match tool {
        "tmux" => Some("brew install tmux  # or the platform's package manager"),
        "git" => Some("brew install git  # or the platform's package manager"),
        "gh" => Some("brew install gh  # or the platform's package manager"),
        "curl" => Some("brew install curl  # or the platform's package manager"),
        "jq" => Some("brew install jq  # or the platform's package manager"),
        "cmux" => Some("brew install --cask cmux  # or see https://cmux.com"),
        "treehouse" => Some("curl -fsSL https://kunchenguid.github.io/treehouse/install.sh | sh"),
        _ => None,
    }
}

fn missing(output: &mut String, tool: &str) {
    if tool == "herdr" {
        output.push_str("MISSING_MANUAL: herdr (instructions: https://herdr.dev)\n");
    } else if let Some(command) = install_command(tool) {
        output.push_str(&format!("MISSING: {tool} (install: {command})\n"));
    }
}

fn backend(paths: &Paths) -> String {
    if let Ok(value) = std::env::var("MX_BACKEND")
        && value != "auto"
        && !value.trim().is_empty()
    {
        return value;
    }
    fs::read_to_string(paths.config.join("backend"))
        .map(|value| value.trim().to_owned())
        .ok()
        .filter(|value| !value.is_empty() && value != "auto")
        .unwrap_or_else(|| {
            if std::env::var("HERDR_ENV").as_deref() == Ok("1") {
                "herdr".to_owned()
            } else {
                "tmux".to_owned()
            }
        })
}

fn auto_detected_herdr(paths: &Paths) -> bool {
    let environment_is_auto = std::env::var("MX_BACKEND")
        .map(|value| value == "auto" || value.trim().is_empty())
        .unwrap_or(true);
    let configured_is_auto = fs::read_to_string(paths.config.join("backend"))
        .map(|value| value.trim().is_empty() || value.trim() == "auto")
        .unwrap_or(true);
    environment_is_auto && configured_is_auto && std::env::var("HERDR_ENV").as_deref() == Ok("1")
}

fn tool_diagnostics(paths: &Paths, output: &mut String) {
    let backend = backend(paths);
    let backend_tools: &[&str] = match backend.as_str() {
        "tmux" => &["tmux"],
        "herdr" => &["herdr", "jq"],
        "cmux" => &["cmux", "jq"],
        unknown => {
            output.push_str(&format!(
                "BACKEND_INVALID: {unknown} (known: tmux herdr cmux)\n"
            ));
            &[]
        }
    };
    for tool in backend_tools {
        let available = if *tool == "cmux" {
            executable(tool)
                || std::env::var_os("MX_BACKEND_CMUX_BUNDLE_BIN")
                    .is_some_and(|path| Path::new(&path).is_file())
        } else {
            executable(tool)
        };
        if !available {
            missing(output, tool);
        }
    }
    for tool in COMMON {
        if !executable(tool) {
            missing(output, tool);
        }
    }
    if executable("treehouse") {
        let lease = Command::new("treehouse")
            .args(["get", "--help"])
            .output()
            .ok()
            .is_some_and(|result| String::from_utf8_lossy(&result.stdout).contains("--lease"));
        if !lease {
            missing(output, "treehouse");
        }
    }
}

fn run_quiet(path: &Path, args: &[&str]) -> bool {
    Command::new(path)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn self_checks(paths: &Paths, output: &mut String, verbose: bool) {
    let vplan = std::env::var_os("MX_VPLAN_SELF_CHECK_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.source_root.join("bin/mx-vplan.sh"));
    if !run_quiet(&vplan, &["--self-check"]) {
        output.push_str("VPLAN_INVALID: bundled mx-vplan.sh self-check failed\n");
    } else if verbose {
        output.push_str("BOOTSTRAP_INFO: vplan self-check passed\n");
    }
    let headroom = Command::new(paths.source_root.join("bin/mx-headroom.sh"))
        .arg("--json")
        .env("MX_HEADROOM_IGNORE_DISPATCH_CONFIG", "1")
        .output();
    let valid = headroom
        .ok()
        .filter(|value| value.status.success())
        .and_then(|value| serde_json::from_slice::<serde_json::Value>(&value.stdout).ok())
        .is_some_and(|value| {
            [
                "model",
                "capacity",
                "in_use",
                "available",
                "candidates",
                "at_limit",
            ]
            .iter()
            .all(|key| value.get(key).is_some())
        });
    if !valid {
        output.push_str("HEADROOM_INVALID: bundled mx-headroom.sh self-check failed\n");
    } else if verbose {
        output.push_str("BOOTSTRAP_INFO: headroom self-check passed\n");
    }
}

fn tangle(paths: &Paths, output: &mut String, detect_only: bool) {
    let branch = Command::new("git")
        .args([
            "-C",
            &paths.root.to_string_lossy(),
            "symbolic-ref",
            "--short",
            "HEAD",
        ])
        .output();
    let Ok(branch) = branch else {
        return;
    };
    if !branch.status.success() {
        return;
    }
    let branch = String::from_utf8_lossy(&branch.stdout).trim().to_owned();
    let default = multplx_domain::lifecycle::fast_forward::default_branch(&paths.root)
        .unwrap_or_else(|| "main".into());
    if branch == default {
        return;
    }
    if detect_only {
        output.push_str(&format!("TANGLE: primary checkout on feature branch '{branch}' (expected '{default}'); the work is safe on that ref - read-only session must leave restore work to the session holding the system lock\n"));
    } else {
        output.push_str(&format!("TANGLE: primary checkout on feature branch '{branch}' (expected '{default}'); the work is safe on that ref - restore the primary with: git -C {} checkout {default}, then re-validate the branch in a proper worktree\n", paths.root.display()));
    }
}

fn profiles(value: &serde_json::Value) -> Option<Vec<&serde_json::Value>> {
    match value {
        serde_json::Value::Object(_) => Some(vec![value]),
        serde_json::Value::Array(values) if !values.is_empty() => Some(values.iter().collect()),
        _ => None,
    }
}

fn profile_error(profile: &serde_json::Value, scope: &str) -> Option<String> {
    let Some(object) = profile.as_object() else {
        return Some(format!("each {scope} profile must be an object"));
    };
    let Some(harness) = object
        .get("harness")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    else {
        return Some(format!("each {scope} profile needs harness"));
    };
    for name in ["model", "effort"] {
        if object
            .get(name)
            .is_some_and(|v| v.as_str().is_none_or(str::is_empty))
        {
            return Some(format!(
                "{scope} profile model and effort must be non-empty strings when present"
            ));
        }
    }
    if !matches!(harness, "claude" | "codex" | "cursor" | "pi") {
        return Some(format!("unverified harness: {harness}"));
    }
    if let Some(effort) = object.get("effort").and_then(|v| v.as_str()) {
        let valid = match harness {
            "codex" => matches!(effort, "low" | "medium" | "high" | "xhigh"),
            _ => matches!(effort, "low" | "medium" | "high" | "xhigh" | "max"),
        };
        if !valid {
            return Some(format!("invalid effort: {harness}:{effort}"));
        }
    }
    None
}

fn render_profile(value: &serde_json::Value) -> String {
    let harness = value["harness"].as_str().unwrap_or_default();
    let model = value.get("model").and_then(|v| v.as_str());
    let effort = value.get("effort").and_then(|v| v.as_str());
    format!(
        "{harness}{}{}",
        model
            .or(effort.map(|_| "default"))
            .map(|v| format!("/{v}"))
            .unwrap_or_default(),
        effort.map(|v| format!("/{v}")).unwrap_or_default()
    )
}

fn actor_dispatch(paths: &Paths, output: &mut String, verbose: bool) {
    let path = paths.config.join("actor-dispatch.json");
    if !path.is_file() {
        return;
    }
    let Ok(value) = fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
        .ok_or(())
    else {
        output.push_str("ACTOR_DISPATCH: invalid config/actor-dispatch.json - malformed JSON\n");
        return;
    };
    let Some(object) = value.as_object() else {
        output.push_str("ACTOR_DISPATCH: invalid config/actor-dispatch.json - top-level value must be an object\n");
        return;
    };
    let mut error = None;
    if let Some(rules) = object.get("rules") {
        if !rules.is_array() {
            error = Some("rules must be an array".into());
        } else {
            for rule in rules.as_array().unwrap() {
                let Some(rule) = rule.as_object() else {
                    error = Some("each rule must be an object".into());
                    break;
                };
                if rule
                    .get("when")
                    .and_then(|v| v.as_str())
                    .is_none_or(str::is_empty)
                {
                    error = Some("each rule needs non-empty when".into());
                    break;
                }
                let Some(use_value) = rule.get("use") else {
                    error = Some("each rule needs use".into());
                    break;
                };
                let Some(items) = profiles(use_value) else {
                    error = Some(
                        if use_value.as_array().is_some() {
                            "each rule needs at least one use profile"
                        } else {
                            "each rule needs use"
                        }
                        .into(),
                    );
                    break;
                };
                if let Some(found) = items
                    .into_iter()
                    .find_map(|item| profile_error(item, "use"))
                {
                    error = Some(found);
                    break;
                }
                if let Some(select) = rule.get("select") {
                    if select.as_str().is_none_or(str::is_empty) {
                        error = Some("select must be a non-empty string".into());
                        break;
                    }
                    if select != "quota-balanced" {
                        error = Some(format!("unknown select: {}", select.as_str().unwrap()));
                        break;
                    }
                }
            }
        }
    }
    if error.is_none()
        && let Some(default) = object.get("default")
    {
        match profiles(default) {
            None => {
                error = Some(
                    if default.as_array().is_some() {
                        "default needs at least one profile"
                    } else {
                        "default must be a profile object or non-empty profile array"
                    }
                    .into(),
                )
            }
            Some(items) => {
                error = items
                    .into_iter()
                    .find_map(|item| profile_error(item, "default"))
            }
        }
    }
    if let Some(error) = error {
        output.push_str(&format!(
            "ACTOR_DISPATCH: invalid config/actor-dispatch.json - {error}\n"
        ));
        return;
    }
    if verbose {
        output.push_str("BOOTSTRAP_INFO: actor dispatch active config/actor-dispatch.json\n");
        for rule in object
            .get("rules")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let use_value = &rule["use"];
            let items = profiles(use_value).unwrap();
            let rendered = if use_value.is_array() {
                format!(
                    "{}[{}]",
                    rule.get("select")
                        .and_then(|v| v.as_str())
                        .unwrap_or("quota-balanced"),
                    items
                        .into_iter()
                        .map(render_profile)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                render_profile(use_value)
            };
            output.push_str(&format!(
                "BOOTSTRAP_INFO: actor dispatch rule: {} -> {rendered}\n",
                rule["when"].as_str().unwrap()
            ));
        }
        if let Some(default) = object.get("default") {
            let items = profiles(default).unwrap();
            let rendered = if default.is_array() {
                format!(
                    "quota-balanced[{}]",
                    items
                        .into_iter()
                        .map(render_profile)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                render_profile(default)
            };
            output.push_str(&format!(
                "BOOTSTRAP_INFO: actor dispatch default: {rendered}\n"
            ));
        }
    }
}

fn project_count(projects: &Path) -> usize {
    fs::read_dir(projects)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| {
            Command::new("git")
                .args([
                    "-C",
                    &entry.path().to_string_lossy(),
                    "remote",
                    "get-url",
                    "origin",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
        .count()
}

fn system_sync(paths: &Paths, output: &mut String) {
    system_sync_with(
        paths,
        output,
        std::env::var("MX_SYSTEM_SYNC_BOOTSTRAP_TIMEOUT").ok(),
        std::env::var("MX_BOOTSTRAP_TEST_TICK_MS").ok(),
    );
}

fn system_sync_with(
    paths: &Paths,
    output: &mut String,
    timeout_override: Option<String>,
    test_tick_override: Option<String>,
) {
    let script = paths.root.join("bin/mx-system-sync.sh");
    if !script.is_file() || !paths.projects.is_dir() {
        return;
    }
    let timeout: u64 = timeout_override
        .filter(|v| !v.is_empty())
        .map(|v| v.parse().unwrap_or(20))
        .unwrap_or_else(|| {
            u64::try_from((5 + 3 * project_count(&paths.projects)).max(20)).unwrap_or(u64::MAX)
        });
    let temp = tempfile::NamedTempFile::new().ok();
    let Some(temp) = temp else {
        return;
    };
    let out = temp.reopen().ok();
    let err = temp.reopen().ok();
    let (Some(out), Some(err)) = (out, err) else {
        return;
    };
    let Ok(mut child) = Command::new(script).stdout(out).stderr(err).spawn() else {
        return;
    };
    let start = Instant::now();
    let test_tick = test_tick_override
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_millis);
    let mut logical_elapsed = 0u64;
    let mut timed_out = false;
    let started_marker = std::env::var_os("MX_FAKE_SYSTEM_SYNC_STARTED_MARKER")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None)
                if test_tick.map_or_else(|| start.elapsed().as_secs(), |_| logical_elapsed)
                    >= timeout =>
            {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break;
            }
            Ok(None) => {
                std::thread::sleep(test_tick.unwrap_or(Duration::from_millis(50)));
                if test_tick.is_some()
                    && started_marker
                        .as_ref()
                        .is_some_and(|marker| !marker.exists())
                    && start.elapsed() < Duration::from_secs(5)
                {
                    continue;
                }
                if test_tick.is_some() {
                    logical_elapsed += 1;
                }
            }
            Err(_) => break,
        }
    }
    let raw = fs::read_to_string(temp.path()).unwrap_or_default();
    for line in raw.lines() {
        if timed_out
            || line.contains(": STUCK:")
            || line.contains(": recovered:")
            || (line.contains(": skipped:")
                && !line.contains("local-only project")
                && !line.contains("no origin remote"))
        {
            output.push_str(&format!("SYSTEM_SYNC: {line}\n"));
        }
    }
    if timed_out {
        let elapsed = test_tick.map_or_else(|| start.elapsed().as_secs(), |_| logical_elapsed);
        output.push_str(&format!("SYSTEM_SYNC: system: skipped: bootstrap refresh timed out (timeout={timeout}s elapsed={}s)\n",elapsed.max(timeout)));
    }
}

fn run_script(path: &Path, args: &[&str]) -> bool {
    Command::new(path)
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

fn meta_value(raw: &str, key: &str) -> String {
    raw.lines()
        .rev()
        .filter_map(|line| line.split_once('='))
        .find_map(|(name, value)| (name == key).then(|| value.to_owned()))
        .unwrap_or_default()
}

fn registry_home(path: &Path, id: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix("- ")?;
            let (candidate, _) = rest.split_once(" - ")?;
            if candidate.trim() != id {
                return None;
            }
            let start = line.find("(home: ")? + "(home: ".len();
            let tail = &line[start..];
            let end = tail.find(';').or_else(|| tail.find(')'))?;
            Some(tail[..end].trim().to_owned())
        })
        .unwrap_or_default()
}

fn daemon_liveness(paths: &Paths, output: &mut String, verbose: bool) {
    use multplx_backend::facade::{BackendName, RuntimeBackend};

    daemon_liveness_with(
        paths,
        output,
        verbose,
        |backend_name, target| match backend_name {
            BackendName::Tmux => multplx_backend::tmux::TmuxBackend::system().agent_state(target),
            BackendName::Herdr => {
                multplx_backend::herdr::HerdrBackend::system().agent_state(target)
            }
            BackendName::Cmux => multplx_backend::cmux::CmuxBackend::system().agent_state(target),
        },
        |backend_name, target| match backend_name {
            BackendName::Tmux => {
                let _ = multplx_backend::tmux::TmuxBackend::system().kill_verified(target);
            }
            BackendName::Herdr => {
                let _ = multplx_backend::herdr::HerdrBackend::system().kill_verified(target);
            }
            BackendName::Cmux => {
                let _ = multplx_backend::cmux::CmuxBackend::system().kill_verified(target);
            }
        },
    );
}

fn daemon_liveness_with(
    paths: &Paths,
    output: &mut String,
    verbose: bool,
    mut agent_state: impl FnMut(
        multplx_backend::facade::BackendName,
        &multplx_backend::facade::BackendTarget,
    ) -> multplx_backend::facade::AgentState,
    mut kill_verified: impl FnMut(
        multplx_backend::facade::BackendName,
        &multplx_backend::facade::BackendTarget,
    ),
) {
    use multplx_backend::facade::{AgentState, BackendName, BackendTarget};

    let mut metas = fs::read_dir(&paths.state)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "meta"))
        .collect::<Vec<_>>();
    metas.sort();
    for meta in metas {
        let raw = fs::read_to_string(&meta).unwrap_or_default();
        if meta_value(&raw, "kind") != "daemon" {
            continue;
        }
        let id = meta.file_stem().unwrap_or_default().to_string_lossy();
        let target_text = meta_value(&raw, "window");
        if target_text.is_empty() {
            continue;
        }
        let backend_text = meta_value(&raw, "backend");
        let backend_text = if backend_text.is_empty() {
            "tmux"
        } else {
            &backend_text
        };
        let Ok(backend_name) = BackendName::parse(backend_text) else {
            output.push_str(&format!("DAEMON_LIVENESS: daemon {id}: skipped: agent recovery classifier unverified (backend={backend_text})\n"));
            continue;
        };
        let Ok(target) = BackendTarget::new(backend_name, target_text, Some(format!("mx-{id}")))
        else {
            output.push_str(&format!("DAEMON_LIVENESS: daemon {id}: skipped: endpoint probe unreadable (backend={backend_text})\n"));
            continue;
        };
        let state = agent_state(backend_name, &target);
        let harness = meta_value(&raw, "harness");
        let verified_harness = matches!(harness.as_str(), "claude" | "codex" | "cursor" | "pi");
        match state {
            AgentState::Alive if verbose => output.push_str(&format!(
                "BOOTSTRAP_INFO: daemon {id} already live (backend={backend_text})\n"
            )),
            AgentState::Alive => {}
            AgentState::Dead | AgentState::Missing if !verified_harness => output.push_str(&format!("DAEMON_LIVENESS: daemon {id}: skipped: recorded harness '{harness}' is unverified for recovery (backend={backend_text})\n")),
            AgentState::Dead | AgentState::Missing => {
                let cause = if state == AgentState::Dead {
                    kill_verified(backend_name, &target);
                    "confirmed agent absence on existing endpoint"
                } else { "recorded endpoint confidently missing" };
                let daemon_home = meta_value(&raw, "home");
                let spawned = Command::new(paths.root.join("bin/mx-spawn.sh"))
                    .args([id.as_ref(), daemon_home.as_str(), harness.as_str(), "--daemon"])
                    .env("MX_SPAWN_NO_GUARD", "1")
                    .env("MX_SPAWN_RECOVERY", "1")
                    .output();
                match spawned {
                    Ok(result) if result.status.success() => if verbose { output.push_str(&format!("BOOTSTRAP_INFO: daemon {id} relaunched after {cause} (backend={backend_text})\n")); },
                    Ok(result) => {
                        let stdout = String::from_utf8_lossy(&result.stdout);
                        let stderr = String::from_utf8_lossy(&result.stderr);
                        let detail = stdout.lines().next().or_else(|| stderr.lines().next()).unwrap_or("unknown error");
                        output.push_str(&format!("DAEMON_LIVENESS: daemon {id}: respawn failed after {cause}: {detail}\n"));
                    }
                    Err(error) => output.push_str(&format!("DAEMON_LIVENESS: daemon {id}: respawn failed after {cause}: {error}\n")),
                }
            }
            AgentState::Ambiguous => output.push_str(&format!("DAEMON_LIVENESS: daemon {id}: skipped: existing endpoint has ambiguous agent process (backend={backend_text})\n")),
            AgentState::Unreadable => output.push_str(&format!("DAEMON_LIVENESS: daemon {id}: skipped: endpoint probe unreadable (backend={backend_text})\n")),
            AgentState::Unverified => output.push_str(&format!("DAEMON_LIVENESS: daemon {id}: skipped: agent recovery classifier unverified (backend={backend_text})\n")),
        }
    }
}

const DAEMON_NUDGE: &str = "broker was updated to the latest - please re-read your AGENTS.md to pick up the new instructions.";

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn marker_path(paths: &Paths, id: &str) -> Option<PathBuf> {
    safe_id(id).then(|| {
        paths
            .state
            .join(".daemon-nudge-pending")
            .join(format!("{id}.pending"))
    })
}

fn write_nudge_marker(
    paths: &Paths,
    id: &str,
    home: &Path,
    commit: &str,
    instructions: &[&str],
) -> bool {
    use std::io::Write;
    let Some(marker) = marker_path(paths, id) else {
        return false;
    };
    let Some(parent) = marker.parent() else {
        return false;
    };
    if fs::create_dir_all(parent).is_err() {
        return false;
    }
    let Ok(mut temporary) = tempfile::NamedTempFile::new_in(parent) else {
        return false;
    };
    if write!(temporary, "id={id}\nselector=mx-{id}\nhome={}\ncommit={commit}\ninstructions={}\nmessage={DAEMON_NUDGE}\n", home.display(), instructions.join(", ")).is_err() { return false; }
    temporary.persist(marker).is_ok()
}

fn send_nudge(
    paths: &Paths,
    id: &str,
    home: &Path,
    commit: &str,
    instructions: &[&str],
    output: &mut String,
) {
    if !safe_id(id) {
        output.push_str(&format!(
            "NUDGE_DAEMONS: daemon {id}: send failed: unsafe id\n"
        ));
        return;
    }
    if !write_nudge_marker(paths, id, home, commit, instructions) {
        output.push_str(&format!(
            "NUDGE_DAEMONS: daemon {id}: send failed: cannot record retry marker\n"
        ));
        return;
    }
    let sent = Command::new(paths.source_root.join("bin/mx-send.sh"))
        .args([format!("mx-{id}"), DAEMON_NUDGE.to_owned()])
        .env("MX_HOME", &paths.home)
        .env("MX_ROOT_OVERRIDE", &paths.root)
        .env("MX_STATE_OVERRIDE", &paths.state)
        .output();
    match sent {
        Ok(result) if result.status.success() => {
            if let Some(marker) = marker_path(paths, id) {
                let _ = fs::remove_file(marker);
            }
            output.push_str(&format!(
                "BOOTSTRAP_INFO: nudged mx-{id} with '{DAEMON_NUDGE}'\n"
            ));
        }
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);
            let detail = stdout
                .lines()
                .next()
                .or_else(|| stderr.lines().next())
                .unwrap_or("unknown error");
            output.push_str(&format!(
                "NUDGE_DAEMONS: daemon {id}: send failed: {detail}\n"
            ));
        }
        Err(error) => output.push_str(&format!(
            "NUDGE_DAEMONS: daemon {id}: send failed: {error}\n"
        )),
    }
}

fn retry_nudges(
    paths: &Paths,
    context: &multplx_domain::lifecycle::fast_forward::Context,
    output: &mut String,
) {
    let mut markers = fs::read_dir(paths.state.join(".daemon-nudge-pending"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|v| v == "pending"))
        .collect::<Vec<_>>();
    markers.sort();
    for marker in markers {
        let raw = fs::read_to_string(&marker).unwrap_or_default();
        let id = meta_value(&raw, "id");
        if !safe_id(&id) {
            output.push_str(&format!(
                "NUDGE_DAEMONS: daemon {}: send failed: retry marker has unsafe id\n",
                if id.is_empty() { "unknown" } else { &id }
            ));
            continue;
        }
        if marker_path(paths, &id).as_deref() != Some(marker.as_path()) {
            output.push_str(&format!(
                "NUDGE_DAEMONS: daemon {id}: send failed: retry marker filename mismatch\n"
            ));
            continue;
        }
        if meta_value(&raw, "selector") != format!("mx-{id}") {
            output.push_str(&format!(
                "NUDGE_DAEMONS: daemon {id}: send failed: retry marker selector mismatch\n"
            ));
            continue;
        }
        if meta_value(&raw, "message") != DAEMON_NUDGE {
            output.push_str(&format!(
                "NUDGE_DAEMONS: daemon {id}: send failed: retry marker message mismatch\n"
            ));
            continue;
        }
        let meta_raw =
            fs::read_to_string(paths.state.join(format!("{id}.meta"))).unwrap_or_default();
        if meta_value(&meta_raw, "kind") != "daemon" {
            output.push_str(&format!("NUDGE_DAEMONS: daemon {id}: send failed: retry target has no live daemon metadata\n"));
            continue;
        }
        let home_raw = meta_value(&meta_raw, "home");
        let home = match multplx_domain::lifecycle::fast_forward::validate_daemon_home(
            context,
            &id,
            Path::new(&home_raw),
        ) {
            Ok(home) => home,
            Err(error) => {
                output.push_str(&format!(
                    "NUDGE_DAEMONS: daemon {id}: send failed: retry target home unsafe: {error}\n"
                ));
                continue;
            }
        };
        if home.to_string_lossy() != meta_value(&raw, "home") {
            output.push_str(&format!(
                "NUDGE_DAEMONS: daemon {id}: send failed: retry target home changed\n"
            ));
            continue;
        }
        let head = Command::new("git")
            .args(["-C", &home.to_string_lossy(), "rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|v| v.status.success())
            .map(|v| String::from_utf8_lossy(&v.stdout).trim().to_owned())
            .unwrap_or_default();
        if head != meta_value(&raw, "commit") {
            output.push_str(&format!("NUDGE_DAEMONS: daemon {id}: send failed: retry target is not at recorded instruction commit\n"));
            continue;
        }
        send_nudge(paths, &id, &home, &head, &[], output);
    }
}

fn daemon_sync(paths: &Paths, output: &mut String) {
    let Some(primary) = multplx_domain::lifecycle::fast_forward::primary_head_commit(&paths.root)
    else {
        return;
    };
    let context = multplx_domain::lifecycle::fast_forward::Context {
        root: paths.root.clone(),
        home: paths.home.clone(),
        marker: ".mx-daemon-home".to_owned(),
    };
    retry_nudges(paths, &context, output);
    let inheritance = multplx_domain::inheritance::InheritancePlanner::new(
        &paths.home,
        &paths.config,
        &paths.data,
    );
    let mut inherited = HashSet::new();
    let mut metas = fs::read_dir(&paths.state)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "meta"))
        .collect::<Vec<_>>();
    metas.sort();
    for meta in metas {
        let raw = fs::read_to_string(&meta).unwrap_or_default();
        if meta_value(&raw, "kind") != "daemon" {
            continue;
        }
        let id = meta.file_stem().unwrap_or_default().to_string_lossy();
        let mut home = meta_value(&raw, "home");
        if home.is_empty() {
            home = registry_home(&paths.data.join("daemons.md"), &id);
        }
        if home.is_empty() {
            continue;
        }
        let home = match multplx_domain::lifecycle::fast_forward::validate_daemon_home(
            &context,
            &id,
            Path::new(&home),
        ) {
            Ok(home) => home,
            Err(error) => {
                output.push_str(&format!(
                    "DAEMON_SYNC: daemon {id}: skipped: unsafe home: {error}\n"
                ));
                continue;
            }
        };
        let result = multplx_domain::lifecycle::fast_forward::fast_forward(
            &home,
            &format!("daemon {id}"),
            &multplx_domain::lifecycle::fast_forward::Base::Commit(primary.clone()),
            true,
            true,
        );
        if result.status == multplx_domain::lifecycle::fast_forward::Status::Skipped {
            output.push_str(&format!("DAEMON_SYNC: {}\n", result.line));
        }
        if result.status == multplx_domain::lifecycle::fast_forward::Status::Updated
            && !result.instructions.is_empty()
        {
            send_nudge(paths, &id, &home, &primary, &result.instructions, output);
        }
        if inherited.insert(home.clone()) {
            let report = tempfile::NamedTempFile::new();
            let lock = multplx_domain::inheritance::acquire_inherit_lock(&home);
            match (report, lock) {
                (Ok(report), Ok(_lock)) => match inheritance.publish_to(&home) {
                    Ok(outcome) => {
                        outcome.append_report(Some(report.path()));
                        let context = multplx_domain::inheritance::RereadContext {
                            id: &id,
                            destination_home: &home,
                            report: report.path(),
                            source_home: &paths.home,
                            root: &paths.source_root,
                            state: &paths.state,
                            skip_pending: false,
                        };
                        let (_, reread_output) = multplx_domain::inheritance::send_reread(&context);
                        output.push_str(&reread_output);
                        if outcome.failed {
                            output.push_str(&format!(
                                "DAEMON_SYNC: daemon {id}: skipped: inheritance failed\n"
                            ));
                        }
                    }
                    Err(_) => output.push_str(&format!(
                        "DAEMON_SYNC: daemon {id}: skipped: inheritance failed\n"
                    )),
                },
                _ => output.push_str(&format!(
                    "DAEMON_SYNC: daemon {id}: skipped: inheritance failed\n"
                )),
            }
        }
    }
    let _ = &paths.data;
}

pub(crate) fn run(args: &[String], paths: &Paths) -> (i32, String, String) {
    if args.first().is_some_and(|v| v == "install") {
        if args.len() < 2 {
            return (
                1,
                String::new(),
                "usage: mx-bootstrap.sh install <tool>...\n".into(),
            );
        }
        let mut stdout = String::new();
        for tool in &args[1..] {
            if tool == "herdr" {
                return (
                    1,
                    stdout,
                    "error: herdr requires manual installation (instructions: https://herdr.dev)\n"
                        .into(),
                );
            }
            let Some(command) = install_command(tool) else {
                return (1, stdout, format!("error: unknown tool {tool}\n"));
            };
            let executable = command.split("  #").next().unwrap();
            stdout.push_str(&format!("installing {tool}: {executable}\n"));
            if !Command::new("bash")
                .args(["-c", executable])
                .status()
                .is_ok_and(|s| s.success())
            {
                return (1, stdout, String::new());
            }
        }
        return (0, stdout, String::new());
    }
    if !args.is_empty() {
        return (
            2,
            String::new(),
            "usage: mx-bootstrap.sh\n       mx-bootstrap.sh install <tool>...\n".into(),
        );
    }
    let verbose = std::env::var("MX_BOOTSTRAP_VERBOSE_FACTS").as_deref() == Ok("1");
    let detect_only = std::env::var("MX_BOOTSTRAP_DETECT_ONLY").as_deref() == Ok("1");
    let mut output = String::new();
    let migration_complete =
        detect_only || run_script(&paths.source_root.join("bin/mx-pr-check-migrate.sh"), &[]);
    if auto_detected_herdr(paths) {
        output.push_str("NOTICE: auto-detected herdr runtime (HERDR_ENV=1)\n");
    }
    tool_diagnostics(paths, &mut output);
    self_checks(paths, &mut output, verbose);
    tangle(paths, &mut output, detect_only);
    if verbose && let Ok(actor) = fs::read_to_string(paths.config.join("actor-harness")) {
        let actor = actor.split_whitespace().collect::<String>();
        if !actor.is_empty() && actor != "default" {
            output.push_str(&format!(
                "BOOTSTRAP_INFO: actor harness override active: {actor}\n"
            ));
        }
    }
    actor_dispatch(paths, &mut output, verbose);
    if !detect_only {
        daemon_liveness(paths, &mut output, verbose);
        if migration_complete {
            daemon_sync(paths, &mut output);
        } else {
            let mut metas = fs::read_dir(&paths.state)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|value| value == "meta"))
                .collect::<Vec<_>>();
            metas.sort();
            for meta in metas {
                let raw = fs::read_to_string(&meta).unwrap_or_default();
                if meta_value(&raw, "kind") == "daemon" {
                    let id = meta.file_stem().unwrap_or_default().to_string_lossy();
                    output.push_str(&format!(
                        "DAEMON_SYNC: daemon {id}: skipped: PR check migration is incomplete\n"
                    ));
                }
            }
        }
        system_sync(paths, &mut output);
    }
    (0, output, String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn paths(root: &Path) -> Paths {
        Paths {
            root: root.to_owned(),
            home: root.to_owned(),
            projects: root.join("projects"),
            config: root.join("config"),
            state: root.join("state"),
            data: root.join("data"),
            source_root: root.to_owned(),
        }
    }

    fn executable_script(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("script");
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("mode");
    }

    #[test]
    fn dispatch_validation_rejects_unverified_harness_and_effort() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("config")).expect("config");
        let fixture = paths(temp.path());
        fs::write(
            fixture.config.join("actor-dispatch.json"),
            r#"{"rules":[{"when":"work","use":{"harness":"spaceship"}}]}"#,
        )
        .expect("dispatch");
        let mut output = String::new();
        actor_dispatch(&fixture, &mut output, false);
        assert_eq!(
            output,
            "ACTOR_DISPATCH: invalid config/actor-dispatch.json - unverified harness: spaceship\n"
        );

        fs::write(
            fixture.config.join("actor-dispatch.json"),
            r#"{"rules":[{"when":"work","use":{"harness":"codex","effort":"max"}}]}"#,
        )
        .expect("dispatch");
        output.clear();
        actor_dispatch(&fixture, &mut output, false);
        assert!(output.ends_with("invalid effort: codex:max\n"));
    }

    #[test]
    fn profile_shapes_and_rendering_preserve_dispatch_contract() {
        let object = serde_json::json!({"harness":"codex"});
        let array = serde_json::json!([
            {"harness":"claude","effort":"max"},
            {"harness":"pi","model":"sonnet","effort":"high"}
        ]);
        assert_eq!(profiles(&object).expect("object").len(), 1);
        assert_eq!(profiles(&array).expect("array").len(), 2);
        assert!(profiles(&serde_json::json!([])).is_none());
        assert!(profiles(&serde_json::json!(null)).is_none());
        assert_eq!(render_profile(&object), "codex");
        assert_eq!(render_profile(&array[0]), "claude/default/max");
        assert_eq!(render_profile(&array[1]), "pi/sonnet/high");

        assert_eq!(
            profile_error(&serde_json::json!("codex"), "use").as_deref(),
            Some("each use profile must be an object")
        );
        assert_eq!(
            profile_error(&serde_json::json!({}), "default").as_deref(),
            Some("each default profile needs harness")
        );
        assert_eq!(
            profile_error(&serde_json::json!({"harness":"pi","model":3}), "use").as_deref(),
            Some("use profile model and effort must be non-empty strings when present")
        );
        assert_eq!(
            profile_error(
                &serde_json::json!({"harness":"claude","effort":"ultra"}),
                "use"
            )
            .as_deref(),
            Some("invalid effort: claude:ultra")
        );
        assert!(profile_error(&array[0], "use").is_none());
    }

    #[test]
    fn dispatch_validation_covers_structural_errors_and_verbose_rendering() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("config")).expect("config");
        let fixture = paths(temp.path());
        let cases = [
            ("{", "malformed JSON"),
            ("[]", "top-level value must be an object"),
            (r#"{"rules":{}}"#, "rules must be an array"),
            (r#"{"rules":[1]}"#, "each rule must be an object"),
            (
                r#"{"rules":[{"use":{"harness":"pi"}}]}"#,
                "each rule needs non-empty when",
            ),
            (r#"{"rules":[{"when":"x"}]}"#, "each rule needs use"),
            (
                r#"{"rules":[{"when":"x","use":[]}]}"#,
                "each rule needs at least one use profile",
            ),
            (
                r#"{"rules":[{"when":"x","use":{"harness":"pi"},"select":false}]}"#,
                "select must be a non-empty string",
            ),
            (
                r#"{"rules":[{"when":"x","use":{"harness":"pi"},"select":"first"}]}"#,
                "unknown select: first",
            ),
            (r#"{"default":[]}"#, "default needs at least one profile"),
            (
                r#"{"default":true}"#,
                "default must be a profile object or non-empty profile array",
            ),
        ];
        for (raw, expected) in cases {
            fs::write(fixture.config.join("actor-dispatch.json"), raw).expect("dispatch");
            let mut output = String::new();
            actor_dispatch(&fixture, &mut output, false);
            assert!(output.contains(expected), "{raw}: {output}");
        }

        fs::write(
            fixture.config.join("actor-dispatch.json"),
            r#"{"rules":[{"when":"review","use":[{"harness":"codex","model":"gpt","effort":"high"},{"harness":"pi"}],"select":"quota-balanced"},{"when":"scout","use":{"harness":"cursor"}}],"default":[{"harness":"claude","effort":"max"}]}"#,
        )
        .expect("dispatch");
        let mut output = String::new();
        actor_dispatch(&fixture, &mut output, true);
        assert!(output.contains("actor dispatch active"));
        assert!(output.contains("review -> quota-balanced[codex/gpt/high, pi]"));
        assert!(output.contains("scout -> cursor"));
        assert!(output.contains("default: quota-balanced[claude/default/max]"));
    }

    #[test]
    fn bootstrap_helpers_parse_metadata_registry_and_safe_marker_ids() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        fs::create_dir_all(&fixture.data).expect("data");
        let registry = fixture.data.join("daemons.md");
        fs::write(
            &registry,
            "- alpha - live (home: /tmp/alpha; harness: codex)\n- beta - live (home: /tmp/beta)\n",
        )
        .expect("registry");
        assert_eq!(meta_value("key=old\nother=x\nkey=new\n", "key"), "new");
        assert_eq!(meta_value("bad\n", "key"), "");
        assert_eq!(registry_home(&registry, "alpha"), "/tmp/alpha");
        assert_eq!(registry_home(&registry, "beta"), "/tmp/beta");
        assert_eq!(registry_home(&registry, "missing"), "");
        assert!(safe_id("daemon_1.ok"));
        assert!(!safe_id(""));
        assert!(!safe_id(".hidden"));
        assert!(!safe_id("space id"));
        assert!(!safe_id(&"x".repeat(65)));

        let marker = marker_path(&fixture, "daemon_1").expect("safe marker");
        assert!(write_nudge_marker(
            &fixture,
            "daemon_1",
            Path::new("/tmp/daemon_1"),
            "abc123",
            &["AGENTS.md", "CLAUDE.md"]
        ));
        let raw = fs::read_to_string(marker).expect("marker");
        assert!(raw.contains("selector=mx-daemon_1"));
        assert!(raw.contains("instructions=AGENTS.md, CLAUDE.md"));
        assert!(marker_path(&fixture, "../escape").is_none());
        assert!(!write_nudge_marker(
            &fixture,
            "../escape",
            Path::new("/tmp/x"),
            "head",
            &[]
        ));
    }

    #[test]
    fn install_and_missing_diagnostics_cover_known_and_manual_tools() {
        for tool in ["tmux", "git", "gh", "curl", "jq", "cmux", "treehouse"] {
            assert!(install_command(tool).is_some(), "{tool}");
        }
        assert!(install_command("herdr").is_none());
        let mut output = String::new();
        missing(&mut output, "herdr");
        missing(&mut output, "tmux");
        missing(&mut output, "unknown");
        assert!(output.contains("MISSING_MANUAL: herdr"));
        assert!(output.contains("MISSING: tmux"));
        assert!(!output.contains("unknown"));
    }

    #[test]
    fn quiet_checks_and_self_checks_report_pass_and_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        fs::create_dir_all(fixture.root.join("bin")).expect("bin");
        executable_script(&fixture.root.join("bin/mx-vplan.sh"), "exit 0");
        executable_script(
            &fixture.root.join("bin/mx-headroom.sh"),
            "printf '%s\\n' '{\"model\":\"x\",\"capacity\":1,\"in_use\":0,\"available\":1,\"candidates\":[],\"at_limit\":false}'",
        );
        let mut output = String::new();
        self_checks(&fixture, &mut output, true);
        assert!(output.contains("vplan self-check passed"));
        assert!(output.contains("headroom self-check passed"));

        executable_script(&fixture.root.join("bin/mx-vplan.sh"), "exit 1");
        executable_script(
            &fixture.root.join("bin/mx-headroom.sh"),
            "printf 'not-json\\n'",
        );
        output.clear();
        self_checks(&fixture, &mut output, false);
        assert!(output.contains("VPLAN_INVALID"));
        assert!(output.contains("HEADROOM_INVALID"));
        assert!(run_quiet(Path::new("/bin/sh"), &["-c", "exit 0"]));
        assert!(!run_quiet(Path::new("/bin/sh"), &["-c", "exit 1"]));
    }

    #[test]
    fn project_count_requires_directories_with_origin_remotes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let projects = temp.path().join("projects");
        fs::create_dir(&projects).expect("projects");
        let with_origin = projects.join("with-origin");
        let local_only = projects.join("local-only");
        fs::create_dir(&with_origin).expect("origin project");
        fs::create_dir(&local_only).expect("local project");
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(&with_origin)
                .status()
                .expect("git")
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://example.invalid/repo.git"
                ])
                .current_dir(&with_origin)
                .status()
                .expect("git")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(&local_only)
                .status()
                .expect("git")
                .success()
        );
        fs::write(projects.join("plain-file"), "x").expect("file");
        assert_eq!(project_count(&projects), 1);
        assert_eq!(project_count(&temp.path().join("absent")), 0);
    }

    #[test]
    fn completed_system_sync_filters_expected_noise_and_keeps_actionable_lines() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        fs::create_dir_all(fixture.root.join("bin")).expect("bin");
        fs::create_dir_all(&fixture.projects).expect("projects");
        executable_script(
            &fixture.root.join("bin/mx-system-sync.sh"),
            "printf '%s\\n' 'a: recovered: updated' 'b: STUCK: dirty' 'c: skipped: unsafe state' 'd: skipped: local-only project' 'e: skipped: no origin remote' 'f: synced'",
        );
        let mut output = String::new();
        system_sync_with(&fixture, &mut output, Some("5".into()), None);
        assert!(output.contains("a: recovered: updated"));
        assert!(output.contains("b: STUCK: dirty"));
        assert!(output.contains("c: skipped: unsafe state"));
        assert!(!output.contains("local-only project"));
        assert!(!output.contains("no origin remote"));
        assert!(!output.contains("f: synced"));
    }

    #[test]
    fn retry_markers_fail_closed_on_each_binding_mismatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        let markers = fixture.state.join(".daemon-nudge-pending");
        fs::create_dir_all(&markers).expect("markers");
        fs::write(markers.join("unsafe.pending"), "id=../unsafe\n").expect("unsafe");
        fs::write(markers.join("wrong-name.pending"), "id=right-name\n").expect("name");
        fs::write(
            markers.join("selector.pending"),
            format!("id=selector\nselector=wrong\nmessage={DAEMON_NUDGE}\n"),
        )
        .expect("selector");
        fs::write(
            markers.join("message.pending"),
            "id=message\nselector=mx-message\nmessage=wrong\n",
        )
        .expect("message");
        fs::write(
            markers.join("absent.pending"),
            format!("id=absent\nselector=mx-absent\nmessage={DAEMON_NUDGE}\n"),
        )
        .expect("absent");
        let context = multplx_domain::lifecycle::fast_forward::Context {
            root: fixture.root.clone(),
            home: fixture.home.clone(),
            marker: ".mx-daemon-home".into(),
        };
        let mut output = String::new();
        retry_nudges(&fixture, &context, &mut output);
        assert!(output.contains("retry marker has unsafe id"));
        assert!(output.contains("retry marker filename mismatch"));
        assert!(output.contains("retry marker selector mismatch"));
        assert!(output.contains("retry marker message mismatch"));
        assert!(output.contains("retry target has no live daemon metadata"));

        output.clear();
        send_nudge(
            &fixture,
            "../unsafe",
            Path::new("/tmp/home"),
            "head",
            &[],
            &mut output,
        );
        assert!(output.contains("send failed: unsafe id"));
    }

    #[test]
    fn daemon_liveness_rejects_unknown_backends_and_unverified_harnesses() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        fs::create_dir_all(&fixture.state).expect("state");
        fs::write(
            fixture.state.join("unknown.meta"),
            "kind=daemon\nwindow=endpoint\nbackend=spaceship\nharness=codex\n",
        )
        .expect("unknown");
        fs::write(
            fixture.state.join("missing.meta"),
            "kind=daemon\nwindow=definitely-missing:window\nbackend=tmux\nharness=spaceship\n",
        )
        .expect("missing");
        fs::write(
            fixture.state.join("ignored.meta"),
            "kind=delivery\nwindow=endpoint\nbackend=spaceship\n",
        )
        .expect("ignored");
        fs::write(
            fixture.state.join("empty.meta"),
            "kind=daemon\nbackend=tmux\nharness=codex\n",
        )
        .expect("empty");
        let mut output = String::new();
        daemon_liveness_with(
            &fixture,
            &mut output,
            false,
            |_, _| multplx_backend::facade::AgentState::Missing,
            |_, _| {},
        );
        assert!(output.contains("unknown: skipped: agent recovery classifier unverified"));
        assert!(output.contains("missing: skipped: recorded harness 'spaceship' is unverified"));
        assert!(!output.contains("ignored"));
        assert!(!output.contains("empty"));
    }

    #[test]
    fn daemon_liveness_reports_verified_missing_endpoint_respawn_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        fs::create_dir_all(&fixture.state).expect("state");
        fs::create_dir_all(fixture.root.join("bin")).expect("bin");
        executable_script(
            &fixture.root.join("bin/mx-spawn.sh"),
            "printf 'spawn refused for fixture\\n'; exit 7",
        );
        fs::write(
            fixture.state.join("daemon.meta"),
            "kind=daemon\nwindow=definitely-missing:window\nbackend=tmux\nharness=codex\nhome=/tmp/daemon\n",
        )
        .expect("meta");
        let mut output = String::new();
        daemon_liveness_with(
            &fixture,
            &mut output,
            false,
            |_, _| multplx_backend::facade::AgentState::Missing,
            |_, _| {},
        );
        assert!(output.contains("respawn failed after recorded endpoint confidently missing"));
        assert!(output.contains("spawn refused for fixture"));
    }

    #[test]
    fn actor_dispatch_accepts_empty_config_and_rejects_profile_field_edges() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("config")).expect("config");
        let fixture = paths(temp.path());
        fs::write(fixture.config.join("actor-dispatch.json"), "{}").expect("dispatch");
        let mut output = String::new();
        actor_dispatch(&fixture, &mut output, true);
        assert_eq!(
            output,
            "BOOTSTRAP_INFO: actor dispatch active config/actor-dispatch.json\n"
        );
        for (raw, expected) in [
            (
                r#"{"rules":[{"when":"x","use":true}]}"#,
                "each rule needs use",
            ),
            (
                r#"{"rules":[{"when":"x","use":{"harness":""}}]}"#,
                "each use profile needs harness",
            ),
            (
                r#"{"default":{"harness":"pi","effort":""}}"#,
                "model and effort must be non-empty strings",
            ),
        ] {
            fs::write(fixture.config.join("actor-dispatch.json"), raw).expect("dispatch");
            output.clear();
            actor_dispatch(&fixture, &mut output, false);
            assert!(output.contains(expected), "{output}");
        }
    }

    #[test]
    fn invalid_backend_and_unsafe_retry_home_are_actionable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        fs::create_dir_all(&fixture.config).expect("config");
        fs::create_dir_all(&fixture.state).expect("state");
        fs::write(fixture.config.join("backend"), "spaceship\n").expect("backend");
        let mut output = String::new();
        tool_diagnostics(&fixture, &mut output);
        assert!(output.contains("BACKEND_INVALID: spaceship"));

        let markers = fixture.state.join(".daemon-nudge-pending");
        fs::create_dir_all(&markers).expect("markers");
        fs::write(
            markers.join("daemon.pending"),
            format!(
                "id=daemon\nselector=mx-daemon\nhome={}\ncommit=head\nmessage={DAEMON_NUDGE}\n",
                fixture.home.join("inside").display()
            ),
        )
        .expect("marker");
        fs::write(
            fixture.state.join("daemon.meta"),
            format!(
                "kind=daemon\nhome={}\n",
                fixture.home.join("inside").display()
            ),
        )
        .expect("meta");
        let context = multplx_domain::lifecycle::fast_forward::Context {
            root: fixture.root.clone(),
            home: fixture.home.clone(),
            marker: ".mx-daemon-home".into(),
        };
        output.clear();
        retry_nudges(&fixture, &context, &mut output);
        assert!(output.contains("retry target home unsafe"));
    }

    #[test]
    fn full_bootstrap_orchestration_is_bounded_when_optional_surfaces_are_absent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        for directory in [&fixture.config, &fixture.state, &fixture.data] {
            fs::create_dir_all(directory).expect("directory");
        }
        fs::write(fixture.config.join("backend"), "invalid-backend\n").expect("backend");
        let (status, stdout, stderr) = run(&[], &fixture);
        assert_eq!(status, 0);
        assert!(stderr.is_empty());
        assert!(stdout.contains("BACKEND_INVALID: invalid-backend"));
        assert!(stdout.contains("VPLAN_INVALID"));
        assert!(stdout.contains("HEADROOM_INVALID"));
    }

    #[test]
    fn nudge_transport_failure_keeps_a_retry_marker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        fs::create_dir_all(&fixture.state).expect("state");
        let mut output = String::new();
        send_nudge(
            &fixture,
            "daemon",
            Path::new("/tmp/daemon"),
            "head",
            &["AGENTS.md"],
            &mut output,
        );
        assert!(output.contains("NUDGE_DAEMONS: daemon daemon: send failed:"));
        assert!(marker_path(&fixture, "daemon").expect("marker").is_file());
    }

    #[test]
    fn daemon_sync_reports_an_unsafe_registered_home() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        for directory in [&fixture.state, &fixture.data, &fixture.config] {
            fs::create_dir_all(directory).expect("directory");
        }
        assert!(
            Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&fixture.root)
                .status()
                .expect("git init")
                .success()
        );
        fs::write(fixture.root.join("tracked"), "x").expect("tracked");
        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(&fixture.root)
                .status()
                .expect("git add")
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "-q",
                    "-m",
                    "base"
                ])
                .current_dir(&fixture.root)
                .status()
                .expect("git commit")
                .success()
        );
        fs::write(
            fixture.state.join("daemon.meta"),
            format!(
                "kind=daemon\nhome={}\n",
                fixture.home.join("inside").display()
            ),
        )
        .expect("meta");
        let mut output = String::new();
        daemon_sync(&fixture, &mut output);
        assert!(output.contains("DAEMON_SYNC: daemon daemon: skipped: unsafe home"));
    }

    #[test]
    fn tangle_distinguishes_detect_only_and_actionable_guidance() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        assert!(
            Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&fixture.root)
                .status()
                .expect("git init")
                .success()
        );
        fs::write(fixture.root.join("tracked"), "x").expect("tracked");
        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(&fixture.root)
                .status()
                .expect("git add")
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "-q",
                    "-m",
                    "base"
                ])
                .current_dir(&fixture.root)
                .status()
                .expect("git commit")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["checkout", "-q", "-b", "feature"])
                .current_dir(&fixture.root)
                .status()
                .expect("git branch")
                .success()
        );
        let mut output = String::new();
        tangle(&fixture, &mut output, true);
        assert!(output.contains("read-only session"));
        output.clear();
        tangle(&fixture, &mut output, false);
        assert!(output.contains("restore the primary with"));
    }

    #[test]
    fn manual_install_and_unknown_arguments_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        let result = run(&["install".into(), "herdr".into()], &fixture);
        assert_eq!(result.0, 1);
        assert_eq!(
            result.2,
            "error: herdr requires manual installation (instructions: https://herdr.dev)\n"
        );
        assert_eq!(run(&["surprise".into()], &fixture).0, 2);
    }

    #[test]
    fn timeout_replays_partial_sync_evidence_before_the_bounded_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = paths(temp.path());
        fs::create_dir_all(fixture.root.join("bin")).expect("bin");
        fs::create_dir_all(&fixture.projects).expect("projects");
        let script = fixture.root.join("bin/mx-system-sync.sh");
        fs::write(
            &script,
            "#!/bin/sh\nprintf 'alpha: synced\\nbeta: skipped: no origin remote\\n'\nwhile :; do :; done\n",
        )
        .expect("system sync");
        let mut permissions = fs::metadata(&script).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("mode");

        let mut output = String::new();
        system_sync_with(&fixture, &mut output, Some("100".into()), Some("20".into()));

        assert_eq!(
            output,
            "SYSTEM_SYNC: alpha: synced\nSYSTEM_SYNC: beta: skipped: no origin remote\nSYSTEM_SYNC: system: skipped: bootstrap refresh timed out (timeout=100s elapsed=100s)\n"
        );
    }
}

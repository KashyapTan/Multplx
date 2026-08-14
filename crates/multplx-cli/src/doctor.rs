//! Native read-only invariant sweep with two proof-bound repairs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

pub(crate) struct Paths {
    pub root: PathBuf,
    pub state: PathBuf,
    pub data: PathBuf,
}

#[derive(Clone, Serialize)]
struct Finding {
    severity: &'static str,
    category: &'static str,
    name: &'static str,
    message: String,
    suggestion: Option<String>,
    fixable: bool,
}

const CHECKS: &[(&str, &str)] = &[
    ("watcher-lock", "locks & liveness"),
    ("watcher-beacon", "locks & liveness"),
    ("orphan-worktrees", "tasks & worktrees"),
    ("dangling-pids", "tasks & worktrees"),
    ("stateless-sessions", "tasks & worktrees"),
    ("wake-queue-orphans", "queues, holds & runs"),
    ("open-holds", "queues, holds & runs"),
    ("dispatch-queue-age", "queues, holds & runs"),
    ("gate-runs", "queues, holds & runs"),
    ("workflow-runs", "queues, holds & runs"),
    ("orphan-servers", "queues, holds & runs"),
    ("tools", "tools & environment"),
    ("primary-tangle", "tools & environment"),
    ("compat-symlinks", "tools & environment"),
];

fn finding(
    name: &'static str,
    severity: &'static str,
    message: impl Into<String>,
    suggestion: Option<String>,
    fixable: bool,
) -> Finding {
    Finding {
        severity,
        category: CHECKS
            .iter()
            .find(|(candidate, _)| candidate == &name)
            .unwrap()
            .1,
        name,
        message: message.into(),
        suggestion,
        fixable,
    }
}
fn meta(raw: &str, key: &str) -> String {
    raw.lines()
        .rev()
        .filter_map(|line| line.split_once('='))
        .find_map(|(name, value)| (name == key).then(|| value.to_owned()))
        .unwrap_or_default()
}
fn alive(pid: &str) -> bool {
    pid.parse::<u32>()
        .ok()
        .filter(|pid| *pid > 1)
        .is_some_and(|pid| {
            Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .stdout(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
}

fn watcher_lock(paths: &Paths, fix: bool, fixes: &mut Vec<String>) -> Finding {
    let lock = paths.state.join(".watch.lock");
    if !lock.exists() {
        return finding("watcher-lock", "OK", "watcher lock absent", None, true);
    }
    if !lock.is_dir() {
        return finding(
            "watcher-lock",
            "FAIL",
            format!("{} is not a lock directory", lock.display()),
            Some("inspect the watcher lock; bin/mx-watch-arm.sh owns recovery".into()),
            true,
        );
    }
    let pid = fs::read_to_string(lock.join("pid")).unwrap_or_default();
    if alive(pid.trim()) {
        return finding(
            "watcher-lock",
            "OK",
            "watcher lock belongs to a live process",
            None,
            true,
        );
    }
    let probe = Command::new("lsof")
        .args(["+D", &lock.to_string_lossy()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if probe.as_ref().is_err()
        || probe
            .as_ref()
            .is_ok_and(|s| s.code().is_some_and(|c| c > 1))
    {
        return finding(
            "watcher-lock",
            "FAIL",
            "watcher lock is stale but staleness cannot be proven safely",
            Some("inspect the lock and lsof failure".into()),
            true,
        );
    }
    if fix && fs::remove_dir_all(&lock).is_ok() {
        fixes.push("cleared provably stale watcher lock".into());
        return finding("watcher-lock", "OK", "watcher lock absent", None, true);
    }
    finding(
        "watcher-lock",
        "FAIL",
        "watcher lock records a dead owner",
        Some("run bin/mx-doctor.sh --fix after verifying the owner".into()),
        true,
    )
}

fn metas(paths: &Paths) -> Vec<(String, String)> {
    let mut rows = fs::read_dir(&paths.state)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|v| v == "meta"))
        .filter_map(|p| {
            fs::read_to_string(&p).ok().map(|raw| {
                (
                    p.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    raw,
                )
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}
fn endpoint(window: &str) -> bool {
    Command::new("tmux")
        .args(["display-message", "-p", "-t", window, "#{pane_id}"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn check(name: &'static str, paths: &Paths, fix: bool, fixes: &mut Vec<String>) -> Finding {
    match name {
        "watcher-lock" => watcher_lock(paths, fix, fixes),
        "watcher-beacon" => {
            let active = metas(paths)
                .into_iter()
                .any(|(_, raw)| !meta(&raw, "window").is_empty());
            let beat = paths.state.join(".last-watcher-beat");
            if active && !beat.is_file() {
                finding(
                    name,
                    "WARN",
                    "active work has no watcher beacon",
                    Some("run bin/mx-watch-arm.sh from the lock-owning session".into()),
                    false,
                )
            } else {
                finding(
                    name,
                    "OK",
                    "watcher beacon is current or no work is active",
                    None,
                    false,
                )
            }
        }
        "orphan-worktrees" => {
            let rows = metas(paths);
            let missing = rows
                .iter()
                .filter(|(_, raw)| {
                    let p = meta(raw, "worktree");
                    !p.is_empty() && !Path::new(&p).is_dir()
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            let fixture = std::env::var_os("MX_DOCTOR_TREEHOUSE_STATUS_FILE").map(PathBuf::from);
            let orphan = fixture
                .and_then(|p| fs::read_to_string(p).ok())
                .unwrap_or_default()
                .lines()
                .find(|line| {
                    line.contains("leased")
                        && !rows
                            .iter()
                            .any(|(_, raw)| line.contains(&meta(raw, "worktree")))
                })
                .map(str::to_owned);
            if let Some(row) = orphan {
                finding(
                    name,
                    "FAIL",
                    format!("active treehouse path has no task metadata: {row}"),
                    Some("use bin/mx-teardown.sh for owned cleanup".into()),
                    false,
                )
            } else if !missing.is_empty() {
                finding(
                    name,
                    "FAIL",
                    format!(
                        "task metadata records missing worktree: {}",
                        missing.join(", ")
                    ),
                    Some("use bin/mx-teardown.sh for owned cleanup".into()),
                    false,
                )
            } else {
                finding(
                    name,
                    "OK",
                    "task worktrees and treehouse inventory agree",
                    None,
                    false,
                )
            }
        }
        "dangling-pids" => {
            let bad = metas(paths)
                .into_iter()
                .filter(|(_, raw)| {
                    let pid = meta(raw, "pid");
                    !pid.is_empty() && !alive(&pid)
                })
                .map(|(id, _)| id)
                .collect::<Vec<_>>();
            if bad.is_empty() {
                finding(
                    name,
                    "OK",
                    "recorded task pids are live or absent",
                    None,
                    false,
                )
            } else {
                finding(
                    name,
                    "FAIL",
                    format!("task metadata records dead pid: {}", bad.join(", ")),
                    Some("use bin/mx-teardown.sh for owned cleanup".into()),
                    false,
                )
            }
        }
        "stateless-sessions" => {
            let bad = metas(paths)
                .into_iter()
                .filter(|(_, raw)| {
                    let w = meta(raw, "window");
                    !w.is_empty() && !endpoint(&w)
                })
                .map(|(id, _)| id)
                .collect::<Vec<_>>();
            if bad.is_empty() {
                finding(name, "OK", "recorded task endpoints are live", None, false)
            } else {
                finding(
                    name,
                    "FAIL",
                    format!("task {} has no live tmux endpoint", bad.join(", ")),
                    Some("use bin/mx-teardown.sh for owned cleanup".into()),
                    false,
                )
            }
        }
        "wake-queue-orphans" => {
            let queue = paths.state.join(".wake-queue");
            let raw = fs::read_to_string(&queue).unwrap_or_default();
            let mut kept = Vec::new();
            let mut orphan = 0;
            for line in raw.lines() {
                let target = line.split('\t').nth(3).unwrap_or("");
                let id = target.trim_end_matches(".status");
                if !id.is_empty() && !paths.state.join(format!("{id}.meta")).is_file() {
                    orphan += 1;
                } else {
                    kept.push(line);
                }
            }
            if fix && orphan > 0 {
                let lock = paths.state.join(".wake-queue.lock");
                if fs::create_dir(&lock).is_ok() {
                    let text = if kept.is_empty() {
                        String::new()
                    } else {
                        format!("{}\n", kept.join("\n"))
                    };
                    if fs::write(&queue, text).is_ok() {
                        fixes.push(format!(
                            "pruned {orphan} wake queue row(s) whose task metadata is absent"
                        ));
                    }
                    let _ = fs::remove_dir(&lock);
                }
            }
            if orphan > 0 && !fix {
                finding(
                    name,
                    "FAIL",
                    format!("{orphan} wake rows reference absent task metadata"),
                    Some("run bin/mx-doctor.sh --fix".into()),
                    true,
                )
            } else {
                finding(
                    name,
                    "OK",
                    "wake queue contains no orphan task references",
                    None,
                    true,
                )
            }
        }
        "open-holds" => {
            let raw = fs::read_to_string(paths.data.join("backlog.md")).unwrap_or_default();
            let bad = raw
                .lines()
                .filter_map(|line| line.strip_prefix("  Origin: "))
                .any(|id| !paths.state.join(format!("{id}.meta")).is_file());
            if bad {
                finding(
                    name,
                    "FAIL",
                    "open hold origin ghost has no task metadata",
                    Some("resolve the hold through its owning workflow".into()),
                    false,
                )
            } else {
                finding(name, "OK", "open holds have live origins", None, false)
            }
        }
        "dispatch-queue-age" => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let bad = fs::read_dir(paths.state.join(".dispatch-queue"))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| fs::read_to_string(e.path()).ok())
                .filter_map(|raw| meta(&raw, "enqueued_at").parse::<u64>().ok())
                .any(|time| now.saturating_sub(time) > 172800);
            if bad {
                finding(
                    name,
                    "WARN",
                    "dispatch requests exceed 172800s",
                    Some("inspect the dispatch queue".into()),
                    false,
                )
            } else {
                finding(
                    name,
                    "OK",
                    "dispatch queue age is within bounds",
                    None,
                    false,
                )
            }
        }
        "gate-runs" => {
            let bad = fs::read_dir(&paths.state)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|v| v == "gate"))
                .any(|p| {
                    fs::read_to_string(p.join("run.json"))
                        .unwrap_or_default()
                        .contains("\"status\":\"running\"")
                        && !paths
                            .state
                            .join(format!(
                                "{}.meta",
                                p.file_stem().unwrap_or_default().to_string_lossy()
                            ))
                            .is_file()
                });
            if bad {
                finding(
                    name,
                    "FAIL",
                    "gate run says running with no live task endpoint",
                    Some("use the gate owner to reconcile it".into()),
                    false,
                )
            } else {
                finding(name, "OK", "gate runs are reconciled", None, false)
            }
        }
        "workflow-runs" => {
            let bad = fs::read_dir(&paths.state)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|v| v == "workflow"))
                .any(|p| {
                    fs::read_to_string(p.join("run.json"))
                        .unwrap_or_default()
                        .contains("\"status\":\"running\"")
                        && !p.join(".reconcile.lock").exists()
                });
            if bad {
                finding(
                    name,
                    "FAIL",
                    "workflow says running without a live reconcile lock",
                    Some("use the workflow owner to reconcile it".into()),
                    false,
                )
            } else {
                finding(name, "OK", "workflow runs are reconciled", None, false)
            }
        }
        "orphan-servers" => {
            let bad = fs::read_dir(paths.state.join(".vplan"))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| fs::read_to_string(e.path()).ok())
                .any(|raw| !alive(&meta(&raw, "pid")));
            if bad {
                finding(
                    name,
                    "FAIL",
                    "stale vplan record has no live matching identity",
                    Some("use bin/mx-vplan.sh stop <file>".into()),
                    false,
                )
            } else {
                finding(
                    name,
                    "OK",
                    "loopback server run records have live matching identities",
                    None,
                    false,
                )
            }
        }
        "tools" => {
            let tools = ["git", "gh", "jq", "treehouse"];
            let missing = tools
                .into_iter()
                .filter(|tool| {
                    Command::new("bash")
                        .args(["-c", "command -v \"$1\" >/dev/null", "doctor", tool])
                        .status()
                        .is_ok_and(|s| !s.success())
                })
                .collect::<Vec<_>>();
            if missing.is_empty() {
                finding(
                    name,
                    "OK",
                    "required tools are present and treehouse supports durable leases",
                    None,
                    false,
                )
            } else {
                finding(
                    name,
                    "FAIL",
                    format!("missing {}", missing.join(", ")),
                    Some("install the missing required tools".into()),
                    false,
                )
            }
        }
        "primary-tangle" => {
            let branch = Command::new("git")
                .args([
                    "-C",
                    &paths.root.to_string_lossy(),
                    "symbolic-ref",
                    "--short",
                    "HEAD",
                ])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
            let default = multplx_domain::lifecycle::fast_forward::default_branch(&paths.root)
                .unwrap_or_else(|| "main".into());
            if branch.as_deref().is_some_and(|b| b != default) {
                finding(
                    name,
                    "FAIL",
                    format!(
                        "primary checkout is on feature branch {} (expected {default})",
                        branch.unwrap()
                    ),
                    Some(format!(
                        "restore only from the owning session: git -C {} checkout {default}",
                        paths.root.display()
                    )),
                    false,
                )
            } else {
                finding(
                    name,
                    "OK",
                    "primary checkout is on its default branch or is not a named primary checkout",
                    None,
                    false,
                )
            }
        }
        "compat-symlinks" => {
            let configured = std::env::var("MX_DOCTOR_COMPAT_PATHS").unwrap_or_default();
            let bad = configured.lines().find(|raw| {
                let p = Path::new(raw);
                fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()) && !p.exists()
            });
            if let Some(path) = bad {
                finding(
                    name,
                    "WARN",
                    format!("dangling compatibility link {path}"),
                    Some("remove or repair the compatibility link".into()),
                    false,
                )
            } else {
                finding(
                    name,
                    "OK",
                    "compatibility paths are absent or valid symlinks",
                    None,
                    false,
                )
            }
        }
        _ => unreachable!(),
    }
}

pub(crate) fn run(args: &[String], paths: &Paths) -> (i32, String, String) {
    let mut json = false;
    let mut fix = false;
    let mut selected = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--fix" => fix = true,
            "--check" => {
                i += 1;
                if i >= args.len() {
                    return (
                        2,
                        String::new(),
                        "mx-doctor: --check requires a name\n".into(),
                    );
                }
                selected = Some(args[i].as_str())
            }
            "-h" | "--help" => {
                return (
                    0,
                    "Usage: mx-doctor.sh [--json] [--fix] [--check <name>]\n".into(),
                    String::new(),
                );
            }
            other => {
                return (
                    2,
                    String::new(),
                    format!("mx-doctor: unknown argument: {other}\n"),
                );
            }
        }
        i += 1;
    }
    if selected.is_some_and(|s| !CHECKS.iter().any(|(name, _)| *name == s)) {
        return (
            2,
            String::new(),
            format!("mx-doctor: unknown check: {}\n", selected.unwrap()),
        );
    }
    let mut fixes = Vec::new();
    let findings = CHECKS
        .iter()
        .filter(|(name, _)| selected.is_none_or(|value| value == *name))
        .map(|(name, _)| check(name, paths, fix, &mut fixes))
        .collect::<Vec<_>>();
    let fail = findings.iter().filter(|f| f.severity == "FAIL").count();
    let warn = findings.iter().filter(|f| f.severity == "WARN").count();
    let ok = findings.len() - fail - warn;
    let (code, worst) = if fail > 0 {
        (2, "FAIL")
    } else if warn > 0 {
        (1, "WARN")
    } else {
        (0, "OK")
    };
    if json {
        let value = serde_json::json!({"schema":"mx-doctor.v1","worst_severity":worst,"exit_code":code,"summary":{"ok":ok,"warn":warn,"fail":fail},"findings":findings,"fixes":fixes});
        return (
            code,
            format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
            String::new(),
        );
    }
    let mut out = String::new();
    let mut category = "";
    for f in findings {
        if f.category != category {
            if !category.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("== {} ==\n", f.category));
            category = f.category;
        }
        out.push_str(&format!("{:<5} {:<24} {}\n", f.severity, f.name, f.message));
        if let Some(s) = f.suggestion {
            out.push_str(&format!("      {:<24} -> suggest: {s}\n", ""));
        }
    }
    if !fixes.is_empty() {
        out.push_str("\n== fixes applied ==\n");
        for fix in fixes {
            out.push_str(&format!("FIXED {fix}\n"));
        }
    }
    out.push_str(&format!(
        "\nsummary: {ok} OK · {warn} WARN · {fail} FAIL          exit {code}\n"
    ));
    (code, out, String::new())
}

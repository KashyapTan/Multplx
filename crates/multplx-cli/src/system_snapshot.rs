//! Native canonical snapshot aggregation.
//!
//! Structured home state remains canonical across the bounded registered-home
//! aggregation. Parent events and terminal evidence are typed, subordinate
//! reconciliation inputs and never replace current structured state.

use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use multplx_core::classification::{open_activities, open_decisions};
use multplx_core::process::{ProcessProbe, SystemProcessProbe};
use regex::Regex;
use rustix::process::{Pid, Signal, kill_process_group};

pub(crate) struct Paths {
    pub(crate) root: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) projects: PathBuf,
    pub(crate) source_root: PathBuf,
}

pub(crate) fn run(args: &[String], paths: &Paths) -> (i32, String, String) {
    let mode = match args {
        [] => "json",
        [argument] if argument == "--json" => "json",
        [argument] if argument == "--daemon-home-summary" => "daemon-home",
        [argument] if matches!(argument.as_str(), "-h" | "--help") => {
            return (0, usage(), String::new());
        }
        _ => return (2, String::new(), usage()),
    };
    let generated = std::env::var("MX_SNAPSHOT_NOW").unwrap_or_else(|_| {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default()
    });
    let backlog = backlog(&paths.data.join("backlog.md"));
    let tasks = tasks(paths, &generated, &backlog);
    let model = if mode == "daemon-home" {
        daemon_home_summary(paths, &generated, &backlog, &tasks)
    } else {
        system_model(paths, &generated, backlog, tasks)
    };
    match serde_json::to_string_pretty(&model) {
        Ok(output) => (0, format!("{output}\n"), String::new()),
        Err(error) => (1, String::new(), format!("mx-system-snapshot: {error}\n")),
    }
}

fn usage() -> String {
    "usage: mx-system-snapshot.sh --json\n       mx-system-snapshot.sh --daemon-home-summary\n\nPrint the read-only canonical orchestration snapshot.\nThe JSON contract includes a task-first mx-portfolio.v1 projection with projects, roles, attempts, briefs, dependencies, decisions, evidence, allocations, domains, and freshness.\nCollection is bounded; unavailable observations remain explicit partial or unknown facts.\n".into()
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Verdict {
    Corroborates,
    Contradicts,
    Inconclusive,
}

impl Verdict {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Corroborates => "corroborates",
            Self::Contradicts => "contradicts",
            Self::Inconclusive => "inconclusive",
        }
    }
}

#[derive(Clone, Debug)]
struct Evidence<'a> {
    key: &'a str,
    verb: &'a str,
    summary: &'a str,
}

fn keyed(key: &str) -> bool {
    !key.is_empty() && key != "default"
}

fn result(evidence: Evidence<'_>, matched: Option<Value>, complete: bool, surface: &str) -> Value {
    let verdict = if !keyed(evidence.key) {
        Verdict::Inconclusive
    } else if matched.is_some() {
        Verdict::Corroborates
    } else if complete {
        Verdict::Contradicts
    } else {
        Verdict::Inconclusive
    };
    json!({
        "key": evidence.key,
        "verb": evidence.verb,
        "summary": evidence.summary,
        "verdict": verdict.as_str(),
        "compared_to": surface,
        "matched": if keyed(evidence.key) { matched } else { None },
    })
}

fn reconcile(summary: &Value, activities: &[Value], decisions: &[Value]) -> Value {
    let activity_results = activities
        .iter()
        .map(|row| {
            let evidence = evidence(row);
            match evidence.verb {
                "working" => {
                    let matched = array(summary, "active_children")
                        .iter()
                        .find(|candidate| !keyed(evidence.key) || candidate["id"] == evidence.key)
                        .map(|candidate| json!({"surface":"active_children","id":candidate["id"],"key":Value::Null,"verb":"working"}));
                    result(
                        evidence,
                        matched,
                        count(summary, "active_children") == array(summary, "active_children").len(),
                        "active_children",
                    )
                }
                "paused" => {
                    let matched = array(summary, "holds")
                        .iter()
                        .find(|candidate| {
                            !keyed(evidence.key)
                                || candidate["id"] == evidence.key
                                || candidate["blocked_by"] == evidence.key
                        })
                        .map(|candidate| json!({"surface":"holds","id":candidate["id"],"key":candidate["blocked_by"],"verb":"paused"}));
                    result(
                        evidence,
                        matched,
                        count(summary, "holds") == array(summary, "holds").len(),
                        "holds",
                    )
                }
                _ => result(evidence, None, false, ""),
            }
        })
        .collect::<Vec<_>>();
    let decision_results = decisions
        .iter()
        .map(|row| {
            let evidence = evidence(row);
            let mut matched = array(summary, "decisions_open")
                .iter()
                .find(|candidate| {
                    candidate["verb"] == evidence.verb
                        && (!keyed(evidence.key)
                            || candidate["key"] == evidence.key
                            || candidate["id"] == evidence.key)
                })
                .map(|candidate| json!({"surface":"decisions_open","id":candidate["id"],"key":candidate["key"],"verb":candidate["verb"]}));
            if evidence.verb == "blocked" && matched.is_none() {
                matched = array(summary, "holds")
                    .iter()
                    .find(|candidate| {
                        !keyed(evidence.key)
                            || candidate["id"] == evidence.key
                            || candidate["blocked_by"] == evidence.key
                    })
                    .map(|candidate| json!({"surface":"holds","id":candidate["id"],"key":candidate["blocked_by"],"verb":"blocked"}));
            }
            let complete = count(summary, "decisions_open")
                == array(summary, "decisions_open").len()
                && (evidence.verb != "blocked"
                    || count(summary, "holds") == array(summary, "holds").len());
            result(
                evidence,
                matched,
                complete,
                if row["verb"] == "blocked" {
                    "decisions_open_or_holds"
                } else {
                    "decisions_open"
                },
            )
        })
        .collect::<Vec<_>>();
    let contradiction = activity_results
        .iter()
        .chain(&decision_results)
        .any(|row| row["verdict"] == "contradicts");
    let inconclusive = activity_results
        .iter()
        .chain(&decision_results)
        .any(|row| row["verdict"] == "inconclusive");
    json!({"provenance":"parent-status-keyed-fold","trust":"untrusted-supplement","activities":activity_results,"decisions":decision_results,"contradiction":contradiction,"inconclusive":inconclusive})
}

fn evidence(row: &Value) -> Evidence<'_> {
    Evidence {
        key: row["key"].as_str().unwrap_or(""),
        verb: row["verb"].as_str().unwrap_or(""),
        summary: row["summary"].as_str().unwrap_or(""),
    }
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map_or(&[], Vec::as_slice)
}

fn count(value: &Value, key: &str) -> usize {
    value["counts"][key].as_u64().unwrap_or(0) as usize
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BoundResult {
    content: String,
    lines_in_window: usize,
    input_truncated: bool,
    reasons: Vec<&'static str>,
}

fn tail_bound(text: &str, line_limit: usize, byte_limit: usize) -> BoundResult {
    let input_truncated_bytes = text.len() > byte_limit;
    let start = text.floor_char_boundary(text.len().saturating_sub(byte_limit));
    let mut byte_window = &text[start..];
    if input_truncated_bytes && let Some(newline) = byte_window.find('\n') {
        byte_window = &byte_window[newline + 1..];
    }
    let mut lines = byte_window.lines().collect::<VecDeque<_>>();
    let input_truncated_lines = lines.len() > line_limit;
    while lines.len() > line_limit {
        lines.pop_front();
    }
    let mut reasons = Vec::new();
    if input_truncated_bytes {
        reasons.push("byte_limit");
    }
    if input_truncated_lines {
        reasons.push("line_limit");
    }
    BoundResult {
        content: lines.iter().copied().collect::<Vec<_>>().join("\n"),
        lines_in_window: lines.len(),
        input_truncated: input_truncated_bytes || input_truncated_lines,
        reasons,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TimedOutput {
    Completed {
        status: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    TimedOut,
    StartFailed,
}

fn run_bounded(mut command: Command, timeout: Duration, byte_limit: usize) -> TimedOutput {
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return TimedOutput::StartFailed;
    };
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    if let Ok(flags) = rustix::fs::fcntl_getfl(&stdout_pipe) {
        let _ = rustix::fs::fcntl_setfl(&stdout_pipe, flags | rustix::fs::OFlags::NONBLOCK);
    }
    if let Ok(flags) = rustix::fs::fcntl_getfl(&stderr_pipe) {
        let _ = rustix::fs::fcntl_setfl(&stderr_pipe, flags | rustix::fs::OFlags::NONBLOCK);
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let drain = |pipe: &mut dyn Read, bytes: &mut Vec<u8>| -> bool {
        let mut buffer = [0_u8; 8192];
        let mut drained = 0usize;
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => return true,
                Ok(count) => {
                    let retained = byte_limit
                        .saturating_add(1)
                        .saturating_sub(bytes.len())
                        .min(count);
                    bytes.extend_from_slice(&buffer[..retained]);
                    drained = drained.saturating_add(count);
                    if drained >= 64 * 1024 {
                        return false;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return false,
                Err(_) => return true,
            }
        }
    };
    let terminate = |child: &mut std::process::Child| {
        if let Some(pid) = Pid::from_raw(child.id() as i32) {
            let _ = kill_process_group(pid, Signal::KILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    };
    let started = Instant::now();
    let status = loop {
        let _ = drain(&mut stdout_pipe, &mut stdout);
        let _ = drain(&mut stderr_pipe, &mut stderr);
        match child.try_wait() {
            Ok(Some(status)) => {
                if let Some(pid) = Pid::from_raw(child.id() as i32) {
                    let _ = kill_process_group(pid, Signal::KILL);
                }
                break status.code().unwrap_or(1);
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Ok(None) => {
                terminate(&mut child);
                return TimedOutput::TimedOut;
            }
            Err(_) => {
                terminate(&mut child);
                return TimedOutput::StartFailed;
            }
        }
    };
    let drain_deadline = Instant::now() + Duration::from_millis(50);
    while Instant::now() < drain_deadline {
        let stdout_done = drain(&mut stdout_pipe, &mut stdout);
        let stderr_done = drain(&mut stderr_pipe, &mut stderr);
        if stdout_done && stderr_done {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    TimedOutput::Completed {
        status,
        stdout,
        stderr,
    }
}

fn status_rows(rows: Vec<multplx_core::classification::OpenStatus>) -> Vec<Value> {
    rows.into_iter()
        .map(|row| json!({"key":row.key,"verb":row.verb,"summary":row.note}))
        .collect()
}

fn parent_observation(
    status: &str,
    summary: &Value,
    line_limit: usize,
    byte_limit: usize,
    activity_limit: usize,
) -> Value {
    let bounded = tail_bound(status, line_limit, byte_limit);
    let all_activities = status_rows(open_activities(
        &bounded.content,
        "paused",
        "resolved",
        "maintainer-held",
    ));
    let records_in_window = all_activities.len();
    let retained_truncated = records_in_window > activity_limit;
    let activities = all_activities
        .into_iter()
        .skip(records_in_window.saturating_sub(activity_limit))
        .collect::<Vec<_>>();
    let decisions = status_rows(open_decisions(status, "resolved", "maintainer-held"));
    let mut reasons = bounded.reasons;
    if retained_truncated {
        reasons.push("activity_limit");
    }
    let reconciliation = reconcile(summary, &activities, &decisions);
    json!({
        "open_activities":activities,
        "open_decisions":decisions,
        "activity_scan":{
            "records":activities,
            "available":true,
            "input_truncated":bounded.input_truncated,
            "retained_truncated":retained_truncated,
            "reasons":reasons,
            "lines_in_window":bounded.lines_in_window,
            "records_in_window":records_in_window,
        },
        "reconciliation":reconciliation,
    })
}

fn terminal_evidence(
    output: TimedOutput,
    observed_at: &str,
    event_note: &str,
    compare: bool,
) -> Value {
    let base = |captured: bool, freshness: &str, reason: Option<&str>| json!({"provenance":"parent-direct-report-terminal","trust":"untrusted-supplement","captured":captured,"observed_at":observed_at,"freshness":freshness,"reason":reason,"lines":0,"bytes":0,"event_note_seen":false,"contradiction":false});
    if !compare {
        return base(
            false,
            "not-collected",
            Some("no useful contradiction check"),
        );
    }
    match output {
        TimedOutput::TimedOut => base(false, "unknown", Some("terminal capture timed out")),
        TimedOutput::StartFailed => base(false, "unknown", Some("terminal capture unavailable")),
        TimedOutput::Completed {
            status: 0, stdout, ..
        } => {
            let content = String::from_utf8_lossy(&stdout);
            let seen = !event_note.is_empty() && content.contains(event_note);
            json!({"provenance":"parent-direct-report-terminal","trust":"untrusted-supplement","captured":true,"observed_at":observed_at,"freshness":"fresh","reason":Value::Null,"lines":content.lines().count(),"bytes":stdout.len(),"event_note_seen":seen,"contradiction":seen})
        }
        TimedOutput::Completed { .. } => {
            base(false, "unknown", Some("terminal capture unavailable"))
        }
    }
}

fn backlog(path: &Path) -> Value {
    #[cfg(unix)]
    if fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o444 == 0) {
        return json!({"path":path,"present":false,"records":[]});
    }
    let Ok(text) = fs::read_to_string(path) else {
        return json!({"path":path,"present":false,"records":[]});
    };
    let item = Regex::new(r"^[-*]\s+\[([ xX])\]\s+(\S+)\s+-\s+(.*)$").unwrap();
    let bold = Regex::new(r"^[-*]\s+\*\*([^*]+)\*\*\s+-\s+(.*)$").unwrap();
    let mut section = None;
    let mut rows: Vec<Value> = Vec::new();
    for line in text.lines() {
        section = match line.trim() {
            "## In flight" => Some("in_flight"),
            "## Queued" => Some("queued"),
            "## Done" => Some("done"),
            value if value.starts_with("## ") => None,
            _ => section,
        };
        let Some(state) = section else { continue };
        if line.trim().is_empty() || line.starts_with("## ") {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            if let Some(last) = rows.last_mut().filter(|row| row["structured"] == true) {
                let body = line.trim();
                if !body.is_empty() {
                    last["body_lines"]
                        .as_array_mut()
                        .unwrap()
                        .push(Value::String(body.into()));
                    let excerpt = last["body_lines"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ");
                    last["body_excerpt"] = Value::String(excerpt.chars().take(240).collect());
                }
            }
            continue;
        }
        let parsed = item
            .captures(line)
            .map(|c| {
                (
                    c[1].eq_ignore_ascii_case("x"),
                    c[2].trim().to_owned(),
                    c[3].to_owned(),
                )
            })
            .or_else(|| {
                bold.captures(line)
                    .map(|c| (false, c[1].trim().to_owned(), c[2].to_owned()))
            });
        if let Some((checked, id, rest)) = parsed {
            rows.push(backlog_row(
                rows.len() + 1,
                state,
                checked,
                &id,
                &rest,
                line,
            ))
        } else {
            rows.push(json!({"order":rows.len()+1,"state":state,"structured":false,"id":Value::Null,"raw":line,"body_lines":[],"body_excerpt":Value::Null}))
        }
    }
    let resolved = rows.iter().filter(|row| row["structured"] == true).fold(
        BTreeMap::new(),
        |mut map, row| {
            let id = row["id"].as_str().unwrap_or("").to_owned();
            let done = row["state"] == "done";
            map.entry(id)
                .and_modify(|value| *value &= done)
                .or_insert(done);
            map
        },
    );
    for row in &mut rows {
        if row["structured"] != true {
            continue;
        }
        let blockers = row["blocked_by_ids"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        row["unresolved_blocker_ids"] = Value::Array(
            blockers
                .iter()
                .filter(|id| {
                    !resolved
                        .get(id.as_str().unwrap_or(""))
                        .copied()
                        .unwrap_or(false)
                })
                .cloned()
                .collect(),
        );
        let role = if row["state"] == "in_flight"
            && !row["hold_reason"].is_null()
            && !row["hold_kind"].is_null()
        {
            "held"
        } else if row["state"] == "in_flight" && row["kind"] == "program" {
            "program"
        } else if row["state"] == "in_flight" {
            "worker"
        } else if row["state"] == "queued" {
            "queued"
        } else {
            "done"
        };
        row["current_role"] = Value::String(role.into());
        row["requires_child_metadata"] = Value::Bool(role == "worker");
        row["maintainer_actionable"] = Value::Bool(
            row["state"] == "queued"
                && row["kind"] == "maintainer"
                && row["hold_kind"] == "maintainer"
                && !row["hold_reason"].is_null()
                && row["unresolved_blocker_ids"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
        );
    }
    json!({"path":path,"present":true,"records":rows})
}
fn backlog_row(order: usize, state: &str, checked: bool, id: &str, rest: &str, raw: &str) -> Value {
    let field = |key| metadata(rest, key);
    let blockers = Regex::new(r"blocked-by:\s*([^\s)]+)")
        .unwrap()
        .captures_iter(rest)
        .map(|c| Value::String(c[1].into()))
        .collect::<Vec<_>>();
    let links = Regex::new(r#"https?://[^\s)"<>]+"#)
        .unwrap()
        .find_iter(rest)
        .map(|m| m.as_str().trim_end_matches('>').to_owned())
        .collect::<Vec<_>>();
    let report = Regex::new(r"data/[^\s)]+/report\.md")
        .unwrap()
        .find(rest)
        .map(|m| m.as_str().to_owned());
    let blocked_reason = Regex::new(r"blocked-by:\s*[^\s)]+\s+-\s*(.*)$")
        .unwrap()
        .captures(rest)
        .map(|c| clean_title(&c[1]));
    let title_source = Regex::new(r#"<?https?://[^\s)"<>]+>?"#)
        .unwrap()
        .replace_all(rest, "")
        .to_string();
    let title = clean_title(title_source.split("blocked-by:").next().unwrap_or(""));
    let completion = ["merged", "reported", "done"]
        .into_iter()
        .find_map(|verb| metadata_word(rest, verb).map(|date| (verb, date)));
    json!({"order":order,"state":state,"structured":true,"id":id,"checked":checked,"title":title,"repo":field("repo"),"kind":field("kind"),"priority":field("priority"),"hold_reason":field("hold"),"hold_kind":field("hold-kind"),"blocked_by":blockers.last().and_then(Value::as_str),"blocked_by_ids":blockers,"blocked_reason":blocked_reason,"since":metadata_word(rest,"since"),"merged":metadata_word(rest,"merged"),"reported":metadata_word(rest,"reported"),"done":metadata_word(rest,"done"),"completion":{"verb":completion.as_ref().map(|v|v.0),"date":completion.as_ref().map(|v|v.1.clone())},"links":links,"pr_url":links.iter().find(|url|url.contains("/pull/")).cloned(),"report_path":report,"local_note":rest.contains("local main").then_some("local main"),"raw":raw,"body_lines":[],"body_excerpt":Value::Null})
}
fn metadata(rest: &str, key: &str) -> Option<String> {
    Regex::new(&format!(r"(?:\(|,\s*){}:\s*([^,)]*)", regex::escape(key)))
        .unwrap()
        .captures(rest)
        .map(|c| c[1].trim().to_owned())
        .filter(|v| !v.is_empty())
}
fn metadata_word(rest: &str, key: &str) -> Option<String> {
    Regex::new(&format!(r"(?:\(|,\s*){}\s+([^,)]*)", regex::escape(key)))
        .unwrap()
        .captures(rest)
        .map(|c| c[1].trim().to_owned())
        .filter(|v| !v.is_empty())
}
fn clean_title(rest: &str) -> String {
    let mut value = rest.to_owned();
    let trailing=Regex::new(r"\s*\(\s*(?:(?:repo|kind|priority|hold|hold-kind):\s*[^)]*|(?:since|merged|reported|done)\s+[^)]*)\s*\)\s*$").unwrap();
    for _ in 0..20 {
        let next = trailing.replace(&value, "").into_owned();
        if next == value {
            break;
        }
        value = next;
    }
    let artifacts = Regex::new(r"(?:\s+-)?\s+(?:data/[^\s)]+/report\.md|local main)\s*$").unwrap();
    Regex::new(r"\s+")
        .unwrap()
        .replace_all(
            artifacts
                .replace(&value, "")
                .trim()
                .trim_end_matches('-')
                .trim(),
            " ",
        )
        .into_owned()
}

fn tasks(paths: &Paths, generated: &str, backlog: &Value) -> Value {
    let paths_to_read = fs::read_dir(&paths.state)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|v| v.to_str()) == Some("meta")).then_some(path)
        })
        .collect::<Vec<_>>();
    let queue = std::sync::Mutex::new(VecDeque::from(paths_to_read));
    let output = std::sync::Mutex::new(Vec::new());
    let workers = env_usize("MX_SNAPSHOT_TASK_CONCURRENCY", 4).clamp(1, 16);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let path = queue.lock().expect("snapshot task queue").pop_front();
                    let Some(path) = path else { break };
                    if let Some(row) = task(paths, &path, generated, backlog) {
                        output.lock().expect("snapshot task output").push(row);
                    }
                }
            });
        }
    });
    let mut rows = output.into_inner().expect("snapshot task output");
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Value::Array(rows)
}
fn meta(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.into(), value.into()))
        .collect()
}
fn task(paths: &Paths, path: &Path, generated: &str, backlog: &Value) -> Option<Value> {
    let id = path.file_stem()?.to_str()?.to_owned();
    let fields = meta(path);
    let normalized = multplx_core::filesystem::read_bounded_regular(path, 4 * 1024 * 1024)
        .map_err(|e| e.to_string())
        .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
        .and_then(|text| multplx_domain::lifecycle::subagent_model::read_meta(&id, &text));
    let coordination_error = normalized.as_ref().err().cloned();
    let coordination = normalized.ok();
    let allocation_observation = coordination.as_ref().and_then(|task| {
        let token = task.allocation.as_ref()?;
        let result = task
            .project
            .as_ref()
            .ok_or_else(|| "allocation project missing".to_owned())
            .and_then(multplx_domain::lifecycle::worktree::Store::new)
            .and_then(|store| store.observe(token));
        Some(match result {
            Ok(observation) => json!(observation),
            Err(error) => json!({"allocation": null, "error": error}),
        })
    });

    let kind = fields
        .get("kind")
        .filter(|v| !v.is_empty())
        .map_or("delivery", String::as_str);
    let backend = fields.get("backend").map_or("tmux", String::as_str);
    let target = fields.get("window").filter(|v| !v.is_empty()).cloned();
    let status_path = paths.state.join(format!("{id}.status"));
    let status = fs::read_to_string(&status_path).unwrap_or_default();
    let last = status
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let current = actor_state(paths, &id);
    let current_state = current["state"].as_str().unwrap_or("unknown");
    let current_source = current["source"].as_str().unwrap_or("none");
    let mut decisions = open_decisions(&status, "resolved", "maintainer-held");
    if kind != "daemon"
        && ((matches!(current_source, "native-event" | "run-step" | "pane")
            && !matches!(current_state, "parked" | "blocked"))
            || matches!(current_state, "done" | "failed"))
    {
        decisions.clear()
    }
    let open = status_rows(decisions);
    let endpoint = target.as_deref().map(|target| {
        multplx_backend::facade::observe_endpoint(
            backend,
            target,
            Some(format!("mx-{id}")),
            kind == "daemon",
        )
    });
    let exists = endpoint
        .as_ref()
        .and_then(|result| result.as_ref().ok().map(|observation| observation.exists));
    let endpoint_detail = endpoint
        .as_ref()
        .map(|result| match result {
            Ok(observation) => observation.detail.clone(),
            Err(error) => error.to_string(),
        })
        .unwrap_or_default();
    let alive = if kind == "daemon" {
        endpoint
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|observation| observation.agent_state.alive_token())
            .unwrap_or("unknown")
    } else {
        "not_checked"
    }
    .to_owned();
    let report = paths.data.join(&id).join("report.md");
    let mut pr = fields.get("pr").filter(|v| !v.is_empty()).cloned();
    let mut pr_source = if pr.is_some() { "meta" } else { "absent" };
    if pr.is_none() {
        pr = Regex::new(r#"https?://[^\s)"]+/pull/[0-9]+"#)
            .unwrap()
            .find(&status)
            .map(|m| m.as_str().into());
        if pr.is_some() {
            pr_source = "status_event"
        }
    }
    let owned = backlog["records"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["structured"] == true && row["id"] == id)
        })
        .cloned()
        .unwrap_or(Value::Null);
    Some(
        json!({"id":id,"coordination_schema":2,"coordination":coordination,"allocation_observation":allocation_observation,"coordination_error":coordination_error,"kind":kind,"harness":fields.get("harness").cloned().unwrap_or_default(),"mode":fields.get("mode").cloned().unwrap_or_default(),"yolo":fields.get("yolo").cloned().unwrap_or_default(),"project":fields.get("project").cloned().unwrap_or_default(),"backend":backend,"paths":{"meta":observed(Some(path)),"status_log":{"path":status_path,"present":status_path.is_file(),"kind":"event_history","last_event":{"state":multplx_core::classification::status_line_verb(last),"note":multplx_core::classification::status_line_note(last),"raw":last}},"worktree":observed(fields.get("worktree").map(Path::new)),"home":observed(fields.get("home").map(Path::new)),"report":observed(Some(&report))},"daemon_projects":fields.get("projects").map(|v|v.split(',').map(str::trim).filter(|v|!v.is_empty()).collect::<Vec<_>>()).unwrap_or_default(),"current_state":{"state":current_state,"source":current_source,"detail":current["detail"],"raw":current["raw"],"observed_at":generated,"freshness":"fresh"},"endpoint":{"detail":endpoint_detail,"target":target,"exists":exists,"agent_alive":alive,"status":if exists==Some(false){"absent"}else if matches!(alive.as_str(),"alive"|"dead"){alive.as_str()}else{"unknown"},"observed_at":generated,"freshness":"fresh"},"pr":{"url":pr,"source":pr_source},"hints":{"pending_decision":open.iter().any(|row|row["verb"]=="needs-decision"),"blocked_event":open.iter().any(|row|row["verb"]=="blocked"),"open_decisions":open,"scout_report_present":report.is_file(),"last_event_text":last},"actions":if kind=="daemon"{json!({"send":format!("bin/mx-send.sh mx-{id} '<request>'"),"watch":"read status/doc return channel; do not routinely mx-peek a daemon for answers","return_channel_note":"Daemon answers come back through status/doc paths after a marked mx-send request."})}else{json!({"watch":format!("bin/mx-peek.sh mx-{id}"),"steer":format!("bin/mx-send.sh mx-{id} '<instruction>'"),"return_channel_note":Value::Null})},"backlog":owned}),
    )
}
fn actor_state(paths: &Paths, id: &str) -> Value {
    let mut command = Command::new(paths.source_root.join("bin/mx-actor-state.sh"));
    command
        .arg(id)
        .env("MX_ROOT_OVERRIDE", &paths.root)
        .env("MX_HOME", &paths.home)
        .env("MX_STATE_OVERRIDE", &paths.state)
        .env("MX_DATA_OVERRIDE", &paths.data)
        .env("MX_CONFIG_OVERRIDE", &paths.config)
        .env("MX_PROJECTS_OVERRIDE", &paths.projects);
    let output = run_bounded(
        command,
        env_duration("MX_SNAPSHOT_TASK_TIMEOUT", 2),
        env_usize("MX_SNAPSHOT_TASK_MAX_BYTES", 65_536),
    );
    let raw = match output {
        TimedOutput::Completed {
            status: 0, stdout, ..
        } => String::from_utf8_lossy(&stdout)
            .lines()
            .next()
            .unwrap_or("")
            .to_owned(),
        TimedOutput::Completed { stdout, stderr, .. } => {
            return json!({"state":"unknown", "source":"error", "detail":String::from_utf8_lossy(&stderr).trim(), "raw":String::from_utf8_lossy(&stdout)});
        }
        TimedOutput::TimedOut => {
            return json!({"state":"unknown", "source":"timeout", "detail":"actor observation timed out", "raw":""});
        }
        TimedOutput::StartFailed => {
            return json!({"state":"unknown", "source":"error", "detail":"actor observation failed to start", "raw":""});
        }
    };
    let mut state = "unknown";
    let mut source = "none";
    let mut detail = "";
    if let Some(rest) = raw.strip_prefix("state: ") {
        let mut parts = rest.splitn(3, " · ");
        state = parts.next().unwrap_or("unknown");
        source = parts
            .next()
            .and_then(|v| v.strip_prefix("source: "))
            .unwrap_or("none");
        detail = parts.next().unwrap_or("");
    }
    json!({"state":state,"source":source,"detail":detail,"raw":raw})
}
fn observed(path: Option<&Path>) -> Value {
    match path {
        Some(path) => json!({"path":path,"present":path.exists()}),
        None => json!({"path":Value::Null,"present":false}),
    }
}
fn daemon_home_summary(paths: &Paths, generated: &str, backlog: &Value, tasks: &Value) -> Value {
    let home = &paths.home;
    let records = backlog["records"].as_array().map_or(&[][..], Vec::as_slice);
    let task_rows = tasks.as_array().map_or(&[][..], Vec::as_slice);
    let owned = records
        .iter()
        .filter(|row| row["state"] == "in_flight" && row["structured"] == true)
        .collect::<Vec<_>>();
    let owned_ids = owned
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let unstructured = records
        .iter()
        .filter(|row| {
            matches!(row["state"].as_str(), Some("in_flight" | "queued"))
                && row["structured"] == false
        })
        .count();
    let orphan = owned
        .iter()
        .filter(|row| {
            row["requires_child_metadata"] == true
                && !task_rows.iter().any(|task| task["id"] == row["id"])
        })
        .map(|row| row["id"].clone())
        .collect::<Vec<_>>();
    let unowned_rows = task_rows
        .iter()
        .filter(|task| !owned_ids.contains(task["id"].as_str().unwrap_or("")))
        .map(|task| (task["id"].clone(), task["current_state"]["state"].clone()))
        .collect::<Vec<_>>();
    let terminal_rows = owned
        .iter()
        .filter(|work| {
            task_rows.iter().any(|task| {
                task["id"] == work["id"]
                    && matches!(
                        task["current_state"]["state"].as_str(),
                        Some("done" | "failed")
                    )
            })
        })
        .filter_map(|row| {
            task_rows
                .iter()
                .find(|task| task["id"] == row["id"])
                .map(|task| (row["id"].clone(), task["current_state"]["state"].clone()))
        })
        .collect::<Vec<_>>();
    let unknown = task_rows
        .iter()
        .filter(|task| task["current_state"]["state"] == "unknown")
        .map(|task| task["id"].clone())
        .collect::<Vec<_>>();
    let active_all=owned.iter().filter(|work|work["current_role"]!="program").filter_map(|work|task_rows.iter().find(|task|task["id"]==work["id"]&&task["current_state"]["state"]=="working")).map(|task|json!({"id":task["id"],"kind":task["kind"],"state":task["current_state"]["state"],"source":task["current_state"]["source"],"doing":task["current_state"]["detail"]})).collect::<Vec<_>>();
    let queued_all = records
        .iter()
        .filter(|row| {
            row["structured"] == true
                && (row["state"] == "queued"
                    || (row["state"] == "in_flight"
                        && row["current_role"] == "held"
                        && !task_rows.iter().any(|task| {
                            task["id"] == row["id"] && task["current_state"]["state"] == "working"
                        })))
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut decisions_all=queued_all.iter().filter(|row|row["maintainer_actionable"]==true).map(|row|json!({"id":row["id"],"key":row["id"],"verb":"maintainer-hold","summary":row["title"],"reason":row["hold_reason"],"source":"backlog"})).collect::<Vec<_>>();
    for task in task_rows {
        for row in task["hints"]["open_decisions"]
            .as_array()
            .into_iter()
            .flatten()
        {
            decisions_all.push(json!({"id":task["id"],"key":row["key"],"verb":row["verb"],"summary":row["summary"],"reason":Value::Null,"source":"status"}))
        }
    }
    let mut holds_all=queued_all.iter().filter(|row|row["unresolved_blocker_ids"].as_array().is_some_and(|v|!v.is_empty())||(!row["hold_reason"].is_null()&&!row["hold_kind"].is_null())).map(|row|json!({"id":row["id"],"title":row["title"],"blocked_by":if row["unresolved_blocker_ids"].as_array().is_some_and(|ids|!ids.is_empty()){json!(row["unresolved_blocker_ids"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","))}else{Value::Null},"blocked_by_ids":row["blocked_by_ids"],"unresolved_blocker_ids":row["unresolved_blocker_ids"],"reason":if !row["hold_reason"].is_null(){row["hold_reason"].clone()}else{row["blocked_reason"].clone()},"source":"backlog"})).collect::<Vec<_>>();
    for work in &owned {
        if (work["hold_reason"].is_null() || work["hold_kind"].is_null())
            && let Some(task) = task_rows.iter().find(|task| {
                task["id"] == work["id"]
                    && matches!(
                        task["current_state"]["state"].as_str(),
                        Some("parked" | "paused" | "blocked")
                    )
            })
        {
            holds_all.push(json!({"id":task["id"],"title":work["title"],"blocked_by":Value::Null,"blocked_by_ids":[],"unresolved_blocker_ids":[],"reason":task["current_state"]["detail"],"source":"child-state"}));
        }
    }
    let mut landed_all=records.iter().filter(|row|row["state"]=="done"&&row["structured"]==true&&row["kind"]!="maintainer").map(|row|json!({"id":row["id"],"title":row["title"],"pr_url":row["pr_url"],"report_path":row["report_path"],"local_note":row["local_note"],"completion":row["completion"]})).collect::<Vec<_>>();
    landed_all.sort_by(|a, b| {
        b["completion"]["date"]
            .as_str()
            .cmp(&a["completion"]["date"].as_str())
            .then(b["id"].as_str().cmp(&a["id"].as_str()))
    });
    let invalidity = if backlog["present"] != true {
        json!({"kind":"missing_backlog","ids":[]})
    } else if unstructured > 0 {
        json!({"kind":"unstructured_current","ids":[]})
    } else if !orphan.is_empty() {
        json!({"kind":"orphan_in_flight","ids":orphan})
    } else if !unowned_rows.is_empty() {
        json!({"kind":"unowned_current","ids":unowned_rows.iter().map(|row|row.0.clone()).collect::<Vec<_>>()})
    } else if !terminal_rows.is_empty() {
        json!({"kind":"terminal_in_flight","ids":terminal_rows.iter().map(|row|row.0.clone()).collect::<Vec<_>>()})
    } else if !unknown.is_empty() {
        json!({"kind":"child_current_unavailable","ids":unknown})
    } else {
        json!({"kind":Value::Null,"ids":[]})
    };
    let valid = invalidity["kind"].is_null();
    let reason = if valid {
        Value::Null
    } else {
        Value::String(
            match invalidity["kind"].as_str() {
                Some("missing_backlog") => "missing structured backlog",
                Some("unstructured_current") => "unstructured current backlog row",
                Some("orphan_in_flight") => {
                    return daemon_summary_invalid(
                        home,
                        generated,
                        &invalidity,
                        &format!(
                            "in-flight backlog item has no child metadata: {}",
                            orphan
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                }
                Some("unowned_current") => {
                    return daemon_summary_invalid(
                        home,
                        generated,
                        &invalidity,
                        &format!(
                            "live child state has no in-flight backlog item: {}",
                            unowned_rows
                                .iter()
                                .map(|(id, state)| format!(
                                    "{}={}",
                                    id.as_str().unwrap_or(""),
                                    state.as_str().unwrap_or("unknown")
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                }
                Some("terminal_in_flight") => {
                    return daemon_summary_invalid(
                        home,
                        generated,
                        &invalidity,
                        &format!(
                            "in-flight backlog item has terminal child state: {}",
                            terminal_rows
                                .iter()
                                .map(|(id, state)| format!(
                                    "{}={}",
                                    id.as_str().unwrap_or(""),
                                    state.as_str().unwrap_or("unknown")
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                }
                _ => "child current state unavailable",
            }
            .into(),
        )
    };
    let reason = if invalidity["kind"] == "child_current_unavailable" {
        json!(format!(
            "child current state unavailable: {}",
            unknown
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    } else {
        reason
    };
    let state = if !valid {
        "unknown"
    } else if decisions_all.iter().any(|row| {
        matches!(
            row["verb"].as_str(),
            Some("needs-decision" | "maintainer-hold")
        )
    }) {
        "maintainer_decision"
    } else if !active_all.is_empty() {
        "active_child_work"
    } else if !holds_all.is_empty() {
        "externally_held"
    } else {
        "no_active_work"
    };
    let endpoints_all=task_rows.iter().map(|task|json!({"id":task["id"],"state":task["current_state"]["state"],"source":task["current_state"]["source"],"endpoint":task["endpoint"]})).collect::<Vec<_>>();
    let domains_all = task_rows
        .iter()
        .filter(|task| task.pointer("/coordination/role").and_then(Value::as_str) == Some("sub-orchestrator"))
        .map(|task| json!({"id":task["id"],"coordination":task["coordination"],"endpoint":task["endpoint"]}))
        .collect::<Vec<_>>();
    let useful_tasks = task_rows
        .iter()
        .filter(|task| {
            task.pointer("/coordination/role").and_then(Value::as_str) != Some("sub-orchestrator")
        })
        .count();
    let sessions = task_rows
        .iter()
        .filter(|task| task.pointer("/endpoint/exists").and_then(Value::as_bool) == Some(true))
        .count();
    let worker_sessions = task_rows
        .iter()
        .filter(|task| {
            task.pointer("/coordination/role").and_then(Value::as_str) != Some("sub-orchestrator")
                && task.pointer("/endpoint/exists").and_then(Value::as_bool) == Some(true)
        })
        .count();
    let coordinator_sessions = task_rows
        .iter()
        .filter(|task| {
            task.pointer("/coordination/role").and_then(Value::as_str) == Some("sub-orchestrator")
                && task.pointer("/endpoint/exists").and_then(Value::as_bool) == Some(true)
        })
        .count();
    let attempts_known = task_rows
        .iter()
        .filter(|task| {
            task.pointer("/coordination/attempt/id")
                .and_then(Value::as_str)
                .is_some()
        })
        .count();
    let child_n = env_usize("MX_SNAPSHOT_DAEMON_CHILDREN", 20);
    let queued_n = env_usize("MX_SNAPSHOT_DAEMON_QUEUED", 20);
    let decision_n = env_usize("MX_SNAPSHOT_DAEMON_DECISIONS", 20);
    let landed_n = env_usize("MX_SNAPSHOT_DAEMON_LANDED_PER_HOME", 10);
    let mut omitted = Vec::new();
    let bounded = |rows: &Vec<Value>, limit: usize, surface: &str, omitted: &mut Vec<Value>| {
        if rows.len() > limit {
            omitted.push(json!({"surface":surface,"count":rows.len()-limit}));
        }
        rows[..rows.len().min(limit)].to_vec()
    };
    let active = bounded(&active_all, child_n, "active_children", &mut omitted);
    let decisions = bounded(&decisions_all, decision_n, "decisions_open", &mut omitted);
    let holds = bounded(&holds_all, queued_n, "holds", &mut omitted);
    let queued = bounded(&queued_all, queued_n, "queued", &mut omitted);
    let endpoints = bounded(&endpoints_all, child_n, "endpoints", &mut omitted);
    let landed = if landed_n == 0 {
        landed_all.clone()
    } else {
        bounded(&landed_all, landed_n, "landed", &mut omitted)
    };
    let task_limit = env_usize("MX_SNAPSHOT_DAEMON_TASKS", 20);
    let canonical_tasks = task_rows
        .iter()
        .take(task_limit)
        .cloned()
        .collect::<Vec<_>>();
    if task_rows.len() > canonical_tasks.len() {
        omitted.push(json!({"surface":"canonical_tasks","shown":canonical_tasks.len(),"total":task_rows.len(),"reason":"task_limit"}));
    }
    let child_workflows = workflow_runs(paths);
    let child_portfolio = portfolio(
        paths,
        generated,
        backlog,
        tasks,
        &json!({"records":[],"total":0,"complete":true}),
        &json!({"workflow_runs":child_workflows}),
    );
    json!({"schema":"mx-daemon-home-summary.v1","generated":generated,"home":home,"valid":valid,"reason":reason,"invalidity":invalidity,"state":state,"portfolio":child_portfolio,"tasks":canonical_tasks,"workflow_runs":child_workflows["records"],"active_children":active,"decisions_open":decisions,"holds":holds,"queued":queued,"landed":landed,"endpoints":endpoints,"domains":domains_all,"counts":{"active_children":active_all.len(),"decisions_open":decisions_all.len(),"holds":holds_all.len(),"queued":queued_all.len(),"landed":landed_all.len(),"endpoints":endpoints_all.len(),"useful_tasks":useful_tasks,"sessions":sessions,"worker_sessions":worker_sessions,"coordinator_sessions":coordinator_sessions,"attempts_known":attempts_known,"coordinator_tasks":domains_all.len(),"tasks_total":task_rows.len(),"tasks_shown":canonical_tasks.len()},"omitted":omitted})
}

fn daemon_summary_invalid(home: &Path, generated: &str, invalidity: &Value, reason: &str) -> Value {
    json!({"schema":"mx-daemon-home-summary.v1","generated":generated,"home":home,"valid":false,"reason":reason,"invalidity":invalidity,"state":"unknown","portfolio":Value::Null,"tasks":[],"workflow_runs":[],"active_children":[],"decisions_open":[],"holds":[],"queued":[],"landed":[],"endpoints":[],"domains":[],"counts":{"active_children":Value::Null,"decisions_open":Value::Null,"holds":Value::Null,"queued":Value::Null,"landed":Value::Null,"endpoints":Value::Null,"useful_tasks":Value::Null,"sessions":Value::Null,"worker_sessions":Value::Null,"coordinator_sessions":Value::Null,"attempts_known":Value::Null,"coordinator_tasks":Value::Null,"tasks_total":Value::Null,"tasks_shown":Value::Null},"omitted":[]})
}
fn system_model(paths: &Paths, generated: &str, backlog: Value, tasks: Value) -> Value {
    let inventory = inventory(&backlog, &tasks);
    let reports = reports(paths, &backlog, &tasks);
    let watcher = watcher(paths);
    let wake = wake_queue(paths);
    let (headroom, headroom_reason) = headroom(paths);
    let daemon_current = daemon_current(paths, generated, &tasks);
    let domains = domain_projection(paths, &tasks, &daemon_current, generated);
    let daemon_landed = daemon_landed(&daemon_current);
    let later_feeds = later(paths);
    let portfolio = portfolio(paths, generated, &backlog, &tasks, &domains, &later_feeds);
    json!({"schema":"mx-system-snapshot.v1","generated":generated,"mx_home":paths.home,"roots":{"mx_root":paths.root,"state":paths.state,"data":paths.data,"config":paths.config,"projects":paths.projects},"backlog":backlog,"tasks":tasks,"portfolio":portfolio,"native_delegations":native_observations(&paths.state),"main_inventory":inventory,"scout_reports":reports,"watcher":watcher,"wake_queue":wake,"dispatch_queue":dispatch(paths),"headroom":headroom,"headroom_reason":headroom_reason,"domains":domains,"vplan_reviews":vplans(paths),"later_feeds":later_feeds,"daemon_current":daemon_current,"daemon_landed":daemon_landed,"daemon_guidance":{"note":"For kind=daemon, catchup selects validated structured state from that registered home; parent events and bounded terminal evidence are fallback-only supplements and never current-state authority."}})
}

/// Build the task-first read model from existing authoritative records.
///
/// This is deliberately a projection: it does not infer sessions, successful
/// checks, project identity, or completion when their owners did not record them.
fn portfolio(
    paths: &Paths,
    generated: &str,
    backlog_root: &Value,
    tasks: &Value,
    domains: &Value,
    later_feeds: &Value,
) -> Value {
    let rows = tasks.as_array().map_or(&[][..], Vec::as_slice);
    let review_records = rows
        .iter()
        .filter_map(|task| task.get("coordination").cloned())
        .filter_map(|record| serde_json::from_value(record).ok())
        .collect::<Vec<multplx_domain::lifecycle::subagent_model::TaskRecord>>();
    let review_queue =
        multplx_domain::lifecycle::delivery_evidence::human_review_queue(&review_records)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| {
                serde_json::to_value(&entry)
                    .ok()
                    .map(|value| (entry.task_key, value))
            })
            .collect::<BTreeMap<_, _>>();
    let catalog = multplx_domain::project_registry::read_catalog(&paths.home);
    let mut reasons = Vec::new();
    if let Err(error) = &catalog {
        reasons.push(format!("project registry unavailable: {error}"));
    }
    let mut projects = catalog
        .as_ref()
        .map(|catalog| {
            catalog
                .projects
                .iter()
                .map(|project| {
                    json!({
                        "id": project.project_id,
                        "display_name": project.display_name,
                        "aliases": project.aliases,
                        "common_git_identity": project.common_git_identity,
                        "remote": project.remote,
                        "checkouts": project.checkouts,
                        "registered": true
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let known_projects = projects
        .iter()
        .filter_map(|project| project["id"].as_str())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let mut unregistered = rows
        .iter()
        .filter_map(|task| {
            let id = task
                .pointer("/coordination/project/project_id")
                .or_else(|| task.get("project"))
                .and_then(Value::as_str)?;
            (!id.is_empty() && !known_projects.contains(id)).then(|| {
                json!({"id":id,"display_name":id,"aliases":[],"common_git_identity":Value::Null,"remote":Value::Null,"checkouts":[],"registered":false})
            })
        })
        .collect::<Vec<_>>();
    unregistered.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    unregistered.dedup_by(|left, right| left["id"] == right["id"]);
    projects.extend(unregistered);
    projects.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));

    let task_states = rows
        .iter()
        .filter_map(|task| {
            let owner = task
                .pointer("/coordination/owner_home")
                .and_then(Value::as_str)
                .unwrap_or_else(|| paths.home.to_str().unwrap_or("legacy-unknown"));
            Some((
                multplx_domain::lifecycle::subagent_model::qualified_task_id(
                    owner,
                    task["id"].as_str()?,
                ),
                task.pointer("/coordination/schedule/state")
                    .or_else(|| task.pointer("/current_state/state"))?
                    .as_str()?
                    .to_owned(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let workflow_rows = later_feeds
        .pointer("/workflow_runs/records")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let (decision_evidence, decision_evidence_truncated) = decision_evidence(&paths.state);
    if decision_evidence_truncated {
        reasons.push("decision evidence scan limit reached".into());
    }
    let mut projected = Vec::with_capacity(rows.len());
    for task in rows {
        let id = task["id"].as_str().unwrap_or("");
        let coordination = task.get("coordination").filter(|value| value.is_object());
        let owner_home = coordination
            .and_then(|row| row["owner_home"].as_str())
            .unwrap_or_else(|| paths.home.to_str().unwrap_or("legacy-unknown"));
        let task_key = multplx_domain::lifecycle::subagent_model::qualified_task_id(owner_home, id);
        if task
            .get("coordination_error")
            .is_some_and(|value| !value.is_null())
        {
            reasons.push(format!("task {id} coordination metadata unavailable"));
        }
        let backlog = task.get("backlog").filter(|value| value.is_object());
        let title = backlog
            .and_then(|row| row["title"].as_str())
            .or_else(|| {
                coordination
                    .and_then(|row| row["briefs"].as_array())
                    .and_then(|briefs| briefs.last())
                    .and_then(|brief| brief["scope"].as_str())
            })
            .filter(|value| !value.is_empty())
            .unwrap_or(id);
        let parent_id = coordination.and_then(|row| row["parent_id"].as_str());
        let root_id = coordination.and_then(|row| row["root_id"].as_str());
        let children = rows
            .iter()
            .filter(|candidate| {
                candidate
                    .pointer("/coordination/parent_id")
                    .and_then(Value::as_str)
                    == Some(id)
            })
            .filter_map(|candidate| {
                let child = candidate["id"].as_str()?;
                let child_home = candidate
                    .pointer("/coordination/owner_home")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| paths.home.to_str().unwrap_or("legacy-unknown"));
                Some(
                    multplx_domain::lifecycle::subagent_model::qualified_task_id(child_home, child),
                )
            })
            .collect::<Vec<_>>();
        let project = coordination
            .and_then(|row| row.get("project"))
            .filter(|value| value.is_object())
            .map(|binding| json!({
                "id": binding["project_id"],
                "display_name": projects.iter().find(|project| project["id"] == binding["project_id"]).map(|project| project["display_name"].clone()).unwrap_or_else(|| binding["project_id"].clone()),
                "checkout_id": binding["checkout_id"],
                "path": binding["canonical_path"],
                "common_git_identity": binding["common_git_identity"],
                "registered": projects.iter().any(|project| project["id"] == binding["project_id"] && project["registered"] == true)
            }))
            .unwrap_or_else(|| {
                let legacy = task["project"].as_str().filter(|value| !value.is_empty());
                json!({"id":legacy,"display_name":legacy,"checkout_id":Value::Null,"path":task.pointer("/paths/worktree/path"),"common_git_identity":Value::Null,"registered":false})
            });
        let dependencies = coordination
            .and_then(|row| row.pointer("/schedule/dependencies"))
            .and_then(Value::as_array)
            .map(|dependencies| {
                dependencies
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|dependency| {
                        let dependency_key = multplx_domain::lifecycle::subagent_model::qualified_task_id(owner_home, dependency);
                        let state = task_states.get(&dependency_key);
                        json!({"task_id":dependency,"task_key":dependency_key,"state":state,"blocking":state.is_none_or(|state| state != "completed")})
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let canonical_decisions = coordination
            .and_then(|row| row.pointer("/schedule/decisions"))
            .and_then(Value::as_array)
            .map(|decisions| {
                decisions.iter().map(|decision| json!({
                    "id":decision["id"],"question":decision["question"],"brief_revision":decision["brief_revision"],
                    "workflow_revision":decision["workflow_revision"],"answer":decision["answer"],
                    "waiting_since":decision_waiting_since(&decision_evidence, id, decision),"source":"coordination"
                })).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let decisions = if canonical_decisions.is_empty() {
            task.pointer("/hints/open_decisions")
                .and_then(Value::as_array)
                .map(|decisions| decisions.iter().map(|decision| json!({
                    "id":decision["key"],"question":decision["summary"],"brief_revision":Value::Null,
                    "workflow_revision":Value::Null,"answer":Value::Null,
                    "waiting_since":decision_waiting_since(&decision_evidence, id, decision),"source":"status-event"
                })).collect())
                .unwrap_or_default()
        } else {
            canonical_decisions
        };
        let workflow = workflow_rows
            .iter()
            .find(|run| {
                run["id"] == id
                    || run["stages"].as_array().is_some_and(|stages| {
                        stages.iter().any(|stage| stage["task_id"] == id)
                    })
            })
            .map(|run| {
                let stage = run["stages"]
                    .as_array()
                    .and_then(|stages| stages.iter().find(|stage| stage["task_id"] == id));
                json!({"id":run["id"],"revision":run["workflow_revision"],"name":run["workflow"],"status":run["status"],"current_stage":run["current_stage"],"task_stage":stage,"updated_at":run["updated_at"]})
            })
            .unwrap_or_else(|| json!({"id":Value::Null,"revision":Value::Null,"name":Value::Null,"status":Value::Null,"current_stage":Value::Null,"updated_at":Value::Null}));
        let attempts = coordination
            .and_then(|row| row["prior_attempts"].as_array())
            .cloned()
            .unwrap_or_default();
        let mut sessions = Vec::new();
        if let Some(row) = coordination {
            let runtime = &row["runtime"];
            if runtime.is_object()
                && ["session_id", "endpoint"].iter().any(|field| {
                    runtime[*field]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
                })
            {
                sessions.push(json!({"attempt_id":row.pointer("/attempt/id"),"provider":runtime["provider"],"session_id":runtime["session_id"],"endpoint":runtime["endpoint"],"persistent":row["persistent"],"current":true,"state":task.pointer("/current_state/state")}));
            }
            if let Some(retained) = row["retained_executions"].as_array() {
                sessions.extend(retained.iter().map(|execution| json!({"attempt_id":execution.pointer("/attempt/id"),"provider":execution.pointer("/runtime/provider"),"session_id":execution.pointer("/runtime/session_id"),"endpoint":execution.pointer("/runtime/endpoint"),"persistent":true,"current":false,"state":"retained"})));
            }
            let mut native_sessions = BTreeMap::new();
            for observation in row["native_observations"]
                .as_array()
                .map_or(&[][..], Vec::as_slice)
            {
                let Some(child_id) = observation["child_id"].as_str() else {
                    continue;
                };
                let provider = observation["provider"].as_str().unwrap_or("unknown");
                let parent_attempt = observation
                    .pointer("/parent_attempt/id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                native_sessions.insert(
                    format!("{provider}\0{child_id}\0{parent_attempt}"),
                    json!({"attempt_id":observation.pointer("/parent_attempt/id"),"provider":provider,"session_id":child_id,"endpoint":Value::Null,"persistent":false,"current":observation["state"]=="started","state":observation["state"],"source":"native-observation","observed_at":observation["observed_at"]}),
                );
            }
            sessions.extend(native_sessions.into_values());
        }
        let briefs = coordination
            .and_then(|row| row["briefs"].as_array())
            .map_or(&[][..], Vec::as_slice);
        let accepted_revision =
            coordination.and_then(|row| row["accepted_brief_revision"].as_u64());
        let accepted_brief = briefs
            .iter()
            .find(|brief| brief["revision"].as_u64() == accepted_revision);
        let delivery = coordination.map(|row| &row["delivery"]);
        let current_commit = delivery.and_then(|delivery| delivery["current_commit"].as_str());
        let current_attempt =
            coordination.and_then(|row| row.pointer("/attempt/id").and_then(Value::as_str));
        let current_generation =
            coordination.and_then(|row| row.pointer("/attempt/generation").and_then(Value::as_u64));
        let current_delivery = delivery
            .and_then(|delivery| delivery["history"].as_array())
            .and_then(|history| {
                history.iter().rev().find(|evidence| {
                    evidence["commit"].as_str() == current_commit
                        && evidence["attempt_id"].as_str() == current_attempt
                        && evidence["attempt_generation"].as_u64() == current_generation
                        && evidence["brief_revision"].as_u64() == accepted_revision
                })
            });
        let observation_source = task
            .pointer("/current_state/source")
            .and_then(Value::as_str);
        let legacy_unknown = coordination
            .and_then(|row| row["legacy_unknown"].as_bool())
            .unwrap_or(false);
        let task_partial = coordination.is_none()
            || legacy_unknown
            || observation_source.is_none()
            || matches!(observation_source, Some("error" | "timeout" | "none"));
        projected.push(json!({
            "key":task_key,"id":id,"title":title,"parent_id":parent_id,"root_id":root_id,"children":children,"project":project,
            "state":coordination.and_then(|row|row.pointer("/schedule/state")).unwrap_or(&task["current_state"]["state"]),
            "priority":coordination.and_then(|row|row.pointer("/schedule/priority")).and_then(Value::as_i64).or_else(||backlog.and_then(|row|row["priority"].as_str()).and_then(|value|value.parse().ok())).unwrap_or(0),
            "role":coordination.and_then(|row|row["role"].as_str()),
            "owner":{"home":owner_home,"coordinator":coordination.and_then(|row|row["owning_coordinator"].as_str()),"parent_id":parent_id},
            "attempt":coordination.map(|row|row["attempt"].clone()).unwrap_or(Value::Null),"prior_attempts":attempts,
            "brief":{"revision":accepted_revision,"digest":coordination.and_then(|row|row["accepted_brief_digest"].as_str()),"path":coordination.and_then(|row|row["accepted_brief_path"].as_str()),"scope":accepted_brief.and_then(|brief|brief["scope"].as_str()),"research":accepted_brief.and_then(|brief|brief["source_artifacts"].as_array()).cloned().unwrap_or_default()},
            "workflow":workflow,"dependencies":dependencies,"decisions":decisions,
            "evidence":{"report":task.pointer("/paths/report").cloned().unwrap_or(Value::Null),"delivery":current_delivery,"history":delivery.and_then(|delivery|delivery["history"].as_array()).cloned().unwrap_or_default(),"review_queue":review_queue.get(&task_key),"pr":{"url":current_delivery.and_then(|evidence|evidence["pr_url"].as_str()).or_else(||task.pointer("/pr/url").and_then(Value::as_str)),"source":if current_delivery.is_some(){"revision-bound-delivery"}else{task.pointer("/pr/source").and_then(Value::as_str).unwrap_or("absent")}}},
            "allocation":coordination.and_then(|row|row.get("allocation")).filter(|value|!value.is_null()).map(|binding|json!({"binding":binding,"observation":task["allocation_observation"]})).unwrap_or(Value::Null),
            "sessions":sessions,"native_observations":coordination.and_then(|row|row["native_observations"].as_array()).cloned().unwrap_or_default(),"latest_change":{"state":task.pointer("/paths/status_log/last_event/state"),"summary":task.pointer("/paths/status_log/last_event/note"),"raw":task.pointer("/paths/status_log/last_event/raw"),"observed_at":Value::Null},
            "freshness":{"status":if task_partial{"partial"}else{"fresh"},"age_seconds":0,"partial":task_partial,"reasons":if coordination.is_none(){vec!["canonical coordination metadata unavailable"]}else if legacy_unknown{vec!["legacy task identity remains incomplete"]}else if task_partial{vec!["runtime observation unavailable"]}else{Vec::<&str>::new()}}
        }));
    }
    let projected_ids = projected
        .iter()
        .filter_map(|task| task["id"].as_str())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    for record in backlog_root
        .pointer("/records")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|record| record["structured"] == true)
        .filter(|record| {
            record["id"]
                .as_str()
                .is_some_and(|id| !projected_ids.contains(id))
        })
    {
        let Some(id) = record["id"].as_str() else {
            continue;
        };
        let key = multplx_domain::lifecycle::subagent_model::qualified_task_id(
            paths.home.to_str().unwrap_or("legacy-unknown"),
            id,
        );
        projected.push(json!({
            "key":key,"id":id,"title":record["title"],"parent_id":Value::Null,"root_id":Value::Null,"children":[],
            "project":{"id":record["repo"],"display_name":record["repo"],"checkout_id":Value::Null,"path":Value::Null,"common_git_identity":Value::Null,"registered":false},
            "state":record["state"],"priority":record["priority"].as_str().and_then(|value|value.parse::<i64>().ok()).unwrap_or(0),"role":Value::Null,
            "owner":{"home":paths.home,"coordinator":Value::Null,"parent_id":Value::Null},"attempt":Value::Null,"prior_attempts":[],
            "brief":{"revision":Value::Null,"digest":Value::Null,"path":Value::Null,"scope":Value::Null,"research":[]},
            "workflow":{"id":Value::Null,"revision":Value::Null,"name":Value::Null,"status":Value::Null,"current_stage":Value::Null,"updated_at":Value::Null},
            "dependencies":record["blocked_by_ids"].as_array().map(|ids|ids.iter().filter_map(Value::as_str).map(|dependency|json!({"task_id":dependency,"task_key":multplx_domain::lifecycle::subagent_model::qualified_task_id(paths.home.to_str().unwrap_or("legacy-unknown"),dependency),"state":Value::Null,"blocking":true})).collect::<Vec<_>>()).unwrap_or_default(),
            "decisions":[],"evidence":{"report":{"path":record["report_path"],"present":record["report_path"].is_string()},"delivery":Value::Null,"history":[],"review_queue":Value::Null,"pr":{"url":record["pr_url"],"source":"backlog"}},
            "allocation":Value::Null,"sessions":[],"native_observations":[],"latest_change":{"state":Value::Null,"summary":Value::Null,"raw":Value::Null,"observed_at":Value::Null},
            "freshness":{"status":"partial","age_seconds":0,"partial":true,"reasons":["execution metadata unavailable"]}
        }));
    }
    let mut task_keys = projected
        .iter()
        .filter_map(|task| task["key"].as_str())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let mut project_ids = projects
        .iter()
        .filter_map(|project| project["id"].as_str())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    for domain in domains["records"].as_array().map_or(&[][..], Vec::as_slice) {
        let Some(child_portfolio) = domain.get("portfolio").filter(|value| value.is_object())
        else {
            continue;
        };
        for project in child_portfolio["projects"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
        {
            if project["id"]
                .as_str()
                .is_some_and(|id| project_ids.insert(id.to_owned()))
            {
                projects.push(project.clone());
            }
        }
        for task in child_portfolio["tasks"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
        {
            if task["key"]
                .as_str()
                .is_some_and(|key| task_keys.insert(key.to_owned()))
            {
                projected.push(task.clone());
            }
        }
    }
    projects.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let sessions = projected
        .iter()
        .map(|task| task["sessions"].as_array().map_or(0, Vec::len))
        .sum::<usize>();
    let attempts = projected
        .iter()
        .map(|task| {
            usize::from(!task["attempt"].is_null())
                + task["prior_attempts"].as_array().map_or(0, Vec::len)
        })
        .sum::<usize>();
    let task_partial = projected
        .iter()
        .any(|task| task.pointer("/freshness/partial") == Some(&Value::Bool(true)));
    if task_partial {
        reasons.push("one or more task observations are partial".into());
    }
    reasons.sort();
    reasons.dedup();
    let partial = task_partial || !reasons.is_empty() || domains["complete"] != true;
    if domains["complete"] != true {
        reasons.push("domain projection incomplete".into());
    }
    let coordinator_tasks = projected
        .iter()
        .filter(|task| task["role"] == "sub-orchestrator")
        .count();
    let useful_tasks = projected.len().saturating_sub(coordinator_tasks);
    let record_total = projected.len();
    projected.sort_by(|left, right| left["key"].as_str().cmp(&right["key"].as_str()));
    projected.truncate(env_usize("MX_SNAPSHOT_PORTFOLIO_TASKS", 100));
    let record_shown = projected.len();
    if record_total > record_shown {
        reasons.push("portfolio task record limit reached".into());
    }
    let partial = partial || record_total > record_shown;
    json!({"schema":"mx-portfolio.v1","generated":generated,"observed_at":generated,"freshness":{"status":if partial{"partial"}else{"fresh"},"age_seconds":0,"partial":partial,"reasons":reasons},"counts":{"tasks":useful_tasks,"coordinators":coordinator_tasks,"records":record_total,"shown":record_shown,"truncated":record_total-record_shown,"sessions":sessions,"attempts":attempts,"projects":projects.len(),"domains":domains["total"]},"projects":projects,"tasks":projected})
}

/// Retained report evidence is the durable owner of a question's creation
/// time. The scan is intentionally bounded; missing or older evidence leaves
/// `waiting_since` unknown instead of substituting task or observation time.
fn decision_evidence(state: &Path) -> (Vec<Value>, bool) {
    let limit = env_usize("MX_SNAPSHOT_DECISION_EVIDENCE", 512);
    let mut paths = fs::read_dir(state.join("evidence"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|value| value.to_str()) == Some("json")).then_some(path)
        })
        .take(limit.saturating_add(1))
        .collect::<Vec<_>>();
    paths.sort();
    let truncated = paths.len() > limit;
    paths.truncate(limit);
    let rows = paths
        .into_iter()
        .filter_map(|path| {
            let bytes = multplx_core::filesystem::read_bounded_regular(&path, 128 * 1024).ok()?;
            let row: Value = serde_json::from_slice(&bytes).ok()?;
            let created = row.pointer("/envelope/created_at")?.as_str()?;
            if row["accepted"] != true
                || row.pointer("/envelope/kind").and_then(Value::as_str) != Some("needs-decision")
                || row
                    .pointer("/envelope/task_id")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                || time::OffsetDateTime::parse(
                    created,
                    &time::format_description::well_known::Rfc3339,
                )
                .is_err()
                || !row["decision"].is_object()
            {
                return None;
            }
            Some(row)
        })
        .collect();
    (rows, truncated)
}

fn decision_waiting_since(evidence: &[Value], task_id: &str, decision: &Value) -> Value {
    evidence
        .iter()
        .filter(|row| {
            row.pointer("/decision/task_id").and_then(Value::as_str) == Some(task_id)
                && row.pointer("/decision/id") == decision.get("id").or_else(|| decision.get("key"))
                && row.pointer("/decision/question")
                    == decision.get("question").or_else(|| decision.get("summary"))
                && decision.get("brief_revision").is_none_or(|revision| {
                    revision.is_null() || row.pointer("/decision/brief_revision") == Some(revision)
                })
                && decision.get("workflow_revision").is_none_or(|revision| {
                    revision.is_null()
                        || row.pointer("/decision/workflow_revision") == Some(revision)
                })
        })
        .filter_map(|row| row.pointer("/envelope/created_at").and_then(Value::as_str))
        .min()
        .map_or(Value::Null, |value| Value::String(value.to_owned()))
}

fn domain_projection(
    paths: &Paths,
    tasks: &Value,
    daemon_current: &Value,
    generated: &str,
) -> Value {
    let task_rows = tasks.as_array().map_or(&[][..], Vec::as_slice);
    let limit = env_usize("MX_SNAPSHOT_DOMAINS", 20);
    let depth_limit = env_usize("MX_SNAPSHOT_DOMAIN_DEPTH", 8);
    let budget = Duration::from_millis(env_usize("MX_SNAPSHOT_DOMAIN_BUDGET_MS", 3_000) as u64);
    let started = Instant::now();
    let cached = array(daemon_current, "records")
        .iter()
        .filter(|record| {
            record["valid"] == true
                && record
                    .pointer("/provenance/selected")
                    .and_then(Value::as_str)
                    == Some("structured-home")
        })
        .filter_map(|record| {
            record["home"]
                .as_str()
                .map(|home| (home.to_owned(), record.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut queue = task_rows
        .iter()
        .filter(|task| {
            task.pointer("/coordination/role").and_then(Value::as_str) == Some("sub-orchestrator")
        })
        .cloned()
        .map(|task| (task, 0usize))
        .collect::<VecDeque<_>>();
    let mut records = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut omitted = 0usize;
    while let Some((coordinator, depth)) = queue.pop_front() {
        let id = coordinator["id"].as_str().unwrap_or("");
        let owner_home = coordinator
            .pointer("/coordination/owner_home")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let qualified =
            multplx_domain::lifecycle::subagent_model::qualified_task_id(owner_home, id);
        if !seen.insert(qualified.clone()) {
            continue;
        }
        if records.len() >= limit {
            omitted = omitted.saturating_add(1 + queue.len());
            break;
        }
        let binding = &coordinator["coordination"]["domain"];
        let runtime_home = coordinator["coordination"]["persistent_home"]
            .as_str()
            .or_else(|| coordinator["coordination"]["owner_home"].as_str());
        let validated_home = runtime_home
            .ok_or_else(|| "runtime home unavailable".to_owned())
            .and_then(|home| {
                if cached.contains_key(home) {
                    Ok(PathBuf::from(home))
                } else {
                    validate_home(paths, id, Path::new(home))
                        .map_err(|error| format!("invalid home: {error}"))
                }
            });
        let summary = validated_home
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|home| {
                if let Some(summary) = cached.get(home.to_string_lossy().as_ref()) {
                    Ok(summary.clone())
                } else if started.elapsed() >= budget {
                    Err("domain observation budget exhausted".into())
                } else {
                    read_child_summary_with_timeout(
                        paths,
                        home,
                        generated,
                        budget.saturating_sub(started.elapsed()),
                    )
                }
            });
        if depth < depth_limit
            && let Ok(summary) = &summary
        {
            for nested in array(summary, "domains") {
                queue.push_back((nested.clone(), depth + 1));
            }
        } else if depth >= depth_limit
            && summary
                .as_ref()
                .is_ok_and(|value| !array(value, "domains").is_empty())
        {
            omitted =
                omitted.saturating_add(array(summary.as_ref().expect("checked"), "domains").len());
        }
        let channel = validated_home
            .as_ref()
            .map(|home| {
                if started.elapsed() >= budget {
                    return json!({"available":false,"health":Value::Null,"reason":"domain observation budget exhausted"});
                }
                match multplx_domain::lifecycle::parent_channel::inspect(
                    &home.join("state"),
                ) {
                    Ok(health) => {
                        json!({"available":true,"health":health,"reason":Value::Null})
                    }
                    Err(error) => {
                        json!({"available":false,"health":Value::Null,"reason":error})
                    }
                }
            })
            .unwrap_or_else(|_| {
                json!({"available":false,"health":Value::Null,"reason":validated_home.as_ref().err().cloned().unwrap_or_else(|| "runtime home unavailable".into())})
            });
        let count = |key: &str| {
            summary.as_ref().ok().and_then(|summary| {
                summary
                    .pointer(&format!("/counts/{key}"))
                    .and_then(Value::as_u64)
            })
        };
        let channel_truncated = channel
            .pointer("/health/scan_truncated")
            .and_then(Value::as_bool)
            == Some(true);
        let undelivered = (!channel_truncated)
            .then(|| {
                channel
                    .pointer("/health/pending_inbox")
                    .and_then(Value::as_u64)
                    .zip(
                        channel
                            .pointer("/health/pending_outbox")
                            .and_then(Value::as_u64),
                    )
                    .map(|(inbox, outbox)| inbox.saturating_add(outbox))
            })
            .flatten();
        let coordinator_session = coordinator
            .pointer("/endpoint/exists")
            .and_then(Value::as_bool)
            .map(u64::from);
        let partial = summary.as_ref().is_err()
            || summary
                .as_ref()
                .is_ok_and(|summary| summary["valid"] != true)
            || channel["available"] != true
            || channel_truncated;
        let observation_reason = summary
            .as_ref()
            .err()
            .cloned()
            .or_else(|| {
                summary
                    .as_ref()
                    .ok()
                    .and_then(|summary| summary["reason"].as_str().map(str::to_owned))
            })
            .or_else(|| channel_truncated.then(|| "parent-channel scan was truncated".into()));
        let children = summary
            .as_ref()
            .ok()
            .map(|summary| summary["endpoints"].clone())
            .unwrap_or_else(|| json!([]));
        let canonical_tasks = summary
            .as_ref()
            .ok()
            .map(|summary| summary["tasks"].clone())
            .unwrap_or_else(|| json!([]));
        let workflows = summary
            .as_ref()
            .ok()
            .map(|summary| summary["workflow_runs"].clone())
            .unwrap_or_else(|| json!([]));
        let child_portfolio = summary
            .as_ref()
            .ok()
            .map(|summary| summary["portfolio"].clone())
            .unwrap_or(Value::Null);
        records.push(json!({
            "domain_id": binding["domain_id"],
            "scope": binding["scope"],
            "projects": binding["projects"],
            "idea_id": binding["idea_id"],
            "scope_revision": binding["scope_revision"],
            "assignment_generation": binding["assignment_generation"],
            "coordinator": {
                "id": id,
                "qualified_id": qualified,
                "owner_home": coordinator["coordination"]["owner_home"],
                "owner_state": coordinator["coordination"]["owner_state"],
                "runtime_home": runtime_home,
                "validated_home": validated_home.as_ref().ok(),
                "parent_id": coordinator["coordination"]["parent_id"],
                "root_id": coordinator["coordination"]["root_id"],
                "attempt": coordinator["coordination"]["attempt"],
                "endpoint": coordinator["endpoint"]
            },
            "channel": channel,
            "observation": {
                "generated": generated,
                "age_seconds": if summary.is_ok() { Some(0u64) } else { None },
                "partial": partial,
                "reason": observation_reason,
                "validated_home": validated_home.as_ref().ok()
            },
            "counts": {
                "useful_tasks": count("useful_tasks"),
                "worker_sessions": count("worker_sessions"),
                "coordinator_sessions": count("coordinator_sessions").zip(coordinator_session).map(|(children, current)| children.saturating_add(current)),
                "current_attempts_known": count("attempts_known"),
                "attempts_unavailable": count("tasks_total").zip(count("attempts_known")).map(|(tasks, attempts)| tasks.saturating_sub(attempts)),
                "undelivered_outcomes": undelivered
            },
            "children": children,
            "tasks": canonical_tasks,
            "workflow_runs": workflows,
            "portfolio": child_portfolio
        }));
    }
    let total = records.len().saturating_add(omitted);
    json!({
        "records": records,
        "total": total,
        "shown": records.len(),
        "truncated": omitted,
        "complete": omitted == 0 && records.iter().all(|record| record.pointer("/observation/partial") == Some(&Value::Bool(false)))
    })
}

fn native_observations(state: &Path) -> Value {
    let directory = state.join("native-delegations");
    let mut rows = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                return None;
            }
            let bytes = multplx_core::filesystem::read_bounded_regular(&path, 1024 * 1024).ok()?;
            let value: Value = serde_json::from_slice(&bytes).ok()?;
            (value["schema"] == "mx-native-delegation-evidence.v1").then_some(value)
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left["observation"]["observation_id"]
            .as_str()
            .cmp(&right["observation"]["observation_id"].as_str())
    });
    Value::Array(rows)
}

#[derive(Clone)]
struct DaemonRoute {
    id: String,
    home: Option<PathBuf>,
    registered: Option<bool>,
    error: Option<String>,
    parent: Value,
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_duration(name: &str, default: u64) -> Duration {
    Duration::from_secs(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default),
    )
}

fn registry(paths: &Paths, generated: &str) -> Value {
    let path = paths.data.join("daemons.md");
    let empty = |present, available, complete, reason: Option<&str>| json!({"present":present,"available":available,"complete":complete,"reason":reason,"provenance":"registered-table","path":path,"freshness":{"status":if available{"fresh"}else{"unavailable"},"observed_at":generated},"records":[],"input_truncated":false,"records_truncated":false,"reasons":reason.into_iter().collect::<Vec<_>>(),"lines_in_window":0,"records_in_window":0});
    if !path.is_file() {
        return empty(false, true, true, None);
    }
    if file_mode(&path).is_some_and(|mode| mode & 0o444 == 0) {
        return empty(
            true,
            false,
            false,
            Some("registered daemon table is unreadable"),
        );
    }
    let max_bytes = env_usize("MX_SNAPSHOT_REGISTRY_BYTES", 65_536);
    let max_lines = env_usize("MX_SNAPSHOT_REGISTRY_LINES", 256);
    let max_records = env_usize("MX_SNAPSHOT_REGISTRY_RECORDS", 40);
    let Ok(bytes) = fs::read(&path) else {
        return empty(
            true,
            false,
            false,
            Some("registered daemon table is unreadable"),
        );
    };
    let byte_truncated = bytes.len() > max_bytes;
    let mut window = &bytes[..bytes.len().min(max_bytes)];
    if byte_truncated {
        window = window
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(&[][..], |index| &window[..=index]);
    }
    let text = String::from_utf8_lossy(window);
    let all_lines = text.lines().collect::<Vec<_>>();
    let line_truncated = all_lines.len() > max_lines;
    let lines = &all_lines[..all_lines.len().min(max_lines)];
    let line = Regex::new(r"^-\s+(\S+)").unwrap();
    let home = Regex::new(r"\(home:\s*([^;)]*);").unwrap();
    let mut records = lines
        .iter()
        .filter_map(|line_text| {
            let id = line.captures(line_text)?.get(1)?.as_str();
            let home = home
                .captures(line_text)
                .and_then(|capture| capture.get(1))
                .map(|value| value.as_str().trim())
                .filter(|value| !value.is_empty());
            Some(json!({"id":id,"home":home,"registered":true,"registry_error":if home.is_some(){Value::Null}else{json!("registry entry has no home")}}))
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let mut counts = BTreeMap::new();
    for record in &records {
        *counts
            .entry(record["id"].as_str().unwrap_or("").to_owned())
            .or_insert(0usize) += 1;
    }
    records.dedup_by(|left, right| {
        if left["id"] == right["id"] {
            if counts[left["id"].as_str().unwrap_or("")] > 1 {
                left["registry_error"] = json!("duplicate daemon id in registry");
            }
            true
        } else {
            false
        }
    });
    let records_in_window = records.len();
    let records_truncated = records_in_window > max_records;
    records.truncate(max_records);
    let mut reasons = Vec::new();
    if byte_truncated {
        reasons.push("byte_limit")
    }
    if line_truncated {
        reasons.push("line_limit")
    }
    if records_truncated {
        reasons.push("record_limit")
    }
    json!({"present":true,"available":true,"complete":reasons.is_empty(),"reason":Value::Null,"provenance":"registered-table","path":path,"freshness":{"status":"fresh","observed_at":generated},"records":records,"input_truncated":byte_truncated||line_truncated,"records_truncated":records_truncated,"reasons":reasons,"lines_in_window":lines.len(),"records_in_window":records_in_window})
}

fn stat_value(path: &Path, bsd_format: &str, gnu_format: &str) -> Option<u64> {
    let darwin = Command::new("uname")
        .output()
        .ok()
        .is_some_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "Darwin");
    let (flag, format) = if darwin {
        ("-f", bsd_format)
    } else {
        ("-c", gnu_format)
    };
    let output = Command::new("stat")
        .args([flag, format])
        .arg(path)
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn file_mode(path: &Path) -> Option<u32> {
    stat_value(path, "%Lp", "%a").and_then(|mode| u32::from_str_radix(&mode.to_string(), 8).ok())
}

fn validate_home(paths: &Paths, id: &str, home: &Path) -> Result<PathBuf, String> {
    if !home.is_absolute() {
        return Err("registered path is not absolute".into());
    }
    let resolved = home
        .canonicalize()
        .map_err(|_| "not a directory".to_owned())?;
    if !resolved.is_dir() {
        return Err("not a directory".into());
    }
    let active = paths
        .home
        .canonicalize()
        .unwrap_or_else(|_| paths.home.clone());
    let root = paths
        .root
        .canonicalize()
        .unwrap_or_else(|_| paths.root.clone());
    if resolved == Path::new("/") {
        return Err("daemon home cannot be the filesystem root".into());
    }
    if resolved == active {
        return Err("daemon home cannot be the active Multplx home".into());
    }
    if resolved == root {
        return Err("daemon home cannot be the Multplx repo".into());
    }
    if resolved.starts_with(&active) {
        return Err("daemon home cannot be inside the active Multplx home".into());
    }
    if resolved.starts_with(&root) {
        return Err("daemon home cannot be inside the Multplx repo".into());
    }
    if active.starts_with(&resolved) {
        return Err("daemon home cannot be an ancestor of the active Multplx home".into());
    }
    if root.starts_with(&resolved) {
        return Err("daemon home cannot be an ancestor of the Multplx repo".into());
    }
    for name in ["data", "state", "config", "projects"] {
        let candidate = resolved.join(name);
        if candidate.exists() {
            #[cfg(unix)]
            if fs::metadata(&candidate)
                .is_ok_and(|metadata| metadata.permissions().mode() & 0o500 == 0)
            {
                return Err(format!("daemon {name} directory is unreadable"));
            }
            let child = candidate
                .canonicalize()
                .map_err(|_| format!("daemon {name} directory cannot be resolved"))?;
            if !child.is_dir() || !child.starts_with(&resolved) || child == resolved {
                return Err(format!(
                    "daemon {name} directory must resolve inside the daemon home"
                ));
            }
        }
    }
    let marker = resolved.join(".mx-daemon-home");
    if fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("daemon marker must not be a symlink".into());
    }
    let marker_id = fs::read_to_string(&marker)
        .map_err(|_| "not a seeded daemon home".to_owned())?
        .trim()
        .to_owned();
    if marker_id != id {
        return Err(format!(
            "marked for daemon {}, expected {id}",
            if marker_id.is_empty() {
                "unknown"
            } else {
                &marker_id
            }
        ));
    }
    if !resolved.join("AGENTS.md").is_file() {
        return Err("not a Multplx home (missing AGENTS.md)".into());
    }
    if !resolved.join("bin").is_dir() {
        return Err("not a Multplx home (missing bin/)".into());
    }
    Ok(resolved)
}

fn read_child_summary_with_timeout(
    paths: &Paths,
    home: &Path,
    generated: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let executable = std::env::current_exe().map_err(|_| "structured home snapshot failed")?;
    let mut command = Command::new(executable);
    command
        .args(["session", "mx-system-snapshot.sh", "--daemon-home-summary"])
        .env("MX_ROOT_OVERRIDE", &paths.root)
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", home.join("state"))
        .env("MX_DATA_OVERRIDE", home.join("data"))
        .env("MX_CONFIG_OVERRIDE", home.join("config"))
        .env("MX_PROJECTS_OVERRIDE", home.join("projects"))
        .env("MX_SNAPSHOT_NOW", generated);
    let limit = env_usize("MX_SNAPSHOT_DAEMON_MAX_BYTES", 262_144);
    match run_bounded(command, timeout, limit + 1) {
        TimedOutput::TimedOut => Err("structured home snapshot timed out".into()),
        TimedOutput::StartFailed => Err("structured home snapshot failed".into()),
        TimedOutput::Completed { status, .. } if status != 0 => {
            Err("structured home snapshot failed".into())
        }
        TimedOutput::Completed { stdout, .. } if stdout.len() > limit => {
            Err("structured home snapshot exceeded byte limit".into())
        }
        TimedOutput::Completed { stdout, .. } => {
            let summary: Value = serde_json::from_slice(&stdout)
                .map_err(|_| "structured home snapshot was malformed or stale")?;
            let shape = summary["schema"] == "mx-daemon-home-summary.v1"
                && summary["home"].as_str() == home.to_str()
                && summary["generated"] == generated
                && summary["valid"].is_boolean()
                && summary["state"].is_string()
                && [
                    "tasks",
                    "workflow_runs",
                    "active_children",
                    "decisions_open",
                    "holds",
                    "queued",
                    "landed",
                    "endpoints",
                    "domains",
                    "omitted",
                ]
                .iter()
                .all(|key| summary[*key].is_array())
                && summary["counts"].is_object()
                && (summary["portfolio"].is_object()
                    || (summary["valid"] == false && summary["portfolio"].is_null()))
                && summary["invalidity"].is_object();
            shape
                .then_some(summary)
                .ok_or_else(|| "structured home snapshot was malformed or stale".into())
        }
    }
}

fn task_event(task: &Value, generated: &str, summary: &Value) -> (Value, String, String) {
    let path = task["paths"]["status_log"]["path"].as_str().map(Path::new);
    let status = path
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default();
    let activities = parent_observation(
        &status,
        summary,
        env_usize("MX_SNAPSHOT_PARENT_ACTIVITY_LINES", 256),
        env_usize("MX_SNAPSHOT_PARENT_ACTIVITY_BYTES", 65_536),
        env_usize("MX_SNAPSHOT_PARENT_ACTIVITIES", 20),
    );
    let raw = task["paths"]["status_log"]["last_event"]["raw"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let note = task["paths"]["status_log"]["last_event"]["note"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let age = path
        .and_then(|path| {
            let _ = stat_value(path, "%z", "%s");
            stat_value(path, "%m", "%Y")
        })
        .map(|modified| (epoch() - modified as i64).max(0));
    let mut observed = activities;
    observed["raw"] = json!(raw);
    observed["note"] = json!(note);
    observed["age_seconds"] = json!(age);
    observed["observed_at"] = json!(generated);
    (observed, raw, note)
}

fn terminal_capture(task: &Value, note: &str, generated: &str, compare: bool) -> Value {
    if !compare {
        return terminal_evidence(TimedOutput::StartFailed, generated, note, false);
    }
    let backend = task["backend"].as_str().unwrap_or("");
    let target = task["endpoint"]["target"].as_str().unwrap_or("");
    if backend != "tmux" || target.is_empty() || task["endpoint"]["exists"] == false {
        return json!({"provenance":"parent-direct-report-terminal","trust":"untrusted-supplement","captured":false,"observed_at":generated,"freshness":"unknown","reason":if target.is_empty(){"no recorded endpoint"}else if task["endpoint"]["exists"]==false{"recorded endpoint is absent"}else{"terminal capture unavailable"},"lines":0,"bytes":0,"event_note_seen":false,"contradiction":false});
    }
    let lines = env_usize("MX_SNAPSHOT_TERMINAL_LINES", 8);
    let bytes = env_usize("MX_SNAPSHOT_TERMINAL_BYTES", 4096);
    let mut command = Command::new("tmux");
    command.args([
        "capture-pane",
        "-p",
        "-t",
        target,
        "-S",
        &format!("-{lines}"),
    ]);
    terminal_evidence(
        run_bounded(
            command,
            env_duration("MX_SNAPSHOT_TERMINAL_TIMEOUT", 2),
            bytes,
        ),
        generated,
        note,
        true,
    )
}

fn daemon_current(paths: &Paths, generated: &str, tasks: &Value) -> Value {
    let registry = registry(paths, generated);
    let registered_ids = array(&registry, "records")
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let task_rows = tasks.as_array().map_or(&[][..], Vec::as_slice);
    let mut routes = array(&registry, "records")
        .iter()
        .map(|row| DaemonRoute {
            id: row["id"].as_str().unwrap_or("").to_owned(),
            home: row["home"].as_str().map(PathBuf::from),
            registered: Some(true),
            error: row["registry_error"].as_str().map(str::to_owned),
            parent: task_rows
                .iter()
                .find(|task| task["id"] == row["id"])
                .cloned()
                .unwrap_or_else(|| json!({})),
        })
        .collect::<Vec<_>>();
    for task in task_rows.iter().filter(|task| task["kind"] == "daemon") {
        let id = task["id"].as_str().unwrap_or("");
        if registered_ids.contains(id) {
            continue;
        }
        let complete = registry["complete"] == true;
        routes.push(DaemonRoute {
            id: id.to_owned(),
            home: task["paths"]["home"]["path"].as_str().map(PathBuf::from),
            registered: complete.then_some(false),
            error: Some(if complete {
                "daemon metadata is not registered".into()
            } else {
                "daemon registration is unknown because the registry read is incomplete or unavailable".into()
            }),
            parent: task.clone(),
        });
    }
    routes.sort_by(|left, right| left.id.cmp(&right.id));
    let total_registered = routes
        .iter()
        .filter(|route| route.registered == Some(true))
        .count();
    let total = routes.len();
    let limit = env_usize("MX_SNAPSHOT_DAEMONS", 20);
    if limit > 0 {
        routes.truncate(limit)
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut prepared = Vec::with_capacity(routes.len());
    for route in routes {
        let mut reason = route.error.clone();
        let mut home = route.home.clone();
        if reason.is_none() && home.is_none() {
            reason = Some("no recorded daemon home".into());
        }
        if reason.is_none() {
            match validate_home(paths, &route.id, home.as_ref().expect("checked")) {
                Ok(resolved) if seen.insert(resolved.clone()) => home = Some(resolved),
                Ok(_) => reason = Some("invalid home: duplicate resolved home route".into()),
                Err(error) => reason = Some(format!("invalid home: {error}")),
            }
        }
        prepared.push((route, home, reason));
    }
    let jobs = prepared
        .iter()
        .enumerate()
        .filter(|(_, (_, _, reason))| reason.is_none())
        .map(|(index, (_, home, _))| (index, home.clone().expect("checked")))
        .collect::<VecDeque<_>>();
    let jobs = std::sync::Mutex::new(jobs);
    let summaries = std::sync::Mutex::new(BTreeMap::<usize, Result<Value, String>>::new());
    let collection_started = Instant::now();
    let collection_budget = env_duration("MX_SNAPSHOT_DAEMON_BUDGET", 6);
    let per_home = env_duration("MX_SNAPSHOT_DAEMON_TIMEOUT", 2);
    let workers = env_usize("MX_SNAPSHOT_DAEMON_CONCURRENCY", 4).clamp(1, 16);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let job = jobs.lock().expect("daemon observation queue").pop_front();
                    let Some((index, home)) = job else { break };
                    let remaining = collection_budget.saturating_sub(collection_started.elapsed());
                    let result = if remaining.is_zero() {
                        Err("registered-home observation budget exhausted".into())
                    } else {
                        read_child_summary_with_timeout(
                            paths,
                            &home,
                            generated,
                            remaining.min(per_home),
                        )
                    };
                    summaries
                        .lock()
                        .expect("daemon observation results")
                        .insert(index, result);
                }
            });
        }
    });
    let mut summaries = summaries.into_inner().expect("daemon observation results");
    let mut records = Vec::new();
    for (index, (route, home, mut reason)) in prepared.into_iter().enumerate() {
        let empty_summary = json!({"portfolio":Value::Null,"tasks":[],"workflow_runs":[],"active_children":[],"decisions_open":[],"holds":[],"queued":[],"landed":[],"endpoints":[],"domains":[],"counts":{"active_children":Value::Null,"decisions_open":Value::Null,"holds":Value::Null,"queued":Value::Null,"landed":Value::Null,"endpoints":Value::Null,"useful_tasks":Value::Null,"sessions":Value::Null,"worker_sessions":Value::Null,"coordinator_sessions":Value::Null,"attempts_known":Value::Null,"coordinator_tasks":Value::Null,"tasks_total":Value::Null},"omitted":[]});
        let summary = if reason.is_none() {
            match summaries
                .remove(&index)
                .unwrap_or_else(|| Err("registered-home observation result unavailable".into()))
            {
                Ok(summary) => {
                    if summary["valid"] != true
                        && summary["invalidity"]["kind"] != "child_current_unavailable"
                    {
                        reason = Some(format!(
                            "structured home state invalid: {}",
                            summary["reason"].as_str().unwrap_or("unknown reason")
                        ));
                    }
                    summary
                }
                Err(error) => {
                    reason = Some(error);
                    empty_summary.clone()
                }
            }
        } else {
            empty_summary.clone()
        };
        let (parent_event, raw, note) = task_event(&route.parent, generated, &summary);
        if let Some(reason) = reason {
            let selected = if raw.is_empty() {
                "unknown"
            } else {
                "parent-event-fallback"
            };
            records.push(json!({"id":route.id,"home":home,"registered":route.registered,"current":{"state":"unknown","reason":reason},"valid":false,"reason":reason,"invalidity":Value::Null,"provenance":{"selected":selected,"structured_home":home,"parent_event_role":"fallback-only-not-current"},"freshness":{"status":if raw.is_empty(){"unknown"}else{"historical-event"},"observed_at":generated,"age_seconds":parent_event["age_seconds"]},"portfolio":Value::Null,"tasks":[],"workflow_runs":[],"active_children":[],"decisions_open":[],"holds":[],"queued":[],"landed":[],"endpoints":[],"domains":[],"counts":empty_summary["counts"],"omitted":[],"parent_event":parent_event,"terminal_evidence":terminal_capture(&route.parent,&note,generated,false),"contradiction":false}));
            continue;
        }
        let summary_valid = summary["valid"] == true;
        let current_reason = (!summary_valid).then(|| {
            format!(
                "structured home state invalid: {}",
                summary["reason"].as_str().unwrap_or("unknown reason")
            )
        });
        let contradiction = parent_event["reconciliation"]["contradiction"] == true;
        let compare_terminal = array(&parent_event["reconciliation"], "activities")
            .iter()
            .any(|row| row["verdict"] == "contradicts" && row["summary"] == note);
        let terminal = terminal_capture(&route.parent, &note, generated, compare_terminal);
        records.push(json!({"id":route.id,"home":home,"registered":route.registered,"current":{"state":summary["state"],"reason":current_reason},"valid":summary_valid,"reason":summary["reason"],"invalidity":summary["invalidity"],"provenance":{"selected":"structured-home","structured_home":home,"summary_valid":summary_valid,"trust":if summary_valid{"complete"}else{"partial-structured"},"parent_event_role":"historical-only"},"freshness":{"status":"fresh","observed_at":generated,"age_seconds":0},"portfolio":summary["portfolio"],"tasks":summary["tasks"],"workflow_runs":summary["workflow_runs"],"active_children":summary["active_children"],"decisions_open":summary["decisions_open"],"holds":summary["holds"],"queued":summary["queued"],"landed":summary["landed"],"endpoints":summary["endpoints"],"domains":summary["domains"],"counts":summary["counts"],"omitted":summary["omitted"],"parent_event":parent_event,"terminal_evidence":terminal,"contradiction":contradiction||terminal["contradiction"]==true}));
    }
    let shown = records.len();
    json!({"registry":registry,"records":records,"total_registered":total_registered,"total":total,"shown":shown,"truncated":total-shown})
}

fn daemon_landed(current: &Value) -> Value {
    let mut records = array(current, "records")
        .iter()
        .filter(|row| row["provenance"]["selected"] == "structured-home")
        .flat_map(|row| {
            array(row, "landed").iter().map(|landed| {
                let mut landed = landed.clone();
                landed["home"] = row["home"].clone();
                landed["home_id"] = row["id"].clone();
                landed
            })
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        right["completion"]["date"]
            .as_str()
            .cmp(&left["completion"]["date"].as_str())
            .then(right["id"].as_str().cmp(&left["id"].as_str()))
    });
    let truncated = array(current, "records")
        .iter()
        .filter(|row| {
            row["provenance"]["selected"] == "structured-home"
                && count(row, "landed") > array(row, "landed").len()
        })
        .map(|row| row["home"].clone())
        .collect::<Vec<_>>();
    let unreadable = array(current, "records")
        .iter()
        .filter(|row| {
            row["current"]["state"] == "unknown"
                && row["provenance"]["selected"] != "structured-home"
        })
        .map(|row| {
            row["home"].as_str().map_or_else(
                || format!("<{}: unavailable>", row["id"].as_str().unwrap_or("unknown")),
                str::to_owned,
            )
        })
        .collect::<Vec<_>>();
    let partial = array(current, "records")
        .iter()
        .filter(|row| {
            row["current"]["state"] == "unknown"
                && row["provenance"]["selected"] == "structured-home"
        })
        .map(|row| {
            row["home"].as_str().map_or_else(
                || format!("<{}: partial>", row["id"].as_str().unwrap_or("unknown")),
                str::to_owned,
            )
        })
        .collect::<Vec<_>>();
    json!({"records":records,"truncated":truncated,"unreadable":unreadable,"partial":partial})
}
fn inventory(backlog: &Value, tasks: &Value) -> Value {
    let rows = backlog["records"].as_array().map_or(&[][..], Vec::as_slice);
    let ids = tasks
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|task| task["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let unstructured = rows
        .iter()
        .filter(|row| {
            matches!(row["state"].as_str(), Some("in_flight" | "queued"))
                && row["structured"] == false
        })
        .count();
    let orphan = rows
        .iter()
        .filter(|row| {
            row["state"] == "in_flight"
                && row["structured"] == true
                && row["requires_child_metadata"] == true
        })
        .filter_map(|row| row["id"].as_str())
        .filter(|id| !ids.contains(id))
        .collect::<Vec<_>>();
    json!({"valid":unstructured==0&&orphan.is_empty(),"reason":if unstructured>0{Some("unstructured current backlog row")}else if !orphan.is_empty(){Some("in-flight backlog item has no child metadata")}else{None},"orphan_in_flight":orphan,"unstructured_current_count":unstructured})
}
fn reports(paths: &Paths, backlog: &Value, tasks: &Value) -> Value {
    let mut rows = Vec::new();
    for entry in fs::read_dir(&paths.data).into_iter().flatten().flatten() {
        let report = entry.path().join("report.md");
        if !report.is_file() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let kind = tasks
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["id"] == id))
            .and_then(|row| row["kind"].as_str())
            .or_else(|| {
                backlog["records"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|row| row["id"] == id))
                    .and_then(|row| row["kind"].as_str())
            })
            .unwrap_or("scout");
        rows.push(json!({"id":id,"path":report,"kind":kind}));
    }
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Value::Array(rows)
}
fn epoch() -> i64 {
    std::env::var("MX_SNAPSHOT_NOW_EPOCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| time::OffsetDateTime::now_utc().unix_timestamp())
}
fn watcher(paths: &Paths) -> Value {
    let lock = paths.state.join(".watch.lock");
    let pid = fs::read_to_string(lock.join("pid"))
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok());
    let identity = fs::read_to_string(lock.join("pid-identity")).unwrap_or_default();
    let current = pid.and_then(process_identity);
    let verified = current.as_deref() == Some(identity.trim())
        && fs::read_to_string(lock.join("mx-home"))
            .ok()
            .is_some_and(|v| v.trim() == paths.home.to_string_lossy())
        && fs::read_to_string(lock.join("watcher-path"))
            .ok()
            .is_some_and(|v| {
                v.trim() == paths.source_root.join("bin/mx-watch.sh").to_string_lossy()
            });
    let age = fs::metadata(paths.state.join(".last-watcher-beat"))
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|m| (epoch() - m.as_secs() as i64).max(0) as u64);
    let grace = std::env::var("MX_WATCHER_STALE_GRACE")
        .or_else(|_| std::env::var("MX_GUARD_GRACE"))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    json!({"lock_present":lock.is_dir(),"pid":pid,"identity_verified":verified,"alive":verified,"beacon_age_secs":age,"stale":age.is_none_or(|age|age>=grace),"afk":paths.state.join(".afk").exists()})
}
fn process_identity(pid: u32) -> Option<String> {
    SystemProcessProbe::default()
        .identity(pid)
        .ok()
        .map(|identity| identity.marker)
}
fn wake_queue(paths: &Paths) -> Value {
    let records = fs::read_to_string(paths.state.join(".wake-queue"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split('\t').next()?.parse::<i64>().ok())
        .collect::<Vec<_>>();
    let queue = multplx_core::wake::WakeQueue::new(paths.state.clone());
    match (
        queue.observe_unfinished_count(),
        queue.observe_inbox_items(),
    ) {
        (Ok(unfinished), Ok(inbox)) => {
            let oldest = records
                .iter()
                .copied()
                .chain(inbox.iter().filter_map(|item| {
                    (item.acknowledged_at.is_none()
                        || item.disposition.as_ref().is_some_and(|disposition| {
                            disposition.kind == multplx_core::wake::WakeDispositionKind::Waiting
                        }))
                    .then_some(i64::try_from(item.record.epoch).unwrap_or(i64::MAX))
                }))
                .min();
            json!({"depth":unfinished,"oldest_age_secs":oldest.map(|oldest|(epoch()-oldest).max(0)),"records":inbox,"available":true,"reason":Value::Null})
        }
        (unfinished, inbox) => {
            let reason = unfinished
                .err()
                .or_else(|| inbox.err())
                .map(|error| error.to_string())
                .unwrap_or_else(|| "wake inbox unavailable".to_owned());
            json!({"depth":records.len(),"oldest_age_secs":records.iter().min().map(|oldest|(epoch()-*oldest).max(0)),"records":[],"available":false,"reason":reason})
        }
    }
}
fn headroom_bin(paths: &Paths) -> PathBuf {
    std::env::var_os("MX_SNAPSHOT_HEADROOM_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.source_root.join("bin/mx-headroom.sh"))
}
fn headroom(paths: &Paths) -> (Value, Value) {
    let mut command = Command::new(headroom_bin(paths));
    command
        .arg("--json")
        .env("MX_ROOT_OVERRIDE", &paths.root)
        .env("MX_HOME", &paths.home)
        .env("MX_STATE_OVERRIDE", &paths.state)
        .env("MX_CONFIG_OVERRIDE", &paths.config);
    let output = run_bounded(
        command,
        env_duration("MX_SNAPSHOT_HEADROOM_TIMEOUT", 2),
        65_536,
    );
    match output {
        TimedOutput::Completed {
            status: 0, stdout, ..
        } => serde_json::from_slice(&stdout)
            .map(|v| (v, Value::Null))
            .unwrap_or((Value::Null, json!("headroom check failed"))),
        TimedOutput::Completed { stderr, .. } => (
            Value::Null,
            json!(
                String::from_utf8_lossy(&stderr)
                    .lines()
                    .next()
                    .unwrap_or("headroom check failed")
            ),
        ),
        TimedOutput::TimedOut => (Value::Null, json!("headroom check timed out")),
        TimedOutput::StartFailed => (Value::Null, json!("headroom check failed")),
    }
}
fn dispatch(paths: &Paths) -> Value {
    let mut command = Command::new(headroom_bin(paths));
    command
        .arg("--queue")
        .env("MX_ROOT_OVERRIDE", &paths.root)
        .env("MX_HOME", &paths.home)
        .env("MX_STATE_OVERRIDE", &paths.state)
        .env("MX_CONFIG_OVERRIDE", &paths.config);
    let stdout = match run_bounded(
        command,
        env_duration("MX_SNAPSHOT_HEADROOM_TIMEOUT", 2),
        262_144,
    ) {
        TimedOutput::Completed {
            status: 0, stdout, ..
        } => stdout,
        TimedOutput::Completed { stderr, .. } => {
            return json!({"depth":0,"records":[],"available":false,"reason":String::from_utf8_lossy(&stderr).lines().next().unwrap_or("dispatch queue read failed")});
        }
        TimedOutput::TimedOut => {
            return json!({"depth":0,"records":[],"available":false,"reason":"dispatch queue read timed out"});
        }
        TimedOutput::StartFailed => {
            return json!({"depth":0,"records":[],"available":false,"reason":"dispatch queue read failed"});
        }
    };
    let rows=String::from_utf8_lossy(&stdout).lines().filter_map(|line|{let parts=line.split('\t').collect::<Vec<_>>();(parts.len()==8).then(||json!({"enqueued_at":parts[0].parse::<u64>().ok(),"id":parts[1],"project":parts[2],"profile":{"harness":null_dash(parts[3]),"model":null_dash(parts[4]),"effort":null_dash(parts[5]),"backend":null_dash(parts[6])},"kind":parts[7]}))}).collect::<Vec<_>>();
    json!({"depth":rows.len(),"records":rows,"available":true,"reason":Value::Null})
}
fn null_dash(value: &str) -> Option<&str> {
    (value != "-").then_some(value)
}
fn vplans(_paths: &Paths) -> Value {
    let mut records = fs::read_dir(_paths.state.join(".vplan"))
        .into_iter().flatten().flatten()
        .filter_map(|entry| {
            let path=entry.path();
            (path.extension().and_then(|v|v.to_str())==Some("run") && !fs::symlink_metadata(&path).ok()?.file_type().is_symlink()).then_some(path)
        })
        .map(|path| {
            let fields=meta(&path);
            let port=fields.get("port").and_then(|v|v.parse::<u16>().ok());
            let alive=fields.get("pid").and_then(|v|v.parse::<u32>().ok()).zip(fields.get("pid_identity")).is_some_and(|(pid,identity)|process_identity(pid).as_deref()==Some(identity));
            json!({
                "artifact": fields.get("artifact"),
                "artifact_root": fields.get("artifact_root"),
                "artifact_sha256": fields.get("artifact_sha256"),
                "task_id": fields.get("task"),
                "attempt_id": fields.get("attempt"),
                "brief_revision": fields.get("brief_revision").and_then(|value| value.parse::<u64>().ok()),
                "project_id": fields.get("project_id"),
                "allocation_id": fields.get("allocation_id"),
                "port": port,
                "started_at": fields.get("started_at"),
                "pid_alive": alive,
                "url": port.map(|port|format!("http://127.0.0.1:{port}/"))
            })
        }).collect::<Vec<_>>();
    records.sort_by(|a, b| {
        a["started_at"]
            .as_str()
            .cmp(&b["started_at"].as_str())
            .then(a["artifact"].as_str().cmp(&b["artifact"].as_str()))
    });
    json!({"records":records})
}
fn later(paths: &Paths) -> Value {
    json!({"gate_runs":gate_runs(paths),"workflow_runs":workflow_runs(paths),"deliveries":deliveries(paths),"upstream_drift":upstream(paths),"doctor":{"available":paths.source_root.join("bin/mx-doctor.sh").is_file()},"timeline":{"available":paths.source_root.join("bin/mx-timeline.sh").is_file()}})
}

fn json_file(path: &Path) -> Option<Value> {
    serde_json::from_slice(&multplx_core::filesystem::read_bounded_regular(path, 1024 * 1024).ok()?)
        .ok()
}
fn directories_with_suffix(state: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut rows = fs::read_dir(state)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.is_dir()
                && !fs::symlink_metadata(&path).ok()?.file_type().is_symlink()
                && entry.file_name().to_string_lossy().ends_with(suffix))
            .then_some(path)
        })
        .collect::<Vec<_>>();
    rows.sort();
    rows
}
fn gate_runs(paths: &Paths) -> Value {
    let supported = paths.source_root.join("bin/mx-deep-review.sh").is_file();
    let records=directories_with_suffix(&paths.state,".gate").into_iter().map(|dir|{let id=dir.file_name().unwrap().to_string_lossy().trim_end_matches(".gate").to_owned();let Some(run)=json_file(&dir.join("run.json")) else{return json!({"id":id,"valid":false,"status":"invalid","step":Value::Null,"round":Value::Null,"parked":false,"pending_decision_key":Value::Null,"approved_head":Value::Null,"summary":Value::Null,"risk_level":Value::Null,"findings":Value::Null,"history":[]})};let round=run["round"].as_u64();let findings=round.map_or(0,|round|fs::read_dir(dir.join("findings")).into_iter().flatten().flatten().filter_map(|entry|{let name=entry.file_name().to_string_lossy().into_owned();(name.starts_with(&format!("round-{round:02}-"))&&!name.ends_with("-raw.json")).then(||json_file(&entry.path())).flatten()}).map(|value|array(&value,"findings").len()).sum());let history=array(&run,"history").iter().map(|step|json!({"step":step,"status":step.as_str().map_or(Value::Null,|step|run["steps"][step].clone()),"round":if step==&run["step"]{run["round"].clone()}else{Value::Null}})).collect::<Vec<_>>();json!({"id":id,"valid":true,"status":run["status"].as_str().unwrap_or("unknown"),"step":run["step"],"round":run["round"],"parked":run["status"]=="parked","pending_decision_key":run["pending_decision_key"],"approved_head":run["approved_head"],"summary":run["summary"],"risk_level":run["risk_level"],"findings":findings,"history":history})}).collect::<Vec<_>>();
    json!({"supported":supported,"available":supported&&!records.is_empty(),"records":records})
}
fn workflow_runs(paths: &Paths) -> Value {
    let supported = paths.source_root.join("bin/mx-workflow.sh").is_file();
    let records = directories_with_suffix(&paths.state, ".workflow")
        .into_iter()
        .map(|dir| {
            let id = dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .trim_end_matches(".workflow")
                .to_owned();
            json_file(&dir.join("run.json")).map_or_else(
                || json!({"id":id,"valid":false,"workflow":Value::Null,"workflow_revision":Value::Null,"status":"invalid","current_stage":Value::Null,"message":Value::Null,"created_at":Value::Null,"updated_at":Value::Null,"stages":[]}),
                |run| {
                    let mut stage_paths = fs::read_dir(dir.join("stages"))
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|entry| entry.path())
                        .collect::<Vec<_>>();
                    stage_paths.sort();
                    let stage_total = stage_paths.len();
                    stage_paths.truncate(env_usize("MX_SNAPSHOT_WORKFLOW_STAGES", 64));
                    let mut stages = stage_paths
                        .iter()
                        .filter_map(|path| json_file(path))
                        .collect::<Vec<_>>();
                    stages.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
                    let stage_shown = stages.len();
                    json!({"id":id,"valid":true,"workflow":run["workflow"],"workflow_revision":run["workflow_revision"],"definition_path":run["definition_path"],"definition_sha256":run["definition_sha256"],"plan_revision":run["plan_revision"],"accepted_brief_revision":run["accepted_brief_revision"],"request_correlation":run["request_correlation"],"dependencies":run["dependencies"],"project":run["project"],"status":run["status"].as_str().unwrap_or("unknown"),"current_stage":run["current_stage"],"message":run["message"],"created_at":run["created_at"],"updated_at":run["updated_at"],"stages":stages,"stage_total":stage_total,"stage_shown":stage_shown,"stages_complete":stage_total==stage_shown})
                },
            )
        })
        .collect::<Vec<_>>();
    json!({"supported":supported,"available":supported&&!records.is_empty(),"records":records})
}
fn deliveries(paths: &Paths) -> Value {
    let supported = paths.source_root.join("bin/mx-deliver.sh").is_file();
    let mut records = Vec::new();
    for entry in fs::read_dir(&paths.state).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_file() || fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
        let base = entry.file_name().to_string_lossy().into_owned();
        let (id, state) = if let Some(id) = base.strip_suffix(".ready-to-push.stale") {
            (id, "stale")
        } else if let Some(id) = base.strip_suffix(".ready-to-push") {
            (id, "pending")
        } else if let Some(id) = base.strip_suffix(".delivered") {
            (id, "delivered")
        } else {
            continue;
        };
        let fields = meta(&path);
        let age = fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|m| (epoch() - m.as_secs() as i64).max(0));
        records.push(json!({"id":id,"state":state,"age_secs":age,"valid":fields.get("version").is_some_and(|v|v=="1")&&fields.get("task").is_some_and(|v|v==id),"approval":fields.get("approval"),"branch":fields.get("branch"),"approved_sha":fields.get("approved_sha"),"title":fields.get("title")}))
    }
    records.sort_by(|a, b| {
        a["state"]
            .as_str()
            .cmp(&b["state"].as_str())
            .then(a["id"].as_str().cmp(&b["id"].as_str()))
    });
    json!({"supported":supported,"available":supported&&!records.is_empty(),"records":records})
}
fn upstream(paths: &Paths) -> Value {
    let command = std::env::var_os("MX_SNAPSHOT_UPSTREAM_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.source_root.join("bin/mx-upstream-diff.sh"));
    if !command.is_file() {
        return json!({"available":false,"reason":"upstream reader is not installed","status":Value::Null,"fork_point":Value::Null,"last_reviewed":Value::Null,"upstream_repo":Value::Null,"retired_reason":Value::Null});
    }
    let mut reader = Command::new(command);
    reader.arg("--status").env("MX_ROOT_OVERRIDE", &paths.root);
    match run_bounded(
        reader,
        env_duration("MX_SNAPSHOT_UPSTREAM_TIMEOUT", 2),
        65_536,
    ) {
        TimedOutput::Completed {
            status: 0 | 3,
            stdout,
            ..
        } => {
            let text = String::from_utf8_lossy(&stdout);
            let fields = text
                .lines()
                .filter_map(|line| line.split_once('='))
                .collect::<BTreeMap<_, _>>();
            json!({"available":true,"reason":Value::Null,"status":fields.get("status"),"fork_point":fields.get("fork_point"),"last_reviewed":fields.get("last_reviewed"),"upstream_repo":fields.get("upstream_repo"),"retired_reason":fields.get("retired_reason").filter(|v|!v.is_empty())})
        }
        TimedOutput::Completed { stderr, .. } => {
            json!({"available":false,"reason":String::from_utf8_lossy(&stderr).lines().next().unwrap_or("upstream status failed"),"status":Value::Null,"fork_point":Value::Null,"last_reviewed":Value::Null,"upstream_repo":Value::Null,"retired_reason":Value::Null})
        }
        TimedOutput::TimedOut => {
            json!({"available":false,"reason":"upstream status timed out","status":Value::Null,"fork_point":Value::Null,"last_reviewed":Value::Null,"upstream_repo":Value::Null,"retired_reason":Value::Null})
        }
        TimedOutput::StartFailed => {
            json!({"available":false,"reason":"upstream status failed","status":Value::Null,"fork_point":Value::Null,"last_reviewed":Value::Null,"upstream_repo":Value::Null,"retired_reason":Value::Null})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verb_and_key_reconciliation_preserves_authority() {
        let summary = json!({
            "active_children":[],
            "decisions_open":[{"id":"child","key":"live-route","verb":"needs-decision"}],
            "holds":[{"id":"legal-release","blocked_by":"external-legal"}],
            "counts":{"active_children":0,"decisions_open":1,"holds":1}
        });
        let activities = vec![
            json!({"key":"legal-release","verb":"paused","summary":"waiting"}),
            json!({"key":"default","verb":"paused","summary":"legacy"}),
            json!({"key":"stale-work","verb":"working","summary":"old"}),
        ];
        let decisions = vec![json!({"key":"stale-route","verb":"needs-decision","summary":"old"})];
        let result = reconcile(&summary, &activities, &decisions);
        assert_eq!(result["activities"][0]["verdict"], "corroborates");
        assert_eq!(result["activities"][1]["verdict"], "inconclusive");
        assert_eq!(result["activities"][2]["verdict"], "contradicts");
        assert_eq!(result["decisions"][0]["verdict"], "contradicts");
        assert_eq!(result["contradiction"], true);
    }

    #[test]
    fn truncated_surfaces_make_absence_inconclusive() {
        let summary = json!({"active_children":[],"decisions_open":[],"holds":[],"counts":{"active_children":1,"decisions_open":0,"holds":0}});
        let result = reconcile(
            &summary,
            &[json!({"key":"hidden","verb":"working","summary":"bounded"})],
            &[],
        );
        assert_eq!(result["activities"][0]["verdict"], "inconclusive");
        assert_eq!(result["contradiction"], false);
    }

    #[test]
    fn tail_bounds_drop_partial_bytes_and_old_lines() {
        let bounded = tail_bound("one\ntwo\nthree\nfour\n", 2, 12);
        assert_eq!(bounded.content, "three\nfour");
        assert!(bounded.input_truncated);
        assert!(bounded.reasons.contains(&"byte_limit"));
    }

    #[test]
    fn bounded_runner_times_out_without_returning_partial_output() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf before; sleep 1; printf after"]);
        assert_eq!(
            run_bounded(command, Duration::from_millis(20), 64),
            TimedOutput::TimedOut
        );
    }

    #[test]
    fn bounded_runner_does_not_wait_for_descendant_held_pipes() {
        let mut command = Command::new("sh");
        command.args(["-c", "sh -c 'sleep 5' & printf parent"]);
        let started = Instant::now();
        let output = run_bounded(command, Duration::from_secs(1), 64);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(matches!(&output, TimedOutput::Completed { status: 0, .. }));
        if let TimedOutput::Completed { stdout, .. } = output {
            assert_eq!(stdout, b"parent");
        }
    }

    #[test]
    fn portfolio_keeps_task_attempt_session_and_evidence_facts_distinct() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        fs::create_dir_all(home.join("data")).unwrap();
        fs::create_dir_all(home.join("state/evidence")).unwrap();
        fs::write(
            home.join("state/evidence/same-name-report-1.json"),
            serde_json::to_vec(&json!({
                "accepted":true,
                "envelope":{"kind":"needs-decision","task_id":"same-name","created_at":"2026-09-16T23:59:00Z"},
                "decision":{"id":"choice","task_id":"same-name","question":"Choose?","brief_revision":3,"workflow_revision":null}
            }))
            .unwrap(),
        )
        .unwrap();
        let paths = Paths {
            root: temp.path().to_path_buf(),
            home: home.clone(),
            state: home.join("state"),
            data: home.join("data"),
            config: home.join("config"),
            projects: home.join("projects"),
            source_root: temp.path().to_path_buf(),
        };
        let task = json!({
            "id":"same-name","project":"legacy","coordination_error":null,
            "coordination":{
                "owner_home":home,"parent_id":"root","root_id":"root","owning_coordinator":null,
                "role":"implementer","persistent":false,
                "runtime":{"provider":"codex","session_id":null,"endpoint":null},
                "attempt":{"id":"attempt-2","generation":2,"brief_revision":3},
                "native_observations":[
                    {"observation_id":"native-1","provider":"codex","child_id":"child-7","parent_attempt":{"id":"attempt-2"},"state":"started","observed_at":"2026-09-17T00:00:00Z"},
                    {"observation_id":"native-2","provider":"codex","child_id":"child-7","parent_attempt":{"id":"attempt-2"},"state":"result","observed_at":"2026-09-17T00:01:00Z"},
                    {"observation_id":"native-3","provider":"codex","child_id":null,"parent_attempt":{"id":"attempt-2"},"state":"started","observed_at":"2026-09-17T00:02:00Z"}
                ],
                "accepted_brief_revision":3,"accepted_brief_digest":"digest","accepted_brief_path":"data/same-name/brief.md",
                "briefs":[{"revision":3,"scope":"Implement it","source_artifacts":["data/same-name/research.md"]}],
                "prior_attempts":[{"id":"attempt-1","generation":1,"brief_revision":2}],"retained_executions":[],
                "schedule":{"state":"waiting-dependency","priority":7,"dependencies":["prerequisite"],"decisions":[{"id":"choice","question":"Choose?","brief_revision":3,"workflow_revision":null,"answer":null}]},
                "project":null,"allocation":null,
                "delivery":{"current_commit":"abc","history":[{"attempt_id":"attempt-2","attempt_generation":2,"brief_revision":3,"commit":"abc","pr_url":"https://example.invalid/pull/1","checks":[],"outcome":"published"}]}
            },
            "allocation_observation":null,"backlog":{"title":"Visible task"},
            "current_state":{"state":"parked","source":"status-log","observed_at":"2026-09-17T00:00:00Z"},
            "paths":{"worktree":{"path":null},"report":{"path":"report.md","present":true},"status_log":{"last_event":{"state":"blocked","note":"dependency","raw":"blocked: dependency"}}},
            "hints":{"open_decisions":[]},"pr":{"url":null,"source":"absent"}
        });
        let projection = portfolio(
            &paths,
            "2026-09-17T00:00:00Z",
            &json!({"records":[{"structured":true,"id":"queued-only","title":"Accepted queue item","repo":"other","state":"queued","priority":"3","blocked_by_ids":[],"report_path":null,"pr_url":null}]}),
            &json!([task]),
            &json!({"complete":true,"total":0}),
            &json!({"workflow_runs":{"records":[{"id":"run-1","workflow":"deliver","workflow_revision":"rev-7","status":"running","current_stage":"implement","updated_at":"2026-09-17T00:00:00Z","stages":[{"id":"implement","task_id":"same-name","status":"waiting-agent"}]}]}}),
        );
        assert_eq!(projection["counts"]["tasks"], 2);
        assert_eq!(projection["counts"]["attempts"], 2);
        assert_eq!(projection["counts"]["sessions"], 1);
        let current = projection["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == "same-name")
            .unwrap();
        assert_eq!(current["state"], "waiting-dependency");
        assert_eq!(current["workflow"]["id"], "run-1");
        assert_eq!(current["workflow"]["revision"], "rev-7");
        assert_eq!(current["dependencies"][0]["blocking"], true);
        assert_eq!(
            current["decisions"][0]["waiting_since"],
            "2026-09-16T23:59:00Z"
        );
        assert_eq!(current["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(current["sessions"][0]["state"], "result");
        assert_eq!(current["native_observations"].as_array().unwrap().len(), 3);
        assert_eq!(current["evidence"]["delivery"]["commit"], "abc");
        assert!(current["key"].as_str().unwrap().contains("#task:same-name"));
        let queued = projection["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == "queued-only")
            .unwrap();
        assert_eq!(queued["freshness"]["status"], "partial");
    }

    #[test]
    fn decision_evidence_uses_only_bounded_accepted_question_records() {
        let temp = tempfile::tempdir().unwrap();
        let evidence = temp.path().join("state/evidence");
        fs::create_dir_all(&evidence).unwrap();
        let record = |accepted: bool, kind: &str, created_at: &str| {
            json!({
                "accepted":accepted,
                "envelope":{"kind":kind,"task_id":"worker","created_at":created_at},
                "decision":{"id":"choice","task_id":"worker","question":"Choose?","brief_revision":2,"workflow_revision":"flow-1"}
            })
        };
        fs::write(
            evidence.join("worker-accepted.json"),
            serde_json::to_vec(&record(true, "needs-decision", "2026-09-17T12:00:00Z")).unwrap(),
        )
        .unwrap();
        fs::write(
            evidence.join("worker-rejected.json"),
            serde_json::to_vec(&record(false, "needs-decision", "2026-09-17T11:00:00Z")).unwrap(),
        )
        .unwrap();
        fs::write(
            evidence.join("worker-invalid-time.json"),
            serde_json::to_vec(&record(true, "needs-decision", "not-a-time")).unwrap(),
        )
        .unwrap();
        fs::write(
            evidence.join("worker-wrong-kind.json"),
            serde_json::to_vec(&record(true, "working", "2026-09-17T10:00:00Z")).unwrap(),
        )
        .unwrap();
        let (rows, truncated) = decision_evidence(&temp.path().join("state"));
        assert!(!truncated);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            decision_waiting_since(
                &rows,
                "worker",
                &json!({"id":"choice","question":"Choose?","brief_revision":2,"workflow_revision":"flow-1"})
            ),
            "2026-09-17T12:00:00Z"
        );
        assert!(
            decision_waiting_since(
                &rows,
                "worker",
                &json!({"id":"other","question":"Choose?","brief_revision":2,"workflow_revision":"flow-1"})
            )
            .is_null()
        );

        for index in 0..513 {
            fs::write(evidence.join(format!("bounded-{index:03}.json")), b"{}").unwrap();
        }
        let (rows, truncated) = decision_evidence(&temp.path().join("state"));
        assert!(truncated);
        assert!(rows.len() <= 1);
    }

    #[test]
    fn nested_domain_home_validation_returns_the_canonical_seeded_home() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let active = temp.path().join("active");
        let nested = temp.path().join("nested");
        for name in ["state", "data", "config", "projects", "bin"] {
            fs::create_dir_all(nested.join(name)).unwrap();
        }
        fs::create_dir_all(active.join("state")).unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::write(nested.join(".mx-daemon-home"), "nested\n").unwrap();
        fs::write(nested.join("AGENTS.md"), "# nested\n").unwrap();
        let paths = Paths {
            root,
            home: active.clone(),
            state: active.join("state"),
            data: active.join("data"),
            config: active.join("config"),
            projects: active.join("projects"),
            source_root: temp.path().to_path_buf(),
        };
        assert_eq!(
            validate_home(&paths, "nested", &nested).unwrap(),
            nested.canonicalize().unwrap()
        );
        assert!(validate_home(&paths, "wrong-id", &nested).is_err());
    }

    #[test]
    fn parent_observation_reconciles_and_discloses_bounds() {
        let summary = json!({"active_children":[],"decisions_open":[],"holds":[],"counts":{"active_children":0,"decisions_open":0,"holds":0}});
        let status =
            "working [key=old]: first\nworking [key=middle]: second\nworking [key=last]: third\n";
        let observed = parent_observation(status, &summary, 2, 4096, 1);
        assert_eq!(observed["activity_scan"]["input_truncated"], true);
        assert_eq!(observed["activity_scan"]["retained_truncated"], true);
        assert_eq!(observed["open_activities"][0]["key"], "last");
        assert_eq!(observed["reconciliation"]["contradiction"], true);
    }

    #[test]
    fn terminal_evidence_is_bounded_and_subordinate() {
        let evidence = terminal_evidence(
            TimedOutput::Completed {
                status: 0,
                stdout: b"Phase 7 started\n> \n".to_vec(),
                stderr: Vec::new(),
            },
            "2026-07-11T18:00:00Z",
            "Phase 7 started",
            true,
        );
        assert_eq!(evidence["captured"], true);
        assert_eq!(evidence["event_note_seen"], true);
        assert_eq!(evidence["contradiction"], true);
        assert!(evidence.get("content").is_none());
    }

    #[test]
    fn domain_projection_reuses_direct_observations_and_keeps_task_session_counts_distinct() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let first = temp.path().join("first");
        let nested = temp.path().join("nested");
        for path in [
            root.join("state"),
            first.join("state"),
            nested.join("state"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        let paths = Paths {
            root: root.clone(),
            home: root.clone(),
            state: root.join("state"),
            data: root.join("data"),
            config: root.join("config"),
            projects: root.join("projects"),
            source_root: root.clone(),
        };
        let coordinator = |id: &str, owner: &Path, runtime: &Path, endpoint: bool| {
            json!({
                "id": id,
                "coordination": {
                    "role": "sub-orchestrator",
                    "owner_home": owner,
                    "owner_state": owner.join("state"),
                    "persistent_home": runtime,
                    "parent_id": "root",
                    "root_id": format!("root-home:{}", root.display()),
                    "attempt": {"id":format!("attempt-{id}"),"generation":1,"brief_revision":1},
                    "domain": {"domain_id":format!("domain-{id}"),"scope":"bounded","projects":[],"idea_id":Value::Null,"scope_revision":1,"assignment_generation":1}
                },
                "endpoint": {"exists":endpoint}
            })
        };
        let first_task = coordinator("first", &root, &first, true);
        let nested_task = coordinator("nested", &first, &nested, false);
        let daemon_current = json!({"records":[
            {"home":first,"valid":true,"reason":Value::Null,"provenance":{"selected":"structured-home"},"domains":[nested_task],"endpoints":[],"counts":{"useful_tasks":2,"worker_sessions":1,"coordinator_sessions":1,"attempts_known":3,"tasks_total":3}},
            {"home":nested,"valid":true,"reason":Value::Null,"provenance":{"selected":"structured-home"},"domains":[],"endpoints":[],"counts":{"useful_tasks":3,"worker_sessions":2,"coordinator_sessions":0,"attempts_known":3,"tasks_total":3}}
        ]});
        let projection = domain_projection(
            &paths,
            &json!([first_task]),
            &daemon_current,
            "2026-09-15T00:00:00Z",
        );
        assert_eq!(projection["total"], 2);
        assert_eq!(projection["records"][0]["counts"]["useful_tasks"], 2);
        assert_eq!(projection["records"][0]["counts"]["worker_sessions"], 1);
        assert_eq!(
            projection["records"][0]["counts"]["coordinator_sessions"],
            2
        );
        assert_eq!(projection["records"][1]["counts"]["useful_tasks"], 3);
        assert_eq!(projection["records"][1]["counts"]["worker_sessions"], 2);
        assert_eq!(
            projection["records"][1]["coordinator"]["validated_home"].as_str(),
            nested.to_str()
        );
    }

    #[test]
    fn domain_projection_reports_unavailable_duplicate_and_bounded_lineage_truthfully() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir_all(root.join("state")).unwrap();
        let paths = Paths {
            root: root.clone(),
            home: root.clone(),
            state: root.join("state"),
            data: root.join("data"),
            config: root.join("config"),
            projects: root.join("projects"),
            source_root: root.clone(),
        };
        let coordinator = |id: &str, owner: Option<&Path>, runtime: Option<&Path>| {
            json!({
                "id":id,
                "coordination":{
                    "role":"sub-orchestrator",
                    "owner_home":owner,
                    "persistent_home":runtime,
                    "domain":{"domain_id":format!("domain-{id}"),"scope":"bounded"}
                },
                "endpoint":{"exists":false}
            })
        };

        let unavailable = coordinator("unavailable", None, None);
        let projection = domain_projection(
            &paths,
            &json!([unavailable.clone(), unavailable]),
            &json!({"records":[]}),
            "2026-09-15T00:00:00Z",
        );
        assert_eq!(
            projection["total"], 1,
            "duplicate canonical domain is suppressed"
        );
        assert_eq!(projection["records"][0]["observation"]["partial"], true);
        assert_eq!(
            projection["records"][0]["observation"]["reason"],
            "runtime home unavailable"
        );
        assert_eq!(projection["records"][0]["channel"]["available"], false);
        assert!(projection["records"][0]["counts"]["useful_tasks"].is_null());

        let mut task_rows = Vec::new();
        let mut summaries = Vec::new();
        for index in 0..22 {
            let runtime = temp.path().join(format!("runtime-{index}"));
            fs::create_dir_all(runtime.join("state")).unwrap();
            task_rows.push(coordinator(
                &format!("coordinator-{index}"),
                Some(&root),
                Some(&runtime),
            ));
            summaries.push(json!({
                "home":runtime,
                "valid":true,
                "provenance":{"selected":"structured-home"},
                "domains":[],
                "counts":{"useful_tasks":0,"worker_sessions":0,"coordinator_sessions":0,"attempts_known":0,"tasks_total":0}
            }));
        }
        let bounded = domain_projection(
            &paths,
            &Value::Array(task_rows),
            &json!({"records":summaries}),
            "2026-09-15T00:00:00Z",
        );
        assert_eq!(bounded["records"].as_array().unwrap().len(), 20);
        assert_eq!(bounded["truncated"], 2);
        assert_eq!(bounded["complete"], false);

        let mut chain = Vec::new();
        let mut homes = Vec::new();
        for index in 0..10 {
            let home = temp.path().join(format!("chain-{index}"));
            fs::create_dir_all(home.join("state")).unwrap();
            homes.push(home);
        }
        for index in (0..10).rev() {
            let nested = (index + 1 < homes.len())
                .then(|| {
                    coordinator(
                        &format!("depth-{}", index + 1),
                        Some(&root),
                        Some(&homes[index + 1]),
                    )
                })
                .into_iter()
                .collect::<Vec<_>>();
            chain.push(json!({
                "home":homes[index],
                "valid":true,
                "provenance":{"selected":"structured-home"},
                "domains":nested,
                "counts":{"useful_tasks":0,"worker_sessions":0,"coordinator_sessions":0,"attempts_known":0,"tasks_total":0}
            }));
        }
        let deep = domain_projection(
            &paths,
            &json!([coordinator("depth-0", Some(&root), Some(&homes[0]))]),
            &json!({"records":chain}),
            "2026-09-15T00:00:00Z",
        );
        assert_eq!(deep["records"].as_array().unwrap().len(), 9);
        assert_eq!(deep["truncated"], 1);
        assert_eq!(deep["complete"], false);
    }
}

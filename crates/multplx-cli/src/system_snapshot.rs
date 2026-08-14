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
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use multplx_core::classification::{open_activities, open_decisions};
use multplx_core::process::{ProcessProbe, SystemProcessProbe};
use regex::Regex;

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
        daemon_home_summary(&paths.home, &generated, &backlog, &tasks)
    } else {
        system_model(paths, &generated, backlog, tasks)
    };
    match serde_json::to_string_pretty(&model) {
        Ok(output) => (0, format!("{output}\n"), String::new()),
        Err(error) => (1, String::new(), format!("mx-system-snapshot: {error}\n")),
    }
}

fn usage() -> String {
    "usage: mx-system-snapshot.sh --json\n       mx-system-snapshot.sh --daemon-home-summary\n\nPrint a read-only structured snapshot of the broker system.\nJSON is the stable machine-readable output contract.\n".into()
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
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return TimedOutput::StartFailed;
    };
    let stdout = child.stdout.take().map(|mut stream| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream
                .by_ref()
                .take((byte_limit + 1) as u64)
                .read_to_end(&mut bytes);
            bytes
        })
    });
    let stderr = child.stderr.take().map(|mut stream| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream
                .by_ref()
                .take((byte_limit + 1) as u64)
                .read_to_end(&mut bytes);
            bytes
        })
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = stdout
                    .and_then(|reader| reader.join().ok())
                    .unwrap_or_default();
                let mut stderr = stderr
                    .and_then(|reader| reader.join().ok())
                    .unwrap_or_default();
                stdout.truncate(byte_limit);
                stderr.truncate(byte_limit);
                return TimedOutput::Completed {
                    status: status.code().unwrap_or(1),
                    stdout,
                    stderr,
                };
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout);
                drop(stderr);
                return TimedOutput::TimedOut;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout);
                drop(stderr);
                return TimedOutput::StartFailed;
            }
        }
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
    let mut rows = fs::read_dir(&paths.state)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|v| v.to_str()) == Some("meta")).then_some(path)
        })
        .filter_map(|path| task(paths, &path, generated, backlog))
        .collect::<Vec<_>>();
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
    let exists = target
        .as_ref()
        .map(|target| endpoint_exists(backend, target));
    let alive = if kind == "daemon" {
        target
            .as_deref()
            .map(|target| agent_alive(backend, target))
            .unwrap_or_else(|| "unknown".into())
    } else {
        "not_checked".into()
    };
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
        json!({"id":id,"kind":kind,"harness":fields.get("harness").cloned().unwrap_or_default(),"mode":fields.get("mode").cloned().unwrap_or_default(),"yolo":fields.get("yolo").cloned().unwrap_or_default(),"project":fields.get("project").cloned().unwrap_or_default(),"backend":backend,"paths":{"meta":observed(Some(path)),"status_log":{"path":status_path,"present":status_path.is_file(),"kind":"event_history","last_event":{"state":multplx_core::classification::status_line_verb(last),"note":multplx_core::classification::status_line_note(last),"raw":last}},"worktree":observed(fields.get("worktree").map(Path::new)),"home":observed(fields.get("home").map(Path::new)),"report":observed(Some(&report))},"daemon_projects":fields.get("projects").map(|v|v.split(',').map(str::trim).filter(|v|!v.is_empty()).collect::<Vec<_>>()).unwrap_or_default(),"current_state":{"state":current_state,"source":current_source,"detail":current["detail"],"raw":current["raw"],"observed_at":generated,"freshness":"fresh"},"endpoint":{"target":target,"exists":exists,"agent_alive":alive,"status":if exists==Some(false){"absent"}else if matches!(alive.as_str(),"alive"|"dead"){alive.as_str()}else{"unknown"},"observed_at":generated,"freshness":"fresh"},"pr":{"url":pr,"source":pr_source},"hints":{"pending_decision":open.iter().any(|row|row["verb"]=="needs-decision"),"blocked_event":open.iter().any(|row|row["verb"]=="blocked"),"open_decisions":open,"scout_report_present":report.is_file(),"last_event_text":last},"actions":if kind=="daemon"{json!({"send":format!("bin/mx-send.sh mx-{id} '<request>'"),"watch":"read status/doc return channel; do not routinely mx-peek a daemon for answers","return_channel_note":"Daemon answers come back through status/doc paths after a marked mx-send request."})}else{json!({"watch":format!("bin/mx-peek.sh mx-{id}"),"steer":format!("bin/mx-send.sh mx-{id} '<instruction>'"),"return_channel_note":Value::Null})},"backlog":owned}),
    )
}
fn actor_state(paths: &Paths, id: &str) -> Value {
    let output = Command::new(paths.source_root.join("bin/mx-actor-state.sh"))
        .arg(id)
        .env("MX_ROOT_OVERRIDE", &paths.root)
        .env("MX_HOME", &paths.home)
        .env("MX_STATE_OVERRIDE", &paths.state)
        .env("MX_DATA_OVERRIDE", &paths.data)
        .env("MX_CONFIG_OVERRIDE", &paths.config)
        .env("MX_PROJECTS_OVERRIDE", &paths.projects)
        .output();
    let raw = output
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .to_owned()
        })
        .unwrap_or_default();
    let mut state = "unknown";
    let mut source = "none";
    let mut detail = "";
    if let Some(rest) = raw.strip_prefix("state: ") {
        let mut parts = rest.split(" · ");
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
fn endpoint_exists(backend: &str, target: &str) -> bool {
    backend == "tmux"
        && Command::new("tmux")
            .args(["display-message", "-p", "-t", target, "#{pane_id}"])
            .output()
            .is_ok_and(|o| o.status.success())
}
fn agent_alive(backend: &str, target: &str) -> String {
    if !endpoint_exists(backend, target) {
        return "unknown".into();
    }
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            target,
            "#{pane_current_command}",
        ])
        .output()
        .ok();
    let command = output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout))
        .unwrap_or_default();
    if ["codex", "claude", "pi"]
        .iter()
        .any(|name| command.contains(name))
    {
        "alive".into()
    } else {
        "dead".into()
    }
}
fn daemon_home_summary(home: &Path, generated: &str, backlog: &Value, tasks: &Value) -> Value {
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
    json!({"schema":"mx-daemon-home-summary.v1","generated":generated,"home":home,"valid":valid,"reason":reason,"invalidity":invalidity,"state":state,"active_children":active,"decisions_open":decisions,"holds":holds,"queued":queued,"landed":landed,"endpoints":endpoints,"counts":{"active_children":active_all.len(),"decisions_open":decisions_all.len(),"holds":holds_all.len(),"queued":queued_all.len(),"landed":landed_all.len(),"endpoints":endpoints_all.len()},"omitted":omitted})
}

fn daemon_summary_invalid(home: &Path, generated: &str, invalidity: &Value, reason: &str) -> Value {
    json!({"schema":"mx-daemon-home-summary.v1","generated":generated,"home":home,"valid":false,"reason":reason,"invalidity":invalidity,"state":"unknown","active_children":[],"decisions_open":[],"holds":[],"queued":[],"landed":[],"endpoints":[],"counts":{"active_children":0,"decisions_open":0,"holds":0,"queued":0,"landed":0,"endpoints":0},"omitted":[]})
}
fn system_model(paths: &Paths, generated: &str, backlog: Value, tasks: Value) -> Value {
    let inventory = inventory(&backlog, &tasks);
    let reports = reports(paths, &backlog, &tasks);
    let watcher = watcher(paths);
    let wake = wake_queue(paths);
    let (headroom, headroom_reason) = headroom(paths);
    let daemon_current = daemon_current(paths, generated, &tasks);
    let daemon_landed = daemon_landed(&daemon_current);
    json!({"schema":"mx-system-snapshot.v1","generated":generated,"mx_home":paths.home,"roots":{"mx_root":paths.root,"state":paths.state,"data":paths.data,"config":paths.config,"projects":paths.projects},"backlog":backlog,"tasks":tasks,"main_inventory":inventory,"scout_reports":reports,"watcher":watcher,"wake_queue":wake,"dispatch_queue":dispatch(paths),"headroom":headroom,"headroom_reason":headroom_reason,"vplan_reviews":vplans(paths),"later_feeds":later(paths),"daemon_current":daemon_current,"daemon_landed":daemon_landed,"daemon_guidance":{"note":"For kind=daemon, catchup selects validated structured state from that registered home; parent events and bounded terminal evidence are fallback-only supplements and never current-state authority."}})
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

fn read_child_summary(paths: &Paths, home: &Path, generated: &str) -> Result<Value, String> {
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
    match run_bounded(
        command,
        env_duration("MX_SNAPSHOT_DAEMON_TIMEOUT", 8),
        limit + 1,
    ) {
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
                    "active_children",
                    "decisions_open",
                    "holds",
                    "queued",
                    "landed",
                    "endpoints",
                    "omitted",
                ]
                .iter()
                .all(|key| summary[*key].is_array())
                && summary["counts"].is_object()
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
    let mut records = Vec::new();
    for route in routes {
        let mut reason = route.error.clone();
        let mut home = route.home.clone();
        if reason.is_none() && home.is_none() {
            reason = Some("no recorded daemon home".into())
        }
        if reason.is_none() {
            match validate_home(paths, &route.id, home.as_ref().unwrap()) {
                Ok(resolved) if seen.insert(resolved.clone()) => home = Some(resolved),
                Ok(_) => reason = Some("invalid home: duplicate resolved home route".into()),
                Err(error) => reason = Some(format!("invalid home: {error}")),
            }
        }
        let empty_summary = json!({"active_children":[],"decisions_open":[],"holds":[],"queued":[],"landed":[],"endpoints":[],"counts":{"active_children":0,"decisions_open":0,"holds":0,"queued":0,"landed":0,"endpoints":0},"omitted":[]});
        let summary = if reason.is_none() {
            match read_child_summary(paths, home.as_ref().unwrap(), generated) {
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
            records.push(json!({"id":route.id,"home":home,"registered":route.registered,"current":{"state":"unknown","reason":reason},"invalidity":Value::Null,"provenance":{"selected":selected,"structured_home":home,"parent_event_role":"fallback-only-not-current"},"freshness":{"status":if raw.is_empty(){"unknown"}else{"historical-event"},"observed_at":generated,"age_seconds":parent_event["age_seconds"]},"active_children":[],"decisions_open":[],"holds":[],"queued":[],"landed":[],"endpoints":[],"counts":empty_summary["counts"],"omitted":[],"parent_event":parent_event,"terminal_evidence":terminal_capture(&route.parent,&note,generated,false),"contradiction":false}));
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
        records.push(json!({"id":route.id,"home":home,"registered":route.registered,"current":{"state":summary["state"],"reason":current_reason},"invalidity":summary["invalidity"],"provenance":{"selected":"structured-home","structured_home":home,"summary_valid":summary_valid,"trust":if summary_valid{"complete"}else{"partial-structured"},"parent_event_role":"historical-only"},"freshness":{"status":"fresh","observed_at":generated,"age_seconds":0},"active_children":summary["active_children"],"decisions_open":summary["decisions_open"],"holds":summary["holds"],"queued":summary["queued"],"landed":summary["landed"],"endpoints":summary["endpoints"],"counts":summary["counts"],"omitted":summary["omitted"],"parent_event":parent_event,"terminal_evidence":terminal,"contradiction":contradiction||terminal["contradiction"]==true}));
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
    json!({"depth":records.len(),"oldest_age_secs":records.iter().min().map(|oldest|(epoch()-*oldest).max(0))})
}
fn headroom_bin(paths: &Paths) -> PathBuf {
    std::env::var_os("MX_SNAPSHOT_HEADROOM_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.source_root.join("bin/mx-headroom.sh"))
}
fn headroom(paths: &Paths) -> (Value, Value) {
    let output = Command::new(headroom_bin(paths))
        .arg("--json")
        .env("MX_ROOT_OVERRIDE", &paths.root)
        .env("MX_HOME", &paths.home)
        .env("MX_STATE_OVERRIDE", &paths.state)
        .env("MX_CONFIG_OVERRIDE", &paths.config)
        .output();
    match output {
        Ok(output) if output.status.success() => serde_json::from_slice(&output.stdout)
            .map(|v| (v, Value::Null))
            .unwrap_or((Value::Null, json!("headroom check failed"))),
        Ok(output) => (
            Value::Null,
            json!(
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("headroom check failed")
            ),
        ),
        Err(_) => (Value::Null, json!("headroom check failed")),
    }
}
fn dispatch(paths: &Paths) -> Value {
    let output = Command::new(headroom_bin(paths))
        .arg("--queue")
        .env("MX_ROOT_OVERRIDE", &paths.root)
        .env("MX_HOME", &paths.home)
        .env("MX_STATE_OVERRIDE", &paths.state)
        .env("MX_CONFIG_OVERRIDE", &paths.config)
        .output();
    let Ok(output) = output else {
        return json!({"depth":0,"records":[],"available":false,"reason":"dispatch queue read failed"});
    };
    if !output.status.success() {
        return json!({"depth":0,"records":[],"available":false,"reason":String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or("dispatch queue read failed")});
    }
    let rows=String::from_utf8_lossy(&output.stdout).lines().filter_map(|line|{let parts=line.split('\t').collect::<Vec<_>>();(parts.len()==8).then(||json!({"enqueued_at":parts[0].parse::<u64>().ok(),"id":parts[1],"project":parts[2],"profile":{"harness":null_dash(parts[3]),"model":null_dash(parts[4]),"effort":null_dash(parts[5]),"backend":null_dash(parts[6])},"kind":parts[7]}))}).collect::<Vec<_>>();
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
            json!({"artifact":fields.get("artifact"),"port":port,"started_at":fields.get("started_at"),"pid_alive":alive,"url":port.map(|port|format!("http://127.0.0.1:{port}/"))})
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
    serde_json::from_slice(&fs::read(path).ok()?).ok()
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
    let records=directories_with_suffix(&paths.state,".workflow").into_iter().map(|dir|{let id=dir.file_name().unwrap().to_string_lossy().trim_end_matches(".workflow").to_owned();json_file(&dir.join("run.json")).map_or_else(||json!({"id":id,"valid":false,"workflow":Value::Null,"status":"invalid","current_stage":Value::Null,"message":Value::Null,"created_at":Value::Null,"updated_at":Value::Null}),|run|json!({"id":id,"valid":true,"workflow":run["workflow"],"status":run["status"].as_str().unwrap_or("unknown"),"current_stage":run["current_stage"],"message":run["message"],"created_at":run["created_at"],"updated_at":run["updated_at"]}))}).collect::<Vec<_>>();
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
    match Command::new(command)
        .arg("--status")
        .env("MX_ROOT_OVERRIDE", &paths.root)
        .output()
    {
        Ok(output) if matches!(output.status.code(), Some(0 | 3)) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let fields = text
                .lines()
                .filter_map(|line| line.split_once('='))
                .collect::<BTreeMap<_, _>>();
            json!({"available":true,"reason":Value::Null,"status":fields.get("status"),"fork_point":fields.get("fork_point"),"last_reviewed":fields.get("last_reviewed"),"upstream_repo":fields.get("upstream_repo"),"retired_reason":fields.get("retired_reason").filter(|v|!v.is_empty())})
        }
        Ok(output) => {
            json!({"available":false,"reason":String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or("upstream status failed"),"status":Value::Null,"fork_point":Value::Null,"last_reviewed":Value::Null,"upstream_repo":Value::Null,"retired_reason":Value::Null})
        }
        Err(_) => {
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
}

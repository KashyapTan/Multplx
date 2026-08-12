//! Deterministic read-only task-journal timeline projection.

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use multplx_core::filesystem::atomic_replace;
use multplx_core::identifiers::TaskId;
use multplx_core::journal::JournalEvent;
use regex::Regex;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::supervision::CommandResult;

pub const HELP: &str = "Render one task's append-only observability journal.\n\nUsage:\n  mx-timeline.sh <task-id> [--since <duration|iso-time>] [--event <glob>]\n                           [--json] [--html]\n\nText output preserves append order and renders one source-attributed row per\nvalid event.\n--json preserves each matching JSONL record.\n--since accepts an ISO-8601 timestamp or an integer plus s, m, h, d, or w.\n--event uses shell-glob matching against the closed event name.\n--html writes data/<task-id>/timeline.html using the installed vplan visual\nmodule's self-check as its availability gate, then prints the artifact path.\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Text,
    Json,
    Html,
}

struct Options {
    task: TaskId,
    since_ms: Option<i128>,
    event: Regex,
    mode: Mode,
}

fn failure(message: impl AsRef<str>) -> CommandResult {
    CommandResult {
        status: 1,
        stdout: String::new(),
        stderr: format!("mx-timeline: {}\n", message.as_ref()),
    }
}

fn shell_glob_regex(pattern: &str) -> Result<Regex, CommandResult> {
    let mut expression = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '*' => expression.push_str(".*"),
            '?' => expression.push('.'),
            '[' => {
                expression.push('[');
                if chars.peek() == Some(&'!') {
                    chars.next();
                    expression.push('^');
                }
                let mut closed = false;
                for next in chars.by_ref() {
                    if next == ']' {
                        expression.push(']');
                        closed = true;
                        break;
                    }
                    if next == '\\' {
                        expression.push_str("\\\\");
                    } else {
                        expression.push(next);
                    }
                }
                if !closed {
                    expression.push(']');
                }
            }
            _ => expression.push_str(&regex::escape(&character.to_string())),
        }
    }
    expression.push('$');
    Regex::new(&expression).map_err(|_| failure("invalid --event glob"))
}

fn now_millis() -> Result<i128, CommandResult> {
    if let Ok(value) = std::env::var("MX_TIMELINE_NOW_MS")
        && !value.is_empty()
    {
        return value
            .parse::<i128>()
            .map_err(|_| failure("invalid --since value"));
    }
    Ok(OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
}

fn parse_since(value: &str) -> Result<i128, CommandResult> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[..bytes.len() - 1].iter().all(u8::is_ascii_digit) {
        let amount = value[..value.len() - 1]
            .parse::<i128>()
            .map_err(|_| failure(format!("invalid --since value: {value}")))?;
        let factor = match bytes[bytes.len() - 1] {
            b's' => 1_000,
            b'm' => 60_000,
            b'h' => 3_600_000,
            b'd' => 86_400_000,
            b'w' => 604_800_000,
            _ => 0,
        };
        if factor > 0 {
            return now_millis().and_then(|now| {
                amount
                    .checked_mul(factor)
                    .and_then(|duration| now.checked_sub(duration))
                    .ok_or_else(|| failure(format!("invalid --since value: {value}")))
            });
        }
    }
    OffsetDateTime::parse(value, &Rfc3339)
        .map(|time| time.unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| failure(format!("invalid --since value: {value}")))
}

fn parse(args: &[String]) -> Result<Options, CommandResult> {
    let Some(raw_task) = args.first() else {
        return Err(CommandResult {
            status: 2,
            stdout: HELP.to_owned(),
            stderr: String::new(),
        });
    };
    if matches!(raw_task.as_str(), "-h" | "--help") {
        return Err(CommandResult {
            status: 0,
            stdout: HELP.to_owned(),
            stderr: String::new(),
        });
    }
    let task = TaskId::parse(raw_task.clone())
        .map_err(|_| failure(format!("invalid task id: {raw_task}")))?;
    let mut since_ms = None;
    let mut event = "*".to_owned();
    let mut mode = Mode::Text;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--since" | "--event" => {
                let name = &args[index];
                let Some(value) = args.get(index + 1) else {
                    return Err(failure(format!("{name} requires a value")));
                };
                if name == "--since" {
                    since_ms = Some(parse_since(value)?);
                } else {
                    event = value.clone();
                }
                index += 2;
            }
            "--json" | "--html" => {
                if mode != Mode::Text {
                    return Err(failure("choose only one output mode"));
                }
                mode = if args[index] == "--json" {
                    Mode::Json
                } else {
                    Mode::Html
                };
                index += 1;
            }
            "-h" | "--help" => {
                return Err(CommandResult {
                    status: 0,
                    stdout: HELP.to_owned(),
                    stderr: String::new(),
                });
            }
            unknown => return Err(failure(format!("unknown argument: {unknown}"))),
        }
    }
    Ok(Options {
        task,
        since_ms,
        event: shell_glob_regex(&event)?,
        mode,
    })
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| {
            fs::metadata(directory.join(name)).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
    })
}

fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
        None => "null".to_owned(),
    }
}

fn detail(event: &str, value: &Value) -> String {
    let get = |key| value.get(key);
    match event {
        "task.spawned" => format!(
            "kind={} backend={} branch={}",
            text(get("kind")),
            text(get("backend")),
            text(get("branch"))
        ),
        "status.reported" => format!(
            "{}{}",
            text(get("raw")),
            if get("validated").and_then(Value::as_bool) == Some(true) {
                " [validated]"
            } else {
                ""
            }
        ),
        "status.classified" => format!(
            "{} (tier: {}; conflicts: {})",
            text(get("verdict")),
            text(get("tier")),
            get("conflicts")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        ),
        "gate.step.started" => {
            format!("step={} round={}", text(get("step")), text(get("round")))
        }
        "gate.step.finished" => format!(
            "step={} round={} findings={} outcome={}",
            text(get("step")),
            text(get("round")),
            text(get("findings")),
            text(get("outcome"))
        ),
        "hold.opened" => format!("{}: {}", text(get("hold_id")), text(get("title"))),
        "hold.resolved" => format!(
            "{} -> {}",
            text(get("hold_id")),
            get("routed_to")
                .and_then(Value::as_array)
                .map(|values| values
                    .iter()
                    .map(|value| text(Some(value)))
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default()
        ),
        "workflow.stage.entered" => {
            format!("run={} stage={}", text(get("run")), text(get("stage")))
        }
        "workflow.stage.gated" => format!(
            "run={} stage={} gate={} outcome={}",
            text(get("run")),
            text(get("stage")),
            text(get("gate")),
            text(get("outcome"))
        ),
        "delivery.queued" | "delivery.pushed" => {
            format!("branch={} sha={}", text(get("branch")), text(get("sha")))
        }
        "delivery.pr_opened" => text(get("pr_url")),
        _ => value.to_string(),
    }
}

struct EventRow {
    raw: String,
    timestamp: String,
    timestamp_ms: i128,
    source: String,
    event: String,
    detail: Value,
}

fn event_row(line: &str, task: &TaskId) -> Option<EventRow> {
    let value: Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;
    let timestamp = object.get("ts")?.as_str()?;
    if timestamp.len() != 20 || !timestamp.ends_with('Z') {
        return None;
    }
    let timestamp_ms = OffsetDateTime::parse(timestamp, &Rfc3339)
        .ok()?
        .unix_timestamp_nanos()
        / 1_000_000;
    if object.get("task")?.as_str()? != task.as_str() {
        return None;
    }
    let source = object.get("source")?.as_str()?.to_owned();
    let event = object.get("event")?.as_str()?.to_owned();
    JournalEvent::parse(&event).ok()?;
    let detail = object.get("detail")?;
    if !detail.is_object() {
        return None;
    }
    Some(EventRow {
        raw: line.to_owned(),
        timestamp: timestamp.to_owned(),
        timestamp_ms,
        source,
        event,
        detail: detail.clone(),
    })
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_html(task: &TaskId, events: &[EventRow]) -> String {
    let rows = events
        .iter()
        .map(|event| {
            format!(
                "<tr>\n<td><time datetime=\"{}\">{}</time></td>\n<td><code>{}</code></td>\n<td><code>{}</code></td>\n<td><pre>{}</pre></td>\n</tr>",
                html_escape(&event.timestamp),
                html_escape(&event.timestamp),
                html_escape(&event.source),
                html_escape(&event.event),
                html_escape(&serde_json::to_string_pretty(&event.detail).unwrap_or_else(|_| "{}".to_owned()))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let plural = if events.len() == 1 { "" } else { "s" };
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"UTF-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<title>{task} timeline · Multplx</title>\n<style>\n:root{{color-scheme:dark;--bg:#0d1117;--surface:#161b22;--border:#30363d;--text:#e6edf3;--muted:#8b949e;--accent:#79c0ff}}\n*{{box-sizing:border-box}}body{{margin:0;padding:40px;background:var(--bg);color:var(--text);font:15px/1.5 system-ui,sans-serif}}\nmain{{max-width:1200px;margin:auto}}h1{{margin:0 0 8px}}.lede{{color:var(--muted);margin:0 0 28px}}\n.table{{overflow:auto;border:1px solid var(--border);border-radius:10px}}table{{width:100%;border-collapse:collapse;background:var(--surface)}}\nth,td{{padding:12px;text-align:left;vertical-align:top;border-bottom:1px solid var(--border)}}th{{color:var(--accent)}}\ncode,pre{{font:12px/1.45 ui-monospace,SFMono-Regular,Menlo,monospace}}pre{{margin:0;white-space:pre-wrap}}\n</style></head><body><main><h1>{task}</h1>\n<p class=\"lede\">Append-only task timeline · {} event{plural} · observability only</p>\n<div class=\"table\"><table><thead><tr><th>Time</th><th>Source</th><th>Event</th><th>Detail</th></tr></thead>\n<tbody>{rows}</tbody></table></div></main></body></html>\n",
        events.len()
    )
}

/// Render one journal without mutating authoritative task state.
#[must_use]
pub fn run(args: &[String], state: &Path, data: &Path, source_root: &Path) -> CommandResult {
    let options = match parse(args) {
        Ok(options) => options,
        Err(result) => return result,
    };
    let journal = state.join(format!("{}.journal", options.task));
    let Ok(metadata) = fs::symlink_metadata(&journal) else {
        return failure(format!("journal not found: {}", journal.display()));
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return failure(format!("journal not found: {}", journal.display()));
    }
    if !command_exists("jq") {
        return failure("jq is required");
    }
    if !command_exists("node") {
        return failure("node is required");
    }
    let Ok(file) = fs::File::open(&journal) else {
        return failure(format!("journal not found: {}", journal.display()));
    };
    let mut malformed = 0usize;
    let mut events = Vec::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            malformed += 1;
            continue;
        };
        let Some(event) = event_row(&line, &options.task) else {
            malformed += 1;
            continue;
        };
        if !options.event.is_match(&event.event)
            || options
                .since_ms
                .is_some_and(|since| event.timestamp_ms < since)
        {
            continue;
        }
        events.push(event);
    }
    let stderr = if malformed == 0 {
        String::new()
    } else {
        format!("mx-timeline: skipped {malformed} malformed journal line(s)\n")
    };
    let output = match options.mode {
        Mode::Json => events
            .iter()
            .map(|event| format!("{}\n", event.raw))
            .collect::<String>(),
        Mode::Text => events
            .iter()
            .map(|event| {
                format!(
                    "{:<8}  {:<20}  {:<24}  {}\n",
                    &event.timestamp[11..19],
                    event.source,
                    event.event,
                    detail(&event.event, &event.detail)
                )
            })
            .collect::<String>(),
        Mode::Html => {
            let vplan = std::env::var_os("MX_VPLAN_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|| source_root.join("bin/mx-vplan.sh"));
            let available = fs::metadata(&vplan)
                .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false);
            if !available {
                return failure("vplan module is unavailable");
            }
            if !Command::new(&vplan)
                .arg("--self-check")
                .output()
                .is_ok_and(|output| output.status.success())
            {
                return failure("vplan module is unavailable or invalid");
            }
            let directory = data.join(options.task.as_str());
            if fs::create_dir_all(&directory).is_err() {
                return failure("could not create timeline artifact directory");
            }
            let artifact = directory.join("timeline.html");
            if atomic_replace(
                &artifact,
                render_html(&options.task, &events).as_bytes(),
                0o600,
            )
            .is_err()
            {
                return failure("could not render timeline artifact");
            }
            format!("{}\n", artifact.display())
        }
    };
    CommandResult {
        status: 0,
        stdout: output,
        stderr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_and_since_parsers_cover_contract_edges() {
        assert!(
            shell_glob_regex("gate.*")
                .expect("glob")
                .is_match("gate.step.finished")
        );
        assert!(
            !shell_glob_regex("gate.*")
                .expect("glob")
                .is_match("status.reported")
        );
        assert!(OffsetDateTime::parse("2026-07-30T12:06:00Z", &Rfc3339).is_ok());
        assert!(
            shell_glob_regex("gate.????")
                .expect("question glob")
                .is_match("gate.step")
        );
        assert!(
            shell_glob_regex("[!a]one")
                .expect("negated class")
                .is_match("zone")
        );
        assert!(shell_glob_regex("[ab]one").expect("class").is_match("aone"));
        assert!(shell_glob_regex("[unterminated").is_ok());
        assert!(parse_since("2026-07-30T12:06:00Z").is_ok());
        for duration in ["1s", "1h", "1d", "1w"] {
            assert!(parse_since(duration).is_ok());
        }
        assert!(parse_since("1x").is_err());
        assert!(parse_since("not-a-time").is_err());
        assert!(parse_since("999999999999999999999999999999999999999999999999999999s").is_err());
        assert!(!command_exists("multplx-command-that-cannot-exist"));
        assert_eq!(text(None), "null");
        assert_eq!(text(Some(&Value::Null)), "null");
        assert_eq!(text(Some(&Value::Bool(true))), "true");
        match parse(&["timeline-fixture".into(), "--help".into()]) {
            Err(result) => assert_eq!(result.status, 0),
            Ok(_) => panic!("help was accepted as ordinary options"),
        }
    }

    #[test]
    fn detail_rendering_is_event_specific() {
        assert_eq!(
            detail(
                "status.reported",
                &serde_json::json!({"raw":"done: yes","validated":true})
            ),
            "done: yes [validated]"
        );
        assert_eq!(
            detail(
                "status.reported",
                &serde_json::json!({"raw":"working: now","validated":false})
            ),
            "working: now"
        );
        assert_eq!(
            detail(
                "delivery.pushed",
                &serde_json::json!({"branch":"mx/a","sha":"abc"})
            ),
            "branch=mx/a sha=abc"
        );
        let cases = [
            (
                "task.spawned",
                serde_json::json!({"kind":"delivery","backend":"tmux","branch":"mx/a"}),
                "kind=delivery backend=tmux branch=mx/a",
            ),
            (
                "status.classified",
                serde_json::json!({"verdict":"working","tier":"native","conflicts":["report"]}),
                "working (tier: native; conflicts: 1)",
            ),
            (
                "gate.step.started",
                serde_json::json!({"step":"tests","round":2}),
                "step=tests round=2",
            ),
            (
                "gate.step.finished",
                serde_json::json!({"step":"tests","round":2,"findings":1,"outcome":"passed"}),
                "step=tests round=2 findings=1 outcome=passed",
            ),
            (
                "hold.opened",
                serde_json::json!({"hold_id":"hold-1","title":"Choose"}),
                "hold-1: Choose",
            ),
            (
                "hold.resolved",
                serde_json::json!({"hold_id":"hold-1","routed_to":["task-a","task-b"]}),
                "hold-1 -> task-a, task-b",
            ),
            (
                "workflow.stage.entered",
                serde_json::json!({"run":"run-1","stage":"build"}),
                "run=run-1 stage=build",
            ),
            (
                "workflow.stage.gated",
                serde_json::json!({"run":"run-1","stage":"build","gate":"review","outcome":"passed"}),
                "run=run-1 stage=build gate=review outcome=passed",
            ),
            (
                "delivery.pr_opened",
                serde_json::json!({"pr_url":"https://example.invalid/pr/1"}),
                "https://example.invalid/pr/1",
            ),
        ];
        for (event, value, expected) in cases {
            assert_eq!(detail(event, &value), expected);
        }
        assert_eq!(
            detail("future.event", &serde_json::json!({"a":1})),
            "{\"a\":1}"
        );
    }

    #[test]
    fn event_parser_rejects_each_malformed_envelope_class() {
        let task = TaskId::parse("timeline-fixture").expect("task");
        assert!(event_row("{broken", &task).is_none());
        assert!(event_row("[]", &task).is_none());
        assert!(event_row("{}", &task).is_none());
        for line in [
            r#"{"ts":"bad","task":"timeline-fixture","source":"test","event":"status.reported","detail":{}}"#,
            r#"{"ts":"2026-07-30T12:00:00Z","task":"other","source":"test","event":"status.reported","detail":{}}"#,
            r#"{"ts":"2026-07-30T12:00:00Z","task":"timeline-fixture","source":"test","event":"future.event","detail":{}}"#,
            r#"{"ts":"2026-07-30T12:00:00Z","task":"timeline-fixture","source":"test","event":"status.reported","detail":"bad"}"#,
        ] {
            assert!(event_row(line, &task).is_none());
        }
    }
}

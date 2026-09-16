//! Newline-delimited JSON-RPC transport for the task-bound `report_status` tool.

use std::io::{BufRead, Write};
use std::path::Path;

use multplx_domain::supervision::{REPORT_STATES, report};
use serde_json::{Value, json};

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.into()}})
}

fn tool_schema() -> Value {
    json!({
        "name":"report_status",
        "description":"Append one validated status event for this task. Use this instead of writing a status file directly.",
        "inputSchema":{
            "type":"object",
            "properties":{
                "state":{"type":"string","enum":REPORT_STATES},
                "message":{"type":"string","maxLength":300},
                "key":{"type":"string","pattern":"^[A-Za-z0-9._-]+$","description":"Required for needs-decision; becomes the stable human question id."},
                "workflow_revision":{"type":"string","minLength":1,"maxLength":256,"description":"Optional workflow revision for a needs-decision question."}
            },
            "required":["state","message"],
            "additionalProperties":false
        }
    })
}

struct ReportArguments<'a> {
    state: &'a str,
    message: &'a str,
    key: Option<&'a str>,
    workflow_revision: Option<&'a str>,
}

fn validate(arguments: &Value) -> Result<ReportArguments<'_>, &'static str> {
    let object = arguments.as_object().ok_or("arguments must be an object")?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "state" | "message" | "key" | "workflow_revision"
        )
    }) {
        return Err("arguments contain an unsupported property");
    }
    let state = object
        .get("state")
        .and_then(Value::as_str)
        .ok_or("state and message are required")?;
    let message = object
        .get("message")
        .and_then(Value::as_str)
        .ok_or("state and message are required")?;
    if !REPORT_STATES.contains(&state) {
        return Err(
            "state must be one of: working, paused, blocked, needs-decision, done, failed, resolved",
        );
    }
    if message.chars().count() > 300 {
        return Err("message must be a string of at most 300 characters");
    }
    if message.contains(['\r', '\n']) {
        return Err("message must be exactly one line");
    }
    let key = object.get("key").map(|value| {
        value
            .as_str()
            .filter(|key| {
                !key.is_empty()
                    && key.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                    })
            })
            .ok_or("key may contain only A-Z, a-z, 0-9, dot, underscore, and dash")
    });
    let key = key.transpose()?;
    if state == "needs-decision" && key.is_none() {
        return Err("needs-decision requires a stable key");
    }
    let workflow_revision = object.get("workflow_revision").map(|value| {
        value
            .as_str()
            .filter(|revision| {
                !revision.is_empty() && revision.len() <= 256 && !revision.contains(['\r', '\n'])
            })
            .ok_or("workflow_revision must be a non-empty string of at most 256 bytes")
    });
    let workflow_revision = workflow_revision.transpose()?;
    if state != "needs-decision" && workflow_revision.is_some() {
        return Err("workflow_revision is valid only for needs-decision");
    }
    Ok(ReportArguments {
        state,
        message,
        key,
        workflow_revision,
    })
}

fn handle(message: &Value, root: &Path) -> Option<Value> {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || message.get("method").and_then(Value::as_str).is_none()
    {
        return Some(error(id, -32600, "invalid JSON-RPC request"));
    }
    match message["method"].as_str().unwrap_or_default() {
        "initialize" => Some(response(
            id,
            json!({
                "protocolVersion":message.pointer("/params/protocolVersion").cloned().unwrap_or_else(|| json!("2025-06-18")),
                "capabilities":{"tools":{}},
                "serverInfo":{"name":"multplx-status","version":"1.0.0"}
            }),
        )),
        "notifications/initialized" | "notifications/cancelled" => None,
        "ping" => Some(response(id, json!({}))),
        "tools/list" => Some(response(id, json!({"tools":[tool_schema()]}))),
        "tools/call" => {
            if message.pointer("/params/name").and_then(Value::as_str) != Some("report_status") {
                return Some(error(id, -32602, "unknown tool"));
            }
            let arguments = message.pointer("/params/arguments").unwrap_or(&Value::Null);
            let arguments = match validate(arguments) {
                Ok(values) => values,
                Err(reason) => return Some(error(id, -32602, reason)),
            };
            let ReportArguments {
                state,
                message: text,
                key,
                workflow_revision,
            } = arguments;
            let task = match std::env::var("MX_TASK_ID") {
                Ok(task) if !task.is_empty() => task,
                _ => {
                    return Some(response(
                        id,
                        json!({"content":[{"type":"text","text":"mx-report-mcp: no task binding found; MX_TASK_ID is unset"}],"isError":true}),
                    ));
                }
            };
            let mut args = vec![
                "--id".to_owned(),
                task.clone(),
                "--state".to_owned(),
                state.to_owned(),
                "--message".to_owned(),
                text.to_owned(),
            ];
            if let Some(key) = key {
                args.extend(["--key".to_owned(), key.to_owned()]);
            }
            if let Some(revision) = workflow_revision {
                args.extend(["--workflow-revision".to_owned(), revision.to_owned()]);
            }
            let result = report(&args, root);
            if result.status == 0 {
                Some(response(
                    id,
                    json!({"content":[{"type":"text","text":format!("{state} status reported for task {task}")}]}),
                ))
            } else {
                Some(response(
                    id,
                    json!({"content":[{"type":"text","text":result.stderr.trim()}],"isError":true}),
                ))
            }
        }
        method => message
            .get("id")
            .map(|_| error(id, -32601, format!("method not found: {method}"))),
    }
}

/// Serve JSON-RPC until stdin closes.
pub fn serve(root: &Path) -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            return 1;
        };
        if line.trim().is_empty() {
            continue;
        }
        let parsed = match serde_json::from_str::<Value>(&line) {
            Ok(parsed) => parsed,
            Err(_) => {
                let _ = writeln!(stdout, "{}", error(Value::Null, -32700, "parse error"));
                let _ = stdout.flush();
                continue;
            }
        };
        if let Some(reply) = handle(&parsed, root)
            && (writeln!(stdout, "{reply}").is_err() || stdout.flush().is_err())
        {
            return 1;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{handle, tool_schema, validate};
    use serde_json::json;

    #[test]
    fn schema_and_validation_share_the_report_vocabulary() {
        assert_eq!(
            tool_schema()["inputSchema"]["properties"]["state"]["enum"],
            json!([
                "working",
                "paused",
                "blocked",
                "needs-decision",
                "done",
                "failed",
                "resolved"
            ])
        );
        assert!(validate(&json!({"state":"done","message":"ok"})).is_ok());
        assert!(validate(&json!({"state":"bogus","message":"ok"})).is_err());
        assert!(validate(&json!({"state":"done","message":"ok","extra":1})).is_err());
        assert!(validate(&json!({"state":"done","message":"x".repeat(301)})).is_err());
        assert!(validate(&json!({"state":"needs-decision","message":"choose"})).is_err());
        assert!(validate(&json!({"state":"needs-decision","message":"choose","key":"api","workflow_revision":"flow-2"})).is_ok());
        assert!(
            validate(&json!({"state":"done","message":"ok","workflow_revision":"flow-2"})).is_err()
        );
    }

    #[test]
    fn json_rpc_methods_and_errors_are_exact() {
        let root = Path::new("/unused");
        let initialize = handle(
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"test"}}),
            root,
        )
        .expect("initialize");
        assert_eq!(initialize["result"]["protocolVersion"], "test");
        assert_eq!(
            handle(&json!({"jsonrpc":"2.0","id":2,"method":"ping"}), root).expect("ping")["result"],
            json!({})
        );
        assert_eq!(
            handle(&json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}), root).expect("list")["result"]
                ["tools"][0]["name"],
            "report_status"
        );
        assert!(
            handle(
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                root
            )
            .is_none()
        );
        assert_eq!(
            handle(&json!({"id":4,"method":"ping"}), root).expect("invalid")["error"]["code"],
            -32600
        );
        assert_eq!(
            handle(&json!({"jsonrpc":"2.0","id":5,"method":"unknown"}), root).expect("unknown")["error"]
                ["code"],
            -32601
        );
        assert!(handle(&json!({"jsonrpc":"2.0","method":"unknown"}), root).is_none());
    }

    #[test]
    fn tool_call_validation_and_unbound_result_are_structured() {
        let root = Path::new("/unused");
        for arguments in [
            json!(null),
            json!({"state":"other","message":"ok"}),
            json!({"state":"done","message":"two\nlines"}),
            json!({"state":"done","message":"ok","key":"bad/key"}),
            json!({"state":"done","message":"ok","extra":true}),
        ] {
            let reply = handle(
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"report_status","arguments":arguments}}),
                root,
            )
            .expect("validation reply");
            assert_eq!(reply["error"]["code"], -32602);
        }
        let unknown = handle(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"other","arguments":{}}}),
            root,
        )
        .expect("unknown tool");
        assert_eq!(unknown["error"]["code"], -32602);
        let unbound = handle(
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"report_status","arguments":{"state":"done","message":"ok"}}}),
            root,
        )
        .expect("unbound");
        assert_eq!(unbound["result"]["isError"], true);
    }
}

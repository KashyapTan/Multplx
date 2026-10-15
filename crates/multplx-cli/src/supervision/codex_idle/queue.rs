//! Exact-item cancellation through the same CODEX_HOME's native control socket.
use super::*;
use std::io::{Read, Write};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig};

pub(super) fn submission_id(output: &str, thread: &str) -> Option<String> {
    let row = output.trim().strip_prefix("Queued message ")?;
    let (id, recipient) = row.split_once(" for thread ")?;
    if recipient.strip_suffix('.')? != thread
        || id.len() != 36
        || !id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return None;
    }
    Some(id.to_owned())
}

struct ProxyIo {
    input: std::process::ChildStdin,
    output: std::process::ChildStdout,
}
impl Read for ProxyIo {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.output.read(bytes)
    }
}
impl Write for ProxyIo {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.input.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.input.flush()
    }
}

fn response<S: Read + Write>(
    socket: &mut WebSocket<S>,
    id: u64,
) -> Result<serde_json::Value, String> {
    // Native notifications are not replies. Bound both their count and each
    // WebSocket message so unrelated event traffic cannot grow this exchange.
    for _ in 0..64 {
        let message = socket.read().map_err(|error| error.to_string())?;
        let Message::Text(text) = message else {
            continue;
        };
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|error| error.to_string())?;
        if value.get("id").and_then(serde_json::Value::as_u64) == Some(id) {
            if let Some(error) = value.get("error") {
                return Err(format!("native queue cancellation rejected: {error}"));
            }
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| "native proxy response has no result".into());
        }
    }
    Err("native queue cancellation exceeded its response frame bound".into())
}

fn exchange(io: impl Read + Write, target: &Target, id: &str) -> Result<(), String> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(64 * 1024))
        .max_frame_size(Some(64 * 1024));
    // app-server proxy forwards socket bytes, including the WebSocket handshake;
    // it does not convert stdin JSONL to the native control protocol.
    let (mut socket, _) =
        tungstenite::client::client_with_config("ws://localhost", io, Some(config))
            .map_err(|error| error.to_string())?;
    socket.send(Message::text(serde_json::json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"multplx-wake-cancellation","version":"1"},"capabilities":{"experimentalApi":true}}}).to_string())).map_err(|error| error.to_string())?;
    let initialized = response(&mut socket, 1)?;
    let actual = initialized
        .get("codexHome")
        .and_then(serde_json::Value::as_str)
        .ok_or("native proxy did not identify CODEX_HOME")?;
    if fs::canonicalize(actual).map_err(|error| error.to_string())?
        != fs::canonicalize(&target.codex_home).map_err(|error| error.to_string())?
    {
        return Err("native proxy CODEX_HOME differs from the exact-session binding".into());
    }
    socket
        .send(Message::text(
            serde_json::json!({"method":"initialized"}).to_string(),
        ))
        .map_err(|error| error.to_string())?;
    socket.send(Message::text(serde_json::json!({"id":2,"method":"thread/queue/delete","params":{"threadId":target.thread,"queuedSubmissionId":id}}).to_string())).map_err(|error| error.to_string())?;
    // false means the exact item has already left the queue. It grants no
    // authority to interrupt a running turn or acknowledge a durable wake.
    response(&mut socket, 2)?
        .get("deleted")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "native queue cancellation has no boolean result".into())
        .map(|_| ())
}

pub(super) fn cancel(target: &Target, id: &str) -> Result<(), String> {
    let child = Command::new(&target.executable)
        .env("CODEX_HOME", &target.codex_home)
        .args(["app-server", "proxy"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut child = Watcher(child);
    let io = ProxyIo {
        input: child.0.stdin.take().ok_or("native proxy has no stdin")?,
        output: child.0.stdout.take().ok_or("native proxy has no stdout")?,
    };
    let target = target.clone();
    let id = id.to_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = tx.send(exchange(io, &target, &id));
    });
    // This new local control exchange has no model work: cap its entire
    // connect/handshake/RPC lifetime at three seconds, then retain its receipt.
    let result = rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|error| format!("native queue cancellation response unavailable: {error}"))
        .and_then(|result| result);
    terminate_group(&mut child.0);
    let _ = worker.join();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_response_binds_submission_to_exact_thread() {
        let thread = "123e4567-e89b-12d3-a456-426614174000";
        let id = "123e4567-e89b-12d3-a456-426614174001";
        assert_eq!(
            submission_id(
                &format!("Queued message {id} for thread {thread}.\n"),
                thread
            ),
            Some(id.into())
        );
        for output in [
            "queued".into(),
            format!("Queued message {id} for thread another."),
            format!("Queued message unsafe for thread {thread}."),
        ] {
            assert_eq!(submission_id(&output, thread), None);
        }
    }

    #[test]
    fn proxy_failure_and_deadline_return_without_touching_other_processes() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let mut target = super::super::tests::target(temp.path());
        target.executable = temp.path().join("missing-codex");
        assert!(cancel(&target, "123e4567-e89b-12d3-a456-426614174001").is_err());
        let proxy = temp.path().join("proxy");
        fs::write(&proxy, "#!/bin/sh\nexec sleep 60\n").unwrap();
        fs::set_permissions(&proxy, fs::Permissions::from_mode(0o755)).unwrap();
        target.executable = proxy;
        let before = std::time::Instant::now();
        assert!(
            cancel(&target, "123e4567-e89b-12d3-a456-426614174001")
                .unwrap_err()
                .contains("response unavailable")
        );
        assert!(before.elapsed() < Duration::from_secs(5));
        assert!(SystemProcessProbe::default().is_alive(std::process::id()));
    }

    #[test]
    fn native_handshake_validates_home_and_deletes_only_exact_submission() {
        use std::os::unix::net::UnixStream;
        for mode in [
            "deleted",
            "already-running",
            "wrong-home",
            "rejected",
            "malformed",
            "bad-delete",
            "closed",
            "no-home",
            "no-result",
            "notification-flood",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let target = super::super::tests::target(temp.path());
            let home = target.codex_home.clone();
            let (client, server) = UnixStream::pair().unwrap();
            let worker = std::thread::spawn(move || {
                let mut socket = tungstenite::accept(server).unwrap();
                let init: serde_json::Value =
                    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(init["method"], "initialize");
                assert_eq!(init["params"]["capabilities"]["experimentalApi"], true);
                if mode == "closed" {
                    return;
                }
                if mode == "malformed" {
                    socket.send(Message::text("invalid json")).unwrap();
                    return;
                }
                if mode == "no-result" {
                    socket.send(Message::text("{\"id\":1}")).unwrap();
                    return;
                }
                if mode == "notification-flood" {
                    for _ in 0..65 {
                        if socket
                            .send(Message::text("{\"method\":\"unrelated\"}"))
                            .is_err()
                        {
                            break;
                        }
                    }
                    return;
                }
                let actual = if mode == "wrong-home" {
                    home.join("foreign")
                } else {
                    home.clone()
                };
                if mode == "no-home" {
                    socket
                        .send(Message::text("{\"id\":1,\"result\":{}}"))
                        .unwrap();
                    return;
                }
                socket
                    .send(Message::text(
                        serde_json::json!({"id":1,"result":{"codexHome":actual}}).to_string(),
                    ))
                    .unwrap();
                if mode == "wrong-home" {
                    return;
                }
                let initialized: serde_json::Value =
                    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(initialized["method"], "initialized");
                let request: serde_json::Value =
                    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(
                    request,
                    serde_json::json!({"id":2,"method":"thread/queue/delete","params":{"threadId":"123e4567-e89b-12d3-a456-426614174000","queuedSubmissionId":"123e4567-e89b-12d3-a456-426614174001"}})
                );
                let reply = match mode {
                    "rejected" => {
                        serde_json::json!({"id":2,"error":{"code":-32601,"message":"unsupported"}})
                    }
                    "bad-delete" => serde_json::json!({"id":2,"result":{}}),
                    _ => serde_json::json!({"id":2,"result":{"deleted":mode=="deleted"}}),
                };
                socket.send(Message::text(reply.to_string())).unwrap();
            });
            let result = exchange(client, &target, "123e4567-e89b-12d3-a456-426614174001");
            assert_eq!(
                result.is_ok(),
                matches!(mode, "deleted" | "already-running"),
                "{mode}: {result:?}"
            );
            worker.join().unwrap();
        }
    }
}

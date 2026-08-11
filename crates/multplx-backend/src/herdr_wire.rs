//! Bounded newline-delimited JSON transport for Herdr control sockets.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const ACK_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_EVENT_FRAME: usize = 1024 * 1024;
const MAX_MOVE_RESPONSE: usize = 4 * 1024 * 1024;
const EVENT_REQUEST_ID: &str = "mx-eventwait";
const MOVE_REQUEST_ID: &str = "mx-workspace-move";

/// One projected native Herdr event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentStatusEvent {
    /// Exact pane identity.
    pub pane_id: String,
    /// Workspace identity when supplied by Herdr.
    pub workspace_id: String,
    /// Native agent status.
    pub agent_status: String,
    /// Native agent identity when supplied by Herdr.
    pub agent: String,
}

impl AgentStatusEvent {
    /// Render the legacy four-field projection.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            clean(&self.pane_id),
            clean(&self.workspace_id),
            clean(&self.agent_status),
            clean(&self.agent)
        )
    }
}

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            other => other,
        })
        .collect()
}

/// Herdr transport failure classes retained by the command adapters.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// Invalid invocation or unsafe input.
    #[error("invalid Herdr wire request: {0}")]
    Invalid(String),
    /// Socket connect failed.
    #[error("could not connect to Herdr socket: {0}")]
    Connect(#[source] io::Error),
    /// Request publication failed.
    #[error("could not send Herdr request: {0}")]
    Send(#[source] io::Error),
    /// Response ended or failed before a complete frame.
    #[error("could not read a complete Herdr response: {0}")]
    Receive(String),
    /// Response did not match the exact allowed protocol shape.
    #[error("malformed or mismatched Herdr response: {0}")]
    Protocol(String),
}

fn connect(path: &Path) -> Result<UnixStream, WireError> {
    if !path.is_absolute() {
        return Err(WireError::Invalid("socket path is not absolute".to_owned()));
    }
    let stream = UnixStream::connect(path).map_err(WireError::Connect)?;
    stream
        .set_write_timeout(Some(CONNECT_TIMEOUT))
        .map_err(WireError::Connect)?;
    Ok(stream)
}

fn write_request(stream: &mut UnixStream, request: &Value) -> Result<(), WireError> {
    serde_json::to_writer(&mut *stream, request)
        .map_err(|error| WireError::Invalid(error.to_string()))?;
    stream.write_all(b"\n").map_err(WireError::Send)?;
    stream.flush().map_err(WireError::Send)
}

fn read_frame(
    reader: &mut BufReader<UnixStream>,
    deadline: Instant,
    maximum: usize,
    cancelled: Option<&AtomicBool>,
) -> Result<Option<Vec<u8>>, WireError> {
    reader
        .get_ref()
        .set_nonblocking(true)
        .map_err(|error| WireError::Receive(format!("set nonblocking: {error}")))?;
    let mut frame = Vec::new();
    loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Ok(None);
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        let available = match reader.fill_buf() {
            Ok(bytes) => bytes,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(error) => return Err(WireError::Receive(format!("read frame: {error}"))),
        };
        if available.is_empty() {
            return Err(WireError::Receive(if frame.is_empty() {
                "socket closed early".to_owned()
            } else {
                "socket closed before newline".to_owned()
            }));
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if frame.len().saturating_add(take) > maximum + 1 {
            return Err(WireError::Receive(format!("frame exceeds {maximum} bytes")));
        }
        let complete = available.get(take.saturating_sub(1)) == Some(&b'\n');
        frame.extend_from_slice(&available[..take]);
        reader.consume(take);
        if complete {
            frame.pop();
            return Ok(Some(frame));
        }
    }
}

/// Send the single whitelisted `workspace.move` request and return its exact JSON response.
pub fn workspace_move(
    socket_path: &Path,
    workspace_id: &str,
    insert_index: u64,
) -> Result<Value, WireError> {
    if workspace_id.is_empty()
        || workspace_id
            .chars()
            .any(|character| matches!(character, '\t' | '\r' | '\n' | '\0'))
    {
        return Err(WireError::Invalid("unsafe workspace id".to_owned()));
    }
    let mut stream = connect(socket_path)?;
    write_request(
        &mut stream,
        &json!({
            "id": MOVE_REQUEST_ID,
            "method": "workspace.move",
            "params": {"workspace_id": workspace_id, "insert_index": insert_index}
        }),
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut reader = BufReader::new(stream);
    let frame = read_frame(&mut reader, deadline, MAX_MOVE_RESPONSE, None)?
        .ok_or_else(|| WireError::Receive("response timed out".to_owned()))?;
    let response: Value =
        serde_json::from_slice(&frame).map_err(|error| WireError::Protocol(error.to_string()))?;
    let valid = response.get("id").and_then(Value::as_str) == Some(MOVE_REQUEST_ID)
        && response.get("error").is_none_or(Value::is_null)
        && response.pointer("/result/type").and_then(Value::as_str) == Some("workspace_list")
        && response
            .pointer("/result/workspaces")
            .is_some_and(Value::is_array);
    if !valid {
        return Err(WireError::Protocol(
            "expected matching workspace_list response".to_owned(),
        ));
    }
    Ok(response)
}

/// Subscribe to native status events and stream normalized projections until the deadline.
///
/// The callback is invoked for the acknowledgement first and then once per accepted event.
/// Returning an I/O error cancels the owned socket immediately.
pub fn event_wait(
    socket_path: &Path,
    timeout: Duration,
    pane_ids: &[String],
    emit: impl FnMut(&str) -> io::Result<()>,
) -> Result<(), WireError> {
    let cancelled = AtomicBool::new(false);
    event_wait_cancelled(socket_path, timeout, pane_ids, &cancelled, emit)
}

/// Subscribe and stream events until timeout or an explicit cancellation flag is raised.
pub fn event_wait_cancelled(
    socket_path: &Path,
    timeout: Duration,
    pane_ids: &[String],
    cancelled: &AtomicBool,
    mut emit: impl FnMut(&str) -> io::Result<()>,
) -> Result<(), WireError> {
    if timeout.is_zero() || pane_ids.is_empty() {
        return Err(WireError::Invalid(
            "timeout must be positive and at least one pane is required".to_owned(),
        ));
    }
    if pane_ids.iter().any(|pane| {
        pane.is_empty()
            || pane
                .chars()
                .any(|character| matches!(character, '\t' | '\r' | '\n' | '\0'))
    }) {
        return Err(WireError::Invalid("unsafe pane id".to_owned()));
    }
    let subscriptions = pane_ids
        .iter()
        .map(|pane| json!({"type": "pane.agent_status_changed", "pane_id": pane}))
        .collect::<Vec<_>>();
    let mut stream = connect(socket_path)?;
    write_request(
        &mut stream,
        &json!({
            "id": EVENT_REQUEST_ID,
            "method": "events.subscribe",
            "params": {"subscriptions": subscriptions}
        }),
    )?;
    let started = Instant::now();
    let deadline = started + timeout;
    let ack_deadline = deadline.min(started + ACK_TIMEOUT);
    let mut reader = BufReader::new(stream);
    let frame = read_frame(&mut reader, ack_deadline, MAX_EVENT_FRAME, Some(cancelled))?
        .ok_or_else(|| WireError::Receive("subscription acknowledgement timed out".to_owned()))?;
    let ack: Value =
        serde_json::from_slice(&frame).map_err(|error| WireError::Protocol(error.to_string()))?;
    if ack.get("id").and_then(Value::as_str) != Some(EVENT_REQUEST_ID)
        || ack.pointer("/result/type").and_then(Value::as_str) != Some("subscription_started")
    {
        return Err(WireError::Protocol(
            "expected matching subscription_started acknowledgement".to_owned(),
        ));
    }
    emit("@subscribed").map_err(|error| WireError::Receive(error.to_string()))?;
    loop {
        let Some(frame) = read_frame(&mut reader, deadline, MAX_EVENT_FRAME, Some(cancelled))?
        else {
            return Ok(());
        };
        let Ok(message) = serde_json::from_slice::<Value>(&frame) else {
            continue;
        };
        if message.get("event").and_then(Value::as_str) != Some("pane.agent_status_changed") {
            continue;
        }
        let data = message.get("data").and_then(Value::as_object);
        let field = |name: &str| {
            data.and_then(|object| object.get(name))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let event = AgentStatusEvent {
            pane_id: field("pane_id"),
            workspace_id: field("workspace_id"),
            agent_status: field("agent_status"),
            agent: field("agent"),
        };
        emit(&event.render()).map_err(|error| WireError::Receive(error.to_string()))?;
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::thread;
    use std::time::Duration;

    use super::{AgentStatusEvent, event_wait, event_wait_cancelled, workspace_move};

    #[test]
    fn projections_scrub_record_delimiters() {
        let event = AgentStatusEvent {
            pane_id: "p\tid".to_owned(),
            workspace_id: "w\nid".to_owned(),
            agent_status: "blocked".to_owned(),
            agent: "cl\raude".to_owned(),
        };
        assert_eq!(event.render(), "p id\tw id\tblocked\tcl aude");
    }

    #[test]
    fn move_rejects_wrong_id_and_accepts_fragmented_response() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("herdr.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = String::new();
            BufReader::new(stream.try_clone().expect("clone"))
                .read_line(&mut request)
                .expect("request");
            assert!(request.contains("\"method\":\"workspace.move\""));
            stream
                .write_all(b"{\"id\":\"mx-workspace-move\",\"result\":")
                .expect("part one");
            stream
                .write_all(b"{\"type\":\"workspace_list\",\"workspaces\":[]}}\n")
                .expect("part two");
        });
        assert!(workspace_move(&socket, "w1", 0).is_ok());
        server.join().expect("server");
    }

    #[test]
    fn move_rejects_wrong_response_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("wrong-move.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = String::new();
            BufReader::new(stream.try_clone().expect("clone"))
                .read_line(&mut request)
                .expect("request");
            stream
                .write_all(b"{\"id\":\"foreign\",\"result\":{\"type\":\"workspace_list\",\"workspaces\":[]}}\n")
                .expect("response");
        });
        assert!(matches!(
            workspace_move(&socket, "w1", 0),
            Err(super::WireError::Protocol(_))
        ));
        server.join().expect("server");
    }

    #[test]
    fn event_transport_requires_matching_ack_and_streams_only_native_events() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("events.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = String::new();
            BufReader::new(stream.try_clone().expect("clone"))
                .read_line(&mut request)
                .expect("request");
            stream
                .write_all(
                    b"{\"id\":\"mx-eventwait\",\"result\":{\"type\":\"subscription_started\"}}\n",
                )
                .expect("ack");
            stream
                .write_all(b"{\"event\":\"ignored\",\"data\":{}}\n")
                .expect("ignored");
            stream
                .write_all(b"{\"event\":\"pane.agent_status_changed\",\"data\":{\"pane_id\":\"w:p\",\"workspace_id\":\"w\",\"agent_status\":\"blocked\",\"agent\":\"claude\"}}\n")
                .expect("event");
        });
        let mut output = Vec::new();
        event_wait(
            &socket,
            Duration::from_millis(200),
            &["w:p".to_owned()],
            |line| {
                output.push(line.to_owned());
                Ok(())
            },
        )
        .expect_err("early close must be an error");
        assert_eq!(output, ["@subscribed", "w:p\tw\tblocked\tclaude"]);
        server.join().expect("server");
    }

    #[test]
    fn event_transport_cancels_without_waiting_for_the_deadline() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("cancel.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = String::new();
            BufReader::new(stream.try_clone().expect("clone"))
                .read_line(&mut request)
                .expect("request");
            stream
                .write_all(
                    b"{\"id\":\"mx-eventwait\",\"result\":{\"type\":\"subscription_started\"}}\n",
                )
                .expect("ack");
            thread::sleep(Duration::from_millis(200));
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancelled);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            trigger.store(true, Ordering::Release);
        });
        let started = std::time::Instant::now();
        event_wait_cancelled(
            &socket,
            Duration::from_secs(2),
            &["w:p".to_owned()],
            &cancelled,
            |_| Ok(()),
        )
        .expect("cancelled read");
        assert!(started.elapsed() < Duration::from_millis(150));
        server.join().expect("server");
    }

    #[test]
    fn event_transport_rejects_invalid_ack_and_oversized_frames() {
        for (name, response, protocol_error) in [
            ("invalid-ack", b"not-json\n".to_vec(), true),
            (
                "oversized",
                [
                    b"{\"id\":\"mx-eventwait\",\"result\":{\"type\":\"subscription_started\"}}\n"
                        .as_slice(),
                    vec![b'x'; super::MAX_EVENT_FRAME + 2].as_slice(),
                ]
                .concat(),
                false,
            ),
        ] {
            let temp = tempfile::tempdir().expect("tempdir");
            let socket = temp.path().join(format!("{name}.sock"));
            let listener = UnixListener::bind(&socket).expect("bind");
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut request = String::new();
                BufReader::new(stream.try_clone().expect("clone"))
                    .read_line(&mut request)
                    .expect("request");
                let _ = stream.write_all(&response);
            });
            let error = event_wait(
                &socket,
                Duration::from_millis(200),
                &["w:p".to_owned()],
                |_| Ok(()),
            )
            .expect_err("malformed transport must fail");
            if protocol_error {
                assert!(matches!(error, super::WireError::Protocol(_)));
            } else {
                assert!(matches!(error, super::WireError::Receive(_)));
            }
            server.join().expect("server");
        }
    }

    #[test]
    fn event_transport_times_out_cleanly_after_ack() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("timeout.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = String::new();
            BufReader::new(stream.try_clone().expect("clone"))
                .read_line(&mut request)
                .expect("request");
            stream
                .write_all(
                    b"{\"id\":\"mx-eventwait\",\"result\":{\"type\":\"subscription_started\"}}\n",
                )
                .expect("ack");
            thread::sleep(Duration::from_millis(80));
        });
        let mut output = Vec::new();
        event_wait(
            &socket,
            Duration::from_millis(20),
            &["w:p".to_owned()],
            |line| {
                output.push(line.to_owned());
                Ok(())
            },
        )
        .expect("deadline is a clean end");
        assert_eq!(output, ["@subscribed"]);
        server.join().expect("server");
    }
}

//! Minimal bounded HTTP/1.1 framing for disposable loopback services.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

pub const MAX_HEADER_BYTES: usize = 32 * 1024;
pub const MAX_CONNECTIONS: usize = 32;

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        let mut body = serde_json::to_vec(value)
            .unwrap_or_else(|_| b"{\"error\":\"serialization failed\"}".to_vec());
        body.push(b'\n');
        Self::new(status, body).header("Content-Type", "application/json; charset=utf-8")
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

pub fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        reason(response.status),
        response.body.len()
    )?;
    for (name, value) in &response.headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

pub fn read_request(stream: &mut TcpStream, max_body: usize) -> Result<Request, Response> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    let end = loop {
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(Response::new(431, b"request headers too large\n".to_vec()));
        }
        let read = stream
            .read(&mut chunk)
            .map_err(|_| Response::new(400, b"bad request\n".to_vec()))?;
        if read == 0 {
            return Err(Response::new(400, b"bad request\n".to_vec()));
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(position) = header_end(&bytes) {
            break position;
        }
    };
    if end > MAX_HEADER_BYTES {
        return Err(Response::new(431, b"request headers too large\n".to_vec()));
    }
    let head = std::str::from_utf8(&bytes[..end])
        .map_err(|_| Response::new(400, b"bad request\n".to_vec()))?;
    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| Response::new(400, b"bad request\n".to_vec()))?;
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let version = parts.next().unwrap_or_default();
    if method.is_empty()
        || target.is_empty()
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || parts.next().is_some()
        || !target.starts_with('/')
    {
        return Err(Response::new(400, b"bad request\n".to_vec()));
    }
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| Response::new(400, b"bad request\n".to_vec()))?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value.bytes().any(|byte| byte == 0)
        {
            return Err(Response::new(400, b"bad request\n".to_vec()));
        }
        let name = name.to_ascii_lowercase();
        if headers.insert(name, value.trim().to_owned()).is_some() {
            return Err(Response::new(400, b"bad request\n".to_vec()));
        }
    }
    if headers
        .get("transfer-encoding")
        .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
    {
        return Err(Response::new(
            400,
            b"unsupported transfer encoding\n".to_vec(),
        ));
    }
    let content_length = match headers.get("content-length") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| Response::new(400, b"bad content length\n".to_vec()))?,
        None => 0,
    };
    if content_length > max_body {
        return Err(Response::new(
            400,
            format!("confirm payload exceeds {max_body} bytes\n").into_bytes(),
        ));
    }
    let body_start = end + 4;
    let available = bytes.len().saturating_sub(body_start);
    if available > content_length {
        bytes.truncate(body_start + content_length);
    }
    while bytes.len().saturating_sub(body_start) < content_length {
        let remaining = content_length - bytes.len().saturating_sub(body_start);
        let chunk_len = chunk.len();
        let read = stream
            .read(&mut chunk[..remaining.min(chunk_len)])
            .map_err(|_| Response::new(400, b"bad request\n".to_vec()))?;
        if read == 0 {
            return Err(Response::new(400, b"incomplete request body\n".to_vec()));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(Request {
        method,
        target,
        headers,
        body: bytes[body_start..body_start + content_length].to_vec(),
    })
}

pub fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        "html" => "text/html; charset=utf-8",
        "jpeg" | "jpg" => "image/jpeg",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "md" | "txt" => "text/plain; charset=utf-8",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

pub fn percent_decode(value: &str) -> Result<String, ()> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(());
        }
        let high = hex(bytes[index + 1]).ok_or(())?;
        let low = hex(bytes[index + 2]).ok_or(())?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded).map_err(|_| ())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            encoded.push(char::from(*byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_HEADER_BYTES, Response, content_type, encode_component, percent_decode, read_request,
        write_response,
    };
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::Path;
    use std::thread;

    fn parse(bytes: Vec<u8>, max_body: usize) -> Result<super::Request, Response> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
        let address = listener.local_addr().expect("address");
        let writer = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).expect("connect");
            stream.write_all(&bytes).expect("write");
        });
        let (mut stream, _) = listener.accept().expect("accept");
        let result = read_request(&mut stream, max_body);
        writer.join().expect("writer");
        result
    }

    #[test]
    fn url_codec_and_mime_contracts_are_bounded_and_deterministic() {
        assert_eq!(encode_component("a b/é"), "a%20b%2F%C3%A9");
        assert_eq!(percent_decode("a%20b%2F%C3%A9"), Ok("a b/é".to_owned()));
        assert!(percent_decode("%zz").is_err());
        assert!(percent_decode("%ff").is_err());
        assert_eq!(
            content_type(Path::new("plan.HTML")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("file.bin")),
            "application/octet-stream"
        );
        for (file, expected) in [
            ("a.css", "text/css; charset=utf-8"),
            ("a.gif", "image/gif"),
            ("a.jpg", "image/jpeg"),
            ("a.mjs", "text/javascript; charset=utf-8"),
            ("a.json", "application/json; charset=utf-8"),
            ("a.md", "text/plain; charset=utf-8"),
            ("a.pdf", "application/pdf"),
            ("a.png", "image/png"),
            ("a.svg", "image/svg+xml"),
            ("a.webp", "image/webp"),
        ] {
            assert_eq!(content_type(Path::new(file)), expected);
        }
        assert!(percent_decode("%").is_err());
    }

    #[test]
    fn request_framing_accepts_complete_bodies_and_rejects_each_ambiguous_shape() {
        let request = parse(
            b"POST /confirm?x=1 HTTP/1.1\r\nHost: local\r\nContent-Length: 4\r\n\r\ntestextra"
                .to_vec(),
            8,
        )
        .expect("request");
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/confirm?x=1");
        assert_eq!(request.body, b"test");
        for (bytes, expected) in [
            (b"GET / HTTP/2\r\n\r\n".to_vec(), 400),
            (b"GET relative HTTP/1.1\r\n\r\n".to_vec(), 400),
            (b"GET / HTTP/1.1 extra\r\n\r\n".to_vec(), 400),
            (b"GET / HTTP/1.1\r\nbad\r\n\r\n".to_vec(), 400),
            (b"GET / HTTP/1.1\r\nBad Header: x\r\n\r\n".to_vec(), 400),
            (
                b"GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n".to_vec(),
                400,
            ),
            (
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
                400,
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: nope\r\n\r\n".to_vec(),
                400,
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: 9\r\n\r\n123456789".to_vec(),
                400,
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\nx".to_vec(),
                400,
            ),
            (vec![b'x'; MAX_HEADER_BYTES + 4097], 431),
        ] {
            assert_eq!(parse(bytes, 8).expect_err("rejected").status, expected);
        }
    }

    #[test]
    fn response_writer_covers_status_vocabulary_and_custom_headers() {
        for status in [200, 304, 400, 403, 404, 405, 409, 413, 415, 431, 503, 599] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
            let address = listener.local_addr().expect("address");
            let reader = thread::spawn(move || {
                let mut stream = TcpStream::connect(address).expect("connect");
                let mut bytes = Vec::new();
                stream.read_to_end(&mut bytes).expect("read");
                bytes
            });
            let (mut stream, _) = listener.accept().expect("accept");
            write_response(
                &mut stream,
                &Response::new(status, b"body".to_vec()).header("X-Test", "yes"),
            )
            .expect("response");
            drop(stream);
            let bytes = reader.join().expect("reader");
            assert!(bytes.starts_with(format!("HTTP/1.1 {status} ").as_bytes()));
            assert!(bytes.windows(13).any(|window| window == b"X-Test: yes\r\n"));
        }
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn mx() -> &'static str {
    env!("CARGO_BIN_EXE_mx")
}

fn command(root: &Path, home: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(mx());
    command
        .args(args)
        .env("MX_RUST_SOURCE_ROOT", root)
        .env("MX_ROOT_OVERRIDE", root)
        .env("MX_HOME", home)
        .env("MX_STATE_OVERRIDE", home.join("state"));
    command
}

fn run(root: &Path, home: &Path, args: &[&str]) -> Output {
    command(root, home, args).output().expect("run mx")
}

fn assert_success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn record(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .expect("record")
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn http_bytes(port: u16, request: &[u8]) -> (u16, BTreeMap<String, String>, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    stream.write_all(request).expect("request");
    stream.flush().expect("flush");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("response");
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("headers");
    let head = String::from_utf8_lossy(&response[..split]);
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .expect("status");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    (status, headers, response[split + 4..].to_vec())
}

fn get(port: u16, path: &str) -> (u16, BTreeMap<String, String>, Vec<u8>) {
    http_bytes(
        port,
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").as_bytes(),
    )
}

fn post(port: u16, path: &str, token: &str, content_type: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: {content_type}\r\nX-Vplan-Token: {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    let (status, _, body) = http_bytes(port, &request);
    (status, body)
}

fn post_maybe_status(port: u16, token: &str, body: &[u8]) -> Option<u16> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    let mut request = format!(
        "POST /confirm HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nX-Vplan-Token: {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    stream.write_all(&request).expect("request");
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    let head_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    std::str::from_utf8(&response[..head_end])
        .ok()?
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn wait_until(description: &str, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {description}");
}

fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn free_port() -> u16 {
    static CLAIMED_PORTS: OnceLock<Mutex<BTreeSet<u16>>> = OnceLock::new();
    let claimed = CLAIMED_PORTS.get_or_init(|| Mutex::new(BTreeSet::new()));
    loop {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral listener");
        let port = listener.local_addr().expect("address").port();
        if port <= 65_516 {
            let mut claimed = claimed.lock().expect("claimed ports");
            if (port..port + 20).all(|candidate| !claimed.contains(&candidate)) {
                claimed.extend(port..port + 20);
                return port;
            }
        }
    }
}

fn contiguous_ports() -> (u16, Vec<TcpListener>) {
    for candidate in 40_000..60_000_u16 {
        let mut listeners = Vec::new();
        for offset in 0..20_u16 {
            match TcpListener::bind(("127.0.0.1", candidate + offset)) {
                Ok(listener) => listeners.push(listener),
                Err(_) => break,
            }
        }
        if listeners.len() == 20 {
            return (candidate, listeners);
        }
    }
    panic!("could not reserve twenty contiguous ports");
}

fn vplan_record(state: &Path) -> PathBuf {
    fs::read_dir(state)
        .expect("state entries")
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("run"))
        .expect("run record")
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).expect("script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("mode");
}

fn artifact(path: &Path) {
    fs::create_dir_all(path.parent().expect("parent")).expect("artifact parent");
    fs::write(
        path,
        "<!DOCTYPE html>\n<html><head><title>Plan</title></head><body><main id=\"target\">Target</main></body></html>\n",
    )
    .expect("artifact");
}

#[test]
fn viz_native_lifecycle_routes_cache_and_security_are_complete() {
    let root = root();
    let home = tempfile::tempdir().expect("home");
    for directory in ["state", "data", "config", "projects"] {
        fs::create_dir(home.path().join(directory)).expect("home directory");
    }
    let readers = home.path().join("readers");
    fs::create_dir(&readers).expect("readers");
    let snapshot = home.path().join("snapshot.json");
    fs::write(
        &snapshot,
        format!(
            "{{\"schema\":\"mx-system-snapshot.v1\",\"generated\":\"2026-08-12T12:00:00Z\",\"roots\":{{\"data\":\"{}\"}},\"tasks\":[],\"scout_reports\":[]}}\n",
            home.path().join("data").display()
        ),
    )
    .expect("snapshot");
    executable(
        &readers.join("snapshot.sh"),
        "#!/bin/sh\ncat \"$MX_VIZ_FIXTURE\"\n",
    );
    executable(
        &readers.join("doctor.sh"),
        "#!/bin/sh\nprintf '%s\\n' '{\"schema\":\"mx-doctor.v1\",\"exit_code\":2}'\nexit 2\n",
    );
    executable(
        &readers.join("timeline.sh"),
        "#!/bin/sh\nprintf '%s\\n' '{\"ts\":\"2026-08-12T12:00:00Z\",\"event\":\"started\"}'\n",
    );
    let port = free_port();
    let mut serve = command(&root, home.path(), &["services", "mx-viz.sh", "serve"]);
    serve
        .env("MX_VIZ_PORT", port.to_string())
        .env("MX_VIZ_IDLE_SECS", "30")
        .env("MX_VIZ_POLL_MS", "77")
        .env("MX_VIZ_REFRESH_SECS", "30")
        .env("MX_VIZ_SNAPSHOT_BIN", readers.join("snapshot.sh"))
        .env("MX_VIZ_DOCTOR_BIN", readers.join("doctor.sh"))
        .env("MX_VIZ_TIMELINE_BIN", readers.join("timeline.sh"))
        .env("MX_VIZ_FIXTURE", &snapshot);
    let url = assert_success(&serve.output().expect("serve"));
    assert_eq!(url, format!("http://127.0.0.1:{port}/"));
    let run_record = home.path().join("state/.viz/server.run");
    let values = record(&run_record);
    assert_eq!(
        fs::metadata(&run_record)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let pid = values["pid"].parse::<u32>().expect("pid");

    let (status, _, body) = get(port, "/");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("content=\"77\""));
    assert_eq!(get(port, "/assets/app.js").0, 200);
    assert_eq!(get(port, "/assets/app.css").0, 200);
    let (status, headers, body) = get(port, "/api/state");
    assert_eq!(status, 200);
    assert!(serde_json::from_slice::<serde_json::Value>(&body).is_ok());
    let etag = headers["etag"].clone();
    let (status, _, _) = http_stream(mut_request(
        port,
        "GET",
        "/api/state",
        &[("If-None-Match", &etag)],
        &[],
    ));
    assert_eq!(status, 304);
    assert_eq!(get(port, "/api/doctor").0, 200);
    assert_eq!(get(port, "/api/timeline/task-1").0, 200);
    assert_eq!(get(port, "/api/timeline/%2e%2e%2fsecret").0, 400);
    assert_eq!(get(port, "/api/meta").0, 200);
    assert_eq!(get(port, "/missing").0, 404);
    assert_eq!(
        http_stream(mut_request(port, "POST", "/", &[], &[]),).0,
        405
    );
    assert_eq!(get(port, "/artifact/not-allowed/file").0, 403);
    assert_eq!(get(port, "/artifact/data/%2e%2e/README.md").0, 403);
    assert_eq!(get(port, "/artifact/docs/missing.md").0, 404);

    let rediscovered = assert_success(&run(
        &root,
        home.path(),
        &["services", "mx-viz.sh", "serve"],
    ));
    assert_eq!(rediscovered, url);
    let status = assert_success(&run(
        &root,
        home.path(),
        &["services", "mx-viz.sh", "status"],
    ));
    assert!(status.contains(&format!("pid={pid}")));
    assert_success(&run(&root, home.path(), &["services", "mx-viz.sh", "stop"]));
    wait_until("viz cleanup", || !run_record.exists());
    assert_eq!(
        assert_success(&run(&root, home.path(), &["services", "mx-viz.sh", "stop"])),
        "dashboard is not running"
    );
    let stopped = run(&root, home.path(), &["services", "mx-viz.sh", "status"]);
    assert_eq!(stopped.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&stopped.stdout).trim(), "stopped");
}

fn mut_request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n",
        body.len()
    )
    .expect("head");
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").expect("header");
    }
    write!(stream, "Connection: close\r\n\r\n").expect("end");
    stream.write_all(body).expect("body");
    stream
}

fn http_stream(mut stream: TcpStream) -> (u16, BTreeMap<String, String>, Vec<u8>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("response");
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("headers");
    let head = String::from_utf8_lossy(&response[..split]);
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .expect("status");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    (status, headers, response[split + 4..].to_vec())
}

#[test]
fn vplan_native_round_trip_assets_validation_and_fault_recovery_are_complete() {
    let root = root();
    fs::create_dir_all(root.join("data")).expect("root data");
    let artifacts = tempfile::Builder::new()
        .prefix(".mx-services-runtime.")
        .tempdir_in(root.join("data"))
        .expect("artifact root");
    let home = tempfile::tempdir().expect("home");
    fs::create_dir(home.path().join("state")).expect("state");
    let file = artifacts.path().join("plan.html");
    artifact(&file);
    let port = free_port();
    let mut review = command(
        &root,
        home.path(),
        &[
            "services",
            "mx-vplan.sh",
            "review",
            file.to_str().expect("path"),
        ],
    );
    review
        .env("MX_VPLAN_PORT", port.to_string())
        .env("MX_VPLAN_IDLE_SECS", "30");
    assert_eq!(
        assert_success(&review.output().expect("review")),
        format!("http://127.0.0.1:{port}/")
    );
    let state = home.path().join("state/.vplan");
    let run_record = vplan_record(&state);
    let values = record(&run_record);
    let token = values["token"].clone();
    assert_eq!(get(port, "/").0, 200);
    assert_eq!(get(port, "/__vplan/sdk.js").0, 200);
    assert_eq!(get(port, "/__vplan/sdk.css").0, 200);
    assert_eq!(get(port, "/__vplan/mermaid.min.js").0, 200);
    assert_eq!(get(port, "/__vplan/root/%2e%2e/README.md").0, 400);
    assert_eq!(get(port, "/__vplan/missing").0, 404);
    assert_eq!(get(port, "/confirm").0, 404);
    assert_eq!(
        post(
            port,
            "/confirm",
            "wrong",
            "application/json",
            b"{\"comments\":[]}"
        )
        .0,
        403
    );
    assert_eq!(
        post(port, "/confirm", &token, "text/plain", b"{\"comments\":[]}").0,
        415
    );
    assert_eq!(
        post(port, "/confirm", &token, "application/json", b"bad").0,
        400
    );
    let payload = br##"{"comments":[{"id":"c1","selector":"#target","anchor_text":"Target","nearest_heading":"Plan","comment":"Change it","ts":"2026-08-12T12:00:00Z","resolved":false}]}"##;
    let (status, response) = post(port, "/confirm", &token, "application/json", payload);
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&response));
    wait_until("vplan cleanup", || !run_record.exists());
    let comments = assert_success(&run(
        &root,
        home.path(),
        &[
            "services",
            "mx-vplan.sh",
            "comments",
            file.to_str().expect("path"),
        ],
    ));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&comments).unwrap()[0]["id"],
        "c1"
    );

    let malformed = artifacts.path().join("malformed.html");
    fs::write(
        &malformed,
        "<html><head></head><body><script type=\"application/json\" id=\"vplan-comments\">bad</script></body></html>",
    )
    .expect("malformed");
    let output = run(
        &root,
        home.path(),
        &[
            "services",
            "mx-vplan.sh",
            "comments",
            malformed.to_str().expect("path"),
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("malformed"));

    let new_file = artifacts.path().join("nested/new.html");
    let created = assert_success(&run(
        &root,
        home.path(),
        &[
            "services",
            "mx-vplan.sh",
            "new",
            new_file.to_str().expect("path"),
        ],
    ));
    assert_eq!(created, new_file.display().to_string());
    assert!(
        !run(
            &root,
            home.path(),
            &[
                "services",
                "mx-vplan.sh",
                "new",
                new_file.to_str().expect("path")
            ],
        )
        .status
        .success()
    );
    assert_success(&run(
        &root,
        home.path(),
        &["services", "mx-vplan.sh", "--self-check"],
    ));
    assert!(
        assert_success(&run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "--help"]
        ))
        .contains("MX_VPLAN_PORT")
    );
}

#[test]
fn local_service_usage_and_invalid_boundaries_fail_before_mutation() {
    let root = root();
    let home = tempfile::tempdir().expect("home");
    fs::create_dir(home.path().join("state")).expect("state");
    for args in [
        vec!["services", "unknown"],
        vec!["services", "mx-viz.sh", "unknown"],
        vec!["services", "mx-vplan.sh", "new"],
        vec!["services", "mx-vplan.sh", "--self-check", "extra"],
        vec!["services", "viz-server"],
        vec!["services", "vplan-server"],
    ] {
        assert!(!run(&root, home.path(), &args).status.success(), "{args:?}");
    }
    assert!(
        assert_success(&run(
            &root,
            home.path(),
            &["services", "mx-viz.sh", "--help"]
        ))
        .contains("MX_VIZ_PORT")
    );
    let outside = home.path().join("outside.html");
    artifact(&outside);
    assert!(
        !run(
            &root,
            home.path(),
            &[
                "services",
                "mx-vplan.sh",
                "review",
                outside.to_str().expect("path")
            ],
        )
        .status
        .success()
    );
    fs::create_dir_all(root.join("data")).expect("root data");
    let newline_artifact = root
        .join("data")
        .join(format!(".mx-newline-{}\nplan.html", std::process::id()));
    artifact(&newline_artifact);
    assert!(
        !run(
            &root,
            home.path(),
            &[
                "services",
                "mx-vplan.sh",
                "comments",
                newline_artifact.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    fs::remove_file(&newline_artifact).expect("remove newline fixture");
}

#[test]
fn stale_record_recovery_and_active_stop_cover_both_controller_contracts() {
    let root = root();
    let home = tempfile::tempdir().expect("home");
    fs::create_dir_all(home.path().join("state")).expect("state");
    let viz_record = home.path().join("state/.viz/server.run");

    assert_success(&run(&root, home.path(), &["services", "mx-viz.sh", "stop"]));
    fs::create_dir_all(viz_record.parent().unwrap()).expect("viz state");
    fs::write(
        &viz_record,
        "version=1\npid=0\ntoken=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .expect("stale record");
    let status = run(&root, home.path(), &["services", "mx-viz.sh", "status"]);
    assert_eq!(status.status.code(), Some(1));
    assert!(!viz_record.exists());
    fs::write(
        &viz_record,
        "version=1\npid=0\ntoken=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .expect("stale record");
    assert!(
        assert_success(&run(&root, home.path(), &["services", "mx-viz.sh", "stop"]))
            .contains("removed stale")
    );

    fs::write(
        &viz_record,
        "version=1\npid=0\ntoken=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .expect("stale record before serve");
    let port = free_port();
    let mut serve = command(&root, home.path(), &["services", "mx-viz.sh", "serve"]);
    serve
        .env("MX_VIZ_PORT", port.to_string())
        .env("MX_VIZ_IDLE_SECS", "30");
    assert_success(&serve.output().expect("serve"));
    assert_success(&run(&root, home.path(), &["services", "mx-viz.sh", "stop"]));

    fs::create_dir_all(root.join("data")).expect("root data");
    let artifacts = tempfile::Builder::new()
        .prefix(".mx-services-stop.")
        .tempdir_in(root.join("data"))
        .expect("artifacts");
    let file = artifacts.path().join("plan.html");
    artifact(&file);
    let port = free_port();
    let mut review = command(
        &root,
        home.path(),
        &["services", "mx-vplan.sh", "review", file.to_str().unwrap()],
    );
    review
        .env("MX_VPLAN_PORT", port.to_string())
        .env("MX_VPLAN_IDLE_SECS", "30");
    let url = assert_success(&review.output().expect("review"));
    assert_eq!(
        assert_success(&run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "review", file.to_str().unwrap()]
        )),
        url
    );
    let state = home.path().join("state/.vplan");
    let record = vplan_record(&state);
    assert!(
        assert_success(&run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()]
        ))
        .contains("stopped review")
    );
    assert!(
        assert_success(&run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()]
        ))
        .contains("no active review")
    );
    fs::write(
        &record,
        "version=1\npid=0\ntoken=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .expect("stale review");
    assert!(
        assert_success(&run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()]
        ))
        .contains("removed stale")
    );

    fs::write(
        &record,
        "version=1\npid=0\ntoken=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .expect("stale review before serve");
    let next_port = free_port();
    let mut review = command(
        &root,
        home.path(),
        &["services", "mx-vplan.sh", "review", file.to_str().unwrap()],
    );
    review
        .env("MX_VPLAN_PORT", next_port.to_string())
        .env("MX_VPLAN_IDLE_SECS", "30");
    assert_success(&review.output().expect("review after stale"));
    assert_success(&run(
        &root,
        home.path(),
        &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()],
    ));
    fs::write(
        &record,
        "version=1\npid=999999\npid_identity=dead-marker\ntoken=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .expect("dead identity review");
    assert!(
        assert_success(&run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()]
        ))
        .contains("removed stale")
    );
}

#[test]
fn server_argument_validation_and_port_exhaustion_fail_before_publication() {
    let root = root();
    let home = tempfile::tempdir().expect("home");
    let state = home.path().join("state");
    let outside = tempfile::tempdir().expect("outside");
    fs::create_dir(&state).expect("state");
    let token = "a".repeat(64);
    let root_s = root.to_str().unwrap();
    let home_s = home.path().to_str().unwrap();
    let state_s = state.to_str().unwrap();
    let outside_s = outside.path().to_str().unwrap();
    let run_record = state.join("server.run");
    let lock = state.join("server.lock");
    let record_s = run_record.to_str().unwrap();
    let lock_s = lock.to_str().unwrap();

    for args in [
        vec![
            "services",
            "viz-server",
            "--serve",
            root_s,
            home_s,
            outside_s,
            record_s,
            lock_s,
            token.as_str(),
            "4890",
        ],
        vec![
            "services",
            "viz-server",
            "--serve",
            root_s,
            home_s,
            state_s,
            record_s,
            lock_s,
            "bad",
            "4890",
        ],
        vec![
            "services",
            "viz-server",
            "--serve",
            root_s,
            home_s,
            state_s,
            record_s,
            lock_s,
            token.as_str(),
            "0",
        ],
    ] {
        assert!(!run(&root, home.path(), &args).status.success());
    }
    let mut bad_refresh = command(
        &root,
        home.path(),
        &[
            "services",
            "viz-server",
            "--serve",
            root_s,
            home_s,
            state_s,
            record_s,
            lock_s,
            token.as_str(),
            "4890",
        ],
    );
    bad_refresh.env("MX_VIZ_REFRESH_SECS", "bad");
    assert!(!bad_refresh.output().expect("bad refresh").status.success());

    let empty_root = tempfile::tempdir().expect("empty root");
    let empty_home = tempfile::tempdir().expect("empty home");
    let empty_state = empty_home.path().join("state");
    fs::create_dir(&empty_state).expect("empty state");
    let empty_record = empty_state.join("record");
    let empty_lock = empty_state.join("lock");
    let empty_args = [
        "services",
        "viz-server",
        "--serve",
        empty_root.path().to_str().unwrap(),
        empty_home.path().to_str().unwrap(),
        empty_state.to_str().unwrap(),
        empty_record.to_str().unwrap(),
        empty_lock.to_str().unwrap(),
        token.as_str(),
        "4890",
    ];
    assert!(!run(&root, home.path(), &empty_args).status.success());

    let outside_artifact = outside.path().join("outside.html");
    artifact(&outside_artifact);
    let artifact_s = outside_artifact.to_str().unwrap();
    for args in [
        vec![
            "services",
            "vplan-server",
            "--serve",
            artifact_s,
            root_s,
            record_s,
            lock_s,
            token.as_str(),
            "4870",
        ],
        vec![
            "services",
            "vplan-server",
            "--serve",
            artifact_s,
            outside_s,
            record_s,
            lock_s,
            "bad",
            "4870",
        ],
        vec![
            "services",
            "vplan-server",
            "--serve",
            artifact_s,
            outside_s,
            record_s,
            lock_s,
            token.as_str(),
            "0",
        ],
    ] {
        assert!(!run(&root, home.path(), &args).status.success());
    }
    let local_artifact = empty_root.path().join("plan.html");
    artifact(&local_artifact);
    let missing_assets = [
        "services",
        "vplan-server",
        "--serve",
        local_artifact.to_str().unwrap(),
        empty_root.path().to_str().unwrap(),
        empty_record.to_str().unwrap(),
        empty_lock.to_str().unwrap(),
        token.as_str(),
        "4870",
    ];
    assert!(!run(&root, home.path(), &missing_assets).status.success());

    let root_artifact = root.join("plans/rust_port/12-viz-vplan-services.html");
    for (entry, variable) in [
        ("mx-viz.sh", "MX_VIZ_PORT"),
        ("mx-vplan.sh", "MX_VPLAN_PORT"),
    ] {
        let args = if entry == "mx-viz.sh" {
            vec!["services", entry, "serve"]
        } else {
            vec!["services", entry, "review", root_artifact.to_str().unwrap()]
        };
        let mut invalid = command(&root, home.path(), &args);
        invalid.env(variable, "bad");
        assert!(!invalid.output().expect("invalid port").status.success());
    }
    assert!(
        !run(&root, home.path(), &["services", "mx-vplan.sh", "unknown"])
            .status
            .success()
    );

    let mut bad_viz_idle = command(&root, home.path(), &["services", "mx-viz.sh", "serve"]);
    bad_viz_idle.env("MX_VIZ_IDLE_SECS", "bad");
    assert!(
        !bad_viz_idle
            .output()
            .expect("bad viz idle")
            .status
            .success()
    );
    let mut bad_vplan_idle = command(
        &root,
        home.path(),
        &[
            "services",
            "mx-vplan.sh",
            "review",
            root_artifact.to_str().unwrap(),
        ],
    );
    bad_vplan_idle.env("MX_VPLAN_IDLE_SECS", "bad");
    assert!(
        !bad_vplan_idle
            .output()
            .expect("bad vplan idle")
            .status
            .success()
    );

    let (base, listeners) = contiguous_ports();
    let snapshot = home.path().join("snapshot.sh");
    executable(
        &snapshot,
        "#!/bin/sh\nprintf '%s\\n' '{\"roots\":{\"data\":\"/tmp\"},\"tasks\":[],\"scout_reports\":[]}'\n",
    );
    let mut serve = command(&root, home.path(), &["services", "mx-viz.sh", "serve"]);
    serve
        .env("MX_VIZ_PORT", base.to_string())
        .env("MX_VIZ_SNAPSHOT_BIN", &snapshot);
    let failed = serve.output().expect("exhausted serve");
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("no loopback port"));
    drop(listeners);
}

#[test]
fn vplan_atomic_publication_faults_preserve_a_recoverable_service() {
    let root = root();
    fs::create_dir_all(root.join("data")).expect("root data");
    let artifacts = tempfile::Builder::new()
        .prefix(".mx-services-fault.")
        .tempdir_in(root.join("data"))
        .expect("artifacts");
    let home = tempfile::tempdir().expect("home");
    fs::create_dir(home.path().join("state")).expect("state");
    let payload = br##"{"comments":[{"id":"fault","selector":"#target","anchor_text":"Target","nearest_heading":"Plan","comment":"Fault","ts":"2026-08-12T12:00:00Z","resolved":false}]}"##;

    for fault in ["before-write", "after-write", "after-mode", "after-rename"] {
        let file = artifacts.path().join(format!("{fault}.html"));
        artifact(&file);
        let port = free_port();
        let mut review = command(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "review", file.to_str().unwrap()],
        );
        review
            .env("MX_VPLAN_PORT", port.to_string())
            .env("MX_VPLAN_IDLE_SECS", "30")
            .env("MX_VPLAN_CONFIRM_FAULT", fault);
        assert_success(&review.output().expect("review"));
        let values = record(&vplan_record(&home.path().join("state/.vplan")));
        let (status, _) = post(
            port,
            "/confirm",
            &values["token"],
            "application/json",
            payload,
        );
        assert_eq!(status, 400);
        assert_success(&run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()],
        ));
    }
}

#[test]
fn vplan_crash_points_leave_only_old_or_fully_published_artifacts_and_stale_records() {
    let root = root();
    fs::create_dir_all(root.join("data")).expect("root data");
    let artifacts = tempfile::Builder::new()
        .prefix(".mx-services-kill.")
        .tempdir_in(root.join("data"))
        .expect("artifacts");
    let home = tempfile::tempdir().expect("home");
    fs::create_dir(home.path().join("state")).expect("state");
    let payload = br##"{"comments":[{"id":"kill","selector":"#target","anchor_text":"Target","nearest_heading":"Plan","comment":"Kill","ts":"2026-08-12T12:00:00Z","resolved":false}]}"##;

    for point in [
        "before-write",
        "after-rename",
        "after-response",
        "before-record-cleanup",
    ] {
        let file = artifacts.path().join(format!("{point}.html"));
        artifact(&file);
        let original = fs::read(&file).expect("original");
        let port = free_port();
        let mut review = command(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "review", file.to_str().unwrap()],
        );
        review
            .env("MX_VPLAN_PORT", port.to_string())
            .env("MX_VPLAN_IDLE_SECS", "30")
            .env("MX_VPLAN_CONFIRM_KILL_POINT", point);
        assert_success(&review.output().expect("review"));
        let run_record = vplan_record(&home.path().join("state/.vplan"));
        let values = record(&run_record);
        let pid = values["pid"].parse::<u32>().expect("pid");
        let status = post_maybe_status(port, &values["token"], payload);
        wait_until("crashed review", || !process_alive(pid));
        let after = fs::read(&file).expect("artifact after crash");
        if point == "before-write" {
            assert_eq!(after, original);
            assert_eq!(status, None);
        } else {
            assert!(String::from_utf8_lossy(&after).contains("vplan-comments"));
            if matches!(point, "after-response" | "before-record-cleanup") {
                assert_eq!(status, Some(200));
            }
        }
        assert!(
            run_record.exists(),
            "{point} unexpectedly cleaned its record"
        );
        assert_success(&run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()],
        ));
        assert!(!run_record.exists());
    }
}

#[test]
fn concurrent_serve_calls_converge_to_one_service_generation() {
    let root = root();
    let home = tempfile::tempdir().expect("home");
    fs::create_dir(home.path().join("state")).expect("state");
    let viz_port = free_port();
    let mut viz_children = Vec::new();
    for _ in 0..6 {
        let mut serve = command(&root, home.path(), &["services", "mx-viz.sh", "serve"]);
        serve
            .env("MX_VIZ_PORT", viz_port.to_string())
            .env("MX_VIZ_IDLE_SECS", "30")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        viz_children.push(serve.spawn().expect("spawn viz serve"));
    }
    let viz_urls = viz_children
        .into_iter()
        .map(|child| assert_success(&child.wait_with_output().expect("viz output")))
        .collect::<Vec<_>>();
    assert!(viz_urls.iter().all(|url| url == &viz_urls[0]));
    assert_eq!(viz_urls[0], format!("http://127.0.0.1:{viz_port}/"));
    assert_success(&run(&root, home.path(), &["services", "mx-viz.sh", "stop"]));

    fs::create_dir_all(root.join("data")).expect("root data");
    let artifacts = tempfile::Builder::new()
        .prefix(".mx-services-concurrent.")
        .tempdir_in(root.join("data"))
        .expect("artifacts");
    let file = artifacts.path().join("plan.html");
    artifact(&file);
    let vplan_port = free_port();
    let mut vplan_children = Vec::new();
    for _ in 0..6 {
        let mut review = command(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "review", file.to_str().unwrap()],
        );
        review
            .env("MX_VPLAN_PORT", vplan_port.to_string())
            .env("MX_VPLAN_IDLE_SECS", "30")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        vplan_children.push(review.spawn().expect("spawn vplan review"));
    }
    let vplan_urls = vplan_children
        .into_iter()
        .map(|child| assert_success(&child.wait_with_output().expect("vplan output")))
        .collect::<Vec<_>>();
    assert!(vplan_urls.iter().all(|url| url == &vplan_urls[0]));
    assert_eq!(vplan_urls[0], format!("http://127.0.0.1:{vplan_port}/"));
    assert_success(&run(
        &root,
        home.path(),
        &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()],
    ));
}

#[test]
fn tampered_live_records_fail_closed_without_signaling_the_service() {
    let root = root();
    let home = tempfile::tempdir().expect("home");
    fs::create_dir(home.path().join("state")).expect("state");
    let viz_port = free_port();
    let mut serve = command(&root, home.path(), &["services", "mx-viz.sh", "serve"]);
    serve
        .env("MX_VIZ_PORT", viz_port.to_string())
        .env("MX_VIZ_IDLE_SECS", "30");
    assert_success(&serve.output().expect("serve"));
    let viz_record = home.path().join("state/.viz/server.run");
    let original = fs::read_to_string(&viz_record).expect("viz record");
    let values = record(&viz_record);
    let pid = values["pid"].parse::<u32>().expect("pid");
    let tampered = original.replace(
        &format!("token={}", values["token"]),
        &format!("token={}", "c".repeat(64)),
    );
    fs::write(&viz_record, &tampered).expect("tamper viz record");
    assert!(
        !run(&root, home.path(), &["services", "mx-viz.sh", "stop"])
            .status
            .success()
    );
    assert!(process_alive(pid));
    fs::write(&viz_record, &original).expect("restore viz record");
    assert_success(&run(&root, home.path(), &["services", "mx-viz.sh", "stop"]));

    fs::create_dir_all(root.join("data")).expect("root data");
    let artifacts = tempfile::Builder::new()
        .prefix(".mx-services-tamper.")
        .tempdir_in(root.join("data"))
        .expect("artifacts");
    let file = artifacts.path().join("plan.html");
    let other = artifacts.path().join("other.html");
    artifact(&file);
    artifact(&other);
    let vplan_port = free_port();
    let mut review = command(
        &root,
        home.path(),
        &["services", "mx-vplan.sh", "review", file.to_str().unwrap()],
    );
    review
        .env("MX_VPLAN_PORT", vplan_port.to_string())
        .env("MX_VPLAN_IDLE_SECS", "30");
    assert_success(&review.output().expect("review"));
    let run_record = vplan_record(&home.path().join("state/.vplan"));
    let original = fs::read_to_string(&run_record).expect("vplan record");
    let values = record(&run_record);
    let pid = values["pid"].parse::<u32>().expect("pid");
    fs::write(
        &run_record,
        original.replace(
            &format!("token={}", values["token"]),
            &format!("token={}", "d".repeat(64)),
        ),
    )
    .expect("tamper token");
    assert!(
        !run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()]
        )
        .status
        .success()
    );
    assert!(process_alive(pid));
    fs::write(
        &run_record,
        original.replace(
            &format!("artifact={}", file.display()),
            &format!("artifact={}", other.display()),
        ),
    )
    .expect("tamper artifact");
    assert!(
        !run(
            &root,
            home.path(),
            &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()]
        )
        .status
        .success()
    );
    assert!(process_alive(pid));
    fs::write(&run_record, &original).expect("restore vplan record");
    assert_success(&run(
        &root,
        home.path(),
        &["services", "mx-vplan.sh", "stop", file.to_str().unwrap()],
    ));
}

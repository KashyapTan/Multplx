//! Disposable GET-only dashboard lifecycle and cached snapshot service.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use multplx_core::process::{
    ProcessProbe, ProcessTerminator, SystemProcessProbe, SystemProcessTerminator,
};
use serde_json::{Value, json};

use crate::http::{
    Request, Response, content_type, encode_component, percent_decode, read_request, write_response,
};

use super::{
    Result, ServiceError, VIZ_HELP, accept_loop, acquire_lock, bind_loopback, canonical_directory,
    ensure_private_directory, http_get_json, is_within, parse_integer_env, parse_port,
    random_token, record_identity, record_live, record_map, record_process_live,
    remove_record_if_matches, run_bounded_command, sha256_hex, shutdown_flag, start_service,
    utc_now, utf8_arg, valid_token, write_record,
};

const VERSION: u64 = 1;
const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug)]
struct SnapshotCache {
    body: Vec<u8>,
    hash: String,
    snapshot_hash: String,
    refreshed_at: Instant,
    generated: Option<String>,
}

#[derive(Debug)]
struct Runtime {
    last_request: Instant,
    last_request_at: String,
    last_poll_at: Option<String>,
    cache: Option<SnapshotCache>,
}

struct ServerContext {
    root: PathBuf,
    asset_directory: PathBuf,
    home: PathBuf,
    state: PathBuf,
    started: String,
    port: u16,
    poll_ms: u64,
    refresh: Duration,
    snapshot_command: PathBuf,
    doctor_command: PathBuf,
    timeline_command: PathBuf,
    runtime: Mutex<Runtime>,
}

fn common_headers(mut response: Response) -> Response {
    response.headers.extend([
        ("Cache-Control".to_owned(), "no-store".to_owned()),
        (
            "Content-Security-Policy".to_owned(),
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'".to_owned(),
        ),
        ("Referrer-Policy".to_owned(), "no-referrer".to_owned()),
        ("X-Content-Type-Options".to_owned(), "nosniff".to_owned()),
        ("X-Frame-Options".to_owned(), "DENY".to_owned()),
    ]);
    response
}

fn artifact_headers(response: &mut Response) {
    response.headers.retain(|(name, _)| {
        !name.eq_ignore_ascii_case("Content-Security-Policy")
            && !name.eq_ignore_ascii_case("X-Frame-Options")
    });
    response.headers.extend([
        (
            "Content-Security-Policy".to_owned(),
            "default-src 'none'; script-src 'none'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'self'".to_owned(),
        ),
        ("X-Frame-Options".to_owned(), "SAMEORIGIN".to_owned()),
    ]);
}

fn response_file(path: &Path, transform: Option<&dyn Fn(String) -> String>) -> Result<Response> {
    let metadata = fs::metadata(path)
        .map_err(|error| ServiceError::new(format!("could not inspect requested file: {error}")))?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(ServiceError::new("requested path is not a bounded file"));
    }
    let mut bytes = fs::read(path)
        .map_err(|error| ServiceError::new(format!("could not read requested file: {error}")))?;
    if let Some(transform) = transform {
        let text = String::from_utf8(bytes)
            .map_err(|_| ServiceError::new("requested text asset is not UTF-8"))?;
        bytes = transform(text).into_bytes();
    }
    Ok(Response::new(200, bytes).header("Content-Type", content_type(path)))
}

fn environment(context: &ServerContext) -> Vec<(OsString, OsString)> {
    vec![
        (
            OsString::from("MX_ROOT_OVERRIDE"),
            context.root.as_os_str().to_owned(),
        ),
        (
            OsString::from("MX_HOME"),
            context.home.as_os_str().to_owned(),
        ),
        (
            OsString::from("MX_STATE_OVERRIDE"),
            context.state.as_os_str().to_owned(),
        ),
    ]
}

fn execute_json(
    command: &Path,
    args: &[&str],
    context: &ServerContext,
    accepted: &[i32],
    timeout: Duration,
) -> Result<(String, Value)> {
    let output = run_bounded_command(
        command,
        args,
        &environment(context),
        timeout,
        MAX_OUTPUT_BYTES,
    )?;
    let code = output.status.code().unwrap_or(1);
    if !accepted.contains(&code) {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(ServiceError::new(format!(
            "{} exited {code}{}{}",
            command.display(),
            if detail.is_empty() { "" } else { ": " },
            detail
        )));
    }
    let raw = String::from_utf8(output.stdout).map_err(|_| {
        ServiceError::new(format!("{} returned non-UTF-8 output", command.display()))
    })?;
    let raw = raw.trim().to_owned();
    if raw.is_empty() {
        return Err(ServiceError::new(format!(
            "{} returned no JSON",
            command.file_name().unwrap_or_default().to_string_lossy()
        )));
    }
    let value = serde_json::from_str(&raw).map_err(|error| {
        ServiceError::new(format!(
            "{} returned invalid JSON: {error}",
            command.display()
        ))
    })?;
    Ok((raw, value))
}

fn artifact_entry(
    root: &Path,
    root_name: &str,
    file: &Path,
    label: String,
    kind: &str,
) -> Option<Value> {
    let allowed_root = fs::canonicalize(root.join(root_name)).ok()?;
    let real_file = fs::canonicalize(file).ok()?;
    let metadata = fs::metadata(&real_file).ok()?;
    if !is_within(root, &allowed_root)
        || !metadata.is_file()
        || !is_within(&allowed_root, &real_file)
    {
        return None;
    }
    let relative = real_file.strip_prefix(&allowed_root).ok()?;
    let path = relative
        .components()
        .map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?
        .join("/");
    let encoded = relative
        .components()
        .map(|component| component.as_os_str().to_str().map(encode_component))
        .collect::<Option<Vec<_>>>()?
        .join("/");
    Some(json!({
        "root":root_name,
        "path":path,
        "label":label,
        "kind":kind,
        "url":format!("/artifact/{root_name}/{encoded}")
    }))
}

fn collect_artifacts(root: &Path, snapshot: &Value) -> Vec<Value> {
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let Some(data_root) = snapshot.pointer("/roots/data").and_then(Value::as_str) else {
        return entries;
    };
    if let Some(tasks) = snapshot.get("tasks").and_then(Value::as_array) {
        for task in tasks {
            let Some(id) = task.get("id").and_then(Value::as_str) else {
                continue;
            };
            for (name, kind) in [
                ("plan.html", "task-plan"),
                ("brief.md", "brief"),
                ("report.md", "report"),
            ] {
                let file = Path::new(data_root).join(id).join(name);
                if let Some(entry) =
                    artifact_entry(root, "data", &file, format!("{id}/{name}"), kind)
                    && let Some(url) = entry.get("url").and_then(Value::as_str)
                    && seen.insert(url.to_owned())
                {
                    entries.push(entry);
                }
            }
        }
    }
    if let Some(reports) = snapshot.get("scout_reports").and_then(Value::as_array) {
        for report in reports {
            let Some(path) = report.get("path").and_then(Value::as_str) else {
                continue;
            };
            let id = report.get("id").and_then(Value::as_str).unwrap_or_default();
            if let Some(entry) = artifact_entry(
                root,
                "data",
                Path::new(path),
                format!("{id}/report.md"),
                "report",
            ) && let Some(url) = entry.get("url").and_then(Value::as_str)
                && seen.insert(url.to_owned())
            {
                entries.push(entry);
            }
        }
    }
    entries
}

impl ServerContext {
    fn refresh_snapshot(&self) -> Result<SnapshotCache> {
        let mut runtime = self.runtime.lock().expect("runtime");
        if let Some(cache) = &runtime.cache
            && cache.refreshed_at.elapsed() < self.refresh
        {
            return Ok(cache.clone());
        }
        let (raw, snapshot) = execute_json(
            &self.snapshot_command,
            &["--json"],
            self,
            &[0],
            Duration::from_secs(60),
        )?;
        let artifacts = collect_artifacts(&self.root, &snapshot);
        let server = json!({"version":VERSION,"started":self.started,"pid":std::process::id()});
        let body = format!(
            "{{\"server\":{},\"artifacts\":{},\"snapshot\":{raw}}}\n",
            serde_json::to_string(&server).expect("server JSON"),
            serde_json::to_string(&artifacts).expect("artifact JSON")
        )
        .into_bytes();
        let cache = SnapshotCache {
            hash: sha256_hex(&body),
            snapshot_hash: sha256_hex(raw.as_bytes()),
            body,
            refreshed_at: Instant::now(),
            generated: snapshot
                .get("generated")
                .and_then(Value::as_str)
                .map(str::to_owned),
        };
        runtime.cache = Some(cache.clone());
        Ok(cache)
    }

    fn serve_artifact(&self, raw_path: &str) -> Result<Response> {
        let decoded = percent_decode(raw_path).map_err(|_| ServiceError::new("forbidden"))?;
        let suffix = decoded
            .strip_prefix("/artifact/")
            .ok_or_else(|| ServiceError::new("forbidden"))?;
        let segments = suffix.split('/').collect::<Vec<_>>();
        if segments.len() < 2
            || segments.iter().any(|segment| {
                segment.is_empty() || *segment == "." || *segment == ".." || segment.contains('\0')
            })
        {
            return Err(ServiceError::new("forbidden"));
        }
        let root_name = segments[0];
        if !matches!(root_name, "data" | "docs") {
            return Err(ServiceError::new("forbidden"));
        }
        let allowed_root = fs::canonicalize(self.root.join(root_name))
            .map_err(|_| ServiceError::new("forbidden"))?;
        if !is_within(&self.root, &allowed_root) {
            return Err(ServiceError::new("forbidden"));
        }
        let mut candidate = allowed_root.clone();
        for segment in &segments[1..] {
            candidate.push(segment);
        }
        let canonical = match fs::canonicalize(&candidate) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ServiceError::new("not found"));
            }
            Err(error) => {
                return Err(ServiceError::new(format!(
                    "could not resolve artifact: {error}"
                )));
            }
        };
        if !is_within(&allowed_root, &canonical) {
            return Err(ServiceError::new("forbidden"));
        }
        let mut response = common_headers(response_file(&canonical, None)?);
        artifact_headers(&mut response);
        Ok(response)
    }

    fn handle(&self, request: Request) -> Response {
        {
            let mut runtime = self.runtime.lock().expect("runtime");
            runtime.last_request = Instant::now();
            runtime.last_request_at = utc_now();
        }
        let raw_path = request.target.split('?').next().unwrap_or("/");
        if request.method != "GET" {
            return common_headers(
                Response::new(405, b"method not allowed\n".to_vec()).header("Allow", "GET"),
            );
        }
        if raw_path.starts_with("/artifact/") {
            return self.serve_artifact(raw_path).unwrap_or_else(|error| {
                let status = if error.message == "not found" {
                    404
                } else if error.message == "forbidden" {
                    403
                } else {
                    503
                };
                common_headers(Response::new(
                    status,
                    format!("{}\n", error.message).into_bytes(),
                ))
            });
        }
        match raw_path {
            "/" => {
                let poll = self.poll_ms.to_string();
                common_headers(
                    response_file(
                        &self.asset_directory.join("index.html"),
                        Some(&|source| source.replace("__MX_VIZ_POLL_MS__", &poll)),
                    )
                    .unwrap_or_else(error_json),
                )
            }
            "/assets/app.js" | "/assets/app.css" => common_headers(
                response_file(
                    &self
                        .asset_directory
                        .join(Path::new(raw_path).file_name().unwrap_or_default()),
                    None,
                )
                .unwrap_or_else(error_json),
            ),
            "/api/meta" => {
                let runtime = self.runtime.lock().expect("runtime");
                common_headers(Response::json(
                    200,
                    &json!({
                        "version":VERSION,
                        "started":self.started,
                        "pid":std::process::id(),
                        "port":self.port,
                        "last_request_at":runtime.last_request_at,
                        "last_poll_at":runtime.last_poll_at,
                        "snapshot_generated":runtime.cache.as_ref().and_then(|cache| cache.generated.clone()),
                        "hash":runtime.cache.as_ref().map(|cache| cache.hash.clone())
                    }),
                ))
            }
            "/api/state" => {
                self.runtime.lock().expect("runtime").last_poll_at = Some(utc_now());
                match self.refresh_snapshot() {
                    Ok(cache) => {
                        let etag = format!("\"{}\"", cache.hash);
                        if request
                            .headers
                            .get("if-none-match")
                            .is_some_and(|value| value == &etag || value == &cache.hash)
                        {
                            return common_headers(
                                Response::new(304, Vec::new())
                                    .header("ETag", etag)
                                    .header("X-Multplx-Content-Hash", cache.hash),
                            );
                        }
                        common_headers(
                            Response::new(200, cache.body)
                                .header("Content-Type", "application/json; charset=utf-8")
                                .header("ETag", etag)
                                .header("X-Multplx-Content-Hash", cache.hash)
                                .header("X-Multplx-Snapshot-Hash", cache.snapshot_hash),
                        )
                    }
                    Err(error) => common_headers(error_json(error)),
                }
            }
            "/api/doctor" => match execute_json(
                &self.doctor_command,
                &["--json"],
                self,
                &[0, 1, 2],
                Duration::from_secs(60),
            ) {
                Ok((_, value)) => common_headers(Response::json(200, &value)),
                Err(error) => common_headers(error_json(error)),
            },
            value if value.starts_with("/api/timeline/") => {
                let encoded = &value["/api/timeline/".len()..];
                let id = match percent_decode(encoded) {
                    Ok(id)
                        if !id.is_empty()
                            && id.len() <= 128
                            && id.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                            }) =>
                    {
                        id
                    }
                    _ => return common_headers(Response::new(400, b"invalid task id\n".to_vec())),
                };
                match run_bounded_command(
                    &self.timeline_command,
                    &[&id, "--json"],
                    &environment(self),
                    Duration::from_secs(30),
                    MAX_OUTPUT_BYTES,
                ) {
                    Ok(output) if output.status.success() => {
                        let records = String::from_utf8(output.stdout).ok().and_then(|text| {
                            text.lines()
                                .filter(|line| !line.is_empty())
                                .map(serde_json::from_str::<Value>)
                                .collect::<std::result::Result<Vec<_>, _>>()
                                .ok()
                        });
                        match records {
                            Some(records) => common_headers(Response::json(
                                200,
                                &json!({"task":id,"records":records}),
                            )),
                            None => common_headers(error_json(ServiceError::new(
                                "timeline returned invalid JSON",
                            ))),
                        }
                    }
                    Ok(output) => common_headers(error_json(ServiceError::new(format!(
                        "timeline exited {}",
                        output.status.code().unwrap_or(1)
                    )))),
                    Err(error) => common_headers(error_json(error)),
                }
            }
            _ => common_headers(Response::new(404, b"not found\n".to_vec())),
        }
    }
}

fn error_json(error: ServiceError) -> Response {
    Response::json(503, &json!({"error":error.message}))
}

fn handle_connection(mut stream: TcpStream, context: &ServerContext) {
    let response = match read_request(&mut stream, 0) {
        Ok(request) => context.handle(request),
        Err(response) => common_headers(response),
    };
    let _ = write_response(&mut stream, &response);
}

pub(super) fn run_server(args: &[OsString]) -> Result<i32> {
    if args.len() != 8 || args.first().and_then(|value| value.to_str()) != Some("--serve") {
        return Err(ServiceError::usage(
            "usage: mx services viz-server --serve <root> <home> <state> <run-record> <lock> <token> <first-port>",
        ));
    }
    let root = canonical_directory(Path::new(utf8_arg(args, 1, "root")?), "Multplx root")?;
    let home = canonical_directory(Path::new(utf8_arg(args, 2, "home")?), "MX_HOME")?;
    let state = canonical_directory(Path::new(utf8_arg(args, 3, "state")?), "state directory")?;
    if !is_within(&home, &state) {
        return Err(ServiceError::new("state path must stay inside MX_HOME"));
    }
    let run_record = PathBuf::from(utf8_arg(args, 4, "run record")?);
    let lock = PathBuf::from(utf8_arg(args, 5, "service lock")?);
    let token = utf8_arg(args, 6, "server token")?.to_owned();
    if !valid_token(&token) {
        return Err(ServiceError::new("server token is invalid"));
    }
    let first_port = utf8_arg(args, 7, "first port")?
        .parse::<u16>()
        .ok()
        .filter(|port| *port >= 1 && *port <= 65_516)
        .ok_or_else(|| ServiceError::new("first port must be an integer from 1 through 65516"))?;
    let idle = Duration::from_secs(parse_integer_env("MX_VIZ_IDLE_SECS", 1800, 1, 86_400)?);
    let poll_ms = parse_integer_env("MX_VIZ_POLL_MS", 2500, 1, 60_000)?;
    let refresh_value = std::env::var("MX_VIZ_REFRESH_SECS").unwrap_or_else(|_| "2".to_owned());
    let refresh_seconds = refresh_value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (0.1..=300.0).contains(value))
        .ok_or_else(|| {
            ServiceError::new("MX_VIZ_REFRESH_SECS must be a number from 0.1 through 300")
        })?;
    let asset_directory = fs::canonicalize(root.join("share/viz")).map_err(|error| {
        ServiceError::new(format!("viz asset directory is unavailable: {error}"))
    })?;
    let snapshot_command = std::env::var_os("MX_VIZ_SNAPSHOT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("bin/mx-system-snapshot.sh"));
    let doctor_command = std::env::var_os("MX_VIZ_DOCTOR_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("bin/mx-doctor.sh"));
    let timeline_command = std::env::var_os("MX_VIZ_TIMELINE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("bin/mx-timeline.sh"));
    let (listener, port) = bind_loopback(first_port)?;
    let shutdown = shutdown_flag()?;
    let started = utc_now();
    let context = Arc::new(ServerContext {
        root,
        asset_directory,
        home,
        state,
        started: started.clone(),
        port,
        poll_ms,
        refresh: Duration::from_secs_f64(refresh_seconds),
        snapshot_command,
        doctor_command,
        timeline_command,
        runtime: Mutex::new(Runtime {
            last_request: Instant::now(),
            last_request_at: started,
            last_poll_at: None,
            cache: None,
        }),
    });
    println!("READY {port}");
    std::io::stdout().flush().ok();
    let idle_context = Arc::clone(&context);
    let handler_context = Arc::clone(&context);
    accept_loop(
        listener,
        shutdown,
        move || {
            idle_context
                .runtime
                .lock()
                .expect("runtime")
                .last_request
                .elapsed()
                >= idle
        },
        move |stream| handle_connection(stream, &handler_context),
    );
    let _guard = acquire_lock(&lock)?;
    remove_record_if_matches(&run_record, std::process::id(), &token)?;
    Ok(0)
}

fn active_paths(root: &Path) -> (PathBuf, PathBuf) {
    let home = std::env::var_os("MX_HOME")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_owned());
    let state = std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"));
    (home, state)
}

fn prepare_state(root: &Path) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    let (home, state) = active_paths(root);
    fs::create_dir_all(&state).map_err(|error| {
        ServiceError::new(format!(
            "could not create dashboard state directory: {error}"
        ))
    })?;
    let viz = state.join(".viz");
    ensure_private_directory(&viz, "dashboard state directory")?;
    let home = canonical_directory(&home, "MX_HOME")?;
    let state = canonical_directory(&state, "state directory")?;
    if !is_within(&home, &state) {
        return Err(ServiceError::new("state path must stay inside MX_HOME"));
    }
    let record = viz.join("server.run");
    let lock = viz.join("serve.lock");
    Ok((home, state, record, lock))
}

fn serve_command(root: &Path) -> Result<i32> {
    let (home, state, record, lock) = prepare_state(root)?;
    let _guard = acquire_lock(&lock)?;
    if record.exists() {
        let existing = record_map(&record)?;
        if record_live(&existing, Some(&home)) {
            let port = existing
                .get("port")
                .ok_or_else(|| ServiceError::new("live dashboard record is missing port"))?;
            println!("http://127.0.0.1:{port}/");
            return Ok(0);
        }
        if record_process_live(&existing) {
            return Err(ServiceError::new(format!(
                "unsafe live dashboard run record: {}",
                record.display()
            )));
        }
        let pid = existing
            .get("pid")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let token = existing.get("token").cloned().unwrap_or_default();
        remove_record_if_matches(&record, pid, &token)?;
        if record.exists() {
            return Err(ServiceError::new(format!(
                "unsafe dashboard run record: {}",
                record.display()
            )));
        }
    }
    let token = random_token()?;
    let first_port = parse_port("MX_VIZ_PORT", 4890)?;
    let service_args = vec![
        OsString::from("--serve"),
        root.as_os_str().to_owned(),
        home.as_os_str().to_owned(),
        state.as_os_str().to_owned(),
        record.as_os_str().to_owned(),
        lock.as_os_str().to_owned(),
        OsString::from(&token),
        OsString::from(first_port.to_string()),
    ];
    let started = start_service("viz-server", &service_args)?;
    let pid = started.child.id();
    let identity = SystemProcessProbe::default().identity(pid).map_err(|_| {
        ServiceError::new("server started but its process identity could not be verified")
    })?;
    let bytes = format!(
        "version=1\nhome={}\nstate={}\nport={}\npid={}\npid_identity={}\ntoken={}\nstarted_at={}\n",
        home.display(),
        state.display(),
        started.port,
        pid,
        identity.marker,
        token,
        utc_now()
    );
    if let Err(error) = write_record(&record, bytes.as_bytes()) {
        let mut terminator = SystemProcessTerminator::default();
        let _ = terminator.terminate(&identity);
        return Err(error);
    }
    let port = started.port;
    drop(started.ready);
    drop(started.errors);
    drop(started.child);
    println!("http://127.0.0.1:{port}/");
    Ok(0)
}

fn status_command(root: &Path) -> Result<i32> {
    let (home, _, record, lock) = prepare_state(root)?;
    let _guard = acquire_lock(&lock)?;
    if !record.exists() {
        println!("stopped");
        return Ok(1);
    }
    let values = record_map(&record)?;
    if !record_live(&values, Some(&home)) {
        let pid = values
            .get("pid")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let token = values.get("token").cloned().unwrap_or_default();
        remove_record_if_matches(&record, pid, &token)?;
        println!("stopped");
        return Ok(1);
    }
    let port = values
        .get("port")
        .ok_or_else(|| ServiceError::new("live dashboard record is missing port"))?;
    let pid = values
        .get("pid")
        .ok_or_else(|| ServiceError::new("live dashboard record is missing pid"))?;
    let started = values.get("started_at").cloned().unwrap_or_default();
    let last_poll = port
        .parse::<u16>()
        .ok()
        .and_then(|port| http_get_json(port, "/api/meta"))
        .and_then(|value| {
            value
                .get("last_poll_at")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "never".to_owned());
    println!("running: http://127.0.0.1:{port}/ pid={pid} started={started} last_poll={last_poll}");
    Ok(0)
}

fn stop_command(root: &Path) -> Result<i32> {
    let (home, _, record, lock) = prepare_state(root)?;
    let guard = acquire_lock(&lock)?;
    if !record.exists() {
        println!("dashboard is not running");
        return Ok(0);
    }
    let values = record_map(&record)?;
    let pid = values
        .get("pid")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let token = values.get("token").cloned().unwrap_or_default();
    let Some(identity) = record_identity(&values) else {
        remove_record_if_matches(&record, pid, &token)?;
        println!("removed stale dashboard record");
        return Ok(0);
    };
    if record_process_live(&values) && !record_live(&values, Some(&home)) {
        return Err(ServiceError::new(format!(
            "unsafe live dashboard run record: {}",
            record.display()
        )));
    }
    let probe = SystemProcessProbe::default();
    if !probe.is_alive(pid) || !probe.identity(pid).is_ok_and(|actual| actual == identity) {
        remove_record_if_matches(&record, pid, &token)?;
        println!("removed stale dashboard record");
        return Ok(0);
    }
    let mut terminator = SystemProcessTerminator::default();
    terminator
        .terminate(&identity)
        .map_err(|_| ServiceError::new(format!("could not stop dashboard process {pid}")))?;
    drop(guard);
    if !terminator.wait_gone(&identity, Duration::from_secs(5)) {
        return Err(ServiceError::new(format!(
            "dashboard process {pid} did not stop after 5 seconds"
        )));
    }
    let _guard = acquire_lock(&lock)?;
    remove_record_if_matches(&record, pid, &token)?;
    println!("stopped dashboard");
    Ok(0)
}

pub(super) fn run_cli(args: &[OsString], source_root: &Path) -> Result<i32> {
    let root = canonical_directory(source_root, "Multplx root")?;
    match args.first().and_then(|value| value.to_str()) {
        Some("-h" | "--help") if args.len() == 1 => {
            print!("{VIZ_HELP}");
            Ok(0)
        }
        Some("serve") if args.len() == 1 => serve_command(&root),
        Some("status") if args.len() == 1 => status_command(&root),
        Some("stop") if args.len() == 1 => stop_command(&root),
        _ => {
            eprint!("{VIZ_HELP}");
            Ok(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Runtime, ServerContext, artifact_entry, collect_artifacts, response_file};
    use crate::http::Request;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    fn script(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("script");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("mode");
    }

    fn request(method: &str, target: &str) -> Request {
        Request {
            method: method.to_owned(),
            target: target.to_owned(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    fn context(root: PathBuf, command: PathBuf) -> ServerContext {
        let started = "2026-08-12T12:00:00Z".to_owned();
        ServerContext {
            asset_directory: root.join("share/viz"),
            home: root.clone(),
            state: root.join("state"),
            root,
            started: started.clone(),
            port: 4890,
            poll_ms: 123,
            refresh: Duration::from_secs(60),
            snapshot_command: command.clone(),
            doctor_command: command.clone(),
            timeline_command: command,
            runtime: Mutex::new(Runtime {
                last_request: Instant::now(),
                last_request_at: started,
                last_poll_at: None,
                cache: None,
            }),
        }
    }

    #[test]
    fn artifact_collection_accepts_only_canonical_allowlisted_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("data/task")).expect("data");
        fs::create_dir(temp.path().join("docs")).expect("docs");
        fs::write(temp.path().join("data/task/plan.html"), "plan").expect("plan");
        fs::write(temp.path().join("data/task/report.md"), "report").expect("report");
        let snapshot = json!({
            "roots":{"data":temp.path().join("data")},
            "tasks":[{"id":"task"}],
            "scout_reports":[]
        });
        let root = fs::canonicalize(temp.path()).expect("canonical root");
        let artifacts = collect_artifacts(&root, &snapshot);
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0]["url"], "/artifact/data/task/plan.html");
        assert!(
            artifact_entry(&root, "data", &root.join("missing"), "x".to_owned(), "x").is_none()
        );
        assert!(collect_artifacts(&root, &json!({})).is_empty());
        let mixed = json!({
            "roots":{"data":root.join("data")},
            "tasks":[{}, {"id":"task"}],
            "scout_reports":[{}, {"id":"r1","path":root.join("data/task/report.md")}]
        });
        assert_eq!(collect_artifacts(&root, &mixed).len(), 2);
    }

    #[test]
    fn direct_dashboard_routes_cover_cache_artifacts_and_command_failures() {
        let temp = tempfile::tempdir().expect("tempdir");
        for directory in ["share/viz", "state", "data/task", "docs"] {
            fs::create_dir_all(temp.path().join(directory)).expect("directory");
        }
        fs::write(
            temp.path().join("share/viz/index.html"),
            "<meta content=\"__MX_VIZ_POLL_MS__\">",
        )
        .expect("index");
        fs::write(temp.path().join("share/viz/app.js"), "js").expect("js");
        fs::write(temp.path().join("share/viz/app.css"), "css").expect("css");
        fs::write(temp.path().join("data/task/plan.html"), "plan").expect("artifact");
        let command = temp.path().join("reader.sh");
        script(
            &command,
            &format!(
                "printf '%s\\n' '{{\"generated\":\"now\",\"roots\":{{\"data\":\"{}\"}},\"tasks\":[{{\"id\":\"task\"}}],\"scout_reports\":[]}}'",
                temp.path().join("data").display()
            ),
        );
        let root = fs::canonicalize(temp.path()).expect("root");
        let context = context(root.clone(), command.clone());
        assert_eq!(context.handle(request("GET", "/")).status, 200);
        assert_eq!(context.handle(request("GET", "/assets/app.js")).status, 200);
        assert_eq!(context.handle(request("GET", "/api/state")).status, 200);
        assert_eq!(context.handle(request("GET", "/api/state")).status, 200);
        assert_eq!(context.handle(request("GET", "/api/meta?x=1")).status, 200);
        assert_eq!(context.handle(request("GET", "/api/doctor")).status, 200);
        assert_eq!(
            context.handle(request("GET", "/api/timeline/task")).status,
            200
        );
        assert_eq!(
            context.handle(request("GET", "/api/timeline/%zz")).status,
            400
        );
        assert_eq!(context.handle(request("POST", "/")).status, 405);
        assert_eq!(context.handle(request("GET", "/missing")).status, 404);
        let artifact = context.handle(request("GET", "/artifact/data/task/plan.html"));
        assert_eq!(artifact.status, 200);
        assert!(
            artifact
                .headers
                .iter()
                .any(|(name, value)| { name == "X-Frame-Options" && value == "SAMEORIGIN" })
        );
        assert_eq!(
            context
                .handle(request("GET", "/artifact/data/task/missing"))
                .status,
            404
        );
        assert_eq!(
            context
                .handle(request("GET", "/artifact/other/file"))
                .status,
            403
        );
        assert_eq!(
            context.handle(request("GET", "/artifact/data/%zz")).status,
            403
        );

        let outside = temp.path().join("outside");
        fs::write(&outside, "outside").expect("outside");
        let link = temp.path().join("data/link");
        std::os::unix::fs::symlink(&outside, &link).expect("link");
        assert_eq!(
            context.handle(request("GET", "/artifact/data/link")).status,
            403
        );
    }

    #[test]
    fn dashboard_file_and_json_error_paths_return_bounded_service_errors() {
        let temp = tempfile::tempdir().expect("tempdir");
        for directory in ["share/viz", "state", "data", "docs"] {
            fs::create_dir_all(temp.path().join(directory)).expect("directory");
        }
        let command = temp.path().join("reader.sh");
        script(&command, "printf bad; printf detail >&2; exit 3");
        let root = fs::canonicalize(temp.path()).expect("root");
        let context = context(root, command);
        assert_eq!(context.handle(request("GET", "/api/state")).status, 503);
        assert_eq!(context.handle(request("GET", "/api/doctor")).status, 503);
        assert_eq!(
            context.handle(request("GET", "/api/timeline/task")).status,
            503
        );
        assert_eq!(context.handle(request("GET", "/")).status, 503);
        assert_eq!(
            context.handle(request("GET", "/assets/app.css")).status,
            503
        );
        script(&context.snapshot_command, "printf bad");
        assert_eq!(context.handle(request("GET", "/api/state")).status, 503);
        script(&context.snapshot_command, "exit 0");
        assert_eq!(context.handle(request("GET", "/api/state")).status, 503);
        script(&context.snapshot_command, "printf '\\377'");
        assert_eq!(context.handle(request("GET", "/api/state")).status, 503);
        script(&context.timeline_command, "printf bad");
        assert_eq!(
            context.handle(request("GET", "/api/timeline/task")).status,
            503
        );
        script(&context.timeline_command, "exit 4");
        assert_eq!(
            context.handle(request("GET", "/api/timeline/task")).status,
            503
        );

        let invalid = temp.path().join("invalid.html");
        fs::write(&invalid, [0xff]).expect("invalid");
        assert!(response_file(&invalid, Some(&|value| value)).is_err());
        assert!(response_file(temp.path(), None).is_err());
    }
}

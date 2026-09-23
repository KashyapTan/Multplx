//! Disposable GET-only dashboard lifecycle and cached snapshot service.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
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
    content_hash: String,
    snapshot_hash: String,
    refreshed_at: Instant,
    refreshed_at_wall: String,
    generated: Option<String>,
    artifact_refs: BTreeMap<String, ArtifactRef>,
}

#[derive(Clone, Debug)]
struct ArtifactRef {
    allowed_root: PathBuf,
    file: PathBuf,
}

#[derive(Clone, Debug, Default)]
struct RefreshMetrics {
    state_requests: u64,
    fresh_hits: u64,
    stale_serves: u64,
    initial_unavailable: u64,
    refresh_attempts: u64,
    refresh_successes: u64,
    refresh_failures: u64,
    not_modified: u64,
    last_refresh_ms: Option<u64>,
    max_refresh_ms: u64,
}

#[derive(Debug)]
struct Runtime {
    last_request: Instant,
    last_request_at: String,
    last_poll_at: Option<String>,
    cache: Option<SnapshotCache>,
    refresh_in_flight: bool,
    last_refresh_attempt: Option<Instant>,
    last_refresh_started_at: Option<String>,
    last_refresh_finished_at: Option<String>,
    last_refresh_error: Option<String>,
    metrics: RefreshMetrics,
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
    command_timeout: Duration,
    snapshot_command: PathBuf,
    doctor_command: PathBuf,
    timeline_command: PathBuf,
    runtime: Mutex<Runtime>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CacheState {
    Fresh,
    Stale,
}

#[derive(Clone, Debug)]
struct SnapshotAccess {
    cache: SnapshotCache,
    cache_state: CacheState,
    refresh_state: &'static str,
    refresh_error: Option<String>,
}

fn with_refresh_error_header(mut response: Response, error: Option<&str>) -> Response {
    let Some(error) = error else {
        return response;
    };
    let value = error
        .chars()
        .take(256)
        .map(|character| {
            if character.is_ascii_graphic() || character == ' ' {
                character
            } else {
                ' '
            }
        })
        .collect::<String>();
    response = response.header("X-Multplx-Refresh-Error", value);
    response
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
        "source_path":file,
        "label":label,
        "kind":kind,
        "url":format!("/artifact/{root_name}/{encoded}")
    }))
}

fn artifact_ref_entry(
    allowed_roots: &[PathBuf],
    file: &Path,
    label: String,
    kind: &str,
) -> Option<(String, Value, ArtifactRef)> {
    let real_file = fs::canonicalize(file).ok()?;
    let metadata = fs::metadata(&real_file).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
        return None;
    }
    let allowed_root = allowed_roots.iter().find_map(|root| {
        let canonical = fs::canonicalize(root).ok()?;
        (is_within(&canonical, &real_file)).then_some(canonical)
    })?;
    let token = sha256_hex(real_file.as_os_str().as_encoded_bytes());
    let entry = json!({
        "root":"ref",
        "path":real_file.strip_prefix(&allowed_root).ok()?.to_string_lossy(),
        "source_path":file,
        "label":label,
        "kind":kind,
        "url":format!("/artifact/ref/{token}")
    });
    Some((
        token,
        entry,
        ArtifactRef {
            allowed_root,
            file: real_file,
        },
    ))
}

fn artifact_owner_roots(
    home: &Path,
    state: &Path,
    snapshot: &Value,
    owner: &str,
) -> Option<Vec<PathBuf>> {
    fn safe_root(owner: &Path, base: &Path, relative: &str) -> Option<PathBuf> {
        let base = fs::canonicalize(base).ok()?;
        if !is_within(owner, &base) {
            return None;
        }
        let candidate = fs::canonicalize(base.join(relative)).ok()?;
        is_within(&base, &candidate).then_some(candidate)
    }
    let owner = fs::canonicalize(owner).ok()?;
    if owner == fs::canonicalize(home).ok()? {
        return Some(
            [
                safe_root(&owner, home, "data"),
                safe_root(&owner, state, "brief-revisions"),
            ]
            .into_iter()
            .flatten()
            .collect(),
        );
    }
    let records = snapshot
        .pointer("/daemon_current/records")
        .and_then(Value::as_array);
    let direct = records.into_iter().flatten().find_map(|record| {
        if record["valid"] != true
            || record
                .pointer("/provenance/selected")
                .and_then(Value::as_str)
                != Some("structured-home")
        {
            return None;
        }
        let candidate = record.get("home").and_then(Value::as_str)?;
        fs::canonicalize(candidate)
            .ok()
            .filter(|candidate| candidate == &owner)
    });
    let nested = snapshot
        .pointer("/domains/records")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|domain| {
            let candidate = domain
                .pointer("/coordinator/validated_home")
                .and_then(Value::as_str)?;
            fs::canonicalize(candidate)
                .ok()
                .filter(|candidate| candidate == &owner)
        });
    let child_home = direct.or(nested)?;
    let state = child_home.join("state");
    Some(
        [
            safe_root(&child_home, &child_home, "data"),
            safe_root(&child_home, &state, "brief-revisions"),
        ]
        .into_iter()
        .flatten()
        .collect(),
    )
}

fn collect_artifacts(root: &Path, snapshot: &Value) -> Vec<Value> {
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut add_path = |file: &Path, label: String, kind: &str| {
        for root_name in ["data", "docs"] {
            if let Some(entry) = artifact_entry(root, root_name, file, label.clone(), kind)
                && let Some(url) = entry.get("url").and_then(Value::as_str)
                && seen.insert(url.to_owned())
            {
                entries.push(entry);
                break;
            }
        }
    };
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
                add_path(&file, format!("{id}/{name}"), kind);
            }
        }
    }
    if let Some(reports) = snapshot.get("scout_reports").and_then(Value::as_array) {
        for report in reports {
            let Some(path) = report.get("path").and_then(Value::as_str) else {
                continue;
            };
            let id = report.get("id").and_then(Value::as_str).unwrap_or_default();
            add_path(Path::new(path), format!("{id}/report.md"), "report");
        }
    }
    let mut tasks = snapshot
        .pointer("/portfolio/tasks")
        .and_then(Value::as_array)
        .map(|tasks| tasks.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(domains) = snapshot
        .pointer("/domains/records")
        .and_then(Value::as_array)
    {
        for domain in domains {
            if let Some(child_tasks) = domain.pointer("/portfolio/tasks").and_then(Value::as_array)
            {
                tasks.extend(child_tasks);
            }
        }
    }
    if let Some(children) = snapshot
        .pointer("/daemon_current/records")
        .and_then(Value::as_array)
    {
        for child in children {
            if let Some(child_tasks) = child.pointer("/portfolio/tasks").and_then(Value::as_array) {
                tasks.extend(child_tasks);
            }
        }
    }
    for task in tasks {
        let id = task.get("id").and_then(Value::as_str).unwrap_or("task");
        if let Some(path) = task.pointer("/brief/path").and_then(Value::as_str) {
            add_path(Path::new(path), format!("{id}/brief"), "brief");
        }
        if let Some(paths) = task.pointer("/brief/research").and_then(Value::as_array) {
            for (index, path) in paths.iter().filter_map(Value::as_str).enumerate() {
                add_path(
                    Path::new(path),
                    format!("{id}/research-{}", index + 1),
                    "research",
                );
            }
        }
        if let Some(path) = task
            .pointer("/evidence/report/path")
            .and_then(Value::as_str)
        {
            add_path(Path::new(path), format!("{id}/report"), "report");
        }
    }
    entries
}

fn collect_cached_artifacts(
    root: &Path,
    home: &Path,
    state: &Path,
    snapshot: &Value,
) -> (Vec<Value>, BTreeMap<String, ArtifactRef>) {
    let mut entries = collect_artifacts(root, snapshot);
    let mut seen = entries
        .iter()
        .filter_map(|entry| entry.get("source_path").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut references = BTreeMap::new();
    let mut add = |allowed_roots: &[PathBuf], path: &str, label: String, kind: &str| {
        if path.starts_with("/artifact/") || !Path::new(path).is_absolute() {
            return;
        }
        let Some((token, entry, reference)) =
            artifact_ref_entry(allowed_roots, Path::new(path), label, kind)
        else {
            return;
        };
        let Some(source_path) = entry.get("source_path").and_then(Value::as_str) else {
            return;
        };
        if seen.insert(source_path.to_owned()) {
            references.insert(token, reference);
            entries.push(entry);
        }
    };
    let mut tasks = snapshot
        .pointer("/portfolio/tasks")
        .and_then(Value::as_array)
        .map(|tasks| tasks.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(domains) = snapshot
        .pointer("/domains/records")
        .and_then(Value::as_array)
    {
        for domain in domains {
            if let Some(child_tasks) = domain.pointer("/portfolio/tasks").and_then(Value::as_array)
            {
                tasks.extend(child_tasks);
            }
        }
    }
    if let Some(children) = snapshot
        .pointer("/daemon_current/records")
        .and_then(Value::as_array)
    {
        for child in children {
            if let Some(child_tasks) = child.pointer("/portfolio/tasks").and_then(Value::as_array) {
                tasks.extend(child_tasks);
            }
        }
    }
    for task in tasks {
        let id = task.get("id").and_then(Value::as_str).unwrap_or("task");
        let Some(allowed_roots) = task
            .pointer("/owner/home")
            .and_then(Value::as_str)
            .and_then(|owner| artifact_owner_roots(home, state, snapshot, owner))
        else {
            continue;
        };
        if let Some(path) = task.pointer("/brief/path").and_then(Value::as_str) {
            add(&allowed_roots, path, format!("{id}/brief"), "brief");
        }
        if let Some(paths) = task.pointer("/brief/research").and_then(Value::as_array) {
            for (index, path) in paths.iter().filter_map(Value::as_str).enumerate() {
                add(
                    &allowed_roots,
                    path,
                    format!("{id}/research-{}", index + 1),
                    "research",
                );
            }
        }
        let report = task.pointer("/evidence/report");
        let report_path = report.and_then(Value::as_str).or_else(|| {
            report
                .and_then(|value| value.get("path"))
                .and_then(Value::as_str)
        });
        if let Some(path) = report_path {
            add(&allowed_roots, path, format!("{id}/report"), "report");
        }
    }
    (entries, references)
}

fn meaningful_value(value: &mut Value, freshness_object: bool) {
    match value {
        Value::Object(fields) => {
            let has_identity = ["event_id", "observation_id", "delivery_id", "decision_id"]
                .iter()
                .any(|name| fields.contains_key(*name));
            if !has_identity {
                fields.remove("generated");
            }
            if freshness_object || (fields.contains_key("freshness") && !has_identity) {
                for name in ["observed_at", "age_seconds", "age_secs"] {
                    fields.remove(name);
                }
            }
            for (name, child) in fields.iter_mut() {
                meaningful_value(
                    child,
                    matches!(name.as_str(), "freshness" | "latest_change"),
                );
            }
        }
        Value::Array(values) => {
            for child in values {
                meaningful_value(child, false);
            }
        }
        _ => {}
    }
}

fn unavailable_error(runtime: &Runtime) -> ServiceError {
    if let Some(error) = &runtime.last_refresh_error {
        ServiceError::new(format!("snapshot refresh failed: {error}"))
    } else {
        ServiceError::new("snapshot refresh is in flight")
    }
}

impl ServerContext {
    fn refresh_snapshot_once(&self) -> Result<SnapshotCache> {
        let (raw, snapshot) = execute_json(
            &self.snapshot_command,
            &["--json"],
            self,
            &[0],
            self.command_timeout,
        )?;
        let (artifacts, artifact_refs) =
            collect_cached_artifacts(&self.root, &self.home, &self.state, &snapshot);
        let server = json!({"version":VERSION,"started":self.started,"pid":std::process::id()});
        let body = format!(
            "{{\"server\":{},\"artifacts\":{},\"snapshot\":{raw}}}\n",
            serde_json::to_string(&server).expect("server JSON"),
            serde_json::to_string(&artifacts).expect("artifact JSON")
        )
        .into_bytes();
        let mut meaningful_snapshot = snapshot.clone();
        meaningful_value(&mut meaningful_snapshot, false);
        let meaningful = json!({"artifacts":artifacts,"snapshot":meaningful_snapshot});
        Ok(SnapshotCache {
            content_hash: sha256_hex(
                serde_json::to_vec(&meaningful).expect("meaningful snapshot JSON"),
            ),
            snapshot_hash: sha256_hex(raw.as_bytes()),
            body,
            refreshed_at: Instant::now(),
            refreshed_at_wall: utc_now(),
            generated: snapshot
                .get("generated")
                .and_then(Value::as_str)
                .map(str::to_owned),
            artifact_refs,
        })
    }

    fn finish_refresh(&self, started: Instant, result: &Result<SnapshotCache>) {
        let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let mut runtime = self.runtime.lock().expect("runtime");
        runtime.refresh_in_flight = false;
        runtime.last_refresh_attempt = Some(Instant::now());
        runtime.last_refresh_finished_at = Some(utc_now());
        runtime.metrics.last_refresh_ms = Some(elapsed_ms);
        runtime.metrics.max_refresh_ms = runtime.metrics.max_refresh_ms.max(elapsed_ms);
        match result {
            Ok(cache) => {
                runtime.cache = Some(cache.clone());
                runtime.last_refresh_error = None;
                runtime.metrics.refresh_successes += 1;
            }
            Err(error) => {
                runtime.last_refresh_error = Some(error.message.clone());
                runtime.metrics.refresh_failures += 1;
            }
        }
    }

    fn refresh_claimed(&self) -> Result<SnapshotCache> {
        let started = Instant::now();
        let result = self.refresh_snapshot_once();
        self.finish_refresh(started, &result);
        result
    }

    fn begin_refresh_locked(runtime: &mut Runtime) {
        runtime.refresh_in_flight = true;
        runtime.last_refresh_attempt = Some(Instant::now());
        runtime.last_refresh_started_at = Some(utc_now());
        runtime.metrics.refresh_attempts += 1;
    }

    fn snapshot(self: &Arc<Self>) -> Result<SnapshotAccess> {
        let mut runtime = self.runtime.lock().expect("runtime");
        runtime.metrics.state_requests += 1;
        if let Some(cache) = runtime.cache.clone() {
            if cache.refreshed_at.elapsed() < self.refresh {
                runtime.metrics.fresh_hits += 1;
                return Ok(SnapshotAccess {
                    cache,
                    cache_state: CacheState::Fresh,
                    refresh_state: if runtime.refresh_in_flight {
                        "in-flight"
                    } else {
                        "idle"
                    },
                    refresh_error: None,
                });
            }
            runtime.metrics.stale_serves += 1;
            let retry_ready = runtime
                .last_refresh_attempt
                .is_none_or(|attempt| attempt.elapsed() >= self.refresh);
            if !runtime.refresh_in_flight && retry_ready {
                Self::begin_refresh_locked(&mut runtime);
                let context = Arc::clone(self);
                thread::spawn(move || {
                    let _ = context.refresh_claimed();
                });
                return Ok(SnapshotAccess {
                    cache,
                    cache_state: CacheState::Stale,
                    refresh_state: "in-flight",
                    refresh_error: None,
                });
            }
            let refresh_error = runtime.last_refresh_error.clone();
            return Ok(SnapshotAccess {
                cache,
                cache_state: CacheState::Stale,
                refresh_state: if runtime.refresh_in_flight {
                    "in-flight"
                } else if runtime.last_refresh_error.is_some() {
                    "failed"
                } else {
                    "idle"
                },
                refresh_error,
            });
        }
        if runtime.refresh_in_flight {
            runtime.metrics.initial_unavailable += 1;
            return Err(unavailable_error(&runtime));
        }
        if runtime.last_refresh_error.is_some()
            && runtime
                .last_refresh_attempt
                .is_some_and(|attempt| attempt.elapsed() < self.refresh)
        {
            runtime.metrics.initial_unavailable += 1;
            return Err(unavailable_error(&runtime));
        }
        Self::begin_refresh_locked(&mut runtime);
        drop(runtime);
        self.refresh_claimed().map(|cache| SnapshotAccess {
            cache,
            cache_state: CacheState::Fresh,
            refresh_state: "idle",
            refresh_error: None,
        })
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
        if root_name == "ref" {
            if segments.len() != 2
                || segments[1].len() != 64
                || !segments[1].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(ServiceError::new("forbidden"));
            }
            let reference = self
                .runtime
                .lock()
                .expect("runtime")
                .cache
                .as_ref()
                .and_then(|cache| cache.artifact_refs.get(segments[1]))
                .cloned()
                .ok_or_else(|| ServiceError::new("not found"))?;
            let allowed_root = fs::canonicalize(&reference.allowed_root)
                .map_err(|_| ServiceError::new("forbidden"))?;
            let file =
                fs::canonicalize(&reference.file).map_err(|_| ServiceError::new("not found"))?;
            if !is_within(&allowed_root, &file) {
                return Err(ServiceError::new("forbidden"));
            }
            let mut response = common_headers(response_file(&file, None)?);
            artifact_headers(&mut response);
            return Ok(response);
        }
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

    fn handle(self: &Arc<Self>, request: Request) -> Response {
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
            "/assets/app.js" | "/assets/app.css" | "/assets/agents-graph.js" => common_headers(
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
                        "snapshot_refreshed_at":runtime.cache.as_ref().map(|cache| cache.refreshed_at_wall.clone()),
                        "snapshot_age_ms":runtime.cache.as_ref().map(|cache| cache.refreshed_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64),
                        "content_hash":runtime.cache.as_ref().map(|cache| cache.content_hash.clone()),
                        "refresh":{
                            "state":if runtime.refresh_in_flight {"in-flight"} else if runtime.last_refresh_error.is_some() {"failed"} else {"idle"},
                            "started_at":runtime.last_refresh_started_at,
                            "finished_at":runtime.last_refresh_finished_at,
                            "error":runtime.last_refresh_error,
                        },
                        "metrics":{
                            "state_requests":runtime.metrics.state_requests,
                            "fresh_hits":runtime.metrics.fresh_hits,
                            "stale_serves":runtime.metrics.stale_serves,
                            "initial_unavailable":runtime.metrics.initial_unavailable,
                            "refresh_attempts":runtime.metrics.refresh_attempts,
                            "refresh_successes":runtime.metrics.refresh_successes,
                            "refresh_failures":runtime.metrics.refresh_failures,
                            "not_modified":runtime.metrics.not_modified,
                            "last_refresh_ms":runtime.metrics.last_refresh_ms,
                            "max_refresh_ms":runtime.metrics.max_refresh_ms,
                        }
                    }),
                ))
            }
            "/api/state" => {
                self.runtime.lock().expect("runtime").last_poll_at = Some(utc_now());
                match self.snapshot() {
                    Ok(access) => {
                        let etag = format!("\"{}\"", access.cache.content_hash);
                        let cache_state = match access.cache_state {
                            CacheState::Fresh => "fresh",
                            CacheState::Stale => "stale",
                        };
                        let age_ms = access
                            .cache
                            .refreshed_at
                            .elapsed()
                            .as_millis()
                            .min(u128::from(u64::MAX))
                            .to_string();
                        if request.headers.get("if-none-match").is_some_and(|value| {
                            value == &etag || value == &access.cache.content_hash
                        }) {
                            self.runtime.lock().expect("runtime").metrics.not_modified += 1;
                            return with_refresh_error_header(
                                common_headers(
                                    Response::new(304, Vec::new())
                                        .header("ETag", etag)
                                        .header("X-Multplx-Content-Hash", access.cache.content_hash)
                                        .header(
                                            "X-Multplx-Snapshot-Hash",
                                            access.cache.snapshot_hash,
                                        )
                                        .header("X-Multplx-Observation-Age-Ms", age_ms)
                                        .header("X-Multplx-Cache", cache_state)
                                        .header("X-Multplx-Refresh", access.refresh_state),
                                ),
                                access.refresh_error.as_deref(),
                            );
                        }
                        with_refresh_error_header(
                            common_headers(
                                Response::new(200, access.cache.body)
                                    .header("Content-Type", "application/json; charset=utf-8")
                                    .header("ETag", etag)
                                    .header("X-Multplx-Content-Hash", access.cache.content_hash)
                                    .header("X-Multplx-Snapshot-Hash", access.cache.snapshot_hash)
                                    .header("X-Multplx-Observation-Age-Ms", age_ms)
                                    .header("X-Multplx-Cache", cache_state)
                                    .header("X-Multplx-Refresh", access.refresh_state),
                            ),
                            access.refresh_error.as_deref(),
                        )
                    }
                    Err(error) => {
                        let runtime = self.runtime.lock().expect("runtime");
                        with_refresh_error_header(common_headers(Response::json(503, &json!({
                            "error":error.message,
                            "cache":"unavailable",
                            "refresh":{
                                "state":if runtime.refresh_in_flight {"in-flight"} else {"failed"},
                                "error":runtime.last_refresh_error,
                            }
                        })).header("X-Multplx-Cache", "unavailable")
                            .header("X-Multplx-Refresh", if runtime.refresh_in_flight {"in-flight"} else {"failed"})), runtime.last_refresh_error.as_deref())
                    }
                }
            }
            "/api/doctor" => match execute_json(
                &self.doctor_command,
                &["--json"],
                self,
                &[0, 1, 2],
                self.command_timeout,
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
                    self.command_timeout,
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

fn handle_connection(mut stream: TcpStream, context: &Arc<ServerContext>) {
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
    let command_timeout = Duration::from_millis(parse_integer_env(
        "MX_VIZ_COMMAND_TIMEOUT_MS",
        10_000,
        50,
        60_000,
    )?);
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
        command_timeout,
        snapshot_command,
        doctor_command,
        timeline_command,
        runtime: Mutex::new(Runtime {
            last_request: Instant::now(),
            last_request_at: started,
            last_poll_at: None,
            cache: None,
            refresh_in_flight: false,
            last_refresh_attempt: None,
            last_refresh_started_at: None,
            last_refresh_finished_at: None,
            last_refresh_error: None,
            metrics: RefreshMetrics::default(),
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
    use super::{
        RefreshMetrics, Runtime, ServerContext, artifact_entry, collect_artifacts,
        collect_cached_artifacts, meaningful_value, response_file, with_refresh_error_header,
    };
    use crate::http::{Request, Response};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
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

    fn context_with(
        root: PathBuf,
        command: PathBuf,
        refresh: Duration,
        command_timeout: Duration,
    ) -> Arc<ServerContext> {
        let started = "2026-08-12T12:00:00Z".to_owned();
        Arc::new(ServerContext {
            asset_directory: root.join("share/viz"),
            home: root.clone(),
            state: root.join("state"),
            root,
            started: started.clone(),
            port: 4890,
            poll_ms: 123,
            refresh,
            command_timeout,
            snapshot_command: command.clone(),
            doctor_command: command.clone(),
            timeline_command: command,
            runtime: Mutex::new(Runtime {
                last_request: Instant::now(),
                last_request_at: started,
                last_poll_at: None,
                cache: None,
                refresh_in_flight: false,
                last_refresh_attempt: None,
                last_refresh_started_at: None,
                last_refresh_finished_at: None,
                last_refresh_error: None,
                metrics: RefreshMetrics::default(),
            }),
        })
    }

    fn context(root: PathBuf, command: PathBuf) -> Arc<ServerContext> {
        context_with(
            root,
            command,
            Duration::from_secs(60),
            Duration::from_secs(1),
        )
    }

    fn header<'a>(response: &'a crate::http::Response, name: &str) -> Option<&'a str> {
        response
            .headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn artifact_collection_accepts_only_canonical_allowlisted_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("data/task")).expect("data");
        fs::create_dir(temp.path().join("docs")).expect("docs");
        fs::write(temp.path().join("data/task/plan.html"), "plan").expect("plan");
        fs::write(temp.path().join("data/task/report.md"), "report").expect("report");
        fs::write(temp.path().join("docs/research.md"), "research").expect("research");
        fs::write(temp.path().join("private.md"), "private").expect("private");
        let snapshot = json!({
            "roots":{"data":temp.path().join("data")},
            "tasks":[{"id":"task"}],
            "scout_reports":[],
            "portfolio":{"tasks":[{
                "id":"task",
                "brief":{"path":temp.path().join("data/task/plan.html"),"research":[
                    temp.path().join("docs/research.md"),
                    temp.path().join("private.md")
                ]},
                "evidence":{"report":{"path":temp.path().join("data/task/report.md")}}
            }]}
        });
        let root = fs::canonicalize(temp.path()).expect("canonical root");
        let artifacts = collect_artifacts(&root, &snapshot);
        assert_eq!(artifacts.len(), 3);
        assert_eq!(artifacts[0]["url"], "/artifact/data/task/plan.html");
        assert!(
            artifacts
                .iter()
                .any(|entry| entry["url"] == "/artifact/docs/research.md")
        );
        assert!(artifacts.iter().all(|entry| entry["path"] != "private.md"));
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
    fn cached_artifact_refs_allow_only_snapshot_linked_home_and_valid_child_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let child = temp.path().join("child");
        let nested = temp.path().join("nested");
        for directory in [
            home.join("data/task"),
            home.join("state/brief-revisions"),
            child.join("data/task"),
            child.join("state/brief-revisions"),
            nested.join("data/task"),
            nested.join("state/brief-revisions"),
            temp.path().join("root/data"),
            temp.path().join("root/docs"),
        ] {
            fs::create_dir_all(directory).expect("directory");
        }
        let brief = home.join("state/brief-revisions/task-2.md");
        let report = home.join("data/task/report.md");
        let child_report = child.join("data/task/report.md");
        let nested_report = nested.join("data/task/report.md");
        let cross_report = child.join("data/task/cross.md");
        let outside = temp.path().join("outside.md");
        for path in [
            &brief,
            &report,
            &child_report,
            &nested_report,
            &cross_report,
            &outside,
        ] {
            fs::write(path, path.display().to_string()).expect("artifact");
        }
        let snapshot = json!({
            "roots":{"data":home.join("data")},
            "tasks":[],"scout_reports":[],
            "portfolio":{"tasks":[
                {"id":"task","owner":{"home":home},"brief":{"path":brief,"research":[outside]},"evidence":{"report":{"path":report}}},
                {"id":"child","owner":{"home":child},"brief":{"path":Value::Null,"research":[]},"evidence":{"report":{"path":child_report}}},
                {"id":"cross-owner","owner":{"home":home},"brief":{"path":Value::Null,"research":[]},"evidence":{"report":{"path":cross_report}}}
            ]},
            "daemon_current":{"records":[{"valid":true,"home":child,"provenance":{"selected":"structured-home"}}]},
            "domains":{"records":[{"coordinator":{"validated_home":nested},"portfolio":{"tasks":[
                {"id":"nested","owner":{"home":nested},"brief":{"path":Value::Null,"research":[]},"evidence":{"report":{"path":nested_report}}}
            ]}}]}
        });
        let (entries, references) = collect_cached_artifacts(
            &temp.path().join("root"),
            &home,
            &home.join("state"),
            &snapshot,
        );
        assert_eq!(entries.len(), 4);
        assert_eq!(references.len(), 4);
        assert!(entries.iter().all(|entry| entry["root"] == "ref"));
        assert!(entries.iter().all(|entry| {
            entry["url"]
                .as_str()
                .is_some_and(|url| url.starts_with("/artifact/ref/"))
        }));
        assert!(entries.iter().all(|entry| {
            entry["source_path"].as_str() != Some(outside.to_string_lossy().as_ref())
        }));
        assert!(
            entries.iter().all(|entry| entry["source_path"].as_str()
                != Some(cross_report.to_string_lossy().as_ref()))
        );

        let escaped_home = temp.path().join("escaped-home");
        let escaped_target = temp.path().join("escaped-target");
        fs::create_dir_all(escaped_home.join("state/brief-revisions")).expect("escaped state");
        fs::create_dir(&escaped_target).expect("escaped target");
        let secret = escaped_target.join("secret.md");
        fs::write(&secret, "secret").expect("secret");
        std::os::unix::fs::symlink(&escaped_target, escaped_home.join("data"))
            .expect("escaped data root");
        let escaped_snapshot = json!({
            "portfolio":{"tasks":[{"id":"escape","owner":{"home":escaped_home},"brief":{"path":Value::Null,"research":[]},"evidence":{"report":{"path":secret}}}]}
        });
        let (_, escaped_refs) = collect_cached_artifacts(
            &temp.path().join("root"),
            &escaped_home,
            &escaped_home.join("state"),
            &escaped_snapshot,
        );
        assert!(escaped_refs.is_empty());
    }

    #[test]
    fn direct_dashboard_routes_cover_cache_artifacts_and_command_failures() {
        let temp = tempfile::tempdir().expect("tempdir");
        for directory in ["share/viz", "state", "data/task", "docs", "child/data/task"] {
            fs::create_dir_all(temp.path().join(directory)).expect("directory");
        }
        fs::write(
            temp.path().join("share/viz/index.html"),
            "<meta content=\"__MX_VIZ_POLL_MS__\">",
        )
        .expect("index");
        fs::write(temp.path().join("share/viz/app.js"), "js").expect("js");
        fs::write(temp.path().join("share/viz/app.css"), "css").expect("css");
        fs::write(temp.path().join("share/viz/agents-graph.js"), "graph js").expect("graph js");
        fs::write(temp.path().join("data/task/plan.html"), "plan").expect("artifact");
        fs::write(
            temp.path().join("child/data/task/report.md"),
            "child report",
        )
        .expect("child artifact");
        let command = temp.path().join("reader.sh");
        script(
            &command,
            &format!(
                "printf '%s\\n' '{{\"generated\":\"now\",\"roots\":{{\"data\":\"{}\"}},\"tasks\":[{{\"id\":\"task\"}}],\"scout_reports\":[],\"portfolio\":{{\"tasks\":[{{\"id\":\"child\",\"owner\":{{\"home\":\"{}\"}},\"brief\":{{\"path\":null,\"research\":[]}},\"evidence\":{{\"report\":{{\"path\":\"{}\"}}}}}}]}},\"daemon_current\":{{\"records\":[{{\"valid\":true,\"home\":\"{}\",\"provenance\":{{\"selected\":\"structured-home\"}}}}]}}}}'",
                temp.path().join("data").display(),
                temp.path().join("child").display(),
                temp.path().join("child/data/task/report.md").display(),
                temp.path().join("child").display()
            ),
        );
        let root = fs::canonicalize(temp.path()).expect("root");
        let context = context(root.clone(), command.clone());
        assert_eq!(context.handle(request("GET", "/")).status, 200);
        assert_eq!(context.handle(request("GET", "/assets/app.js")).status, 200);
        assert_eq!(
            context
                .handle(request("GET", "/assets/agents-graph.js"))
                .status,
            200
        );
        let state = context.handle(request("GET", "/api/state"));
        assert_eq!(state.status, 200);
        let envelope: Value = serde_json::from_slice(&state.body).expect("state JSON");
        let reference_url = envelope["artifacts"]
            .as_array()
            .expect("artifacts")
            .iter()
            .find(|entry| entry["root"] == "ref")
            .and_then(|entry| entry["url"].as_str())
            .expect("opaque artifact URL")
            .to_owned();
        assert_eq!(context.handle(request("GET", &reference_url)).status, 200);
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
        fs::remove_file(temp.path().join("child/data/task/report.md"))
            .expect("remove child report");
        std::os::unix::fs::symlink(
            temp.path().join("outside"),
            temp.path().join("child/data/task/report.md"),
        )
        .expect("replace child report");
        assert_eq!(context.handle(request("GET", &reference_url)).status, 403);
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
        assert_eq!(
            context
                .runtime
                .lock()
                .expect("runtime")
                .metrics
                .refresh_attempts,
            1,
            "failed initial refreshes must respect the retry interval",
        );
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

    #[test]
    fn refresh_error_header_is_single_line_and_bounded() {
        let error = format!("bad\r\nreader\t{}", "x".repeat(300));
        let response = with_refresh_error_header(Response::new(200, Vec::new()), Some(&error));
        let value = header(&response, "X-Multplx-Refresh-Error").expect("refresh error");
        assert_eq!(value.chars().count(), 256);
        assert!(!value.chars().any(char::is_control));
        assert!(value.starts_with("bad  reader "));
    }

    #[test]
    fn meaningful_hash_input_excludes_freshness_but_keeps_identified_event_time() {
        let mut first = json!({
            "generated":"one",
            "portfolio":{
                "generated":"one",
                "observed_at":"one",
                "freshness":{"status":"fresh","age_seconds":0,"observed_at":"one"},
                "tasks":[{"latest_change":{"state":"working","observed_at":"one"}}]
            },
            "event":{"event_id":"event-1","generated":"one","observed_at":"one"}
        });
        let mut age_only = json!({
            "generated":"two",
            "portfolio":{
                "generated":"two",
                "observed_at":"two",
                "freshness":{"status":"fresh","age_seconds":9,"observed_at":"two"},
                "tasks":[{"latest_change":{"state":"working","observed_at":"two"}}]
            },
            "event":{"event_id":"event-1","generated":"one","observed_at":"one"}
        });
        meaningful_value(&mut first, false);
        meaningful_value(&mut age_only, false);
        assert_eq!(first, age_only);

        let mut later_event = age_only.clone();
        later_event["event"]["generated"] = json!("three");
        later_event["event"]["observed_at"] = json!("three");
        assert_ne!(first, later_event);
    }

    #[test]
    fn snapshot_refresh_is_single_flight_and_does_not_hold_runtime_mutex() {
        let temp = tempfile::tempdir().expect("tempdir");
        for directory in ["share/viz", "state", "data", "docs"] {
            fs::create_dir_all(temp.path().join(directory)).expect("directory");
        }
        let count = temp.path().join("count");
        let command = temp.path().join("reader.sh");
        script(
            &command,
            &format!(
                "count=$(cat '{}' 2>/dev/null || printf 0); printf '%s' $((count + 1)) > '{}'; sleep 0.2; printf '%s\\n' '{{\"generated\":\"now\",\"roots\":{{\"data\":\"{}\"}},\"tasks\":[],\"scout_reports\":[]}}'",
                count.display(),
                count.display(),
                temp.path().join("data").display()
            ),
        );
        let root = fs::canonicalize(temp.path()).expect("root");
        let context = context_with(
            root,
            command,
            Duration::from_millis(10),
            Duration::from_secs(1),
        );
        let owner_context = Arc::clone(&context);
        let owner = std::thread::spawn(move || owner_context.handle(request("GET", "/api/state")));
        let wait_started = Instant::now();
        while !count.exists() && wait_started.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(count.exists(), "reader did not start");
        let started = Instant::now();
        for _ in 0..8 {
            let response = context.handle(request("GET", "/api/state"));
            assert_eq!(response.status, 503);
            assert_eq!(header(&response, "X-Multplx-Cache"), Some("unavailable"));
            assert_eq!(header(&response, "X-Multplx-Refresh"), Some("in-flight"));
        }
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(context.handle(request("GET", "/api/meta")).status, 200);
        assert_eq!(owner.join().expect("owner response").status, 200);
        assert_eq!(fs::read_to_string(count).expect("count"), "1");
        let runtime = context.runtime.lock().expect("runtime");
        assert_eq!(runtime.metrics.refresh_attempts, 1);
        assert_eq!(runtime.metrics.refresh_successes, 1);
        assert_eq!(runtime.metrics.initial_unavailable, 8);
    }

    #[test]
    fn stale_snapshot_is_served_while_one_background_refresh_runs() {
        let temp = tempfile::tempdir().expect("tempdir");
        for directory in ["share/viz", "state", "data", "docs"] {
            fs::create_dir_all(temp.path().join(directory)).expect("directory");
        }
        let command = temp.path().join("reader.sh");
        script(
            &command,
            &format!(
                "printf '%s\\n' '{{\"generated\":\"one\",\"marker\":\"alpha\",\"roots\":{{\"data\":\"{}\"}},\"tasks\":[],\"scout_reports\":[]}}'",
                temp.path().join("data").display()
            ),
        );
        let root = fs::canonicalize(temp.path()).expect("root");
        let context = context_with(
            root,
            command.clone(),
            Duration::from_secs(1),
            Duration::from_secs(1),
        );
        let first = context.handle(request("GET", "/api/state"));
        assert_eq!(first.status, 200);
        let first_etag = header(&first, "ETag").expect("etag").to_owned();
        let first_snapshot_hash = header(&first, "X-Multplx-Snapshot-Hash")
            .expect("snapshot hash")
            .to_owned();
        {
            let stale_instant = Instant::now()
                .checked_sub(Duration::from_secs(2))
                .expect("stale instant");
            let mut runtime = context.runtime.lock().expect("runtime");
            runtime.cache.as_mut().expect("cache").refreshed_at = stale_instant;
            runtime.last_refresh_attempt = Some(stale_instant);
        }
        script(
            &command,
            &format!(
                "sleep 0.2; printf '%s\\n' '{{\"generated\":\"two\",\"marker\":\"alpha\",\"roots\":{{\"data\":\"{}\"}},\"tasks\":[],\"scout_reports\":[]}}'",
                temp.path().join("data").display()
            ),
        );
        let started = Instant::now();
        let stale = context.handle(request("GET", "/api/state"));
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(stale.status, 200);
        assert_eq!(header(&stale, "X-Multplx-Cache"), Some("stale"));
        assert_eq!(header(&stale, "X-Multplx-Refresh"), Some("in-flight"));
        for _ in 0..8 {
            assert_eq!(context.handle(request("GET", "/api/state")).status, 200);
        }
        let wait_started = Instant::now();
        loop {
            if !context.runtime.lock().expect("runtime").refresh_in_flight {
                break;
            }
            assert!(wait_started.elapsed() < Duration::from_secs(1));
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut conditional = request("GET", "/api/state");
        conditional
            .headers
            .insert("if-none-match".to_owned(), first_etag.clone());
        let refreshed = context.handle(conditional);
        assert_eq!(refreshed.status, 304);
        assert_eq!(header(&refreshed, "ETag"), Some(first_etag.as_str()));
        assert_ne!(
            header(&refreshed, "X-Multplx-Snapshot-Hash"),
            Some(first_snapshot_hash.as_str())
        );
        let runtime = context.runtime.lock().expect("runtime");
        assert_eq!(runtime.metrics.refresh_attempts, 2);
        assert_eq!(runtime.metrics.refresh_successes, 2);
        assert_eq!(runtime.metrics.stale_serves, 9);
        assert_eq!(runtime.metrics.not_modified, 1);
    }
}

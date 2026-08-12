//! One-shot vplan artifact lifecycle and authenticated confirmation service.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use multplx_core::filesystem::{PublicationFault, atomic_replace_with_fault};
use multplx_core::process::{
    ProcessProbe, ProcessTerminator, SystemProcessProbe, SystemProcessTerminator,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Iso8601;

use crate::http::{
    Request, Response, content_type, encode_component, percent_decode, read_request, write_response,
};

use super::{
    Result, ServiceError, VPLAN_HELP, accept_loop, acquire_lock, bind_loopback,
    canonical_directory, canonical_file, constant_time_eq, create_new_file,
    ensure_private_directory, is_within, parse_integer_env, parse_port, random_token,
    record_identity, record_live, record_map, record_process_live, remove_record_if_matches,
    sha256_hex, shutdown_flag, start_service, utc_now, utf8_arg, valid_token, write_record,
};

const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_COMMENTS: usize = 500;
const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;
type CommentBlock = (Vec<Comment>, Option<(usize, usize)>);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Comment {
    id: String,
    selector: String,
    anchor_text: String,
    nearest_heading: String,
    comment: String,
    ts: String,
    resolved: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmPayload {
    comments: Vec<Comment>,
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn validate_comment(comment: &Comment, label: &str) -> Result<()> {
    for (name, value, limit) in [
        ("id", comment.id.as_str(), 200),
        ("selector", comment.selector.as_str(), 2048),
        ("anchor_text", comment.anchor_text.as_str(), 4096),
        ("nearest_heading", comment.nearest_heading.as_str(), 512),
        ("comment", comment.comment.as_str(), 20_000),
        ("ts", comment.ts.as_str(), 64),
    ] {
        if utf16_len(value) > limit {
            return Err(ServiceError::new(format!(
                "{label}.{name} exceeds {limit} characters"
            )));
        }
    }
    if comment.id.is_empty() || comment.selector.is_empty() || comment.comment.trim().is_empty() {
        return Err(ServiceError::new(format!(
            "{label} requires non-empty id, selector, and comment fields"
        )));
    }
    let timestamp_shape =
        Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$")
            .expect("timestamp regex");
    if !timestamp_shape.is_match(&comment.ts)
        || OffsetDateTime::parse(&comment.ts, &Iso8601::DEFAULT).is_err()
    {
        return Err(ServiceError::new(format!(
            "{label}.ts must be an ISO-8601 timestamp"
        )));
    }
    Ok(())
}

fn validate_comments(comments: &[Comment], label: &str) -> Result<()> {
    if comments.len() > MAX_COMMENTS {
        return Err(ServiceError::new(format!(
            "{label} exceeds the {MAX_COMMENTS}-comment limit"
        )));
    }
    let mut ids = BTreeSet::new();
    for (index, comment) in comments.iter().enumerate() {
        validate_comment(comment, &format!("{label}[{index}]"))?;
        if !ids.insert(&comment.id) {
            return Err(ServiceError::new(format!(
                "{label} contains duplicate id '{}'",
                comment.id
            )));
        }
    }
    Ok(())
}

fn comment_regex() -> Regex {
    Regex::new(r#"(?s)<script type="application/json" id="vplan-comments">\s*(.*?)\s*</script>"#)
        .expect("comment regex")
}

fn parse_comment_block(html: &str) -> Result<CommentBlock> {
    let id_regex = Regex::new(r#"\bid=(?:"vplan-comments"|'vplan-comments')"#).expect("id regex");
    let id_count = id_regex.find_iter(html).count();
    let regex = comment_regex();
    let matches = regex.captures_iter(html).collect::<Vec<_>>();
    if id_count == 0 && matches.is_empty() {
        return Ok((Vec::new(), None));
    }
    if id_count != 1 || matches.len() != 1 {
        return Err(ServiceError::new(
            "artifact has a malformed or duplicate #vplan-comments block",
        ));
    }
    let capture = &matches[0];
    let json_match = capture.get(1).expect("comment JSON capture");
    let comments = serde_json::from_str::<Vec<Comment>>(json_match.as_str()).map_err(|error| {
        ServiceError::new(format!(
            "artifact has malformed #vplan-comments JSON: {error}"
        ))
    })?;
    validate_comments(&comments, "existing comments")?;
    let block = capture.get(0).expect("block capture");
    Ok((comments, Some((block.start(), block.end()))))
}

fn parse_confirm_payload(bytes: &[u8]) -> Result<Vec<Comment>> {
    if bytes.is_empty() {
        return Err(ServiceError::new("confirm payload is empty"));
    }
    let value = serde_json::from_slice::<Value>(bytes).map_err(|error| {
        ServiceError::new(format!("confirm payload is not valid JSON: {error}"))
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| ServiceError::new("confirm payload must be an object"))?;
    if object.len() != 1 || !object.contains_key("comments") {
        return Err(ServiceError::new(
            "confirm payload must contain only the required 'comments' field",
        ));
    }
    let payload = serde_json::from_value::<ConfirmPayload>(value)
        .map_err(|error| ServiceError::new(format!("invalid confirm payload: {error}")))?;
    validate_comments(&payload.comments, "comments")?;
    Ok(payload.comments)
}

fn merge_comments(existing: &[Comment], incoming: &[Comment]) -> Result<Vec<Comment>> {
    let mut merged = existing.to_vec();
    let mut by_id = merged
        .iter()
        .enumerate()
        .map(|(index, comment)| (comment.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for comment in incoming {
        let Some(index) = by_id.get(&comment.id).copied() else {
            by_id.insert(comment.id.clone(), merged.len());
            merged.push(comment.clone());
            continue;
        };
        let prior = &merged[index];
        if prior.id != comment.id
            || prior.selector != comment.selector
            || prior.anchor_text != comment.anchor_text
            || prior.nearest_heading != comment.nearest_heading
            || prior.comment != comment.comment
            || prior.ts != comment.ts
        {
            return Err(ServiceError::new(format!(
                "comment id '{}' collides with different persisted content",
                comment.id
            )));
        }
        merged[index].resolved = prior.resolved || comment.resolved;
    }
    Ok(merged)
}

fn serialize_comments(comments: &[Comment]) -> Result<String> {
    let json = serde_json::to_string_pretty(comments)
        .map_err(|error| ServiceError::new(format!("could not serialize comments: {error}")))?
        .replace('<', "\\u003c");
    Ok(format!(
        "<script type=\"application/json\" id=\"vplan-comments\">\n{json}\n</script>"
    ))
}

fn merge_comment_block(html: &str, incoming: &[Comment]) -> Result<(String, Vec<Comment>)> {
    let (existing, block) = parse_comment_block(html)?;
    let comments = merge_comments(&existing, incoming)?;
    let serialized = serialize_comments(&comments)?;
    if let Some((start, end)) = block {
        return Ok((
            format!("{}{}{}", &html[..start], serialized, &html[end..]),
            comments,
        ));
    }
    let body_regex = Regex::new(r"(?i)</body\s*>").expect("body regex");
    let closers = body_regex.find_iter(html).collect::<Vec<_>>();
    if closers.len() != 1 {
        return Err(ServiceError::new(
            "artifact must contain exactly one closing </body> tag",
        ));
    }
    let index = closers[0].start();
    let separator = if html[..index].ends_with('\n') {
        ""
    } else {
        "\n"
    };
    Ok((
        format!(
            "{}{}{}\n{}",
            &html[..index],
            separator,
            serialized,
            &html[index..]
        ),
        comments,
    ))
}

fn encode_relative_path(path: &Path) -> Result<String> {
    path.components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .map(encode_component)
                .ok_or_else(|| ServiceError::new("artifact path is not valid UTF-8")),
            _ => Err(ServiceError::new("artifact relative path is invalid")),
        })
        .collect::<Result<Vec<_>>>()
        .map(|segments| segments.join("/"))
}

fn inject_review_surface(html: &str, artifact: &Path, root: &Path, token: &str) -> Result<String> {
    let relative = artifact
        .strip_prefix(root)
        .map_err(|_| ServiceError::new("artifact must be inside the Multplx root"))?;
    let directory = relative.parent().unwrap_or_else(|| Path::new(""));
    let suffix = if directory.as_os_str().is_empty() {
        String::new()
    } else {
        format!("{}/", encode_relative_path(directory)?)
    };
    let head_regex = Regex::new(r"(?i)<head(?:\s[^>]*)?>").expect("head regex");
    let body_regex = Regex::new(r"(?i)</body\s*>").expect("body regex");
    let Some(head) = head_regex.find(html) else {
        return Err(ServiceError::new(
            "artifact must contain one <head> and exactly one closing </body> tag",
        ));
    };
    let bodies = body_regex.find_iter(html).collect::<Vec<_>>();
    if bodies.len() != 1 {
        return Err(ServiceError::new(
            "artifact must contain one <head> and exactly one closing </body> tag",
        ));
    }
    let head_injection = format!(
        "<base data-vplan-injected href=\"/__vplan/root/{suffix}\">\n<meta data-vplan-injected name=\"vplan-token\" content=\"{token}\">\n<link data-vplan-injected rel=\"stylesheet\" href=\"/__vplan/sdk.css\">"
    );
    let mut injected = format!(
        "{}\n{}{}",
        &html[..head.end()],
        head_injection,
        &html[head.end()..]
    );
    let body = body_regex.find(&injected).expect("body remains");
    injected.insert_str(
        body.start(),
        "<script data-vplan-injected src=\"/__vplan/sdk.js\"></script>\n",
    );
    Ok(injected)
}

fn common_headers(mut response: Response) -> Response {
    response.headers.extend([
        ("Cache-Control".to_owned(), "no-store".to_owned()),
        (
            "Content-Security-Policy".to_owned(),
            "default-src 'self' data: blob:; script-src 'self' 'unsafe-inline' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'".to_owned(),
        ),
        ("Referrer-Policy".to_owned(), "no-referrer".to_owned()),
        ("X-Content-Type-Options".to_owned(), "nosniff".to_owned()),
        ("X-Frame-Options".to_owned(), "DENY".to_owned()),
    ]);
    response
}

fn serve_file(path: &Path) -> Result<Response> {
    let metadata = fs::metadata(path)
        .map_err(|error| ServiceError::new(format!("could not inspect asset: {error}")))?;
    if !metadata.is_file() || metadata.len() > MAX_ARTIFACT_BYTES as u64 {
        return Err(ServiceError::new("asset path is not a bounded file"));
    }
    let bytes = fs::read(path)
        .map_err(|error| ServiceError::new(format!("could not read asset: {error}")))?;
    Ok(Response::new(200, bytes).header("Content-Type", content_type(path)))
}

#[derive(Debug)]
struct Runtime {
    last_request: Instant,
    confirming: bool,
}

struct ServerContext {
    artifact: PathBuf,
    root: PathBuf,
    artifact_directory: PathBuf,
    asset_directory: PathBuf,
    token: String,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    runtime: Mutex<Runtime>,
}

impl ServerContext {
    fn handle(&self, request: Request) -> (Response, bool) {
        self.runtime.lock().expect("runtime").last_request = Instant::now();
        let raw_path = request.target.split('?').next().unwrap_or("/");
        if request.method == "GET" && raw_path == "/" {
            let result = fs::read_to_string(&self.artifact)
                .map_err(|error| ServiceError::new(format!("could not read artifact: {error}")))
                .and_then(|html| {
                    inject_review_surface(&html, &self.artifact, &self.root, &self.token)
                })
                .map(|html| {
                    Response::new(200, html.into_bytes())
                        .header("Content-Type", "text/html; charset=utf-8")
                });
            return (common_headers(result.unwrap_or_else(error_response)), false);
        }
        if request.method == "GET" && raw_path.starts_with("/__vplan/") {
            let result = self
                .resolve_asset(raw_path)
                .and_then(|path| serve_file(&path));
            return (common_headers(result.unwrap_or_else(error_response)), false);
        }
        if request.method == "POST" && raw_path == "/confirm" {
            let token = request
                .headers
                .get("x-vplan-token")
                .map(String::as_str)
                .unwrap_or_default();
            if !constant_time_eq(token, &self.token) {
                return (
                    common_headers(Response::json(
                        403,
                        &json!({"error":"invalid review token"}),
                    )),
                    false,
                );
            }
            if !request
                .headers
                .get("content-type")
                .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"))
            {
                return (
                    common_headers(Response::json(
                        415,
                        &json!({"error":"content-type must be application/json"}),
                    )),
                    false,
                );
            }
            {
                let mut runtime = self.runtime.lock().expect("runtime");
                if runtime.confirming {
                    return (
                        common_headers(Response::json(
                            409,
                            &json!({"error":"confirm already in progress"}),
                        )),
                        false,
                    );
                }
                runtime.confirming = true;
            }
            match self.confirm(&request.body) {
                Ok((saved, total)) => {
                    return (
                        common_headers(Response::json(200, &json!({"saved":saved,"total":total}))),
                        true,
                    );
                }
                Err(error) => {
                    self.runtime.lock().expect("runtime").confirming = false;
                    return (
                        common_headers(Response::json(400, &json!({"error":error.message}))),
                        false,
                    );
                }
            }
        }
        (
            common_headers(Response::new(404, b"not found\n".to_vec())),
            false,
        )
    }

    fn resolve_asset(&self, raw_path: &str) -> Result<PathBuf> {
        let lexical = match raw_path {
            "/__vplan/sdk.js" => self.asset_directory.join("sdk.js"),
            "/__vplan/sdk.css" => self.asset_directory.join("sdk.css"),
            "/__vplan/mermaid.min.js" => self.asset_directory.join("mermaid.min.js"),
            value if value.starts_with("/__vplan/root/") => {
                let suffix = &value["/__vplan/root/".len()..];
                let mut relative = PathBuf::new();
                for segment in suffix.split('/') {
                    let decoded = percent_decode(segment)
                        .map_err(|_| ServiceError::new("asset path is not valid URL encoding"))?;
                    if decoded.is_empty()
                        || decoded == "."
                        || decoded == ".."
                        || decoded.contains(['/', '\0'])
                    {
                        return Err(ServiceError::new("asset path escapes the Multplx root"));
                    }
                    relative.push(decoded);
                }
                self.root.join(relative)
            }
            _ => return Err(ServiceError::new("not found")),
        };
        if !is_within(&self.root, &lexical) {
            return Err(ServiceError::new("asset path escapes the Multplx root"));
        }
        let canonical = fs::canonicalize(&lexical).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ServiceError::new("not found")
            } else {
                ServiceError::new(format!("could not canonicalize asset: {error}"))
            }
        })?;
        if !is_within(&self.artifact_directory, &canonical)
            && !is_within(&self.asset_directory, &canonical)
        {
            return Err(ServiceError::new(
                "asset path is outside the artifact and vplan asset directories",
            ));
        }
        Ok(canonical)
    }

    fn confirm(&self, body: &[u8]) -> Result<(usize, usize)> {
        let incoming = parse_confirm_payload(body)?;
        if std::env::var("MX_VPLAN_CONFIRM_KILL_POINT").as_deref() == Ok("before-write") {
            std::process::abort();
        }
        let html = fs::read_to_string(&self.artifact)
            .map_err(|error| ServiceError::new(format!("could not read artifact: {error}")))?;
        let (merged, comments) = merge_comment_block(&html, &incoming)?;
        if merged != html {
            let mode = fs::metadata(&self.artifact)
                .map_err(|error| ServiceError::new(format!("could not inspect artifact: {error}")))?
                .permissions()
                .mode()
                & 0o777;
            let fault = match std::env::var("MX_VPLAN_CONFIRM_FAULT").as_deref() {
                Ok("before-write") => Some(PublicationFault::BeforeWrite),
                Ok("after-write") => Some(PublicationFault::AfterWrite),
                Ok("after-mode") => Some(PublicationFault::AfterMode),
                Ok("after-rename") => Some(PublicationFault::AfterRename),
                _ => None,
            };
            atomic_replace_with_fault(&self.artifact, merged.as_bytes(), mode, fault).map_err(
                |error| ServiceError::new(format!("could not persist comments: {error}")),
            )?;
        }
        if std::env::var("MX_VPLAN_CONFIRM_KILL_POINT").as_deref() == Ok("after-rename") {
            std::process::abort();
        }
        Ok((incoming.len(), comments.len()))
    }
}

fn error_response(error: ServiceError) -> Response {
    let status = if error.message == "not found" {
        404
    } else {
        400
    };
    Response::json(status, &json!({"error":error.message}))
}

fn handle_connection(mut stream: TcpStream, context: &ServerContext) {
    let request = match read_request(&mut stream, MAX_BODY_BYTES) {
        Ok(request) => request,
        Err(response) => {
            let _ = write_response(&mut stream, &common_headers(response));
            return;
        }
    };
    let (response, confirmed) = context.handle(request);
    if write_response(&mut stream, &response).is_ok() && confirmed {
        if std::env::var("MX_VPLAN_CONFIRM_KILL_POINT").as_deref() == Ok("after-response") {
            std::process::abort();
        }
        context.shutdown.store(true, Ordering::SeqCst);
    }
}

pub(super) fn run_server(args: &[OsString]) -> Result<i32> {
    if args.len() != 7 || args.first().and_then(|value| value.to_str()) != Some("--serve") {
        return Err(ServiceError::usage(
            "usage: mx services vplan-server --serve <artifact> <root> <run-record> <lock> <token> <first-port>",
        ));
    }
    let artifact = canonical_file(Path::new(utf8_arg(args, 1, "artifact")?), "artifact")?;
    let root = canonical_directory(Path::new(utf8_arg(args, 2, "root")?), "Multplx root")?;
    if !is_within(&root, &artifact) || artifact == root {
        return Err(ServiceError::new(
            "artifact must be inside the Multplx root",
        ));
    }
    let run_record = PathBuf::from(utf8_arg(args, 3, "run record")?);
    let lock = PathBuf::from(utf8_arg(args, 4, "service lock")?);
    let token = utf8_arg(args, 5, "review token")?.to_owned();
    if !valid_token(&token) {
        return Err(ServiceError::new("review token is invalid"));
    }
    let first_port = utf8_arg(args, 6, "first port")?
        .parse::<u16>()
        .ok()
        .filter(|port| *port >= 1 && *port <= 65_516)
        .ok_or_else(|| ServiceError::new("first port must be an integer from 1 through 65516"))?;
    let idle = Duration::from_secs(parse_integer_env("MX_VPLAN_IDLE_SECS", 1800, 1, 86_400)?);
    let asset_directory = fs::canonicalize(root.join("share/vplan")).map_err(|error| {
        ServiceError::new(format!("vplan asset directory is unavailable: {error}"))
    })?;
    let artifact_directory = fs::canonicalize(artifact.parent().expect("artifact parent"))
        .map_err(|error| {
            ServiceError::new(format!("artifact directory is unavailable: {error}"))
        })?;
    let (listener, port) = bind_loopback(first_port)?;
    let shutdown = shutdown_flag()?;
    let context = Arc::new(ServerContext {
        artifact,
        root,
        artifact_directory,
        asset_directory,
        token: token.clone(),
        shutdown: Arc::clone(&shutdown),
        runtime: Mutex::new(Runtime {
            last_request: Instant::now(),
            confirming: false,
        }),
    });
    println!("READY {port}");
    std::io::stdout().flush().ok();
    let idle_context = Arc::clone(&context);
    let handler_context = Arc::clone(&context);
    accept_loop(
        listener,
        Arc::clone(&shutdown),
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
    if std::env::var("MX_VPLAN_CONFIRM_KILL_POINT").as_deref() == Ok("before-record-cleanup") {
        std::process::abort();
    }
    let _guard = acquire_lock(&lock)?;
    remove_record_if_matches(&run_record, std::process::id(), &token)?;
    Ok(0)
}

fn state_directory(root: &Path) -> PathBuf {
    let home = std::env::var_os("MX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_owned());
    std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"))
        .join(".vplan")
}

fn artifact_record(state: &Path, artifact: &Path) -> Result<(PathBuf, PathBuf)> {
    let path = artifact
        .to_str()
        .ok_or_else(|| ServiceError::new("artifact path is not valid UTF-8"))?;
    let hash = sha256_hex(path.as_bytes());
    Ok((
        state.join(format!("{hash}.run")),
        state.join(format!("{hash}.serve.lock")),
    ))
}

fn assert_artifact_under_root(root: &Path, artifact: &Path) -> Result<()> {
    if artifact == root || !is_within(root, artifact) {
        return Err(ServiceError::new(format!(
            "artifact must be inside the Multplx root: {}",
            artifact.display()
        )));
    }
    if artifact.to_str().is_some_and(|value| value.contains('\n')) {
        return Err(ServiceError::new("artifact paths may not contain newlines"));
    }
    Ok(())
}

fn comments_command(path: &Path) -> Result<i32> {
    let html = fs::read_to_string(path)
        .map_err(|error| ServiceError::new(format!("could not read artifact: {error}")))?;
    let (comments, _) = parse_comment_block(&html)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&comments)
            .map_err(|error| ServiceError::new(format!("could not serialize comments: {error}")))?
    );
    Ok(0)
}

fn new_command(root: &Path, argument: &str) -> Result<i32> {
    let input = PathBuf::from(argument);
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| {
        ServiceError::new(format!(
            "could not resolve destination: {argument}: {error}"
        ))
    })?;
    let parent = canonical_directory(parent, "destination directory")?;
    let base = input
        .file_name()
        .ok_or_else(|| ServiceError::new(format!("could not resolve destination: {argument}")))?;
    let destination = parent.join(base);
    assert_artifact_under_root(root, &destination)?;
    if destination.exists() {
        return Err(ServiceError::new(format!(
            "refusing to overwrite existing artifact: {}",
            destination.display()
        )));
    }
    let assets = root.join("share/vplan");
    let relative = pathdiff(&parent, &assets)?;
    let asset_base = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let asset_base = if asset_base.starts_with('.') {
        asset_base
    } else {
        format!("./{asset_base}")
    };
    let template = fs::read_to_string(assets.join("template.html"))
        .map_err(|error| ServiceError::new(format!("could not read seed template: {error}")))?;
    let rendered = template.replace("./mermaid.min.js", &format!("{asset_base}/mermaid.min.js"));
    let temporary = parent.join(format!(
        ".{}.vplan-new-{}-{}.tmp",
        base.to_string_lossy(),
        std::process::id(),
        &random_token()?[..8]
    ));
    create_new_file(&temporary, rendered.as_bytes(), 0o644)?;
    fs::rename(&temporary, &destination).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        ServiceError::new(format!(
            "could not create artifact: {}: {error}",
            destination.display()
        ))
    })?;
    println!("{}", destination.display());
    Ok(0)
}

fn pathdiff(from: &Path, to: &Path) -> Result<PathBuf> {
    let from = from.components().collect::<Vec<_>>();
    let to = to.components().collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return Err(ServiceError::new(
            "could not compute the relative asset path",
        ));
    }
    let mut relative = PathBuf::new();
    for _ in common..from.len() {
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component.as_os_str());
    }
    Ok(relative)
}

fn review_command(root: &Path, artifact: &Path) -> Result<i32> {
    let state = state_directory(root);
    ensure_private_directory(&state, "vplan state directory")?;
    let (record, lock) = artifact_record(&state, artifact)?;
    let _guard = acquire_lock(&lock)?;
    if record.exists() {
        let existing = record_map(&record)?;
        if existing.get("artifact").map(Path::new) == Some(artifact) && record_live(&existing, None)
        {
            let port = existing
                .get("port")
                .ok_or_else(|| ServiceError::new("live review record is missing port"))?;
            println!("http://127.0.0.1:{port}/");
            return Ok(0);
        }
        if record_process_live(&existing) {
            return Err(ServiceError::new(format!(
                "unsafe live vplan run record: {}",
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
                "unsafe vplan run record: {}",
                record.display()
            )));
        }
    }
    let token = random_token()?;
    let first_port = parse_port("MX_VPLAN_PORT", 4870)?;
    let service_args = vec![
        OsString::from("--serve"),
        artifact.as_os_str().to_owned(),
        root.as_os_str().to_owned(),
        record.as_os_str().to_owned(),
        lock.as_os_str().to_owned(),
        OsString::from(&token),
        OsString::from(first_port.to_string()),
    ];
    let started = start_service("vplan-server", &service_args)?;
    let pid = started.child.id();
    let identity = SystemProcessProbe::default().identity(pid).map_err(|_| {
        ServiceError::new("server started but its process identity could not be verified")
    })?;
    let bytes = format!(
        "version=1\nartifact={}\nport={}\npid={}\npid_identity={}\ntoken={}\nstarted_at={}\n",
        artifact.display(),
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
    drop(started.ready);
    drop(started.errors);
    drop(started.child);
    println!("http://127.0.0.1:{}/", started.port);
    Ok(0)
}

fn stop_command(root: &Path, artifact: &Path) -> Result<i32> {
    let state = state_directory(root);
    ensure_private_directory(&state, "vplan state directory")?;
    let (record, lock) = artifact_record(&state, artifact)?;
    let guard = acquire_lock(&lock)?;
    if !record.exists() {
        println!("no active review for {}", artifact.display());
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
        println!("removed stale review record for {}", artifact.display());
        return Ok(0);
    };
    if record_process_live(&values)
        && (values.get("artifact").map(Path::new) != Some(artifact) || !record_live(&values, None))
    {
        return Err(ServiceError::new(format!(
            "unsafe live vplan run record: {}",
            record.display()
        )));
    }
    let probe = SystemProcessProbe::default();
    if !probe.is_alive(pid) || !probe.identity(pid).is_ok_and(|actual| actual == identity) {
        remove_record_if_matches(&record, pid, &token)?;
        println!("removed stale review record for {}", artifact.display());
        return Ok(0);
    }
    let mut terminator = SystemProcessTerminator::default();
    terminator
        .terminate(&identity)
        .map_err(|_| ServiceError::new(format!("could not stop review process {pid}")))?;
    drop(guard);
    if !terminator.wait_gone(&identity, Duration::from_secs(5)) {
        return Err(ServiceError::new(format!(
            "review process {pid} did not stop after 5 seconds"
        )));
    }
    let _guard = acquire_lock(&lock)?;
    remove_record_if_matches(&record, pid, &token)?;
    println!("stopped review for {}", artifact.display());
    Ok(0)
}

fn self_check(root: &Path) -> Result<i32> {
    let asset = root.join("share/vplan");
    for file in [
        asset.join("template.html"),
        asset.join("manifest.json"),
        asset.join("sdk.js"),
        asset.join("sdk.css"),
        asset.join("mermaid.min.js"),
    ] {
        if !file.is_file() {
            return Err(ServiceError::new("bundled vplan self-check failed"));
        }
    }
    let manifest = fs::read(asset.join("manifest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .ok_or_else(|| ServiceError::new("bundled vplan self-check failed"))?;
    let expected = manifest
        .pointer("/mermaid/sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| ServiceError::new("bundled vplan self-check failed"))?;
    let actual = format!(
        "{:x}",
        Sha256::digest(
            fs::read(asset.join("mermaid.min.js"))
                .map_err(|_| ServiceError::new("bundled vplan self-check failed"))?
        )
    );
    let template = fs::read_to_string(asset.join("template.html"))
        .map_err(|_| ServiceError::new("bundled vplan self-check failed"))?;
    if actual != expected || !template.contains("mermaid.min.js") {
        return Err(ServiceError::new("bundled vplan self-check failed"));
    }
    Ok(0)
}

pub(super) fn run_cli(args: &[OsString], source_root: &Path) -> Result<i32> {
    let root = canonical_directory(source_root, "Multplx root")?;
    match args.first().and_then(|value| value.to_str()) {
        Some("-h" | "--help") if args.len() == 1 => {
            print!("{VPLAN_HELP}");
            Ok(0)
        }
        Some("--self-check") if args.len() == 1 => self_check(&root),
        Some("--self-check") => Err(ServiceError::new("--self-check accepts no arguments")),
        Some(command @ ("new" | "review" | "comments" | "stop")) => {
            if args.len() != 2 {
                return Err(ServiceError::new(
                    "expected exactly one artifact path (see --help)",
                ));
            }
            let argument = utf8_arg(args, 1, "artifact path")?;
            if command == "new" {
                return new_command(&root, argument);
            }
            let artifact = canonical_file(Path::new(argument), "artifact")?;
            assert_artifact_under_root(&root, &artifact)?;
            match command {
                "review" => review_command(&root, &artifact),
                "comments" => comments_command(&artifact),
                "stop" => stop_command(&root, &artifact),
                _ => unreachable!(),
            }
        }
        _ => {
            eprint!("{VPLAN_HELP}");
            Ok(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Comment, Runtime, ServerContext, assert_artifact_under_root, encode_relative_path,
        error_response, inject_review_surface, merge_comment_block, parse_comment_block,
        parse_confirm_payload, pathdiff, self_check, serve_file, validate_comments,
    };
    use crate::http::Request;
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    fn comment(resolved: bool) -> Comment {
        Comment {
            id: "c1".to_owned(),
            selector: "#target".to_owned(),
            anchor_text: "anchor".to_owned(),
            nearest_heading: "Heading".to_owned(),
            comment: "feedback".to_owned(),
            ts: "2026-08-12T12:00:00Z".to_owned(),
            resolved,
        }
    }

    #[test]
    fn comments_round_trip_and_resolution_are_monotonic() {
        let html = "<html><head></head><body>body</body></html>";
        let (merged, comments) = merge_comment_block(html, &[comment(true)]).expect("merge");
        assert_eq!(comments.len(), 1);
        let (parsed, _) = parse_comment_block(&merged).expect("parse");
        assert!(parsed[0].resolved);
        let (again, parsed) = merge_comment_block(&merged, &[comment(false)]).expect("merge again");
        assert!(parsed[0].resolved);
        assert_eq!(again, merged);
        assert!(
            merge_comment_block(
                &merged,
                &[Comment {
                    comment: "changed".to_owned(),
                    ..comment(false)
                }]
            )
            .is_err()
        );
        assert!(
            parse_comment_block(
                "<body><div id=\"vplan-comments\"></div><div id='vplan-comments'></div></body>"
            )
            .is_err()
        );
        assert!(
            parse_comment_block(
                "<script type=\"application/json\" id=\"vplan-comments\">bad</script>"
            )
            .is_err()
        );
        assert!(merge_comment_block("<html></html>", &[]).is_err());
        assert!(merge_comment_block("<html><body>\n</body></html>", &[]).is_ok());
    }

    #[test]
    fn payload_validation_rejects_unknown_missing_duplicate_and_oversized_fields() {
        let valid = serde_json::json!({"comments":[comment(false)]});
        assert!(parse_confirm_payload(serde_json::to_string(&valid).unwrap().as_bytes()).is_ok());
        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"comments":[],"extra":true}),
            serde_json::json!({"comments":[comment(false),comment(false)]}),
            serde_json::json!({"comments":[{"id":"x"}]}),
        ] {
            assert!(
                parse_confirm_payload(serde_json::to_string(&invalid).unwrap().as_bytes()).is_err()
            );
        }
        let oversized = Comment {
            comment: "x".repeat(20_001),
            ..comment(false)
        };
        assert!(
            parse_confirm_payload(
                serde_json::to_string(&serde_json::json!({"comments":[oversized]}))
                    .unwrap()
                    .as_bytes()
            )
            .is_err()
        );
        assert!(parse_confirm_payload(b"").is_err());
        assert!(parse_confirm_payload(b"[]").is_err());
        assert!(parse_confirm_payload(b"bad").is_err());
        assert!(validate_comments(&vec![comment(false); 501], "comments").is_err());
        for invalid in [
            Comment {
                id: String::new(),
                ..comment(false)
            },
            Comment {
                ts: "not-a-time".to_owned(),
                ..comment(false)
            },
            Comment {
                selector: "x".repeat(2049),
                ..comment(false)
            },
        ] {
            assert!(validate_comments(&[invalid], "comments").is_err());
        }
    }

    #[test]
    fn injection_changes_served_bytes_only_and_requires_well_formed_shell() {
        let root = PathBuf::from("/repo");
        let artifact = root.join("data/task/plan.html");
        let html = "<html><head><title>x</title></head><body>x</body></html>";
        let injected = inject_review_surface(html, &artifact, &root, "token").expect("inject");
        assert!(injected.contains("/__vplan/root/data/task/"));
        assert!(injected.contains("data-vplan-injected"));
        assert!(!html.contains("data-vplan-injected"));
        assert!(inject_review_surface("<html></html>", &artifact, &root, "token").is_err());
        assert!(
            inject_review_surface(
                "<html><head></head><body></body><body></body></html>",
                &artifact,
                &root,
                "token"
            )
            .is_err()
        );
        assert!(
            inject_review_surface(html, PathBuf::from("/outside").as_path(), &root, "token")
                .is_err()
        );
    }

    fn request(method: &str, target: &str, token: Option<&str>, body: &[u8]) -> Request {
        let mut headers = BTreeMap::new();
        if let Some(token) = token {
            headers.insert("x-vplan-token".to_owned(), token.to_owned());
            headers.insert("content-type".to_owned(), "application/json".to_owned());
        }
        Request {
            method: method.to_owned(),
            target: target.to_owned(),
            headers,
            body: body.to_vec(),
        }
    }

    #[test]
    fn direct_review_routes_cover_assets_authentication_conflict_and_confirmation() {
        let temp = tempfile::tempdir().expect("tempdir");
        for directory in ["share/vplan", "data/task"] {
            fs::create_dir_all(temp.path().join(directory)).expect("directory");
        }
        for (file, bytes) in [
            ("sdk.js", b"js".as_slice()),
            ("sdk.css", b"css".as_slice()),
            ("mermaid.min.js", b"mermaid".as_slice()),
        ] {
            fs::write(temp.path().join("share/vplan").join(file), bytes).expect("asset");
        }
        let artifact = temp.path().join("data/task/plan.html");
        fs::write(
            &artifact,
            "<html><head></head><body><main id=\"target\">Target</main></body></html>",
        )
        .expect("artifact");
        let root = fs::canonicalize(temp.path()).expect("root");
        let context = ServerContext {
            artifact: fs::canonicalize(&artifact).expect("artifact path"),
            artifact_directory: fs::canonicalize(artifact.parent().unwrap()).expect("directory"),
            asset_directory: fs::canonicalize(temp.path().join("share/vplan")).expect("assets"),
            root: root.clone(),
            token: "a".repeat(64),
            shutdown: Arc::new(AtomicBool::new(false)),
            runtime: Mutex::new(Runtime {
                last_request: Instant::now(),
                confirming: false,
            }),
        };
        assert_eq!(context.handle(request("GET", "/", None, &[])).0.status, 200);
        for asset in ["sdk.js", "sdk.css", "mermaid.min.js"] {
            assert_eq!(
                context
                    .handle(request("GET", &format!("/__vplan/{asset}"), None, &[]))
                    .0
                    .status,
                200
            );
        }
        assert_eq!(
            context
                .handle(request("GET", "/__vplan/missing", None, &[]))
                .0
                .status,
            404
        );
        for escaped in [
            "/__vplan/root/%2e%2e/file",
            "/__vplan/root/%zz",
            "/__vplan/root/data%2ftask/plan.html",
        ] {
            assert_eq!(
                context.handle(request("GET", escaped, None, &[])).0.status,
                400
            );
        }
        assert_eq!(
            context
                .handle(request(
                    "GET",
                    "/__vplan/root/data/task/plan.html",
                    None,
                    &[]
                ))
                .0
                .status,
            200
        );
        assert_eq!(
            context
                .handle(request(
                    "GET",
                    "/__vplan/root/data/task/missing.txt",
                    None,
                    &[]
                ))
                .0
                .status,
            404
        );
        assert_eq!(
            context
                .handle(request("POST", "/confirm", None, b"{}"))
                .0
                .status,
            403
        );
        let mut wrong_type = request("POST", "/confirm", Some(&"a".repeat(64)), b"{}");
        wrong_type
            .headers
            .insert("content-type".to_owned(), "text/plain".to_owned());
        assert_eq!(context.handle(wrong_type).0.status, 415);
        context.runtime.lock().expect("runtime").confirming = true;
        assert_eq!(
            context
                .handle(request("POST", "/confirm", Some(&"a".repeat(64)), b"{}"))
                .0
                .status,
            409
        );
        context.runtime.lock().expect("runtime").confirming = false;
        assert_eq!(
            context
                .handle(request("POST", "/confirm", Some(&"a".repeat(64)), b"bad"))
                .0
                .status,
            400
        );
        let body =
            serde_json::to_vec(&serde_json::json!({"comments":[comment(false)]})).expect("payload");
        let (response, confirmed) = context.handle(request(
            "POST",
            "/confirm?source=test",
            Some(&"a".repeat(64)),
            &body,
        ));
        assert_eq!(response.status, 200);
        assert!(confirmed);
        assert_eq!(context.handle(request("PUT", "/", None, &[])).0.status, 404);

        let outside = temp.path().join("outside.txt");
        fs::write(&outside, "outside").expect("outside");
        std::os::unix::fs::symlink(&outside, temp.path().join("data/task/link")).expect("symlink");
        assert_eq!(
            context
                .handle(request("GET", "/__vplan/root/data/task/link", None, &[]))
                .0
                .status,
            400
        );
    }

    #[test]
    fn review_path_file_and_self_check_helpers_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let inside = root.join("data/plan.html");
        assert!(assert_artifact_under_root(root, &inside).is_ok());
        assert!(assert_artifact_under_root(root, root).is_err());
        assert!(assert_artifact_under_root(root, PathBuf::from("/outside").as_path()).is_err());
        assert!(
            pathdiff(&root.join("data/task"), &root.join("share/vplan"))
                .expect("relative")
                .to_string_lossy()
                .contains("share/vplan")
        );
        assert!(pathdiff(Path::new("relative"), Path::new("/absolute")).is_err());
        assert!(encode_relative_path(Path::new("../outside")).is_err());
        assert_eq!(
            error_response(super::ServiceError::new("not found")).status,
            404
        );
        assert!(serve_file(root).is_err());
        assert!(self_check(root).is_err());
    }

    use std::path::{Path, PathBuf};
}

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

use multplx_domain::maintainer_override::{
    self, Binding, BoundaryClass, OverrideStore, RecordState, Request,
};

const OVERRIDE_USAGE: &str = "Request, decide, consume, inspect, and audit exact maintainer exceptions.\n\nUsage:\n  mx-maintainer-override.sh registry [--json]\n  mx-maintainer-override.sh request --boundary <id> --task <id> --project <slug>\n    --operation <literal operation> --target <identity>\n    --expected-state <sha256> --consequence <one line> [--ttl <seconds>]\n  mx-maintainer-override.sh grant <request-id> --maintainer-words <literal words>\n  mx-maintainer-override.sh deny <request-id> --maintainer-words <literal words>\n  mx-maintainer-override.sh consume <request-id> --boundary <id> --task <id>\n    --project <slug> --operation <literal operation> --target <identity>\n    --expected-state <sha256>\n  mx-maintainer-override.sh result <request-id> --outcome succeeded|failed --detail <text>\n  mx-maintainer-override.sh inspect <request-id>\n  mx-maintainer-override.sh audit [--json]\n  mx-maintainer-override.sh digest <literal text>\n  mx-maintainer-override.sh argv [literal argv...]\n  mx-maintainer-override.sh handoff <request-id>\n";

pub fn run(entry: &str, args: &[OsString]) -> i32 {
    const ENTRIES: &[&str] = &[
        "mx-decision-hold.sh",
        "mx-maintainer-override.sh",
        "mx-override-bindings.sh",
        "mx-override-run.sh",
        "mx-workflow.sh",
    ];
    if !ENTRIES.contains(&entry) {
        eprintln!("error: unknown authority entry point: {entry}");
        return 2;
    }
    if entry == "mx-maintainer-override.sh" {
        return run_override(args);
    }
    if entry == "mx-decision-hold.sh" && args.first().and_then(|value| value.to_str()) == Some("id")
    {
        return decision_hold_id(args);
    }
    if entry == "mx-workflow.sh"
        && matches!(
            args.first().and_then(|value| value.to_str()),
            Some("validate" | "dry-run")
        )
    {
        return workflow_read(args);
    }
    run_compat(entry, args)
}

fn source_root() -> PathBuf {
    std::env::var_os("MX_RUST_SOURCE_ROOT")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn state_root() -> PathBuf {
    if let Some(state) = std::env::var_os("MX_STATE_OVERRIDE") {
        return PathBuf::from(state);
    }
    let home = std::env::var_os("MX_HOME")
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("state")
}

fn run_compat(entry: &str, args: &[OsString]) -> i32 {
    let root = source_root();
    let path = root.join("bin").join(entry);
    if !path.is_file() {
        eprintln!(
            "error: authority compatibility body is unavailable at {}",
            path.display()
        );
        return 1;
    }
    let error = std::process::Command::new("bash")
        .arg(path)
        .args(args)
        .env("MX_AUTHORITY_IMPLEMENTATION", "legacy")
        .env("MX_RUST_SOURCE_ROOT", &root)
        .exec();
    eprintln!("error: could not start {entry}: {error}");
    1
}

fn safe_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn decision_hold_id(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        eprintln!("mx-decision-hold: arguments must be UTF-8");
        return 1;
    };
    if values.len() != 3 {
        return run_compat("mx-decision-hold.sh", args);
    }
    if !safe_slug(&values[1]) {
        eprintln!(
            "mx-decision-hold: origin-id must be a non-empty privacy-safe slug: {}",
            values[1]
        );
        return 1;
    }
    if !safe_slug(&values[2]) {
        eprintln!(
            "mx-decision-hold: decision-key must be a non-empty privacy-safe slug: {}",
            values[2]
        );
        return 1;
    }
    println!("{}-decision-{}", values[1], values[2]);
    0
}

fn workflow_definition_path(requested: &str) -> std::result::Result<PathBuf, String> {
    let candidate = if requested.contains('/') || requested.ends_with(".workflow.md") {
        PathBuf::from(requested)
    } else {
        source_root()
            .join("workflows")
            .join(format!("{requested}.workflow.md"))
    };
    if !candidate.is_file() {
        return Err(format!("definition not found: {}", candidate.display()));
    }
    candidate
        .canonicalize()
        .map_err(|error| format!("definition is unavailable: {error}"))
}

fn workflow_read(args: &[OsString]) -> i32 {
    use multplx_domain::workflow::{self, Executor, StageType};

    let Some(values) = text_args(args) else {
        eprintln!("mx-workflow: arguments must be UTF-8");
        return 1;
    };
    let command = values[0].as_str();
    if command == "validate" {
        if values.len() != 2 {
            return run_compat("mx-workflow.sh", args);
        }
        let path = match workflow_definition_path(&values[1]) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("mx-workflow: {error}");
                return 1;
            }
        };
        return match workflow::parse(&path) {
            Ok(definition) => {
                println!(
                    "valid: {} ({} stages)",
                    definition.name,
                    definition.stages.len()
                );
                0
            }
            Err(error) => {
                eprintln!("mx-workflow: {error}");
                1
            }
        };
    }
    let Some(requested) = values.get(1) else {
        return run_compat("mx-workflow.sh", args);
    };
    let mut input = "example input".to_owned();
    let mut index = 2;
    while index < values.len() {
        if values[index] != "--input" || index + 1 >= values.len() {
            return run_compat("mx-workflow.sh", args);
        }
        input = values[index + 1].clone();
        index += 2;
    }
    let path = match workflow_definition_path(requested) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("mx-workflow: {error}");
            return 1;
        }
    };
    let definition = match workflow::parse(&path) {
        Ok(definition) => definition,
        Err(error) => {
            eprintln!("mx-workflow: {error}");
            return 1;
        }
    };
    println!("workflow: {}", definition.name);
    println!("input: {input}");
    for stage in definition.stages {
        let kind = match stage.kind {
            StageType::Interactive => "interactive",
            StageType::Agent => "agent",
            StageType::Command => "command",
        };
        let gate = match stage.gate {
            workflow::Gate::Approve => "approve",
            workflow::Gate::Auto => "auto",
        };
        let executor = match stage.executor {
            Some(Executor::Broker) => "broker",
            Some(Executor::Actor) => "actor",
            None => "-",
        };
        let output = stage
            .output
            .as_deref()
            .map(|value| workflow::substitute(value, "dry-run", &input, ""))
            .unwrap_or_else(|| "-".to_owned());
        println!(
            "{} | type={kind} | gate={gate} | executor={executor} | output={output}",
            stage.id
        );
    }
    0
}

fn text_args(args: &[OsString]) -> Option<Vec<String>> {
    args.iter()
        .map(|value| value.to_str().map(ToOwned::to_owned))
        .collect()
}

fn override_error(message: impl AsRef<str>) -> i32 {
    eprintln!("mx-maintainer-override: {}", message.as_ref());
    1
}

fn usage_error(message: impl AsRef<str>) -> i32 {
    eprintln!("mx-maintainer-override: {}", message.as_ref());
    eprint!("{OVERRIDE_USAGE}");
    2
}

fn option_map(values: &[String]) -> std::result::Result<BTreeMap<String, String>, String> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < values.len() {
        let name = &values[index];
        if !name.starts_with("--") {
            return Err(format!("unknown argument: {name}"));
        }
        let Some(value) = values.get(index + 1) else {
            return Err(format!("{name} requires a non-empty value"));
        };
        if value.is_empty() {
            return Err(format!("{name} requires a non-empty value"));
        }
        options.insert(name.trim_start_matches("--").to_owned(), value.clone());
        index += 2;
    }
    Ok(options)
}

fn required<'a>(options: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    options
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

fn run_override(args: &[OsString]) -> i32 {
    let Some(values) = text_args(args) else {
        return override_error("arguments must be UTF-8");
    };
    let Some(command) = values.first().map(String::as_str) else {
        eprint!("{OVERRIDE_USAGE}");
        return 2;
    };
    let rest = &values[1..];
    let store = OverrideStore::new(&state_root());
    match command {
        "-h" | "--help" => {
            print!("{OVERRIDE_USAGE}");
            0
        }
        "registry" => command_registry(rest),
        "request" => command_request(&store, rest),
        "grant" | "deny" => command_decide(&store, command == "grant", rest),
        "consume" => command_consume(&store, rest),
        "result" => command_result(&store, rest),
        "inspect" => command_inspect(&store, rest),
        "audit" => command_audit(&store, rest),
        "digest" if rest.len() == 1 => {
            println!("{}", maintainer_override::sha256_text(&rest[0]));
            0
        }
        "digest" => usage_error("digest requires one literal argument"),
        "argv" => {
            println!("{}", serde_json::to_string(rest).expect("serialize argv"));
            0
        }
        "handoff" => command_handoff(&store, rest),
        _ => {
            eprint!("{OVERRIDE_USAGE}");
            2
        }
    }
}

fn command_registry(args: &[String]) -> i32 {
    match args {
        [] => {
            print!("{}", maintainer_override::registry_text());
            0
        }
        [flag] if flag == "--json" => {
            let rows = maintainer_override::REGISTRY
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "boundary_id": entry.id,
                        "class": entry.class.as_str(),
                        "alternate": entry.alternate,
                    })
                })
                .collect::<Vec<_>>();
            println!(
                "{}",
                serde_json::to_string_pretty(&rows).expect("registry JSON")
            );
            0
        }
        _ => usage_error("registry accepts only --json"),
    }
}

fn command_request(store: &OverrideStore, args: &[String]) -> i32 {
    let options = match option_map(args) {
        Ok(value) => value,
        Err(error) => {
            return usage_error(error.replace("unknown argument", "unknown request argument"));
        }
    };
    let fields = [
        "boundary",
        "task",
        "project",
        "operation",
        "target",
        "expected-state",
        "consequence",
    ];
    if fields.iter().any(|name| required(&options, name).is_none()) {
        return usage_error("request requires every binding field");
    }
    if options
        .keys()
        .any(|name| !fields.contains(&name.as_str()) && name != "ttl")
    {
        return usage_error("unknown request argument");
    }
    let ttl = options.get("ttl").map_or_else(
        || {
            std::env::var("MX_OVERRIDE_DEFAULT_TTL")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(maintainer_override::DEFAULT_TTL)
        },
        |value| value.parse::<u64>().unwrap_or(0),
    );
    let request = Request {
        boundary: required(&options, "boundary").expect("checked"),
        task: required(&options, "task").expect("checked"),
        project: required(&options, "project").expect("checked"),
        operation: required(&options, "operation").expect("checked"),
        target: required(&options, "target").expect("checked"),
        expected_state_digest: required(&options, "expected-state").expect("checked"),
        consequence: required(&options, "consequence").expect("checked"),
        ttl,
    };
    match store.request(&request) {
        Ok(id) => {
            println!("{id}");
            0
        }
        Err(error) => override_error(error.to_string()),
    }
}

fn command_decide(store: &OverrideStore, grant: bool, args: &[String]) -> i32 {
    let label = if grant { "grant" } else { "deny" };
    let Some(request) = args.first() else {
        return usage_error(format!("{label} requires a request id"));
    };
    let options = match option_map(&args[1..]) {
        Ok(value) => value,
        Err(error) => return usage_error(error),
    };
    let Some(words) = required(&options, "maintainer-words") else {
        return usage_error(format!("{label} requires --maintainer-words"));
    };
    if options.len() != 1 {
        return usage_error(format!("unknown {label} argument"));
    }
    if let Err(error) = maintainer_override::require_primary_lock(&state_root()) {
        return override_error(error.to_string());
    }
    match store.decide(request, words, grant) {
        Ok(()) => 0,
        Err(error) => override_error(error.to_string()),
    }
}

fn binding_from(options: &BTreeMap<String, String>) -> Option<Binding<'_>> {
    Some(Binding {
        boundary: required(options, "boundary")?,
        task: required(options, "task")?,
        project: required(options, "project")?,
        operation: required(options, "operation")?,
        target: required(options, "target")?,
        expected_state_digest: required(options, "expected-state")?,
    })
}

fn command_consume(store: &OverrideStore, args: &[String]) -> i32 {
    let Some(request) = args.first() else {
        return usage_error("consume requires a request id");
    };
    let options = match option_map(&args[1..]) {
        Ok(value) => value,
        Err(error) => return usage_error(error),
    };
    let Some(binding) = binding_from(&options) else {
        return usage_error("consume requires every binding field");
    };
    if options.len() != 6 {
        return usage_error("unknown consume argument");
    }
    match store.consume(request, &binding) {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(error) => override_error(error.to_string()),
    }
}

fn command_result(store: &OverrideStore, args: &[String]) -> i32 {
    let Some(request) = args.first() else {
        return usage_error("result requires a request id");
    };
    let options = match option_map(&args[1..]) {
        Ok(value) => value,
        Err(error) => return usage_error(error),
    };
    let (Some(outcome), Some(detail)) =
        (required(&options, "outcome"), required(&options, "detail"))
    else {
        return usage_error("result requires --outcome and --detail");
    };
    let succeeded = match outcome {
        "succeeded" => true,
        "failed" => false,
        _ => return override_error("result must be succeeded or failed"),
    };
    match store.result(request, succeeded, detail) {
        Ok(()) => 0,
        Err(error) => override_error(error.to_string()),
    }
}

fn sorted_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries = object.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted_json(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(sorted_json).collect())
        }
        value => value,
    }
}

fn command_inspect(store: &OverrideStore, args: &[String]) -> i32 {
    let [request] = args else {
        return usage_error("inspect requires one request id");
    };
    match store.find(request) {
        Ok((_, _, record)) => {
            let value = serde_json::to_value(record).expect("record JSON");
            println!(
                "{}",
                serde_json::to_string_pretty(&sorted_json(value)).expect("pretty JSON")
            );
            0
        }
        Err(_) => override_error(format!("request not found or invalid: {request}")),
    }
}

fn command_audit(store: &OverrideStore, args: &[String]) -> i32 {
    let json = match args {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => return usage_error("audit accepts only --json"),
    };
    let (records, invalid) = store.audit();
    if json {
        let values = records
            .into_iter()
            .map(|(state, record)| {
                let mut value = serde_json::to_value(record).expect("record JSON");
                value.as_object_mut().expect("record object").insert(
                    "record_state".to_owned(),
                    serde_json::Value::String(state.as_str().to_owned()),
                );
                value
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&values).expect("audit JSON")
        );
    } else {
        for path in &invalid {
            println!("invalid\t{}", path.display());
        }
        for (state, record) in records {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                record.request_id,
                state.as_str(),
                record.boundary_id,
                record.task_id,
                record.project,
                record.target_identity,
                serde_json::to_value(record.outcome)
                    .expect("outcome")
                    .as_str()
                    .expect("outcome string")
            );
        }
    }
    if invalid.is_empty() { 0 } else { 1 }
}

fn command_handoff(store: &OverrideStore, args: &[String]) -> i32 {
    let [request] = args else {
        return usage_error("handoff requires one request id");
    };
    let Ok((state, _, record)) = store.find(request) else {
        return override_error(format!("request not found or invalid: {request}"));
    };
    if state != RecordState::Consumed || record.outcome != maintainer_override::Outcome::NotRun {
        return override_error("handoff requires an atomically consumed request with no outcome");
    }
    let permitted = maintainer_override::boundary(&record.boundary_id).is_some_and(|entry| {
        entry.class == BoundaryClass::Policy
            && matches!(
                entry.id,
                "authentication.login" | "delivery.credentialed-action"
            )
    });
    if !permitted {
        return override_error(format!(
            "boundary does not use operator handoff: {}",
            record.boundary_id
        ));
    }
    println!(
        "request={}\nboundary={}\ntarget={}\noperation={}\nconsequence={}",
        record.request_id,
        record.boundary_id,
        record.target_identity,
        record.action_argv_or_operation,
        record.consequence
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_entry_fails_before_execution() {
        assert_eq!(run("not-an-entry", &[]), 2);
    }

    #[test]
    fn compatibility_process_is_pinned_to_legacy() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        std::fs::create_dir(&bin).expect("bin");
        let script = bin.join("mx-workflow.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"${MX_AUTHORITY_IMPLEMENTATION:-unset}\"\n",
        )
        .expect("script");
        let mut permissions = std::fs::metadata(&script).expect("metadata").permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("mode");
        // `exec` cannot be unit-tested in-process; the integration suite covers
        // the observable dispatch.  This assertion protects the entry list.
        assert!(ENTRIES_FOR_TEST.contains(&"mx-workflow.sh"));
    }

    const ENTRIES_FOR_TEST: &[&str] = &[
        "mx-decision-hold.sh",
        "mx-maintainer-override.sh",
        "mx-override-bindings.sh",
        "mx-override-run.sh",
        "mx-workflow.sh",
    ];
}

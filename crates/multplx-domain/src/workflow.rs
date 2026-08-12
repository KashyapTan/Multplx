//! Constrained workflow definitions, immutable snapshots, and typed run order.
//!
//! The parser intentionally accepts only the version-1 grammar documented in
//! `docs/workflows.md`; it is not a general YAML parser.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use multplx_core::filesystem::atomic_replace;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub workflow_version: u32,
    pub name: String,
    pub description: String,
    pub stages: Vec<Stage>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: StageType,
    pub gate: Gate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor: Option<Executor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fresh_session: Option<bool>,
    pub brief_from: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<Contract>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    pub body: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StageType {
    Interactive,
    Agent,
    Command,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gate {
    Approve,
    Auto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Executor {
    Broker,
    Actor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Contract {
    Output,
    LocalCommits,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Scalar {
    Text(String),
    Bool(bool),
    Integer(u64),
}

#[derive(Clone, Debug, Default)]
struct RawStage {
    fields: BTreeMap<String, Scalar>,
    brief_from: Option<Vec<String>>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct WorkflowError(String);

impl WorkflowError {
    fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

pub type Result<T> = std::result::Result<T, WorkflowError>;

fn strip_comment(value: &str) -> String {
    let mut quote = None;
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if let Some(active) = quote {
            if *byte == active && (index == 0 || bytes[index - 1] != b'\\') {
                quote = None;
            }
            continue;
        }
        if matches!(*byte, b'"' | b'\'') {
            quote = Some(*byte);
            continue;
        }
        if *byte == b'#' && (index == 0 || bytes[index - 1].is_ascii_whitespace()) {
            return value[..index].trim_end().to_owned();
        }
    }
    value.trim_end().to_owned()
}

fn scalar(raw: &str, label: &str) -> Result<Scalar> {
    let value = strip_comment(raw);
    let value = value.trim();
    if value.is_empty() {
        return Err(WorkflowError::new(format!("{label} must not be empty")));
    }
    if value.starts_with(['"', '\'']) {
        let quote = value.as_bytes()[0];
        if value.len() < 2 || value.as_bytes()[value.len() - 1] != quote {
            return Err(WorkflowError::new(format!(
                "{label} has an unterminated quoted scalar"
            )));
        }
        let interior = &value[1..value.len() - 1];
        if interior.as_bytes().iter().enumerate().any(|(index, byte)| {
            *byte == quote && (index == 0 || interior.as_bytes()[index - 1] != b'\\')
        }) {
            return Err(WorkflowError::new(format!(
                "{label} has an unescaped quote inside a quoted scalar"
            )));
        }
        return Ok(Scalar::Text(interior.to_owned()));
    }
    match value {
        "true" => Ok(Scalar::Bool(true)),
        "false" => Ok(Scalar::Bool(false)),
        value if value.bytes().all(|byte| byte.is_ascii_digit()) => value
            .parse::<u64>()
            .map(Scalar::Integer)
            .map_err(|_| WorkflowError::new(format!("{label} is out of range"))),
        value => Ok(Scalar::Text(value.to_owned())),
    }
}

fn inline_list(raw: &str, label: &str) -> Result<Vec<String>> {
    let value = strip_comment(raw);
    let value = value.trim();
    if !value.starts_with('[') || !value.ends_with(']') {
        return Err(WorkflowError::new(format!(
            "{label} must use an inline list"
        )));
    }
    let interior = value[1..value.len() - 1].trim();
    if interior.is_empty() {
        return Ok(Vec::new());
    }
    interior
        .split(',')
        .enumerate()
        .map(
            |(index, entry)| match scalar(entry.trim(), &format!("{label}[{index}]"))? {
                Scalar::Text(value) => Ok(value),
                _ => Err(WorkflowError::new(format!(
                    "{label} must contain stage ids"
                ))),
            },
        )
        .collect()
}

fn text_field(fields: &BTreeMap<String, Scalar>, field: &str, label: &str) -> Result<String> {
    match fields.get(field) {
        Some(Scalar::Text(value)) if !value.is_empty() && !value.contains(['\r', '\n']) => {
            Ok(value.clone())
        }
        _ => Err(WorkflowError::new(format!(
            "{label} must be one non-empty line"
        ))),
    }
}

fn optional_text(
    fields: &BTreeMap<String, Scalar>,
    field: &str,
    label: &str,
) -> Result<Option<String>> {
    fields
        .get(field)
        .map(|_| text_field(fields, field, label))
        .transpose()
}

fn slug(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(WorkflowError::new(format!(
            "{label} must be a privacy-safe slug"
        )));
    }
    Ok(())
}

fn substitutions(value: &str, label: &str, allowed: &[&str], has_output: bool) -> Result<()> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'{' {
            index += 1;
            continue;
        }
        let Some(relative) = value[index + 1..].find('}') else {
            index += 1;
            continue;
        };
        let end = index + 1 + relative;
        let name = &value[index + 1..end];
        if name.contains('{') {
            index += 1;
            continue;
        }
        if !allowed.contains(&name) {
            return Err(WorkflowError::new(format!(
                "{label} uses unknown substitution {{{name}}}"
            )));
        }
        if name == "output" && !has_output {
            return Err(WorkflowError::new(format!(
                "{label} uses {{output}} without declaring output"
            )));
        }
        index = end + 1;
    }
    Ok(())
}

pub fn parse(path: &Path) -> Result<Definition> {
    let metadata = fs::symlink_metadata(path).map_err(|_| {
        WorkflowError::new(format!(
            "definition must be a regular non-symlink file: {}",
            path.display()
        ))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(WorkflowError::new(format!(
            "definition must be a regular non-symlink file: {}",
            path.display()
        )));
    }
    let text = fs::read_to_string(path)
        .map_err(|error| WorkflowError::new(format!("cannot read definition: {error}")))?
        .replace("\r\n", "\n");
    parse_text(&text)
}

pub fn parse_text(text: &str) -> Result<Definition> {
    let lines = text.split('\n').collect::<Vec<_>>();
    if lines.first().copied() != Some("---") {
        return Err(WorkflowError::new(
            "definition must begin with YAML frontmatter",
        ));
    }
    let closing = lines
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, line)| (*line == "---").then_some(index))
        .ok_or_else(|| WorkflowError::new("definition frontmatter has no closing ---"))?;
    let top_allowed = ["workflow_version", "name", "description", "stages"];
    let stage_allowed = [
        "id",
        "title",
        "type",
        "gate",
        "output",
        "executor",
        "fresh_session",
        "brief_from",
        "contract",
        "run",
    ];
    let mut top = BTreeMap::new();
    let mut stages: Vec<RawStage> = Vec::new();
    let mut in_stages = false;
    for (index, original) in lines.iter().enumerate().take(closing).skip(1) {
        let clean = strip_comment(original);
        if clean.trim().is_empty() {
            continue;
        }
        if clean.contains('\t') {
            return Err(WorkflowError::new(format!(
                "frontmatter line {} contains a tab",
                index + 1
            )));
        }
        if !clean.starts_with(' ') {
            let Some((key, raw)) = clean.split_once(':') else {
                return Err(WorkflowError::new(format!(
                    "unsupported frontmatter syntax at line {}",
                    index + 1
                )));
            };
            if key.is_empty()
                || !key.bytes().enumerate().all(|(position, byte)| {
                    if position == 0 {
                        byte.is_ascii_alphabetic() || byte == b'_'
                    } else {
                        byte.is_ascii_alphanumeric() || byte == b'_'
                    }
                })
            {
                return Err(WorkflowError::new(format!(
                    "unsupported frontmatter syntax at line {}",
                    index + 1
                )));
            }
            if !top_allowed.contains(&key) {
                return Err(WorkflowError::new(format!(
                    "unknown top-level field '{key}'"
                )));
            }
            if key == "stages" {
                if !raw.trim().is_empty() {
                    return Err(WorkflowError::new("stages must be a block list"));
                }
                in_stages = true;
                continue;
            }
            if in_stages {
                return Err(WorkflowError::new(format!(
                    "top-level field '{key}' appears after stages"
                )));
            }
            if top.contains_key(key) {
                return Err(WorkflowError::new(format!(
                    "duplicate top-level field '{key}'"
                )));
            }
            top.insert(key.to_owned(), scalar(raw, key)?);
            continue;
        }
        if let Some(rest) = clean.strip_prefix("  - ") {
            if !in_stages {
                return Err(WorkflowError::new(format!(
                    "stage appears before stages at line {}",
                    index + 1
                )));
            }
            let Some((key, raw)) = rest.split_once(':') else {
                return Err(WorkflowError::new(format!(
                    "unsupported frontmatter syntax at line {}",
                    index + 1
                )));
            };
            if key != "id" {
                return Err(WorkflowError::new(format!(
                    "each stage must begin with id at line {}",
                    index + 1
                )));
            }
            let mut stage = RawStage::default();
            stage.fields.insert(
                "id".to_owned(),
                scalar(raw, &format!("stage {} id", stages.len() + 1))?,
            );
            stages.push(stage);
            continue;
        }
        if let Some(rest) = clean.strip_prefix("    ") {
            let Some(stage) = stages.last_mut() else {
                return Err(WorkflowError::new(format!(
                    "stage field appears before a stage id at line {}",
                    index + 1
                )));
            };
            let Some((key, raw)) = rest.split_once(':') else {
                return Err(WorkflowError::new(format!(
                    "unsupported frontmatter syntax at line {}",
                    index + 1
                )));
            };
            if !stage_allowed.contains(&key) {
                return Err(WorkflowError::new(format!("unknown stage field '{key}'")));
            }
            let stage_id = stage
                .fields
                .get("id")
                .and_then(|value| match value {
                    Scalar::Text(value) => Some(value.as_str()),
                    _ => None,
                })
                .unwrap_or("");
            if key == "brief_from" {
                if stage.brief_from.is_some() {
                    return Err(WorkflowError::new(format!(
                        "duplicate field '{key}' in stage {stage_id}"
                    )));
                }
                stage.brief_from = Some(inline_list(raw, &format!("stage {stage_id} brief_from"))?);
            } else {
                if stage.fields.contains_key(key) {
                    return Err(WorkflowError::new(format!(
                        "duplicate field '{key}' in stage {stage_id}"
                    )));
                }
                stage.fields.insert(
                    key.to_owned(),
                    scalar(raw, &format!("stage {stage_id} {key}"))?,
                );
            }
            continue;
        }
        return Err(WorkflowError::new(format!(
            "unsupported frontmatter syntax at line {}",
            index + 1
        )));
    }

    match top.get("workflow_version") {
        Some(Scalar::Integer(1)) => {}
        Some(Scalar::Integer(value)) => {
            return Err(WorkflowError::new(format!(
                "unsupported workflow_version '{value}'"
            )));
        }
        _ => return Err(WorkflowError::new("unsupported workflow_version ''")),
    }
    let name = text_field(&top, "name", "name")?;
    slug(&name, "name")?;
    let description = text_field(&top, "description", "description")?;
    if !in_stages || stages.is_empty() {
        return Err(WorkflowError::new("stages must contain at least one stage"));
    }

    let mut bodies = BTreeMap::new();
    let mut body_id: Option<String> = None;
    let mut body_lines: Vec<String> = Vec::new();
    let finish = |id: Option<String>,
                  lines: &mut Vec<String>,
                  bodies: &mut BTreeMap<String, String>|
     -> Result<()> {
        let Some(id) = id else { return Ok(()) };
        while lines.first().is_some_and(|line| line.trim().is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        let body = lines.join("\n");
        if body.trim().is_empty() {
            return Err(WorkflowError::new(format!(
                "stage body '{id}' must not be empty"
            )));
        }
        if bodies.insert(id.clone(), body).is_some() {
            return Err(WorkflowError::new(format!("duplicate stage body '{id}'")));
        }
        Ok(())
    };
    for (offset, line) in lines.iter().enumerate().skip(closing + 1) {
        if let Some(id) = line
            .strip_prefix("## ")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            finish(body_id.take(), &mut body_lines, &mut bodies)?;
            body_id = Some(id.to_owned());
            body_lines.clear();
        } else if body_id.is_some() {
            body_lines.push((*line).to_owned());
        } else if !line.trim().is_empty() {
            return Err(WorkflowError::new(format!(
                "content before first stage body at line {}",
                offset + 1
            )));
        }
    }
    finish(body_id, &mut body_lines, &mut bodies)?;

    let mut normalized = Vec::new();
    let mut ids = BTreeSet::new();
    let mut prior: Vec<Stage> = Vec::new();
    for (index, raw) in stages.into_iter().enumerate() {
        let id = text_field(&raw.fields, "id", &format!("stage {} id", index + 1))?;
        slug(&id, &format!("stage {} id", index + 1))?;
        if !ids.insert(id.clone()) {
            return Err(WorkflowError::new(format!("duplicate stage id '{id}'")));
        }
        let title = text_field(&raw.fields, "title", &format!("stage {id} title"))?;
        let kind = match text_field(&raw.fields, "type", &format!("stage {id} type"))?.as_str() {
            "interactive" => StageType::Interactive,
            "agent" => StageType::Agent,
            "command" => StageType::Command,
            other => {
                return Err(WorkflowError::new(format!(
                    "stage {id} has unknown type '{other}'"
                )));
            }
        };
        let gate = match text_field(&raw.fields, "gate", &format!("stage {id} gate"))?.as_str() {
            "approve" => Gate::Approve,
            "auto" => Gate::Auto,
            other => {
                return Err(WorkflowError::new(format!(
                    "stage {id} has unknown gate '{other}'"
                )));
            }
        };
        let body = bodies.remove(&id).ok_or_else(|| {
            WorkflowError::new(format!("stage {id} has no matching markdown body"))
        })?;
        let brief_from = raw.brief_from.unwrap_or_default();
        for source in &brief_from {
            slug(source, &format!("stage {id} brief_from entry"))?;
            let Some(source_stage) = prior.iter().find(|stage| &stage.id == source) else {
                return Err(WorkflowError::new(format!(
                    "stage {id} brief_from references non-prior stage '{source}'"
                )));
            };
            if source_stage.output.is_none() {
                return Err(WorkflowError::new(format!(
                    "stage {id} brief_from source '{source}' declares no output"
                )));
            }
        }
        let output = optional_text(&raw.fields, "output", &format!("stage {id} output"))?;
        if let Some(output) = &output {
            let safe = !output.starts_with('/')
                && !output.contains('\\')
                && output.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'.' | b'_' | b'/' | b'{' | b'}' | b'-')
                })
                && !output.split('/').any(|part| part == "..");
            if !safe {
                return Err(WorkflowError::new(format!(
                    "stage {id} output must be a safe relative path"
                )));
            }
            if output == "state/{run}.workflow" || output.starts_with("state/{run}.workflow/") {
                return Err(WorkflowError::new(format!(
                    "stage {id} output cannot target workflow control state"
                )));
            }
            substitutions(output, &format!("stage {id} output"), &["run"], false)?;
        }
        let contract =
            match optional_text(&raw.fields, "contract", &format!("stage {id} contract"))?
                .as_deref()
            {
                None => None,
                Some("output") => Some(Contract::Output),
                Some("local-commits") => Some(Contract::LocalCommits),
                Some(other) => {
                    return Err(WorkflowError::new(format!(
                        "stage {id} has unknown contract '{other}'"
                    )));
                }
            };
        if contract == Some(Contract::Output) && output.is_none() {
            return Err(WorkflowError::new(format!(
                "stage {id} contract output requires an output path"
            )));
        }
        if gate == Gate::Auto
            && kind != StageType::Command
            && output.is_none()
            && contract != Some(Contract::LocalCommits)
        {
            return Err(WorkflowError::new(format!(
                "stage {id} uses auto gate without a verifiable contract"
            )));
        }
        let mut executor = None;
        let mut fresh_session = None;
        let mut run = None;
        match kind {
            StageType::Interactive => {
                if gate != Gate::Approve {
                    return Err(WorkflowError::new(format!(
                        "interactive stage {id} must use gate approve"
                    )));
                }
                for field in ["executor", "fresh_session", "contract", "run"] {
                    if raw.fields.contains_key(field) {
                        return Err(WorkflowError::new(format!(
                            "interactive stage {id} cannot set {field}"
                        )));
                    }
                }
                if !brief_from.is_empty() {
                    return Err(WorkflowError::new(format!(
                        "interactive stage {id} cannot set brief_from"
                    )));
                }
            }
            StageType::Agent => {
                executor =
                    match optional_text(&raw.fields, "executor", &format!("stage {id} executor"))?
                        .as_deref()
                    {
                        Some("broker") => Some(Executor::Broker),
                        Some("actor") => Some(Executor::Actor),
                        _ => {
                            return Err(WorkflowError::new(format!(
                                "agent stage {id} requires executor broker or actor"
                            )));
                        }
                    };
                if raw.fields.contains_key("run") {
                    return Err(WorkflowError::new(format!(
                        "agent stage {id} cannot set run"
                    )));
                }
                if executor == Some(Executor::Broker) && raw.fields.contains_key("fresh_session") {
                    return Err(WorkflowError::new(format!(
                        "broker stage {id} cannot set fresh_session"
                    )));
                }
                if executor == Some(Executor::Broker) && contract == Some(Contract::LocalCommits) {
                    return Err(WorkflowError::new(format!(
                        "broker stage {id} cannot use local-commits"
                    )));
                }
                if executor == Some(Executor::Actor) {
                    fresh_session = match raw.fields.get("fresh_session") {
                        None => Some(false),
                        Some(Scalar::Bool(value)) => Some(*value),
                        _ => {
                            return Err(WorkflowError::new(format!(
                                "stage {id} fresh_session must be true or false"
                            )));
                        }
                    };
                }
            }
            StageType::Command => {
                run = Some(text_field(
                    &raw.fields,
                    "run",
                    &format!("command stage {id} run"),
                )?);
                for field in ["executor", "fresh_session"] {
                    if raw.fields.contains_key(field) {
                        return Err(WorkflowError::new(format!(
                            "command stage {id} cannot set {field}"
                        )));
                    }
                }
                if !brief_from.is_empty() {
                    return Err(WorkflowError::new(format!(
                        "command stage {id} cannot set brief_from"
                    )));
                }
                substitutions(
                    run.as_deref().expect("command"),
                    &format!("stage {id} run"),
                    &["run", "output"],
                    output.is_some(),
                )?;
                if contract == Some(Contract::LocalCommits)
                    && !prior.iter().any(|stage| {
                        stage.kind == StageType::Agent && stage.executor == Some(Executor::Actor)
                    })
                {
                    return Err(WorkflowError::new(format!(
                        "command stage {id} local-commits requires a prior actor stage"
                    )));
                }
            }
        }
        substitutions(
            &body,
            &format!("stage {id} body"),
            &["run", "input", "output"],
            output.is_some(),
        )?;
        let stage = Stage {
            id,
            title,
            kind,
            gate,
            output,
            executor,
            fresh_session,
            brief_from,
            contract,
            run,
            body,
        };
        prior.push(stage.clone());
        normalized.push(stage);
    }
    if let Some(extra) = bodies.keys().next() {
        return Err(WorkflowError::new(format!(
            "markdown body '{extra}' has no matching stage"
        )));
    }
    Ok(Definition {
        workflow_version: 1,
        name,
        description,
        stages: normalized,
    })
}

#[must_use]
pub fn substitute(value: &str, run: &str, input: &str, output: &str) -> String {
    value
        .replace("{run}", run)
        .replace("{input}", input)
        .replace("{output}", output)
}

pub fn output_path(home: &Path, declared: &str, run: &str) -> Result<PathBuf> {
    if declared.is_empty() {
        return Err(WorkflowError::new("output path is empty"));
    }
    let relative = substitute(declared, run, "", "");
    let candidate = Path::new(&relative);
    if candidate.is_absolute()
        || candidate.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(WorkflowError::new(format!(
            "output escapes the Multplx home: {relative}"
        )));
    }
    let home = home
        .canonicalize()
        .map_err(|_| WorkflowError::new(format!("output escapes the Multplx home: {relative}")))?;
    let mut current = home.clone();
    for component in candidate.components() {
        let Component::Normal(part) = component else {
            return Err(WorkflowError::new(format!(
                "output escapes the Multplx home: {relative}"
            )));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(WorkflowError::new(format!(
                    "output escapes the Multplx home: {relative}"
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => {
                return Err(WorkflowError::new(format!(
                    "output escapes the Multplx home: {relative}"
                )));
            }
        }
    }
    Ok(home.join(candidate))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StageStatus {
    Pending,
    Running,
    Ready,
    WaitingAgent,
    WaitingExternal,
    WaitingFailure,
    WaitingApproval,
    Done,
    Passed,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunState {
    definition_ids: Vec<String>,
    order: Vec<String>,
    statuses: BTreeMap<String, StageStatus>,
}

impl RunState {
    #[must_use]
    pub fn new(definition: &Definition) -> Self {
        let ids = definition
            .stages
            .iter()
            .map(|stage| stage.id.clone())
            .collect::<Vec<_>>();
        Self {
            definition_ids: ids.clone(),
            order: ids,
            statuses: BTreeMap::new(),
        }
    }

    pub fn from_records(
        definition: &Definition,
        order: Vec<String>,
        statuses: BTreeMap<String, StageStatus>,
    ) -> Result<Self> {
        let state = Self {
            definition_ids: definition
                .stages
                .iter()
                .map(|stage| stage.id.clone())
                .collect(),
            order,
            statuses,
        };
        state.validate()?;
        Ok(state)
    }

    pub fn validate(&self) -> Result<()> {
        let declared = self.definition_ids.iter().cloned().collect::<BTreeSet<_>>();
        let ordered = self.order.iter().cloned().collect::<BTreeSet<_>>();
        if declared != ordered || self.order.len() != declared.len() {
            return Err(WorkflowError::new(
                "stage order does not match the immutable definition",
            ));
        }
        let mut unmet = false;
        for stage in &self.order {
            match self
                .statuses
                .get(stage)
                .copied()
                .unwrap_or(StageStatus::Pending)
            {
                StageStatus::Skipped => {}
                StageStatus::Passed if unmet => {
                    return Err(WorkflowError::new(format!(
                        "out-of-order passed record for stage {stage}"
                    )));
                }
                StageStatus::Passed => {}
                _ => unmet = true,
            }
        }
        Ok(())
    }

    pub fn skip(&mut self, stage: &str) -> Result<()> {
        if !self.order.iter().any(|value| value == stage) {
            return Err(WorkflowError::new("workflow stage not found"));
        }
        if self
            .statuses
            .get(stage)
            .copied()
            .unwrap_or(StageStatus::Pending)
            != StageStatus::Pending
        {
            return Err(WorkflowError::new("only a pending stage can be skipped"));
        }
        self.statuses.insert(stage.to_owned(), StageStatus::Skipped);
        self.validate()
    }

    pub fn reorder(&mut self, stage: &str, before: &str) -> Result<()> {
        if stage == before {
            return Err(WorkflowError::new("a stage cannot move before itself"));
        }
        for id in [stage, before] {
            if self
                .statuses
                .get(id)
                .copied()
                .unwrap_or(StageStatus::Pending)
                != StageStatus::Pending
            {
                return Err(WorkflowError::new(
                    "reorder requires both stages to remain pending",
                ));
            }
        }
        let from = self
            .order
            .iter()
            .position(|value| value == stage)
            .ok_or_else(|| WorkflowError::new("workflow reorder stage was not found"))?;
        self.order.remove(from);
        let target = self
            .order
            .iter()
            .position(|value| value == before)
            .ok_or_else(|| WorkflowError::new("workflow reorder stage was not found"))?;
        self.order.insert(target, stage.to_owned());
        self.validate()
    }

    #[must_use]
    pub fn next(&self) -> Option<&str> {
        self.order
            .iter()
            .find(|stage| {
                !matches!(
                    self.statuses.get(*stage),
                    Some(StageStatus::Passed | StageStatus::Skipped)
                )
            })
            .map(String::as_str)
    }
}

pub fn create_snapshot(
    definition_path: &Path,
    run_dir: &Path,
    definition: &Definition,
    input: &str,
) -> Result<String> {
    if run_dir.exists() {
        return Err(WorkflowError::new("run id already exists"));
    }
    for child in ["stages", "prompts", "schemas", "agents", "commands"] {
        fs::create_dir_all(run_dir.join(child))
            .map_err(|error| WorkflowError::new(error.to_string()))?;
    }
    let source =
        fs::read(definition_path).map_err(|error| WorkflowError::new(error.to_string()))?;
    atomic_replace(run_dir.join("definition.workflow.md"), &source, 0o600)
        .map_err(|error| WorkflowError::new(error.to_string()))?;
    let normalized = serde_json::to_vec_pretty(definition)
        .map_err(|error| WorkflowError::new(error.to_string()))?;
    let mut normalized_line = normalized;
    normalized_line.push(b'\n');
    atomic_replace(run_dir.join("definition.json"), &normalized_line, 0o600)
        .map_err(|error| WorkflowError::new(error.to_string()))?;
    let order = definition
        .stages
        .iter()
        .map(|stage| stage.id.as_str())
        .collect::<Vec<_>>();
    let mut order_bytes =
        serde_json::to_vec_pretty(&order).map_err(|error| WorkflowError::new(error.to_string()))?;
    order_bytes.push(b'\n');
    atomic_replace(run_dir.join("stage-order.json"), &order_bytes, 0o600)
        .map_err(|error| WorkflowError::new(error.to_string()))?;
    atomic_replace(run_dir.join("input.txt"), input.as_bytes(), 0o600)
        .map_err(|error| WorkflowError::new(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(&source)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "---\nworkflow_version: 1\nname: demo\ndescription: Demo workflow.\nstages:\n  - id: plan\n    title: Plan\n    type: agent\n    executor: broker\n    gate: auto\n    output: data/{run}/plan.md\n    contract: output\n  - id: build\n    title: Build\n    type: agent\n    executor: actor\n    fresh_session: true\n    brief_from: [plan]\n    gate: approve\n    output: data/{run}/build.md\n    contract: local-commits\n---\n\n## plan\n\nPlan {input} into {output}.\n\n## build\n\nBuild {input} from {output}.\n";

    fn one_stage(fields: &str, body: &str) -> String {
        format!(
            "---\nworkflow_version: 1\nname: demo\ndescription: Demo workflow.\nstages:\n  - id: only\n    title: Only\n{fields}---\n\n## only\n\n{body}\n"
        )
    }

    fn assert_invalid(definition: impl AsRef<str>, expected: &str) {
        let error = parse_text(definition.as_ref()).expect_err("invalid definition");
        assert!(
            error.to_string().contains(expected),
            "expected {expected:?}, got {error}"
        );
    }

    #[test]
    fn parser_closes_schema_and_command_substitutions() {
        let definition = parse_text(VALID).expect("valid definition");
        assert_eq!(definition.name, "demo");
        assert_eq!(definition.stages[1].fresh_session, Some(true));
        let unsafe_command = VALID.replace(
            "type: agent\n    executor: broker",
            "type: command\n    run: printf {input}",
        );
        assert!(parse_text(&unsafe_command).is_err());
        assert!(parse_text(&VALID.replace("workflow_version: 1", "workflow_version: 2")).is_err());
    }

    #[test]
    fn run_order_refuses_out_of_order_pass_skip_replay_and_invalid_reorder() {
        let definition = parse_text(VALID).expect("definition");
        let mut state = RunState::new(&definition);
        assert_eq!(state.next(), Some("plan"));
        state.skip("plan").expect("skip pending");
        assert_eq!(state.next(), Some("build"));
        assert!(state.skip("plan").is_err());
        assert!(state.reorder("build", "build").is_err());
        let invalid = BTreeMap::from([("build".to_owned(), StageStatus::Passed)]);
        assert!(
            RunState::from_records(
                &definition,
                vec!["plan".to_owned(), "build".to_owned()],
                invalid
            )
            .is_err()
        );
    }

    #[test]
    fn output_path_rejects_existing_symlink_components() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let outside = temp.path().join("outside");
        fs::create_dir(&home).expect("home");
        fs::create_dir(&outside).expect("outside");
        std::os::unix::fs::symlink(&outside, home.join("data")).expect("symlink");
        assert!(output_path(&home, "data/{run}/result.md", "run-1").is_err());
    }

    #[test]
    fn snapshot_is_private_and_definition_digest_is_immutable_input() {
        let temp = tempfile::tempdir().expect("tempdir");
        let definition_path = temp.path().join("demo.workflow.md");
        fs::write(&definition_path, VALID).expect("definition");
        let definition = parse(&definition_path).expect("parse");
        let run = temp.path().join("state/run.workflow");
        let digest =
            create_snapshot(&definition_path, &run, &definition, "exact input").expect("snapshot");
        assert_eq!(digest, format!("{:x}", Sha256::digest(VALID.as_bytes())));
        assert_eq!(
            fs::read_to_string(run.join("input.txt")).expect("input"),
            "exact input"
        );
        assert!(create_snapshot(&definition_path, &run, &definition, "again").is_err());
    }

    #[test]
    fn parser_rejects_every_structural_frontmatter_class() {
        assert_invalid("workflow_version: 1", "begin with YAML frontmatter");
        assert_invalid("---\nworkflow_version: 1", "no closing");
        assert_invalid(
            VALID.replace("workflow_version: 1", "workflow_version:\t1"),
            "contains a tab",
        );
        assert_invalid(
            VALID.replace("name: demo", "unknown: demo"),
            "unknown top-level field",
        );
        assert_invalid(
            VALID.replace("name: demo", "name: demo\nname: again"),
            "duplicate top-level field",
        );
        assert_invalid(
            VALID.replace("stages:", "stages: inline"),
            "stages must be a block list",
        );
        assert_invalid(
            VALID.replace(
                "description: Demo workflow.\nstages:",
                "stages:\ndescription: Demo workflow.",
            ),
            "appears after stages",
        );
        assert_invalid(
            VALID.replace("  - id: plan", "  - title: plan"),
            "each stage must begin with id",
        );
        assert_invalid(
            VALID.replace("    title: Plan", "    mystery: Plan"),
            "unknown stage field",
        );
        assert_invalid(
            VALID.replace("    title: Plan", "    title: Plan\n    title: Again"),
            "duplicate field",
        );
        assert_invalid(
            VALID.replace("workflow_version: 1", "workflow_version: false"),
            "unsupported workflow_version",
        );
        assert_invalid(
            VALID.replace("workflow_version: 1", "workflow_version: 9"),
            "unsupported workflow_version '9'",
        );
        assert_invalid(
            VALID.replace("name: demo", "name: ../demo"),
            "privacy-safe slug",
        );
        assert_invalid(
            "---\nworkflow_version: 1\nname: demo\ndescription: Demo.\nstages:\n---\n",
            "at least one stage",
        );
    }

    #[test]
    fn parser_rejects_body_binding_and_stage_semantic_failures() {
        assert_invalid(
            VALID.replace("## plan\n\nPlan", "text before body\n\n## plan\n\nPlan"),
            "content before first stage body",
        );
        assert_invalid(
            VALID.replace("Plan {input} into {output}.", "   "),
            "must not be empty",
        );
        assert_invalid(
            VALID.replace("## build", "## plan\n\nDuplicate.\n\n## build"),
            "duplicate stage body",
        );
        assert_invalid(
            VALID.replace("  - id: build", "  - id: plan"),
            "duplicate stage id",
        );
        assert_invalid(
            VALID.replace("## build", "## extra"),
            "has no matching markdown body",
        );
        assert_invalid(
            VALID.replace("    type: agent", "    type: mystery"),
            "unknown type",
        );
        assert_invalid(
            VALID.replace("    gate: auto", "    gate: mystery"),
            "unknown gate",
        );
        assert_invalid(
            VALID.replace("brief_from: [plan]", "brief_from: [later]"),
            "references non-prior stage",
        );
        assert_invalid(
            "---\nworkflow_version: 1\nname: demo\ndescription: Demo.\nstages:\n  - id: source\n    title: Source\n    type: interactive\n    gate: approve\n  - id: sink\n    title: Sink\n    type: agent\n    executor: actor\n    gate: approve\n    brief_from: [source]\n---\n\n## source\n\nChoose.\n\n## sink\n\nBuild.\n",
            "declares no output",
        );
        assert_invalid(
            VALID.replace("data/{run}/plan.md", "../outside.md"),
            "safe relative path",
        );
        assert_invalid(
            VALID.replace("data/{run}/plan.md", "state/{run}.workflow/control"),
            "workflow control state",
        );
        assert_invalid(
            VALID.replace("data/{run}/plan.md", "data/{input}/plan.md"),
            "unknown substitution",
        );
        assert_invalid(
            VALID.replace("contract: output", "contract: mystery"),
            "unknown contract",
        );
        assert_invalid(
            one_stage(
                "    type: agent\n    executor: broker\n    gate: auto\n",
                "Work.",
            ),
            "without a verifiable contract",
        );
    }

    #[test]
    fn parser_covers_each_stage_executor_constraint() {
        let interactive = one_stage(
            "    type: interactive\n    gate: approve\n",
            "Approve {input}.",
        );
        assert_eq!(
            parse_text(&interactive).expect("interactive").stages[0].kind,
            StageType::Interactive
        );
        assert_invalid(
            one_stage(
                "    type: interactive\n    gate: auto\n    output: data/{run}/approval.md\n",
                "Approve.",
            ),
            "must use gate approve",
        );
        assert_invalid(
            one_stage(
                "    type: interactive\n    gate: approve\n    executor: broker\n",
                "Approve.",
            ),
            "cannot set executor",
        );
        assert_invalid(
            "---\nworkflow_version: 1\nname: demo\ndescription: Demo.\nstages:\n  - id: source\n    title: Source\n    type: agent\n    executor: broker\n    gate: approve\n    output: data/{run}/source.md\n  - id: approval\n    title: Approval\n    type: interactive\n    gate: approve\n    brief_from: [source]\n---\n\n## source\n\nWork.\n\n## approval\n\nApprove.\n",
            "interactive stage approval cannot set brief_from",
        );
        assert_invalid(
            "---\nworkflow_version: 1\nname: demo\ndescription: Demo.\nstages:\n  - id: source\n    title: Source\n    type: agent\n    executor: broker\n    gate: approve\n    output: data/{run}/source.md\n    contract: local-commits\n---\n\n## source\n\nWork.\n",
            "cannot use local-commits",
        );
        assert_invalid(
            one_stage("    type: agent\n    gate: approve\n", "Work."),
            "requires executor broker or actor",
        );
        assert_invalid(
            one_stage(
                "    type: agent\n    executor: broker\n    gate: approve\n    run: true\n",
                "Work.",
            ),
            "cannot set run",
        );
        assert_invalid(
            one_stage(
                "    type: agent\n    executor: broker\n    fresh_session: true\n    gate: approve\n",
                "Work.",
            ),
            "cannot set fresh_session",
        );
        assert_invalid(
            one_stage(
                "    type: agent\n    executor: actor\n    fresh_session: maybe\n    gate: approve\n",
                "Work.",
            ),
            "fresh_session must be true or false",
        );
        let actor = one_stage(
            "    type: agent\n    executor: actor\n    gate: approve\n",
            "Work.",
        );
        assert_eq!(
            parse_text(&actor).expect("actor default").stages[0].fresh_session,
            Some(false)
        );
        let command = one_stage(
            "    type: command\n    gate: auto\n    run: printf ok\n",
            "Run for {run}.",
        );
        assert_eq!(
            parse_text(&command).expect("command").stages[0]
                .run
                .as_deref(),
            Some("printf ok")
        );
        assert_invalid(
            one_stage(
                "    type: command\n    gate: auto\n    run: printf {input}\n",
                "Run.",
            ),
            "unknown substitution",
        );
        assert_invalid(
            one_stage(
                "    type: command\n    gate: auto\n    executor: actor\n    run: printf ok\n",
                "Run.",
            ),
            "cannot set executor",
        );
        assert_invalid(
            "---\nworkflow_version: 1\nname: demo\ndescription: Demo.\nstages:\n  - id: first\n    title: First\n    type: command\n    gate: auto\n    output: data/{run}/first.md\n    run: printf ok\n  - id: second\n    title: Second\n    type: command\n    gate: auto\n    brief_from: [first]\n    run: printf ok\n---\n\n## first\n\nFirst.\n\n## second\n\nSecond.\n",
            "cannot set brief_from",
        );
        assert_invalid(
            one_stage(
                "    type: command\n    gate: auto\n    contract: local-commits\n    run: printf ok\n",
                "Run.",
            ),
            "requires a prior actor stage",
        );
        assert_invalid(
            one_stage(
                "    type: interactive\n    gate: approve\n",
                "Use {output}.",
            ),
            "without declaring output",
        );
        assert_invalid(
            one_stage(
                "    type: command\n    gate: auto\n    contract: output\n    run: printf ok\n",
                "Run.",
            ),
            "requires an output path",
        );
        assert_invalid(
            format!("{VALID}\n## extra\n\nUnexpected.\n"),
            "markdown body 'extra' has no matching stage",
        );
    }

    #[test]
    fn paths_records_and_snapshot_failures_are_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home");
        assert!(output_path(&home, "", "run").is_err());
        assert!(output_path(&home, "/tmp/out", "run").is_err());
        assert!(output_path(&home, "../out", "run").is_err());
        assert_eq!(
            output_path(&home, "data/{run}/out.md", "run-2").expect("safe output"),
            home.canonicalize()
                .expect("canonical home")
                .join("data/run-2/out.md")
        );
        assert!(parse(&home).is_err());
        assert!(parse(&home.join("missing.workflow.md")).is_err());

        let definition = parse_text(VALID).expect("definition");
        let statuses = BTreeMap::from([
            ("plan".to_owned(), StageStatus::Passed),
            ("build".to_owned(), StageStatus::Ready),
        ]);
        let state = RunState::from_records(
            &definition,
            vec!["plan".to_owned(), "build".to_owned()],
            statuses,
        )
        .expect("ordered records");
        assert_eq!(state.next(), Some("build"));
        assert!(
            RunState::from_records(&definition, vec!["plan".to_owned()], BTreeMap::new()).is_err()
        );
        let mut state = RunState::new(&definition);
        assert!(state.skip("missing").is_err());
        assert!(state.reorder("missing", "build").is_err());
        assert!(state.reorder("plan", "missing").is_err());

        let missing = temp.path().join("missing.workflow.md");
        assert!(
            create_snapshot(
                &missing,
                &temp.path().join("run.workflow"),
                &definition,
                "input"
            )
            .is_err()
        );
    }
}

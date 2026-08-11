//! Byte-compatible Markdown backlog parsing, rendering, and transactions.
//!
//! Parsing, validation, pure mutation, and publication are deliberately
//! separate. Read-only operations never lock or repair malformed input.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use multplx_core::filesystem::atomic_replace;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use regex::Regex;
use time::OffsetDateTime;

const SECTIONS: [&str; 3] = ["In flight", "Queued", "Done"];
const SCAFFOLD: &str = "## In flight\n\n## Queued\n\n## Done\n";

pub struct AddRequest<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub repo: &'a str,
    pub kind: &'a str,
    pub body: &'a str,
    pub start: bool,
    pub blockers: &'a [String],
}

/// Backlog failure without the stable `mx-backlog:` CLI prefix.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct BacklogError {
    message: String,
}

impl BacklogError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// CLI-level failure with its compatibility exit status.
#[derive(Debug)]
pub struct CliFailure {
    pub code: i32,
    pub message: String,
    pub usage: bool,
}

impl From<BacklogError> for CliFailure {
    fn from(error: BacklogError) -> Self {
        Self {
            code: 1,
            message: error.to_string(),
            usage: false,
        }
    }
}

#[derive(Clone, Debug)]
struct Record {
    line: String,
    newline: bool,
}

impl Record {
    fn render(&self) -> String {
        if self.newline {
            format!("{}\n", self.line)
        } else {
            self.line.clone()
        }
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub checked: bool,
    pub section: String,
    pub state: String,
    pub title: String,
    pub metadata: HashMap<String, String>,
    pub blockers: Vec<String>,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
pub struct Backlog {
    path: PathBuf,
    text: String,
    records: Vec<Record>,
    heading_indexes: HashMap<String, usize>,
    pub items: Vec<Item>,
    by_id: HashMap<String, usize>,
}

impl Backlog {
    /// Parse the current Markdown grammar from exact bytes.
    pub fn parse(path: impl Into<PathBuf>, bytes: &[u8]) -> Result<Self, BacklogError> {
        let path = path.into();
        let text = String::from_utf8_lossy(bytes).into_owned();
        let records = records(&text);
        let heading = Regex::new(r"^##\s+(.+?)\s*$").expect("heading regex");
        let item_start = Regex::new(r"^- \[[ x]\] ").expect("item regex");
        let mut heading_indexes = HashMap::new();
        let mut heading_order = Vec::new();
        for (index, record) in records.iter().enumerate() {
            let Some(captures) = heading.captures(&record.line) else {
                continue;
            };
            let name = captures.get(1).expect("heading name").as_str();
            if !SECTIONS.contains(&name) {
                return Err(BacklogError::new(format!(
                    "unknown backlog section \"{name}\" in {}",
                    path.display()
                )));
            }
            if heading_indexes.insert(name.to_owned(), index).is_some() {
                return Err(BacklogError::new(format!(
                    "duplicate backlog section \"{name}\" in {}",
                    path.display()
                )));
            }
            heading_order.push(name.to_owned());
        }
        for section in SECTIONS {
            if !heading_indexes.contains_key(section) {
                return Err(BacklogError::new(format!(
                    "missing backlog section \"## {section}\" in {}",
                    path.display()
                )));
            }
        }
        let mut items = Vec::new();
        let mut by_id = HashMap::new();
        for (section_number, section) in heading_order.iter().enumerate() {
            let mut index = heading_indexes[section] + 1;
            let end = heading_order
                .get(section_number + 1)
                .map_or(records.len(), |next| heading_indexes[next]);
            while index < end {
                let line = &records[index].line;
                if line.is_empty() {
                    index += 1;
                    continue;
                }
                if !item_start.is_match(line) {
                    if line.starts_with([' ', '\t']) {
                        return Err(BacklogError::new(format!(
                            "orphaned or non-2-space continuation at {}:{}: {line}",
                            path.display(),
                            index + 1
                        )));
                    }
                    return Err(BacklogError::new(format!(
                        "truncated or unrecognized backlog content at {}:{}: {line}",
                        path.display(),
                        index + 1
                    )));
                }
                let item_start_index = index;
                let mut item = parse_header(line, section)?;
                index += 1;
                while index < end && !item_start.is_match(&records[index].line) {
                    let body = &records[index].line;
                    if body.is_empty() || body.starts_with("  ") {
                        index += 1;
                        continue;
                    }
                    if heading.is_match(body) {
                        break;
                    }
                    return Err(BacklogError::new(format!(
                        "non-2-space continuation at {}:{}: {body}",
                        path.display(),
                        index + 1
                    )));
                }
                item.start = item_start_index;
                item.end = index;
                if by_id.insert(item.id.clone(), items.len()).is_some() {
                    return Err(BacklogError::new(format!(
                        "duplicate backlog item id \"{}\" in {}",
                        item.id,
                        path.display()
                    )));
                }
                items.push(item);
            }
        }
        Ok(Self {
            path,
            text,
            records,
            heading_indexes,
            items,
            by_id,
        })
    }

    fn item(&self, id: &str) -> Option<&Item> {
        self.by_id.get(id).map(|index| &self.items[*index])
    }

    fn body_parts(&self, item: &Item) -> (Vec<Record>, Vec<Record>) {
        let content = self.records[item.start + 1..item.end].to_vec();
        let mut body_end = content.len();
        while body_end > 0 && content[body_end - 1].line.is_empty() {
            body_end -= 1;
        }
        (content[..body_end].to_vec(), content[body_end..].to_vec())
    }

    fn body_text(&self, item: &Item) -> String {
        self.body_parts(item)
            .0
            .iter()
            .map(|record| {
                if record.line.is_empty() {
                    String::new()
                } else {
                    record.line[2..].to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn item_block(&self, item: &Item) -> Vec<Record> {
        let (body, _) = self.body_parts(item);
        let mut block = vec![self.records[item.start].clone()];
        block.extend(body);
        block
    }

    fn replace_range(&self, start: usize, end: usize, replacement: &[Record]) -> String {
        render_records(
            &self.records[..start]
                .iter()
                .chain(replacement)
                .chain(&self.records[end..])
                .cloned()
                .collect::<Vec<_>>(),
        )
    }

    fn remove_items(&self, selected: &HashSet<String>) -> String {
        let mut ranges = selected
            .iter()
            .filter_map(|id| self.item(id))
            .map(|item| (item.start, item.end))
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| std::cmp::Reverse(range.0));
        let mut records = self.records.clone();
        for (start, end) in ranges {
            records.drain(start..end);
        }
        let mut index = 0;
        while index + 1 < records.len() {
            if records[index].line.starts_with("## ") && records[index + 1].line.starts_with("## ")
            {
                records.insert(
                    index + 1,
                    Record {
                        line: String::new(),
                        newline: true,
                    },
                );
                index += 1;
            }
            index += 1;
        }
        render_records(&records)
    }

    fn insert_blocks(&self, section: &str, blocks: &[Vec<Record>], at_start: bool) -> String {
        let insertion = if at_start {
            self.heading_indexes[section] + 1
        } else {
            let start = self.heading_indexes[section];
            let mut end = self
                .heading_indexes
                .values()
                .copied()
                .filter(|value| *value > start)
                .min()
                .unwrap_or(self.records.len());
            while end > start + 1 && self.records[end - 1].line.is_empty() {
                end -= 1;
            }
            end
        };
        let mut normalized = Vec::new();
        for block in blocks {
            for record in block {
                normalized.push(Record {
                    line: record.line.clone(),
                    newline: true,
                });
            }
        }
        if insertion >= self.records.len() || !self.records[insertion].line.is_empty() {
            normalized.push(Record {
                line: String::new(),
                newline: true,
            });
        }
        self.replace_range(insertion, insertion, &normalized)
    }
}

fn records(text: &str) -> Vec<Record> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::new();
    for part in text.split_inclusive('\n') {
        output.push(Record {
            line: part.strip_suffix('\n').unwrap_or(part).to_owned(),
            newline: part.ends_with('\n'),
        });
    }
    output
}

fn render_records(records: &[Record]) -> String {
    records.iter().map(Record::render).collect()
}

fn parse_header(line: &str, section: &str) -> Result<Item, BacklogError> {
    let pattern = Regex::new(r"^- \[([ x])\] ([^\s]+)(?:\s+(.*))?$").expect("header regex");
    let Some(captures) = pattern.captures(line) else {
        return Err(BacklogError::new(format!(
            "invalid item header in {section}: {line}"
        )));
    };
    let id = captures.get(2).expect("id").as_str().to_owned();
    if !valid_id(&id) {
        return Err(BacklogError::new(format!("invalid item id: {id}")));
    }
    let remainder = captures.get(3).map_or("", |capture| capture.as_str());
    let mut metadata = HashMap::new();
    for field in [
        "repo",
        "kind",
        "since",
        "hold",
        "hold-kind",
        "report",
        "note",
        "pr",
    ] {
        let pattern =
            Regex::new(&format!(r"\({}: ([^)]*)\)", regex::escape(field))).expect("metadata regex");
        if let Some(value) = pattern.captures(remainder) {
            metadata.insert(
                field.replace('-', "_"),
                value.get(1).expect("metadata value").as_str().to_owned(),
            );
        }
    }
    let blocker_pattern =
        Regex::new(r"(?:^|\s)blocked-by:\s*([A-Za-z0-9._-]+)").expect("blocker regex");
    let blockers = blocker_pattern
        .captures_iter(remainder)
        .map(|captures| captures.get(1).expect("blocker").as_str().to_owned())
        .collect();
    let mut title = remainder.trim_start_matches('-').trim_start().to_owned();
    let mut boundaries = [
        " (repo: ",
        " (kind: ",
        " (since ",
        " (since: ",
        " (hold: ",
        " (hold-kind: ",
        " (report: ",
        " (note: ",
        " (pr: ",
    ]
    .iter()
    .filter_map(|needle| title.find(needle))
    .collect::<Vec<_>>();
    if let Some(found) = Regex::new(r"\sblocked-by:\s*")
        .expect("boundary regex")
        .find(&title)
    {
        boundaries.push(found.start());
    }
    if let Some(boundary) = boundaries.into_iter().min() {
        title.truncate(boundary);
    }
    title = Regex::new(r"\s+-\s*$")
        .expect("trailing dash regex")
        .replace(&title, "")
        .trim()
        .to_owned();
    Ok(Item {
        id,
        checked: captures.get(1).expect("check").as_str() == "x",
        section: section.to_owned(),
        state: section.to_lowercase().replace(' ', "_"),
        title,
        metadata,
        blockers,
        start: 0,
        end: 0,
    })
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn canonical_body(body: &str) -> Vec<Record> {
    if body.is_empty() {
        return Vec::new();
    }
    body.split('\n')
        .map(|line| Record {
            line: if line.is_empty() {
                String::new()
            } else {
                format!("  {line}")
            },
            newline: true,
        })
        .collect()
}

fn csv(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serialize string")
}

fn now_iso() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.nanosecond() / 1_000_000
    )
}

fn append_archive(existing: &str, heading: &str, content: &str) -> String {
    let mut output = if existing.is_empty() {
        "# Backlog archive\n".to_owned()
    } else {
        existing.to_owned()
    };
    if !output.ends_with('\n') {
        output.push('\n');
    }
    if !output.ends_with("\n\n") {
        output.push('\n');
    }
    output.push_str(&format!("## {heading}\n\n{content}"));
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

#[derive(Debug)]
struct Opened {
    backlog: Backlog,
    mode: u32,
}

fn open(path: &Path, allow_missing: bool) -> Result<Opened, BacklogError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(BacklogError::new(format!(
                    "backlog must not be a symlink: {}",
                    path.display()
                )));
            }
            if !metadata.is_file() {
                return Err(BacklogError::new(format!(
                    "backlog is not a regular file: {}",
                    path.display()
                )));
            }
            let bytes = fs::read(path).map_err(|error| {
                BacklogError::new(format!("cannot read backlog {}: {error}", path.display()))
            })?;
            Ok(Opened {
                backlog: Backlog::parse(path, &bytes)?,
                mode: metadata.mode() & 0o777,
            })
        }
        Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => Ok(Opened {
            backlog: Backlog::parse(path, SCAFFOLD.as_bytes())?,
            mode: 0o644,
        }),
        Err(error) => Err(BacklogError::new(format!(
            "cannot read backlog {}: {error}",
            path.display()
        ))),
    }
}

fn ensure_parent(path: &Path) -> Result<(), BacklogError> {
    let parent = path
        .parent()
        .ok_or_else(|| BacklogError::new("backlog path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| {
        BacklogError::new(format!(
            "cannot create backlog parent {}: {error}",
            parent.display()
        ))
    })
}

fn publish(path: &Path, text: &str, mode: u32) -> Result<(), BacklogError> {
    ensure_parent(path)?;
    atomic_replace(path, text.as_bytes(), mode)
        .map_err(|error| BacklogError::new(error.to_string()))
}

fn lock_paths(paths: &[PathBuf]) -> Result<Vec<DirectoryLock>, BacklogError> {
    let unique = paths.iter().cloned().collect::<BTreeSet<_>>();
    let mut locks = Vec::new();
    for path in unique {
        let lock = PathBuf::from(format!("{}.mx-lock", path.display()));
        ensure_parent(&lock)?;
        match DirectoryLock::acquire_wait(
            &lock,
            &SystemProcessProbe::default(),
            Duration::from_millis(250),
        ) {
            Ok(acquired) => locks.push(acquired),
            Err(_) => {
                return Err(BacklogError::new(format!(
                    "backlog is busy: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(locks)
}

fn publish_two(
    first: &Opened,
    first_text: &str,
    second: &Opened,
    second_text: &str,
) -> Result<(), BacklogError> {
    publish_two_with_fault(
        first,
        first_text,
        second,
        second_text,
        std::env::var("MX_BACKLOG_TEST_FAIL_SECOND_PUBLISH").as_deref() == Ok("1"),
    )
}

fn publish_two_with_fault(
    first: &Opened,
    first_text: &str,
    second: &Opened,
    second_text: &str,
    fail_second: bool,
) -> Result<(), BacklogError> {
    let first_path = &first.backlog.path;
    let second_path = &second.backlog.path;
    publish(first_path, first_text, first.mode)?;
    let second_result = if fail_second {
        Err(BacklogError::new("injected second publish failure"))
    } else {
        publish(second_path, second_text, second.mode)
    };
    if let Err(error) = second_result {
        let rollback = publish(first_path, &first.backlog.text, first.mode);
        if rollback.is_err() {
            return Err(BacklogError::new(format!(
                "two-file backlog transaction failed and rollback also failed: {error}"
            )));
        }
        return Err(BacklogError::new(format!(
            "two-file backlog transaction failed and was rolled back: {error}"
        )));
    }
    Ok(())
}

/// Store one validated backlog path.
pub struct BacklogStore {
    path: PathBuf,
}

impl BacklogStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn validate(&self) -> Result<(), BacklogError> {
        open(&self.path, false).map(|_| ())
    }

    pub fn list(&self, limit: usize) -> Result<String, BacklogError> {
        if limit == 0 {
            return Err(BacklogError::new("list limit must be a positive integer"));
        }
        let opened = open(&self.path, false)?;
        let selected = opened.backlog.items.iter().take(limit).collect::<Vec<_>>();
        let mut output = format!(
            "tasks[{}]{{id,state,kind,repo,title,blocked_by,hold_kind,hold_reason}}:\n",
            selected.len()
        );
        for item in selected {
            let fields = [
                item.id.clone(),
                item.state.clone(),
                item.metadata
                    .get("kind")
                    .cloned()
                    .unwrap_or_else(|| "-".to_owned()),
                item.metadata
                    .get("repo")
                    .cloned()
                    .unwrap_or_else(|| "-".to_owned()),
                item.title.clone(),
                if item.blockers.is_empty() {
                    "none".to_owned()
                } else {
                    item.blockers.join(",")
                },
                item.metadata
                    .get("hold_kind")
                    .cloned()
                    .unwrap_or_else(|| "-".to_owned()),
                item.metadata
                    .get("hold")
                    .cloned()
                    .unwrap_or_else(|| "-".to_owned()),
            ];
            output.push_str("  ");
            output.push_str(
                &fields
                    .iter()
                    .map(|field| csv(field))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            output.push('\n');
        }
        if opened.backlog.items.len() > limit {
            output.push_str(&format!(
                "(truncated {} item(s))\n",
                opened.backlog.items.len() - limit
            ));
        }
        Ok(output)
    }

    pub fn show(&self, id: &str) -> Result<String, BacklogError> {
        let opened = open(&self.path, false)?;
        let item = opened
            .backlog
            .item(id)
            .ok_or_else(|| BacklogError::new(format!("backlog item not found: {id}")))?;
        let metadata = &item.metadata;
        let blockers = item.blockers.join(",");
        Ok(format!(
            "{id}:\n  state: {}\n  title: {}\n  kind: {}\n  repo: {}\n  held: {}\n  blocked: {}\n  blocked_by: {}\n  hold_kind: {}\n  hold_reason: {}\n  body: {}\n",
            item.state,
            item.title,
            metadata.get("kind").map_or("", String::as_str),
            metadata.get("repo").map_or("", String::as_str),
            if item.state == "queued" && metadata.contains_key("hold") {
                "yes"
            } else {
                "no"
            },
            if blockers.is_empty() { "no" } else { "yes" },
            if blockers.contains(',') {
                json_string(&blockers)
            } else {
                blockers
            },
            metadata.get("hold_kind").map_or("", String::as_str),
            metadata.get("hold").map_or("", String::as_str),
            json_string(&opened.backlog.body_text(item)),
        ))
    }

    pub fn ready(&self) -> Result<String, BacklogError> {
        let opened = open(&self.path, false)?;
        let done = opened
            .backlog
            .items
            .iter()
            .filter(|item| item.state == "done")
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        let mut output = String::new();
        for item in &opened.backlog.items {
            if item.state == "queued"
                && !item.metadata.contains_key("hold")
                && item
                    .blockers
                    .iter()
                    .all(|blocker| done.contains(blocker.as_str()))
            {
                output.push_str(&item.id);
                output.push('\n');
            }
        }
        Ok(output)
    }

    fn mutate(
        &self,
        callback: impl FnOnce(&Backlog) -> Result<String, BacklogError>,
    ) -> Result<(), BacklogError> {
        let _locks = lock_paths(std::slice::from_ref(&self.path))?;
        let opened = open(&self.path, false)?;
        let next = callback(&opened.backlog)?;
        if next != opened.backlog.text {
            publish(&self.path, &next, opened.mode)?;
        }
        Ok(())
    }

    pub fn add(&self, request: &AddRequest<'_>) -> Result<(), BacklogError> {
        let AddRequest {
            id,
            title,
            repo,
            kind,
            body,
            start,
            blockers,
        } = request;
        if !valid_id(id) {
            return Err(BacklogError::new(format!("invalid item id: {id}")));
        }
        if title.is_empty() || title.contains(['\r', '\n']) {
            return Err(BacklogError::new("title must be one non-empty line"));
        }
        if blockers.iter().any(|blocker| !valid_id(blocker)) {
            return Err(BacklogError::new("--blocked-by requires a valid id"));
        }
        self.mutate(|backlog| {
            if backlog.item(id).is_some() {
                return Err(BacklogError::new(format!(
                    "backlog item already exists: {id}"
                )));
            }
            let section = if *start { "In flight" } else { "Queued" };
            let header = format!(
                "- [ ] {id} - {title}{} (repo: {repo}) (kind: {kind})",
                blockers
                    .iter()
                    .map(|blocker| format!(" blocked-by: {blocker}"))
                    .collect::<String>()
            );
            let mut block = vec![Record {
                line: header,
                newline: true,
            }];
            block.extend(canonical_body(body));
            Ok(backlog.insert_blocks(section, &[block], false))
        })
    }

    pub fn hold(&self, id: &str, reason: &str, kind: &str) -> Result<(), BacklogError> {
        if reason.contains(['\r', '\n', '(', ')']) {
            return Err(BacklogError::new(
                "hold reason must be one line without parentheses",
            ));
        }
        self.mutate(|backlog| {
            let item = backlog
                .item(id)
                .ok_or_else(|| BacklogError::new(format!("backlog item not found: {id}")))?;
            if item.state == "done" {
                return Err(BacklogError::new(format!("cannot hold done item: {id}")));
            }
            let metadata = Regex::new(r"\s+\(hold(?:-kind)?: [^)]*\)").expect("hold regex");
            let mut header = metadata
                .replace_all(&backlog.records[item.start].line, "")
                .into_owned();
            header.push_str(&format!(" (hold: {reason}) (hold-kind: {kind})"));
            Ok(backlog.replace_range(
                item.start,
                item.start + 1,
                &[Record {
                    line: header,
                    newline: true,
                }],
            ))
        })
    }

    pub fn block(&self, id: &str, blocker: &str) -> Result<(), BacklogError> {
        if !valid_id(blocker) {
            return Err(BacklogError::new("--by requires a valid blocker id"));
        }
        self.mutate(|backlog| {
            let item = backlog
                .item(id)
                .ok_or_else(|| BacklogError::new(format!("backlog item not found: {id}")))?;
            if item.state == "done" {
                return Err(BacklogError::new(format!("cannot block done item: {id}")));
            }
            if item.blockers.iter().any(|value| value == blocker) {
                return Ok(backlog.text.clone());
            }
            let header = format!("{} blocked-by: {blocker}", backlog.records[item.start].line);
            Ok(backlog.replace_range(
                item.start,
                item.start + 1,
                &[Record {
                    line: header,
                    newline: true,
                }],
            ))
        })
    }

    pub fn unblock(&self, id: &str, blocker: &str) -> Result<(), BacklogError> {
        self.mutate(|backlog| {
            let item = backlog
                .item(id)
                .ok_or_else(|| BacklogError::new(format!("backlog item not found: {id}")))?;
            if !item.blockers.iter().any(|value| value == blocker) {
                return Err(BacklogError::new(format!(
                    "{id} is not blocked by {blocker}"
                )));
            }
            if std::env::var("MX_BACKLOG_TEST_FAIL_UNBLOCK_ID").as_deref() == Ok(id) {
                let marker = std::env::var_os("MX_BACKLOG_TEST_FAIL_UNBLOCK_ONCE_FILE");
                let should_fail = marker.as_ref().is_none_or(|path| !Path::new(path).exists());
                if should_fail {
                    if let Some(path) = marker {
                        let _ = fs::write(path, b"failed once\n");
                    }
                    return Err(BacklogError::new(format!(
                        "injected unblock failure for {id}"
                    )));
                }
            }
            let pattern = Regex::new(&format!(
                r"\s+blocked-by:\s*{}(\s|$)",
                regex::escape(blocker)
            ))
            .expect("unblock regex");
            let header = pattern
                .replace(&backlog.records[item.start].line, "$1")
                .into_owned();
            Ok(backlog.replace_range(
                item.start,
                item.start + 1,
                &[Record {
                    line: header,
                    newline: true,
                }],
            ))
        })
    }

    pub fn update(&self, id: &str, body: &str, archive_body: bool) -> Result<(), BacklogError> {
        let archive = archive_path(&self.path);
        let paths = if archive_body {
            vec![self.path.clone(), archive.clone()]
        } else {
            vec![self.path.clone()]
        };
        let _locks = lock_paths(&paths)?;
        let opened = open(&self.path, false)?;
        let item = opened
            .backlog
            .item(id)
            .ok_or_else(|| BacklogError::new(format!("backlog item not found: {id}")))?;
        let (_, separators) = opened.backlog.body_parts(item);
        let mut replacement = vec![opened.backlog.records[item.start].clone()];
        replacement.extend(canonical_body(body));
        replacement.extend(separators);
        let next = opened
            .backlog
            .replace_range(item.start, item.end, &replacement);
        if !archive_body {
            return publish(&self.path, &next, opened.mode);
        }
        let archive_opened = open_archive(&archive)?;
        let old_body = opened.backlog.body_text(item);
        let archived = append_archive(
            &archive_opened.backlog.text,
            &format!("Superseded body: {id} ({})", now_iso()),
            &render_records(&canonical_body(&old_body)),
        );
        publish_two(&opened, &next, &archive_opened, &archived)
    }

    pub fn done(
        &self,
        id: &str,
        artifact: Option<(&str, &str)>,
        keep: usize,
    ) -> Result<(), BacklogError> {
        let archive = archive_path(&self.path);
        let _locks = lock_paths(&[self.path.clone(), archive.clone()])?;
        let opened = open(&self.path, false)?;
        let item = opened
            .backlog
            .item(id)
            .ok_or_else(|| BacklogError::new(format!("backlog item not found: {id}")))?;
        if item.state == "done" {
            return Ok(());
        }
        let mut header = opened.backlog.records[item.start]
            .line
            .replacen("- [ ]", "- [x]", 1);
        let metadata = Regex::new(r"\s+\(hold(?:-kind)?: [^)]*\)").expect("hold regex");
        header = metadata.replace_all(&header, "").into_owned();
        if let Some((kind, value)) = artifact {
            header.push_str(&format!(" ({kind}: {value})"));
        }
        let mut block = vec![Record {
            line: header,
            newline: true,
        }];
        block.extend(opened.backlog.body_parts(item).0);
        let selected = HashSet::from([id.to_owned()]);
        let without = Backlog::parse(
            &self.path,
            opened.backlog.remove_items(&selected).as_bytes(),
        )?;
        let mut next = without.insert_blocks("Done", &[block], true);
        let reparsed = Backlog::parse(&self.path, next.as_bytes())?;
        let overflow = reparsed
            .items
            .iter()
            .filter(|candidate| candidate.state == "done")
            .skip(keep)
            .collect::<Vec<_>>();
        if overflow.is_empty() {
            return publish(&self.path, &next, opened.mode);
        }
        let mut archive_content = String::new();
        for (index, candidate) in overflow.iter().enumerate() {
            if index > 0 {
                archive_content.push('\n');
            }
            archive_content.push_str(&render_records(&reparsed.item_block(candidate)));
        }
        let selected = overflow
            .iter()
            .map(|item| item.id.clone())
            .collect::<HashSet<_>>();
        next = reparsed.remove_items(&selected);
        let archive_opened = open_archive(&archive)?;
        let archived = append_archive(
            &archive_opened.backlog.text,
            &format!("Archived Done ({})", now_iso()),
            &archive_content,
        );
        publish_two(&opened, &next, &archive_opened, &archived)
    }
}

fn archive_path(backlog: &Path) -> PathBuf {
    std::env::var_os("MX_BACKLOG_ARCHIVE").map_or_else(
        || {
            backlog
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("done-archive.md")
        },
        PathBuf::from,
    )
}

fn open_archive(path: &Path) -> Result<Opened, BacklogError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(BacklogError::new(format!(
                    "backlog must not be a symlink: {}",
                    path.display()
                )));
            }
            if !metadata.is_file() {
                return Err(BacklogError::new(format!(
                    "backlog is not a regular file: {}",
                    path.display()
                )));
            }
            let text = String::from_utf8_lossy(&fs::read(path).map_err(|error| {
                BacklogError::new(format!("cannot read backlog {}: {error}", path.display()))
            })?)
            .into_owned();
            Ok(Opened {
                backlog: Backlog {
                    path: path.to_path_buf(),
                    text,
                    records: Vec::new(),
                    heading_indexes: HashMap::new(),
                    items: Vec::new(),
                    by_id: HashMap::new(),
                },
                mode: metadata.permissions().mode() & 0o777,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Opened {
            backlog: Backlog {
                path: path.to_path_buf(),
                text: String::new(),
                records: Vec::new(),
                heading_indexes: HashMap::new(),
                items: Vec::new(),
                by_id: HashMap::new(),
            },
            mode: 0o644,
        }),
        Err(error) => Err(BacklogError::new(format!(
            "cannot read backlog {}: {error}",
            path.display()
        ))),
    }
}

/// Atomically move a connected item set between two backlogs.
pub fn move_items(source: &Path, destination: &Path, ids: &[String]) -> Result<(), BacklogError> {
    if ids.is_empty() {
        return Err(BacklogError::new("mv needs at least one item id"));
    }
    let same_canonical = fs::canonicalize(source)
        .ok()
        .zip(fs::canonicalize(destination).ok())
        .is_some_and(|(left, right)| left == right);
    if source == destination || same_canonical {
        return Err(BacklogError::new(
            "source and destination backlogs must differ",
        ));
    }
    let _locks = lock_paths(&[source.to_path_buf(), destination.to_path_buf()])?;
    let source_opened = open(source, false)?;
    let destination_opened = open(destination, true)?;
    let selected = ids.iter().cloned().collect::<HashSet<_>>();
    if selected.len() != ids.len() {
        return Err(BacklogError::new("mv item ids must be unique"));
    }
    for id in &selected {
        if source_opened.backlog.item(id).is_none() {
            return Err(BacklogError::new(format!(
                "source backlog item not found: {id}"
            )));
        }
        if destination_opened.backlog.item(id).is_some() {
            return Err(BacklogError::new(format!(
                "destination already contains item: {id}"
            )));
        }
    }
    for item in &source_opened.backlog.items {
        let item_selected = selected.contains(&item.id);
        for blocker in &item.blockers {
            let blocker_source = source_opened.backlog.item(blocker).is_some();
            let blocker_destination = destination_opened.backlog.item(blocker).is_some();
            if item_selected && blocker_source && !selected.contains(blocker) {
                return Err(BacklogError::new(format!(
                    "moving {} would strand blocker {blocker} in the source backlog",
                    item.id
                )));
            }
            if !item_selected && selected.contains(blocker) {
                return Err(BacklogError::new(format!(
                    "moving {blocker} would strand dependent {} in the source backlog",
                    item.id
                )));
            }
            if item_selected && blocker_destination {
                return Err(BacklogError::new(format!(
                    "moving {} would retain a cross-backlog dependency on {blocker}",
                    item.id
                )));
            }
        }
    }
    let mut by_section: BTreeMap<&str, Vec<Vec<Record>>> = SECTIONS
        .iter()
        .map(|section| (*section, Vec::new()))
        .collect();
    for item in &source_opened.backlog.items {
        if selected.contains(&item.id) {
            by_section
                .get_mut(item.section.as_str())
                .expect("section")
                .push(source_opened.backlog.item_block(item));
        }
    }
    let source_next = source_opened.backlog.remove_items(&selected);
    let mut destination_next = destination_opened.backlog.text.clone();
    for section in SECTIONS {
        let blocks = &by_section[section];
        if blocks.is_empty() {
            continue;
        }
        let parsed = Backlog::parse(destination, destination_next.as_bytes())?;
        destination_next = parsed.insert_blocks(section, blocks, false);
    }
    publish_two(
        &source_opened,
        &source_next,
        &destination_opened,
        &destination_next,
    )
}

/// Stable help text from the legacy operator entry point.
pub const USAGE: &str = "Usage:\n  mx-backlog.sh list [--file <path>] [--limit <n>]\n  mx-backlog.sh show <id> [--file <path>] [--full]\n  mx-backlog.sh add <id> <title> [--file <path>] [options]\n  mx-backlog.sh done <id> [--file <path>] [--report p | --note s | --pr url]\n  mx-backlog.sh ready [--file <path>]\n  mx-backlog.sh hold <id> [--file <path>] --reason <text> --kind <kind>\n  mx-backlog.sh update <id> [--file <path>] (--body <text> | --body-file <path>) [--archive-body]\n  mx-backlog.sh block <id> [--file <path>] --by <blocker-id>\n  mx-backlog.sh unblock <id> [--file <path>] --by <blocker-id>\n  mx-backlog.sh mv <id>... --file <source> --to <destination>\n  mx-backlog.sh validate [--file <path>]\n";

fn usage_failure() -> CliFailure {
    CliFailure {
        code: 2,
        message: String::new(),
        usage: true,
    }
}

fn option_failure(message: impl Into<String>) -> CliFailure {
    CliFailure {
        code: 2,
        message: message.into(),
        usage: false,
    }
}

/// Execute the exact operator CLI grammar over typed store operations.
pub fn run_cli(args: &[OsString], default_file: PathBuf) -> Result<String, CliFailure> {
    let values = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let Some(command) = values.first().map(String::as_str) else {
        return Err(usage_failure());
    };
    if matches!(command, "-h" | "--help") {
        return Ok(USAGE.to_owned());
    }
    if !matches!(
        command,
        "list"
            | "show"
            | "add"
            | "done"
            | "ready"
            | "hold"
            | "update"
            | "block"
            | "unblock"
            | "mv"
            | "validate"
    ) {
        return Err(usage_failure());
    }
    let mut file = default_file;
    let mut destination = None;
    let mut limit = "80".to_owned();
    let mut keep = None;
    let mut positionals = Vec::new();
    let mut options: HashMap<String, Vec<String>> = HashMap::new();
    let mut flags = HashSet::new();
    let mut index = 1;
    while index < values.len() {
        let argument = &values[index];
        let (name, inline) = argument.strip_prefix("--").map_or(("", None), |value| {
            value
                .split_once('=')
                .map_or((value, None), |(a, b)| (a, Some(b)))
        });
        match name {
            "file" | "to" | "limit" | "keep" | "repo" | "kind" | "body" | "body-file"
            | "blocked-by" | "report" | "note" | "pr" | "reason" | "by" => {
                let value = if let Some(value) = inline {
                    value.to_owned()
                } else {
                    index += 1;
                    values.get(index).cloned().ok_or_else(|| {
                        option_failure(format!("mx-backlog: --{name} requires a value"))
                    })?
                };
                match name {
                    "file" => file = PathBuf::from(value),
                    "to" => destination = Some(PathBuf::from(value)),
                    "limit" => limit = value,
                    "keep" => keep = Some(value),
                    "body-file" => {
                        let path = PathBuf::from(&value);
                        let metadata = fs::symlink_metadata(&path).ok();
                        if !metadata
                            .is_some_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
                        {
                            return Err(CliFailure {
                                code: 1,
                                message: format!(
                                    "body file must be a regular non-symlink file: {}",
                                    path.display()
                                ),
                                usage: false,
                            });
                        }
                        let body = String::from_utf8_lossy(&fs::read(&path).map_err(|error| {
                            CliFailure {
                                code: 1,
                                message: error.to_string(),
                                usage: false,
                            }
                        })?)
                        .into_owned();
                        options
                            .entry("body".to_owned())
                            .or_default()
                            .push(body.trim_end_matches('\n').to_owned());
                    }
                    _ => options.entry(name.to_owned()).or_default().push(value),
                }
            }
            "full" => {}
            "archive-body" | "start" => {
                flags.insert(name.to_owned());
            }
            "" if argument.starts_with("--") => {
                return Err(option_failure(format!(
                    "mx-backlog: unknown option: {argument}"
                )));
            }
            "" => positionals.push(argument.clone()),
            _ => {
                return Err(option_failure(format!(
                    "mx-backlog: unknown option: {argument}"
                )));
            }
        }
        index += 1;
    }
    let store = BacklogStore::new(&file);
    let one = |name: &str, required: bool| -> Result<String, CliFailure> {
        options
            .get(name)
            .and_then(|values| values.first())
            .cloned()
            .or_else(|| (!required).then(String::new))
            .ok_or_else(|| BacklogError::new(format!("--{name} is required")).into())
    };
    match command {
        "list" if positionals.is_empty() => {
            let limit = limit
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| BacklogError::new("list limit must be a positive integer"))?;
            store.list(limit).map_err(Into::into)
        }
        "show" if positionals.len() == 1 => store.show(&positionals[0]).map_err(Into::into),
        "ready" if positionals.is_empty() => store.ready().map_err(Into::into),
        "validate" if positionals.is_empty() => {
            store.validate()?;
            Ok(String::new())
        }
        "add" if positionals.len() == 2 => {
            let blockers = options.get("blocked-by").cloned().unwrap_or_default();
            let repo = one("repo", false)?;
            let kind = one("kind", false)?;
            let body = one("body", false)?;
            store.add(&AddRequest {
                id: &positionals[0],
                title: &positionals[1],
                repo: if repo.is_empty() { "broker" } else { &repo },
                kind: if kind.is_empty() { "delivery" } else { &kind },
                body: &body,
                start: flags.contains("start"),
                blockers: &blockers,
            })?;
            Ok(String::new())
        }
        "hold" if positionals.len() == 1 => {
            store.hold(&positionals[0], &one("reason", true)?, &one("kind", true)?)?;
            Ok(String::new())
        }
        "update" if positionals.len() == 1 => {
            store.update(
                &positionals[0],
                &one("body", true)?,
                flags.contains("archive-body"),
            )?;
            Ok(String::new())
        }
        "block" if positionals.len() == 1 => {
            store.block(&positionals[0], &one("by", true)?)?;
            Ok(String::new())
        }
        "unblock" if positionals.len() == 1 => {
            store.unblock(&positionals[0], &one("by", true)?)?;
            Ok(String::new())
        }
        "done" if positionals.len() == 1 => {
            let artifacts = ["report", "note", "pr"]
                .iter()
                .filter_map(|kind| {
                    options
                        .get(*kind)
                        .and_then(|values| values.first())
                        .map(|value| (*kind, value.as_str()))
                })
                .collect::<Vec<_>>();
            if artifacts.len() > 1 {
                return Err(BacklogError::new(
                    "done accepts only one of --report, --note, or --pr",
                )
                .into());
            }
            let raw_keep = keep
                .or_else(|| std::env::var("MX_BACKLOG_DONE_KEEP").ok())
                .unwrap_or_else(|| "10".to_owned());
            let keep = raw_keep.parse::<usize>().map_err(|_| {
                BacklogError::new("MX_BACKLOG_DONE_KEEP must be a non-negative integer")
            })?;
            store.done(&positionals[0], artifacts.first().copied(), keep)?;
            Ok(String::new())
        }
        "mv" if !positionals.is_empty() && destination.is_some() => {
            move_items(&file, &destination.expect("checked"), &positionals)?;
            Ok(String::new())
        }
        _ => Err(usage_failure()),
    }
}

/// Resolve the local backend selector without changing its whitespace contract.
#[must_use]
pub fn backend_value(config: &Path) -> String {
    let path = config.join("backlog-backend");
    match fs::read_to_string(path) {
        Ok(value) => {
            let stripped = value
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            if stripped.is_empty() {
                "owned".to_owned()
            } else {
                stripped
            }
        }
        Err(_) => "owned".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaffold(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("parent");
        fs::write(path, SCAFFOLD).expect("scaffold");
    }

    #[test]
    fn parse_render_noop_is_byte_exact_for_legacy_order_and_no_final_newline() {
        let bytes =
            b"## Done\n\n## Queued\n- [ ] item - title (repo: broker)\n  body\n\n## In flight";
        let parsed = Backlog::parse("backlog.md", bytes).expect("parse");
        assert_eq!(parsed.text.as_bytes(), bytes);
        assert_eq!(parsed.body_text(parsed.item("item").expect("item")), "body");
    }

    #[test]
    fn malformed_duplicate_unknown_and_continuation_shapes_fail() {
        for text in [
            "## In flight\n## Queued\n## Done\n## Other\n",
            "## In flight\n## Queued\n- [ ] a - one\n- [ ] a - two\n## Done\n",
            "## In flight\n## Queued\n- [ ] a - one\n\tbody\n## Done\n",
            "## In flight\n## Queued\nfragment\n## Done\n",
        ] {
            assert!(Backlog::parse("bad.md", text.as_bytes()).is_err());
        }
    }

    #[test]
    fn section_and_continuation_failure_matrix_reports_each_structural_violation() {
        assert_eq!(
            Record {
                line: "tail".to_owned(),
                newline: false,
            }
            .render(),
            "tail"
        );
        let cases = [
            "## In flight\n\n## Queued\n\n## Unexpected\n",
            "## In flight\n\n## Queued\n\n## Queued\n\n## Done\n",
            "## In flight\n\n## Queued\n",
            "## In flight\n\n orphan\n\n## Queued\n\n## Done\n",
            "## In flight\n\ntruncated\n\n## Queued\n\n## Done\n",
            "## In flight\n\n- [ ] task - Title\n body\n\n## Queued\n\n## Done\n",
        ];
        for text in cases {
            assert!(
                Backlog::parse("fixture.md", text.as_bytes()).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn crud_retention_and_connected_move_preserve_bodies_and_modes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        scaffold(&source);
        scaffold(&destination);
        fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).expect("mode");
        let store = BacklogStore::new(&source);
        store
            .add(&AddRequest {
                id: "blocker",
                title: "Blocker",
                repo: "broker",
                kind: "delivery",
                body: "one\n\n## Intent\ntwo",
                start: false,
                blockers: &[],
            })
            .expect("add blocker");
        let blockers = ["blocker".to_owned()];
        store
            .add(&AddRequest {
                id: "dependent",
                title: "Dependent",
                repo: "broker",
                kind: "delivery",
                body: "",
                start: false,
                blockers: &blockers,
            })
            .expect("add dependent");
        assert!(move_items(&source, &destination, &["blocker".to_owned()]).is_err());
        move_items(
            &source,
            &destination,
            &["blocker".to_owned(), "dependent".to_owned()],
        )
        .expect("move connected");
        let destination_text = fs::read_to_string(destination).expect("destination");
        assert!(destination_text.contains("  ## Intent"));
        assert_eq!(
            fs::metadata(source).expect("mode").permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn concurrent_adds_serialize_without_lost_items() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("backlog.md");
        scaffold(&path);
        std::thread::scope(|scope| {
            for index in 0..12 {
                let path = path.clone();
                scope.spawn(move || {
                    for _ in 0..50 {
                        let id = format!("item-{index}");
                        let result = BacklogStore::new(&path).add(&AddRequest {
                            id: &id,
                            title: "Title",
                            repo: "broker",
                            kind: "delivery",
                            body: "",
                            start: false,
                            blockers: &[],
                        });
                        if result.is_ok()
                            || result
                                .as_ref()
                                .is_err_and(|error| error.to_string().contains("already exists"))
                        {
                            return;
                        }
                    }
                    panic!("writer never acquired lock");
                });
            }
        });
        let parsed = open(&path, false).expect("open");
        assert_eq!(parsed.backlog.items.len(), 12);
    }

    #[test]
    fn store_queries_and_mutations_cover_the_complete_lifecycle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("backlog.md");
        scaffold(&path);
        let store = BacklogStore::new(&path);
        store.validate().expect("validate");
        store
            .add(&AddRequest {
                id: "alpha",
                title: "Alpha, quoted",
                repo: "broker",
                kind: "delivery",
                body: "first\n\nsecond, \"quoted\"",
                start: false,
                blockers: &[],
            })
            .expect("alpha");
        store
            .add(&AddRequest {
                id: "beta",
                title: "Beta",
                repo: "repo",
                kind: "investigation",
                body: "",
                start: true,
                blockers: &["alpha".to_owned(), "external".to_owned()],
            })
            .expect("beta");
        assert!(
            store
                .list(1)
                .expect("list")
                .contains("(truncated 1 item(s))")
        );
        assert!(store.list(80).expect("list").contains("\"Alpha, quoted\""));
        assert!(
            store
                .show("alpha")
                .expect("show")
                .contains("first\\n\\nsecond")
        );
        assert!(store.ready().expect("ready").contains("alpha"));
        assert!(store.list(0).is_err());
        assert!(store.show("missing").is_err());

        store
            .hold("alpha", "maintainer answer", "maintainer")
            .expect("hold");
        let held = store.show("alpha").expect("held");
        assert!(held.contains("held: yes"));
        assert!(held.contains("hold_kind: maintainer"));
        store.block("alpha", "gate").expect("block");
        store.block("alpha", "gate").expect("idempotent block");
        assert!(
            store
                .show("alpha")
                .expect("blocked")
                .contains("blocked: yes")
        );
        store.unblock("alpha", "gate").expect("unblock");
        assert!(store.unblock("alpha", "gate").is_err());
        store.update("alpha", "replacement", false).expect("update");
        store
            .update("alpha", "final body", true)
            .expect("archived update");
        assert!(
            fs::read_to_string(temp.path().join("done-archive.md"))
                .expect("archive")
                .contains("Superseded body: alpha")
        );

        store
            .done("alpha", Some(("note", "completed")), 10)
            .expect("done alpha");
        store
            .done("alpha", Some(("pr", "ignored-idempotently")), 10)
            .expect("done idempotent");
        assert!(store.hold("alpha", "reason", "kind").is_err());
        assert!(store.block("alpha", "gate").is_err());
        store
            .done("beta", Some(("report", "reports/beta.md")), 0)
            .expect("done beta with retention");
        let archive = fs::read_to_string(temp.path().join("done-archive.md")).expect("archive");
        assert!(archive.contains("Archived Done"));
        assert!(archive.contains("beta - Beta"));
    }

    #[test]
    fn mutation_validation_refuses_bad_ids_titles_reasons_and_blockers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("backlog.md");
        scaffold(&path);
        let store = BacklogStore::new(&path);
        for (id, title, blockers) in [
            ("bad/id", "title", Vec::new()),
            ("valid", "", Vec::new()),
            ("valid", "two\nlines", Vec::new()),
            ("valid", "title", vec!["bad/id".to_owned()]),
        ] {
            assert!(
                store
                    .add(&AddRequest {
                        id,
                        title,
                        repo: "broker",
                        kind: "delivery",
                        body: "",
                        start: false,
                        blockers: &blockers,
                    })
                    .is_err()
            );
        }
        store
            .add(&AddRequest {
                id: "valid",
                title: "Valid",
                repo: "broker",
                kind: "delivery",
                body: "",
                start: false,
                blockers: &[],
            })
            .expect("valid");
        assert!(
            store
                .add(&AddRequest {
                    id: "valid",
                    title: "Duplicate",
                    repo: "broker",
                    kind: "delivery",
                    body: "",
                    start: false,
                    blockers: &[],
                })
                .is_err()
        );
        assert!(store.hold("valid", "bad (reason)", "kind").is_err());
        assert!(store.hold("missing", "reason", "kind").is_err());
        assert!(store.block("valid", "bad/id").is_err());
        assert!(store.block("missing", "valid").is_err());
        assert!(store.update("missing", "body", false).is_err());
        assert!(store.done("missing", None, 10).is_err());
    }

    #[test]
    fn cli_grammar_covers_commands_inline_options_body_files_and_errors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("backlog.md");
        let destination = temp.path().join("destination.md");
        let body_file = temp.path().join("body.txt");
        scaffold(&path);
        scaffold(&destination);
        fs::write(&body_file, "from file\n").expect("body file");
        let call = |args: &[&str]| {
            run_cli(
                &args.iter().map(OsString::from).collect::<Vec<_>>(),
                path.clone(),
            )
        };
        assert!(call(&["--help"]).expect("help").contains("mx-backlog.sh"));
        assert!(call(&[]).expect_err("usage").usage);
        assert!(call(&["unknown"]).expect_err("usage").usage);
        call(&[
            "add",
            "one",
            "One",
            "--repo=repo",
            "--kind",
            "delivery",
            "--body-file",
            body_file.to_str().expect("body path"),
            "--start",
        ])
        .expect("add");
        call(&["add", "two", "Two", "--blocked-by=one"]).expect("add blocked");
        assert!(
            call(&["list", "--limit=1", "--full"])
                .expect("list")
                .contains("truncated")
        );
        assert!(call(&["show", "one"]).expect("show").contains("from file"));
        call(&["hold", "one", "--reason=waiting", "--kind=maintainer"]).expect("hold");
        call(&["update", "one", "--body=replaced", "--archive-body"]).expect("update");
        call(&["block", "one", "--by=gate"]).expect("block");
        call(&["unblock", "one", "--by=gate"]).expect("unblock");
        call(&["done", "one", "--pr=https://example.test/pr/1", "--keep=10"]).expect("done");
        call(&[
            "mv",
            "one",
            "two",
            "--to",
            destination.to_str().expect("destination"),
        ])
        .expect("move");
        call(&["ready"]).expect("ready");
        call(&["validate"]).expect("validate");

        for args in [
            vec!["list", "--limit=0"],
            vec!["add", "only-id"],
            vec!["hold", "one"],
            vec!["done", "one", "--note=x", "--pr=y"],
            vec!["mv", "one"],
            vec!["validate", "extra"],
            vec!["list", "--unknown"],
            vec!["list", "--limit"],
        ] {
            assert!(call(&args).is_err(), "{args:?}");
        }
        assert!(
            call(&["add", "three", "Three", "--body-file", "missing"])
                .expect_err("missing body")
                .message
                .contains("regular non-symlink")
        );
    }

    #[test]
    fn two_file_publication_rolls_back_and_move_refusals_are_atomic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        scaffold(&source);
        scaffold(&destination);
        let store = BacklogStore::new(&source);
        store
            .add(&AddRequest {
                id: "one",
                title: "One",
                repo: "broker",
                kind: "delivery",
                body: "old",
                start: false,
                blockers: &[],
            })
            .expect("add");
        let before = fs::read(&source).expect("before");
        let first = open(&source, false).expect("source open");
        let archive_path = temp.path().join("done-archive.md");
        let second = open_archive(&archive_path).expect("archive open");
        let error = publish_two_with_fault(
            &first,
            &first.backlog.text.replace("  old", "  new"),
            &second,
            "archive bytes\n",
            true,
        )
        .expect_err("injected failure");
        assert!(error.to_string().contains("rolled back"));
        assert_eq!(fs::read(&source).expect("after"), before);

        assert!(move_items(&source, &destination, &[]).is_err());
        assert!(move_items(&source, &source, &["one".to_owned()]).is_err());
        assert!(move_items(&source, &destination, &["one".to_owned(), "one".to_owned()]).is_err());
        assert!(move_items(&source, &destination, &["missing".to_owned()]).is_err());
        BacklogStore::new(&destination)
            .add(&AddRequest {
                id: "one",
                title: "Existing",
                repo: "broker",
                kind: "delivery",
                body: "",
                start: false,
                blockers: &[],
            })
            .expect("destination item");
        assert!(move_items(&source, &destination, &["one".to_owned()]).is_err());
        assert_eq!(fs::read(&source).expect("source unchanged"), before);
    }

    #[test]
    fn generated_parser_round_trips_preserve_literal_body_and_metadata() {
        for index in 0..128 {
            let id = format!("item-{index}");
            let title = format!("Title {index}");
            let body = format!("line {index}\n\n## Intent\nliteral {}, comma", index % 7);
            let text = format!(
                "## In flight\n\n## Queued\n- [ ] {id} - {title} blocked-by: root (repo: repo-{index}) (kind: generated)\n  {}\n\n## Done\n",
                body.replace('\n', "\n  ")
            );
            let parsed = Backlog::parse("generated.md", text.as_bytes()).expect("generated parse");
            assert_eq!(parsed.text, text);
            let item = parsed.item(&id).expect("generated item");
            assert_eq!(item.title, title);
            assert_eq!(item.blockers, ["root"]);
            assert_eq!(parsed.body_text(item), body);
        }
    }

    #[test]
    fn backend_selector_preserves_whitespace_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert_eq!(backend_value(temp.path()), "owned");
        fs::write(temp.path().join("backlog-backend"), "  manual \n").expect("selector");
        assert_eq!(backend_value(temp.path()), "manual");
        fs::write(temp.path().join("backlog-backend"), " \n\t").expect("empty selector");
        assert_eq!(backend_value(temp.path()), "owned");
    }
}

//! Persistent daemon-home validation and transactional seeding.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use multplx_core::filesystem::atomic_replace;
use multplx_core::identifiers::TaskId;
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use regex::Regex;
use rustix::fs::OFlags;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::project_registry::{DeliveryMode, resolve as resolve_project_mode};

pub const USAGE: &str = "usage: mx-home-seed.sh <id> <home|-> {<project>...|--no-projects}\n       mx-home-seed.sh validate\n";

const MARKER: &str = ".mx-daemon-home";
const TRANSACTION_PREFIX: &str = ".home-seed.transaction.";

#[derive(Clone, Debug)]
struct Route {
    id: String,
    home: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Context {
    pub root: PathBuf,
    pub home: PathBuf,
    pub data: PathBuf,
    pub projects: PathBuf,
    pub state: PathBuf,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Output {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OriginalFile {
    path: String,
    backup: Option<String>,
    mode: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SeedJournal {
    state: String,
    id: String,
    home: String,
    created_home: bool,
    acquired_home: bool,
    created_projects: Vec<String>,
    originals: Vec<OriginalFile>,
}

fn error(message: impl Into<String>) -> Output {
    Output {
        status: 1,
        stdout: String::new(),
        stderr: format!("error: {}\n", message.into()),
    }
}

fn lexical(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            Component::RootDir => output.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                output.pop();
            }
            Component::Normal(value) => output.push(value),
        }
    }
    output
}

pub fn resolved(path: &Path) -> PathBuf {
    if path.exists() {
        return fs::canonicalize(path).unwrap_or_else(|_| lexical(path));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut probe = absolute.as_path();
    let mut tail = Vec::new();
    while !probe.exists() {
        if let Some(name) = probe.file_name() {
            tail.push(name.to_os_string());
        }
        let Some(parent) = probe.parent() else { break };
        probe = parent;
    }
    let mut output = fs::canonicalize(probe).unwrap_or_else(|_| lexical(probe));
    for part in tail.into_iter().rev() {
        output.push(part);
    }
    lexical(&output)
}

fn path_text(path: &Path, label: &str) -> Result<String, String> {
    let value = path
        .to_str()
        .ok_or_else(|| format!("{label} path is not valid UTF-8"))?;
    if value.contains(['\n', '\r', ';', ')']) {
        return Err(if label == "daemon home" {
            format!("daemon home path contains registry delimiters: {value}")
        } else {
            format!("{label} path contains a registry delimiter: {value}")
        });
    }
    Ok(value.to_owned())
}

fn routes(path: &Path) -> Vec<Route> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let home = Regex::new(r"\(home: ([^;)]+);").expect("home regex");
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("- ")?;
            let id = rest.split_whitespace().next()?.to_owned();
            let home = home.captures(line)?.get(1)?.as_str();
            Some(Route {
                id,
                home: resolved(Path::new(home)),
            })
        })
        .collect()
}

fn ancestor(older: &Path, newer: &Path) -> bool {
    older != newer && newer.starts_with(older)
}

pub fn validate_registry(path: &Path) -> Result<(), String> {
    let routes = routes(path);
    let mut homes = BTreeMap::<PathBuf, String>::new();
    let mut ids = BTreeMap::<String, PathBuf>::new();
    for route in &routes {
        if let Some(owner) = homes.get(&route.home)
            && owner != &route.id
        {
            return Err(format!(
                "error: duplicate daemon home assignment:\n{}: {}, {}\n",
                route.home.display(),
                owner,
                route.id
            ));
        }
        homes.insert(route.home.clone(), route.id.clone());
        if let Some(home) = ids.get(&route.id) {
            return Err(format!(
                "error: duplicate daemon id assignment:\n{}: {}, {}\n",
                route.id,
                home.display(),
                route.home.display()
            ));
        }
        ids.insert(route.id.clone(), route.home.clone());
    }
    for (index, left) in routes.iter().enumerate() {
        for right in routes.iter().skip(index + 1) {
            let (container, child) = if ancestor(&left.home, &right.home) {
                (left, right)
            } else if ancestor(&right.home, &left.home) {
                (right, left)
            } else {
                continue;
            };
            return Err(format!(
                "error: overlapping daemon home assignment:\n{} ({}) contains {} ({})\n",
                container.home.display(),
                container.id,
                child.home.display(),
                child.id
            ));
        }
    }
    Ok(())
}

fn command(program: &str, args: &[&std::ffi::OsStr], cwd: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command
        .output()
        .map_err(|error_value| format!("could not start {program}: {error_value}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("{program} failed")
        } else {
            detail
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn real_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error_value| {
        format!("cannot inspect {label} {}: {error_value}", path.display())
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{label} is linked or not a directory: {}",
            path.display()
        ));
    }
    fs::canonicalize(path)
        .map_err(|error_value| format!("cannot resolve {label} {}: {error_value}", path.display()))
}

fn require_owned(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error_value| format!("cannot inspect {label} ownership: {error_value}"))?;
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(format!(
            "{label} must be owned by the current user: {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_home_boundary(context: &Context, home: &Path) -> Result<PathBuf, String> {
    let home = resolved(home);
    let active = resolved(&context.home);
    let root = resolved(&context.root);
    if home == Path::new("/") {
        return Err(format!(
            "daemon home cannot be the filesystem root: {}",
            home.display()
        ));
    }
    for (protected, label) in [(&active, "active Multplx home"), (&root, "Multplx repo")] {
        if home == *protected {
            return Err(format!(
                "daemon home cannot be the {label}: {}",
                home.display()
            ));
        }
        if ancestor(protected, &home) {
            return Err(format!(
                "daemon home cannot be inside the {label}: {}",
                home.display()
            ));
        }
        if ancestor(&home, protected) {
            return Err(format!(
                "daemon home cannot be an ancestor of the {label}: {}",
                home.display()
            ));
        }
    }
    Ok(home)
}

fn validate_child(home: &Path, child: &Path, label: &str) -> Result<PathBuf, String> {
    let home = resolved(home);
    let child = resolved(child);
    if !ancestor(&home, &child) {
        return Err(format!(
            "daemon {label} must resolve inside the daemon home: {}",
            child.display()
        ));
    }
    Ok(child)
}

fn validate_operational_dirs(context: &Context, home: &Path) -> Result<(), String> {
    for name in ["data", "state", "config", "projects"] {
        let path = home.join(name);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink())
            && !path.exists()
        {
            return Err(format!(
                "daemon {name} directory must resolve inside the daemon home: {}",
                path.display()
            ));
        }
        let child = validate_child(home, &path, &format!("{name} directory"))?;
        let active = resolved(&context.home);
        let root = resolved(&context.root);
        if child == active || ancestor(&active, &child) {
            return Err(format!(
                "daemon {name} directory cannot be inside the active Multplx home: {}",
                path.display()
            ));
        }
        if child == root || ancestor(&root, &child) {
            return Err(format!(
                "daemon {name} directory cannot be inside the Multplx repo: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_leaf_files(home: &Path) -> Result<(), String> {
    for relative in ["data/projects.md", "data/charter.md", MARKER] {
        let path = home.join(relative);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(format!(
                "daemon leaf file must not be a symlink: {}",
                path.display()
            ));
        }
        if path.exists() {
            validate_child(home, &path, "leaf file")?;
        }
    }
    Ok(())
}

fn validate_assignment(registry: &Path, id: &str, home: &Path) -> Result<(), String> {
    let marker = home.join(MARKER);
    if marker.is_file() {
        let owner = fs::read_to_string(&marker)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if owner != id {
            return Err(format!(
                "daemon home {} is already marked for {}",
                home.display(),
                if owner.is_empty() { "unknown" } else { &owner }
            ));
        }
    }
    for route in routes(registry) {
        if route.id == id && route.home != home {
            return Err(format!(
                "daemon id {id} is already registered to home {}; retire it before assigning {}",
                route.home.display(),
                home.display()
            ));
        }
        if route.id != id && route.home == home {
            return Err(format!(
                "daemon home {} is already registered to {}",
                home.display(),
                route.id
            ));
        }
        if route.id != id && (ancestor(&route.home, home) || ancestor(home, &route.home)) {
            return Err(format!(
                "daemon home {} overlaps registered daemon home {} for {}",
                home.display(),
                route.home.display(),
                route.id
            ));
        }
    }
    Ok(())
}

fn verify_broker_home(context: &Context, home: &Path) -> Result<PathBuf, String> {
    let home = validate_home_boundary(context, home)?;
    let home = real_directory(&home, "daemon home")?;
    if !home.join("AGENTS.md").is_file() {
        return Err(format!(
            "{} is not a Multplx home (missing AGENTS.md)",
            home.display()
        ));
    }
    if !home.join("bin").is_dir() {
        return Err(format!(
            "{} is not a Multplx home (missing bin/)",
            home.display()
        ));
    }
    require_owned(&home, "daemon home")?;
    validate_operational_dirs(context, &home)?;
    Ok(home)
}

fn section(text: &str, heading: &str) -> String {
    let wanted = format!("# {heading}");
    let mut active = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line == wanted {
            active = true;
            continue;
        }
        if active && line.starts_with("# ") {
            break;
        }
        if active {
            lines.push(line);
        }
    }
    lines.join("\n")
}

fn normalize_registry_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if ";()".contains(character) {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn charter_fields(path: &Path) -> Result<(String, String), String> {
    let text = fs::read_to_string(path).map_err(|error_value| {
        format!(
            "cannot read daemon charter brief at {}: {error_value}",
            path.display()
        )
    })?;
    if text.contains("{TASK}") {
        return Err(format!(
            "daemon charter brief at {} still contains {{TASK}}; fill it before seeding",
            path.display()
        ));
    }
    let summary = env::var("MX_DAEMON_CHARTER").ok().map_or_else(
        || normalize_registry_text(&section(&text, "Charter")),
        |value| normalize_registry_text(&value),
    );
    if summary.is_empty() {
        return Err(format!(
            "daemon charter brief at {} has an empty Charter section; fill it before seeding",
            path.display()
        ));
    }
    let scope = env::var("MX_DAEMON_SCOPE").ok().map_or_else(
        || normalize_registry_text(&section(&text, "Routing scope")),
        |value| normalize_registry_text(&value),
    );
    if scope.is_empty() {
        return Err(format!(
            "daemon charter brief at {} has an empty Routing scope section; fill it before seeding",
            path.display()
        ));
    }
    Ok((summary, scope))
}

fn normalized_origin(repo: &Path, url: &str) -> PathBuf {
    if url.starts_with("file://")
        || url.contains("://")
        || (url.contains(':') && !url.starts_with(['.', '/']))
    {
        PathBuf::from(url)
    } else {
        resolved(&repo.join(url))
    }
}

fn project_origin(context: &Context, project: &str) -> Result<(PathBuf, String), String> {
    let source = context.projects.join(project);
    if !source.is_dir() {
        return Err(format!(
            "project {project} not found at {}",
            source.display()
        ));
    }
    command(
        "git",
        &[
            "-C".as_ref(),
            source.as_os_str(),
            "rev-parse".as_ref(),
            "--is-inside-work-tree".as_ref(),
        ],
        None,
    )
    .map_err(|_| format!("project {project} is not a git repo"))?;
    let mode = resolve_project_mode(&context.data.join("projects.md"), project).mode;
    if mode == DeliveryMode::LocalOnly {
        return Err(format!(
            "project {project} is local-only; daemon routes support only deep-review and direct-PR projects"
        ));
    }
    let origin = command(
        "git",
        &[
            "-C".as_ref(),
            source.as_os_str(),
            "remote".as_ref(),
            "get-url".as_ref(),
            "origin".as_ref(),
        ],
        None,
    )
    .map_err(|_| {
        format!(
            "project {project} is {} but has no origin remote",
            mode.as_str()
        )
    })?;
    if origin.is_empty() {
        return Err(format!(
            "project {project} is {} but has no origin remote",
            mode.as_str()
        ));
    }
    let origin = normalized_origin(&source, &origin)
        .to_string_lossy()
        .into_owned();
    Ok((source, origin))
}

fn project_registry_line(context: &Context, project: &str, today: &str) -> String {
    fs::read_to_string(context.data.join("projects.md"))
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.split_whitespace().take(2).eq(["-", project]))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| format!("- {project} - cloned project (added {today})"))
}

fn today() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

fn journal_path(context: &Context, id: &str) -> PathBuf {
    context.data.join(format!("{TRANSACTION_PREFIX}{id}"))
}

fn publish_journal(path: &Path, journal: &SeedJournal) -> Result<(), String> {
    let bytes = serde_json::to_vec(journal).map_err(|error_value| error_value.to_string())?;
    atomic_replace(path.join("journal.json"), &bytes, 0o600)
        .map_err(|error_value| error_value.to_string())
}

fn read_owned_regular_nofollow(path: &Path, label: &str) -> Result<(Vec<u8>, u32), String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(OFlags::NOFOLLOW.bits() as i32)
        .open(path)
        .map_err(|error_value| format!("cannot open {label} {}: {error_value}", path.display()))?;
    let metadata = file.metadata().map_err(|error_value| {
        format!("cannot inspect {label} {}: {error_value}", path.display())
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "{label} must be a regular file: {}",
            path.display()
        ));
    }
    let owner = metadata.uid();
    let effective = rustix::process::geteuid().as_raw();
    if owner != effective {
        return Err(format!(
            "{label} must be owned by effective uid {effective}: {} is owned by {owner}",
            path.display()
        ));
    }
    const MAX_SEED_FILE_BYTES: u64 = 16 * 1024 * 1024;
    if metadata.len() > MAX_SEED_FILE_BYTES {
        return Err(format!(
            "{label} is unexpectedly large ({} bytes): {}",
            metadata.len(),
            path.display()
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error_value| format!("cannot read {label} {}: {error_value}", path.display()))?;
    Ok((bytes, metadata.mode() & 0o777))
}

fn backup_file(
    transaction: &Path,
    path: &Path,
    key: &str,
    journal: &mut SeedJournal,
) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(format!(
            "daemon seed file must be absent or regular: {}",
            path.display()
        )),
        Ok(_) => {
            let backup = format!("backup-{key}");
            let (bytes, mode) = read_owned_regular_nofollow(path, "daemon seed file")?;
            atomic_replace(transaction.join(&backup), &bytes, 0o600)
                .map_err(|error_value| error_value.to_string())?;
            journal.originals.push(OriginalFile {
                path: path_text(path, "seed file")?,
                backup: Some(backup),
                mode,
            });
            Ok(())
        }
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {
            journal.originals.push(OriginalFile {
                path: path_text(path, "seed file")?,
                backup: None,
                mode: 0o600,
            });
            Ok(())
        }
        Err(error_value) => Err(error_value.to_string()),
    }
}

fn safe_created_home(context: &Context, home: &Path) -> bool {
    validate_home_boundary(context, home).is_ok()
}

fn rollback(context: &Context, transaction: &Path, journal: &SeedJournal) -> Vec<String> {
    let mut warnings = Vec::new();
    let home = PathBuf::from(&journal.home);
    for original in journal.originals.iter().rev() {
        let path = PathBuf::from(&original.path);
        if (journal.acquired_home || journal.created_home) && path.starts_with(&home) {
            continue;
        }
        match &original.backup {
            Some(backup) => match read_owned_regular_nofollow(
                &transaction.join(backup),
                "home seed rollback backup",
            ) {
                Ok((bytes, _)) => {
                    if let Err(error_value) = atomic_replace(&path, &bytes, original.mode) {
                        warnings.push(format!(
                            "warning: failed to restore {}: {error_value}",
                            path.display()
                        ));
                    }
                }
                Err(error_value) => warnings.push(format!(
                    "warning: failed to read rollback backup for {}: {error_value}",
                    path.display()
                )),
            },
            None => {
                let _ = fs::remove_file(&path);
                if path.file_name().and_then(|name| name.to_str()) == Some("brief.md")
                    && path
                        .parent()
                        .is_some_and(|parent| parent.parent() == Some(&context.data))
                {
                    let _ = fs::remove_dir(path.parent().expect("brief parent"));
                }
            }
        }
    }
    if journal.acquired_home {
        if safe_created_home(context, &home) {
            let result = command(
                "treehouse",
                &["return".as_ref(), "--force".as_ref(), home.as_os_str()],
                Some(&context.root),
            );
            if result.is_err() {
                warnings.push(format!("warning: failed to return treehouse-acquired home {} during seed rollback; lease may still be held", home.display()));
            }
        }
    } else if journal.created_home {
        if safe_created_home(context, &home)
            && let Err(error_value) = fs::remove_dir_all(&home)
            && error_value.kind() != std::io::ErrorKind::NotFound
        {
            warnings.push(format!(
                "warning: failed to remove created daemon home {}: {error_value}",
                home.display()
            ));
        }
    } else {
        for project in journal.created_projects.iter().rev() {
            let project = PathBuf::from(project);
            if validate_child(&home.join("projects"), &project, "created project").is_ok() {
                let _ = fs::remove_dir_all(project);
            }
        }
    }
    warnings
}

fn recover(context: &Context) -> Result<(), String> {
    let entries = match fs::read_dir(&context.data) {
        Ok(entries) => entries,
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error_value) => return Err(error_value.to_string()),
    };
    for entry in entries.filter_map(Result::ok) {
        let entry_name = entry.file_name();
        let Some(name) = entry_name.to_str().map(str::to_owned) else {
            if entry_name.to_string_lossy().starts_with(TRANSACTION_PREFIX) {
                return Err("home seed transaction name is not valid UTF-8".to_owned());
            }
            continue;
        };
        if !name.starts_with(TRANSACTION_PREFIX) {
            continue;
        }
        let transaction = entry.path();
        let metadata =
            fs::symlink_metadata(&transaction).map_err(|error_value| error_value.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "home seed transaction is linked or not a directory: {}",
                transaction.display()
            ));
        }
        require_owned(&transaction, "home seed transaction")?;
        let (bytes, _) = read_owned_regular_nofollow(
            &transaction.join("journal.json"),
            "home seed recovery journal",
        )?;
        let journal: SeedJournal = serde_json::from_slice(&bytes).map_err(|error_value| {
            format!("malformed home seed recovery journal: {error_value}")
        })?;
        let suffix = name
            .strip_prefix(TRANSACTION_PREFIX)
            .ok_or("malformed home seed transaction name")?;
        TaskId::parse(suffix).map_err(|_| "malformed home seed transaction id")?;
        if journal.id != suffix {
            return Err(
                "home seed recovery journal identity does not match its directory".to_owned(),
            );
        }
        if journal.state != "prepared" && journal.state != "committed" {
            return Err("malformed home seed recovery state".to_owned());
        }
        if journal.home.is_empty() {
            if journal.created_home
                || !journal.created_projects.is_empty()
                || !journal.originals.is_empty()
            {
                return Err(
                    "home seed recovery journal records mutations without a daemon home".to_owned(),
                );
            }
            fs::remove_dir_all(&transaction).map_err(|error_value| error_value.to_string())?;
            continue;
        }
        let home = validate_home_boundary(context, Path::new(&journal.home))?;
        let allowed = [
            context.data.join("daemons.md"),
            context.data.join(&journal.id).join("brief.md"),
            home.join("data/projects.md"),
            home.join("data/charter.md"),
            home.join(MARKER),
        ]
        .into_iter()
        .map(|path| resolved(&path))
        .collect::<BTreeSet<_>>();
        if journal.originals.iter().any(|original| {
            !allowed.contains(&resolved(Path::new(&original.path)))
                || original.backup.as_ref().is_some_and(|backup| {
                    !matches!(
                        backup.as_str(),
                        "backup-parent-registry"
                            | "backup-parent-brief"
                            | "backup-sub-registry"
                            | "backup-charter"
                            | "backup-marker"
                    )
                })
        }) || journal.created_projects.iter().any(|project| {
            validate_child(
                &home.join("projects"),
                Path::new(project),
                "created project",
            )
            .is_err()
        }) {
            return Err("home seed recovery journal contains an unsafe mutation target".to_owned());
        }
        if journal.state == "prepared" {
            let warnings = rollback(context, &transaction, &journal);
            if !warnings.is_empty() {
                return Err(warnings.join("\n"));
            }
        }
        fs::remove_dir_all(&transaction).map_err(|error_value| error_value.to_string())?;
    }
    Ok(())
}

fn injected(point: &str) -> Result<(), String> {
    if env::var("MX_HOME_SEED_CRASH_AFTER").as_deref() == Ok(point) {
        std::process::exit(96);
    }
    if env::var("MX_HOME_SEED_FAIL_AFTER").as_deref() == Ok(point) {
        return Err(format!("injected home seed failure after {point}"));
    }
    Ok(())
}

fn projectless_empty(home: &Path) -> Result<(), String> {
    let projects = home.join("projects");
    let mut clones = Vec::new();
    let mut registry_projects = Vec::new();
    match fs::symlink_metadata(&projects) {
        Ok(metadata) if metadata.file_type().is_symlink() => return Err(format!("cannot inspect existing projects directory at {} because it is a symlink; resolve the symlink or retire or clean this home before seeding with --no-projects", projects.display())),
        Ok(metadata) if !metadata.is_dir() => return Err(format!("cannot inspect existing projects directory at {} because it is not a directory; resolve its path or retire or clean this home before seeding with --no-projects", projects.display())),
        Ok(_) => {
            for entry in fs::read_dir(&projects).map_err(|_| format!("cannot inspect existing projects directory at {}; resolve its access permissions or retire or clean this home before seeding with --no-projects", projects.display()))? {
                clones.push(entry.map_err(|_| format!("cannot inspect existing projects directory at {}; resolve its access permissions or retire or clean this home before seeding with --no-projects", projects.display()))?.file_name().to_string_lossy().into_owned());
            }
        }
        Err(error_value) if error_value.kind() == std::io::ErrorKind::NotFound => {}
        Err(error_value) => return Err(error_value.to_string()),
    }
    let registry = home.join("data/projects.md");
    if registry.is_file() {
        let text = fs::read_to_string(&registry).map_err(|_| format!("cannot inspect existing project registry at {}; resolve its access permissions or retire or clean this home before seeding with --no-projects", registry.display()))?;
        registry_projects.extend(text.lines().filter_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next() == Some("-")).then(|| fields.next().unwrap_or_default().to_owned())
        }));
    }
    if !clones.is_empty() || !registry_projects.is_empty() {
        let mut message = format!(
            "cannot seed project-less daemon home {} because it contains project data",
            home.display()
        );
        if !clones.is_empty() {
            message.push_str(&format!(
                "\nerror: projects/ entries: {}",
                clones.join(", ")
            ));
        }
        if !registry_projects.is_empty() {
            message.push_str(&format!(
                "\nerror: data/projects.md entries: {}",
                registry_projects.join(", ")
            ));
        }
        message
            .push_str("\nerror: retire or clean this home first before seeding with --no-projects");
        return Err(message);
    }
    Ok(())
}

fn seed(args: &[OsString], context: &Context) -> Result<String, String> {
    if args.len() < 3 {
        return Err(USAGE.trim_end().to_owned());
    }
    let id = args[0].to_str().ok_or("daemon id is not valid UTF-8")?;
    TaskId::parse(id).map_err(|_| format!("invalid daemon id: {id}"))?;
    let requested = PathBuf::from(&args[1]);
    let mut no_projects = false;
    let mut projects = Vec::new();
    for arg in &args[2..] {
        let value = arg.to_str().ok_or("project name is not valid UTF-8")?;
        if value == "--no-projects" {
            no_projects = true;
        } else {
            projects.push(value.to_owned());
        }
    }
    if no_projects && !projects.is_empty() {
        return Err("--no-projects cannot be combined with a project list".to_owned());
    }
    if !no_projects && projects.is_empty() {
        return Err(
            "daemon needs at least one project, or --no-projects for a project-less home"
                .to_owned(),
        );
    }
    let mut seen = BTreeSet::new();
    if projects.iter().any(|project| !seen.insert(project.clone())) {
        return Err("project list contains a duplicate".to_owned());
    }
    path_text(&context.root, "Multplx root")?;
    path_text(&context.home, "active Multplx home")?;
    path_text(&context.data, "active data")?;
    if requested != Path::new("-") {
        path_text(&requested, "daemon home")?;
    }
    validate_registry(&context.data.join("daemons.md"))
        .map_err(|value| value.trim_start_matches("error: ").trim_end().to_owned())?;
    for project in &projects {
        project_origin(context, project)?;
    }

    fs::create_dir_all(&context.data).map_err(|error_value| error_value.to_string())?;
    let data = real_directory(&context.data, "active data directory")?;
    require_owned(&data, "active data directory")?;
    let _lock = DirectoryLock::acquire_wait(
        data.join(".home-seed.lock"),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|error_value| format!("cannot acquire home seed lock: {error_value}"))?;
    recover(context)?;
    validate_registry(&context.data.join("daemons.md"))
        .map_err(|value| value.trim_start_matches("error: ").trim_end().to_owned())?;

    let transaction = journal_path(context, id);
    fs::create_dir(&transaction)
        .map_err(|error_value| format!("cannot create home seed transaction: {error_value}"))?;
    fs::set_permissions(&transaction, fs::Permissions::from_mode(0o700))
        .map_err(|error_value| error_value.to_string())?;
    let mut journal = SeedJournal {
        state: "prepared".to_owned(),
        id: id.to_owned(),
        home: String::new(),
        created_home: false,
        acquired_home: requested == Path::new("-"),
        created_projects: Vec::new(),
        originals: Vec::new(),
    };
    publish_journal(&transaction, &journal)?;

    let operation = (|| -> Result<String, String> {
        let home = if journal.acquired_home {
            let value = command(
                "treehouse",
                &[
                    "get".as_ref(),
                    "--lease".as_ref(),
                    "--lease-holder".as_ref(),
                    id.as_ref(),
                ],
                Some(&context.root),
            )?;
            if value.is_empty() {
                return Err("treehouse get --lease did not report a Multplx home".to_owned());
            }
            PathBuf::from(value)
        } else {
            resolved(&requested)
        };
        journal.home = path_text(&home, "daemon home")?;
        journal.created_home = !journal.acquired_home && !home.exists();
        publish_journal(&transaction, &journal)?;
        let home = validate_home_boundary(context, &home)?;
        journal.home = path_text(&home, "daemon home")?;
        publish_journal(&transaction, &journal)?;
        validate_assignment(&context.data.join("daemons.md"), id, &home)?;
        if journal.created_home {
            fs::create_dir_all(home.parent().ok_or("daemon home has no parent")?)
                .map_err(|error_value| error_value.to_string())?;
            command(
                "git",
                &[
                    "clone".as_ref(),
                    "--quiet".as_ref(),
                    context.root.as_os_str(),
                    home.as_os_str(),
                ],
                None,
            )?;
            if !home.join("AGENTS.md").is_file() && context.root.join("AGENTS.md").is_file() {
                fs::copy(context.root.join("AGENTS.md"), home.join("AGENTS.md"))
                    .map_err(|error_value| error_value.to_string())?;
            }
        }
        let home = verify_broker_home(context, &home)?;
        journal.home = path_text(&home, "daemon home")?;
        publish_journal(&transaction, &journal)?;
        validate_assignment(&context.data.join("daemons.md"), id, &home)?;
        validate_operational_dirs(context, &home)?;
        validate_leaf_files(&home)?;
        if no_projects {
            projectless_empty(&home)?;
            let existing_brief = context.data.join(id).join("brief.md");
            if existing_brief.is_file() {
                let text = fs::read_to_string(&existing_brief)
                    .map_err(|error_value| error_value.to_string())?;
                let clones = section(&text, "Project clones");
                if !clones.contains("None. This is a project-less domain")
                    || clones
                        .lines()
                        .any(|line| line.trim_start().starts_with("- "))
                {
                    return Err(format!(
                        "cannot seed project-less daemon home because existing charter brief at {} conflicts with --no-projects\nerror: re-scaffold it with mx-brief.sh {id} --daemon --no-projects or remove the stale brief before seeding",
                        existing_brief.display()
                    ));
                }
            }
        }
        injected("home")?;

        for directory in [
            &context.data,
            &home.join("data"),
            &home.join("state"),
            &home.join("config"),
            &home.join("projects"),
        ] {
            fs::create_dir_all(directory).map_err(|error_value| error_value.to_string())?;
        }
        let parent_brief = context.data.join(id).join("brief.md");
        for (key, path) in [
            ("parent-registry", context.data.join("daemons.md")),
            ("parent-brief", parent_brief.clone()),
            ("sub-registry", home.join("data/projects.md")),
            ("charter", home.join("data/charter.md")),
            ("marker", home.join(MARKER)),
        ] {
            backup_file(&transaction, &path, key, &mut journal)?;
        }
        publish_journal(&transaction, &journal)?;

        if !parent_brief.is_file() {
            if env::var("MX_DAEMON_CHARTER")
                .ok()
                .is_none_or(|value| value.is_empty())
            {
                return Err(format!(
                    "no filled daemon charter brief at {}; set MX_DAEMON_CHARTER or scaffold one and replace {{TASK}}",
                    parent_brief.display()
                ));
            }
            let mut brief_args = vec![OsString::from(id), OsString::from("--daemon")];
            if no_projects {
                brief_args.push(OsString::from("--no-projects"));
            } else {
                brief_args.extend(projects.iter().map(OsString::from));
            }
            super::brief::run(
                &brief_args,
                &context.root,
                &context.home,
                &context.data,
                &context.state,
            )
            .map_err(|error_value| error_value.message)?;
        }
        if no_projects {
            let text =
                fs::read_to_string(&parent_brief).map_err(|error_value| error_value.to_string())?;
            let clones = section(&text, "Project clones");
            if !clones.contains("None. This is a project-less domain")
                || clones
                    .lines()
                    .any(|line| line.trim_start().starts_with("- "))
            {
                return Err(format!(
                    "cannot seed project-less daemon home because existing charter brief at {} conflicts with --no-projects\nerror: re-scaffold it with mx-brief.sh {id} --daemon --no-projects or remove the stale brief before seeding",
                    parent_brief.display()
                ));
            }
        }
        let (summary, scope) = charter_fields(&parent_brief)?;
        injected("brief")?;

        for project in &projects {
            let (source, origin) = project_origin(context, project)?;
            let destination = validate_child(
                &home.join("projects"),
                &home.join("projects").join(project),
                "project destination",
            )?;
            if destination.exists() {
                real_directory(&destination, "seeded project")?;
                command(
                    "git",
                    &[
                        "-C".as_ref(),
                        destination.as_os_str(),
                        "rev-parse".as_ref(),
                        "--is-inside-work-tree".as_ref(),
                    ],
                    None,
                )
                .map_err(|_| {
                    format!(
                        "seeded project {project} at {} is not a git repo",
                        destination.display()
                    )
                })?;
                let actual = command(
                    "git",
                    &[
                        "-C".as_ref(),
                        destination.as_os_str(),
                        "remote".as_ref(),
                        "get-url".as_ref(),
                        "origin".as_ref(),
                    ],
                    None,
                )?;
                if normalized_origin(&destination, &actual) != normalized_origin(&source, &origin) {
                    return Err(format!(
                        "seeded project {project} at {} has origin {actual}; expected {origin}",
                        destination.display()
                    ));
                }
            } else {
                journal
                    .created_projects
                    .push(path_text(&destination, "created project")?);
                publish_journal(&transaction, &journal)?;
                command(
                    "git",
                    &[
                        "clone".as_ref(),
                        "--quiet".as_ref(),
                        origin.as_ref(),
                        destination.as_os_str(),
                    ],
                    None,
                )?;
            }
        }
        injected("projects")?;

        let today = today();
        let sub_registry = home.join("data/projects.md");
        let selected = projects.iter().cloned().collect::<BTreeSet<_>>();
        let mut sub_lines = fs::read_to_string(&sub_registry)
            .unwrap_or_default()
            .lines()
            .filter(|line| {
                line.split_whitespace()
                    .nth(1)
                    .is_none_or(|name| !selected.contains(name))
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        sub_lines.extend(
            projects
                .iter()
                .map(|project| project_registry_line(context, project, &today)),
        );
        let sub_text = if sub_lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", sub_lines.join("\n"))
        };
        atomic_replace(&sub_registry, sub_text.as_bytes(), 0o600)
            .map_err(|error_value| error_value.to_string())?;
        let charter = fs::read(&parent_brief).map_err(|error_value| error_value.to_string())?;
        atomic_replace(home.join("data/charter.md"), &charter, 0o600)
            .map_err(|error_value| error_value.to_string())?;
        atomic_replace(home.join(MARKER), format!("{id}\n").as_bytes(), 0o600)
            .map_err(|error_value| error_value.to_string())?;

        let registry = context.data.join("daemons.md");
        let mut lines = fs::read_to_string(&registry)
            .unwrap_or_default()
            .lines()
            .filter(|line| {
                !line
                    .strip_prefix("- ")
                    .is_some_and(|rest| rest.split_whitespace().next() == Some(id))
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        lines.push(format!(
            "- {id} - {summary} (home: {}; scope: {scope}; projects: {}; added {today})",
            home.display(),
            projects.join(", ")
        ));
        atomic_replace(
            &registry,
            format!("{}\n", lines.join("\n")).as_bytes(),
            0o600,
        )
        .map_err(|error_value| error_value.to_string())?;
        injected("registry")?;
        validate_registry(&registry)
            .map_err(|value| value.trim_start_matches("error: ").trim_end().to_owned())?;
        Ok(format!("home={}\n", home.display()))
    })();

    match operation {
        Ok(stdout) => {
            journal.state = "committed".to_owned();
            publish_journal(&transaction, &journal)?;
            fs::remove_dir_all(transaction).map_err(|error_value| error_value.to_string())?;
            Ok(stdout)
        }
        Err(error_value) => {
            let warnings = rollback(context, &transaction, &journal);
            let _ = fs::remove_dir_all(transaction);
            if warnings.is_empty() {
                Err(error_value)
            } else {
                Err(format!("{error_value}\n{}", warnings.join("\n")))
            }
        }
    }
}

pub fn run(args: &[OsString], context: &Context) -> Output {
    if args.len() == 1 && args[0] == "validate" {
        return match validate_registry(&context.data.join("daemons.md")) {
            Ok(()) => Output::default(),
            Err(stderr) => Output {
                status: 1,
                stdout: String::new(),
                stderr,
            },
        };
    }
    if args.is_empty() || (args.len() == 1 && matches!(args[0].to_str(), Some("-h" | "--help"))) {
        return Output {
            status: 0,
            stdout: String::new(),
            stderr: USAGE.to_owned(),
        };
    }
    match seed(args, context) {
        Ok(stdout) => Output {
            status: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) if message.starts_with("usage:") => Output {
            status: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
        Err(message) => error(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context(temp: &Path) -> Context {
        Context {
            root: temp.join("root"),
            home: temp.join("active"),
            data: temp.join("active/data"),
            projects: temp.join("active/projects"),
            state: temp.join("active/state"),
        }
    }

    #[test]
    fn registry_rejects_duplicate_ids_homes_and_nesting() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("daemons.md");
        fs::write(
            &registry,
            format!(
                "- a - a (home: {}/one; scope: a; projects: p; added 2026-01-01)\n- b - b (home: {}/one/child; scope: b; projects: p; added 2026-01-01)\n",
                temp.path().display(),
                temp.path().display()
            ),
        )
        .expect("registry");
        assert!(validate_registry(&registry).is_err());
        fs::write(
            &registry,
            format!(
                "- a - a (home: {}/one; scope: a; projects: p; added 2026-01-01)\n- a - b (home: {}/two; scope: b; projects: p; added 2026-01-01)\n",
                temp.path().display(),
                temp.path().display()
            ),
        )
        .expect("registry");
        assert!(validate_registry(&registry).is_err());
    }

    #[test]
    fn recovery_removes_a_pre_mutation_journal_with_no_home() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = test_context(temp.path());
        fs::create_dir_all(&context.data).expect("data");
        let transaction = journal_path(&context, "crash");
        fs::create_dir(&transaction).expect("transaction");
        fs::set_permissions(&transaction, fs::Permissions::from_mode(0o700))
            .expect("transaction mode");
        publish_journal(
            &transaction,
            &SeedJournal {
                state: "prepared".to_owned(),
                id: "crash".to_owned(),
                home: String::new(),
                created_home: false,
                acquired_home: false,
                created_projects: Vec::new(),
                originals: Vec::new(),
            },
        )
        .expect("journal");

        recover(&context).expect("recovery");

        assert!(!transaction.exists());
    }
}

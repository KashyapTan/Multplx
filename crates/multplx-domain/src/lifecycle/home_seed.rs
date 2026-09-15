//! Persistent sub-agent home validation and transactional seeding.

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

pub const USAGE: &str = "Seed a persistent sub-agent home; ownership survives idle sessions.\nusage: mx home-seed <id> <home|-> {<project>...|--no-projects} [--git-allocation PROJECT ALLOCATION]\n       mx home-seed validate\nA new home is a private directory using installed runtime assets.\nFor a deliberate Git-backed home, first acquire a persistent worktree for this id, then pass its exact path and allocation.\n";

const MARKER: &str = ".mx-daemon-home";
const TRANSACTION_PREFIX: &str = ".home-seed.transaction.";

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HomeBinding {
    pub id: String,
    pub owner_home: PathBuf,
    pub path: PathBuf,
    pub lease_id: String,
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HomeAllocation {
    pub version: u32,
    pub binding: HomeBinding,
    pub runtime_root: PathBuf,
    pub state: String,
    pub retained_path: Option<PathBuf>,
    #[serde(default)]
    pub directory_identity: Option<(u64, u64)>,
    #[serde(default)]
    pub git_allocation: Option<super::worktree::Allocation>,
}

pub fn read_home_allocation(data: &Path, id: &str) -> Result<Option<HomeAllocation>, String> {
    TaskId::parse(id).map_err(|e| e.to_string())?;
    let path = data.join(format!(".home-allocation-{id}.json"));
    if !path.exists() && fs::symlink_metadata(&path).is_err() {
        return Ok(None);
    }
    let bytes = multplx_core::filesystem::read_bounded_regular(&path, 1024 * 1024)
        .map_err(|e| e.to_string())?;
    let allocation: HomeAllocation = serde_json::from_slice(&bytes)
        .map_err(|e| format!("corrupt home allocation; retained: {e}"))?;
    if allocation.version != 1
        || allocation.binding.id != id
        || allocation.binding.generation == 0
        || allocation.binding.lease_id.is_empty()
        || allocation.binding.lease_id.len() > 128
        || !allocation
            .binding
            .lease_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || !allocation.binding.path.is_absolute()
        || !allocation.binding.owner_home.is_absolute()
        || !matches!(
            allocation.state.as_str(),
            "reserved" | "active" | "archiving" | "retired"
        )
    {
        return Err("invalid home allocation; retained".into());
    }
    Ok(Some(allocation))
}

/// Retirement preserves private operational material in an explicit archive.
/// Exact lease identity fences delayed teardown after a home path is reused.
pub fn retire_home(data: &Path, token: &HomeBinding) -> Result<PathBuf, String> {
    let mut allocation =
        read_home_allocation(data, &token.id)?.ok_or("home allocation missing; retained")?;
    if allocation.binding != *token {
        return Err("stale home allocation lease; retained".into());
    }
    let archive = token
        .owner_home
        .parent()
        .ok_or("owner home parent missing")?
        .join(format!(
            ".multplx-retired-homes-{}",
            &crate::maintainer_override::sha256_text(&token.owner_home.to_string_lossy())[..16]
        ))
        .join(&token.lease_id);
    if allocation.git_allocation.is_none()
        && allocation
            .retained_path
            .as_ref()
            .is_some_and(|path| path != &archive)
    {
        return Err("home archive identity mismatch".into());
    }
    if allocation.state == "retired" {
        return allocation
            .retained_path
            .ok_or("retired home has no retained path".into());
    }
    if let Some(git) = &allocation.git_allocation {
        let store = super::worktree::Store::new(&git.project)?;
        let current = store.inspect(&git.binding.allocation_id)?;
        if current.binding != git.binding
            || current.owner_home != token.owner_home
            || !current.binding.persistent
            || Path::new(&current.binding.path) != token.path
        {
            return Err("Git-backed home ownership changed; retained".into());
        }
        verify_home_directory(&allocation, &token.path)?;
        super::worktree::occupants(&token.path)?;
        store.retain(
            &git.binding,
            "retired persistent home; private material retained",
        )?;
        let _lock = home_publication_lock(data, token)?;
        allocation.state = "retired".into();
        allocation.retained_path = Some(token.path.clone());
        atomic_replace(
            data.join(format!(".home-allocation-{}.json", token.id)),
            &serde_json::to_vec(&allocation).map_err(|e| e.to_string())?,
            0o600,
        )
        .map_err(|e| e.to_string())?;
        return Ok(token.path.clone());
    }
    if token.path.exists() {
        verify_home_directory(&allocation, &token.path)?;
        let metadata = fs::symlink_metadata(&token.path).map_err(|e| e.to_string())?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || resolved(&token.path) != token.path
        {
            return Err("unsafe private home path; retained".into());
        }
        super::worktree::occupants(&token.path)?;
        let _lock = home_publication_lock(data, token)?;
        if fs::symlink_metadata(&archive).is_ok() {
            return Err("archive path already occupied; retain both paths".into());
        }
        fs::create_dir_all(archive.parent().ok_or("archive parent missing")?)
            .map_err(|e| e.to_string())?;
        if resolved(archive.parent().expect("archive parent"))
            != archive.parent().expect("archive parent")
        {
            return Err("archive parent traverses a symlink".into());
        }
        allocation.state = "archiving".into();
        allocation.retained_path = Some(archive.clone());
        atomic_replace(
            data.join(format!(".home-allocation-{}.json", token.id)),
            &serde_json::to_vec(&allocation).map_err(|e| e.to_string())?,
            0o600,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(&token.path, &archive)
            .map_err(|e| format!("home archive move failed; retained: {e}"))?;
    } else if allocation.state != "archiving" || !archive.is_dir() {
        return Err("home/archive disposition cannot be proven".into());
    }
    verify_home_directory(&allocation, &archive)?;
    let _lock = home_publication_lock(data, token)?;
    allocation.state = "retired".into();
    allocation.retained_path = Some(archive.clone());
    atomic_replace(
        data.join(format!(".home-allocation-{}.json", token.id)),
        &serde_json::to_vec(&allocation).map_err(|e| e.to_string())?,
        0o600,
    )
    .map_err(|e| e.to_string())?;
    Ok(archive)
}

// Home seed and retirement share a short publication lock. All external Git and
// occupant probes run before acquiring it; exact ownership is checked again.
fn home_publication_lock(data: &Path, token: &HomeBinding) -> Result<DirectoryLock, String> {
    let lock = DirectoryLock::acquire_wait(
        data.join(".home-seed.lock"),
        &SystemProcessProbe::default(),
        Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())?;
    let current =
        read_home_allocation(data, &token.id)?.ok_or("home receipt disappeared; retained")?;
    if current.binding != *token {
        return Err("stale home allocation lease; retained".into());
    }
    Ok(lock)
}

fn verify_home_directory(allocation: &HomeAllocation, path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || resolved(path) != path
        || allocation.directory_identity != Some((metadata.dev(), metadata.ino()))
    {
        return Err("home directory identity changed or unproven; retained".into());
    }
    Ok(())
}

/// Verify the leased directory before starting a new persistent process.
pub fn verify_active_home(allocation: &HomeAllocation) -> Result<(), String> {
    if allocation.state != "active" {
        return Err("persistent home is not active".into());
    }
    verify_home_directory(allocation, &allocation.binding.path)?;
    if let Some(git) = &allocation.git_allocation {
        let store = super::worktree::Store::new(&git.project)?;
        let current = store.inspect(&git.binding.allocation_id)?;
        if current.binding != git.binding
            || current.state != super::worktree::State::Active
            || current.owner_home != allocation.binding.owner_home
            || !current.binding.persistent
            || Path::new(&current.binding.path) != allocation.binding.path
        {
            return Err("persistent Git home allocation changed; retained".into());
        }
        if store
            .list()?
            .iter()
            .any(|o| o.path == allocation.binding.path && o.error.is_some())
        {
            return Err("persistent Git home identity cannot be verified".into());
        }
    }
    Ok(())
}

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
    #[serde(default)]
    home_binding: Option<HomeBinding>,
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
    let output = super::worktree::command_output(&mut command)
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

// Read both charter headings during the lean prompt transition, without allowing
// conflicting or duplicated sections to hide a project-bearing charter.
fn projectless_charter(text: &str) -> bool {
    let headings = ["# Project references", "# Project clones"];
    let found: Vec<_> = text
        .lines()
        .filter(|line| headings.contains(line))
        .collect();
    if found.len() != 1 {
        return false;
    }
    let projects = section(text, found[0].trim_start_matches("# "));
    projects.contains("None. This is a project-less domain")
        && !projects
            .lines()
            .any(|line| line.trim_start().starts_with("- "))
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
    let local = context.projects.join(project);
    let source = if local.is_dir() {
        local
    } else {
        crate::project_registry::resolve_checkout(&context.home, project)
            .map_err(|_| {
                format!(
                    "project {project} not found at {} or in the canonical catalog",
                    local.display()
                )
            })?
            .canonical_path
    };
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
    .unwrap_or_default();
    if origin.is_empty() {
        return Ok((source, String::new()));
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
    if (journal.acquired_home || journal.created_home)
        && home.exists()
        && safe_created_home(context, &home)
    {
        warnings.push(format!("warning: retained unfinished home {} after seed rollback; inspect and retry the same home", home.display()));
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
            home.join("data/projects.json"),
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
                            | "backup-project-catalog"
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
            for warning in &warnings {
                eprintln!("{warning}");
            }
            if warnings
                .iter()
                .any(|warning| !warning.starts_with("warning: retained unfinished home"))
            {
                return Err(warnings.join("\n"));
            }
        }
        if journal.state == "committed" {
            activate_home(context, &journal)?;
        }
        fs::remove_dir_all(&transaction).map_err(|error_value| error_value.to_string())?;
    }
    Ok(())
}

fn activate_home(context: &Context, journal: &SeedJournal) -> Result<(), String> {
    if let Some(mut allocation) = read_home_allocation(&context.data, &journal.id)? {
        if allocation.binding.path != Path::new(&journal.home)
            || allocation.binding.owner_home != resolved(&context.home)
            || journal.home_binding.as_ref() != Some(&allocation.binding)
        {
            return Err("home allocation identity changed".into());
        }
        verify_home_directory(&allocation, Path::new(&journal.home))?;
        allocation.state = "active".into();
        atomic_replace(
            context
                .data
                .join(format!(".home-allocation-{}.json", journal.id)),
            &serde_json::to_vec(&allocation).map_err(|e| e.to_string())?,
            0o600,
        )
        .map_err(|e| e.to_string())?;
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
    let mut git_request = None;
    let mut arguments = args[2..].iter();
    while let Some(arg) = arguments.next() {
        let value = arg.to_str().ok_or("project name is not valid UTF-8")?;
        if value == "--no-projects" {
            no_projects = true;
        } else if value == "--git-allocation" {
            if git_request.is_some() {
                return Err("duplicate --git-allocation".into());
            }
            let project = arguments
                .next()
                .and_then(|a| a.to_str())
                .ok_or("--git-allocation requires PROJECT ALLOCATION")?;
            let allocation = arguments
                .next()
                .and_then(|a| a.to_str())
                .ok_or("--git-allocation requires PROJECT ALLOCATION")?;
            git_request = Some((project, allocation));
        } else if value.starts_with('-') {
            return Err(format!("unknown home-seed option: {value}"));
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
    let existing_brief = context.data.join(id).join("brief.md");
    if existing_brief.is_file() {
        charter_fields(&existing_brief)?;
    } else {
        match env::var("MX_DAEMON_CHARTER").ok() {
            None => {
                return Err(format!(
                    "no filled daemon charter brief at {}; set MX_DAEMON_CHARTER or scaffold one and replace {{TASK}}",
                    existing_brief.display()
                ));
            }
            Some(value) if normalize_registry_text(&value).is_empty() => {
                return Err("empty Charter section".into());
            }
            _ => {}
        }
        if env::var("MX_DAEMON_SCOPE").is_ok_and(|value| normalize_registry_text(&value).is_empty())
        {
            return Err("empty Routing scope section".into());
        }
    }
    let mut references = Vec::new();
    for project in &projects {
        let (source, _) = project_origin(context, project)?;
        let binding = crate::project_registry::bind_project_at(
            &context.home,
            &context.data,
            &context.projects,
            &source,
        )?;
        let catalog = crate::project_registry::read_catalog(&context.home)?;
        let record = catalog
            .projects
            .into_iter()
            .find(|p| p.project_id == binding.project_id)
            .ok_or("project reference missing")?;
        references.push((project.clone(), binding, record));
    }

    // Resolve and verify the deliberate Git allocation before the home publication lock.
    // This never adopts an arbitrary existing Git checkout.
    let git_allocation = if let Some((selector, allocation_id)) = git_request {
        if requested == Path::new("-") {
            return Err("--git-allocation requires its exact home path".into());
        }
        let project = if Path::new(selector).exists() {
            crate::project_registry::bind_project_at(
                &context.home,
                &context.data,
                &context.projects,
                Path::new(selector),
            )?
        } else {
            crate::project_registry::resolve_checkout(&context.home, selector)?
        };
        let store = super::worktree::Store::new(&project)?;
        let allocation = store.inspect(allocation_id)?;
        if allocation.state != super::worktree::State::Active
            || !allocation.binding.persistent
            || allocation.binding.task_id != id
            || allocation.owner_home != resolved(&context.home)
            || Path::new(&allocation.binding.path) != resolved(&requested)
        {
            return Err("Git home requires an active persistent allocation owned by this home and task at the exact requested path".into());
        }
        if store
            .list()?
            .iter()
            .any(|o| o.path == resolved(&requested) && o.error.is_some())
        {
            return Err("Git home allocation identity cannot be verified".into());
        }
        Some(allocation)
    } else {
        None
    };

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
        home_binding: None,
        created_projects: Vec::new(),
        originals: Vec::new(),
    };
    publish_journal(&transaction, &journal)?;

    let operation = (|| -> Result<String, String> {
        let home = if journal.acquired_home {
            let active = resolved(&context.home);
            let parent = active.parent().ok_or("active home has no parent")?;
            parent
                .join(format!(
                    ".multplx-homes-{}",
                    &crate::maintainer_override::sha256_text(&active.to_string_lossy())[..16]
                ))
                .join(id)
        } else {
            resolved(&requested)
        };
        let home = validate_home_boundary(context, &home)?;
        journal.home = path_text(&home, "daemon home")?;
        journal.created_home = !home.exists();
        publish_journal(&transaction, &journal)?;
        validate_assignment(&context.data.join("daemons.md"), id, &home)?;
        if let Some(prior) = read_home_allocation(&context.data, id)? {
            if prior.binding.owner_home != resolved(&context.home) {
                return Err("home allocation belongs to another owner; retained".into());
            }
            if prior.state == "active" {
                if prior.binding.path != home {
                    return Err("active home allocation path changed; retained".into());
                }
                verify_home_directory(&prior, &home)?;
            }
            if prior.state == "retired" && home.exists() && git_allocation.is_none() {
                return Err(
                    "retired home path is occupied; retained until explicitly reconciled".into(),
                );
            }
        }
        let resume_private = read_home_allocation(&context.data, id)?
            .is_some_and(|a| a.state == "reserved" && a.binding.path == home);
        if journal.created_home || resume_private || git_allocation.is_some() {
            let receipt = context.data.join(format!(".home-allocation-{id}.json"));
            let previous = read_home_allocation(&context.data, id)?;
            if previous
                .as_ref()
                .is_some_and(|p| p.state == "reserved" && p.directory_identity.is_none())
                && home.exists()
                && git_allocation.is_none()
            {
                return Err(
                    "unfinished home path has no directory identity; retained for reconciliation"
                        .into(),
                );
            }
            let generation = match previous.as_ref() {
                Some(prior) if prior.state == "retired" => prior
                    .binding
                    .generation
                    .checked_add(1)
                    .ok_or("home generation exhausted")?,
                Some(prior) if prior.state == "reserved" && prior.binding.path == home => {
                    prior.binding.generation
                }
                Some(prior)
                    if prior.state == "active"
                        && prior.binding.path == home
                        && prior.git_allocation.as_ref().map(|a| &a.binding)
                            == git_allocation.as_ref().map(|a| &a.binding) =>
                {
                    prior.binding.generation
                }
                Some(_) => {
                    return Err("existing home allocation unresolved; retain and reconcile".into());
                }
                None => 1,
            };
            let binding = previous
                .as_ref()
                .filter(|prior| matches!(prior.state.as_str(), "reserved" | "active"))
                .map(|prior| prior.binding.clone())
                .unwrap_or_else(|| HomeBinding {
                    id: id.into(),
                    owner_home: resolved(&context.home),
                    path: home.clone(),
                    generation,
                    lease_id: super::subagent_model::new_identity("home"),
                });
            let mut allocation = HomeAllocation {
                version: 1,
                binding,
                runtime_root: resolved(&context.root),
                state: "reserved".into(),
                retained_path: None,
                directory_identity: previous
                    .as_ref()
                    .filter(|p| matches!(p.state.as_str(), "reserved" | "active"))
                    .and_then(|p| p.directory_identity),
                git_allocation: git_allocation.clone().or_else(|| {
                    previous
                        .as_ref()
                        .filter(|p| matches!(p.state.as_str(), "reserved" | "active"))
                        .and_then(|p| p.git_allocation.clone())
                }),
            };
            if allocation.directory_identity.is_some() {
                verify_home_directory(&allocation, &home)?;
            }
            atomic_replace(
                &receipt,
                &serde_json::to_vec(&allocation).map_err(|e| e.to_string())?,
                0o600,
            )
            .map_err(|e| e.to_string())?;
            if journal.created_home {
                fs::create_dir_all(home.parent().ok_or("home parent missing")?)
                    .map_err(|e| e.to_string())?;
                fs::create_dir(&home)
                    .map_err(|e| format!("home path appeared during reservation; retained: {e}"))?;
            }
            fs::set_permissions(&home, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
            let directory = fs::symlink_metadata(&home).map_err(|e| e.to_string())?;
            allocation.directory_identity = Some((directory.dev(), directory.ino()));
            atomic_replace(
                &receipt,
                &serde_json::to_vec(&allocation).map_err(|e| e.to_string())?,
                0o600,
            )
            .map_err(|e| e.to_string())?;
            // Runtime assets remain at their installation owner; homes contain
            // private coordination/configuration state, never a runtime clone.
            for name in ["bin", "share", ".agents"] {
                let source = context.root.join(name);
                if source.exists() && fs::symlink_metadata(home.join(name)).is_err() {
                    std::os::unix::fs::symlink(resolved(&source), home.join(name))
                        .map_err(|e| e.to_string())?;
                }
            }
            if !home.join("AGENTS.md").exists() {
                fs::copy(context.root.join("AGENTS.md"), home.join("AGENTS.md"))
                    .map_err(|e| format!("runtime contract unavailable: {e}"))?;
            }
        }
        let home = verify_broker_home(context, &home)?;
        journal.home = path_text(&home, "daemon home")?;
        journal.home_binding = read_home_allocation(&context.data, id)?.map(|a| a.binding);
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
                if !projectless_charter(&text) {
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
            ("project-catalog", home.join("data/projects.json")),
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
            if !projectless_charter(&text) {
                return Err(format!(
                    "cannot seed project-less daemon home because existing charter brief at {} conflicts with --no-projects\nerror: re-scaffold it with mx-brief.sh {id} --daemon --no-projects or remove the stale brief before seeding",
                    parent_brief.display()
                ));
            }
        }
        let (summary, scope) = charter_fields(&parent_brief)?;
        injected("brief")?;

        for (alias, binding, record) in &references {
            let local = home.join("projects").join(alias);
            if fs::symlink_metadata(&local).is_ok() && resolved(&local) != binding.canonical_path {
                return Err(format!(
                    "existing project checkout conflicts with borrowed reference {alias}: {}; retain and reconcile",
                    local.display()
                ));
            }
            crate::project_registry::remember_reference(&home, record, binding, alias)?;
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
            activate_home(context, &journal)?;
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

    fn owned_home(context: &Context, path: &Path, generation: u64) -> HomeAllocation {
        fs::create_dir_all(&context.data).unwrap();
        fs::create_dir_all(path).unwrap();
        let metadata = fs::metadata(path).unwrap();
        let allocation = HomeAllocation {
            version: 1,
            binding: HomeBinding {
                id: "durable".into(),
                owner_home: resolved(&context.home),
                path: resolved(path),
                lease_id: super::super::subagent_model::new_identity("home"),
                generation,
            },
            runtime_root: resolved(&context.root),
            state: "active".into(),
            retained_path: None,
            directory_identity: Some((metadata.dev(), metadata.ino())),
            git_allocation: None,
        };
        write_allocation(context, &allocation);
        allocation
    }

    fn write_allocation(context: &Context, allocation: &HomeAllocation) {
        atomic_replace(
            context.data.join(".home-allocation-durable.json"),
            &serde_json::to_vec(allocation).unwrap(),
            0o600,
        )
        .unwrap();
    }

    fn projectless_seed_fixture(context: &Context) {
        fs::create_dir_all(context.root.join("bin")).unwrap();
        fs::create_dir_all(context.data.join("durable")).unwrap();
        fs::write(context.root.join("AGENTS.md"), "fixture runtime").unwrap();
        fs::write(context.data.join("durable/brief.md"), "# Charter\nNone. This is a project-less domain\n# Project references\nNone. This is a project-less domain\n# Routing scope\nMeasurement domain\n").unwrap();
    }

    #[test]
    fn seeding_a_retired_private_home_uses_a_new_generation_and_directory() {
        let temp = tempfile::tempdir().unwrap();
        let context = test_context(temp.path());
        projectless_seed_fixture(&context);
        let path = resolved(&temp.path().join("private"));
        let args = [
            "durable".into(),
            path.as_os_str().to_owned(),
            "--no-projects".into(),
        ];
        seed(&args, &context).unwrap();
        let first = read_home_allocation(&context.data, "durable")
            .unwrap()
            .unwrap();
        fs::write(path.join("unfinished"), "old progress").unwrap();
        let archive = retire_home(&context.data, &first.binding).unwrap();
        seed(&args, &context).unwrap();
        let next = read_home_allocation(&context.data, "durable")
            .unwrap()
            .unwrap();
        assert_eq!(next.binding.generation, first.binding.generation + 1);
        assert_ne!(next.binding.lease_id, first.binding.lease_id);
        assert_ne!(next.directory_identity, first.directory_identity);
        assert!(next.git_allocation.is_none());
        assert_eq!(next.state, "active");
        assert_eq!(
            fs::read_to_string(archive.join("unfinished")).unwrap(),
            "old progress"
        );
        assert!(retire_home(&context.data, &first.binding).is_err());
    }

    #[test]
    fn malformed_home_lease_and_replaced_active_directory_cannot_redirect_writes() {
        let temp = tempfile::tempdir().unwrap();
        let context = test_context(temp.path());
        projectless_seed_fixture(&context);
        let path = resolved(&temp.path().join("private"));
        let args = [
            "durable".into(),
            path.as_os_str().to_owned(),
            "--no-projects".into(),
        ];
        seed(&args, &context).unwrap();
        let first = read_home_allocation(&context.data, "durable")
            .unwrap()
            .unwrap();
        let mut corrupt = first.clone();
        corrupt.binding.lease_id = "../foreign".into();
        write_allocation(&context, &corrupt);
        assert!(
            retire_home(&context.data, &corrupt.binding)
                .unwrap_err()
                .contains("invalid home allocation")
        );
        write_allocation(&context, &first);
        let old = path.with_file_name("preserved-old-home");
        fs::rename(&path, &old).unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("foreign"), "keep").unwrap();
        assert!(
            seed(&args, &context)
                .unwrap_err()
                .contains("directory identity")
        );
        assert_eq!(fs::read_to_string(path.join("foreign")).unwrap(), "keep");
        assert!(!path.join("AGENTS.md").exists());
        assert!(old.join("AGENTS.md").exists());
    }

    #[test]
    fn private_retirement_preserves_material_and_fences_reused_path() {
        let temp = tempfile::tempdir().unwrap();
        let context = test_context(temp.path());
        let path = temp.path().join("private");
        let first = owned_home(&context, &path, 1);
        fs::write(path.join("unfinished"), "keep").unwrap();
        let archive = retire_home(&context.data, &first.binding).unwrap();
        assert_eq!(
            fs::read_to_string(archive.join("unfinished")).unwrap(),
            "keep"
        );
        assert!(!path.exists());
        assert_eq!(retire_home(&context.data, &first.binding).unwrap(), archive);
        let second = owned_home(&context, &path, 2);
        fs::write(path.join("new"), "new owner").unwrap();
        assert!(
            retire_home(&context.data, &first.binding)
                .unwrap_err()
                .contains("stale")
        );
        assert!(path.join("new").exists());
        assert_ne!(first.binding.lease_id, second.binding.lease_id);
    }

    #[test]
    fn retirement_reconciles_rename_before_receipt_and_refuses_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let context = test_context(temp.path());
        let path = temp.path().join("private");
        let mut allocation = owned_home(&context, &path, 1);
        let archive = retire_home(&context.data, &allocation.binding).unwrap();
        allocation.state = "archiving".into();
        allocation.retained_path = Some(archive.clone());
        write_allocation(&context, &allocation);
        assert_eq!(
            retire_home(&context.data, &allocation.binding).unwrap(),
            archive
        );
        let second = owned_home(&context, &path, 2);
        fs::rename(&path, temp.path().join("original")).unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(path.join("foreign"), "keep").unwrap();
        assert!(
            retire_home(&context.data, &second.binding)
                .unwrap_err()
                .contains("identity")
        );
        assert!(path.join("foreign").exists());
        fs::remove_dir_all(&path).unwrap();
        std::os::unix::fs::symlink(temp.path().join("original"), &path).unwrap();
        assert!(retire_home(&context.data, &second.binding).is_err());
    }

    #[test]
    fn deliberate_git_home_retirement_keeps_worktree_and_persistent_reservation() {
        let temp = tempfile::tempdir().unwrap();
        let context = test_context(temp.path());
        fs::create_dir_all(&context.home).unwrap();
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        for args in [
            vec!["init", "-q"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "base",
                "--allow-empty",
            ],
        ] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(&project)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let project = crate::project_registry::bind_project(&context.home, &project).unwrap();
        let store = super::super::worktree::Store::new(&project).unwrap();
        let git = store
            .acquire(
                &super::super::worktree::Acquire {
                    request_id: "home",
                    owner_home: &context.home,
                    project: &project,
                    task_id: "durable",
                    attempt_id: "home-attempt",
                    persistent: true,
                },
                None,
            )
            .unwrap();
        let path = PathBuf::from(&git.binding.path);
        let mut home = owned_home(&context, &path, 1);
        home.git_allocation = Some(git.clone());
        write_allocation(&context, &home);
        fs::write(path.join("private-state"), "preserve").unwrap();
        assert_eq!(retire_home(&context.data, &home.binding).unwrap(), path);
        assert_eq!(retire_home(&context.data, &home.binding).unwrap(), path);
        assert!(path.join("private-state").exists());
        assert_eq!(
            store.inspect(&git.binding.allocation_id).unwrap().state,
            super::super::worktree::State::Retained
        );
        assert!(store.release(&git.binding).is_err());
    }

    #[test]
    fn projectless_charters_accept_both_versions_and_reject_ambiguity() {
        for heading in ["Project clones", "Project references"] {
            let text = format!(
                "# Charter\nNone. This is a project-less domain\n# {heading}\nNone. This is a project-less domain; select later.\n# Coordination\nUse a scoped task.\n"
            );
            assert!(projectless_charter(&text));
            assert!(!projectless_charter(
                &text.replace("select later.", "select later.\n- project")
            ));
            assert!(!projectless_charter(&format!(
                "{text}# {heading}\n- hidden\n"
            )));
        }
        assert!(!projectless_charter(
            "# Charter\nNone. This is a project-less domain\n# Project references\n- actual-project\n"
        ));
        assert!(!projectless_charter(
            "# Project clones\n- old-project\n# Project references\nNone. This is a project-less domain\n"
        ));
        assert!(!projectless_charter(
            "# Charter\nNone. This is a project-less domain\n"
        ));
    }

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
                home_binding: None,
                created_projects: Vec::new(),
                originals: Vec::new(),
            },
        )
        .expect("journal");

        recover(&context).expect("recovery");

        assert!(!transaction.exists());
    }
}

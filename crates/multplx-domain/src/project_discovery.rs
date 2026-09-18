//! Bounded, cached discovery of configured local project roots.
//!
//! Discovery only reads filesystem and Git metadata. It never registers a
//! checkout, loads repository instructions, or runs project content.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::project_registry::{self, ProjectCatalog};

pub const DISCOVERY_SCHEMA_VERSION: u32 = 1;
const CONFIG_FILE: &str = "project-discovery.json";
const CACHE_FILE: &str = "project-discovery-cache.json";
const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIRECTORIES: u64 = 50_000;
const DEFAULT_EXCLUSIONS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "vendor",
    ".cache",
    ".venv",
    "__pycache__",
];
const PROJECT_MARKERS: &[&str] = &[
    "Cargo.toml",
    "go.mod",
    "package.json",
    "pyproject.toml",
    "pom.xml",
    "build.gradle",
    "Makefile",
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryRoot {
    pub path: PathBuf,
    pub max_depth: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryConfig {
    pub schema_version: u32,
    pub roots: Vec<DiscoveryRoot>,
    pub exclusions: Vec<String>,
    pub follow_symlinks: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            schema_version: DISCOVERY_SCHEMA_VERSION,
            roots: Vec::new(),
            exclusions: DEFAULT_EXCLUSIONS.iter().map(ToString::to_string).collect(),
            follow_symlinks: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateKind {
    Git,
    Unversioned,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryCandidate {
    pub canonical_path: PathBuf,
    pub display_name: String,
    pub kind: CandidateKind,
    pub project_id: Option<String>,
    pub checkout_id: Option<String>,
    pub parent_repository: Option<PathBuf>,
    pub registered: bool,
    pub task_ready: bool,
    pub limitation: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanStatus {
    Scanning,
    Complete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryCache {
    pub schema_version: u32,
    pub status: ScanStatus,
    pub scanned_directories: u64,
    pub completed_roots: usize,
    pub total_roots: usize,
    pub updated_at: u64,
    pub candidates: Vec<DiscoveryCandidate>,
    pub errors: Vec<String>,
}

impl DiscoveryCache {
    fn empty(total_roots: usize) -> Self {
        Self {
            schema_version: DISCOVERY_SCHEMA_VERSION,
            status: ScanStatus::Scanning,
            scanned_directories: 0,
            completed_roots: 0,
            total_roots,
            updated_at: now(),
            candidates: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != DISCOVERY_SCHEMA_VERSION
            || self.completed_roots > self.total_roots
            || self.scanned_directories > MAX_DIRECTORIES
        {
            return Err("invalid project discovery cache".into());
        }
        let mut paths = HashSet::new();
        if self.candidates.iter().any(|candidate| {
            !candidate.canonical_path.is_absolute() || !paths.insert(&candidate.canonical_path)
        }) {
            return Err("invalid or duplicate discovery candidate".into());
        }
        Ok(())
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn config_path(config: &Path) -> PathBuf {
    config.join(CONFIG_FILE)
}

fn cache_path(data: &Path) -> PathBuf {
    data.join(CACHE_FILE)
}

fn read_json(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    multplx_core::filesystem::read_bounded_regular(path, limit).map_err(|error| error.to_string())
}

pub fn read_config(config: &Path) -> Result<DiscoveryConfig, String> {
    let path = config_path(config);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DiscoveryConfig::default());
        }
        Err(error) => return Err(error.to_string()),
        Ok(_) => {}
    }
    let value: DiscoveryConfig = serde_json::from_slice(&read_json(&path, MAX_CONFIG_BYTES)?)
        .map_err(|error| error.to_string())?;
    validate_config(&value)?;
    Ok(value)
}

fn validate_config(config: &DiscoveryConfig) -> Result<(), String> {
    if config.schema_version != DISCOVERY_SCHEMA_VERSION {
        return Err("unsupported project discovery configuration version".into());
    }
    let mut roots = HashSet::new();
    if config.roots.iter().any(|root| {
        !root.path.is_absolute() || root.max_depth > 32 || !roots.insert(root.path.clone())
    }) {
        return Err("invalid or duplicate discovery root".into());
    }
    if config.exclusions.iter().any(|value| {
        value.is_empty() || value == "." || value == ".." || value.contains(['/', '\0'])
    }) {
        return Err("discovery exclusions must be filename components".into());
    }
    Ok(())
}

fn write_config(config_dir: &Path, value: &DiscoveryConfig) -> Result<(), String> {
    validate_config(value)?;
    fs::create_dir_all(config_dir).map_err(|error| error.to_string())?;
    multplx_core::filesystem::atomic_replace(
        config_path(config_dir),
        &serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?,
        0o600,
    )
    .map_err(|error| error.to_string())
}

pub fn add_root(config_dir: &Path, path: &Path, max_depth: u16) -> Result<DiscoveryConfig, String> {
    if max_depth > 32 {
        return Err("discovery depth must be between 0 and 32".into());
    }
    let path = fs::canonicalize(path)
        .map_err(|error| format!("discovery root {} is unavailable: {error}", path.display()))?;
    if path.parent().is_none() {
        return Err("the filesystem root cannot be a discovery root".into());
    }
    fs::create_dir_all(config_dir).map_err(|error| error.to_string())?;
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        config_dir.join(".project-discovery.lock"),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    let mut config = read_config(config_dir)?;
    if let Some(root) = config.roots.iter_mut().find(|root| root.path == path) {
        root.max_depth = max_depth;
    } else {
        config.roots.push(DiscoveryRoot { path, max_depth });
        config
            .roots
            .sort_by(|left, right| left.path.cmp(&right.path));
    }
    write_config(config_dir, &config)?;
    Ok(config)
}

pub fn remove_root(config_dir: &Path, path: &Path) -> Result<DiscoveryConfig, String> {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    fs::create_dir_all(config_dir).map_err(|error| error.to_string())?;
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        config_dir.join(".project-discovery.lock"),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    let mut config = read_config(config_dir)?;
    let before = config.roots.len();
    config
        .roots
        .retain(|root| root.path != canonical && root.path != path);
    if config.roots.len() == before {
        return Err(format!(
            "discovery root {} is not configured",
            path.display()
        ));
    }
    write_config(config_dir, &config)?;
    Ok(config)
}

pub fn set_options(
    config_dir: &Path,
    exclusions: Option<Vec<String>>,
    follow_symlinks: Option<bool>,
) -> Result<DiscoveryConfig, String> {
    fs::create_dir_all(config_dir).map_err(|error| error.to_string())?;
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        config_dir.join(".project-discovery.lock"),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    let mut config = read_config(config_dir)?;
    if let Some(exclusions) = exclusions {
        config.exclusions = exclusions;
    }
    if let Some(follow) = follow_symlinks {
        config.follow_symlinks = follow;
    }
    write_config(config_dir, &config)?;
    Ok(config)
}

pub fn read_cache(data: &Path) -> Result<Option<DiscoveryCache>, String> {
    let path = cache_path(data);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(_) => {}
    }
    let cache: DiscoveryCache = serde_json::from_slice(&read_json(&path, MAX_CACHE_BYTES)?)
        .map_err(|error| error.to_string())?;
    cache.validate()?;
    Ok(Some(cache))
}

fn write_cache(data: &Path, cache: &DiscoveryCache) -> Result<(), String> {
    cache.validate()?;
    fs::create_dir_all(data).map_err(|error| error.to_string())?;
    multplx_core::filesystem::atomic_replace(
        cache_path(data),
        &serde_json::to_vec_pretty(cache).map_err(|error| error.to_string())?,
        0o600,
    )
    .map_err(|error| error.to_string())
}

fn directory_identity(path: &Path) -> Result<(u64, u64), String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    Ok((metadata.dev(), metadata.ino()))
}

fn is_excluded(path: &Path, exclusions: &BTreeSet<&str>) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == ".git" || exclusions.contains(name))
}

fn marker_project(path: &Path) -> bool {
    PROJECT_MARKERS
        .iter()
        .any(|marker| path.join(marker).is_file())
}

fn registered(catalog: &ProjectCatalog, checkout_id: &str) -> bool {
    catalog
        .projects
        .iter()
        .flat_map(|project| &project.checkouts)
        .any(|checkout| checkout.checkout_id == checkout_id)
}

fn parent_repository(path: &Path, repositories: &[PathBuf]) -> Option<PathBuf> {
    repositories
        .iter()
        .filter(|candidate| candidate.as_path() != path && path.starts_with(candidate))
        .max_by_key(|candidate| candidate.components().count())
        .cloned()
}

fn write_progress(
    data: &Path,
    cache: &mut DiscoveryCache,
    prior: &[DiscoveryCandidate],
    refreshed: &[DiscoveryCandidate],
) -> Result<(), String> {
    let paths = refreshed
        .iter()
        .map(|candidate| candidate.canonical_path.clone())
        .collect::<HashSet<_>>();
    cache.candidates = refreshed
        .iter()
        .cloned()
        .chain(
            prior
                .iter()
                .filter(|candidate| !paths.contains(&candidate.canonical_path))
                .cloned(),
        )
        .collect();
    cache.updated_at = now();
    write_cache(data, cache)
}

fn record_error(cache: &mut DiscoveryCache, error: String) {
    if cache.errors.len() < 511 {
        cache.errors.push(error);
    } else if cache.errors.len() == 511 {
        cache
            .errors
            .push("additional discovery errors omitted at the 512-record bound".into());
    }
}

/// Refresh the persistent cache. A scanning snapshot retains cached rows and
/// is republished in bounded chunks, allowing a UI to poll useful progress.
pub fn refresh(home: &Path, config_dir: &Path, data: &Path) -> Result<DiscoveryCache, String> {
    fs::create_dir_all(data).map_err(|error| error.to_string())?;
    let _refresh_lock = multplx_core::locks::DirectoryLock::acquire_wait(
        data.join(".project-discovery-refresh.lock"),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|error| format!("another discovery refresh is active: {error}"))?;
    let config = read_config(config_dir)?;
    let catalog = project_registry::read_catalog(home)?;
    let mut cache = DiscoveryCache::empty(config.roots.len());
    let prior = read_cache(data)?.map_or_else(Vec::new, |cache| cache.candidates);
    cache.candidates.clone_from(&prior);
    write_cache(data, &cache)?;
    let exclusions = config
        .exclusions
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut seen_directories = HashMap::new();
    let mut seen_candidates = HashSet::new();
    let mut refreshed = Vec::new();

    for root in &config.roots {
        if !root.path.is_dir() {
            record_error(
                &mut cache,
                format!("discovery root {} is unavailable", root.path.display()),
            );
            cache.completed_roots += 1;
            cache.updated_at = now();
            write_cache(data, &cache)?;
            continue;
        }
        let mut queue = VecDeque::from([(root.path.clone(), 0_u16)]);
        while let Some((path, depth)) = queue.pop_front() {
            if cache.scanned_directories >= MAX_DIRECTORIES {
                record_error(
                    &mut cache,
                    format!("discovery stopped at the {MAX_DIRECTORIES} directory safety bound"),
                );
                queue.clear();
                break;
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(value) => value,
                Err(error) => {
                    record_error(
                        &mut cache,
                        format!("cannot inspect {}: {error}", path.display()),
                    );
                    continue;
                }
            };
            if metadata.file_type().is_symlink() && !config.follow_symlinks {
                continue;
            }
            let canonical = match fs::canonicalize(&path) {
                Ok(value) if value.is_dir() => value,
                Ok(_) => continue,
                Err(error) => {
                    record_error(
                        &mut cache,
                        format!("cannot resolve {}: {error}", path.display()),
                    );
                    continue;
                }
            };
            let identity = match directory_identity(&canonical) {
                Ok(value) => value,
                Err(error) => {
                    record_error(
                        &mut cache,
                        format!("cannot identify {}: {error}", canonical.display()),
                    );
                    continue;
                }
            };
            let remaining = root.max_depth.saturating_sub(depth);
            if seen_directories
                .get(&identity)
                .is_some_and(|prior_remaining| *prior_remaining >= remaining)
            {
                continue;
            }
            seen_directories.insert(identity, remaining);
            cache.scanned_directories += 1;

            let git_marker = canonical.join(".git");
            if git_marker.is_dir() || git_marker.is_file() {
                match project_registry::inspect_checkout(&canonical) {
                    Ok(binding) if seen_candidates.insert(binding.canonical_path.clone()) => {
                        refreshed.push(DiscoveryCandidate {
                            display_name: binding
                                .canonical_path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                            canonical_path: binding.canonical_path,
                            kind: CandidateKind::Git,
                            project_id: Some(binding.project_id),
                            checkout_id: Some(binding.checkout_id.clone()),
                            parent_repository: None,
                            registered: registered(&catalog, &binding.checkout_id),
                            task_ready: true,
                            limitation: None,
                        });
                    }
                    Ok(_) => {}
                    Err(error) => {
                        if seen_candidates.insert(canonical.clone()) {
                            refreshed.push(DiscoveryCandidate {
                                display_name: canonical
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                                canonical_path: canonical.clone(),
                                kind: CandidateKind::Git,
                                project_id: None,
                                checkout_id: None,
                                parent_repository: None,
                                registered: false,
                                task_ready: false,
                                limitation: Some(error.clone()),
                            });
                        }
                        record_error(&mut cache, error);
                    }
                }
            } else if marker_project(&canonical) {
                match project_registry::inspect_checkout(&canonical) {
                    Ok(binding) if seen_candidates.insert(binding.canonical_path.clone()) => {
                        refreshed.push(DiscoveryCandidate {
                            display_name: binding
                                .canonical_path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                            canonical_path: binding.canonical_path,
                            kind: CandidateKind::Git,
                            project_id: Some(binding.project_id),
                            checkout_id: Some(binding.checkout_id.clone()),
                            parent_repository: None,
                            registered: registered(&catalog, &binding.checkout_id),
                            task_ready: true,
                            limitation: None,
                        });
                    }
                    Ok(_) => {}
                    Err(_) if seen_candidates.insert(canonical.clone()) => {
                        refreshed.push(DiscoveryCandidate {
                            display_name: canonical
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                            canonical_path: canonical.clone(),
                            kind: CandidateKind::Unversioned,
                            project_id: None,
                            checkout_id: None,
                            parent_repository: None,
                            registered: false,
                            task_ready: false,
                            limitation: Some(
                                "unversioned project; Git worktrees and commit-bound tasks are unavailable"
                                    .into(),
                            ),
                        });
                    }
                    Err(_) => {}
                }
            }
            if cache.scanned_directories.is_multiple_of(128) {
                write_progress(data, &mut cache, &prior, &refreshed)?;
            }
            if depth >= root.max_depth {
                continue;
            }
            let mut children = match fs::read_dir(&canonical) {
                Ok(entries) => {
                    let mut children = Vec::new();
                    for entry in entries.filter_map(Result::ok) {
                        let child = entry.path();
                        if is_excluded(&child, &exclusions) {
                            continue;
                        }
                        let Ok(kind) = entry.file_type() else {
                            continue;
                        };
                        if kind.is_dir() || (kind.is_symlink() && config.follow_symlinks) {
                            children.push(child);
                        }
                        if children.len() as u64 + cache.scanned_directories >= MAX_DIRECTORIES {
                            record_error(
                                &mut cache,
                                format!(
                                    "discovery stopped adding children at the {MAX_DIRECTORIES} directory safety bound"
                                ),
                            );
                            break;
                        }
                    }
                    children
                }
                Err(error) => {
                    record_error(
                        &mut cache,
                        format!("cannot read {}: {error}", canonical.display()),
                    );
                    continue;
                }
            };
            children.sort();
            for child in children {
                queue.push_back((child, depth + 1));
            }
        }
        cache.completed_roots += 1;
        write_progress(data, &mut cache, &prior, &refreshed)?;
    }

    cache.candidates = refreshed;
    let repositories = cache
        .candidates
        .iter()
        .filter(|candidate| candidate.kind == CandidateKind::Git)
        .map(|candidate| candidate.canonical_path.clone())
        .collect::<Vec<_>>();
    for candidate in &mut cache.candidates {
        candidate.parent_repository = parent_repository(&candidate.canonical_path, &repositories);
    }
    cache
        .candidates
        .sort_by(|left, right| left.canonical_path.cmp(&right.canonical_path));
    cache.status = ScanStatus::Complete;
    cache.updated_at = now();
    write_cache(data, &cache)?;
    Ok(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::process::Command;

    fn git(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        git(path, &["init", "--quiet", "-b", "main"]);
        fs::write(path.join("file"), "base\n").unwrap();
        git(path, &["add", "file"]);
        git(
            path,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=f@example.test",
                "commit",
                "--quiet",
                "-m",
                "base",
            ],
        );
    }

    #[test]
    fn configured_nested_discovery_is_cached_bounded_and_non_mutating() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let config = home.join("config");
        let data = home.join("data");
        let root = temp.path().join("dev root");
        let outer = root.join("team/outer");
        let nested = outer.join("modules/nested");
        let linked = root.join("linked");
        let unborn = root.join("empty-repository");
        let plain = root.join("plain");
        repo(&outer);
        repo(&nested);
        git(
            &outer,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        );
        fs::create_dir_all(&unborn).unwrap();
        git(&unborn, &["init", "--quiet"]);
        fs::create_dir_all(&plain).unwrap();
        fs::write(plain.join("package.json"), "{}\n").unwrap();
        fs::create_dir_all(root.join("target/ignored")).unwrap();
        repo(&root.join("target/ignored"));
        fs::write(outer.join("dirty"), "untouched\n").unwrap();
        add_root(&config, &root, 5).unwrap();

        let cache = refresh(&home, &config, &data).unwrap();
        assert_eq!(cache.status, ScanStatus::Complete);
        assert_eq!(cache.completed_roots, 1);
        assert_eq!(cache.candidates.len(), 5);
        assert_eq!(
            cache
                .candidates
                .iter()
                .filter(|row| row.kind == CandidateKind::Git)
                .count(),
            4
        );
        assert_eq!(
            cache
                .candidates
                .iter()
                .filter(|row| row.kind == CandidateKind::Unversioned)
                .count(),
            1
        );
        let unborn_path = fs::canonicalize(&unborn).unwrap();
        let unborn = cache
            .candidates
            .iter()
            .find(|row| row.canonical_path == unborn_path)
            .unwrap();
        assert!(!unborn.task_ready);
        assert!(unborn.limitation.as_deref().unwrap().contains("unborn"));
        let nested_path = fs::canonicalize(&nested).unwrap();
        let outer_path = fs::canonicalize(&outer).unwrap();
        let nested = cache
            .candidates
            .iter()
            .find(|row| row.canonical_path == nested_path)
            .unwrap();
        assert_eq!(
            nested.parent_repository.as_deref(),
            Some(outer_path.as_path())
        );
        assert_eq!(
            fs::read_to_string(outer.join("dirty")).unwrap(),
            "untouched\n"
        );
        assert_eq!(read_cache(&data).unwrap(), Some(cache));
    }

    #[test]
    fn overlaps_and_symlink_cycles_deduplicate_and_missing_roots_remain_visible() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let config = home.join("config");
        let data = home.join("data");
        let root = temp.path().join("root");
        let project = root.join("deep/project");
        let missing = temp.path().join("missing-root");
        repo(&project);
        fs::create_dir(&missing).unwrap();
        add_root(&config, &root, 1).unwrap();
        add_root(&config, &root.join("deep"), 3).unwrap();
        add_root(&config, &missing, 1).unwrap();
        symlink(&root, root.join("cycle")).unwrap();
        set_options(&config, None, Some(true)).unwrap();
        fs::remove_dir(&missing).unwrap();

        let cache = refresh(&home, &config, &data).unwrap();
        assert_eq!(cache.candidates.len(), 1);
        assert!(
            cache
                .errors
                .iter()
                .any(|error| error.contains("unavailable"))
        );
        assert!(cache.scanned_directories < 20);
    }

    #[test]
    fn root_configuration_is_idempotent_private_and_validated() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let root = temp.path().join("root");
        fs::create_dir_all(&root).unwrap();
        add_root(&config, &root, 2).unwrap();
        let updated = add_root(&config, &root, 7).unwrap();
        assert_eq!(updated.roots.len(), 1);
        assert_eq!(updated.roots[0].max_depth, 7);
        assert_eq!(
            fs::metadata(config_path(&config))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(add_root(&config, Path::new("/"), 1).is_err());
        assert!(set_options(&config, Some(vec!["../escape".into()]), None).is_err());
        assert!(remove_root(&config, &root).unwrap().roots.is_empty());
    }

    fn candidate(path: PathBuf) -> DiscoveryCandidate {
        DiscoveryCandidate {
            display_name: "fixture".into(),
            canonical_path: path,
            kind: CandidateKind::Unversioned,
            project_id: None,
            checkout_id: None,
            parent_repository: None,
            registered: false,
            task_ready: false,
            limitation: Some("fixture".into()),
        }
    }

    #[test]
    fn malformed_configuration_and_cache_fail_closed_at_public_boundaries() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();

        assert!(add_root(&config, &root, 33).unwrap_err().contains("depth"));
        assert!(
            add_root(&config, &root.join("missing"), 1)
                .unwrap_err()
                .contains("unavailable")
        );
        assert!(
            remove_root(&config, &root)
                .unwrap_err()
                .contains("not configured")
        );
        for invalid in [
            DiscoveryConfig {
                schema_version: 2,
                ..DiscoveryConfig::default()
            },
            DiscoveryConfig {
                roots: vec![DiscoveryRoot {
                    path: PathBuf::from("relative"),
                    max_depth: 0,
                }],
                ..DiscoveryConfig::default()
            },
            DiscoveryConfig {
                roots: vec![
                    DiscoveryRoot {
                        path: root.clone(),
                        max_depth: 0,
                    },
                    DiscoveryRoot {
                        path: root.clone(),
                        max_depth: 0,
                    },
                ],
                ..DiscoveryConfig::default()
            },
        ] {
            assert!(validate_config(&invalid).is_err());
        }

        fs::create_dir_all(&config).unwrap();
        fs::write(config_path(&config), b"not json").unwrap();
        assert!(read_config(&config).is_err());
        fs::remove_file(config_path(&config)).unwrap();
        symlink(&root, config_path(&config)).unwrap();
        assert!(read_config(&config).is_err());

        assert!(read_cache(&data).unwrap().is_none());
        fs::create_dir_all(&data).unwrap();
        fs::write(cache_path(&data), b"not json").unwrap();
        assert!(read_cache(&data).is_err());
        let mut invalid = DiscoveryCache::empty(0);
        invalid.schema_version = 2;
        assert!(invalid.validate().is_err());
        invalid.schema_version = DISCOVERY_SCHEMA_VERSION;
        invalid.completed_roots = 1;
        assert!(invalid.validate().is_err());
        invalid.completed_roots = 0;
        invalid.scanned_directories = MAX_DIRECTORIES + 1;
        assert!(invalid.validate().is_err());
        invalid.scanned_directories = 0;
        invalid.candidates = vec![candidate(PathBuf::from("relative"))];
        assert!(invalid.validate().is_err());
        let absolute = root.canonicalize().unwrap();
        invalid.candidates = vec![candidate(absolute.clone()), candidate(absolute)];
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn scanning_progress_retains_prior_rows_and_bounds_error_history() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let prior_path = temp.path().join("prior");
        let fresh_path = temp.path().join("fresh");
        let replaced_path = temp.path().join("replaced");
        for path in [&prior_path, &fresh_path, &replaced_path] {
            fs::create_dir(path).unwrap();
        }
        let prior = vec![
            candidate(prior_path.clone()),
            candidate(replaced_path.clone()),
        ];
        let refreshed = vec![candidate(fresh_path), candidate(replaced_path)];
        let mut cache = DiscoveryCache::empty(1);
        write_progress(&data, &mut cache, &prior, &refreshed).unwrap();
        assert_eq!(cache.candidates.len(), 3);
        assert!(
            cache
                .candidates
                .iter()
                .any(|row| row.canonical_path == prior_path)
        );
        assert_eq!(read_cache(&data).unwrap(), Some(cache.clone()));

        for index in 0..600 {
            record_error(&mut cache, format!("error-{index}"));
        }
        assert_eq!(cache.errors.len(), 512);
        assert!(cache.errors.last().unwrap().contains("omitted"));
    }

    #[test]
    fn marker_only_git_root_is_registered_and_default_symlinks_are_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let config = home.join("config");
        let data = home.join("data");
        let repository = temp.path().join("repository");
        let component = repository.join("component");
        let linked = temp.path().join("linked-component");
        repo(&repository);
        fs::create_dir(&component).unwrap();
        fs::write(component.join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        symlink(&component, &linked).unwrap();
        let registered = project_registry::register_project(
            &home,
            &repository,
            None,
            project_registry::CheckoutOwnership::UserOwned,
        )
        .unwrap();
        add_root(&config, &component, 0).unwrap();
        add_root(&config, &linked, 0).unwrap();

        let cache = refresh(&home, &config, &data).unwrap();
        assert_eq!(cache.candidates.len(), 1);
        assert_eq!(
            cache.candidates[0].canonical_path,
            registered.canonical_path
        );
        assert_eq!(
            cache.candidates[0].checkout_id,
            Some(registered.checkout_id)
        );
        assert!(cache.candidates[0].registered);
        assert!(cache.candidates[0].task_ready);
        assert!(
            !cache
                .candidates
                .iter()
                .any(|row| row.canonical_path == linked)
        );
    }
}

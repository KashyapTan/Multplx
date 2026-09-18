//! Versioned filesystem project/checkout ownership and immutable task bindings.
//! The flat `projects.md` delivery reader remains a bounded legacy adapter.

use std::fs;
use std::path::Path;

/// Supported delivery posture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryMode {
    DeepReview,
    DirectPr,
    LocalOnly,
}

impl DeliveryMode {
    /// Registry spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeepReview => "deep-review",
            Self::DirectPr => "direct-PR",
            Self::LocalOnly => "local-only",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "deep-review" => Some(Self::DeepReview),
            "direct-PR" => Some(Self::DirectPr),
            "local-only" => Some(Self::LocalOnly),
            _ => None,
        }
    }
}

/// Resolved mode and optional warning, kept separate for exact stream routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolution {
    pub mode: DeliveryMode,
    pub yolo: bool,
    pub warning: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProjectRegistry {
    path: std::path::PathBuf,
}

impl ProjectRegistry {
    #[must_use]
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn resolve(&self, name: &str) -> Resolution {
        resolve(&self.path, name)
    }
}

impl Resolution {
    /// Exact two-word stdout contract.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{} {}\n",
            self.mode.as_str(),
            if self.yolo { "on" } else { "off" }
        )
    }
}

fn fallback(warning: String) -> Resolution {
    Resolution {
        mode: DeliveryMode::DirectPr,
        yolo: false,
        warning: Some(warning),
    }
}

/// Resolve one exact project name without mutating or repairing the registry.
pub fn resolve(path: &Path, name: &str) -> Resolution {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            return fallback(format!(
                "warn: no registry at {}; defaulting {name} to direct-PR off; review tools remain opt-in",
                path.display()
            ));
        }
    };
    let mut parsed = None;
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() != Some(&"-") || fields.get(1) != Some(&name) {
            continue;
        }
        let mut mode = "direct-PR".to_owned();
        let mut yolo = false;
        if fields.get(2).is_some_and(|field| field.starts_with('[')) {
            let mut bracket = Vec::new();
            for field in fields.iter().skip(2) {
                bracket.push(*field);
                if field.ends_with(']') {
                    break;
                }
            }
            let joined = bracket.join(" ");
            let inner = joined.trim_start_matches('[').trim_end_matches(']');
            let tokens: Vec<&str> = inner.split_whitespace().collect();
            if let Some(first) = tokens.first().copied()
                && first != "+yolo"
                && !first.is_empty()
            {
                mode = first.to_owned();
            }
            yolo = tokens.contains(&"+yolo");
        }
        parsed = Some((mode, yolo));
        break;
    }
    let Some((raw_mode, yolo)) = parsed else {
        return fallback(format!(
            "warn: project \"{name}\" not in registry; defaulting to direct-PR off; review tools remain opt-in"
        ));
    };
    let Some(mode) = DeliveryMode::parse(&raw_mode) else {
        return fallback(format!(
            "warn: unknown mode \"{raw_mode}\" for {name}; defaulting to direct-PR off; review tools remain opt-in"
        ));
    };
    Resolution {
        mode,
        yolo,
        warning: None,
    }
}

/// Resolve a launch path without confusing the broker checkout with a same-named clone.
pub fn resolve_path(registry: &Path, projects: &Path, root: &Path, project: &Path) -> Resolution {
    if fs::canonicalize(root).ok().as_ref() == Some(&project.to_path_buf()) {
        return Resolution {
            mode: DeliveryMode::DirectPr,
            yolo: false,
            warning: None,
        };
    }
    let name = project
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if fs::canonicalize(projects.join(name)).ok().as_ref() == Some(&project.to_path_buf()) {
        resolve(registry, name)
    } else {
        fallback(format!(
            "warn: unregistered project path {}; defaulting to direct-PR off; review tools remain opt-in",
            project.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_yolo_defaults_and_ambiguous_names_match_registry_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("projects.md");
        fs::write(
            &registry,
            "- app [local-only +yolo] - app\n- app-extra [direct-PR] - other\n- default - default\n- bad [unsafe +yolo] - bad\n",
        )
        .expect("registry");
        assert_eq!(resolve(&registry, "app").render(), "local-only on\n");
        assert_eq!(resolve(&registry, "app-extra").render(), "direct-PR off\n");
        assert_eq!(resolve(&registry, "default").render(), "direct-PR off\n");
        let bad = resolve(&registry, "bad");
        assert_eq!(bad.render(), "direct-PR off\n");
        assert!(bad.warning.expect("warning").contains("unknown mode"));
        assert!(resolve(&registry, "missing").warning.is_some());
        assert!(
            resolve(&temp.path().join("absent"), "app")
                .warning
                .is_some()
        );
    }

    #[test]
    fn typed_registry_and_delivery_mode_render_every_variant() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("projects.md");
        fs::write(
            &registry,
            "- review [deep-review] - review\n- direct [direct-PR +yolo] - direct\n- local [local-only] - local\n",
        )
        .expect("registry");
        let typed = ProjectRegistry::new(&registry);
        assert_eq!(typed.resolve("review").mode, DeliveryMode::DeepReview);
        assert_eq!(typed.resolve("direct").mode, DeliveryMode::DirectPr);
        assert!(typed.resolve("direct").yolo);
        assert_eq!(typed.resolve("local").mode, DeliveryMode::LocalOnly);
        assert_eq!(typed.resolve("local").render(), "local-only off\n");
        assert_eq!(DeliveryMode::DeepReview.as_str(), "deep-review");
        assert_eq!(DeliveryMode::DirectPr.as_str(), "direct-PR");
        assert_eq!(DeliveryMode::LocalOnly.as_str(), "local-only");
    }
}

/// Current filesystem project registry version. Discovery is a consumer, not an owner.
pub const PROJECT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckoutOwnership {
    Managed,
    UserOwned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationDestination {
    Local,
    PullRequest,
}

/// Immutable task/request/attempt routing, captured before dispatch.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBinding {
    pub project_id: String,
    pub checkout_id: String,
    pub canonical_path: std::path::PathBuf,
    pub checkout_identity: String,
    pub common_git_dir: std::path::PathBuf,
    pub common_git_identity: String,
    pub starting_revision: String,
    pub ownership: CheckoutOwnership,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutRecord {
    pub checkout_id: String,
    pub canonical_path: std::path::PathBuf,
    pub checkout_identity: String,
    pub common_git_dir: std::path::PathBuf,
    pub ownership: CheckoutOwnership,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRecord {
    pub project_id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub common_git_identity: String,
    pub remote: Option<String>,
    pub publication: PublicationDestination,
    /// Only an explicitly selected review is recorded here; old registry modes do not opt in.
    pub review: Option<String>,
    pub checkouts: Vec<CheckoutRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCatalog {
    pub schema_version: u32,
    pub projects: Vec<ProjectRecord>,
}

impl Default for ProjectCatalog {
    fn default() -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            projects: Vec::new(),
        }
    }
}

fn identity(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;
    let metadata =
        fs::metadata(path).map_err(|e| format!("cannot inspect {}: {e}", path.display()))?;
    let created = metadata
        .created()
        .ok()
        .and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|v| v.as_nanos());
    Ok(format!("{}:{}:{created:?}", metadata.dev(), metadata.ino()))
}

fn stable_id(prefix: &str, value: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{prefix}-{:x}", Sha256::digest(value.as_bytes()))
}

fn git_value(path: &Path, args: &[&str]) -> Result<String, String> {
    let output = crate::lifecycle::worktree::command_output(
        std::process::Command::new("git")
            // Registry inspection is read-only. Suppress optional Git locks and
            // index refreshes so observing a checkout cannot mutate its home.
            .env("GIT_OPTIONAL_LOCKS", "0")
            .arg("-C")
            .arg(path)
            .args(args),
    )
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Git {} at {}: {}",
            args.join(" "),
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_owned())
}

fn inspect_checkout(path: &Path) -> Result<ProjectBinding, String> {
    let selected = fs::canonicalize(path).map_err(|e| {
        format!(
            "checkout unavailable at {}: {e}; repair its recorded location",
            path.display()
        )
    })?;
    let canonical_path = fs::canonicalize(git_value(&selected, &["rev-parse", "--show-toplevel"])?)
        .map_err(|e| e.to_string())?;
    let common_git_dir = fs::canonicalize(git_value(
        &canonical_path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?)
    .map_err(|e| e.to_string())?;
    let checkout_identity = identity(&canonical_path)?;
    let common_git_identity = identity(&common_git_dir)?;
    let starting_revision = git_value(&canonical_path, &["rev-parse", "--verify", "HEAD^{commit}"]).map_err(|e| format!("checkout requires a named starting commit (unborn repositories cannot dispatch): {e}"))?;
    Ok(ProjectBinding {
        project_id: stable_id("project", &common_git_identity),
        checkout_id: stable_id(
            "checkout",
            &format!("{common_git_identity}:{checkout_identity}"),
        ),
        canonical_path,
        checkout_identity,
        common_git_dir,
        common_git_identity,
        starting_revision,
        ownership: CheckoutOwnership::UserOwned,
    })
}

/// Read and structurally validate canonical identities, without probing or mutating checkout files.
pub fn read_catalog(home: &Path) -> Result<ProjectCatalog, String> {
    let path = home.join("data/projects.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProjectCatalog::default());
        }
        Err(error) => return Err(error.to_string()),
        Ok(_) => {}
    }
    let bytes = multplx_core::filesystem::read_bounded_regular(&path, 4 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    let catalog: ProjectCatalog =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid project registry: {e}"))?;
    catalog.validate()?;
    Ok(catalog)
}

impl ProjectCatalog {
    pub fn validate(&self) -> Result<(), String> {
        use std::collections::HashSet;
        if self.schema_version != PROJECT_SCHEMA_VERSION {
            return Err("unsupported project registry version".to_owned());
        }
        let mut projects = HashSet::new();
        let mut repositories = HashSet::new();
        let mut checkouts = HashSet::new();
        let mut paths = HashSet::new();
        for project in &self.projects {
            if project.project_id.is_empty()
                || project.common_git_identity.is_empty()
                || project.display_name.is_empty()
                || project.checkouts.is_empty()
                || !projects.insert(&project.project_id)
                || !repositories.insert(&project.common_git_identity)
            {
                return Err("duplicate, empty or conflicting project identity".to_owned());
            }
            let mut aliases = HashSet::new();
            if project
                .aliases
                .iter()
                .any(|alias| alias.is_empty() || !aliases.insert(alias))
            {
                return Err("empty or duplicate project alias".to_owned());
            }
            for checkout in &project.checkouts {
                if checkout.checkout_id.is_empty()
                    || checkout.checkout_identity.is_empty()
                    || !checkout.canonical_path.is_absolute()
                    || !checkout.common_git_dir.is_absolute()
                    || !checkouts.insert(&checkout.checkout_id)
                    || !paths.insert(&checkout.canonical_path)
                {
                    return Err("duplicate, empty or conflicting checkout identity".to_owned());
                }
            }
        }
        Ok(())
    }
}

/// Remember a local source; registration never changes branches, files or Git configuration.
/// Ownership is explicit and cannot be upgraded by rediscovery of a borrowed checkout.
pub fn register_project(
    home: &Path,
    path: &Path,
    alias: Option<&str>,
    ownership: CheckoutOwnership,
) -> Result<ProjectBinding, String> {
    register_project_at(
        home,
        &home.join("data"),
        &home.join("projects"),
        path,
        alias,
        ownership,
    )
}

/// Preserve explicitly configured legacy source roots while keeping one canonical catalog owner.
pub fn register_project_at(
    home: &Path,
    legacy_data: &Path,
    legacy_projects: &Path,
    path: &Path,
    alias: Option<&str>,
    ownership: CheckoutOwnership,
) -> Result<ProjectBinding, String> {
    let observed = inspect_checkout(path)?;
    let remote = git_value(&observed.canonical_path, &["remote", "get-url", "origin"]).ok();
    let has_remote = remote.is_some();
    let remote = remote.and_then(|value| public_remote(&value));
    let data = home.join("data");
    fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        data.join(".projects.lock"),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())?;
    fs::create_dir_all(home.join("state")).map_err(|e| e.to_string())?;
    crate::lifecycle::subagent_model::require_writer_version(&home.join("state"))?;
    let mut catalog = read_catalog(home)?;
    // Revalidate filesystem identity under the publication lock without holding it across Git commands.
    if identity(&observed.canonical_path)? != observed.checkout_identity
        || identity(&observed.common_git_dir)? != observed.common_git_identity
    {
        return Err("checkout changed during registration; retry after location repair".to_owned());
    }
    for project in &catalog.projects {
        for checkout in &project.checkouts {
            if checkout.canonical_path == observed.canonical_path
                && (checkout.checkout_identity != observed.checkout_identity
                    || project.common_git_identity != observed.common_git_identity)
            {
                return Err(
                    "recorded checkout was replaced; explicit location repair required".to_owned(),
                );
            }
        }
    }
    let index = catalog
        .projects
        .iter()
        .position(|p| p.common_git_identity == observed.common_git_identity)
        .unwrap_or_else(|| {
            let display_name = observed
                .canonical_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            catalog.projects.push(ProjectRecord {
                project_id: observed.project_id.clone(),
                display_name,
                aliases: Vec::new(),
                common_git_identity: observed.common_git_identity.clone(),
                publication: if has_remote {
                    PublicationDestination::PullRequest
                } else {
                    PublicationDestination::Local
                },
                remote,
                review: None,
                checkouts: Vec::new(),
            });
            catalog.projects.len() - 1
        });
    let project = &mut catalog.projects[index];
    if ownership == CheckoutOwnership::Managed
        && legacy_checkout_ownership_at(legacy_data, legacy_projects, path)?
            == CheckoutOwnership::Managed
    {
        let name = binding_name(&observed.canonical_path);
        let resolution = resolve(&legacy_data.join("projects.md"), &name);
        project.publication = if resolution.mode == DeliveryMode::LocalOnly {
            PublicationDestination::Local
        } else {
            PublicationDestination::PullRequest
        };
    }

    if let Some(alias) = alias {
        if alias.trim().is_empty() {
            return Err("project alias must not be empty".to_owned());
        }
        if !project.aliases.iter().any(|v| v == alias) {
            project.aliases.push(alias.to_owned());
        }
    }
    let existing = project
        .checkouts
        .iter()
        .find(|c| c.canonical_path == observed.canonical_path);
    let mut binding = observed;
    binding.project_id.clone_from(&project.project_id);
    if let Some(checkout) = existing {
        binding.checkout_id.clone_from(&checkout.checkout_id);
        binding.ownership = checkout.ownership;
    } else {
        binding.ownership = ownership;
        project.checkouts.push(CheckoutRecord {
            checkout_id: binding.checkout_id.clone(),
            canonical_path: binding.canonical_path.clone(),
            checkout_identity: binding.checkout_identity.clone(),
            common_git_dir: binding.common_git_dir.clone(),
            ownership,
        });
    }
    catalog.validate()?;
    let bytes = serde_json::to_vec_pretty(&catalog).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("project registry exceeds supported size".to_owned());
    }
    multplx_core::filesystem::atomic_replace(data.join("projects.json"), &bytes, 0o600)
        .map_err(|e| e.to_string())?;
    Ok(binding)
}

pub fn bind_project(home: &Path, path: &Path) -> Result<ProjectBinding, String> {
    bind_project_at(home, &home.join("data"), &home.join("projects"), path)
}

/// Remember a previously inspected source in a new home without copying it.
/// Home seeding observes Git before taking its publication lock, then this
/// owner rechecks filesystem identity and publishes only catalog metadata.
pub fn remember_reference(
    home: &Path,
    source: &ProjectRecord,
    binding: &ProjectBinding,
    alias: &str,
) -> Result<(), String> {
    if source.project_id != binding.project_id
        || source.common_git_identity != binding.common_git_identity
        || identity(&binding.canonical_path)? != binding.checkout_identity
        || identity(&binding.common_git_dir)? != binding.common_git_identity
    {
        return Err("project reference changed before home publication".into());
    }
    let data = home.join("data");
    fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        data.join(".projects.lock"),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())?;
    let mut catalog = read_catalog(home)?;
    let mut reference = source.clone();
    reference
        .checkouts
        .retain(|c| c.checkout_id == binding.checkout_id);
    if reference.checkouts.len() != 1 {
        return Err("source checkout is not in its project record".into());
    }
    reference.checkouts[0].ownership = CheckoutOwnership::UserOwned;
    reference.aliases = vec![alias.into()];
    if let Some(existing) = catalog
        .projects
        .iter_mut()
        .find(|p| p.project_id == reference.project_id)
    {
        for checkout in &reference.checkouts {
            if !existing
                .checkouts
                .iter()
                .any(|c| c.checkout_id == checkout.checkout_id)
            {
                existing.checkouts.push(checkout.clone());
            }
        }
        if !existing.aliases.contains(&alias.to_owned()) {
            existing.aliases.push(alias.into());
        }
    } else {
        catalog.projects.push(reference);
    }
    catalog.validate()?;
    multplx_core::filesystem::atomic_replace(
        data.join("projects.json"),
        &serde_json::to_vec(&catalog).map_err(|e| e.to_string())?,
        0o600,
    )
    .map_err(|e| e.to_string())
}

pub fn bind_project_at(
    home: &Path,
    legacy_data: &Path,
    legacy_projects: &Path,
    path: &Path,
) -> Result<ProjectBinding, String> {
    let ownership = legacy_checkout_ownership_at(legacy_data, legacy_projects, path)?;
    register_project_at(home, legacy_data, legacy_projects, path, None, ownership)
}

/// Check a previously accepted immutable binding. HEAD may advance; its recorded base must still exist.
pub fn validate_binding(home: &Path, binding: &ProjectBinding) -> Result<(), String> {
    let catalog = read_catalog(home)?;
    let project = catalog
        .projects
        .iter()
        .find(|p| p.project_id == binding.project_id)
        .ok_or("unknown project identity")?;
    let checkout = project
        .checkouts
        .iter()
        .find(|c| c.checkout_id == binding.checkout_id)
        .ok_or("unknown checkout identity")?;
    if checkout.canonical_path != binding.canonical_path
        || checkout.checkout_identity != binding.checkout_identity
        || checkout.common_git_dir != binding.common_git_dir
        || project.common_git_identity != binding.common_git_identity
        || checkout.ownership != binding.ownership
    {
        return Err("binding conflicts with recorded project/checkout identity".to_owned());
    }
    verify_location(binding)
}

fn github_project(remote: &str) -> Option<String> {
    let public = public_remote(remote)?;
    let path = if let Some(rest) = public.strip_prefix("git@github.com:") {
        rest
    } else {
        let (_, rest) = public.split_once("://")?;
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?;
        if host != "github.com" {
            return None;
        }
        path
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let repository = parts.next()?;
    if parts.next().is_some()
        || owner.is_empty()
        || repository.is_empty()
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || !repository
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return None;
    }
    Some(format!("{owner}/{repository}"))
}

/// Validate the exact task-bound repository and forge before publication.
/// Worktrees may have a different checkout identity, but they must share the
/// immutable common Git identity and registered canonical GitHub remote.
pub fn validate_publication_location(
    home: &Path,
    project: &ProjectBinding,
    worktree: &Path,
) -> Result<String, String> {
    validate_binding(home, project)?;
    let observed = inspect_checkout(worktree)
        .map_err(|error| format!("publication worktree is unavailable: {error}"))?;
    if observed.common_git_identity != project.common_git_identity
        || observed.common_git_dir != project.common_git_dir
        || observed.project_id != project.project_id
    {
        return Err("publication worktree belongs to another task project".into());
    }
    let catalog = read_catalog(home)?;
    let registered = catalog
        .projects
        .iter()
        .find(|candidate| candidate.project_id == project.project_id)
        .ok_or("unknown task project identity")?;
    if registered.publication != PublicationDestination::PullRequest {
        return Err("task project is remote-free; pull-request publication unavailable".into());
    }
    let registered_remote = registered
        .remote
        .as_deref()
        .ok_or("task project has no safe registered publication remote")?;
    let registered_project = github_project(registered_remote)
        .ok_or("task-bound remote is not a supported GitHub repository")?;
    let actual_remote = git_value(worktree, &["remote", "get-url", "origin"])
        .map_err(|_| "publication worktree has no origin remote")?;
    let actual_project = github_project(&actual_remote)
        .ok_or("publication origin is not a supported GitHub repository")?;
    if actual_project != registered_project {
        return Err("publication origin differs from task-bound registered forge".into());
    }
    Ok(registered_project)
}

/// Validate a durable resource's source identity even when its home registry is
/// unavailable. This does not register, adopt or change checkout ownership.
pub fn verify_location(binding: &ProjectBinding) -> Result<(), String> {
    let actual = inspect_checkout(&binding.canonical_path)?;
    if actual.canonical_path != binding.canonical_path
        || actual.project_id != binding.project_id
        || actual.checkout_id != binding.checkout_id
        || actual.checkout_identity != binding.checkout_identity
        || actual.common_git_dir != binding.common_git_dir
        || actual.common_git_identity != binding.common_git_identity
    {
        return Err("checkout moved or replaced; explicit location repair required".to_owned());
    }
    if binding.starting_revision.len() != 40 && binding.starting_revision.len() != 64
        || !binding
            .starting_revision
            .bytes()
            .all(|v| v.is_ascii_hexdigit())
    {
        return Err("starting revision must be a full commit identity".to_owned());
    }
    git_value(
        &binding.canonical_path,
        &[
            "cat-file",
            "-e",
            &format!("{}^{{commit}}", binding.starting_revision),
        ],
    )?;
    Ok(())
}

/// Exact IDs and aliases are accepted; ambiguous names/checkouts return candidate paths.
pub fn resolve_checkout(home: &Path, selector: &str) -> Result<ProjectBinding, String> {
    let catalog = read_catalog(home)?;
    let canonical = fs::canonicalize(selector).ok();
    let exact_checkout = catalog
        .projects
        .iter()
        .flat_map(|p| p.checkouts.iter())
        .any(|c| c.checkout_id == selector);
    let exact_path = canonical.as_ref().is_some_and(|path| {
        catalog
            .projects
            .iter()
            .flat_map(|p| p.checkouts.iter())
            .any(|c| &c.canonical_path == path)
    });
    let exact_project = catalog.projects.iter().any(|p| p.project_id == selector);
    let mut matches = Vec::new();
    for project in &catalog.projects {
        for checkout in &project.checkouts {
            let selected = if exact_checkout {
                selector == checkout.checkout_id
            } else if exact_path {
                canonical.as_ref() == Some(&checkout.canonical_path)
            } else if exact_project {
                selector == project.project_id
            } else {
                selector == project.display_name || project.aliases.iter().any(|a| a == selector)
            };
            if selected {
                matches.push((project, checkout));
            }
        }
    }
    if matches.len() != 1 {
        return Err(format!(
            "project selector {selector:?} has {} candidates: {}",
            matches.len(),
            matches
                .iter()
                .map(|(_, c)| c.canonical_path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let (project, checkout) = matches[0];
    let observed = inspect_checkout(&checkout.canonical_path)?;
    let binding = ProjectBinding {
        project_id: project.project_id.clone(),
        checkout_id: checkout.checkout_id.clone(),
        canonical_path: checkout.canonical_path.clone(),
        checkout_identity: checkout.checkout_identity.clone(),
        common_git_dir: checkout.common_git_dir.clone(),
        common_git_identity: project.common_git_identity.clone(),
        starting_revision: observed.starting_revision,
        ownership: checkout.ownership,
    };
    validate_binding(home, &binding)?;
    Ok(binding)
}

/// Unregister metadata only. File deletion is never a consequence of forgetting a checkout.
pub fn unregister_checkout(home: &Path, checkout_id: &str) -> Result<(), String> {
    let data = home.join("data");
    let _lock = multplx_core::locks::DirectoryLock::acquire_wait(
        data.join(".projects.lock"),
        &multplx_core::process::SystemProcessProbe::default(),
        std::time::Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())?;
    fs::create_dir_all(home.join("state")).map_err(|e| e.to_string())?;
    crate::lifecycle::subagent_model::require_writer_version(&home.join("state"))?;
    let mut catalog = read_catalog(home)?;
    if !catalog
        .projects
        .iter()
        .any(|p| p.checkouts.iter().any(|c| c.checkout_id == checkout_id))
    {
        return Err("unknown checkout identity".to_owned());
    }
    for project in &mut catalog.projects {
        project.checkouts.retain(|c| c.checkout_id != checkout_id);
    }
    catalog.projects.retain(|p| !p.checkouts.is_empty());
    multplx_core::filesystem::atomic_replace(
        data.join("projects.json"),
        &serde_json::to_vec_pretty(&catalog).map_err(|e| e.to_string())?,
        0o600,
    )
    .map_err(|e| e.to_string())
}

fn binding_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Legacy managed ownership is known only for an exact registered flat checkout.
/// This is a reader conversion, not a migration of the home or a claim on arbitrary paths.
pub fn legacy_checkout_ownership(home: &Path, path: &Path) -> Result<CheckoutOwnership, String> {
    legacy_checkout_ownership_at(&home.join("data"), &home.join("projects"), path)
}

pub fn legacy_checkout_ownership_at(
    data: &Path,
    projects: &Path,
    path: &Path,
) -> Result<CheckoutOwnership, String> {
    let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let name = binding_name(&canonical);
    let slot = projects.join(&name);
    if fs::symlink_metadata(projects).is_ok_and(|m| m.file_type().is_symlink())
        || fs::symlink_metadata(&slot).is_ok_and(|m| m.file_type().is_symlink())
        || fs::canonicalize(&slot).ok().as_ref() != Some(&canonical)
    {
        return Ok(CheckoutOwnership::UserOwned);
    }
    let text = fs::read_to_string(data.join("projects.md")).unwrap_or_default();
    let matching: Vec<_> = text
        .lines()
        .filter(|line| {
            let mut words = line.split_whitespace();
            words.next() == Some("-") && words.next() == Some(name.as_str())
        })
        .collect();
    if matching.len() > 1 {
        return Err(format!("ambiguous legacy project identity {name:?}"));
    }
    if matching.len() == 1 && resolve(&data.join("projects.md"), &name).warning.is_none() {
        Ok(CheckoutOwnership::Managed)
    } else {
        Ok(CheckoutOwnership::UserOwned)
    }
}

// A remote is a display identity, never a credential container. Reject embedded
// URL credentials/query/fragment; SSH's conventional git@host is not a secret.
fn public_remote(remote: &str) -> Option<String> {
    if remote.contains(['?', '#']) {
        return None;
    }
    if let Some((scheme, rest)) = remote.split_once("://") {
        let authority = rest.split('/').next().unwrap_or_default();
        if authority.contains('@')
            && !(scheme == "ssh" && authority.starts_with("git@") && !authority.contains("git:"))
        {
            return None;
        }
    } else if let Some((user, _)) = remote.split_once('@')
        && user != "git"
    {
        return None;
    }
    Some(remote.to_owned())
}

/// Check maintenance authority before any fetch, branch update or cleanup.
pub fn checkout_ownership(home: &Path, path: &Path) -> Result<CheckoutOwnership, String> {
    checkout_ownership_at(home, &home.join("projects"), path)
}

pub fn checkout_ownership_at(
    home: &Path,
    projects: &Path,
    path: &Path,
) -> Result<CheckoutOwnership, String> {
    let catalog = read_catalog(home)?;
    let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
    for project in &catalog.projects {
        if let Some(checkout) = project
            .checkouts
            .iter()
            .find(|c| c.canonical_path == canonical)
        {
            let binding = resolve_checkout(home, &checkout.checkout_id)?;
            validate_binding(home, &binding)?;
            return Ok(checkout.ownership);
        }
    }
    legacy_checkout_ownership_at(&home.join("data"), projects, path)
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn git(path: &Path, args: &[&str]) -> String {
        git_value(path, args).expect("test git")
    }
    fn repo(path: &Path) {
        fs::create_dir_all(path).expect("repo");
        git(path, &["init", "--quiet", "-b", "main"]);
        fs::write(path.join("file"), "base\n").expect("file");
        git(path, &["add", "file"]);
        git(
            path,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "--quiet",
                "-m",
                "base",
            ],
        );
    }

    #[test]
    fn publication_location_is_task_project_and_registered_forge_bound() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let source = temp.path().join("source");
        repo(&source);
        git(
            &source,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/example/project.git",
            ],
        );
        let binding = register_project(&home, &source, None, CheckoutOwnership::UserOwned).unwrap();
        assert_eq!(
            validate_publication_location(&home, &binding, &source).unwrap(),
            "example/project"
        );

        git(
            &source,
            &[
                "remote",
                "set-url",
                "origin",
                "git@github.com:other/project.git",
            ],
        );
        assert!(
            validate_publication_location(&home, &binding, &source)
                .unwrap_err()
                .contains("differs from task-bound")
        );
        git(
            &source,
            &[
                "remote",
                "set-url",
                "origin",
                "ssh://git@github.com/example/project.git",
            ],
        );
        assert_eq!(
            validate_publication_location(&home, &binding, &source).unwrap(),
            "example/project"
        );

        let foreign = temp.path().join("foreign");
        repo(&foreign);
        git(
            &foreign,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/example/project.git",
            ],
        );
        assert!(
            validate_publication_location(&home, &binding, &foreign)
                .unwrap_err()
                .contains("another task project")
        );

        let local = temp.path().join("local");
        repo(&local);
        let local_binding =
            register_project(&home, &local, None, CheckoutOwnership::UserOwned).unwrap();
        assert!(
            validate_publication_location(&home, &local_binding, &local)
                .unwrap_err()
                .contains("remote-free")
        );

        let unsupported = temp.path().join("unsupported");
        repo(&unsupported);
        git(
            &unsupported,
            &[
                "remote",
                "add",
                "origin",
                "https://gitlab.example/example/project.git",
            ],
        );
        let unsupported_binding =
            register_project(&home, &unsupported, None, CheckoutOwnership::UserOwned).unwrap();
        assert!(
            validate_publication_location(&home, &unsupported_binding, &unsupported)
                .unwrap_err()
                .contains("not a supported GitHub")
        );
    }

    #[test]
    fn clones_names_remotes_symlinks_overlapping_paths_and_worktrees_keep_identity() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let a = temp.path().join("a/app");
        let b = temp.path().join("b/app");
        repo(&a);
        fs::create_dir_all(b.parent().unwrap()).unwrap();
        git(
            temp.path(),
            &["clone", "--quiet", a.to_str().unwrap(), b.to_str().unwrap()],
        );
        git(&b, &["remote", "remove", "origin"]);
        for path in [&a, &b] {
            git(
                path,
                &["remote", "add", "origin", "https://example.test/shared.git"],
            );
        }
        let aa = register_project(&home, &a, Some("ambiguous"), CheckoutOwnership::UserOwned)
            .expect("a");
        let bb = register_project(&home, &b, Some("ambiguous"), CheckoutOwnership::UserOwned)
            .expect("b");
        assert_ne!(aa.project_id, bb.project_id);
        assert!(
            resolve_checkout(&home, "app")
                .unwrap_err()
                .contains("2 candidates")
        );
        assert!(resolve_checkout(&home, "ambiguous").is_err());
        let link = temp.path().join("alias");
        symlink(&a, &link).expect("link");
        assert_eq!(bind_project(&home, &link).expect("symlink"), aa);
        fs::create_dir(a.join("nested")).expect("nested");
        assert_eq!(bind_project(&home, &a.join("nested")).expect("overlap"), aa);
        let worktree = temp.path().join("worktree");
        git(
            &a,
            &[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                worktree.to_str().unwrap(),
                "HEAD",
            ],
        );
        let ww = bind_project(&home, &worktree).expect("worktree");
        assert_eq!(ww.project_id, aa.project_id);
        assert_ne!(ww.checkout_id, aa.checkout_id);
        assert_eq!(ww.common_git_identity, aa.common_git_identity);
        assert!(resolve_checkout(&home, &aa.project_id).is_err());
        assert_eq!(
            resolve_checkout(&home, a.to_str().unwrap()).expect("exact path"),
            aa
        );
        // An alias matching another checkout ID cannot shadow the exact ID.
        register_project(
            &home,
            &b,
            Some(&aa.checkout_id),
            CheckoutOwnership::UserOwned,
        )
        .expect("shadow alias");
        assert_eq!(
            resolve_checkout(&home, &aa.checkout_id).expect("exact ID"),
            aa
        );
        assert_eq!(read_catalog(&home).expect("catalog").projects.len(), 2);
    }

    #[test]
    fn three_queued_task_bindings_remain_fixed_when_display_context_and_head_change() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let mut requests = Vec::new();
        for i in 0..3 {
            let path = temp.path().join(format!("repo-{i}"));
            repo(&path);
            requests.push((
                format!("request-{i}"),
                bind_project(&home, &path).expect("bind"),
            ));
        }
        let encoded = serde_json::to_string(&requests).expect("requests");
        let display_context = resolve_checkout(&home, "repo-2").expect("selected");
        assert_eq!(display_context, requests[2].1);
        for (_, binding) in &requests {
            git(
                &binding.canonical_path,
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.test",
                    "commit",
                    "--quiet",
                    "--allow-empty",
                    "-m",
                    "new HEAD",
                ],
            );
            validate_binding(&home, binding).expect("old base is still valid");
        }
        assert_eq!(serde_json::to_string(&requests).unwrap(), encoded);
        assert_eq!(
            read_catalog(&home).unwrap().projects[0].publication,
            PublicationDestination::Local
        );
    }

    #[test]
    fn moved_replaced_and_substituted_repositories_fail_without_mutation() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let path = temp.path().join("app");
        repo(&path);
        let binding = bind_project(&home, &path).expect("bind");
        let mut forged = binding.clone();
        forged.project_id = "project-forged".into();
        assert!(verify_location(&forged).is_err());
        forged = binding.clone();
        forged.checkout_id = "checkout-forged".into();
        assert!(verify_location(&forged).is_err());
        fs::rename(&path, temp.path().join("old-app")).expect("move");
        assert!(validate_binding(&home, &binding).is_err());
        repo(&path);
        assert!(validate_binding(&home, &binding).is_err());
        assert!(bind_project(&home, &path).is_err());
        let catalog = read_catalog(&home).unwrap();
        assert_eq!(
            catalog.projects[0].checkouts[0].checkout_identity,
            binding.checkout_identity
        );
        assert!(temp.path().join("old-app/file").is_file());
    }

    #[test]
    fn borrowed_ownership_cannot_upgrade_and_forget_never_removes_files() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let path = temp.path().join("app");
        repo(&path);
        let binding = bind_project(&home, &path).unwrap();
        assert_eq!(
            register_project(&home, &path, None, CheckoutOwnership::Managed)
                .unwrap()
                .ownership,
            CheckoutOwnership::UserOwned
        );
        fs::write(path.join("file"), "dirty\n").unwrap();
        unregister_checkout(&home, &binding.checkout_id).unwrap();
        assert_eq!(fs::read_to_string(path.join("file")).unwrap(), "dirty\n");
        assert!(read_catalog(&home).unwrap().projects.is_empty());
    }

    #[test]
    fn duplicate_identity_corrupt_registry_and_legacy_ambiguity_refuse() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let path = home.join("projects/app");
        repo(&path);
        bind_project(&home, &path).unwrap();
        let mut catalog = read_catalog(&home).unwrap();
        catalog.projects.push(catalog.projects[0].clone());
        fs::write(
            home.join("data/projects.json"),
            serde_json::to_vec(&catalog).unwrap(),
        )
        .unwrap();
        assert!(read_catalog(&home).is_err());
        assert!(bind_project(&home, &path).is_err());
        fs::remove_file(home.join("data/projects.json")).unwrap();
        fs::write(
            home.join("data/projects.md"),
            "- app [direct-PR]\n- app [local-only]\n",
        )
        .unwrap();
        assert!(legacy_checkout_ownership(&home, &path).is_err());
    }

    #[test]
    fn legacy_mapping_preserves_destination_ownership_without_implicit_review() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let path = home.join("projects/app");
        repo(&path);
        fs::create_dir_all(home.join("data")).unwrap();
        fs::write(
            home.join("data/projects.md"),
            "- app [local-only +yolo] - old\n",
        )
        .unwrap();
        let binding = bind_project(&home, &path).unwrap();
        assert_eq!(binding.ownership, CheckoutOwnership::Managed);
        let catalog = read_catalog(&home).unwrap();
        assert_eq!(
            catalog.projects[0].publication,
            PublicationDestination::Local
        );
        assert_eq!(catalog.projects[0].review, None);
        assert_eq!(catalog.projects[0].remote, None);
        let borrowed = temp.path().join("borrowed");
        repo(&borrowed);
        symlink(&borrowed, home.join("projects/borrowed")).unwrap();
        fs::write(home.join("data/projects.md"), "- borrowed [direct-PR]\n").unwrap();
        assert_eq!(
            bind_project(&home, &borrowed).unwrap().ownership,
            CheckoutOwnership::UserOwned
        );
    }

    #[test]
    fn explicit_legacy_roots_preserve_managed_local_destination_without_claiming_unregistered_paths()
     {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let data = temp.path().join("configured-data");
        let projects = temp.path().join("configured-projects");
        let path = projects.join("app");
        repo(&path);
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("projects.md"), "- app [local-only +yolo]\n").unwrap();
        assert_eq!(
            publication_for_path_at(&home, &data, &projects, &path).unwrap(),
            PublicationDestination::Local
        );
        let binding = bind_project_at(&home, &data, &projects, &path).unwrap();
        assert_eq!(binding.ownership, CheckoutOwnership::Managed);
        assert_eq!(
            read_catalog(&home).unwrap().projects[0].publication,
            PublicationDestination::Local
        );
        let other = projects.join("unregistered");
        repo(&other);
        assert_eq!(
            bind_project_at(&home, &data, &projects, &other)
                .unwrap()
                .ownership,
            CheckoutOwnership::UserOwned
        );
    }

    #[test]
    fn credential_remotes_and_unborn_sources_never_create_unsafe_records() {
        assert_eq!(public_remote("https://secret@example.test/app"), None);
        assert_eq!(
            public_remote("https://example.test/app?access_token=secret"),
            None
        );
        assert_eq!(public_remote("ssh://git:secret@example.test/app"), None);
        assert!(public_remote("git@example.test:app").is_some());
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("app");
        repo(&path);
        git(
            &path,
            &[
                "remote",
                "add",
                "origin",
                "https://user:secret@example.test/app",
            ],
        );
        let home = temp.path().join("home");
        bind_project(&home, &path).unwrap();
        assert!(
            !fs::read_to_string(home.join("data/projects.json"))
                .unwrap()
                .contains("secret")
        );
        let empty = temp.path().join("empty");
        fs::create_dir(&empty).unwrap();
        git(&empty, &["init", "--quiet"]);
        assert!(bind_project(&home, &empty).unwrap_err().contains("unborn"));
    }
}

/// Resolve the canonical publication destination without registering or mutating a checkout.
/// Legacy deep-review maps to PR publication; it never selects a review workflow.
pub fn publication_for_path(home: &Path, path: &Path) -> Result<PublicationDestination, String> {
    publication_for_path_at(home, &home.join("data"), &home.join("projects"), path)
}

pub fn publication_for_path_at(
    home: &Path,
    legacy_data: &Path,
    legacy_projects: &Path,
    path: &Path,
) -> Result<PublicationDestination, String> {
    let observed = inspect_checkout(path)?;
    let catalog = read_catalog(home)?;
    for project in &catalog.projects {
        if project
            .checkouts
            .iter()
            .any(|c| c.canonical_path == observed.canonical_path)
        {
            resolve_checkout(
                home,
                observed
                    .canonical_path
                    .to_str()
                    .ok_or("checkout path must be UTF-8")?,
            )?;
            return Ok(project.publication);
        }
    }
    if legacy_checkout_ownership_at(legacy_data, legacy_projects, &observed.canonical_path)?
        == CheckoutOwnership::Managed
    {
        let mode = resolve(
            &legacy_data.join("projects.md"),
            &binding_name(&observed.canonical_path),
        )
        .mode;
        return Ok(if mode == DeliveryMode::LocalOnly {
            PublicationDestination::Local
        } else {
            PublicationDestination::PullRequest
        });
    }
    Ok(
        if git_value(&observed.canonical_path, &["remote", "get-url", "origin"]).is_ok() {
            PublicationDestination::PullRequest
        } else {
            PublicationDestination::Local
        },
    )
}

/// Retirement cannot delete a borrowed source, its contents, or a containing directory.
/// Phase 03's allocation owner may release its own isolated allocations; remembered
/// user checkout locations are never disposal authority.
pub fn protect_borrowed_checkouts(home: &Path, target: &Path) -> Result<(), String> {
    let target = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    for project in read_catalog(home)?.projects {
        for checkout in project.checkouts {
            if checkout.ownership == CheckoutOwnership::UserOwned
                && (target.starts_with(&checkout.canonical_path)
                    || checkout.canonical_path.starts_with(&target))
            {
                return Err(format!(
                    "REFUSED: cleanup target {} overlaps user-owned checkout {}; unregistering a project never authorizes deleting its files",
                    target.display(),
                    checkout.canonical_path.display()
                ));
            }
        }
    }
    Ok(())
}

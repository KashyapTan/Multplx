use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use regex::Regex;
use serde::Deserialize;

const HELP: &str = "Validate the tracked documentation audience inventory.\n\nUsage:\n  mx doc-audience-check\n  mx doc-audience-check --root <repo> [--inventory <path>]\n\nThe inventory owns classification and setup routing.\nThis check validates structure only and does not keyword-lint prose.\n";
const REQUIRED_PATTERNS: &[&str] = &["*.md", "*.mdx", "*.rst", "*.txt", "docs/examples/*"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scope {
    tracked_patterns: Vec<String>,
    #[serde(default)]
    excluded_prefixes: Vec<String>,
}

#[derive(Deserialize)]
struct Surface {
    path: String,
    audience: String,
}

#[derive(Deserialize)]
struct OwnerPointer {
    source: String,
    target: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inventory {
    version: u64,
    scope: Scope,
    allowed_audiences: Vec<String>,
    setup_audiences: Vec<String>,
    readme_setup_targets: Vec<String>,
    required_owner_pointers: Vec<OwnerPointer>,
    surfaces: Vec<Surface>,
}

fn parse_args(args: &[OsString]) -> Result<Option<(PathBuf, Option<PathBuf>)>, String> {
    let mut root = std::env::current_dir().map_err(|error| error.to_string())?;
    let mut inventory = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].to_string_lossy().as_ref() {
            "-h" | "--help" => return Ok(None),
            "--root" => {
                index += 1;
                root = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or("--root requires a path")?;
            }
            "--inventory" => {
                index += 1;
                inventory = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or("--inventory requires a path")?,
                );
            }
            value => return Err(format!("unknown argument: {value}")),
        }
        index += 1;
    }
    Ok(Some((root, inventory)))
}

fn git_tracked(root: &Path, patterns: &[String]) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
        ])
        .args(patterns)
        .output()
        .map_err(|error| format!("git ls-files failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|row| !row.is_empty())
        .map(|row| String::from_utf8_lossy(row).into_owned())
        .filter(|path| root.join(path).exists())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (
                (bytes[index + 1] as char).to_digit(16),
                (bytes[index + 2] as char).to_digit(16),
            )
        {
            output.push((high * 16 + low) as u8);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn normalize_link(raw: &str) -> &str {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(trimmed);
    trimmed.split_whitespace().next().unwrap_or("")
}

fn lexical(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                output.pop();
            }
            Component::CurDir => {}
            other => output.push(other.as_os_str()),
        }
    }
    output
}

fn local_links(root: &Path, source: &Path) -> Result<Vec<(String, PathBuf, String)>, String> {
    let text = fs::read_to_string(source).map_err(|error| {
        format!(
            "cannot read prose surface {}: {error}",
            source.strip_prefix(root).unwrap_or(source).display()
        )
    })?;
    let markdown = Regex::new(r#"!?\[[^\]]*\]\(([^)]+)\)"#).expect("markdown links");
    let html = Regex::new(r#"(?i)\b(?:href|src)=[\"']([^\"']+)[\"']"#).expect("html links");
    let mut output = Vec::new();
    for captures in markdown
        .captures_iter(&text)
        .chain(html.captures_iter(&text))
    {
        let raw = captures[1].to_owned();
        let value = normalize_link(&raw);
        let (path_part, fragment) = value.split_once('#').unwrap_or((value, ""));
        if path_part.contains("://")
            || path_part.starts_with("mailto:")
            || path_part.starts_with("data:")
        {
            continue;
        }
        if path_part.is_empty() && fragment.is_empty() {
            continue;
        }
        if Path::new(path_part).is_absolute() {
            return Err(format!(
                "absolute local link in {}: {raw}",
                source.strip_prefix(root).unwrap_or(source).display()
            ));
        }
        let target = if path_part.is_empty() {
            source.to_path_buf()
        } else {
            lexical(
                &source
                    .parent()
                    .unwrap_or(root)
                    .join(percent_decode(path_part)),
            )
        };
        if !target.starts_with(root) {
            return Err(format!(
                "local link escapes repository in {}: {raw}",
                source.strip_prefix(root).unwrap_or(source).display()
            ));
        }
        let fragment = percent_decode(fragment);
        output.push((raw, target, fragment));
    }
    Ok(output)
}

fn heading_slug(value: &str) -> String {
    let tag = Regex::new(r"<[^>]+>").unwrap();
    let invalid = Regex::new(r"[^\pL\pN_\- ]").unwrap();
    invalid
        .replace_all(
            &tag.replace_all(value, "")
                .replace('`', "")
                .trim()
                .to_lowercase(),
            "",
        )
        .replace(' ', "-")
}

fn anchors(path: &Path) -> Result<BTreeSet<String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read link target {}: {error}", path.display()))?;
    let heading = Regex::new(r"^#{1,6}\s+(.+?)\s*#*\s*$").unwrap();
    let explicit = Regex::new(r#"(?i)<(?:a|span)\s+(?:name|id)=[\"']([^\"']+)[\"']"#).unwrap();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut result = BTreeSet::new();
    for line in text.lines() {
        if let Some(captures) = heading.captures(line) {
            let base = heading_slug(&captures[1]);
            if !base.is_empty() {
                let count = counts.entry(base.clone()).or_default();
                result.insert(if *count == 0 {
                    base
                } else {
                    format!("{base}-{count}")
                });
                *count += 1;
            }
        }
        for captures in explicit.captures_iter(line) {
            result.insert(captures[1].to_owned());
        }
    }
    Ok(result)
}

fn validate(root: &Path, inventory_path: &Path) -> Result<(usize, usize), String> {
    let bytes = fs::read(inventory_path)
        .map_err(|_| format!("inventory is missing: {}", inventory_path.display()))?;
    let inventory: Inventory = serde_json::from_slice(&bytes)
        .map_err(|error| format!("inventory is unreadable: {error}"))?;
    if inventory.version != 1 {
        return Err("inventory version must be 1".to_owned());
    }
    if inventory.scope.tracked_patterns != REQUIRED_PATTERNS {
        return Err("scope.trackedPatterns must match the fixed maintained-prose scope".to_owned());
    }
    let exclusions = &inventory.scope.excluded_prefixes;
    if exclusions.iter().any(|prefix| !prefix.ends_with('/'))
        || exclusions.iter().collect::<BTreeSet<_>>().len() != exclusions.len()
    {
        return Err(
            "scope.excludedPrefixes must be an array of unique directory prefixes".to_owned(),
        );
    }
    let unsupported = exclusions
        .iter()
        .filter(|prefix| prefix.as_str() != "firstmate/")
        .cloned()
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "scope.excludedPrefixes contains unsupported roots: {}",
            unsupported.join(", ")
        ));
    }
    let audiences = inventory.allowed_audiences.iter().collect::<BTreeSet<_>>();
    if audiences.is_empty()
        || inventory.setup_audiences.is_empty()
        || inventory.readme_setup_targets.is_empty()
        || inventory
            .allowed_audiences
            .iter()
            .chain(&inventory.setup_audiences)
            .chain(&inventory.readme_setup_targets)
            .any(|value| value.is_empty())
    {
        return Err(
            "allowedAudiences, setupAudiences, and readmeSetupTargets must be non-empty string arrays"
                .to_owned(),
        );
    }
    if inventory
        .setup_audiences
        .iter()
        .any(|audience| !audiences.contains(audience))
    {
        return Err("setupAudiences contains an audience outside allowedAudiences".to_owned());
    }
    let setup = inventory.setup_audiences.iter().collect::<BTreeSet<_>>();
    let mut classifications = BTreeMap::new();
    for surface in &inventory.surfaces {
        if surface.path.is_empty() || !audiences.contains(&surface.audience) {
            return Err(format!(
                "{}: unsupported audience {:?}",
                surface.path, surface.audience
            ));
        }
        if classifications
            .insert(surface.path.clone(), surface.audience.clone())
            .is_some()
        {
            return Err(format!(
                "surfaces classified more than once: {}",
                surface.path
            ));
        }
    }
    let patterns = inventory.scope.tracked_patterns.clone();
    let tracked = git_tracked(root, &patterns)?
        .into_iter()
        .filter(|path| !exclusions.iter().any(|prefix| path.starts_with(prefix)))
        .collect::<BTreeSet<_>>();
    let classified = classifications.keys().cloned().collect::<BTreeSet<_>>();
    let missing = tracked.difference(&classified).cloned().collect::<Vec<_>>();
    let extra = classified.difference(&tracked).cloned().collect::<Vec<_>>();
    if !missing.is_empty() || !extra.is_empty() {
        let mut details = Vec::new();
        if !missing.is_empty() {
            details.push(format!("unclassified: {}", missing.join(", ")));
        }
        if !extra.is_empty() {
            details.push(format!("not tracked/in scope: {}", extra.join(", ")));
        }
        return Err(details.join("; "));
    }
    let readme = root.join("README.md");
    let readme_targets = local_links(root, &readme)?
        .into_iter()
        .filter_map(|(_, target, _)| {
            target
                .strip_prefix(root)
                .ok()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
        })
        .collect::<BTreeSet<_>>();
    for target in &inventory.readme_setup_targets {
        if !readme_targets.contains(target) {
            return Err(format!(
                "README setup target is not linked from README.md: {target}"
            ));
        }
        if classifications
            .get(target)
            .is_none_or(|audience| !setup.contains(audience))
        {
            return Err(format!(
                "README setup target {target} has disallowed audience {:?}",
                classifications.get(target)
            ));
        }
    }
    if inventory.required_owner_pointers.is_empty() {
        return Err("requiredOwnerPointers must be a non-empty array".to_owned());
    }
    for pointer in &inventory.required_owner_pointers {
        if pointer.source.is_empty() || pointer.target.is_empty() {
            return Err(
                "requiredOwnerPointers entries need non-empty source and target".to_owned(),
            );
        }
        let source = root.join(&pointer.source);
        let target = root.join(&pointer.target);
        if !source.exists() {
            return Err(format!(
                "owner-pointer source is missing: {}",
                pointer.source
            ));
        }
        if !target.exists() {
            return Err(format!(
                "owner-pointer target is missing: {}",
                pointer.target
            ));
        }
        let text = fs::read_to_string(&source).map_err(|error| {
            format!(
                "owner-pointer source is unreadable {}: {error}",
                pointer.source
            )
        })?;
        let linked = local_links(root, &source)?
            .into_iter()
            .filter_map(|(_, path, _)| {
                path.strip_prefix(root)
                    .ok()
                    .map(|path| path.to_string_lossy().replace('\\', "/"))
            })
            .collect::<BTreeSet<_>>();
        if !text.contains(&pointer.target) && !linked.contains(&pointer.target) {
            return Err(format!(
                "required owner pointer missing: {} -> {}",
                pointer.source, pointer.target
            ));
        }
    }
    let mut checked = 0;
    let mut anchor_cache = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for path in &tracked {
        if !matches!(
            Path::new(path).extension().and_then(|value| value.to_str()),
            Some("md" | "mdx")
        ) {
            continue;
        }
        let source = root.join(path);
        for (raw, target, fragment) in local_links(root, &source)? {
            checked += 1;
            if !target.exists() {
                return Err(format!("unresolved local link in {path}: {raw}"));
            }
            let canonical = target.canonicalize().map_err(|error| {
                format!("cannot resolve local link target in {path}: {raw}: {error}")
            })?;
            if !canonical.starts_with(root) {
                return Err(format!("local link escapes repository in {path}: {raw}"));
            }
            if !fragment.is_empty()
                && target.is_file()
                && matches!(
                    target.extension().and_then(|value| value.to_str()),
                    Some("md" | "mdx")
                )
                && !anchor_cache
                    .entry(target.clone())
                    .or_insert(anchors(&target)?)
                    .contains(&fragment)
            {
                return Err(format!("unresolved local anchor in {path}: {raw}"));
            }
        }
    }
    Ok((tracked.len(), checked))
}

pub(super) fn run(args: &[OsString]) -> i32 {
    let (root, inventory) = match parse_args(args) {
        Ok(Some(values)) => values,
        Ok(None) => {
            print!("{HELP}");
            return 0;
        }
        Err(error) => {
            eprintln!("mx-doc-audience-check: {error}");
            return 2;
        }
    };
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("mx-doc-audience-check: cannot resolve root: {error}");
            return 1;
        }
    };
    let inventory = inventory.map_or_else(
        || root.join("docs/documentation-audiences.json"),
        |path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        },
    );
    match validate(&root, &inventory) {
        Ok((surfaces, links)) => {
            println!("mx-doc-audience-check: ok surfaces={surfaces} local_links={links}");
            0
        }
        Err(error) => {
            eprintln!("mx-doc-audience-check: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_and_heading_helpers_match_contract() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(heading_slug("Hello, `World`!"), "hello-world");
    }
}

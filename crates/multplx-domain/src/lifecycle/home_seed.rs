//! Persistent daemon-home validation and transactional seeding.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use regex::Regex;

pub const USAGE: &str = "usage: mx-home-seed.sh <id> <home|-> {<project>...|--no-projects}\n       mx-home-seed.sh validate\n";

#[derive(Clone, Debug)]
struct Route {
    id: String,
    home: PathBuf,
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
        std::env::current_dir()
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

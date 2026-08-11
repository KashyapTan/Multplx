//! Validated main-to-daemon backlog handoff.

use std::fs;
use std::path::{Path, PathBuf};

use crate::backlog::move_items;
use crate::inheritance::validate_daemon_home;

#[derive(Debug)]
pub struct HandoffFailure {
    pub message: String,
}

fn fail(message: impl Into<String>) -> HandoffFailure {
    HandoffFailure {
        message: message.into(),
    }
}

fn registry_home(registry: &Path, id: &str) -> Result<PathBuf, HandoffFailure> {
    let text = fs::read_to_string(registry).map_err(|_| {
        fail(format!(
            "error: no daemon registry at {}",
            registry.display()
        ))
    })?;
    let mut matching = None;
    for line in text.lines() {
        if line == format!("- {id}") || line.starts_with(&format!("- {id} ")) {
            matching = Some(line);
        }
    }
    let line = matching.ok_or_else(|| {
        fail(format!(
            "error: daemon {id} is not registered in {}",
            registry.display()
        ))
    })?;
    let marker = "(home:";
    let Some(start) = line.rfind(marker) else {
        return Err(fail(format!(
            "error: daemon {id} has no home in {}",
            registry.display()
        )));
    };
    let remainder = line[start + marker.len()..].trim_start();
    let Some(end) = remainder.find(';') else {
        return Err(fail(format!(
            "error: daemon {id} has no home in {}",
            registry.display()
        )));
    };
    let home = remainder[..end].trim_end();
    if home.is_empty() {
        return Err(fail(format!(
            "error: daemon {id} has no home in {}",
            registry.display()
        )));
    }
    Ok(PathBuf::from(home))
}

fn validate_backlog(label: &str, path: &Path) -> Result<(), HandoffFailure> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(fail(format!(
            "error: {label} must not be a symlink: {}",
            path.display()
        )));
    }
    if path.exists() && !path.is_file() {
        return Err(fail(format!(
            "error: {label} is not a regular file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn classify(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut section = "## Queued".to_owned();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("##") {
            let normalized = rest.split_whitespace().collect::<Vec<_>>().join(" ");
            section = format!("## {normalized}");
            continue;
        }
        if line.starts_with("- [ ] ") || line.starts_with("- [x] ") {
            let id = line[6..].split_whitespace().next().unwrap_or("");
            if id == key {
                return Some(section);
            }
        }
    }
    None
}

fn noncanonical_lines(path: &Path, key: &str) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut capturing = false;
    let mut output = Vec::new();
    for line in text.lines() {
        if line.starts_with("- [ ] ") || line.starts_with("- [x] ") {
            if capturing {
                break;
            }
            capturing = line[6..].split_whitespace().next().unwrap_or("") == key;
            continue;
        }
        if capturing && line.starts_with("##") {
            break;
        }
        if capturing
            && (line.starts_with(' ') || line.starts_with('\t'))
            && !line.starts_with("  ")
            && !line.trim().is_empty()
        {
            output.push(line.to_owned());
        }
    }
    output
}

pub fn run(
    root: &Path,
    home: &Path,
    data: &Path,
    id: &str,
    keys: &[String],
) -> Result<String, HandoffFailure> {
    if keys.is_empty() {
        return Err(fail(
            "usage: mx-backlog-handoff.sh <daemon-id> <item-key>...",
        ));
    }
    let registry = data.join("daemons.md");
    let raw_home = registry_home(&registry, id)?;
    let destination_home = validate_daemon_home(id, &raw_home, home, root).map_err(|reason| {
        fail(format!(
            "error: Multplx home {} is unsafe: {reason}",
            raw_home.display()
        ))
    })?;
    let source = data.join("backlog.md");
    let destination = destination_home.path.join("data/backlog.md");
    validate_backlog("main backlog", &source)?;
    validate_backlog("daemon backlog", &destination)?;

    let mut to_move = Vec::new();
    let mut already = Vec::new();
    let mut missing = Vec::new();
    let mut in_flight = Vec::new();
    let mut done = Vec::new();
    let mut nonqueued = Vec::new();
    for key in keys {
        if classify(&destination, key).is_some() {
            already.push(key.clone());
        } else {
            match classify(&source, key).as_deref() {
                Some("## Queued") => to_move.push(key.clone()),
                Some("## In flight") => in_flight.push(key.clone()),
                Some("## Done") => done.push(key.clone()),
                Some(_) => nonqueued.push(key.clone()),
                None => missing.push(key.clone()),
            }
        }
    }
    let mut errors = String::new();
    if !in_flight.is_empty() {
        errors.push_str(&format!(
            "error: refusing to hand off in-flight backlog items: {}\n",
            in_flight.join(" ")
        ));
    }
    if !done.is_empty() {
        errors.push_str(&format!(
            "error: refusing to hand off Done (historical) backlog items: {}; handoffs move in-scope queued work only - Done records stay with their home and are pruned/archived.\n",
            done.join(" ")
        ));
    }
    if !nonqueued.is_empty() {
        errors.push_str(&format!(
            "error: refusing to hand off non-queued backlog items: {}; handoffs move in-scope queued work only.\n",
            nonqueued.join(" ")
        ));
    }
    if !missing.is_empty() {
        errors.push_str(&format!(
            "error: no backlog item matched these keys in {}: {}\n",
            source.display(),
            missing.join(" ")
        ));
    }
    if !errors.is_empty() {
        errors.push_str("       nothing was moved.");
        return Err(fail(errors));
    }
    if to_move.is_empty() {
        return Ok(format!(
            "nothing to move: {} already present in {}\n",
            if already.is_empty() {
                "no keys".to_owned()
            } else {
                already.join(" ")
            },
            destination.display()
        ));
    }
    let mut malformed = String::new();
    for key in &to_move {
        for line in noncanonical_lines(&source, key) {
            malformed.push_str(&format!(
                "error: refusing to hand off {key}: non-2-space continuation line: {line}\n"
            ));
        }
    }
    if !malformed.is_empty() {
        malformed.push_str("       nothing was moved.");
        return Err(fail(malformed));
    }
    fs::create_dir_all(destination_home.path.join("data"))
        .map_err(|error| fail(error.to_string()))?;
    move_items(&source, &destination, &to_move).map_err(|error| {
        fail(format!(
            "mx-backlog: {error}\nerror: backlog move failed; nothing was moved."
        ))
    })?;
    let mut output = format!(
        "handed off {} item(s) to {id}: {}\n  into {}\n",
        to_move.len(),
        to_move.join(" "),
        destination.display()
    );
    if !already.is_empty() {
        output.push_str(&format!(
            "  already present (skipped): {}\n",
            already.join(" ")
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backlog(queued: &str, in_flight: &str, done: &str) -> String {
        format!("## In flight\n{in_flight}\n## Queued\n{queued}\n## Done\n{done}")
    }

    fn seeded_homes() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let active = temp.path().join("active");
        let daemon = temp.path().join("daemon");
        for path in [&root, &active, &daemon] {
            fs::create_dir_all(path).expect("home");
        }
        fs::create_dir_all(active.join("data")).expect("active data");
        for name in ["data", "state", "config", "projects", "bin"] {
            fs::create_dir_all(daemon.join(name)).expect("daemon surface");
        }
        fs::write(daemon.join(".mx-daemon-home"), "worker\n").expect("marker");
        fs::write(daemon.join("AGENTS.md"), "# daemon\n").expect("agents");
        fs::write(
            active.join("data/daemons.md"),
            format!(
                "- worker - tests (home: {}; scope: tests)\n",
                daemon.display()
            ),
        )
        .expect("registry");
        (temp, root, active, daemon)
    }

    #[test]
    fn registry_uses_last_home_field_after_parenthesized_prose() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("daemons.md");
        fs::write(
            &registry,
            "- daemon - work (id is legacy) (home: /first; note: x) (home: /last; scope: work)\n",
        )
        .expect("registry");
        assert_eq!(
            registry_home(&registry, "daemon").expect("home"),
            Path::new("/last")
        );
    }

    #[test]
    fn lightweight_classification_accepts_whitespace_headings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("backlog.md");
        fs::write(&path, "##\tDone\n- [x] old - old\n").expect("backlog");
        assert_eq!(classify(&path, "old").as_deref(), Some("## Done"));
    }

    #[test]
    fn handoff_moves_queued_items_and_reports_idempotent_keys() {
        let (_temp, root, active, daemon) = seeded_homes();
        fs::write(
            active.join("data/backlog.md"),
            backlog("- [ ] new - New\n  body\n", "", ""),
        )
        .expect("source");
        fs::write(
            daemon.join("data/backlog.md"),
            backlog("- [ ] existing - Existing\n", "", ""),
        )
        .expect("destination");
        let keys = ["new".to_owned(), "existing".to_owned()];
        let output = run(&root, &active, &active.join("data"), "worker", &keys).expect("handoff");
        assert!(output.contains("handed off 1 item(s)"));
        assert!(output.contains("already present (skipped): existing"));
        assert!(
            !fs::read_to_string(active.join("data/backlog.md"))
                .expect("source")
                .contains("new - New")
        );
        assert!(
            fs::read_to_string(daemon.join("data/backlog.md"))
                .expect("destination")
                .contains("  body")
        );

        let second =
            run(&root, &active, &active.join("data"), "worker", &keys).expect("idempotent");
        assert!(second.starts_with("nothing to move:"));
    }

    #[test]
    fn handoff_refuses_every_nonqueued_class_without_mutation() {
        let (_temp, root, active, daemon) = seeded_homes();
        let source = backlog(
            "- [ ] queued - Queued\n",
            "- [ ] active - Active\n",
            "- [x] historical - Historical\n",
        )
        .replace("## Queued", "## Other\n- [ ] other - Other\n\n## Queued");
        fs::write(active.join("data/backlog.md"), &source).expect("source");
        fs::write(daemon.join("data/backlog.md"), backlog("", "", "")).expect("destination");
        let keys = [
            "active".to_owned(),
            "historical".to_owned(),
            "other".to_owned(),
            "missing".to_owned(),
        ];
        let error =
            run(&root, &active, &active.join("data"), "worker", &keys).expect_err("refusal");
        assert!(error.message.contains("in-flight"));
        assert!(error.message.contains("Done (historical)"));
        assert!(error.message.contains("non-queued"));
        assert!(error.message.contains("no backlog item matched"));
        assert!(error.message.ends_with("nothing was moved."));
        assert_eq!(
            fs::read_to_string(active.join("data/backlog.md")).expect("after"),
            source
        );
    }

    #[test]
    fn handoff_refuses_noncanonical_body_and_unsafe_backlogs() {
        let (_temp, root, active, daemon) = seeded_homes();
        fs::write(
            active.join("data/backlog.md"),
            backlog("- [ ] bad - Bad\n body\n", "", ""),
        )
        .expect("source");
        fs::write(daemon.join("data/backlog.md"), backlog("", "", "")).expect("destination");
        let error = run(
            &root,
            &active,
            &active.join("data"),
            "worker",
            &["bad".to_owned()],
        )
        .expect_err("noncanonical");
        assert!(error.message.contains("non-2-space continuation"));

        fs::remove_file(daemon.join("data/backlog.md")).expect("remove");
        fs::create_dir(daemon.join("data/backlog.md")).expect("directory");
        let error = run(
            &root,
            &active,
            &active.join("data"),
            "worker",
            &["bad".to_owned()],
        )
        .expect_err("unsafe destination");
        assert!(
            error
                .message
                .contains("daemon backlog is not a regular file")
        );
    }

    #[test]
    fn registry_and_home_validation_fail_closed() {
        let (_temp, root, active, _daemon) = seeded_homes();
        assert!(
            run(&root, &active, &active.join("data"), "worker", &[])
                .expect_err("usage")
                .message
                .starts_with("usage:")
        );
        assert!(
            run(
                &root,
                &active,
                &active.join("data"),
                "missing",
                &["x".to_owned()]
            )
            .expect_err("missing daemon")
            .message
            .contains("not registered")
        );
        fs::write(active.join("data/daemons.md"), "- worker - no home\n").expect("registry");
        assert!(
            run(
                &root,
                &active,
                &active.join("data"),
                "worker",
                &["x".to_owned()]
            )
            .expect_err("missing home")
            .message
            .contains("has no home")
        );
        fs::remove_file(active.join("data/daemons.md")).expect("remove registry");
        assert!(
            run(
                &root,
                &active,
                &active.join("data"),
                "worker",
                &["x".to_owned()]
            )
            .expect_err("missing registry")
            .message
            .contains("no daemon registry")
        );
    }

    #[test]
    fn malformed_registry_home_fields_and_backlog_symlinks_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("daemons.md");
        for row in [
            "- worker - incomplete (home: /tmp/worker)\n",
            "- worker - empty (home: ; scope: test)\n",
        ] {
            fs::write(&registry, row).expect("registry");
            assert!(
                registry_home(&registry, "worker")
                    .expect_err("malformed home")
                    .message
                    .contains("has no home")
            );
        }
        let target = temp.path().join("target-backlog");
        fs::write(&target, backlog("", "", "")).expect("target");
        let linked = temp.path().join("linked-backlog");
        std::os::unix::fs::symlink(&target, &linked).expect("backlog symlink");
        assert!(
            validate_backlog("fixture backlog", &linked)
                .expect_err("symlink refusal")
                .message
                .contains("must not be a symlink")
        );
    }
}

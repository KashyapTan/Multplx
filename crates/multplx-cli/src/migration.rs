//! CLI boundary for explicit operational-home migration.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::PathBuf;

use multplx_domain::lifecycle::migration;

pub const HELP: &str = "Usage: mx migrate inspect [--home PATH] [--operation ID] [--coordinator TASK]...\n       mx migrate apply [--home PATH] [--operation ID] [--coordinator TASK]...\n       mx migrate rollback --operation ID [--home PATH]\n       mx migrate summary [--home PATH] [--limit N]\n       mx migrate relocate-worktree --metadata FILE --path PATH --project SELECTOR --task TASK --request ID [--home PATH]\n\ninspect is read-only and reports converted, alias-readable, retained and blocked records.\napply refuses a live home writer, writes a matching private backup, and resumes one\nstable multi-file transaction on retry. It never starts an agent, publishes work,\nchanges a journal/queue/receipt, moves a repository, or converts historical approval.\nUse --coordinator only when a persistent legacy task's recorded responsibility is\nknown to be coordination; persistence alone does not infer that role.\nrollback requires the same runtime version, operation backup, home identity, stopped\nwriters and unchanged migrated records. It retains backup and rollback evidence.\nsummary reads bounded task context without replaying transcripts. Missing or corrupt\nhomes are reported as unavailable. Legacy aliases remain readable through Phase 12.\n\nrelocate-worktree is the separate explicit A10 transfer boundary. It accepts one\nrecorded path from inert supplied metadata, a current task/attempt/project owner, and\na quiescent Git worktree. Git moves and reference publication resume on exact retry;\nforeign pool entries and uncertain owners remain untouched.\n\nSupported source homes: unversioned/schema-1 task metadata and schema-2 homes.\nA newer home version is never modified. Treehouse paths remain retained until an\nexact task/attempt owner can use the built-in worktree transfer contract; migration\ndoes not enumerate or take over external pools.\n";

#[derive(Default)]
struct Options {
    home: Option<PathBuf>,
    operation: Option<String>,
    coordinators: BTreeSet<String>,
    limit: Option<usize>,
    metadata: Option<PathBuf>,
    path: Option<PathBuf>,
    project: Option<String>,
    task: Option<String>,
    request: Option<String>,
}

fn parse(args: &[OsString]) -> Result<(&str, Options), String> {
    let action = args
        .first()
        .and_then(|value| value.to_str())
        .ok_or_else(|| HELP.to_owned())?;
    if !matches!(
        action,
        "inspect" | "apply" | "rollback" | "summary" | "relocate-worktree"
    ) {
        return Err(HELP.to_owned());
    }
    let mut options = Options::default();
    let mut index = 1;
    while index < args.len() {
        let key = args[index]
            .to_str()
            .ok_or("migration arguments must be UTF-8")?;
        let value = |index: &mut usize| -> Result<&str, String> {
            *index += 1;
            args.get(*index)
                .and_then(|value| value.to_str())
                .ok_or_else(|| format!("{key} requires a UTF-8 value"))
        };
        match key {
            "--home" if options.home.is_none() => {
                options.home = Some(PathBuf::from(value(&mut index)?));
            }
            "--operation" if options.operation.is_none() => {
                options.operation = Some(value(&mut index)?.to_owned());
            }
            "--coordinator" if matches!(action, "inspect" | "apply") => {
                if !options.coordinators.insert(value(&mut index)?.to_owned()) {
                    return Err("duplicate --coordinator task".into());
                }
            }
            "--limit" if action == "summary" && options.limit.is_none() => {
                let limit = value(&mut index)?;
                options.limit = Some(
                    limit
                        .parse::<usize>()
                        .ok()
                        .filter(|value| *value > 0 && *value <= 10_000)
                        .ok_or("--limit must be between 1 and 10000")?,
                );
            }
            "--metadata" if action == "relocate-worktree" && options.metadata.is_none() => {
                options.metadata = Some(PathBuf::from(value(&mut index)?));
            }
            "--path" if action == "relocate-worktree" && options.path.is_none() => {
                options.path = Some(PathBuf::from(value(&mut index)?));
            }
            "--project" if action == "relocate-worktree" && options.project.is_none() => {
                options.project = Some(value(&mut index)?.to_owned());
            }
            "--task" if action == "relocate-worktree" && options.task.is_none() => {
                options.task = Some(value(&mut index)?.to_owned());
            }
            "--request" if action == "relocate-worktree" && options.request.is_none() => {
                options.request = Some(value(&mut index)?.to_owned());
            }
            _ => return Err(format!("unexpected migration argument {key:?}\n\n{HELP}")),
        }
        index += 1;
    }
    if action == "rollback" && options.operation.is_none() {
        return Err("rollback requires --operation ID".into());
    }
    if action == "relocate-worktree"
        && (options.metadata.is_none()
            || options.path.is_none()
            || options.project.is_none()
            || options.task.is_none()
            || options.request.is_none())
    {
        return Err(
            "relocate-worktree requires --metadata, --path, --project, --task and --request".into(),
        );
    }
    Ok((action, options))
}

pub fn run(args: &[OsString]) -> i32 {
    if args.is_empty() || args.iter().any(|arg| arg == "-h" || arg == "--help") {
        print!("{HELP}");
        return 0;
    }
    let (action, options) = match parse(args) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("mx migrate: {error}");
            return 2;
        }
    };
    let (_, default_home, _) = crate::active_paths();
    let home = options.home.as_deref().unwrap_or(&default_home);
    let operation = options.operation.as_deref().unwrap_or("lean-phase09-v1");
    let result = match action {
        "inspect" => migration::inspect(home, operation, &options.coordinators).and_then(json),
        "apply" => migration::apply(home, operation, &options.coordinators).and_then(json),
        "rollback" => migration::rollback(home, operation).and_then(json),
        "summary" => json(migration::restart_summary(
            home,
            options.limit.unwrap_or(20),
        )),
        "relocate-worktree" => migration::relocate_worktree(
            home,
            options.metadata.as_deref().expect("parsed"),
            options.path.as_deref().expect("parsed"),
            options.project.as_deref().expect("parsed"),
            options.task.as_deref().expect("parsed"),
            options.request.as_deref().expect("parsed"),
        )
        .and_then(json),
        _ => unreachable!(),
    };
    match result {
        Ok(output) => {
            println!("{output}");
            0
        }
        Err(error) => {
            eprintln!("mx migrate: {error}");
            1
        }
    }
}

fn json(value: impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parser_covers_every_action_and_option() {
        let args = words(&[
            "inspect",
            "--home",
            "/tmp/home",
            "--operation",
            "upgrade",
            "--coordinator",
            "alpha",
            "--coordinator",
            "beta",
        ]);
        let (action, options) = parse(&args).unwrap();
        assert_eq!(action, "inspect");
        assert_eq!(options.home, Some(PathBuf::from("/tmp/home")));
        assert_eq!(options.operation.as_deref(), Some("upgrade"));
        assert_eq!(options.coordinators.len(), 2);

        let args = words(&["summary", "--limit", "37", "--home", "/tmp/home"]);
        let (action, options) = parse(&args).unwrap();
        assert_eq!(action, "summary");
        assert_eq!(options.limit, Some(37));

        let args = words(&[
            "relocate-worktree",
            "--metadata",
            "/tmp/treehouse.json",
            "--path",
            "/tmp/legacy",
            "--project",
            "project",
            "--task",
            "task",
            "--request",
            "request",
            "--home",
            "/tmp/home",
        ]);
        let (action, options) = parse(&args).unwrap();
        assert_eq!(action, "relocate-worktree");
        assert_eq!(options.metadata, Some(PathBuf::from("/tmp/treehouse.json")));
        assert_eq!(options.path, Some(PathBuf::from("/tmp/legacy")));
        assert_eq!(options.project.as_deref(), Some("project"));
        assert_eq!(options.task.as_deref(), Some("task"));
        assert_eq!(options.request.as_deref(), Some("request"));

        assert_eq!(parse(&words(&["apply"])).unwrap().0, "apply");
        assert_eq!(
            parse(&words(&["rollback", "--operation", "upgrade"]))
                .unwrap()
                .0,
            "rollback"
        );
    }

    #[test]
    fn parser_rejects_incomplete_ambiguous_and_out_of_range_requests() {
        for args in [
            vec![],
            words(&["unknown"]),
            words(&["rollback"]),
            words(&["summary", "--limit", "0"]),
            words(&["summary", "--limit", "10001"]),
            words(&["summary", "--limit", "not-a-number"]),
            words(&["inspect", "--coordinator", "same", "--coordinator", "same"]),
            words(&["inspect", "--home"]),
            words(&["inspect", "--unexpected", "value"]),
            words(&["relocate-worktree", "--metadata", "/tmp/treehouse.json"]),
        ] {
            assert!(parse(&args).is_err(), "accepted {args:?}");
        }
    }

    #[test]
    fn public_boundary_covers_help_usage_summary_and_inspection() {
        assert_eq!(run(&[]), 0);
        assert_eq!(run(&words(&["--help"])), 0);
        assert_eq!(run(&words(&["unknown"])), 2);

        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join("state")).unwrap();
        std::fs::create_dir_all(home.join("data")).unwrap();
        std::fs::create_dir_all(home.join("config")).unwrap();
        let home = home.to_string_lossy();
        assert_eq!(
            run(&words(&[
                "summary",
                "--home",
                home.as_ref(),
                "--limit",
                "1",
            ])),
            0
        );
        assert_eq!(
            run(&words(&[
                "inspect",
                "--home",
                home.as_ref(),
                "--operation",
                "cli-test",
            ])),
            0
        );
        assert_eq!(
            run(&words(&[
                "apply",
                "--home",
                home.as_ref(),
                "--operation",
                "cli-test",
            ])),
            0
        );
        assert_eq!(
            run(&words(&[
                "rollback",
                "--home",
                home.as_ref(),
                "--operation",
                "cli-test",
            ])),
            0
        );
        assert_eq!(
            run(&words(&[
                "relocate-worktree",
                "--home",
                home.as_ref(),
                "--metadata",
                "/missing/treehouse.json",
                "--path",
                "/missing/worktree",
                "--project",
                "project",
                "--task",
                "task",
                "--request",
                "request",
            ])),
            1
        );
    }
}

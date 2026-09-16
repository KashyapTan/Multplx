//! Routed, generation-fenced task ownership transfer command.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use multplx_domain::handoff::{
    ExecutionDisposition, ScopeChange, TransferRequest, retained_transfer_request, transfer_task,
};
use multplx_domain::lifecycle::subagent_model::{TaskRecord, qualified_task_id, read_meta};

pub(crate) const USAGE: &str = "usage: mx task-transfer <task-id> --to <coordinator-id|root> --request-id <id> --expected-generation <n> --reason <text> [--to-state <absolute-path>] [--scope <text> --scope-reason <text> [--accept <text>]... [--source <path>]...] [--authority-state <absolute-path>]\n\nRun this command in the task's current owner home. A successor coordinator supplies --authority-state to route the command to the retained canonical record. The command durably records intent, stops and verifies the old task endpoint, then changes its attempt generation and parent route while retaining the one canonical task record at its recorded authority state. The successor resumes it with mx spawn ... --authority-state <the reported path>.\n";

#[derive(Debug, Default)]
struct Arguments {
    task_id: String,
    to: String,
    request_id: String,
    expected_generation: Option<u64>,
    reason: String,
    to_state: Option<PathBuf>,
    scope: Option<String>,
    scope_reason: Option<String>,
    acceptance: Vec<String>,
    sources: Vec<String>,
}

fn parse(args: &[OsString]) -> Result<Arguments, String> {
    if args
        .iter()
        .any(|arg| matches!(arg.to_str(), Some("-h" | "--help")))
    {
        return Err(USAGE.to_owned());
    }
    let Some(task_id) = args.first().and_then(|arg| arg.to_str()) else {
        return Err(USAGE.to_owned());
    };
    let mut parsed = Arguments {
        task_id: task_id.to_owned(),
        ..Arguments::default()
    };
    let mut index = 1;
    while index < args.len() {
        let option = args[index]
            .to_str()
            .ok_or("task transfer argument is not UTF-8")?;
        let value = args
            .get(index + 1)
            .and_then(|arg| arg.to_str())
            .ok_or_else(|| format!("missing value for {option}"))?;
        match option {
            "--to" => parsed.to = value.to_owned(),
            "--request-id" => parsed.request_id = value.to_owned(),
            "--expected-generation" => {
                parsed.expected_generation = value.parse::<u64>().ok().filter(|value| *value > 0)
            }
            "--reason" => parsed.reason = value.to_owned(),
            "--to-state" => parsed.to_state = Some(PathBuf::from(value)),
            "--scope" => parsed.scope = Some(value.to_owned()),
            "--scope-reason" => parsed.scope_reason = Some(value.to_owned()),
            "--accept" => parsed.acceptance.push(value.to_owned()),
            "--source" => parsed.sources.push(value.to_owned()),
            _ => return Err(format!("unknown task transfer option: {option}\n{USAGE}")),
        }
        index += 2;
    }
    if parsed.to.is_empty()
        || parsed.request_id.is_empty()
        || parsed.expected_generation.is_none()
        || parsed.reason.trim().is_empty()
        || parsed.scope.is_some() != parsed.scope_reason.is_some()
    {
        return Err(USAGE.to_owned());
    }
    Ok(parsed)
}

fn active_home() -> PathBuf {
    std::env::var_os("MX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("MX_ROOT_OVERRIDE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn active_state(home: &Path) -> PathBuf {
    std::env::var_os("MX_STATE_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("state"))
}

fn read_task(state: &Path, id: &str) -> Result<TaskRecord, String> {
    let path = state.join(format!("{id}.meta"));
    read_meta(
        id,
        &fs::read_to_string(&path)
            .map_err(|error| format!("cannot read task authority {}: {error}", path.display()))?,
    )
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    fs::canonicalize(left).ok() == fs::canonicalize(right).ok()
}

fn canonical_current_owner(task: &TaskRecord) -> Result<String, String> {
    let owner = task
        .owning_coordinator
        .as_deref()
        .or(task.parent_id.as_deref())
        .ok_or("task has no current coordinator")?;
    if owner.starts_with("root-home:") || owner.contains("#task:") {
        Ok(owner.to_owned())
    } else {
        Ok(qualified_task_id(
            task.parent_home
                .as_deref()
                .ok_or("task parent home missing")?,
            owner,
        ))
    }
}

fn destination(
    args: &Arguments,
    task: &TaskRecord,
) -> Result<(String, PathBuf, PathBuf, PathBuf, PathBuf), String> {
    let root_id = task
        .root_id
        .as_deref()
        .ok_or("task root identity missing")?;
    let root_home = root_id
        .strip_prefix("root-home:")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("task root identity is invalid")?;
    if args.to == "root" || args.to == root_id {
        let root_state = args
            .to_state
            .clone()
            .unwrap_or_else(|| root_home.join("state"));
        if !root_state.is_absolute() || !root_state.is_dir() {
            return Err("destination root state must be an existing absolute directory".into());
        }
        let root_state = fs::canonicalize(root_state)
            .map_err(|error| format!("cannot resolve destination root state: {error}"))?;
        return Ok((
            root_id.to_owned(),
            root_home.clone(),
            root_state.clone(),
            root_home.clone(),
            root_state,
        ));
    }
    let id = args
        .to
        .rsplit_once("#task:")
        .map_or(args.to.as_str(), |(_, id)| id);
    let state = args
        .to_state
        .clone()
        .unwrap_or_else(|| root_home.join("state"));
    if !state.is_absolute() {
        return Err("destination coordinator state must be absolute".into());
    }
    let coordinator = read_task(&state, id)?;
    let owner_home = PathBuf::from(
        coordinator
            .owner_home
            .as_deref()
            .ok_or("destination coordinator owner home missing")?,
    );
    let owner_state = PathBuf::from(
        coordinator
            .owner_state
            .as_deref()
            .ok_or("destination coordinator owner state missing")?,
    );
    if owner_state != state {
        return Err("destination coordinator was read from the wrong owner state".into());
    }
    let canonical = qualified_task_id(
        owner_home
            .to_str()
            .ok_or("destination coordinator home is not UTF-8")?,
        id,
    );
    if args.to.contains("#task:") && args.to != canonical {
        return Err("destination coordinator qualified identity is wrong-home".into());
    }
    let runtime_home = coordinator
        .persistent_home
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| owner_home.clone());
    Ok((
        canonical,
        owner_home,
        owner_state,
        runtime_home.clone(),
        runtime_home.join("state"),
    ))
}

pub(crate) fn run(args: &[OsString], route: Option<(PathBuf, PathBuf)>) -> i32 {
    let args = match parse(args) {
        Ok(args) => args,
        Err(message) if message == USAGE => {
            print!("{message}");
            return if args
                .iter()
                .any(|arg| matches!(arg.to_str(), Some("-h" | "--help")))
            {
                0
            } else {
                2
            };
        }
        Err(message) => {
            eprintln!("error: {message}");
            return 2;
        }
    };
    let (home, state) = route.unwrap_or_else(|| {
        let home = active_home();
        let state = active_state(&home);
        (home, state)
    });
    let state = match fs::canonicalize(&state) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("error: cannot resolve canonical task authority state: {error}");
            return 1;
        }
    };
    let task = match read_task(&state, &args.task_id) {
        Ok(task) => task,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    if task
        .owner_state
        .as_deref()
        .is_none_or(|owner| !same_existing_path(Path::new(owner), &state))
        || task
            .owner_home
            .as_deref()
            .is_none_or(|owner| !same_existing_path(Path::new(owner), &home))
    {
        eprintln!("error: task-transfer must run in the canonical task owner home");
        return 1;
    }
    let from = match canonical_current_owner(&task) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let (
        to,
        destination_coordinator_owner_home,
        destination_coordinator_owner_state,
        destination_home,
        destination_state,
    ) = match destination(&args, &task) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let proposed = TransferRequest {
        transfer_id: args.request_id,
        task_id: args.task_id.clone(),
        authoritative_state: state.clone(),
        expected_attempt_generation: args.expected_generation.expect("validated"),
        from_coordinator: from,
        to_coordinator: to,
        destination_coordinator_owner_home,
        destination_coordinator_owner_state,
        destination_home,
        destination_state,
        reason: args.reason,
        scope_change: args.scope.map(|scope| ScopeChange {
            expected_revision: task.accepted_brief_revision.unwrap_or_default(),
            scope,
            acceptance_criteria: args.acceptance,
            source_artifacts: args.sources,
            reason: args.scope_reason.expect("paired scope reason"),
        }),
    };
    let request = match retained_transfer_request(&state, &proposed.transfer_id) {
        Ok(Some(retained)) => {
            let scope_same = match (&retained.scope_change, &proposed.scope_change) {
                (None, None) => true,
                (Some(left), Some(right)) => {
                    left.scope == right.scope
                        && left.acceptance_criteria == right.acceptance_criteria
                        && left.source_artifacts == right.source_artifacts
                        && left.reason == right.reason
                }
                _ => false,
            };
            let same = retained.transfer_id == proposed.transfer_id
                && retained.task_id == proposed.task_id
                && retained.authoritative_state == proposed.authoritative_state
                && retained.expected_attempt_generation == proposed.expected_attempt_generation
                && retained.to_coordinator == proposed.to_coordinator
                && retained.destination_coordinator_owner_home
                    == proposed.destination_coordinator_owner_home
                && retained.destination_coordinator_owner_state
                    == proposed.destination_coordinator_owner_state
                && retained.destination_home == proposed.destination_home
                && retained.destination_state == proposed.destination_state
                && retained.reason == proposed.reason
                && scope_same;
            if !same {
                eprintln!("error: task-transfer retry conflicts with retained request");
                return 1;
            }
            retained
        }
        Ok(None) => proposed,
        Err(error) => {
            eprintln!("{}", error.message);
            return 1;
        }
    };
    let meta = state.join(format!("{}.meta", args.task_id));
    match transfer_task(&request, |record| {
        let Some(endpoint) = record.runtime.endpoint.as_deref() else {
            return Ok(ExecutionDisposition::Absent);
        };
        let current = read_task(&state, &record.task_id)?;
        if current.attempt != record.attempt || current.runtime.endpoint != record.runtime.endpoint
        {
            return Ok(ExecutionDisposition::Uncertain);
        }
        super::kill_teardown_endpoint(&meta)?;
        match multplx_backend::facade::observe_endpoint(
            &record.runtime.provider,
            endpoint,
            Some(format!("mx-{}", record.task_id)),
            false,
        ) {
            Ok(observation) if !observation.exists => Ok(ExecutionDisposition::Stopped),
            Ok(_) => Ok(ExecutionDisposition::Uncertain),
            Err(error) => Err(error.to_string()),
        }
    }) {
        Ok(outcome) => {
            println!(
                "transferred task={} request={} generation={} authority_state={} resumed={}",
                outcome.task_id,
                outcome.transfer_id,
                outcome.accepted_generation,
                outcome.authoritative_state.display(),
                outcome.resumed
            );
            0
        }
        Err(error) => {
            eprintln!("{}", error.message);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multplx_domain::lifecycle::subagent_model::{ArtifactKind, AssignmentRole, write_meta};

    fn os(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn write_record(path: &Path, record: &TaskRecord) {
        let kind = if record.role == AssignmentRole::SubOrchestrator {
            "daemon"
        } else {
            "delivery"
        };
        fs::write(
            path,
            write_meta(&format!("kind={kind}\n"), record).expect("record"),
        )
        .expect("write");
    }

    fn task(root: &Path, owner: &Path, state: &Path) -> TaskRecord {
        let root_id = format!("root-home:{}", root.display());
        let mut task = TaskRecord::new(
            "work".into(),
            AssignmentRole::Implementer,
            ArtifactKind::Implementation,
            false,
            root_id.clone(),
            root_id,
            owner.to_string_lossy().into_owned(),
        );
        task.owner_state = Some(state.to_string_lossy().into_owned());
        task
    }

    #[test]
    fn transfer_argument_parser_rejects_ambiguous_and_malformed_intent() {
        assert_eq!(parse(&os(&["--help"])).unwrap_err(), USAGE);
        assert_eq!(parse(&[]).unwrap_err(), USAGE);
        assert!(
            parse(&os(&["work", "--to"]))
                .unwrap_err()
                .contains("missing value")
        );
        assert!(
            parse(&os(&["work", "--wat", "value"]))
                .unwrap_err()
                .contains("unknown task transfer option")
        );
        for generation in ["0", "no"] {
            assert_eq!(
                parse(&os(&[
                    "work",
                    "--to",
                    "root",
                    "--request-id",
                    "move",
                    "--expected-generation",
                    generation,
                    "--reason",
                    "needed",
                ]))
                .unwrap_err(),
                USAGE
            );
        }
        assert_eq!(
            parse(&os(&[
                "work",
                "--to",
                "root",
                "--request-id",
                "move",
                "--expected-generation",
                "1",
                "--reason",
                "needed",
                "--scope",
                "new",
            ]))
            .unwrap_err(),
            USAGE
        );

        let parsed = parse(&os(&[
            "work",
            "--to",
            "root",
            "--request-id",
            "move",
            "--expected-generation",
            "2",
            "--reason",
            "needed",
            "--to-state",
            "/tmp/state",
            "--scope",
            "new",
            "--scope-reason",
            "priority",
            "--accept",
            "passes",
            "--source",
            "plan.md",
        ]))
        .expect("complete intent");
        assert_eq!(parsed.expected_generation, Some(2));
        assert_eq!(parsed.acceptance, ["passes"]);
        assert_eq!(parsed.sources, ["plan.md"]);
        assert_eq!(parsed.to_state.as_deref(), Some(Path::new("/tmp/state")));
    }

    #[cfg(unix)]
    #[test]
    fn transfer_argument_parser_rejects_non_utf8_words() {
        use std::os::unix::ffi::OsStringExt;

        assert!(
            parse(&[OsString::from("work"), OsString::from_vec(vec![0xff])])
                .unwrap_err()
                .contains("not UTF-8")
        );
        assert!(
            parse(&[
                OsString::from("work"),
                OsString::from("--to"),
                OsString::from_vec(vec![0xff]),
            ])
            .unwrap_err()
            .contains("missing value")
        );
    }

    #[test]
    fn current_owner_requires_a_complete_canonical_route() {
        let temp = tempfile::tempdir().unwrap();
        let mut record = task(temp.path(), temp.path(), temp.path());
        assert_eq!(
            canonical_current_owner(&record).unwrap(),
            record.root_id.clone().unwrap()
        );

        record.owning_coordinator = Some("coordinator".into());
        record.parent_home = Some(temp.path().to_string_lossy().into_owned());
        assert_eq!(
            canonical_current_owner(&record).unwrap(),
            qualified_task_id(temp.path().to_str().unwrap(), "coordinator")
        );
        let qualified = qualified_task_id(temp.path().to_str().unwrap(), "other");
        record.owning_coordinator = Some(qualified.clone());
        assert_eq!(canonical_current_owner(&record).unwrap(), qualified);

        record.owning_coordinator = Some("coordinator".into());
        record.parent_home = None;
        assert!(
            canonical_current_owner(&record)
                .unwrap_err()
                .contains("parent home")
        );
        record.owning_coordinator = None;
        record.parent_id = None;
        assert!(
            canonical_current_owner(&record)
                .unwrap_err()
                .contains("no current")
        );
    }

    #[test]
    fn destination_resolution_validates_root_and_private_coordinator_routes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let root_state = temp.path().join("root-authority");
        let runtime = temp.path().join("runtime");
        for path in [&root, &root_state, &runtime] {
            fs::create_dir_all(path).unwrap();
        }
        let record = task(&root, &root, &root_state);
        let mut args = Arguments {
            to: "root".into(),
            to_state: Some(root_state.clone()),
            ..Arguments::default()
        };
        let root_route = destination(&args, &record).expect("custom root state");
        assert_eq!(root_route.0, record.root_id.clone().unwrap());
        assert_eq!(root_route.2, fs::canonicalize(&root_state).unwrap());

        args.to_state = Some(PathBuf::from("relative"));
        assert!(
            destination(&args, &record)
                .unwrap_err()
                .contains("root state")
        );
        let mut invalid_root = record.clone();
        invalid_root.root_id = Some("root-home:relative".into());
        assert!(
            destination(&args, &invalid_root)
                .unwrap_err()
                .contains("root identity")
        );
        invalid_root.root_id = None;
        assert!(
            destination(&args, &invalid_root)
                .unwrap_err()
                .contains("root identity missing")
        );

        let mut coordinator = TaskRecord::new(
            "coordinator".into(),
            AssignmentRole::SubOrchestrator,
            ArtifactKind::Coordination,
            true,
            record.root_id.clone().unwrap(),
            record.root_id.clone().unwrap(),
            root.to_string_lossy().into_owned(),
        );
        coordinator.owner_state = Some(root_state.to_string_lossy().into_owned());
        coordinator.persistent_home = Some(runtime.to_string_lossy().into_owned());
        write_record(&root_state.join("coordinator.meta"), &coordinator);
        args.to = "coordinator".into();
        args.to_state = Some(root_state.clone());
        let route = destination(&args, &record).expect("private route");
        assert_eq!(route.3, runtime);
        assert_eq!(route.4, runtime.join("state"));

        args.to = qualified_task_id(temp.path().join("wrong").to_str().unwrap(), "coordinator");
        assert!(
            destination(&args, &record)
                .unwrap_err()
                .contains("wrong-home")
        );
        args.to = "coordinator".into();
        args.to_state = Some(PathBuf::from("relative"));
        assert!(
            destination(&args, &record)
                .unwrap_err()
                .contains("must be absolute")
        );

        args.to_state = Some(root_state.clone());
        coordinator.owner_state =
            Some(temp.path().join("elsewhere").to_string_lossy().into_owned());
        write_record(&root_state.join("coordinator.meta"), &coordinator);
        assert!(
            destination(&args, &record)
                .unwrap_err()
                .contains("wrong owner state")
        );
        coordinator.owner_state = None;
        write_record(&root_state.join("coordinator.meta"), &coordinator);
        assert!(
            destination(&args, &record)
                .unwrap_err()
                .contains("owner state missing")
        );
    }

    #[test]
    fn command_returns_distinct_usage_authority_and_retained_journal_failures() {
        assert_eq!(run(&os(&["--help"]), None), 0);
        assert_eq!(run(&os(&["work", "--bad", "x"]), None), 2);

        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let state = temp.path().join("authority");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&state).unwrap();
        let valid = os(&[
            "work",
            "--to",
            "root",
            "--request-id",
            "move",
            "--expected-generation",
            "1",
            "--reason",
            "needed",
            "--to-state",
            state.to_str().unwrap(),
        ]);
        assert_eq!(run(&valid, Some((home.clone(), state.clone()))), 1);

        let wrong_owner = temp.path().join("wrong-owner");
        fs::create_dir(&wrong_owner).unwrap();
        let record = task(&home, &wrong_owner, &state);
        write_record(&state.join("work.meta"), &record);
        assert_eq!(run(&valid, Some((home.clone(), state.clone()))), 1);

        let mut record = task(&home, &home, &state);
        record.root_id = Some(format!("root-home:{}", home.display()));
        write_record(&state.join("work.meta"), &record);
        fs::write(state.join(".ownership-transfer.move.json"), "not-json\n").unwrap();
        assert_eq!(run(&valid, Some((home, state))), 1);

        let success = tempfile::tempdir().unwrap();
        let home = success.path().join("home");
        let state = success.path().join("custom-state");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&state).unwrap();
        let mut record = task(&home, &home, &state);
        record.parent_id = Some("old-owner".into());
        record.parent_home = Some(home.to_string_lossy().into_owned());
        record.parent_state = Some(state.to_string_lossy().into_owned());
        record.owning_coordinator = Some(qualified_task_id(home.to_str().unwrap(), "old-owner"));
        write_record(&state.join("work.meta"), &record);
        let successful = os(&[
            "work",
            "--to",
            "root",
            "--request-id",
            "success",
            "--expected-generation",
            "1",
            "--reason",
            "return to root",
            "--to-state",
            state.to_str().unwrap(),
        ]);
        assert_eq!(run(&successful, Some((home.clone(), state.clone()))), 0);
        assert_eq!(run(&successful, Some((home.clone(), state.clone()))), 0);
        let mut conflict = successful;
        let reason = conflict
            .iter()
            .position(|word| word == "return to root")
            .unwrap();
        conflict[reason] = "different reason".into();
        assert_eq!(run(&conflict, Some((home, state))), 1);
    }
}

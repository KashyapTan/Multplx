//! Native persistent-daemon spawn preflight and metadata transaction.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use multplx_core::filesystem::atomic_replace;
use multplx_core::identifiers::TaskId;

use super::home_seed::resolved;

#[derive(Clone, Debug)]
pub struct Context {
    pub root: PathBuf,
    pub home: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub projects: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub id: String,
    pub home: PathBuf,
    pub project: PathBuf,
    pub kind: String,
    pub backend: String,
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub single_checkout_override: Option<String>,
    pub single_checkout_record: Option<PathBuf>,
    pub single_checkout_base_head: Option<String>,
    pub single_checkout_base_branch: Option<String>,
}

fn descendant(parent: &Path, child: &Path) -> bool {
    parent != child && child.starts_with(parent)
}

fn registry_fields(path: &Path, id: &str) -> Result<BTreeMap<String, String>, String> {
    let text = fs::read_to_string(path).map_err(|error_value| error_value.to_string())?;
    let line = text
        .lines()
        .find(|line| {
            line.strip_prefix("- ")
                .is_some_and(|tail| tail.split_whitespace().next() == Some(id))
        })
        .ok_or_else(|| format!("no daemon registry entry for {id}"))?;
    let mut fields = BTreeMap::new();
    let Some(start) = line.find("(home: ") else {
        return Err("malformed daemon registry entry".to_owned());
    };
    let details = &line[start + 1..];
    let details = details
        .strip_suffix(')')
        .ok_or("malformed daemon registry entry")?;
    for field in details.split("; ") {
        if let Some((key, value)) = field.split_once(": ") {
            fields.insert(key.to_owned(), value.to_owned());
        }
    }
    Ok(fields)
}

pub fn parse(
    args: &[OsString],
    context: &Context,
    default_harness: &str,
) -> Result<Request, String> {
    let mut positional = Vec::new();
    let mut daemon = false;
    let mut scout = false;
    let mut backend = "tmux".to_owned();
    let mut harness = None;
    let mut model = "default".to_owned();
    let mut effort = "default".to_owned();
    let mut index = 0;
    while index < args.len() {
        let value = args[index]
            .to_str()
            .ok_or("spawn argument is not valid UTF-8")?;
        match value {
            "--daemon" => daemon = true,
            "--scout" => scout = true,
            "--harness" | "--model" | "--effort" | "--backend" => {
                let next = args
                    .get(index + 1)
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| format!("{value} requires a value"))?
                    .to_owned();
                match value {
                    "--harness" => harness = Some(next),
                    "--model" => model = next,
                    "--effort" => effort = next,
                    _ => backend = next,
                }
                index += 1;
            }
            value if value.starts_with("--") => {
                return Err(format!("unsupported native daemon spawn option: {value}"));
            }
            _ => positional.push(value.to_owned()),
        }
        index += 1;
    }
    let id = positional.first().ok_or("invalid spawn request")?.clone();
    TaskId::parse(&id).map_err(|_| "invalid spawn request")?;
    if !matches!(backend.as_str(), "tmux" | "herdr" | "cmux") {
        return Err(format!("unknown backend '{backend}'"));
    }
    if daemon && backend == "cmux" {
        return Err("backend=cmux does not support --daemon spawns yet".to_owned());
    }
    let fields = registry_fields(&context.data.join("daemons.md"), &id).unwrap_or_default();
    if !daemon {
        let project_arg = positional.get(1).ok_or("invalid spawn request")?;
        let project = if let Some(relative) = project_arg.strip_prefix("projects/") {
            context.projects.join(relative)
        } else {
            PathBuf::from(project_arg)
        };
        let project = fs::canonicalize(&project).map_err(|_| {
            format!(
                "no brief at {}",
                context.data.join(&id).join("brief.md").display()
            )
        })?;
        if !context.data.join(&id).join("brief.md").is_file() {
            return Err(format!(
                "no brief at {}",
                context.data.join(&id).join("brief.md").display()
            ));
        }
        if positional.len() > 2 {
            harness = positional.get(2).cloned();
        }
        return Ok(Request {
            id,
            home: context.home.clone(),
            project,
            kind: if scout { "scout" } else { "delivery" }.to_owned(),
            backend,
            harness: harness.unwrap_or_else(|| default_harness.to_owned()),
            model,
            effort,
            single_checkout_override: None,
            single_checkout_record: None,
            single_checkout_base_head: None,
            single_checkout_base_branch: None,
        });
    }
    let candidate = positional.get(1).map(PathBuf::from);
    let explicit_home = candidate.as_ref().filter(|path| path.is_dir()).cloned();
    if explicit_home.is_some() {
        if positional.len() > 2 {
            harness = positional.get(2).cloned();
        }
    } else if positional.len() > 1 {
        harness = positional.get(1).cloned();
    }
    let home = explicit_home
        .unwrap_or_else(|| PathBuf::from(fields.get("home").cloned().unwrap_or_default()));
    let home = resolved(&home);
    let active = resolved(&context.home);
    let root = resolved(&context.root);
    let reason = if home == Path::new("/") {
        Some("the filesystem root")
    } else if home == active {
        Some("the active Multplx home")
    } else if home == root {
        Some("the Multplx repo")
    } else if descendant(&active, &home) {
        Some("inside the active Multplx home")
    } else if descendant(&root, &home) {
        Some("inside the Multplx repo")
    } else if descendant(&home, &active) {
        Some("an ancestor of the active Multplx home")
    } else if descendant(&home, &root) {
        Some("an ancestor of the Multplx repo")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Err(format!(
            "daemon home cannot be {reason}: {}",
            home.display()
        ));
    }
    if !home.is_dir() {
        return Err(format!(
            "Multplx home does not exist or is not a directory: {}",
            home.display()
        ));
    }
    for name in ["data", "state", "config", "projects"] {
        let path = home.join(name);
        if path.exists() || fs::symlink_metadata(&path).is_ok() {
            let canonical = fs::canonicalize(&path).map_err(|_| {
                format!(
                    "daemon {name} directory must resolve inside the daemon home: {}",
                    path.display()
                )
            })?;
            if !descendant(&home, &canonical) {
                return Err(format!(
                    "daemon {name} directory must resolve inside the daemon home: {}",
                    path.display()
                ));
            }
        }
    }
    let marker = home.join(".mx-daemon-home");
    if !marker.is_file() {
        return Err(format!(
            "Multplx home {} is not a seeded daemon home",
            home.display()
        ));
    }
    let marker_id = fs::read_to_string(marker).unwrap_or_default();
    if marker_id.trim_end() != id {
        return Err(format!(
            "Multplx home {} is marked for daemon {}, expected {id}",
            home.display(),
            marker_id.trim_end()
        ));
    }
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
    if fields
        .get("home")
        .is_some_and(|value| resolved(Path::new(value)) != home)
    {
        return Err("daemon registry home does not match spawn target".to_owned());
    }
    Ok(Request {
        id,
        project: home.clone(),
        home,
        kind: "daemon".to_owned(),
        backend,
        harness: harness.unwrap_or_else(|| default_harness.to_owned()),
        model,
        effort,
        single_checkout_override: None,
        single_checkout_record: None,
        single_checkout_base_head: None,
        single_checkout_base_branch: None,
    })
}

pub fn publish_meta(context: &Context, request: &Request, endpoint: &str) -> Result<(), String> {
    publish_meta_for_worktree(context, request, endpoint, &request.project)
}

pub fn publish_meta_for_worktree(
    context: &Context,
    request: &Request,
    endpoint: &str,
    actor_worktree: &Path,
) -> Result<(), String> {
    let fields = registry_fields(&context.data.join("daemons.md"), &request.id).unwrap_or_default();
    let projects = fields.get("projects").cloned().unwrap_or_default();
    let worktree = if request.kind == "daemon" {
        &request.home
    } else {
        actor_worktree
    };
    let mode = if request.kind == "daemon" {
        "daemon"
    } else {
        "deep-review"
    };
    let mut text = format!(
        "window={endpoint}\nworktree={}\nproject={}\nharness={}\nkind={}\nmode={mode}\nyolo=off\nmodel={}\neffort={}\ntasktmp=/tmp/mx-{}\n",
        worktree.display(),
        request.project.display(),
        request.harness,
        request.kind,
        request.model,
        request.effort,
        request.id
    );
    if request.backend != "tmux" {
        text.push_str(&format!("backend={}\n", request.backend));
    }
    if request.kind == "daemon" {
        text.push_str(&format!(
            "home={}\nprojects={projects}\n",
            request.home.display()
        ));
    }
    if let (Some(request_id), Some(record), Some(head), Some(branch)) = (
        request.single_checkout_override.as_deref(),
        request.single_checkout_record.as_deref(),
        request.single_checkout_base_head.as_deref(),
        request.single_checkout_base_branch.as_deref(),
    ) {
        text.push_str(&format!(
            "single_checkout=yes\nsingle_checkout_override={request_id}\nsingle_checkout_record={}\nsingle_checkout_base_head={head}\nsingle_checkout_base_branch={branch}\n",
            record.display()
        ));
    }
    atomic_replace(
        context.state.join(format!("{}.meta", request.id)),
        text.as_bytes(),
        0o600,
    )
    .map_err(|error_value| error_value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(temp: &Path) -> Context {
        let base = fs::canonicalize(temp).expect("canonical tempdir");
        let value = Context {
            root: base.join("root"),
            home: base.join("home"),
            data: base.join("home/data"),
            state: base.join("home/state"),
            projects: base.join("home/projects"),
        };
        for path in [&value.root, &value.data, &value.state, &value.projects] {
            fs::create_dir_all(path).expect("directory");
        }
        value
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn delivery_and_daemon_parsing_cover_success_and_closed_refusals() {
        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        let project = context.projects.join("project");
        fs::create_dir(&project).expect("project");
        fs::create_dir_all(context.data.join("task")).expect("brief directory");
        fs::write(context.data.join("task/brief.md"), "brief\n").expect("brief");
        let delivery = parse(
            &args(&[
                "task",
                "projects/project",
                "--scout",
                "--harness",
                "pi",
                "--model",
                "m",
                "--effort",
                "high",
                "--backend",
                "herdr",
            ]),
            &context,
            "codex",
        )
        .expect("delivery");
        assert_eq!(
            (
                delivery.kind.as_str(),
                delivery.harness.as_str(),
                delivery.backend.as_str()
            ),
            ("scout", "pi", "herdr")
        );
        assert_eq!(
            delivery.project,
            fs::canonicalize(project).expect("project")
        );
        assert!(
            parse(&args(&["task", "/missing"]), &context, "codex")
                .expect_err("brief")
                .contains("no brief")
        );
        assert!(
            parse(
                &args(&["task", "/tmp", "--backend", "bad"]),
                &context,
                "codex"
            )
            .expect_err("backend")
            .contains("unknown backend")
        );
        assert!(
            parse(&args(&["task", "/tmp", "--unknown"]), &context, "codex")
                .expect_err("option")
                .contains("unsupported")
        );

        let daemon = temp.path().join("daemon");
        for name in ["bin", "data", "state", "config", "projects"] {
            fs::create_dir_all(daemon.join(name)).expect("daemon directory");
        }
        fs::write(daemon.join(".mx-daemon-home"), "daemon\n").expect("marker");
        fs::write(daemon.join("AGENTS.md"), "agents\n").expect("agents");
        fs::write(
            context.data.join("daemons.md"),
            format!(
                "- daemon - live (home: {}; projects: one,two)\n",
                daemon.display()
            ),
        )
        .expect("registry");
        let request = parse(
            &args(&[
                "daemon",
                daemon.to_str().unwrap(),
                "--daemon",
                "--backend",
                "herdr",
            ]),
            &context,
            "claude",
        )
        .expect("daemon");
        assert_eq!(
            (request.kind.as_str(), request.harness.as_str()),
            ("daemon", "claude")
        );
        publish_meta(&context, &request, "session:workspace:pane").expect("publish");
        let meta = fs::read_to_string(context.state.join("daemon.meta")).expect("meta");
        assert!(meta.contains("backend=herdr\n"));
        assert!(meta.contains("projects=one,two\n"));
        assert!(
            parse(
                &args(&["daemon", "--daemon", "--backend", "cmux"]),
                &context,
                "codex"
            )
            .expect_err("cmux")
            .contains("does not support")
        );
        fs::write(daemon.join(".mx-daemon-home"), "other\n").expect("marker");
        assert!(
            parse(
                &args(&["daemon", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("marker")
            .contains("marked for daemon")
        );
    }

    #[test]
    fn daemon_home_safety_matrix_and_single_checkout_metadata_are_closed() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        for path in [
            Path::new("/"),
            context.home.as_path(),
            context.root.as_path(),
        ] {
            assert!(
                parse(
                    &args(&["daemon", path.to_str().unwrap(), "--daemon"]),
                    &context,
                    "codex"
                )
                .is_err()
            );
        }
        let inside = context.home.join("inside");
        fs::create_dir(&inside).expect("inside");
        assert!(
            parse(
                &args(&["daemon", inside.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("inside")
            .contains("inside the active")
        );
        let daemon = fs::canonicalize(temp.path())
            .expect("canonical")
            .join("daemon-two");
        fs::create_dir(&daemon).expect("daemon");
        assert!(
            parse(
                &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("seed")
            .contains("not a seeded")
        );
        fs::write(daemon.join(".mx-daemon-home"), "two\n").expect("marker");
        assert!(
            parse(
                &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("agents")
            .contains("missing AGENTS")
        );
        fs::write(daemon.join("AGENTS.md"), "agents\n").expect("agents");
        assert!(
            parse(
                &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("bin")
            .contains("missing bin")
        );
        fs::create_dir(daemon.join("bin")).expect("bin");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).expect("outside");
        symlink(&outside, daemon.join("data")).expect("symlink");
        assert!(
            parse(
                &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
                &context,
                "codex"
            )
            .expect_err("containment")
            .contains("resolve inside")
        );
        fs::remove_file(daemon.join("data")).expect("remove");
        let mut request = parse(
            &args(&["two", daemon.to_str().unwrap(), "--daemon"]),
            &context,
            "codex",
        )
        .expect("daemon");
        request.single_checkout_override = Some("request".into());
        request.single_checkout_record = Some(context.state.join("record.json"));
        request.single_checkout_base_head = Some("head".into());
        request.single_checkout_base_branch = Some("main".into());
        request.kind = "delivery".into();
        request.backend = "tmux".into();
        publish_meta_for_worktree(
            &context,
            &request,
            "session:window",
            Path::new("/tmp/actor-worktree"),
        )
        .expect("publish");
        let meta = fs::read_to_string(context.state.join("two.meta")).expect("meta");
        assert!(meta.contains("single_checkout=yes\n"));
        assert!(meta.contains("worktree=/tmp/actor-worktree\n"));
        assert!(!meta.contains("backend=tmux"));
    }

    #[test]
    fn parser_and_publication_fault_matrix_rejects_malformed_inputs() {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let context = context(temp.path());
        assert!(parse(&[], &context, "codex").is_err());
        assert!(parse(&[OsString::from_vec(vec![0xff])], &context, "codex").is_err());
        for option in ["--harness", "--model", "--effort", "--backend"] {
            assert!(parse(&args(&["task", option]), &context, "codex").is_err());
        }
        assert!(parse(&args(&["task"]), &context, "codex").is_err());

        let registry = context.data.join("daemons.md");
        fs::write(&registry, "- malformed - live without details\n").expect("registry");
        assert!(registry_fields(&registry, "malformed").is_err());
        fs::write(&registry, "- malformed - live (home: /tmp\n").expect("registry");
        assert!(registry_fields(&registry, "malformed").is_err());

        let inside_root = context.root.join("inside");
        fs::create_dir(&inside_root).expect("inside root");
        assert!(
            parse(
                &args(&["daemon", inside_root.to_str().unwrap(), "--daemon"]),
                &context,
                "codex",
            )
            .expect_err("inside root")
            .contains("inside the Multplx repo")
        );
        let ancestor = fs::canonicalize(temp.path()).expect("ancestor");
        assert!(
            parse(
                &args(&["daemon", ancestor.to_str().unwrap(), "--daemon"]),
                &context,
                "codex",
            )
            .expect_err("ancestor")
            .contains("ancestor of the active")
        );

        let daemon = temp.path().join("valid-daemon");
        fs::create_dir_all(daemon.join("bin")).expect("bin");
        fs::write(daemon.join(".mx-daemon-home"), "daemon\n").expect("marker");
        fs::write(daemon.join("AGENTS.md"), "agents\n").expect("agents");
        fs::write(
            &registry,
            "- daemon - live (home: /different; harness: codex)\n",
        )
        .expect("registry");
        assert!(
            parse(
                &args(&[
                    "daemon",
                    daemon.to_str().unwrap(),
                    "explicit-harness",
                    "--daemon",
                ]),
                &context,
                "codex",
            )
            .expect_err("registry mismatch")
            .contains("registry home does not match")
        );

        let request = Request {
            id: "task".into(),
            home: context.home.clone(),
            project: context.root.clone(),
            kind: "delivery".into(),
            backend: "tmux".into(),
            harness: "codex".into(),
            model: "default".into(),
            effort: "default".into(),
            single_checkout_override: None,
            single_checkout_record: None,
            single_checkout_base_head: None,
            single_checkout_base_branch: None,
        };
        let mut invalid_context = context.clone();
        invalid_context.state = temp.path().join("state-file");
        fs::write(&invalid_context.state, "not a directory").expect("state file");
        assert!(publish_meta(&invalid_context, &request, "window").is_err());
    }
}

//! Delivery, scout, and daemon brief scaffolding.

use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use multplx_core::identifiers::TaskId;

use crate::project_registry::{DeliveryMode, resolve as resolve_project_mode};

pub const HELP: &str = r#"Scaffold a sub-agent assignment at data/<task-id>/brief.md in the active home.
Usage: mx brief <task-id> <repo-name|project-path> [--scout|--review]
                [--context-file PATH] [--herdr-lab] [--mode MODE] [--yolo on|off]
       mx brief <task-id> --daemon {<project>...|--no-projects} [--context-file PATH]
The bin/mx-brief.sh compatibility entry accepts the same arguments.
Default: implementation. --scout: research report. --review: review report.
--daemon scaffolds a persistent sub-orchestrator charter using the legacy home
interface; MX_DAEMON_CHARTER and MX_DAEMON_SCOPE supply its outcome and scope.
--no-projects deliberately leaves project selection unbound; bind a repository
before implementation. It is mutually exclusive with a project list.
Replace {TASK} with the accepted outcome, acceptance criteria and constraints.
--context-file copies UTF-8 handoff context verbatim, including original research
pointers, accepted revision, known task/attempt identity, checkout and base commit.
These are scaffold inputs, not validated runtime identity: Phase 02 owns binding.
Missing identities must remain explicitly unknown, never fabricated.
Project paths use the existing resolver; bare names refer to registered clones.
Keep repository instructions scoped to the selected task. No launch cwd or URL
is required by this scaffolder. Phase 11 owns discovery and local registration.
--mode and --yolo retain legacy resolution compatibility with spawn; they do not
request review tools or grant merge authority. Only local-only changes the output
destination. Deep-review and vplan require explicit task/workflow selection.
Phase 03 owns built-in worktree commands; Phase 05 owns named coordinator spawn.
The templates describe the lean contract; later phases replace runtime fences.
Do not deploy this partial redesign into operational homes.
--herdr-lab retains the isolated Herdr lifecycle helper contract for experiments.
Status uses report_status or the validated task-bound mx-report fallback.
Refuses to overwrite an existing brief.
"#;

#[derive(Debug)]
pub struct BriefError {
    pub message: String,
    pub code: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Delivery,
    Scout,
    Review,
    Daemon,
}

fn error(message: impl Into<String>) -> BriefError {
    BriefError {
        message: message.into(),
        code: 1,
    }
}

fn shell_quote(value: &Path) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "'\\''"))
}

fn status_contract(root: &Path, state: &Path, id: &str) -> String {
    format!(
        "Report status with `report_status` when available, otherwise:\n`{} --id {id} --state {{state}} --message \"{{one short line}}\"`\nStates: working, paused, needs-decision, blocked, done, failed, resolved.\nNever write to `{}` by hand.\nUse `paused: {{why}}` for a known external wait; `blocked` when the parent must act.\nWhen a decision is answered or a blocker clears, report `resolved` with the same `--key <slug>`.\nPreserve correlation tokens on replies; report meaningful outcomes and artifact pointers.\nA working event is progress, not task completion.",
        shell_quote(&root.join("bin/mx-report")),
        state.join(format!("{id}.status")).display()
    )
}

fn constraints() -> &'static str {
    "Delegate within the accepted scope using supported sessions or available native tools.\nFollow the selected workflow's stages, outputs and explicit user-interaction points.\nDeep-review and vplan run only when explicitly requested or clearly included in the selected workflow.\nOnly humans merge PRs; never merge, enable auto-merge, enqueue a merge or push the PR result to the remote target branch.\nKeep unresolved human scope decisions visible while independent work continues."
}

fn herdr_section(root: &Path, id: &str, enabled: bool) -> String {
    if !enabled {
        return "# Herdr lifecycle declaration - NOT ENABLED\n**HARD SAFETY GATE:** this scaffold cannot inspect the task text that replaces `{TASK}` later.\nIf the task will start, stop, delete, restart, profile, or otherwise drive Herdr lifecycle behavior, stop and regenerate the brief with `--herdr-lab` before dispatch.\nDo not add Herdr lifecycle commands to this unguarded brief by hand.".to_owned();
    }
    let helper = shell_quote(&root.join("bin/mx-herdr-lab.sh"));
    format!(
        "# Herdr isolation - HARD SAFETY CONTRACT\nThis brief was explicitly scaffolded with `--herdr-lab` because the task will drive Herdr lifecycle behavior.\nOn Herdr 0.7.3 the API socket is not relocatable by `HERDR_CONFIG_PATH`, `XDG_CONFIG_HOME`, or `HOME`.\nA named non-`default` session plus a trailing `--session <name>` on every call is the only viable local isolation.\n\n1. Set `HERDR_LAB_HELPER={helper}` and generate the session name with `HERDR_LAB_SESSION=$(\"$HERDR_LAB_HELPER\" name {id})`.\n   Install `trap '\"$HERDR_LAB_HELPER\" teardown \"$HERDR_LAB_SESSION\"' EXIT` before provisioning, then provision only with `\"$HERDR_LAB_HELPER\" provision \"$HERDR_LAB_SESSION\"`.\n2. Run every task-specific non-lifecycle Herdr command through `\"$HERDR_LAB_HELPER\" run \"$HERDR_LAB_SESSION\" <arguments...>`.\n   The helper appends the required trailing `--session \"$HERDR_LAB_SESSION\"`; `HERDR_SESSION` alone is never accepted as isolation.\n3. Teardown only through `\"$HERDR_LAB_HELPER\" teardown \"$HERDR_LAB_SESSION\"`.\n   It re-checks refuse-default immediately before stop and again immediately before delete, and fails closed on ambiguity.\n4. If an experiment requires a deliberate mid-run session stop, use only `\"$HERDR_LAB_HELPER\" stop \"$HERDR_LAB_SESSION\"`; it performs the same immediate refuse-default check.\n5. Forbidden commands: direct `herdr server stop`, every other server-global operation such as `herdr server live-handoff` or reload/update operations, direct `herdr session stop`, direct `herdr session delete`, and any Herdr call scoped only by ambient or inline `HERDR_SESSION`.\n6. The helper records the live default session before provisioning and verifies the identical system state after teardown.\n   A missing, stopped, or changed default session is a hard tripwire failure, never a cleanup warning to ignore.\n\nNever bypass the helper, even for a read-only lifecycle probe or cleanup after failure.\nThe maintainer system uses the running `default` session."
    )
}

fn report_assignment(
    root: &Path,
    data: &Path,
    state: &Path,
    id: &str,
    repo: &str,
    herdr: bool,
    review: bool,
) -> String {
    let role = if review { "reviewer" } else { "researcher" };
    format!(
        "You are a {role} sub-agent.\n\n# Task\n{{TASK}}\n\n# Setup\nProject/checkout reference: `{repo}`.\nUse the assigned isolated working location and this repository's applicable instructions.\nThis assignment produces a report; scratch changes are not a delivered implementation.\nPreserve useful artifacts outside disposable scratch state before cleanup.\n\n{}\n\n# Coordination\n{}\n{}\n\n# Definition of done\nWrite the report to `{}` with findings, original source/artifact pointers, relevant evidence, unresolved questions and limitations.\nFor review, identify the exact revision assessed; do not present historical evidence as current.\nReport the completed outcome through the validated status channel.\n",
        herdr_section(root, id, herdr),
        status_contract(root, state, id),
        constraints(),
        data.join(id).join("report.md").display(),
    )
}

fn daemon(root: &Path, state: &Path, id: &str, projects: &[String], no_projects: bool) -> String {
    let charter = env::var("MX_DAEMON_CHARTER").unwrap_or_else(|_| "{TASK}".to_owned());
    let scope = env::var("MX_DAEMON_SCOPE").unwrap_or_else(|_| charter.clone());
    let projects = if no_projects {
        "None. This is a project-less domain; bind an explicit repository before implementation."
            .to_owned()
    } else {
        projects
            .iter()
            .map(|project| format!("- {project}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "You are a persistent sub-orchestrator with one bounded assignment.\n\n# Charter\n{charter}\n\n# Routing scope\n{scope}\n\n# Project references\n{projects}\nProject references are non-exclusive; they do not claim unrelated tasks.\n\n# Coordination\nDelegate project implementation, code fixes and test-code changes to sub-agents.\nOwn synthesis, briefs, task coordination and communication within this scope.\nReconcile your home's recorded children and pending work on restart; an empty queue means idle, not invented work or retirement.\nParent route: task `{id}`, status owner `{}`; keep this separate from your own operational home.\nA marked request carries `corr=<id>`; include that exact token in your parent status reply.\nFor detailed outcomes, record an artifact in your home's data directory and report its pointer.\nDo not replace original child evidence with a summary alone.\n{}\n{}\n\n# Definition of done\nReport assigned outcomes, failures and unresolved human decisions through the parent channel.\nPersistence does not end when one task completes; retain child ownership and pending outcomes until reconciled or transferred.\nThis charter is scaffolding; A11 coordinator identity and runtime outcome relays are implemented in Phase 05.\n",
        state.display(),
        status_contract(root, state, id),
        constraints(),
    )
}

fn delivery(
    root: &Path,
    _data: &Path,
    state: &Path,
    id: &str,
    repo: &str,
    herdr: bool,
    selected_mode: DeliveryMode,
) -> String {
    let destination = if selected_mode == DeliveryMode::LocalOnly {
        "The destination is **local-only**: deliver the scoped local branch and evidence without a remote push or PR."
    } else {
        "You may commit, push your task branch, open or update its PR and make ordinary follow-up fixes within scope."
    };
    format!(
        "You are an implementer sub-agent.\n\n# Task\n{{TASK}}\n\n# Setup\nProject/checkout reference: `{repo}`.\nVerify `pwd -P` and `git rev-parse --show-toplevel` identify the assigned isolated worktree, not the primary checkout, before editing or committing.\nIf isolation or the recorded starting revision cannot be established, retain the work and report blocked.\nUse task branch `mx/{id}` and the selected repository's applicable instructions.\n\n{}\n\n# Coordination\n{}\n{}\n\n# Definition of done\n{destination}\nMeet the accepted criteria and report the actual commit, exact checks and results, limitations, original artifact pointers and PR reference where applicable.\nDistinguish implementation complete, checks passing, PR ready and human merged.\n",
        herdr_section(root, id, herdr),
        status_contract(root, state, id),
        constraints(),
    )
}

pub fn run(
    args: &[OsString],
    root: &Path,
    home: &Path,
    data: &Path,
    state: &Path,
) -> Result<String, BriefError> {
    let mut selected_mode = None;
    let mut selected_yolo = None;
    let mut kind = Kind::Delivery;
    let mut herdr = false;
    let mut no_projects = false;
    let mut positional = Vec::new();
    let mut context_file = None;
    let mut arguments = args.iter();
    while let Some(arg) = arguments.next() {
        match arg.to_string_lossy().as_ref() {
            "--mode" => {
                selected_mode = Some(
                    DeliveryMode::parse(
                        arguments
                            .next()
                            .and_then(|value| value.to_str())
                            .ok_or_else(|| error("--mode requires a value"))?,
                    )
                    .ok_or_else(|| error("invalid delivery mode"))?,
                )
            }
            "--yolo" => {
                selected_yolo = Some(match arguments.next().and_then(|value| value.to_str()) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err(error("--yolo requires on or off")),
                })
            }
            "--context-file" => {
                if context_file.is_some() {
                    return Err(error("--context-file may be supplied only once"));
                }
                context_file = Some(
                    arguments
                        .next()
                        .ok_or_else(|| error("--context-file requires a path"))?
                        .clone(),
                );
            }
            "--scout" | "--review" | "--daemon" => {
                if kind != Kind::Delivery {
                    return Err(error("choose only one assignment flag"));
                }
                kind = match arg.to_str() {
                    Some("--scout") => Kind::Scout,
                    Some("--review") => Kind::Review,
                    _ => Kind::Daemon,
                };
            }
            "--herdr-lab" => herdr = true,
            "--no-projects" => no_projects = true,
            value if value.starts_with("--") => {
                return Err(error(format!("unknown option: {value}")));
            }
            value => positional.push(value.to_owned()),
        }
    }
    let id = positional.first().ok_or_else(|| error("missing task id"))?;
    TaskId::parse(id).map_err(|_| error(format!("invalid task id: {id}")))?;
    if kind == Kind::Daemon && herdr {
        return Err(error(
            "--herdr-lab applies only to actor delivery or scout briefs",
        ));
    }
    if no_projects && kind != Kind::Daemon {
        return Err(error("--no-projects applies only to --daemon charters"));
    }
    let projects = positional.get(1..).unwrap_or_default();
    if kind == Kind::Daemon {
        if no_projects && !projects.is_empty() {
            return Err(error(
                "--no-projects cannot be combined with a project list",
            ));
        }
        if !no_projects && projects.is_empty() {
            return Err(error(
                "--daemon requires at least one project, or --no-projects for a project-less home",
            ));
        }
    } else if positional.get(1).is_none() {
        return Err(error("missing repo name"));
    }
    if kind == Kind::Daemon && (selected_mode.is_some() || selected_yolo.is_some()) {
        return Err(error("daemon briefs do not accept task mode or yolo"));
    }
    if kind != Kind::Daemon && positional.len() != 2 {
        return Err(error(
            "ordinary briefs require exactly one project reference",
        ));
    }
    let context = context_file
        .map(|path| {
            let text = fs::read_to_string(&path)
                .map_err(|io| error(format!("cannot read context file: {io}")))?;
            if text.trim().is_empty() {
                return Err(error("context file must not be empty"));
            }
            Ok(text)
        })
        .transpose()?;
    let repo = positional.get(1).map(String::as_str).unwrap_or("");
    let resolution = if repo.contains('/') || repo == "." || repo == ".." {
        let projects = env::var_os("MX_PROJECTS_OVERRIDE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join("projects"));
        let project = repo
            .strip_prefix("projects/")
            .map(|relative| projects.join(relative))
            .unwrap_or_else(|| repo.into());
        let project = fs::canonicalize(project).map_err(|io| error(io.to_string()))?;
        crate::project_registry::resolve_path(&data.join("projects.md"), &projects, root, &project)
    } else {
        resolve_project_mode(&data.join("projects.md"), repo)
    };
    let (mode, yolo) = if kind == Kind::Daemon {
        (DeliveryMode::DeepReview, false)
    } else {
        let (mode, yolo) =
            super::spawn::task_authority(state, id, &resolution, selected_mode, selected_yolo)
                .map_err(error)?;
        (
            DeliveryMode::parse(&mode).expect("validated task mode"),
            yolo,
        )
    };
    let path = data.join(id).join("brief.md");
    fs::create_dir_all(path.parent().expect("brief parent"))
        .map_err(|error_value| error(error_value.to_string()))?;
    let mut body = match kind {
        Kind::Daemon => daemon(root, state, id, projects, no_projects),
        Kind::Scout | Kind::Review => report_assignment(
            root,
            data,
            state,
            id,
            &positional[1],
            herdr,
            kind == Kind::Review,
        ),
        Kind::Delivery => delivery(root, data, state, id, &positional[1], herdr, mode),
    };
    if let Some(context) = context {
        body.push_str("\n# Accepted handoff context\n");
        body.push_str(&context);
        if !body.ends_with('\n') {
            body.push('\n');
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|io| {
            if io.kind() == std::io::ErrorKind::AlreadyExists {
                error(format!("{} already exists", path.display()))
            } else {
                error(io.to_string())
            }
        })?;
    file.write_all(body.as_bytes())
        .map_err(|io| error(io.to_string()))?;
    Ok(match kind {
        Kind::Daemon if env::var("MX_DAEMON_CHARTER").is_ok() => {
            format!("scaffolded: {} (daemon charter)", path.display())
        }
        Kind::Daemon => format!(
            "scaffolded: {} (daemon charter; replace {{TASK}})",
            path.display()
        ),
        Kind::Review => format!("scaffolded: {} (review; replace {{TASK}})", path.display()),
        Kind::Scout => format!("scaffolded: {} (scout; replace {{TASK}})", path.display()),
        Kind::Delivery => format!(
            "scaffolded: {} (delivery, mode={}, yolo={}; replace {{TASK}})",
            path.display(),
            mode.as_str(),
            if yolo { "on" } else { "off" }
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn scaffolds_every_brief_kind_and_delivery_mode() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("root");
        let home = temp.path().join("home");
        let data = home.join("data");
        let state = home.join("state");
        fs::create_dir_all(root.join("bin")).expect("root");
        fs::create_dir_all(&data).expect("data");
        fs::create_dir_all(&state).expect("state");
        fs::write(
            data.join("projects.md"),
            "- direct [direct-PR] - direct\n- local [local-only] - local\n",
        )
        .expect("registry");

        let direct =
            run(&args(&["deliver", "direct"]), &root, &home, &data, &state).expect("direct brief");
        assert!(direct.contains("mode=direct-PR"));
        let direct_body = fs::read_to_string(data.join("deliver/brief.md")).expect("brief");
        assert!(direct_body.contains("You may commit, push your task branch"));
        assert!(direct_body.contains("Herdr lifecycle declaration - NOT ENABLED"));

        run(
            &args(&["local-task", "local", "--herdr-lab"]),
            &root,
            &home,
            &data,
            &state,
        )
        .expect("local brief");
        let local = fs::read_to_string(data.join("local-task/brief.md")).expect("local");
        assert!(local.contains("local-only"));
        assert!(local.contains("Herdr isolation - HARD SAFETY CONTRACT"));

        run(
            &args(&["scout", "direct", "--scout", "--herdr-lab"]),
            &root,
            &home,
            &data,
            &state,
        )
        .expect("scout brief");
        let scout = fs::read_to_string(data.join("scout/brief.md")).expect("scout");
        assert!(scout.contains("researcher sub-agent"));
        assert!(scout.contains("report.md"));

        let daemon = run(
            &args(&["daemon", "--daemon", "direct", "local"]),
            &root,
            &home,
            &data,
            &state,
        )
        .expect("daemon brief");
        assert!(daemon.contains("replace {TASK}"));
        let daemon_body = fs::read_to_string(data.join("daemon/brief.md")).expect("daemon");
        assert!(daemon_body.contains("- direct\n- local"));

        run(
            &args(&["no-projects", "--daemon", "--no-projects"]),
            &root,
            &home,
            &data,
            &state,
        )
        .expect("project-less daemon");
        assert!(
            fs::read_to_string(data.join("no-projects/brief.md"))
                .expect("project-less")
                .contains("project-less domain")
        );
    }

    #[test]
    fn explicit_project_identity_matches_spawn_with_same_named_clone() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Multplx");
        let home = temp.path().join("home");
        let data = home.join("data");
        let state = home.join("state");
        let projects = home.join("projects");
        let clone = projects.join("Multplx");
        for path in [&root, &data, &state, &clone] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            data.join("projects.md"),
            "- Multplx [local-only +yolo] - clone\n",
        )
        .unwrap();
        let context = super::super::spawn::Context {
            root: root.clone(),
            home: home.clone(),
            data: data.clone(),
            state: state.clone(),
            projects,
        };
        for (id, reference, project, expected) in [
            ("self", root.to_str().unwrap(), &root, "deep-review"),
            ("clone-path", clone.to_str().unwrap(), &clone, "local-only"),
            ("clone-name", "Multplx", &clone, "local-only"),
        ] {
            let output = run(&args(&[id, reference]), &root, &home, &data, &state).unwrap();
            let launch = super::super::spawn::parse(
                &args(&[id, project.to_str().unwrap()]),
                &context,
                "codex",
            )
            .unwrap();
            assert_eq!(launch.mode, expected);
            assert!(output.contains(&format!("mode={}", launch.mode)));
            assert!(output.contains(if launch.yolo { "yolo=on" } else { "yolo=off" }));
            let body = fs::read_to_string(data.join(id).join("brief.md")).unwrap();
            assert!(body.contains(if expected == "deep-review" {
                "open or update its PR"
            } else {
                "**local-only**"
            }));
        }
    }

    #[test]
    fn refuses_invalid_or_ambiguous_brief_requests() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data = temp.path().join("data");
        let state = temp.path().join("state");
        for values in [
            vec![],
            vec!["bad/id", "repo"],
            vec!["task"],
            vec!["task", "repo", "--no-projects"],
            vec!["daemon", "--daemon"],
            vec!["daemon", "--daemon", "--herdr-lab", "repo"],
            vec!["daemon", "--daemon", "--no-projects", "repo"],
        ] {
            assert!(run(&args(&values), temp.path(), temp.path(), &data, &state).is_err());
        }
        run(
            &args(&["once", "repo"]),
            temp.path(),
            temp.path(),
            &data,
            &state,
        )
        .expect("first scaffold");
        let error = run(
            &args(&["once", "repo"]),
            temp.path(),
            temp.path(),
            &data,
            &state,
        )
        .expect_err("must not overwrite");
        assert!(error.message.contains("already exists"));
    }
}

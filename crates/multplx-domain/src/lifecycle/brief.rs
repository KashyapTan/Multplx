//! Delivery, scout, and daemon brief scaffolding.

use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use multplx_core::identifiers::TaskId;

use crate::project_registry::{DeliveryMode, resolve as resolve_project_mode};

pub const HELP: &str = r#"Scaffold an actor brief or persistent daemon charter at
data/<task-id>/brief.md under the active Multplx home.
For ordinary tasks, the standard Setup/Rules/Definition-of-done contract is
filled in. Multplx then replaces the {TASK} placeholder with the task
description, acceptance criteria, and context, and may adjust other sections
when the task genuinely deviates (e.g. working an existing external PR instead
of creating a new one).
Usage: mx-brief.sh <task-id> <repo-name> [--scout] [--herdr-lab]
       mx-brief.sh <task-id> --daemon {<project>...|--no-projects}
  --scout writes the scout contract instead: the deliverable is a report at
  data/<task-id>/report.md (no branch, no push, no PR) and the worktree is scratch.
  --daemon writes a persistent daemon charter. The project list
  is cloned into the daemon home, while the natural-language scope
  tells the main broker when to route work there; routine churn stays in its own home;
  maintainer-relevant escalations and marked from-broker replies append to this
  home's status file.
  --no-projects writes a project-less charter for a domain whose subject is the
  Multplx repo itself (its home is a broker worktree, its actors take pooled
  worktrees of the same repo). It is mutually exclusive with a project list, and
  omitting both still fails loudly so an accidental omission is never silent.
  Set MX_DAEMON_CHARTER='<charter>' to fill the charter text.
  Set MX_DAEMON_SCOPE='<scope>' to write a routing scope distinct from the charter text.
  --herdr-lab is mandatory when the task will issue Herdr lifecycle commands.
  It adds the hard isolation contract backed by bin/mx-herdr-lab.sh.
  The flag must be explicit because {TASK} is filled after scaffolding and the
  caller-supplied repo string cannot reliably identify this repo. Briefs made
  without it carry a loud declaration so an omitted contract cannot be silent.
For delivery tasks, the definition of done is shaped by the project's delivery mode
(data/projects.md via mx-project-mode.sh; see the project-management skill
and AGENTS.md task lifecycle):
  deep-review  implement -> actor-driven local validation -> delivery service -> PR -> maintainer merge (default)
  direct-PR    implement -> approved delivery service -> PR (no full pipeline) -> maintainer merge
  local-only   implement on branch, stop and report "ready in branch" (no push/PR);
               maintainer approves, broker merges to local main
Delivery briefs begin with a worktree-isolation assertion before the branch step.
Scout tasks ignore mode - their deliverable is a report, not a merge.
Every scaffold's status protocol uses the closed actor-writable vocabulary
owned by bin/mx-report and distinguishes "paused" from "blocked": pause for a
known external wait expected to clear on its own, blocked when broker must act.
DELIVERY TASK include a project-memory section so durable project-intrinsic
learnings can be committed to AGENTS.md through the project's delivery path;
it carries the AGENTS.md authoring bar (widely useful knowledge only, pointers
over copied detail) and has the actor add the mx-ensure-agents-md.sh
self-governance section when a touched project AGENTS.md lacks it.
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
        "Report status with the `report_status` tool when available, otherwise:\n   `{} --id {id} --state {{state}} --message \"{{one short line}}\"`\n   States: working, paused, needs-decision, blocked, done, failed, resolved.\n   `mx-report` rejects anything else and prints the valid options; fix the call and retry.\n   Never write to `{}` by hand.\n   Use `paused: {{why}}` - distinct from `blocked:` - ONLY when deliberately idling on a known external wait expected to clear on its own; use `blocked:` when stuck and broker must act.",
        root.join("bin/mx-report").display(),
        shell_quote(&state.join(format!("{id}.status")))
    )
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

fn scout(root: &Path, data: &Path, state: &Path, id: &str, repo: &str, herdr: bool) -> String {
    format!(
        "You are an actor: an autonomous agent coordinated through the broker. Work independently; do not wait for a human.\n\n# Task\n{{TASK}}\n\n{}\n\n# Setup\nYou are in a disposable git worktree of {repo}, at a detached HEAD on a clean default branch.\nThis is a SCOUT task: the deliverable is a written report, not a PR.\nThe worktree is your laboratory - install, run, edit, and make scratch commits freely; all of it is discarded at teardown.\nThe report is the only thing that survives, so anything worth keeping must be in it.\n\n# Rules\n1. Never push to any remote and never open a PR.\n2. Stay inside this worktree; the only file you may write outside it is the report below. The validated status writer owns status-file writes.\n3. Use official gh for GitHub operations and a first-class browser tool only when browser work is required.\n4. {}\n   Each report wakes broker, so report sparingly: only phase changes a supervisor would act on and the needs-decision/blocked/paused/done/failed states. No step-by-step FYI progress lines; broker reads your pane for that.\n5. If you hit the same obstacle twice, report `blocked` and stop; broker will help.\n6. If a decision belongs to a human (product choices, destructive actions), report `needs-decision` and stop. Multplx will reply with the decision.\n   When broker replies or a blocker clears and you resume, report `resolved` with the same `--key <slug>` if you opened one, so the decision or blocker is durably closed and does not keep resurfacing.\n7. Never invoke Multplx lifecycle or credentialed delivery commands from this worker.\n\n# Definition of done\nWrite your findings to `{}`.\nThe report must stand alone: what you did, what you found, the evidence (commands run, output, file:line references), and what you recommend.\nBefore reporting done, read and follow `{}` and pass its shared completion gate for the report and any visual review.\nWhen the report is complete, report `done` through rule 4 and stop.\nIf your findings reveal work that should be delivered, say so in the report; the broker may promote this task in place.\n",
        herdr_section(root, id, herdr),
        status_contract(root, state, id),
        data.join(id).join("report.md").display(),
        root.join(".agents/skills/decision-hold-lifecycle/SKILL.md")
            .display()
    )
}

fn daemon(root: &Path, state: &Path, id: &str, projects: &[String], no_projects: bool) -> String {
    let charter = env::var("MX_DAEMON_CHARTER").unwrap_or_else(|_| "{TASK}".to_owned());
    let scope = env::var("MX_DAEMON_SCOPE").unwrap_or_else(|_| charter.clone());
    let (project_body, note) = if no_projects {
        ("None. This is a project-less domain: its subject is the Multplx repo this home lives in, so it needs no separate clones under `projects/`; its actors take pooled worktrees of that Multplx repo.".to_owned(), "This domain has no separate project clones: its subject is the Multplx repo this home lives in, and its actors take pooled worktrees of that repo.")
    } else {
        (
            projects
                .iter()
                .map(|project| format!("- {project}"))
                .collect::<Vec<_>>()
                .join("\n"),
            "The projects above are local clones for work you coordinate; they are not an exclusive ownership claim.",
        )
    };
    format!(
        "You are a persistent daemon coordinated through the main broker. Work independently; do not wait for a human.\n\n# Charter\n{charter}\n\n# Routing scope\n{scope}\n\n# Project clones\n{project_body}\n\n# Operating model\nYou are in an isolated Multplx home. The local `AGENTS.md` is your job description, and your local `data/`, `state/`, `config/`, and `projects/` dirs are yours to operate.\n{note}\nDelegate project work to your own actors with the normal broker lifecycle: brief, spawn, status, watcher, steer, teardown, and recovery.\nDo not invent a second delegation system.\nYou do not generate your own work.\nAct only on tasks the main broker routes to you.\nNever start a survey, audit, or self-directed work on your own initiative.\n\n# Requests from the main broker\nA request relayed by the main broker carries an untypable marker and a privacy-safe `corr=<id>` token; include that exact token in your parent status reply.\nFor a terse result, a status line is the whole answer.\nFor a detailed answer, write it to a doc under your home's `data/` and report a status that points to that doc.\nBefore treating an investigation or visual review as complete, load `decision-hold-lifecycle` and pass its shared completion gate.\nA message with NO marker is the maintainer typing directly into your pane; stay conversational and do not force it onto the status path.\n\n# Escalation to main broker\nHandle routine work yourself.\nReport only true maintainer-relevant outcomes or a declared external wait with the `report_status` tool when available, otherwise:\n   `{}`\nStates: working, paused, needs-decision, blocked, done, failed, resolved.\n`mx-report` rejects anything else and prints the valid options; fix the call and retry.\nNever write to `{}` by hand.\nUse `paused: {{why}}` only for a known external wait; use `blocked:` when broker must act.\nUse this only for material phase changes, a maintainer decision, a real blocker, a failure, or work ready for review.\nA marked request requires one correlated answer after the work; it does not require a separate receipt or start acknowledgement.\nNever report `working` merely to acknowledge receipt or announce that a marked request has started.\nWhen a routed-work phase has a supervisor-actionable material change worth reporting under the rule above, give that reported phase a stable key.\nIf its first reportable event is `working [key=<work-slug>]: {{material phase}}`, use the same key on its later `paused`, `done`, `failed`, `needs-decision`, or `blocked` event so the earlier working phase is superseded.\nWhen a keyed phase ends without another reportable state, report `resolved` with the same `--key <work-slug>`.\nWhen a decision is answered or a blocker clears, report `resolved` with the same key.\n\n# Definition of done\nYou are persistent by default. Do not exit just because your queue is empty.\nOn startup and restart, run normal broker bootstrap and recovery through `bin/mx-session-start.sh` for your own home, but only to RECONCILE work that is already yours: in-flight actors, tracked backlog items, and durable watches recorded in this home.\nWhen you have no assigned or in-flight work after that reconciliation, go idle and wait silently for the main broker to route you a task.\nAn empty queue is a healthy resting state, not a cue to invent work: never spawn a survey, audit, or any self-directed \"find work\" task on your own initiative.\nIf this charter cannot be carried out, report `blocked` or `failed` through the validated status path and stop.\n",
        root.join("bin/mx-report").display().to_string()
            + &format!(
                " --id {id} --state {{state}} --message \"{{one short line including corr=<id>}}\""
            ),
        shell_quote(&state.join(format!("{id}.status")))
    )
}

fn delivery(root: &Path, data: &Path, state: &Path, id: &str, repo: &str, herdr: bool) -> String {
    let mode = resolve_project_mode(&data.join("projects.md"), repo).mode;
    let (rule, done) = match mode {
        DeliveryMode::DirectPr => (
            format!(
                "1. Never push to any remote, open a PR, or merge a PR. Commit only on your local `mx/{id}` branch; the credentialed delivery service owns every remote write."
            ),
            format!(
                "# Definition of done\nThis project delivers **direct-PR** without the full validation pipeline, but remote delivery is still separate from agent work.\nThe task is complete only when the worktree is clean and the implementation is committed on your local branch `mx/{id}`.\nReport `done` with `ready for delivery at {{full commit SHA}}` through the validated status path and stop.\nDo not push, open a PR, or merge.\nThe configured approval authority accepts the local commit, then the non-agent delivery service pushes exactly that approved SHA and opens the PR."
            ),
        ),
        DeliveryMode::LocalOnly => (
            format!(
                "1. Never push to any remote and never open a PR. Work only on your `mx/{id}` branch; broker handles the merge into local `main`."
            ),
            format!(
                "# Definition of done\nThis project delivers **local-only**: no remote, no PR, no pipeline.\nThe task is complete only when committed on your branch `mx/{id}`. Do NOT push, do NOT open a PR, do NOT merge.\nKeep your branch a clean fast-forward onto the current default branch.\nWhen implemented and committed, report `done` with `ready in branch mx/{id}` and stop.\nThe configured merge authority approves the ready branch, then broker merges it into local `main` through the guarded fast-forward path."
            ),
        ),
        DeliveryMode::DeepReview => (
            format!(
                "1. Never push to any remote, open a PR, or merge a PR. Commit only on your local `mx/{id}` branch; the validation gate and credentialed delivery service own the handoff."
            ),
            format!(
                "# Definition of done\nAfter implementation is clean and committed on your local branch `mx/{id}`, you must drive its local validation:\n`{} {id} --intent-file {}`\nThe gate owns review, focused testing, documentation, lint, and its own fix commits.\nAsk-user findings are never yours to answer.\nIf the gate parks on one, preserve its emitted decision request and stop while Multplx applies the authority contract in its `AGENTS.md`.\nDo not silently bypass broker's authority check and any required maintainer escalation.\nAfter Multplx supplies the accepted answer, run the respond command and rerun the gate.\nThe task is complete only when deep-review passes, the worktree is clean, and the gate writes the pending exact-SHA delivery record.\nDo not push, open a PR, merge, invoke credentialed delivery, or synthesize gate state.\nThe non-agent delivery service separately verifies approval and pushes exactly the validated SHA.",
                root.join("bin/mx-deep-review.sh").display(),
                data.join(id).join("brief.md").display()
            ),
        ),
    };
    format!(
        "You are an actor: an autonomous agent coordinated through the broker. Work independently; do not wait for a human.\n\n# Task\n{{TASK}}\n\n{}\n\n# Setup\nYou are in a disposable git worktree of {repo}, at a detached HEAD on a clean default branch.\n\n**Verify isolation before anything else.** Run `pwd -P` and `git rev-parse --show-toplevel`; both must resolve to the disposable task worktree you were launched in, such as a treehouse pool path, not the primary checkout broker operates from.\nThe path check is authoritative: `git rev-parse --git-dir` and `git rev-parse --git-common-dir` can help inspect the repo, but they do not prove you are outside the primary checkout.\nIf the top-level path is the primary checkout or not the worktree you were launched in, STOP - do not branch or commit here - report `blocked` through the validated status path and stop.\n\n1. First action: create your branch: `git checkout -b mx/{id}`\n\n# Rules\n{rule}\n2. Stay inside this worktree; modify nothing outside it.\n3. Use official gh only for read-only GitHub operations and a first-class browser tool only when browser work is required.\n4. {}\n   Each report wakes broker, so report sparingly: only phase changes a supervisor would act on and terminal or waiting states.\n   A mid-task `working:` line (including setup complete) is nonterminal: do not end the turn after it; continue the same stage until a defined `done:` gate under Definition of done.\n5. If you hit the same obstacle twice, report `blocked` and stop.\n6. If a decision belongs above the implementation worker, report `needs-decision` and stop. When broker replies or a blocker clears, report `resolved` with the same key.\n7. Never invoke Multplx lifecycle or credentialed delivery commands from this worker.\n\n# Project memory\nIf `AGENTS.md` or `CLAUDE.md` already exists, or if this task produced durable project-intrinsic knowledge, run `{}` in the worktree.\nRecord only project knowledge useful to almost every future session.\nFor anything the codebase already shows, prefer a pointer to the authoritative file, command, or doc over copying the detail.\nIf you touch a project `AGENTS.md` that lacks `## Maintaining this file`, add that short self-governance section from `{}` in the same pass.\nKeep it proportionate: skip `AGENTS.md` edits for trivial tasks that produced no durable project knowledge.\n\n{done}\n",
        herdr_section(root, id, herdr),
        status_contract(root, state, id),
        root.join("bin/mx-ensure-agents-md.sh .").display(),
        root.join("bin/mx-ensure-agents-md.sh").display(),
    )
}

pub fn run(
    args: &[OsString],
    root: &Path,
    _home: &Path,
    data: &Path,
    state: &Path,
) -> Result<String, BriefError> {
    let mut kind = Kind::Delivery;
    let mut herdr = false;
    let mut no_projects = false;
    let mut positional = Vec::new();
    for arg in args {
        match arg.to_string_lossy().as_ref() {
            "--scout" => kind = Kind::Scout,
            "--daemon" => kind = Kind::Daemon,
            "--herdr-lab" => herdr = true,
            "--no-projects" => no_projects = true,
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
    let path = data.join(id).join("brief.md");
    fs::create_dir_all(path.parent().expect("brief parent"))
        .map_err(|error_value| error(error_value.to_string()))?;
    let body = match kind {
        Kind::Daemon => daemon(root, state, id, projects, no_projects),
        Kind::Scout => scout(root, data, state, id, &positional[1], herdr),
        Kind::Delivery => delivery(root, data, state, id, &positional[1], herdr),
    };
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
        Kind::Scout => format!("scaffolded: {} (scout; replace {{TASK}})", path.display()),
        Kind::Delivery => format!(
            "scaffolded: {} (delivery, mode={}; replace {{TASK}})",
            path.display(),
            resolve_project_mode(&data.join("projects.md"), &positional[1])
                .mode
                .as_str()
        ),
    })
}

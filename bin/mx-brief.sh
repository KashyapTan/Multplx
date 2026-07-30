#!/usr/bin/env bash
# Scaffold an actor brief or persistent daemon charter at
# data/<task-id>/brief.md under the active Multplx home.
# For ordinary tasks, the standard Setup/Rules/Definition-of-done contract is
# filled in. Multplx then replaces the {TASK} placeholder with the task
# description, acceptance criteria, and context, and may adjust other sections
# when the task genuinely deviates (e.g. working an existing external PR instead
# of creating a new one).
# Usage: mx-brief.sh <task-id> <repo-name> [--scout] [--herdr-lab]
#        mx-brief.sh <task-id> --daemon {<project>...|--no-projects}
#   --scout writes the scout contract instead: the deliverable is a report at
#   data/<task-id>/report.md (no branch, no push, no PR) and the worktree is scratch.
#   --daemon writes a persistent daemon charter. The project list
#   is cloned into the daemon home, while the natural-language scope
#   tells the main broker when to route work there; routine churn stays in its own home;
#   maintainer-relevant escalations and marked from-broker replies append to this
#   home's status file.
#   --no-projects writes a project-less charter for a domain whose subject is the
#   Multplx repo itself (its home is a broker worktree, its actors take pooled
#   worktrees of the same repo). It is mutually exclusive with a project list, and
#   omitting both still fails loudly so an accidental omission is never silent.
#   Set MX_DAEMON_CHARTER='<charter>' to fill the charter text.
#   Set MX_DAEMON_SCOPE='<scope>' to write a routing scope distinct from the charter text.
#   --herdr-lab is mandatory when the task will issue Herdr lifecycle commands.
#   It adds the hard isolation contract backed by bin/mx-herdr-lab.sh.
#   The flag must be explicit because {TASK} is filled after scaffolding and the
#   caller-supplied repo string cannot reliably identify this repo. Briefs made
#   without it carry a loud declaration so an omitted contract cannot be silent.
# For delivery tasks, the definition of done is shaped by the project's delivery mode
# (data/projects.md via mx-project-mode.sh; see the project-management skill
# and AGENTS.md task lifecycle):
#   deep-review  implement -> actor-driven local validation -> delivery service -> PR -> maintainer merge (default)
#   direct-PR    implement -> approved delivery service -> PR (no full pipeline) -> maintainer merge
#   local-only   implement on branch, stop and report "ready in branch" (no push/PR);
#                maintainer approves, broker merges to local main
# Delivery briefs begin with a worktree-isolation assertion before the branch step.
# Scout tasks ignore mode - their deliverable is a report, not a merge.
# Every scaffold's status protocol uses the closed actor-writable vocabulary
# owned by bin/mx-report and distinguishes "paused" from "blocked": pause for a
# known external wait expected to clear on its own, blocked when broker must act.
# DELIVERY TASK include a project-memory section so durable project-intrinsic
# learnings can be committed to AGENTS.md through the project's delivery path;
# it carries the AGENTS.md authoring bar (widely useful knowledge only, pointers
# over copied detail) and has the actor add the mx-ensure-agents-md.sh
# self-governance section when a touched project AGENTS.md lacks it.
# Refuses to overwrite an existing brief.
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  awk '
    NR == 1 { next }
    /^#/ { sub(/^# ?/, ""); print; next }
    { exit }
  ' "$0"
}

case "${1:-}" in
  -h|--help) usage; exit 0 ;;
esac

# shellcheck source=bin/mx-marker-lib.sh
. "$SCRIPT_DIR/mx-marker-lib.sh"
MX_ROOT="${MX_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/.." && pwd)}"
MX_HOME="${MX_HOME:-${MX_ROOT_OVERRIDE:-$MX_ROOT}}"
DATA="${MX_DATA_OVERRIDE:-$MX_HOME/data}"
STATE="${MX_STATE_OVERRIDE:-$MX_HOME/state}"
KIND=delivery
HERDR_LAB=0
NO_PROJECTS=0
POS=()
for a in "$@"; do
  case "$a" in
    --scout) KIND=scout ;;
    --daemon) KIND=daemon ;;
    --herdr-lab) HERDR_LAB=1 ;;
    --no-projects) NO_PROJECTS=1 ;;
    *) POS+=("$a") ;;
  esac
done
ID=${POS[0]}

if [ "$KIND" = daemon ] && [ "$HERDR_LAB" -eq 1 ]; then
  echo "error: --herdr-lab applies only to actor delivery or scout briefs" >&2
  exit 1
fi

if [ "$NO_PROJECTS" -eq 1 ] && [ "$KIND" != daemon ]; then
  echo "error: --no-projects applies only to --daemon charters" >&2
  exit 1
fi

BRIEF="$DATA/$ID/brief.md"
[ -e "$BRIEF" ] && { echo "error: $BRIEF already exists" >&2; exit 1; }
mkdir -p "$DATA/$ID"

shell_quote() {
  printf "'"
  printf '%s' "$1" | sed "s/'/'\\\\''/g"
  printf "'"
}

STATUS_FILE=$(shell_quote "$STATE/$ID.status")

if [ "$KIND" = daemon ]; then
DAEMON_PROJECTS=""
idx=1
while [ "$idx" -lt "${#POS[@]}" ]; do
  DAEMON_PROJECTS="${DAEMON_PROJECTS}${DAEMON_PROJECTS:+ }${POS[$idx]}"
  idx=$((idx + 1))
done
if [ "$NO_PROJECTS" -eq 1 ]; then
  [ -z "$DAEMON_PROJECTS" ] || { echo "error: --no-projects cannot be combined with a project list" >&2; exit 1; }
else
  [ -n "$DAEMON_PROJECTS" ] || { echo "error: --daemon requires at least one project, or --no-projects for a project-less home" >&2; exit 1; }
fi
DAEMON_CHARTER=${MX_DAEMON_CHARTER:-"{TASK}"}
DAEMON_SCOPE=${MX_DAEMON_SCOPE:-${MX_DAEMON_CHARTER:-"{TASK}"}}
if [ "$NO_PROJECTS" -eq 1 ]; then
  PROJECT_CLONES_BODY="None. This is a project-less domain: its subject is the Multplx repo this home lives in, so it needs no separate clones under \`projects/\`; its actors take pooled worktrees of that Multplx repo."
  PROJECT_CLONES_NOTE="This domain has no separate project clones: its subject is the Multplx repo this home lives in, and its actors take pooled worktrees of that repo."
else
  PROJECT_CLONES_BODY=$(printf '%s\n' "$DAEMON_PROJECTS" | tr ' ' '\n' | sed 's/^/- /')
  PROJECT_CLONES_NOTE="The projects above are local clones for work you coordinate; they are not an exclusive ownership claim."
fi
cat > "$BRIEF" <<EOF
You are a persistent daemon coordinated through the main broker. Work independently; do not wait for a human.

# Charter
$DAEMON_CHARTER

# Routing scope
$DAEMON_SCOPE

# Project clones
$PROJECT_CLONES_BODY

# Operating model
You are in an isolated Multplx home. The local \`AGENTS.md\` is your job description, and your local \`data/\`, \`state/\`, \`config/\`, and \`projects/\` dirs are yours to operate.
$PROJECT_CLONES_NOTE
Delegate project work to your own actors with the normal broker lifecycle: brief, spawn, status, watcher, steer, teardown, and recovery.
Do not invent a second delegation system.
You do not generate your own work.
Act only on tasks the main broker routes to you.
Never start a survey, audit, or "find improvements" sweep on your own initiative; that is not your job and it is unwanted.

# Requests from the main broker
You are a broker in your own home, so an incoming message reaches you in your own chat.
You must distinguish who it is from, because the answer goes to a different place.
A request relayed to you by the main broker is tagged with a leading \`$MX_FROM_BROKER_LABEL\` marker followed by an invisible system separator; this marker is untypable, so a human never produces it.
When a message carries that marker, do the work, then respond via the STATUS/ESCALATION path below, never only in this chat: the main broker does not read your chat, so a chat-only reply is lost.
Marked requests also carry a privacy-safe \`corr=<id>\` token after the marker; include that exact token in your parent status reply (or in the status pointer to a detailed doc) so the parent can correlate the answer.
Use the \`report_status\` tool when available.
Otherwise call \`$MX_ROOT/bin/mx-report --id $ID --state {state} --message "{one short line including corr=<id>}"\`.
The correlation token rides in \`--message\` unchanged.
For a terse result, a status line is the whole answer.
For a detailed answer (an investigation, a plan, an audit), write it to a doc under your home's \`data/\` and report a status that points to that doc - the scout-report pattern - so the main broker is woken and can read it.
Before treating an investigation or visual review as complete, load \`decision-hold-lifecycle\` from this home's \`.agents/skills/\` and pass its shared completion gate.
A message with NO marker is the maintainer typing directly into your pane: treat it as authoritative maintainer intervention and stay conversational exactly as you would for any maintainer message; do not force it onto the status path.

# Escalation to main broker
Handle routine work yourself.
Report only true maintainer-relevant outcomes or a declared external wait with the \`report_status\` tool when available, otherwise:
   \`$MX_ROOT/bin/mx-report --id $ID --state {state} --message "{one short line}"\`
States: working, paused, needs-decision, blocked, done, failed, resolved.
\`mx-report\` rejects anything else and prints the valid options; fix the call and retry.
Never write to \`$STATUS_FILE\` by hand.
Use \`paused: {why}\` (distinct from \`blocked:\`) only when your domain is deliberately idling on a known external wait you expect to clear on its own; use \`blocked:\` when you are stuck and need broker to act.
Use this only for material phase changes, a maintainer decision, a real blocker, a failure, or work ready for review.
This is also how you return the answer to a marked from-broker request above.
A marked request requires one correlated answer after the work; it does not require a separate receipt or start acknowledgement.
Never report \`working\` merely to acknowledge receipt or announce that a marked request has started.
When a routed-work phase has a supervisor-actionable material change worth reporting under the rule above, give that reported phase a stable key.
If its first reportable event is \`working [key=<work-slug>]: {material phase}\`, use the same key on its later \`paused\`, \`done\`, \`failed\`, \`needs-decision\`, or \`blocked\` event so the earlier working phase is superseded.
When a keyed phase ends without another reportable state, report \`resolved\` with the same \`--key <work-slug>\`.
When a decision you escalated is answered or a blocker clears and your domain resumes, report \`resolved\` (with the same \`--key <slug>\` if you opened it with one) so it is durably closed instead of resurfacing behind later unrelated events.
Routine internal supervision, heartbeats, retries, and actor churn stay inside your own home and must not touch that status file.

# Definition of done
You are persistent by default. Do not exit just because your queue is empty.
On startup and restart, run normal broker bootstrap and recovery through \`bin/mx-session-start.sh\` for your own home, but only to RECONCILE work that is already yours: in-flight actors, tracked backlog items, and durable watches recorded in this home.
When you have no assigned or in-flight work after that reconciliation, go idle and wait silently for the main broker to route you a task.
An empty queue is a healthy resting state, not a cue to invent work: never spawn a survey, audit, or any self-directed "find work" task on your own initiative.
If this charter cannot be carried out, report \`blocked\` or \`failed\` through the validated status path and stop.
EOF
if [ "$DAEMON_CHARTER" = "{TASK}" ]; then
  echo "scaffolded: $BRIEF (daemon charter; replace {TASK})"
else
  echo "scaffolded: $BRIEF (daemon charter)"
fi
exit 0
fi

REPO=${POS[1]}

if [ "$HERDR_LAB" -eq 1 ]; then
HERDR_LAB_HELPER=$(shell_quote "$MX_ROOT/bin/mx-herdr-lab.sh")
# shellcheck disable=SC2016  # single quotes are deliberate: these lines are literal brief text whose backtick-wrapped $(...) and "$HERDR_LAB_SESSION" snippets must reach the reading agent verbatim, not expand at scaffold time; only the '"$VAR"' break-outs interpolate.
HERDR_SECTION=$(printf '%s\n' \
'# Herdr isolation - HARD SAFETY CONTRACT' \
'This brief was explicitly scaffolded with `--herdr-lab` because the task will drive Herdr lifecycle behavior.' \
'On Herdr 0.7.3 the API socket is not relocatable by `HERDR_CONFIG_PATH`, `XDG_CONFIG_HOME`, or `HOME`.' \
'A named non-`default` session plus a trailing `--session <name>` on every call is the only viable local isolation.' \
'' \
'1. Set `HERDR_LAB_HELPER='"$HERDR_LAB_HELPER"'` and generate the session name with `HERDR_LAB_SESSION=$("$HERDR_LAB_HELPER" name '"$ID"')`.' \
'   Install `trap '\''"$HERDR_LAB_HELPER" teardown "$HERDR_LAB_SESSION"'\'' EXIT` before provisioning, then provision only with `"$HERDR_LAB_HELPER" provision "$HERDR_LAB_SESSION"`.' \
'2. Run every task-specific non-lifecycle Herdr command through `"$HERDR_LAB_HELPER" run "$HERDR_LAB_SESSION" <arguments...>`.' \
'   The helper appends the required trailing `--session "$HERDR_LAB_SESSION"`; `HERDR_SESSION` alone is never accepted as isolation.' \
'3. Teardown only through `"$HERDR_LAB_HELPER" teardown "$HERDR_LAB_SESSION"`.' \
'   It re-checks refuse-default immediately before stop and again immediately before delete, and fails closed on ambiguity.' \
'4. If an experiment requires a deliberate mid-run session stop, use only `"$HERDR_LAB_HELPER" stop "$HERDR_LAB_SESSION"`; it performs the same immediate refuse-default check.' \
'5. Forbidden commands: direct `herdr server stop`, every other server-global operation such as `herdr server live-handoff` or reload/update operations, direct `herdr session stop`, direct `herdr session delete`, and any Herdr call scoped only by ambient or inline `HERDR_SESSION`.' \
'6. The helper records the live default session before provisioning and verifies the identical system state after teardown.' \
'   A missing, stopped, or changed default session is a hard tripwire failure, never a cleanup warning to ignore.' \
'' \
'Never bypass the helper, even for a read-only lifecycle probe or cleanup after failure.' \
'The maintainer system uses the running `default` session.')
else
HERDR_SECTION=$(cat <<'EOF'
# Herdr lifecycle declaration - NOT ENABLED
**HARD SAFETY GATE:** this scaffold cannot inspect the task text that replaces `{TASK}` later.
If the task will start, stop, delete, restart, profile, or otherwise drive Herdr lifecycle behavior, stop and regenerate the brief with `--herdr-lab` before dispatch.
Do not add Herdr lifecycle commands to this unguarded brief by hand.
EOF
)
fi

if [ "$KIND" = scout ]; then
cat > "$BRIEF" <<EOF
You are an actor: an autonomous agent coordinated through the broker. Work independently; do not wait for a human.

# Task
{TASK}

$HERDR_SECTION

# Setup
You are in a disposable git worktree of $REPO, at a detached HEAD on a clean default branch.
This is a SCOUT task: the deliverable is a written report, not a PR.
The worktree is your laboratory - install, run, edit, and make scratch commits freely; all of it is discarded at teardown.
The report is the only thing that survives, so anything worth keeping must be in it.

# Rules
1. Never push to any remote and never open a PR.
2. Stay inside this worktree; the only file you may write outside it is the report below. The validated status writer owns status-file writes.
3. Use official gh for GitHub operations and a first-class browser tool only when browser work is required.
4. Report status with the \`report_status\` tool when available, otherwise:
   \`$MX_ROOT/bin/mx-report --id $ID --state {state} --message "{one short line}"\`
   States: working, paused, needs-decision, blocked, done, failed, resolved.
   \`mx-report\` rejects anything else and prints the valid options; fix the call and retry.
   Never write to \`$STATUS_FILE\` by hand.
   Each report wakes broker, so report sparingly: only phase changes a supervisor
   would act on and the needs-decision/blocked/paused/done/failed states. No step-by-step
   FYI progress lines; broker reads your pane for that.
   Use \`paused: {why}\` - distinct from \`blocked:\` - ONLY when you are deliberately idling on a
   known external wait you expect to clear on its own (an upstream release, a rate-limit reset):
   broker then leaves your idle pane alone and rechecks it on a long cadence instead of
   treating it as a possible wedge. Use \`blocked:\` when you are stuck and need help.
5. If you hit the same obstacle twice, report \`blocked\` and stop; broker will help.
6. If a decision belongs to a human (product choices, destructive actions),
   report \`needs-decision\` and stop. Multplx will reply with the decision.
   When broker replies or a blocker clears and you resume, report \`resolved\` with the same \`--key <slug>\` if you opened one, so the decision or blocker is durably closed and does not keep resurfacing.
7. Never invoke Multplx lifecycle or credentialed delivery commands from this worker.

# Definition of done
Write your findings to \`$DATA/$ID/report.md\`.
The report must stand alone: what you did, what you found, the evidence (commands run, output, file:line references), and what you recommend.
Before reporting done, read and follow \`$MX_ROOT/.agents/skills/decision-hold-lifecycle/SKILL.md\` and pass its shared completion gate for the report and any visual review.
When the report is complete, report \`done\` through rule 4 and stop.
If your findings reveal work that should be delivered (e.g. you reproduced a bug and the fix is clear), say so in the report; the broker may promote this task in place, and you would then receive mode-specific delivery instructions as a follow-up message.
EOF
echo "scaffolded: $BRIEF (scout; replace {TASK})"
exit 0
fi

# DELIVERY TASK: shape Setup / Rule 1 / Definition of done by the project's delivery mode.
# yolo does not affect the brief because the worker never owns approval decisions;
# broker applies the authority contract in AGENTS.md section 7, so discard it.
read -r MODE _ <<EOF
$("$MX_ROOT/bin/mx-project-mode.sh" "$REPO")
EOF

case "$MODE" in
  direct-PR)
    SETUP2=""
    RULE1='1. Never push to any remote, open a PR, or merge a PR. Commit only on your local `mx/'"$ID"'` branch; the credentialed delivery service owns every remote write.'
    IFS= read -r -d '' DOD <<EOF || true
# Definition of done
This project delivers **direct-PR** without the full validation pipeline, but remote delivery is still separate from agent work.
The task is complete only when the worktree is clean and the implementation is committed on your local branch \`mx/$ID\`.
Report \`done\` with \`ready for delivery at {full commit SHA}\` through the validated status path and stop.
Do not push, open a PR, or merge.
The configured approval authority accepts the local commit, then the non-agent delivery service pushes exactly that approved SHA and opens the PR.
EOF
    ;;
  local-only)
    SETUP2=""
    RULE1="1. Never push to any remote and never open a PR. Work only on your \`mx/$ID\` branch; broker handles the merge into local \`main\`."
    IFS= read -r -d '' DOD <<EOF || true
# Definition of done
This project delivers **local-only**: no remote, no PR, no pipeline.
The task is complete only when committed on your branch \`mx/$ID\`. Do NOT push, do NOT open a PR, do NOT merge.
Keep your branch a clean fast-forward onto the current default branch - if \`main\` has advanced, rebase onto it so the eventual merge stays a fast-forward.
When it is implemented and committed, report \`done\` with \`ready in branch mx/$ID\` through the validated status path and stop.
The configured merge authority approves the ready branch, then broker merges it into local \`main\` through the guarded fast-forward path.
EOF
    ;;
  *)  # deep-review (default)
    SETUP2=""
    RULE1='1. Never push to any remote, open a PR, or merge a PR. Commit only on your local `mx/'"$ID"'` branch; the validation gate and credentialed delivery service own the handoff.'
    IFS= read -r -d '' DOD <<EOF || true
# Definition of done
After implementation is clean and committed on your local branch \`mx/$ID\`, you must drive its local validation:
\`$MX_ROOT/bin/mx-deep-review.sh $ID --intent-file $BRIEF\`
The gate owns review, focused testing, documentation, lint, and its own fix commits.
Ask-user findings are never yours to answer.
If the gate parks on one, preserve its emitted decision request and stop while Multplx applies the authority contract in its \`AGENTS.md\`.
Do not silently bypass broker's authority check and any required maintainer escalation.
After Multplx supplies the accepted answer, you - never broker - run:
\`$MX_ROOT/bin/mx-deep-review.sh respond $ID --decision {key} --answer "{accepted answer}"\`
Then rerun the original gate command to resume.
The task is complete only when deep-review passes, the worktree is clean, and the gate writes the pending exact-SHA delivery record.
Do not push, open a PR, merge, invoke credentialed delivery, or synthesize gate state.
The non-agent delivery service separately verifies approval and pushes exactly the validated SHA.
EOF
    ;;
esac

cat > "$BRIEF" <<EOF
You are an actor: an autonomous agent coordinated through the broker. Work independently; do not wait for a human.

# Task
{TASK}

$HERDR_SECTION

# Setup
You are in a disposable git worktree of $REPO, at a detached HEAD on a clean default branch.

**Verify isolation before anything else.** Run \`pwd -P\` and \`git rev-parse --show-toplevel\`; both must resolve to the disposable task worktree you were launched in, such as a treehouse pool path, not the primary checkout broker operates from.
The path check is authoritative: \`git rev-parse --git-dir\` and \`git rev-parse --git-common-dir\` can help inspect the repo, but they do not prove you are outside the primary checkout.
If the top-level path is the primary checkout or not the worktree you were launched in, STOP - do not branch or commit here - report \`blocked\` through the validated status path and stop.

1. First action: create your branch: \`git checkout -b mx/$ID\`$SETUP2

# Rules
$RULE1
2. Stay inside this worktree; modify nothing outside it.
3. Use official gh only for read-only GitHub operations and a first-class browser tool only when browser work is required.
4. Report status with the \`report_status\` tool when available, otherwise:
   \`$MX_ROOT/bin/mx-report --id $ID --state {state} --message "{one short line}"\`
   States: working, paused, needs-decision, blocked, done, failed, resolved.
   \`mx-report\` rejects anything else and prints the valid options; fix the call and retry.
   Never write to \`$STATUS_FILE\` by hand.
   Each report wakes broker, so report sparingly: only phase changes a supervisor
   would act on (setup done, bug reproduced, fix implemented, validation passed) and the
   needs-decision/blocked/paused/done/failed states. No step-by-step FYI progress lines;
   broker reads your pane for that.
   A mid-task \`working:\` line (including setup complete) is nonterminal: do not end the
   turn after it; continue the same stage until a defined \`done:\` gate under Definition of done.
   Use \`paused: {why}\` - distinct from \`blocked:\` - ONLY when you are deliberately idling on a
   known external wait you expect to clear on its own (an upstream release, a rate-limit reset,
   a scheduled window): broker then leaves your idle pane alone and rechecks it on a long
   cadence instead of treating it as a possible wedge. Use \`blocked:\` when you are stuck and need help.
5. If you hit the same obstacle twice, report \`blocked\` and stop; broker will help.
6. If a decision belongs above the implementation worker (product choices, destructive actions, ask-user findings),
   report \`needs-decision\` and stop. Multplx will apply the configured authority and reply with the decision.
   When broker replies or a blocker clears and you resume, report \`resolved\` with the same \`--key <slug>\` if you opened one, so the decision or blocker is durably closed and does not keep resurfacing.
7. Never invoke Multplx lifecycle or credentialed delivery commands from this worker.

# Project memory
If \`AGENTS.md\` or \`CLAUDE.md\` already exists, or if this task produced durable project-intrinsic knowledge, run \`$MX_ROOT/bin/mx-ensure-agents-md.sh .\` in the worktree.
Record only project knowledge useful to almost every future session.
For anything the codebase already shows, prefer a pointer to the authoritative file, command, or doc over copying the detail.
If you touch a project \`AGENTS.md\` that lacks \`## Maintaining this file\`, add that short self-governance section from \`$MX_ROOT/bin/mx-ensure-agents-md.sh\` in the same pass.
Keep it proportionate: skip \`AGENTS.md\` edits for trivial tasks that produced no durable project knowledge.

$DOD
EOF
echo "scaffolded: $BRIEF (delivery, mode=$MODE; replace {TASK})"

#!/usr/bin/env node

const count = Number(process.argv[2] ?? 0);
if (![0, 1, 5, 10, 20].includes(count)) {
  process.stderr.write("usage: portfolio.mjs 0|1|5|10|20\n");
  process.exit(2);
}

const generated = "2026-09-17T16:00:00Z";
const projects = [
  { id: "alpha", display_name: "Console", path: "/work/console", checkout_id: "alpha-main", registered: true },
  { id: "beta", display_name: "Console", path: "/work/labs/console", checkout_id: "beta-main", registered: true },
  { id: "gamma", display_name: "Runtime", path: "/work/org/platform/runtime", checkout_id: "gamma-main", registered: true },
];
const states = ["working", "queued", "blocked", "validating", "completed"];
const roles = ["implementer", "researcher", "reviewer"];
const coordinatorCount = count >= 5 ? Math.min(3, Math.ceil(count / 8)) : 0;
const recordCount = count + coordinatorCount;
const tasks = Array.from({ length: recordCount }, (_, index) => {
  const coordinator = index >= count;
  const ordinal = index + 1;
  const project = projects[index % projects.length];
  const key = `/mx/root#task:task-${ordinal}`;
  const parent = index > 0 && index % 4 === 1 ? `task-${ordinal - 1}` : null;
  const child = index + 1 < count && index % 4 === 0 ? [`/mx/root#task:task-${ordinal + 1}`] : [];
  const state = states[index % states.length];
  const role = coordinator ? "sub-orchestrator" : roles[index % roles.length];
  const decision = index % 7 === 2 ? [{ id: `decision-${ordinal}`, question: `Choose rollout scope for task ${ordinal}`, brief_revision: 2, workflow_revision: "wf-2", answer: null, waiting_since: "2026-09-17T15:42:00Z", source: "coordination" }] : [];
  const partial = index === count - 1 && count >= 10;
  return {
    key,
    id: `task-${ordinal}`,
    title: ordinal === count && count > 1 ? "A deliberately long task title that verifies wrapping without hiding current ownership or status" : `Portfolio task ${ordinal}`,
    parent_id: parent,
    root_id: parent || `task-${ordinal}`,
    children: child,
    project: { ...project, common_git_identity: `git-${project.id}` },
    state,
    priority: 20 - index,
    role,
    owner: { home: "/mx/root", coordinator: role === "sub-orchestrator" ? `coordinator-${ordinal}` : "root", parent_id: parent },
    attempt: { id: `attempt-${ordinal}-2`, generation: 2, brief_revision: 2 },
    prior_attempts: index % 3 === 0 ? [{ id: `attempt-${ordinal}-1`, generation: 1, outcome: "replaced" }] : [],
    brief: { revision: 2, digest: `brief-${ordinal}`, path: `/artifact/data/task-${ordinal}/brief.md`, scope: `Implement task ${ordinal}`, research: [`/artifact/data/task-${ordinal}/research.md`] },
    workflow: { id: `workflow-${ordinal}`, revision: "wf-2", name: "delivery", status: state, current_stage: state === "completed" ? "deliver" : "implement", updated_at: generated },
    dependencies: index % 5 === 2 ? [{ task_id: `task-${ordinal - 1}`, task_key: `/mx/root#task:task-${ordinal - 1}`, state: "working", blocking: true }] : [],
    decisions: decision,
    evidence: {
      report: `/artifact/data/task-${ordinal}/report.md`,
      delivery: state === "completed" ? { outcome: "published", commit: `abcdef${ordinal}`, checks: [{ label: "Focused checks passed", status: "passed" }], review: { summary: "Review clear", findings: [] }, pr_url: `https://example.invalid/pr/${ordinal}` } : null,
      history: [],
      review_queue: state === "completed" ? { task_key: key, task_id: `task-${ordinal}`, owner_home: "/mx/root", project_id: project.id, priority: 20 - index, commit: `abcdef${ordinal}`, pr_url: `https://example.invalid/pr/${ordinal}`, outcome: "published", state: "ready", revision_freshness: "current", checks_passing: true, review_complete: true, review_clear: true, pr_ready: true, checks: [], review: null, limitations: [], dependencies: [], blocked_by: [] } : null,
      pr: state === "completed" ? { url: `https://example.invalid/pr/${ordinal}`, source: "revision-bound-delivery" } : { url: null, source: "absent" },
    },
    allocation: { binding: { owner_task: key, generation: 2, path: `/worktrees/task-${ordinal}` }, observation: { state: index % 6 === 4 ? "retained" : "leased", persistence: index % 6 === 4 ? "persistent-zero-process" : "active", recovery_issue: index === count - 1 && count >= 10 ? "allocation owner could not be confirmed" : null } },
    sessions: [{ attempt_id: `attempt-${ordinal}-2`, provider: index % 2 ? "codex" : "claude", session_id: `session-${ordinal}`, endpoint: `pane-${ordinal}`, persistent: index % 4 === 3, current: true, state }],
    native_observations: index % 6 === 0 ? [{ observation_id: `native-${ordinal}`, provider: "native", child_id: null, parent_session_id: `parent-${ordinal}`, turn_id: null, parent_attempt: null, state: "observed", observed_at: generated, artifact: null, recovery: "opaque-child-identity" }] : [],
    latest_change: { state, summary: `Task ${ordinal} advanced`, observed_at: generated },
    freshness: { status: partial ? "partial" : "fresh", age_seconds: 0, partial, reasons: partial ? ["runtime observation unavailable"] : [] },
  };
});

const domainTasks = tasks.filter((task) => task.role === "sub-orchestrator");
const snapshot = {
  schema: "mx-system-snapshot.v1",
  generated,
  portfolio: {
    schema: "mx-portfolio.v1",
    generated,
    observed_at: generated,
    freshness: { status: count >= 10 ? "partial" : "fresh", age_seconds: 0, partial: count >= 10, reasons: count >= 10 ? ["one runtime observation unavailable"] : [] },
    counts: { tasks: count, coordinators: coordinatorCount, records: tasks.length, sessions: tasks.length, attempts: tasks.length + tasks.filter((task) => task.prior_attempts.length).length, projects: count ? Math.min(3, count) : 0, domains: domainTasks.length },
    projects: count ? projects.slice(0, Math.min(3, count)) : [],
    tasks,
  },
  domains: {
    records: domainTasks.map((task, index) => {
      const unavailable = count >= 10 && index === domainTasks.length - 1;
      return { domain_id: `domain-${task.id}`, scope: "project", projects: [task.project.id], coordinator: { id: task.id, qualified_id: task.key, runtime_home: `/mx/domains/${task.id}` }, channel: unavailable ? { available: false, health: null, reason: "coordinator endpoint unavailable" } : { available: true, health: { pending_inbox: 1, pending_outbox: 1 } }, observation: { generated, age_seconds: unavailable ? null : 0, partial: unavailable, reason: unavailable ? "coordinator endpoint unavailable" : null }, counts: { useful_tasks: unavailable ? null : 2, worker_sessions: unavailable ? null : 1, coordinator_sessions: unavailable ? null : 1, current_attempts_known: unavailable ? null : 2, attempts_unavailable: unavailable ? null : 0, undelivered_outcomes: unavailable ? null : 2 }, children: [], tasks: [], workflow_runs: [] };
    }),
    total: domainTasks.length, shown: domainTasks.length, truncated: 0, complete: !tasks.some((task) => task.freshness.partial),
  },
  vplan_reviews: { records: [] },
  later_feeds: { doctor: { available: true }, timeline: { available: true } },
};

process.stdout.write(`${JSON.stringify(snapshot)}\n`);

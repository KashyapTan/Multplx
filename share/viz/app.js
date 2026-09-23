"use strict";

const pollMs = Number(document.querySelector('meta[name="mx-viz-poll-ms"]')?.content || 2500);
const agentsGraphApi = window.MxAgentsGraph;
const hiddenPollMs = Math.max(15000, pollMs * 6);
const staleAfterSeconds = Math.max(15, Math.ceil(pollMs / 1000) * 4);
const list = (value) => Array.isArray(value) ? value : [];
const object = (value) => value && typeof value === "object" && !Array.isArray(value) ? value : {};
const valueOr = (value, fallback = "—") => value === null || value === undefined || value === "" ? fallback : value;
const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = String(text);
  return node;
};
const clear = (node) => node.replaceChildren();

const connectionNote = document.querySelector("#connection-note");
const liveDot = document.querySelector("#live-dot");
const dialog = document.querySelector("#detail-dialog");
const controls = ["search", "project-filter", "status-filter", "role-filter", "priority-filter"]
  .map((id) => document.getElementById(id));
let currentPayload = null;
let portfolio = null;
let generatedAt = null;
let observationAgeMs = null;
let observationAgeReadAt = null;
let serviceCache = null;
let meaningfulHash = null;
let pollTimer = null;
let pollInFlight = false;
let lastError = null;
let lastAppliedHash = null;
const expanded = new Set();
let normalizedAgentGraph = null;
let selectedAgentKey = null;
let agentSearchValue = "";
let lastRenderedAgentSearch = null;
const collapsedAgentKeys = new Set();
let attentionItemCount = 0;
let agentsViewActive = false;

function ageText(seconds) {
  if (!Number.isFinite(seconds)) return "unknown";
  if (seconds < 60) return `${Math.max(0, Math.floor(seconds))}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

function formatLocalTime(input, options = {}) {
  const date = input instanceof Date ? input : new Date(input);
  if (Number.isNaN(date.getTime())) return String(input || "unknown");
  const time = date.toLocaleTimeString(undefined, {
    hour: "numeric", minute: "2-digit", second: options.seconds ? "2-digit" : undefined, hour12: true,
  });
  if (options.date === false) return time;
  const now = new Date();
  const sameDay = date.toDateString() === now.toDateString();
  return sameDay ? time : `${date.toLocaleDateString(undefined, { month: "short", day: "numeric" })} · ${time}`;
}

function titleCase(value) {
  return String(value ?? "").replace(/[_-]/g, " ").replace(/\b\w/g, (character) => character.toUpperCase());
}

function announce(message) {
  document.querySelector("#aria-live").textContent = message;
}

function projectRecord(task) {
  const raw = task.project;
  if (typeof raw === "string") return { id: raw, display: raw, path: raw };
  const project = object(raw);
  return {
    id: String(project.id || project.project_id || project.path || "unassigned"),
    display: project.display_name || project.display || project.name || project.id || project.path || "Unassigned",
    path: project.path || project.checkout || project.checkout_path || "",
  };
}

function stateName(task) {
  const state = task.state ?? task.current_state;
  return String(typeof state === "object" ? state.state || state.status || "unknown" : state || "unknown").toLowerCase();
}

function taskRole(task) {
  return String(task.role || task.assignment?.role || task.current_role || "unassigned").toLowerCase();
}

function taskOwner(task) {
  const owner = task.owner || task.assignment?.owner || task.allocation?.owner;
  return typeof owner === "object" ? owner.coordinator || owner.home || owner.parent_id || "unknown" : owner || "unknown";
}

function priorityName(task) {
  const raw = task.priority;
  if (typeof raw === "object") return String(raw.label || raw.name || raw.value || "normal").toLowerCase();
  if (Number.isFinite(Number(raw))) return `p${Number(raw)}`;
  return String(raw || "normal").toLowerCase();
}

function priorityRank(priority, task) {
  if (Number.isFinite(Number(task?.priority))) return -Number(task.priority);
  const ranks = { urgent: 0, critical: 0, high: 1, normal: 2, medium: 2, low: 3 };
  return ranks[priority] ?? 2;
}

function normalizedTasks(source) {
  if (["mx-portfolio.v1", "mx-portfolio.compat.v1"].includes(source?.schema)) return list(source.tasks);
  return [];
}

function legacyPortfolio(snapshot) {
  const tasks = list(snapshot.tasks).map((task) => ({
    ...task,
    title: task.title || task.id,
    state: task.current_state,
    role: task.role || task.kind || "worker",
    owner: task.owner || task.id,
    priority: task.priority || "normal",
    sessions: task.endpoint ? [{ provider: task.harness, endpoint: task.endpoint, state: task.current_state?.state }] : [],
    decisions: list(task.hints?.open_decisions),
    freshness: { status: task.current_state?.state === "unknown" ? "unknown" : "fresh" },
  }));
  return {
    schema: "mx-portfolio.compat.v1",
    generated: snapshot.generated,
    observed_at: snapshot.generated,
    tasks,
    projects: [],
    domains: [],
    freshness: { status: snapshot.watcher?.stale ? "stale" : "fresh", partial: false, reasons: [] },
    counts: { tasks: tasks.length, sessions: tasks.reduce((sum, task) => sum + list(task.sessions).length, 0), projects: new Set(tasks.map((task) => projectRecord(task).id)).size },
  };
}

function portfolioFrom(payload) {
  const candidate = payload?.snapshot?.portfolio || payload?.portfolio;
  if (!candidate) return legacyPortfolio(payload?.snapshot || {});
  if (candidate.schema === "mx-portfolio.v1") return candidate;
  return {
    schema: "mx-portfolio.unsupported",
    tasks: [], projects: [], domains: [],
    counts: { tasks: 0, sessions: 0, attempts: 0, projects: 0, domains: 0 },
    freshness: { status: "unknown", partial: true, reasons: [`unsupported portfolio schema: ${candidate.schema || "missing"}`] },
  };
}

function currentFilters() {
  return {
    search: document.querySelector("#search").value.trim().toLowerCase(),
    project: document.querySelector("#project-filter").value,
    status: document.querySelector("#status-filter").value,
    role: document.querySelector("#role-filter").value,
    priority: document.querySelector("#priority-filter").value,
  };
}

function taskSearchText(task) {
  const project = projectRecord(task);
  return [task.id, task.title, task.summary, project.id, project.display, project.path, stateName(task), taskRole(task), taskOwner(task),
    ...list(task.dependencies).flatMap((dependency) => [dependency.task_id, dependency.state]),
    ...list(task.sessions).flatMap((session) => [session.provider, session.session_id, session.state]),
  ].filter(Boolean).join(" ").toLowerCase();
}

function filteredTasks() {
  const filters = currentFilters();
  return normalizedTasks(portfolio).filter((task) => {
    const project = projectRecord(task);
    return (!filters.search || taskSearchText(task).includes(filters.search))
      && (!filters.project || project.id === filters.project)
      && (!filters.status || stateName(task) === filters.status)
      && (!filters.role || taskRole(task) === filters.role)
      && (!filters.priority || priorityName(task) === filters.priority);
  }).sort((a, b) => priorityRank(priorityName(a), a) - priorityRank(priorityName(b), b)
    || projectRecord(a).display.localeCompare(projectRecord(b).display)
    || String(a.title || a.id).localeCompare(String(b.title || b.id)));
}

function stableFocusKey(node = document.activeElement) {
  return node?.dataset?.focusKey || null;
}

function restoreFocus(key) {
  if (!key) return;
  const candidate = [...document.querySelectorAll("[data-focus-key]")].find((node) => node.dataset.focusKey === key);
  candidate?.focus({ preventScroll: true });
}

function updateSelect(select, values, label) {
  const selected = select.value;
  const options = [el("option", "", label), ...[...values].filter(Boolean).sort().map((value) => {
    const option = el("option", "", titleCase(value));
    option.value = value;
    return option;
  })];
  options[0].value = "";
  select.replaceChildren(...options);
  select.value = [...values].includes(selected) ? selected : "";
}

function updateFilterOptions(tasks) {
  updateSelect(document.querySelector("#project-filter"), new Set(tasks.map((task) => projectRecord(task).id)), "All projects");
  updateSelect(document.querySelector("#status-filter"), new Set(tasks.map(stateName)), "All states");
  updateSelect(document.querySelector("#role-filter"), new Set(tasks.map(taskRole)), "All roles");
  updateSelect(document.querySelector("#priority-filter"), new Set(tasks.map(priorityName)), "All priorities");
}

function statusTone(status) {
  if (["passed", "done", "complete", "completed", "merged", "current", "fresh", "healthy"].includes(status)) return "green";
  if (["queued", "parked", "paused", "waiting", "stale", "partial"].includes(status)) return "amber";
  if (["failed", "error", "invalid", "blocked", "interrupted", "unavailable"].includes(status)) return "red";
  if (["running", "working", "validating", "active", "ready"].includes(status)) return "blue";
  return "neutral";
}

function chip(text, tone) {
  return el("span", `chip tone-${tone || statusTone(String(text).toLowerCase())}`, text);
}

function artifactLink(label, url, kind, renderable = true) {
  const link = el("a", "", label);
  link.href = url;
  if (renderable && String(url).startsWith("/artifact/")) {
    link.addEventListener("click", (event) => {
      if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      openArtifact(label, url, kind);
    });
  } else {
    link.target = "_blank";
    link.rel = "noreferrer";
  }
  return link;
}

function artifactForSource(source) {
  if (!source || String(source).startsWith("/artifact/")) return null;
  return list(currentPayload?.artifacts).find((artifact) => artifact.source_path === source) || null;
}

function appendEvidenceLink(target, label, record, kind) {
  if (!record) return;
  const value = typeof record === "string" ? { label: record, path: record } : record;
  const source = value.url || value.path || value.artifact;
  const catalogEntry = artifactForSource(source);
  const url = value.url || catalogEntry?.url || source;
  const row = el("div", "evidence-row");
  row.append(el("span", "evidence-label", label));
  if (url && (String(url).startsWith("/artifact/") || String(url).startsWith("http"))) {
    row.append(artifactLink(value.label || value.title || value.status || catalogEntry?.label || source || url, url, kind));
  } else {
    row.append(el("span", "", value.label || value.title || value.status || value.result || valueOr(url)));
  }
  if (value.revision || value.commit || value.head) row.append(el("code", "", String(value.revision || value.commit || value.head).slice(0, 12)));
  target.append(row);
}

function detailSection(title, className = "") {
  const section = el("section", `detail-section ${className}`.trim());
  section.append(el("h4", "", title));
  return section;
}

function factGrid(rows) {
  const grid = el("dl", "fact-grid");
  for (const [label, value] of rows) {
    if (value === null || value === undefined || value === "") continue;
    grid.append(el("dt", "", label), el("dd", "", value));
  }
  return grid;
}

function renderExecution(task) {
  const section = detailSection("Execution and ownership");
  const attempt = object(task.attempt);
  const allocation = object(task.allocation);
  const binding = object(allocation.binding);
  const observation = object(allocation.observation);
  const latest = object(task.latest_change);
  section.append(factGrid([
    ["Owner", taskOwner(task)], ["Role", titleCase(taskRole(task))],
    ["Attempt", attempt.id], ["Generation", attempt.generation], ["Brief revision", attempt.brief_revision || task.brief?.revision],
    ["Allocation owner", binding.owner_task || binding.owner || binding.task_key],
    ["Allocation generation", binding.generation || binding.attempt_generation],
    ["Allocation", observation.state || observation.status || binding.state],
    ["Worktree", binding.path || binding.worktree_path || observation.path],
    ["Persistence", observation.persistence || observation.lease_state || binding.persistence || task.persistence],
    ["Recovery", observation.recovery_issue || observation.issue || binding.recovery_issue],
    ["Latest change", latest.summary || latest.state],
    ["Change observed", latest.observed_at ? formatLocalTime(latest.observed_at, { seconds: true }) : "timestamp unavailable"],
  ]));
  const sessions = list(task.sessions);
  const attempts = list(task.prior_attempts);
  const rows = sessions.length ? sessions : attempts;
  if (rows.length) {
    const nested = el("div", "nested-list");
    for (const record of rows) {
      const item = el("div", "nested-row");
      item.append(chip(record.role || record.provider || "session", "neutral"), el("strong", "", record.session_id || record.attempt_id || record.id || "unreported id"));
      item.append(el("span", "nested-meta", [record.owner, record.state, record.endpoint?.agent_alive || record.persistent && "persistent"].filter(Boolean).join(" · ")));
      nested.append(item);
    }
    section.append(nested);
  } else {
    section.append(el("p", "empty-inline", "No child session is observable. This does not imply idle work."));
  }
  const nativeObservations = list(task.native_observations);
  if (nativeObservations.length) {
    const observed = el("div", "nested-list");
    for (const record of nativeObservations) {
      const item = el("div", "nested-row");
      item.append(chip("provider observation", "neutral"), el("strong", "", record.child_id || record.observation_id || "identity unavailable"));
      item.append(el("span", "nested-meta", [record.provider, record.state, record.child_id ? "child identified" : "not counted as a session"].filter(Boolean).join(" · ")));
      observed.append(item);
    }
    section.append(observed);
  }
  return section;
}

function renderWorkflow(task) {
  const section = detailSection("Workflow and dependencies");
  const workflow = object(task.workflow);
  const taskStage = object(workflow.task_stage);
  section.append(factGrid([
    ["Workflow", workflow.name || workflow.id], ["Revision", workflow.revision],
    ["Current stage", taskStage.stage || taskStage.id || workflow.current_stage || workflow.stage],
    ["Stage status", taskStage.status], ["Stage role", taskStage.role], ["Stage attempt", taskStage.attempt_id],
  ]));
  const stages = list(workflow.stages).length ? list(workflow.stages) : Object.keys(taskStage).length ? [taskStage] : [];
  if (stages.length) {
    const track = el("ol", "stage-track");
    for (const stage of stages) {
      const name = typeof stage === "string" ? stage : stage.name || stage.id;
      const status = typeof stage === "string" ? (name === workflow.current_stage ? "active" : "unknown") : stage.status || "unknown";
      const item = el("li", status === "active" || name === workflow.current_stage ? "current" : "");
      item.append(chip(status), document.createTextNode(name || "stage"));
      track.append(item);
    }
    section.append(track);
  }
  const dependencies = list(task.dependencies);
  if (dependencies.length) {
    const depList = el("div", "nested-list");
    for (const dependency of dependencies) {
      const row = el("div", "nested-row");
      row.append(chip(dependency.blocking ? "blocking" : dependency.state || "dependency", dependency.blocking ? "red" : null));
      const target = el("a", "", dependency.task_id || dependency.id || "unknown task");
      target.href = hashForTask(dependency.task_key || dependency.task_id || dependency.id);
      row.append(target, el("span", "nested-meta", dependency.reason || dependency.state || "state unknown"));
      depList.append(row);
    }
    section.append(depList);
  }
  return section;
}

function renderDecisions(task) {
  const decisions = list(task.decisions).filter((decision) => !decision.answer && decision.state !== "resolved");
  if (!decisions.length) return null;
  const section = detailSection("Pending decisions", "decision-detail");
  for (const decision of decisions) {
    const card = el("article", "decision-card");
    card.append(el("strong", "", decision.question || decision.summary || decision.id || "Decision required"));
    card.append(factGrid([
      ["Target brief", decision.brief_revision], ["Workflow revision", decision.workflow_revision],
      ["Waiting", decision.waiting_since ? `${ageText((Date.now() - Date.parse(decision.waiting_since)) / 1000)} · since ${formatLocalTime(decision.waiting_since)}` : null],
    ]));
    card.append(el("span", "readonly-note", "Viewer only · respond through the ordinary Multplx workflow"));
    section.append(card);
  }
  return section;
}

function renderEvidence(task) {
  const section = detailSection("Evidence and delivery");
  const brief = object(task.brief);
  appendEvidenceLink(section, "Brief", brief, "brief");
  for (const research of list(brief.research || task.research)) appendEvidenceLink(section, "Research", research, "research");
  const evidence = object(task.evidence);
  appendEvidenceLink(section, "Report", evidence.report, "report");
  const delivery = object(evidence.delivery);
  for (const record of list(evidence.test_review || delivery.checks || evidence.checks || evidence.reviews)) appendEvidenceLink(section, "Check / review", record, "review");
  appendEvidenceLink(section, "Review", delivery.review, "review");
  appendEvidenceLink(section, "Delivery", evidence.delivery, "delivery");
  const reviewQueue = object(evidence.review_queue);
  if (Object.keys(reviewQueue).length) {
    section.append(factGrid([
      ["Human review", titleCase(reviewQueue.state)],
      ["Revision", titleCase(reviewQueue.revision_freshness)],
      ["Checks passing", reviewQueue.checks_passing === true ? "Yes" : reviewQueue.checks_passing === false ? "No" : null],
      ["Review complete", reviewQueue.review_complete === true ? "Yes" : reviewQueue.review_complete === false ? "No" : null],
      ["PR ready", reviewQueue.pr_ready === true ? "Yes" : reviewQueue.pr_ready === false ? "No" : null],
      ["Blocked by", list(reviewQueue.blocked_by).join(", ")],
    ]));
  }
  appendEvidenceLink(section, "Pull request", evidence.pr || task.pr, "pull request");
  if (section.children.length === 1) section.append(el("p", "empty-inline", "No current-revision evidence is published."));
  return section;
}

function taskDetails(task) {
  const body = el("div", "task-details");
  body.append(renderExecution(task), renderWorkflow(task));
  const decisions = renderDecisions(task);
  if (decisions) body.append(decisions);
  body.append(renderEvidence(task));
  if (list(task.children).length) {
    const children = detailSection("Nested tasks");
    for (const key of task.children) {
      const child = normalizedTasks(portfolio).find((candidate) => taskKey(candidate) === key);
      const link = el("a", "nested-row", child?.title || key);
      link.href = hashForTask(key, child ? projectRecord(child).id : "");
      children.append(link);
    }
    body.append(children);
  }
  return body;
}

function taskKey(task) {
  return String(task.key || task.id || "unknown");
}

function hashForTask(id, project = "") {
  const params = new URLSearchParams();
  if (id) params.set("task", id);
  if (project) params.set("project", project);
  return `#${params.toString()}`;
}

function setHashForTask(task) {
  const next = hashForTask(taskKey(task), projectRecord(task).id);
  if (window.location.hash !== next) history.replaceState(null, "", next);
  lastAppliedHash = next;
}

function taskRow(task) {
  const id = taskKey(task);
  const details = el("details", "task-row");
  details.dataset.taskId = id;
  details.id = `task-${encodeURIComponent(id)}`;
  details.open = expanded.has(id);
  const summary = el("summary", "task-summary");
  summary.dataset.focusKey = `task:${id}`;
  const project = projectRecord(task);
  const left = el("div", "task-main");
  const title = el("div", "task-title-line");
  title.append(chip(priorityName(task), priorityRank(priorityName(task), task) <= -1 ? "red" : "neutral"), el("strong", "task-title", task.title || id));
  const identity = el("div", "task-identity", `${task.id || id} · ${project.display}${project.path && project.path !== project.display ? ` · ${project.path}` : ""}`);
  left.append(title, identity);
  const stage = task.workflow?.current_stage || task.workflow?.stage;
  const right = el("div", "task-signals");
  right.append(chip(stateName(task)), chip(taskRole(task), "neutral"));
  if (stage) right.append(el("span", "signal-text", stage));
  const openDecisions = list(task.decisions).filter((decision) => !decision.answer && decision.state !== "resolved");
  if (openDecisions.length) right.append(chip(`${openDecisions.length} decision${openDecisions.length === 1 ? "" : "s"}`, "red"));
  const freshness = object(task.freshness);
  right.append(chip(freshness.status || "unknown", freshness.status === "stale" || freshness.status === "partial" ? "amber" : null));
  summary.append(left, right);
  details.append(summary, taskDetails(task));
  details.addEventListener("toggle", () => {
    if (details.open) {
      expanded.add(id);
      setHashForTask(task);
    } else {
      expanded.delete(id);
      if (new URLSearchParams(location.hash.slice(1)).get("task") === id) { history.replaceState(null, "", "#"); lastAppliedHash = "#"; }
    }
  });
  return details;
}

function renderTasks() {
  if (!portfolio) return;
  const focusKey = stableFocusKey();
  const target = document.querySelector("#task-list");
  const tasks = filteredTasks();
  clear(target);
  const grouped = new Map();
  for (const task of tasks) {
    const project = projectRecord(task);
    if (!grouped.has(project.id)) grouped.set(project.id, { project, tasks: [] });
    grouped.get(project.id).tasks.push(task);
  }
  for (const { project, tasks: rows } of grouped.values()) {
    const group = el("section", "project-group");
    const heading = el("div", "project-heading");
    const text = el("div");
    text.append(el("h3", "", project.display), project.path ? el("span", "project-path", project.path) : document.createTextNode(""));
    const coordinators = rows.filter((task) => taskRole(task) === "sub-orchestrator").length;
    const useful = rows.length - coordinators;
    const groupCount = coordinators ? `${useful} task${useful === 1 ? "" : "s"} · ${coordinators} coordinator${coordinators === 1 ? "" : "s"}` : `${useful} task${useful === 1 ? "" : "s"}`;
    heading.append(text, el("span", "section-meta", groupCount));
    group.append(heading, ...rows.map(taskRow));
    target.append(group);
  }
  if (!tasks.length) {
    const all = normalizedTasks(portfolio);
    const state = el("div", "empty-state");
    state.append(el("strong", "", all.length ? "No tasks match these filters" : "No accepted tasks yet"));
    state.append(el("p", "", all.length ? "Clear or change a filter to return to the portfolio." : "The shared projection is healthy and currently contains zero tasks."));
    target.append(state);
  }
  const total = normalizedTasks(portfolio).length;
  const useful = Number(portfolio.counts?.tasks ?? total);
  const coordinators = Number(portfolio.counts?.coordinators ?? 0);
  document.querySelector("#task-summary").textContent = `Showing ${tasks.length} of ${total} records · ${useful} tasks · ${coordinators} coordinators`;
  restoreFocus(focusKey);
}

function renderCounts() {
  const counts = object(portfolio.counts);
  const tasks = normalizedTasks(portfolio);
  const rows = [
    ["Tasks", counts.tasks ?? tasks.length],
    ["Coordinators", counts.coordinators ?? tasks.filter((task) => taskRole(task) === "sub-orchestrator").length],
    ["Sessions", counts.sessions ?? tasks.reduce((sum, task) => sum + list(task.sessions).length, 0)],
    ["Attempts", counts.attempts ?? tasks.reduce((sum, task) => sum + (task.attempt ? 1 : 0) + list(task.prior_attempts).length, 0)],
    ["Projects", counts.projects ?? new Set(tasks.map((task) => projectRecord(task).id)).size],
    ["Domains", counts.domains ?? list(portfolio.domains).length],
  ];
  const target = document.querySelector("#portfolio-counts");
  target.replaceChildren(...rows.flatMap(([label, value]) => [el("div", "count-card", ""),]).map((card, index) => {
    const [label, value] = rows[index];
    card.append(el("dt", "", label), el("dd", "", value));
    return card;
  }));
}

function deliveryStatus(task) {
  const evidence = object(task.evidence);
  const reviewQueue = object(evidence.review_queue);
  return String(reviewQueue.state || "").toLowerCase();
}

function renderAttention() {
  const target = document.querySelector("#attention-list");
  const panel = document.querySelector("#attention-panel");
  const items = [];
  for (const task of normalizedTasks(portfolio)) {
    for (const decision of list(task.decisions).filter((row) => !row.answer && row.state !== "resolved")) items.push({ task, decision });
    const delivery = deliveryStatus(task);
    if (["ready", "needs-checks", "blocked-by-dependencies", "review-findings", "review-not-run"].includes(delivery)) items.push({ task, delivery });
  }
  clear(target);
  for (const item of items) {
    const link = el("a", "attention-item");
    link.href = hashForTask(taskKey(item.task), projectRecord(item.task).id);
    link.append(chip(item.decision ? "decision" : "delivery", item.decision ? "red" : "green"));
    link.append(el("strong", "", item.task.title || item.task.id));
    link.append(el("span", "", item.decision?.question || item.decision?.summary || titleCase(item.delivery)));
    target.append(link);
  }
  panel.hidden = items.length === 0;
  attentionItemCount = items.length;
  panel.hidden = attentionItemCount === 0 || agentsViewActive;
  document.querySelector("#attention-count").textContent = `${items.length} actionable`;
}

function renderDomains() {
  const target = document.querySelector("#domains");
  clear(target);
  const domains = list(currentPayload?.snapshot?.domains?.records);
  for (const domain of domains) {
    const row = el("article", "side-row");
    const coordinator = object(domain.coordinator);
    const observation = object(domain.observation);
    row.append(el("strong", "", domain.domain_id || domain.scope || "unnamed domain"));
    row.append(el("span", "", [coordinator.qualified_id || coordinator.id || coordinator.runtime_home, observation.partial ? "partial" : "current", domain.counts?.useful_tasks != null && `${domain.counts.useful_tasks} tasks`].filter(Boolean).join(" · ")));
    const pending = domain.counts?.undelivered_outcomes ?? domain.undelivered_outcomes ?? domain.pending_outcomes;
    if (pending) row.append(chip(`${pending} undelivered`, "red"));
    if (observation.reason) row.append(el("span", "", observation.reason));
    if (observation.partial) row.append(chip("partial", "amber"));
    const workflows = list(domain.workflow_runs);
    if (workflows.length) {
      row.append(el("span", "", workflows.map((run) => `${run.workflow || run.id}: ${run.current_stage || run.status || "unknown"}`).join(" · ")));
    }
    target.append(row);
  }
  const issues = normalizedTasks(portfolio).filter((task) => {
    const observation = object(task.allocation?.observation);
    return observation.recovery_issue || observation.issue || ["retained", "uncertain"].includes(observation.state);
  });
  for (const task of issues) {
    const row = el("a", "side-row");
    const observation = object(task.allocation?.observation);
    row.href = hashForTask(taskKey(task), projectRecord(task).id);
    row.append(el("strong", "", task.title || task.id), el("span", "", observation.recovery_issue || observation.issue || `${observation.state} allocation`));
    target.append(row);
  }
  if (!domains.length && !issues.length) target.append(el("p", "empty-inline", "No domain or allocation exceptions are reported."));
}

function renderArtifacts() {
  const target = document.querySelector("#artifacts");
  clear(target);
  for (const artifact of list(currentPayload?.artifacts)) {
    const row = el("div", "side-row artifact-row");
    row.append(artifactLink(artifact.label, artifact.url, artifact.kind), el("span", "", artifact.kind));
    target.append(row);
  }
  for (const review of list(currentPayload?.snapshot?.vplan_reviews?.records).filter((record) => record.pid_alive && record.url)) {
    const row = el("div", "side-row artifact-row");
    row.append(artifactLink(review.artifact || "Open vplan review", review.url, "live review", false), el("span", "", "live review"));
    target.append(row);
  }
  if (!target.children.length) target.append(el("p", "empty-inline", "No browsable artifacts are present."));
}

function currentAgeSeconds() {
  if (Number.isFinite(observationAgeMs)) return (observationAgeMs + (observationAgeReadAt ? Date.now() - observationAgeReadAt : 0)) / 1000;
  return generatedAt ? (Date.now() - generatedAt) / 1000 : NaN;
}

function renderFreshness() {
  const freshness = object(portfolio?.freshness);
  const age = Math.max(Number(freshness.age_seconds) || 0, currentAgeSeconds());
  const sourceStatus = serviceCache === "stale" ? "stale" : freshness.status;
  const status = lastError ? "error" : age > staleAfterSeconds && sourceStatus === "fresh" ? "stale" : sourceStatus || "unknown";
  const partial = freshness.partial === true;
  const banner = document.querySelector("#state-banner");
  const ageChip = document.querySelector("#snapshot-age");
  ageChip.textContent = `observed ${ageText(age)} ago`;
  ageChip.className = `age-chip tone-${statusTone(status)}`;
  banner.className = `state-banner ${status}`;
  if (lastError) banner.textContent = `Refresh failed. Showing the last good snapshot (${ageText(age)} old): ${lastError}`;
  else if (partial) banner.textContent = `Partial observation · ${list(freshness.reasons).join(" · ") || "one or more sources are unavailable"}`;
  else if (status === "stale") banner.textContent = `Stale snapshot · last observation ${ageText(age)} ago`;
  else if (status === "unknown" || status === "unavailable") banner.textContent = "Observation freshness is unavailable; task states remain unknown where evidence is missing.";
  else banner.textContent = "Shared projection current";
  banner.hidden = ["current", "fresh"].includes(status) && !partial;
}

function render(payload) {
  const focusKey = stableFocusKey();
  currentPayload = payload;
  portfolio = portfolioFrom(payload);
  const timestamp = Date.parse(portfolio.observed_at || portfolio.generated || payload.snapshot?.generated);
  generatedAt = Number.isFinite(timestamp) ? timestamp : null;
  const tasks = normalizedTasks(portfolio);
  updateFilterOptions(tasks);
  renderCounts();
  renderAttention();
  renderTasks();
  renderAgents(currentPayload?.snapshot || currentPayload || {});
  renderDomains();
  renderArtifacts();
  renderFreshness();
  document.querySelector("#doctor-button").hidden = payload.snapshot?.later_feeds?.doctor?.available !== true;
  restoreFocus(focusKey);
  applyHash(false);
}

function agentSearchMatches(node, query) {
  if (!query) return false;
  return [node.id, node.title, node.home, node.role, node.state, node.freshness, node.provider, node.session]
    .filter(Boolean).join(" ").toLowerCase().includes(query);
}

function agentOwnerLabel(home) {
  if (!home) return "owner home unknown";
  const parts = home.split(/[\\/]/).filter(Boolean);
  return parts.length > 2 ? `…/${parts.slice(-2).join("/")}` : home;
}

function agentCard(node, isRoot = false) {
  const coordinator = node.role === "sub-orchestrator";
  const card = isRoot ? el("div", "agent-card root-card") : el("button", `agent-card${coordinator ? " coordinator" : ""}${node.unresolved ? " unresolved-card" : ""}`);
  card.dataset.agentCardKey = node.key;
  if (!isRoot) {
    card.type = "button";
    card.setAttribute("aria-pressed", String(selectedAgentKey === node.key));
    card.title = `${node.id}\nQualified task: ${node.key}\nOwner: ${node.home || "unknown"}\nOpen this exact assignment in Tasks`;
    card.addEventListener("click", () => {
      selectedAgentKey = node.key;
      showAgentTask(node.task);
    });
  } else {
    card.tabIndex = 0;
    card.setAttribute("role", "group");
    card.setAttribute("aria-label", "Main orchestrator; session health is not observed by this snapshot");
  }
  card.append(
    el("span", "agent-card-role", isRoot ? "Main orchestrator" : node.role),
    el("strong", "agent-card-title", node.title || node.id),
  );
  if (!isRoot && node.title && node.title !== node.id) card.append(el("span", "agent-card-scope", node.id));
  const facts = el("div", "agent-card-facts");
  if (isRoot) {
    facts.append(chip("health unknown", "neutral"), chip(`snapshot ${node.freshness}`, node.freshness));
    card.append(facts, el("span", "agent-card-owner", node.home ? `Home · ${agentOwnerLabel(node.home)}` : "Home identity unavailable"));
    card.title = node.home || "Root home identity unavailable";
    card.append(el("span", "agent-card-scope", "Session not observed · no health inferred from child work."));
    return card;
  }
  facts.append(chip(node.state || "unknown", statusTone(String(node.state || "unknown").toLowerCase())), chip(node.freshness || "freshness unknown", statusTone(String(node.freshness || "unknown").toLowerCase())));
  facts.append(chip(node.provider || "provider not recorded", "neutral"));
  card.title = `${card.title}\nSession details: ${node.sessionDetails || node.session}`;
  card.append(facts, el("span", "agent-card-owner", `${agentOwnerLabel(node.home)} · Session: ${node.session}`));
  if (node.domain?.coordinator?.runtime_home) {
    const runtime = node.domain.coordinator.runtime_home;
    const validated = node.domain.coordinator.validated_home === runtime;
    card.title += `\nRuntime home ${validated ? "validated" : "unvalidated"}: ${runtime}`;
  }
  return card;
}

function agentBranch(node, graph, matching, ancestors) {
  const branch = el("div", `agent-branch${node.unresolved ? " agent-unresolved-branch" : ""}`);
  branch.dataset.agentBranchKey = node.key;
  const isRoot = node.kind === "root";
  const isMatch = matching.has(node.key);
  if (!isRoot && agentSearchValue && !isMatch && !ancestors.has(node.key)) branch.classList.add("agent-dimmed");
  if (isMatch) branch.classList.add("agent-match");
  const row = el("div", "agent-card-row");
  row.append(agentCard(node, isRoot));
  const children = node.children || [];
  const collapsible = children.length > 0 && !isRoot;
  const collapsed = !isRoot && collapsedAgentKeys.has(node.key) && !ancestors.has(node.key);
  if (collapsible) {
    const toggle = el("button", "agent-collapse", `${collapsed ? "+" : "−"}${children.length}`);
    toggle.type = "button";
    toggle.setAttribute("aria-expanded", String(!collapsed));
    toggle.setAttribute("aria-label", `${collapsed ? "Expand" : "Collapse"} ${children.length} child assignments for ${node.id}`);
    toggle.dataset.agentCollapseKey = node.key;
    toggle.addEventListener("click", () => {
      if (collapsedAgentKeys.has(node.key)) collapsedAgentKeys.delete(node.key);
      else collapsedAgentKeys.add(node.key);
      renderAgents(currentPayload?.snapshot || currentPayload || {});
    });
    row.append(toggle);
  }
  branch.append(row);
  if (children.length) {
    const childGroup = el("div", "agent-children");
    childGroup.id = `agent-children-${encodeURIComponent(node.key)}`;
    childGroup.setAttribute("role", "group");
    childGroup.hidden = !isRoot && collapsed;
    if (collapsible) row.lastChild.setAttribute("aria-controls", childGroup.id);
    const nextAncestors = new Set(ancestors);
    nextAncestors.add(node.key);
    for (const child of children) childGroup.append(agentBranch(child, graph, matching, nextAncestors));
    branch.append(childGroup);
  }
  if (node.unresolved) branch.append(el("span", "agent-unresolved-reason", node.unresolved));
  return branch;
}

function drawAgentConnectors(graph) {
  if (!graph) return;
  const canvas = document.querySelector("#agents-canvas");
  const viewport = document.querySelector("#agents-viewport");
  const svg = document.querySelector("#agents-connectors");
  svg.replaceChildren();
  svg.setAttribute("width", "1");
  svg.setAttribute("height", "1");
  svg.setAttribute("viewBox", "0 0 1 1");
  const cards = [...canvas.querySelectorAll("[data-agent-card-key]")]
    .filter((card) => !card.closest("[hidden]"));
  const byKey = new Map(cards.map((card) => [card.dataset.agentCardKey, card]));
  const rect = canvas.getBoundingClientRect();
  const width = Math.max(viewport.clientWidth - 2, canvas.scrollWidth, 1);
  const height = Math.max(canvas.scrollHeight, 1);
  svg.setAttribute("width", String(width));
  svg.setAttribute("height", String(height));
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.replaceChildren();
  const svgNamespace = "http:" + "//www.w3.org/2000/svg";
  for (const node of graph.nodes) {
    if (node.unresolved || !node.parentKey) continue;
    const parent = byKey.get(node.parentKey);
    const child = byKey.get(node.key);
    if (!parent || !child) continue;
    const parentRect = parent.getBoundingClientRect();
    const childRect = child.getBoundingClientRect();
    const x1 = parentRect.left + parentRect.width / 2 - rect.left;
    const y1 = parentRect.bottom - rect.top;
    const x2 = childRect.left + childRect.width / 2 - rect.left;
    const y2 = childRect.top - rect.top;
    const bend = Math.max(12, Math.min(38, (y2 - y1) / 2));
    const path = document.createElementNS(svgNamespace, "path");
    path.setAttribute("d", `M ${x1} ${y1} C ${x1} ${y1 + bend}, ${x2} ${y2 - bend}, ${x2} ${y2}`);
    path.setAttribute("class", node.role === "sub-orchestrator" ? "agent-link coordinator-link" : "agent-link");
    svg.append(path);
  }
}

function renderAgents(snapshot) {
  if (!agentsGraphApi) return;
  const viewport = document.querySelector("#agents-viewport");
  const scrollLeft = viewport.scrollLeft;
  const scrollTop = viewport.scrollTop;
  const focusedKey = document.activeElement?.dataset?.agentCardKey || null;
  const focusedCollapseKey = document.activeElement?.dataset?.agentCollapseKey || null;
  const graph = agentsGraphApi.normalize({ ...object(snapshot), portfolio: portfolio || {} });
  normalizedAgentGraph = graph;
  const layout = agentsGraphApi.layout(graph);
  const query = document.querySelector("#agent-search").value.trim().toLowerCase();
  const searchChanged = query !== lastRenderedAgentSearch;
  lastRenderedAgentSearch = query;
  agentSearchValue = query;
  const matching = new Set(graph.nodes.filter((node) => agentSearchMatches(node, query)).map((node) => node.key));
  const ancestors = new Set();
  const byKey = new Map(graph.nodes.map((node) => [node.key, node]));
  for (const key of matching) {
    let node = byKey.get(key);
    while (node?.parentKey && node.parentKey !== graph.rootKey) {
      ancestors.add(node.parentKey);
      node = byKey.get(node.parentKey);
    }
  }
  const tree = document.querySelector("#agents-tree");
  clear(tree);
  tree.append(agentBranch(graph.root, graph, matching, ancestors));
  if (graph.nodes.length === 0) tree.append(el("p", "agent-empty", "No assignments in the bounded task projection."));
  const unresolvedPanel = document.querySelector("#agents-unresolved-panel");
  const unresolvedList = document.querySelector("#agents-unresolved");
  clear(unresolvedList);
  unresolvedPanel.hidden = graph.unresolved.length === 0;
  for (const node of graph.unresolved) unresolvedList.append(agentBranch(node, graph, matching, ancestors));
  const coordinators = graph.nodes.filter((node) => node.role === "sub-orchestrator").length;
  const searchSummary = query ? ` · ${matching.size} match${matching.size === 1 ? "" : "es"}${matching.size ? "" : " · no matching assignments"}` : "";
  const assignmentCount = graph.truncated ? `${graph.nodes.length} of ${graph.total} assignments shown` : `${graph.nodes.length} assignments`;
  const summary = `${assignmentCount} · ${coordinators} coordinators · root session not observed${graph.partial ? " · projection partial" : " · projection complete"}${searchSummary}`;
  document.querySelector("#agents-summary").textContent = summary;
  const warning = document.querySelector("#agents-warning");
  const warnings = [...graph.partialReasons];
  if (graph.unresolved.length) warnings.push(`${graph.unresolved.length} assignment${graph.unresolved.length === 1 ? "" : "s"} with unresolved ownership`);
  warning.hidden = warnings.length === 0;
  warning.textContent = warnings.join(" · ");
  const expandButton = document.querySelector("#agents-expand");
  const collapseButton = document.querySelector("#agents-collapse");
  const branches = graph.nodes.filter((node) => node.role === "sub-orchestrator" && node.children.length);
  expandButton.disabled = branches.length === 0 || branches.every((node) => !collapsedAgentKeys.has(node.key));
  collapseButton.disabled = branches.length === 0 || branches.every((node) => collapsedAgentKeys.has(node.key));
  const rootButton = document.querySelector("#agents-root");
  rootButton.disabled = false;
  requestAnimationFrame(() => {
    drawAgentConnectors(graph);
    viewport.scrollLeft = scrollLeft;
    viewport.scrollTop = scrollTop;
    if (focusedKey && !document.querySelector("#agents-view").hidden) {
      [...document.querySelectorAll("[data-agent-card-key]")]
        .find((card) => card.dataset.agentCardKey === focusedKey)
        ?.focus({ preventScroll: true });
    } else if (focusedCollapseKey && !document.querySelector("#agents-view").hidden) {
      [...document.querySelectorAll("[data-agent-collapse-key]")]
        .find((button) => button.dataset.agentCollapseKey === focusedCollapseKey)
        ?.focus({ preventScroll: true });
    }
  });
  if (searchChanged && query && matching.size) {
    const selected = graph.nodes.find((node) => matching.has(node.key));
    if (selected && layout.depth.has(selected.key)) {
      requestAnimationFrame(() => [...document.querySelectorAll("[data-agent-card-key]")]
        .find((card) => card.dataset.agentCardKey === selected.key)
        ?.scrollIntoView({ block: "nearest", inline: "nearest" }));
    }
  }
}

function showAgentTask(task) {
  if (!task) return;
  for (const control of controls) control.value = "";
  document.querySelector("#search").value = "";
  document.querySelector("#tasks-view-button").click();
  renderTasks();
  const key = taskKey(task);
  const row = [...document.querySelectorAll(".task-row")].find((candidate) => candidate.dataset.taskId === key);
  if (row) {
    expanded.add(key);
    row.open = true;
    setHashForTask(task);
    requestAnimationFrame(() => row.scrollIntoView({ block: "center" }));
    announce(`Opened task details for ${task.id || key}`);
  } else announce(`Task ${task.id || key} is not present in the current task projection`);
}

function centerAgentRoot() {
  const viewport = document.querySelector("#agents-viewport");
  const rootCard = document.querySelector("#agents-tree [data-agent-card-key]");
  if (!rootCard) return;
  const viewportRect = viewport.getBoundingClientRect();
  const cardRect = rootCard.getBoundingClientRect();
  viewport.scrollLeft += cardRect.left + cardRect.width / 2 - (viewportRect.left + viewport.clientWidth / 2);
}

function setAgentsView(agentsVisible) {
  const agents = document.querySelector("#agents-view");
  const tasks = document.querySelector("#tasks-view");
  const agentsButton = document.querySelector("#agents-view-button");
  const tasksButton = document.querySelector("#tasks-view-button");
  const wasAgentsVisible = !agents.hidden;
  agents.hidden = !agentsVisible;
  tasks.hidden = agentsVisible;
  agentsViewActive = agentsVisible;
  document.querySelector("#attention-panel").hidden = agentsVisible || attentionItemCount === 0;
  agentsButton.classList.toggle("selected", agentsVisible);
  agentsButton.setAttribute("aria-pressed", String(agentsVisible));
  tasksButton.classList.toggle("selected", !agentsVisible);
  tasksButton.setAttribute("aria-pressed", String(!agentsVisible));
  requestAnimationFrame(() => {
    if (agentsVisible) {
      drawAgentConnectors(normalizedAgentGraph);
      agents.scrollIntoView({ block: "start" });
      if (!wasAgentsVisible) centerAgentRoot();
    } else tasks.scrollIntoView({ block: "start" });
  });
}

function setConnected(message = "Live") {
  connectionNote.textContent = message;
  connectionNote.classList.remove("bad");
  liveDot.classList.remove("disconnected");
}

function readObservationHeaders(response) {
  const raw = response.headers.get("X-Multplx-Observation-Age-Ms");
  observationAgeMs = raw !== null && Number.isFinite(Number(raw)) ? Number(raw) : null;
  observationAgeReadAt = Date.now();
  const cache = response.headers.get("X-Multplx-Cache");
  serviceCache = cache || serviceCache;
  const refresh = response.headers.get("X-Multplx-Refresh");
  lastError = response.headers.get("X-Multplx-Refresh-Error");
  return [cache, refresh].filter(Boolean).join(" · ");
}

async function poll() {
  if (pollInFlight) return;
  pollInFlight = true;
  const headers = meaningfulHash ? { "If-None-Match": meaningfulHash } : {};
  try {
    const response = await fetch("/api/state", { headers, cache: "no-store" });
    const serviceState = readObservationHeaders(response);
    if (response.status === 304) {
      if (portfolio) renderFreshness();
      setConnected(serviceState || "Live · unchanged");
      return;
    }
    if (!response.ok) throw new Error(`snapshot request returned ${response.status}`);
    const payload = await response.json();
    meaningfulHash = response.headers.get("ETag") || response.headers.get("X-Multplx-Snapshot-Hash");
    render(payload);
    setConnected(serviceState || "Live");
  } catch (error) {
    lastError = error.message;
    connectionNote.textContent = `Connection lost · ${error.message}`;
    connectionNote.classList.add("bad");
    liveDot.classList.add("disconnected");
    if (portfolio) renderFreshness();
    else {
      const banner = document.querySelector("#state-banner");
      banner.className = "state-banner error";
      banner.textContent = `Dashboard unavailable: ${error.message}`;
    }
  } finally {
    pollInFlight = false;
    schedulePoll();
  }
}

function schedulePoll(delay = document.hidden ? hiddenPollMs : pollMs) {
  clearTimeout(pollTimer);
  pollTimer = setTimeout(poll, delay);
}

function applyHash(forceScroll = true) {
  if (!portfolio) return;
  if (!forceScroll && lastAppliedHash === location.hash) return;
  const params = new URLSearchParams(location.hash.slice(1));
  const project = params.get("project");
  const projectSelect = document.querySelector("#project-filter");
  const projectChanged = Boolean(project && projectSelect.value !== project && [...projectSelect.options].some((option) => option.value === project));
  if (projectChanged) projectSelect.value = project;
  const id = params.get("task");
  lastAppliedHash = location.hash;
  if (!id) { if (project) renderTasks(); return; }
  expanded.add(id);
  if (projectChanged) renderTasks();
  const row = [...document.querySelectorAll(".task-row")].find((node) => node.dataset.taskId === id);
  if (row) {
    row.open = true;
    if (forceScroll) requestAnimationFrame(() => row.scrollIntoView({ block: "nearest" }));
  }
}

function taskSummaries() {
  return [...document.querySelectorAll(".task-row > summary")];
}

function moveTaskFocus(current, offset) {
  const rows = taskSummaries();
  const index = rows.indexOf(current);
  if (index < 0) return;
  rows[Math.max(0, Math.min(rows.length - 1, index + offset))]?.focus();
}

for (const control of controls) control.addEventListener("input", renderTasks);
document.querySelector("#filters").addEventListener("submit", (event) => event.preventDefault());
document.querySelector("#clear-filters").addEventListener("click", () => {
  for (const control of controls) control.value = "";
  renderTasks();
  document.querySelector("#search").focus();
});
document.querySelector("#tasks-view-button").addEventListener("click", () => setAgentsView(false));
document.querySelector("#agents-view-button").addEventListener("click", () => setAgentsView(true));
document.querySelector("#agent-search").addEventListener("input", () => renderAgents(currentPayload?.snapshot || currentPayload || {}));
document.querySelector("#agents-expand").addEventListener("click", () => {
  for (const node of normalizedAgentGraph?.nodes || []) {
    if (node.role === "sub-orchestrator") collapsedAgentKeys.delete(node.key);
  }
  renderAgents(currentPayload?.snapshot || currentPayload || {});
});
document.querySelector("#agents-collapse").addEventListener("click", () => {
  for (const node of normalizedAgentGraph?.nodes || []) {
    if (node.role === "sub-orchestrator" && node.children.length) collapsedAgentKeys.add(node.key);
  }
  renderAgents(currentPayload?.snapshot || currentPayload || {});
});
document.querySelector("#agents-root").addEventListener("click", () => {
  setAgentsView(true);
  const viewport = document.querySelector("#agents-viewport");
  viewport.scrollTop = 0;
  centerAgentRoot();
  document.querySelector("#agents-tree [data-agent-card-key]")?.focus({ preventScroll: true });
});
document.querySelector("#agent-search").addEventListener("input", () => {
  const summary = document.querySelector("#agents-summary").textContent;
  announce(summary);
});
document.querySelector("#agents-viewport").addEventListener("keydown", (event) => {
  if (!["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
  const cards = [...document.querySelectorAll("#agents-view [data-agent-card-key]")]
    .filter((card) => !card.closest("[hidden]") && !card.closest(".agent-dimmed"));
  const current = cards.indexOf(event.target.closest("[data-agent-card-key]"));
  if (current < 0) return;
  event.preventDefault();
  const step = event.key === "ArrowUp" || event.key === "ArrowLeft" ? -1 : 1;
  const target = cards[Math.max(0, Math.min(cards.length - 1, current + step))];
  target?.focus();
  target?.scrollIntoView({ block: "nearest", inline: "nearest" });
});
document.querySelector("#task-list").addEventListener("keydown", (event) => {
  if (!event.target.matches(".task-row > summary")) return;
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    moveTaskFocus(event.target, event.key === "ArrowDown" ? 1 : -1);
  }
});
document.addEventListener("keydown", (event) => {
  if (event.key === "/" && !/input|select|textarea/i.test(event.target.tagName)) {
    event.preventDefault();
    (document.querySelector("#agents-view").hidden ? document.querySelector("#search") : document.querySelector("#agent-search")).focus();
  }
  if (event.key === "Escape" && document.activeElement === document.querySelector("#agent-search") && document.querySelector("#agent-search").value) {
    document.querySelector("#agent-search").value = "";
    renderAgents(currentPayload?.snapshot || currentPayload || {});
  }
  if (event.key === "Escape" && document.activeElement === document.querySelector("#search") && document.querySelector("#search").value) {
    document.querySelector("#search").value = "";
    renderTasks();
  }
});
window.addEventListener("hashchange", () => { applyHash(true); renderTasks(); });
document.addEventListener("visibilitychange", () => schedulePoll(document.hidden ? hiddenPollMs : 0));

function safeLinkTarget(target) {
  try {
    const url = new URL(target, window.location.href);
    return ["http" + ":", "https" + ":"].includes(url.protocol) ? url.href : null;
  } catch { return null; }
}

function appendInlineMarkdown(parent, source) {
  const tokenPattern = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*|\[[^\]]+\]\([^)]+\))/g;
  let offset = 0;
  for (const match of source.matchAll(tokenPattern)) {
    if (match.index > offset) parent.append(document.createTextNode(source.slice(offset, match.index)));
    const token = match[0];
    if (token.startsWith("`")) parent.append(el("code", "", token.slice(1, -1)));
    else if (token.startsWith("**")) parent.append(el("strong", "", token.slice(2, -2)));
    else if (token.startsWith("*")) parent.append(el("em", "", token.slice(1, -1)));
    else {
      const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token);
      const href = safeLinkTarget(link?.[2]);
      if (link && href) {
        const anchor = el("a", "", link[1]);
        anchor.href = href;
        anchor.target = "_blank";
        anchor.rel = "noreferrer";
        parent.append(anchor);
      } else parent.append(document.createTextNode(link?.[1] || token));
    }
    offset = match.index + token.length;
  }
  if (offset < source.length) parent.append(document.createTextNode(source.slice(offset)));
}

function renderMarkdown(source) {
  const holder = el("div", "markdown-body");
  const lines = String(source).replace(/\r\n/g, "\n").split("\n");
  let code = null;
  let codeText = [];
  let listNode = null;
  let listType = null;
  for (const line of lines) {
    if (/^```/.test(line)) {
      listNode = null;
      if (code) { code.textContent = codeText.join("\n"); code = null; codeText = []; }
      else { const pre = el("pre"); code = el("code"); pre.append(code); holder.append(pre); }
      continue;
    }
    if (code) { codeText.push(line); continue; }
    if (!line.trim()) { listNode = null; continue; }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      listNode = null;
      const node = el(`h${heading[1].length}`);
      appendInlineMarkdown(node, heading[2]);
      holder.append(node);
      continue;
    }
    const item = /^(\s*)([-*]|\d+\.)\s+(.*)$/.exec(line);
    if (item) {
      const nextType = item[2].endsWith(".") ? "ol" : "ul";
      if (!listNode || listType !== nextType) { listType = nextType; listNode = el(nextType); holder.append(listNode); }
      const node = el("li"); appendInlineMarkdown(node, item[3]); listNode.append(node); continue;
    }
    listNode = null;
    const quote = /^>\s?(.*)$/.exec(line);
    const node = el(quote ? "blockquote" : "p");
    appendInlineMarkdown(node, quote ? quote[1] : line);
    holder.append(node);
  }
  if (code) code.textContent = codeText.join("\n");
  return holder;
}

function prepareDialog(title, subtitle = "", fullscreen = false) {
  document.querySelector("#dialog-title").textContent = title;
  document.querySelector("#dialog-subtitle").textContent = subtitle;
  dialog.classList.toggle("dialog-fullscreen", fullscreen);
  if (!dialog.open) dialog.showModal();
}

async function openArtifact(label, url) {
  prepareDialog(label, url, true);
  const body = document.querySelector("#dialog-body");
  clear(body);
  if (/\.html?(?:\?|$)/i.test(url)) {
    const frame = el("iframe", "artifact-frame");
    frame.title = label;
    frame.setAttribute("sandbox", "");
    frame.src = url;
    body.append(frame);
    return;
  }
  body.append(el("p", "empty-inline", "Loading artifact…"));
  try {
    const response = await fetch(url, { cache: "no-store" });
    if (!response.ok) throw new Error(`artifact returned ${response.status}`);
    const text = await response.text();
    clear(body);
    if (/\.md(?:\?|$)/i.test(url)) body.append(renderMarkdown(text));
    else { const pre = el("pre"); pre.textContent = text; body.append(pre); }
  } catch (error) { clear(body); body.append(el("p", "state-banner error", error.message)); }
}

document.querySelector("#doctor-button").addEventListener("click", async () => {
  prepareDialog("Doctor summary");
  const body = document.querySelector("#dialog-body");
  body.replaceChildren(el("p", "empty-inline", "Running an explicit read-only invariant sweep…"));
  try {
    const response = await fetch("/api/doctor", { cache: "no-store" });
    const payload = await response.json();
    if (!response.ok) throw new Error(payload.error || `doctor returned ${response.status}`);
    const pre = el("pre"); pre.textContent = JSON.stringify(payload, null, 2); body.replaceChildren(pre);
  } catch (error) { body.replaceChildren(el("p", "state-banner error", error.message)); }
});

document.querySelector("#dialog-close").addEventListener("click", () => dialog.close());
dialog.addEventListener("click", (event) => {
  const rect = dialog.getBoundingClientRect();
  const inside = event.clientX >= rect.left && event.clientX <= rect.right && event.clientY >= rect.top && event.clientY <= rect.bottom;
  if (!inside) dialog.close();
});
dialog.addEventListener("close", () => { dialog.classList.remove("dialog-fullscreen"); document.querySelector("#dialog-subtitle").textContent = ""; });

setInterval(() => { if (portfolio) renderFreshness(); }, 1000);
poll();

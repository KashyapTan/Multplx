"use strict";

const pollMs = Number(document.querySelector('meta[name="mx-viz-poll-ms"]')?.content || 2500);
const connectionNote = document.querySelector("#connection-note");
const dialog = document.querySelector("#detail-dialog");
let currentHash = null;
let currentSnapshot = null;
let generatedAt = null;

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = String(text);
  return node;
};

const clear = (node) => node.replaceChildren();
const list = (value) => Array.isArray(value) ? value : [];
const valueOr = (value, fallback = "—") => value === null || value === undefined || value === "" ? fallback : value;
const ageText = (seconds) => {
  if (!Number.isFinite(seconds)) return "unknown";
  if (seconds < 60) return `${Math.max(0, Math.floor(seconds))}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
};

function badge(text, className = "") {
  return el("span", `badge ${className}`.trim(), text);
}

function sourceClass(source) {
  if (source === "native-event") return "authoritative";
  if (source === "validated-report" || source === "status-log" || source === "run-step") return "report";
  if (source === "pane") return "heuristic";
  return "error";
}

function detailList(rows) {
  const dl = el("dl", "detail-list");
  for (const [label, value] of rows) {
    const dt = el("dt", "", label);
    const dd = el("dd");
    if (value instanceof Node) dd.append(value); else dd.textContent = valueOr(value);
    dl.append(dt, dd);
  }
  return dl;
}

function healthItem(label, value, detail, tone = "") {
  const item = el("div", "health-item");
  item.append(el("span", "health-label", label), el("strong", `health-value ${tone}`.trim(), value));
  if (detail) item.append(el("span", "health-detail", detail));
  return item;
}

function renderHealth(snapshot) {
  const target = document.querySelector("#health-strip");
  clear(target);
  const generated = Date.parse(snapshot.generated);
  generatedAt = Number.isFinite(generated) ? generated : null;
  const age = generatedAt ? (Date.now() - generatedAt) / 1000 : Infinity;
  const freshnessTone = age > 60 ? "bad" : age > 10 ? "warn" : "good";
  target.append(healthItem("Snapshot", ageText(age), snapshot.generated || "generation time unavailable", freshnessTone));

  const watcher = snapshot.watcher || {};
  let watcherValue = "DOWN";
  let watcherTone = "bad";
  if (watcher.alive && watcher.stale) { watcherValue = "STALE"; watcherTone = "warn"; }
  else if (watcher.alive) { watcherValue = "LIVE"; watcherTone = "good"; }
  target.append(healthItem("Watcher", watcherValue, `beacon ${ageText(watcher.beacon_age_secs)} · identity ${watcher.identity_verified ? "verified" : "unverified"}`, watcherTone));
  target.append(healthItem("Away mode", watcher.afk ? "AFK" : "Present", watcher.afk ? "sub-supervisor may own routine wakes" : "normal supervision", watcher.afk ? "warn" : "good"));

  const headroom = snapshot.headroom;
  if (headroom) {
    const item = healthItem("Headroom", `${headroom.in_use}/${headroom.capacity}`, `${headroom.available} dispatch slot(s) available`, headroom.at_limit ? "bad" : "good");
    const gauge = el("div", "gauge");
    const fill = el("span");
    fill.style.width = `${headroom.capacity > 0 ? Math.min(100, (headroom.in_use / headroom.capacity) * 100) : 100}%`;
    gauge.append(fill);
    item.append(gauge);
    target.append(item);
  } else {
    target.append(healthItem("Headroom", "UNKNOWN", snapshot.headroom_reason || "capacity check unavailable", "warn"));
  }
  target.append(healthItem("Wake queue", snapshot.wake_queue?.depth ?? "?", `oldest ${ageText(snapshot.wake_queue?.oldest_age_secs)}`, snapshot.wake_queue?.depth ? "warn" : "good"));
  target.append(healthItem("Dispatch queue", snapshot.dispatch_queue?.depth ?? "?", snapshot.dispatch_queue?.available === false ? snapshot.dispatch_queue.reason : "FIFO parked requests", snapshot.dispatch_queue?.depth ? "warn" : "good"));
}

function artifactLink(label, url) {
  const link = el("a", "", label);
  link.href = url;
  link.target = "_blank";
  link.rel = "noreferrer";
  return link;
}

function renderTaskCard(task, timelineAvailable) {
  const card = el("article", "task-card");
  const top = el("div", "task-top");
  top.append(el("span", "task-id", task.id), badge(task.current_state?.state || "unknown", task.current_state?.state === "failed" ? "error" : ""));
  card.append(top);
  const badges = el("div", "badges");
  badges.append(badge(task.kind || "task"), badge(task.backend || "unknown"));
  badges.append(badge(`source: ${task.current_state?.source || "none"}`, sourceClass(task.current_state?.source)));
  if (list(task.hints?.open_decisions).length) badges.append(badge(`${task.hints.open_decisions.length} decision`, "error"));
  card.append(badges);
  card.append(detailList([
    ["Harness", valueOr(task.harness)],
    ["Project", valueOr(task.project)],
    ["Endpoint", task.endpoint?.exists === true ? "present" : task.endpoint?.exists === false ? "absent" : "unknown"],
    ["Doing", valueOr(task.current_state?.detail)],
  ]));
  const event = task.paths?.status_log?.last_event?.raw;
  if (event) card.append(el("p", "historical", `Last event (historical): ${event}`));
  const links = el("div", "links");
  if (task.pr?.url) links.append(artifactLink("Pull request ↗", task.pr.url));
  if (task.paths?.report?.present) {
    const relative = `${encodeURIComponent(task.id)}/report.md`;
    links.append(artifactLink("Report", `/artifact/data/${relative}`));
  }
  if (timelineAvailable) {
    const button = el("button", "secondary", "Timeline");
    button.type = "button";
    button.addEventListener("click", () => showTimeline(task.id));
    links.append(button);
  }
  if (links.childElementCount) card.append(links);
  return card;
}

function renderTasks(snapshot) {
  const board = document.querySelector("#tasks-board");
  clear(board);
  const tasks = list(snapshot.tasks);
  document.querySelector("#task-count").textContent = `${tasks.length} task${tasks.length === 1 ? "" : "s"}`;
  const groups = [
    ["Actors", tasks.filter((task) => task.kind !== "daemon")],
    ["Daemons", tasks.filter((task) => task.kind === "daemon")],
  ];
  for (const [title, rows] of groups) {
    if (!rows.length) continue;
    const group = el("div", "group");
    const heading = el("div", "group-title");
    heading.append(el("span", "", title), el("span", "", rows.length));
    const grid = el("div", "card-grid");
    for (const task of rows) grid.append(renderTaskCard(task, snapshot.later_feeds?.timeline?.available));
    group.append(heading, grid);
    board.append(group);
  }
  if (!tasks.length) board.append(el("p", "empty", "No live task metadata is present."));

  const daemonRows = list(snapshot.daemon_current?.records);
  if (daemonRows.length) {
    const group = el("div", "group");
    const heading = el("div", "group-title");
    heading.append(el("span", "", "Daemon-home summaries"), el("span", "", daemonRows.length));
    const grid = el("div", "card-grid");
    for (const daemon of daemonRows) {
      const card = el("article", "task-card");
      const top = el("div", "task-top");
      top.append(el("span", "task-id", daemon.id), badge(daemon.current?.state || "unknown"));
      card.append(top, detailList([
        ["Active children", daemon.counts?.active_children ?? 0],
        ["Open decisions", daemon.counts?.decisions_open ?? 0],
        ["Holds", daemon.counts?.holds ?? 0],
        ["Queued", daemon.counts?.queued ?? 0],
        ["Landed", daemon.counts?.landed ?? 0],
      ]));
      grid.append(card);
    }
    group.append(heading, grid);
    board.append(group);
  }
}

function renderDecisions(snapshot) {
  const target = document.querySelector("#decisions");
  clear(target);
  const rows = [];
  for (const task of list(snapshot.tasks)) {
    for (const decision of list(task.hints?.open_decisions)) rows.push({ owner: task.id, ...decision });
  }
  for (const daemon of list(snapshot.daemon_current?.records)) {
    for (const decision of list(daemon.decisions_open)) rows.push({ owner: daemon.id, ...decision });
  }
  for (const decision of rows) {
    const card = el("article", "decision");
    card.append(el("span", "decision-key", `${decision.owner} · ${decision.key || decision.verb || "decision"}`));
    card.append(el("p", "", decision.summary || decision.reason || "Maintainer input is required."));
    target.append(card);
  }
  if (!rows.length) target.append(el("p", "empty", "Nothing is waiting on a maintainer decision."));
}

function renderBacklog(snapshot) {
  const target = document.querySelector("#backlog");
  clear(target);
  const columns = el("div", "backlog-columns");
  for (const [state, title] of [["in_flight", "In flight"], ["queued", "Queued"], ["done", "Done"]]) {
    const column = el("div", "backlog-column");
    const rows = list(snapshot.backlog?.records).filter((row) => row.state === state);
    column.append(el("h3", "", `${title} · ${rows.length}`));
    for (const row of rows) {
      const text = row.structured ? `${row.id} · ${row.title || "untitled"}` : row.raw;
      column.append(el("div", "backlog-item", text));
    }
    if (!rows.length) column.append(el("p", "empty", "None"));
    columns.append(column);
  }
  target.append(columns);
  if (snapshot.main_inventory?.valid === false) {
    target.append(el("div", "warning-box", `Inventory warning: ${snapshot.main_inventory.reason}`));
  }
}

function renderArtifacts(snapshot, artifacts) {
  const target = document.querySelector("#artifacts");
  clear(target);
  const active = list(snapshot.vplan_reviews?.records).filter((record) => record.pid_alive);
  if (active.length) {
    const group = el("div", "group");
    group.append(el("div", "group-title", "Active vplan reviews"));
    for (const review of active) group.append(artifactLink(review.artifact || "Open review", review.url));
    target.append(group);
  }
  const container = el("div", "artifact-list");
  for (const artifact of list(artifacts)) {
    const row = el("div", "artifact");
    row.append(artifactLink(artifact.label, artifact.url), el("span", "artifact-kind", artifact.kind));
    container.append(row);
  }
  target.append(container);
  if (!list(artifacts).length && !active.length) target.append(el("p", "empty", "No browsable artifacts are present."));
}

function renderRecords(target, records, fields, emptyText) {
  clear(target);
  for (const record of records) {
    const card = el("article", "record");
    const top = el("div", "record-top");
    top.append(el("span", "task-id", record.id), badge(record.status || record.state || "unknown", record.valid === false ? "error" : ""));
    card.append(top, detailList(fields(record)));
    target.append(card);
  }
  if (!records.length) target.append(el("p", "empty", emptyText));
}

function renderOptionalFeeds(snapshot) {
  const feeds = snapshot.later_feeds || {};
  const gatePanel = document.querySelector("#gate-panel");
  gatePanel.hidden = feeds.gate_runs?.available !== true;
  if (!gatePanel.hidden) renderRecords(document.querySelector("#gate-runs"), list(feeds.gate_runs.records), (row) => [["Step", row.step], ["Round", row.round], ["Decision", row.pending_decision_key]], "No gate runs are recorded.");
  const workflowPanel = document.querySelector("#workflow-panel");
  workflowPanel.hidden = feeds.workflow_runs?.available !== true;
  if (!workflowPanel.hidden) renderRecords(document.querySelector("#workflow-runs"), list(feeds.workflow_runs.records), (row) => [["Workflow", row.workflow], ["Stage", row.current_stage], ["Message", row.message]], "No workflow runs are recorded.");
  const deliveryPanel = document.querySelector("#delivery-panel");
  deliveryPanel.hidden = feeds.deliveries?.available !== true;
  if (!deliveryPanel.hidden) renderRecords(document.querySelector("#deliveries"), list(feeds.deliveries.records), (row) => [["Approval", row.approval], ["Branch", row.branch], ["Age", ageText(row.age_secs)]], "No delivery handoffs are recorded.");
  const upstreamPanel = document.querySelector("#upstream-panel");
  upstreamPanel.hidden = feeds.upstream_drift?.available !== true;
  if (!upstreamPanel.hidden) {
    const target = document.querySelector("#upstream-drift");
    clear(target);
    const drift = feeds.upstream_drift;
    target.append(detailList([["Status", drift.status], ["Fork point", drift.fork_point], ["Reviewed through", drift.last_reviewed], ["Repository", drift.upstream_repo]]));
  }
  const doctor = document.querySelector("#doctor-button");
  doctor.hidden = feeds.doctor?.available !== true;
}

function render(payload) {
  currentSnapshot = payload.snapshot;
  renderHealth(payload.snapshot);
  renderTasks(payload.snapshot);
  renderDecisions(payload.snapshot);
  renderBacklog(payload.snapshot);
  renderArtifacts(payload.snapshot, payload.artifacts);
  renderOptionalFeeds(payload.snapshot);
}

async function poll() {
  const headers = currentHash ? { "If-None-Match": currentHash } : {};
  try {
    const response = await fetch("/api/state", { headers, cache: "no-store" });
    if (response.status === 304) {
      connectionNote.classList.remove("bad");
      connectionNote.textContent = `Live · unchanged · polling every ${pollMs}ms`;
      return;
    }
    if (!response.ok) throw new Error(`snapshot request returned ${response.status}`);
    const payload = await response.json();
    currentHash = response.headers.get("ETag") || response.headers.get("X-Multplx-Content-Hash");
    render(payload);
    connectionNote.classList.remove("bad");
    connectionNote.textContent = `Live · canonical snapshot · polling every ${pollMs}ms`;
  } catch (error) {
    connectionNote.textContent = `Connection lost · ${error.message}`;
    connectionNote.classList.add("bad");
  }
}

async function showTimeline(id) {
  document.querySelector("#dialog-title").textContent = `Timeline · ${id}`;
  const body = document.querySelector("#dialog-body");
  clear(body);
  body.append(el("p", "muted", "Loading the sanctioned timeline reader…"));
  dialog.showModal();
  try {
    const response = await fetch(`/api/timeline/${encodeURIComponent(id)}`, { cache: "no-store" });
    if (!response.ok) throw new Error(`timeline returned ${response.status}`);
    const payload = await response.json();
    clear(body);
    for (const record of list(payload.records)) {
      const row = el("div", "timeline-row");
      row.append(el("span", "muted", record.ts), el("span", "decision-key", `${record.source} · ${record.event}`), el("span", "", JSON.stringify(record.detail)));
      body.append(row);
    }
    if (!list(payload.records).length) body.append(el("p", "empty", "The journal has no valid events."));
  } catch (error) {
    clear(body);
    body.append(el("p", "bad", error.message));
  }
}

document.querySelector("#doctor-button").addEventListener("click", async () => {
  document.querySelector("#dialog-title").textContent = "Doctor summary";
  const body = document.querySelector("#dialog-body");
  clear(body);
  body.append(el("p", "muted", "Running an explicit read-only invariant sweep…"));
  dialog.showModal();
  try {
    const response = await fetch("/api/doctor", { cache: "no-store" });
    const payload = await response.json();
    if (!response.ok) throw new Error(payload.error || `doctor returned ${response.status}`);
    clear(body);
    const pre = el("pre", "");
    pre.textContent = JSON.stringify(payload, null, 2);
    body.append(pre);
  } catch (error) {
    clear(body);
    body.append(el("p", "bad", error.message));
  }
});
document.querySelector("#dialog-close").addEventListener("click", () => dialog.close());

poll();
setInterval(poll, pollMs);
setInterval(() => { if (currentSnapshot) renderHealth(currentSnapshot); }, 1000);

"use strict";

const pollMs = Number(document.querySelector('meta[name="mx-viz-poll-ms"]')?.content || 2500);
const connectionNote = document.querySelector("#connection-note");
const liveDot = document.querySelector("#live-dot");
const dialog = document.querySelector("#detail-dialog");
const decisionDrawer = document.querySelector("#decision-drawer");
const maintainerNode = document.querySelector("#maintainer-node");
const svgNamespace = "http:" + "//www.w3.org/2000/svg";
let currentHash = null;
let currentSnapshot = null;
let generatedAt = null;
let timelineAvailable = false;
let drawerOpen = false;
let redrawScheduled = false;

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

function announce(message) {
  document.querySelector("#aria-live").textContent = message;
}

function artifactLink(label, url) {
  const link = el("a", "", label);
  link.href = url;
  link.target = "_blank";
  link.rel = "noreferrer";
  return link;
}

function detailList(rows) {
  const dl = el("dl", "detail-list");
  for (const [label, value] of rows) {
    const dt = el("dt", "", label);
    const dd = el("dd");
    if (value instanceof Node) dd.append(value);
    else dd.textContent = valueOr(value);
    dl.append(dt, dd);
  }
  return dl;
}

function statusStyle(task) {
  if (list(task.hints?.open_decisions).length) return "blocked";
  const state = String(task.current_state?.state || "unknown").toLowerCase();
  if (["working", "running", "validating"].includes(state)) return "running";
  if (["parked", "paused", "queued", "waiting"].includes(state)) return "queued";
  if (["blocked", "needs-decision"].includes(state)) return "blocked";
  if (["failed", "error"].includes(state)) return "failed";
  if (["done", "finished", "complete", "completed"].includes(state)) return "done";
  if (["idle", "stopped"].includes(state)) return "idle";
  return "unknown";
}

function displayTasks(snapshot) {
  const tasks = [...list(snapshot.tasks)];
  const known = new Set(tasks.map((task) => task.id));
  for (const daemon of list(snapshot.daemon_current?.records)) {
    if (known.has(daemon.id)) continue;
    tasks.push({
      id: daemon.id,
      kind: "daemon",
      harness: "",
      project: daemon.home || "",
      current_state: daemon.current || { state: "unknown", source: "none" },
      hints: { open_decisions: list(daemon.decisions_open) },
      daemon_summary: daemon.counts || null,
    });
  }
  return tasks;
}

function decisionRows(snapshot) {
  const rows = [];
  const seen = new Set();
  const add = (owner, decision) => {
    const key = `${owner}\u0000${decision.key || decision.verb || "decision"}\u0000${decision.summary || decision.reason || ""}`;
    if (seen.has(key)) return;
    seen.add(key);
    rows.push({ owner, ...decision });
  };
  for (const task of list(snapshot.tasks)) {
    for (const decision of list(task.hints?.open_decisions)) add(task.id, decision);
  }
  for (const daemon of list(snapshot.daemon_current?.records)) {
    for (const decision of list(daemon.decisions_open)) add(daemon.id, decision);
  }
  return rows;
}

function renderDecisionDrawer(snapshot) {
  const rows = decisionRows(snapshot);
  const badge = document.querySelector("#decision-badge");
  const summary = document.querySelector("#maintainer-summary");
  badge.hidden = rows.length === 0;
  badge.textContent = rows.length;
  summary.textContent = rows.length === 0
    ? "no decisions pending"
    : `${rows.length} decision${rows.length === 1 ? "" : "s"} need you`;

  const target = document.querySelector("#decision-list");
  clear(target);
  if (!rows.length) {
    target.append(el("div", "drawer-empty", "Nothing is waiting on a maintainer decision."));
  } else {
    for (const decision of rows) {
      const item = el("article", "decision-item");
      const top = el("div", "decision-top");
      top.append(
        el("span", "decision-ref", decision.key || decision.verb || "decision"),
        el("span", "decision-ref", decision.owner),
      );
      item.append(
        top,
        el("p", "decision-summary", decision.summary || decision.reason || "Maintainer input is required."),
        el("span", "readonly-note", "Viewer only · respond through the ordinary Multplx workflow"),
      );
      target.append(item);
    }
  }
  decisionDrawer.hidden = !drawerOpen;
  maintainerNode.setAttribute("aria-expanded", String(drawerOpen));
}

function openDrawer() {
  drawerOpen = true;
  if (currentSnapshot) renderDecisionDrawer(currentSnapshot);
  announce("Decision viewer opened");
}

function closeDrawer() {
  drawerOpen = false;
  decisionDrawer.hidden = true;
  maintainerNode.setAttribute("aria-expanded", "false");
}

function toggleDrawer() {
  if (drawerOpen) closeDrawer();
  else openDrawer();
}

function brokerStat(label, value, dotClass = "") {
  const cell = el("div", "broker-stat");
  cell.append(el("div", "broker-stat-label", label));
  const display = el("div", "broker-stat-value");
  if (dotClass) display.append(el("span", `dot-sm ${dotClass}`));
  display.append(document.createTextNode(String(value)));
  cell.append(display);
  return cell;
}

function renderBroker(snapshot) {
  const target = document.querySelector("#broker-stats");
  clear(target);
  const watcher = snapshot.watcher || {};
  let watcherValue = "down";
  let watcherDot = "dot-red";
  if (watcher.alive && watcher.stale) {
    watcherValue = "stale";
    watcherDot = "dot-yellow";
  } else if (watcher.alive) {
    watcherValue = "up";
    watcherDot = "dot-green";
  }
  target.append(
    brokerStat("watcher", watcherValue, watcherDot),
    brokerStat("away mode", watcher.afk ? "present" : "absent", watcher.afk ? "dot-yellow" : "dot-green"),
    brokerStat("beacon", `${ageText(watcher.beacon_age_secs)} ago`),
  );
  const headroom = snapshot.headroom;
  target.append(
    brokerStat("headroom", headroom
      ? `${headroom.in_use}/${headroom.capacity} · ${headroom.available} free`
      : "unknown", headroom?.at_limit ? "dot-red" : headroom ? "dot-green" : "dot-yellow"),
    brokerStat("wake queue", snapshot.wake_queue?.depth ?? "?", snapshot.wake_queue?.depth ? "dot-yellow" : "dot-green"),
    brokerStat("dispatch queue", snapshot.dispatch_queue?.depth ?? "?", snapshot.dispatch_queue?.depth ? "dot-yellow" : "dot-green"),
  );

  const age = generatedAt ? (Date.now() - generatedAt) / 1000 : Infinity;
  document.querySelector("#snapshot-age").textContent = `snapshot ${ageText(age)} old`;
}

function actorNode(task) {
  const interactive = timelineAvailable && task.synthetic !== true;
  const node = el(interactive ? "button" : "div", `node actor-node status-${statusStyle(task)}`);
  if (interactive) {
    node.type = "button";
    node.title = `View ${task.id} timeline`;
    node.addEventListener("click", () => showTimeline(task.id));
  }
  node.dataset.connectorStatus = statusStyle(task);
  node.append(
    el("span", "node-eyebrow", task.kind || "task"),
    el("strong", "node-title", task.id || "unknown"),
  );
  const status = el("span", "status-pill");
  status.append(el("span", "dot"), document.createTextNode(task.current_state?.state || "unknown"));
  node.append(status);
  const decisions = list(task.hints?.open_decisions);
  if (decisions.length) node.append(el("span", "needs-you", "needs you"));
  const detail = task.daemon_summary
    ? `${task.daemon_summary.active_children ?? 0} active · ${task.daemon_summary.queued ?? 0} queued`
    : [task.harness, task.current_state?.source].filter(Boolean).join(" · ");
  if (detail) node.append(el("div", "node-meta", detail));
  if (task.project) node.title = `${node.title ? `${node.title}\n` : ""}${task.project}`;
  return node;
}

function renderTree(snapshot) {
  const tasks = displayTasks(snapshot);
  const row = document.querySelector("#actors-row");
  clear(row);
  for (const task of tasks) row.append(actorNode(task));
  if (!tasks.length) {
    const ghost = el("div", "node actor-node ghost status-idle");
    ghost.dataset.connectorStatus = "idle";
    ghost.append(
      el("span", "node-eyebrow", "idle"),
      el("strong", "node-title", "no active workers"),
      el("span", "node-sub", "waiting on new work"),
    );
    row.append(ghost);
  }
  document.querySelector("#actor-count").textContent = `${tasks.length} active`;
  renderDecisionDrawer(snapshot);
  scheduleRedraw();
}

function backlogMeta(row) {
  if (!row.structured) return "unstructured inventory row";
  if (row.hold_reason) return row.hold_reason;
  if (row.title) return row.title;
  if (row.pr?.url) return row.pr.url;
  if (row.local_note) return row.local_note;
  if (row.completion?.date) return row.completion.date;
  if (list(row.unresolved_blocker_ids).length) return `blocked by ${row.unresolved_blocker_ids.join(", ")}`;
  return row.current_role || row.kind || "tracked work";
}

function renderBacklog(snapshot) {
  const target = document.querySelector("#backlog");
  clear(target);
  for (const [state, title] of [["in_flight", "In flight"], ["done", "Done"], ["queued", "Queued"]]) {
    const column = el("section", "backlog-column");
    const rows = list(snapshot.backlog?.records).filter((row) => row.state === state);
    const heading = el("div", "backlog-column-head");
    heading.append(el("span", "", title), el("span", "backlog-count", rows.length));
    const items = el("ul", "backlog-items");
    for (const row of rows) {
      const item = el("li", `backlog-item ${state}`);
      const line = el("div", "backlog-id-line");
      const titleText = row.structured
        ? `${row.kind || "work"} · ${row.id || "untitled"}`
        : valueOr(row.raw, "unstructured row");
      line.append(el("span", "backlog-status-tag", state.replace("_", " ")), el("span", "backlog-title", titleText));
      item.append(line, el("div", "backlog-meta", backlogMeta(row)));
      items.append(item);
    }
    if (!rows.length) items.append(el("li", "empty", "None"));
    column.append(heading, items);
    target.append(column);
  }
  const warning = document.querySelector("#inventory-warning");
  warning.hidden = snapshot.main_inventory?.valid !== false;
  warning.textContent = snapshot.main_inventory?.valid === false
    ? `Inventory warning: ${snapshot.main_inventory.reason || "the canonical inventory is inconsistent"}`
    : "";
}

function artifactRow(label, url, kind, extraClass = "") {
  const row = el("div", extraClass || "artifact");
  row.append(artifactLink(label, url));
  const kindClass = String(kind).includes("report") ? "report" : String(kind).includes("review") ? "review" : "";
  row.append(el("span", `artifact-kind ${kindClass}`.trim(), kind));
  return row;
}

function renderArtifacts(snapshot, artifacts) {
  const target = document.querySelector("#artifacts");
  clear(target);
  const active = list(snapshot.vplan_reviews?.records).filter((record) => record.pid_alive && record.url);
  for (const review of active) {
    target.append(artifactRow(review.artifact || "Open vplan review", review.url, "live review", "active-review"));
  }
  for (const artifact of list(artifacts)) {
    target.append(artifactRow(artifact.label, artifact.url, artifact.kind));
  }
  if (!active.length && !list(artifacts).length) target.append(el("p", "empty", "No browsable artifacts are present."));
}

function renderRecords(target, records, fields, emptyText) {
  clear(target);
  for (const record of records) {
    const card = el("article", "record");
    const top = el("div", "record-top");
    top.append(
      el("span", "record-id", record.id || "record"),
      el("span", "record-status", record.status || record.state || "unknown"),
    );
    card.append(top, detailList(fields(record)));
    target.append(card);
  }
  if (!records.length) target.append(el("p", "empty", emptyText));
}

function renderOptionalFeeds(snapshot) {
  const feeds = snapshot.later_feeds || {};
  const gatePanel = document.querySelector("#gate-panel");
  gatePanel.hidden = feeds.gate_runs?.available !== true;
  if (!gatePanel.hidden) {
    renderRecords(document.querySelector("#gate-runs"), list(feeds.gate_runs.records), (row) => [
      ["Step", row.step], ["Round", row.round], ["Decision", row.pending_decision_key],
    ], "No gate runs are recorded.");
  }
  const workflowPanel = document.querySelector("#workflow-panel");
  workflowPanel.hidden = feeds.workflow_runs?.available !== true;
  if (!workflowPanel.hidden) {
    renderRecords(document.querySelector("#workflow-runs"), list(feeds.workflow_runs.records), (row) => [
      ["Workflow", row.workflow], ["Stage", row.current_stage], ["Message", row.message],
    ], "No workflow runs are recorded.");
  }
  const deliveryPanel = document.querySelector("#delivery-panel");
  deliveryPanel.hidden = feeds.deliveries?.available !== true;
  if (!deliveryPanel.hidden) {
    renderRecords(document.querySelector("#deliveries"), list(feeds.deliveries.records), (row) => [
      ["Approval", row.approval], ["Branch", row.branch], ["Age", ageText(row.age_secs)],
    ], "No delivery handoffs are recorded.");
  }
  const upstreamPanel = document.querySelector("#upstream-panel");
  upstreamPanel.hidden = feeds.upstream_drift?.available !== true;
  if (!upstreamPanel.hidden) {
    const target = document.querySelector("#upstream-drift");
    clear(target);
    const drift = feeds.upstream_drift;
    target.append(detailList([
      ["Status", drift.status], ["Fork point", drift.fork_point],
      ["Reviewed through", drift.last_reviewed], ["Repository", drift.upstream_repo],
    ]));
  }
  document.querySelector("#doctor-button").hidden = feeds.doctor?.available !== true;
}

function render(payload) {
  currentSnapshot = payload.snapshot;
  const parsedGenerated = Date.parse(currentSnapshot.generated);
  generatedAt = Number.isFinite(parsedGenerated) ? parsedGenerated : null;
  timelineAvailable = currentSnapshot.later_feeds?.timeline?.available === true;
  renderBroker(currentSnapshot);
  renderTree(currentSnapshot);
  renderBacklog(currentSnapshot);
  renderArtifacts(currentSnapshot, payload.artifacts);
  renderOptionalFeeds(currentSnapshot);
}

function setConnected(message) {
  connectionNote.textContent = message;
  connectionNote.classList.remove("bad");
  liveDot.classList.remove("disconnected");
}

async function poll() {
  const headers = currentHash ? { "If-None-Match": currentHash } : {};
  try {
    const response = await fetch("/api/state", { headers, cache: "no-store" });
    if (response.status === 304) {
      setConnected(`Live · ${pollMs / 1000}s refresh`);
      return;
    }
    if (!response.ok) throw new Error(`snapshot request returned ${response.status}`);
    const payload = await response.json();
    currentHash = response.headers.get("ETag") || response.headers.get("X-Multplx-Content-Hash");
    render(payload);
    setConnected(`Live · ${pollMs / 1000}s refresh`);
  } catch (error) {
    connectionNote.textContent = `Connection lost · ${error.message}`;
    connectionNote.classList.add("bad");
    liveDot.classList.add("disconnected");
  }
}

function scheduleRedraw() {
  if (redrawScheduled) return;
  redrawScheduled = true;
  requestAnimationFrame(() => {
    redrawScheduled = false;
    drawConnectors();
  });
}

function drawConnectors() {
  const svg = document.querySelector("#connectors");
  const stage = document.querySelector("#tree-stage");
  const stageRect = stage.getBoundingClientRect();
  if (stageRect.width === 0) return;
  svg.setAttribute("viewBox", `0 0 ${stageRect.width} ${stageRect.height}`);
  svg.setAttribute("width", stageRect.width);
  svg.setAttribute("height", stageRect.height);

  const edge = (node, side) => {
    const rect = node.getBoundingClientRect();
    return {
      x: rect.left + rect.width / 2 - stageRect.left,
      y: (side === "top" ? rect.top : rect.bottom) - stageRect.top,
    };
  };
  const rootBottom = edge(maintainerNode, "bottom");
  const broker = document.querySelector("#broker-node");
  const brokerTop = edge(broker, "top");
  const brokerBottom = edge(broker, "bottom");
  const staticPath = document.createElementNS(svgNamespace, "path");
  staticPath.setAttribute("class", "path-static");
  staticPath.setAttribute("d", `M ${rootBottom.x} ${rootBottom.y} L ${brokerTop.x} ${brokerTop.y}`);
  const paths = [staticPath];
  const busY = brokerBottom.y + 28;
  for (const actor of document.querySelectorAll(".actor-node")) {
    const actorTop = edge(actor, "top");
    const status = actor.dataset.connectorStatus || "idle";
    const path = document.createElementNS(svgNamespace, "path");
    path.setAttribute("class", `path-${status === "failed" || status === "unknown" ? "blocked" : status}`);
    path.setAttribute("d", `M ${brokerBottom.x} ${brokerBottom.y} L ${brokerBottom.x} ${busY} L ${actorTop.x} ${busY} L ${actorTop.x} ${actorTop.y}`);
    paths.push(path);
  }
  svg.replaceChildren(...paths);
}

async function showTimeline(id) {
  document.querySelector("#dialog-title").textContent = `Timeline · ${id}`;
  const body = document.querySelector("#dialog-body");
  clear(body);
  body.append(el("p", "empty", "Loading the sanctioned timeline reader…"));
  dialog.showModal();
  try {
    const response = await fetch(`/api/timeline/${encodeURIComponent(id)}`, { cache: "no-store" });
    if (!response.ok) throw new Error(`timeline returned ${response.status}`);
    const payload = await response.json();
    clear(body);
    for (const record of list(payload.records)) {
      const row = el("div", "timeline-row");
      row.append(
        el("span", "timeline-time", valueOr(record.ts)),
        el("span", "timeline-event", `${valueOr(record.source, "source")} · ${valueOr(record.event, "event")}`),
        el("span", "timeline-detail", JSON.stringify(record.detail)),
      );
      body.append(row);
    }
    if (!list(payload.records).length) body.append(el("p", "empty", "The journal has no valid events."));
  } catch (error) {
    clear(body);
    body.append(el("p", "connection-note bad", error.message));
  }
}

document.querySelector("#doctor-button").addEventListener("click", async () => {
  document.querySelector("#dialog-title").textContent = "Doctor summary";
  const body = document.querySelector("#dialog-body");
  clear(body);
  body.append(el("p", "empty", "Running an explicit read-only invariant sweep…"));
  dialog.showModal();
  const broker = document.querySelector("#broker-node");
  broker.classList.remove("doctor-flash");
  void broker.offsetWidth;
  broker.classList.add("doctor-flash");
  try {
    const response = await fetch("/api/doctor", { cache: "no-store" });
    const payload = await response.json();
    if (!response.ok) throw new Error(payload.error || `doctor returned ${response.status}`);
    clear(body);
    const pre = el("pre");
    pre.textContent = JSON.stringify(payload, null, 2);
    body.append(pre);
  } catch (error) {
    clear(body);
    body.append(el("p", "connection-note bad", error.message));
  }
});

maintainerNode.addEventListener("click", toggleDrawer);
document.querySelector("#drawer-close").addEventListener("click", closeDrawer);
document.addEventListener("click", (event) => {
  if (drawerOpen && !decisionDrawer.contains(event.target) && !maintainerNode.contains(event.target)) closeDrawer();
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && drawerOpen) closeDrawer();
});
document.querySelector("#dialog-close").addEventListener("click", () => dialog.close());
window.addEventListener("resize", scheduleRedraw);

function updateClock() {
  document.querySelector("#clock").textContent = `· ${new Date().toISOString().replace(/\.\d{3}Z$/, "Z")}`;
  if (currentSnapshot) renderBroker(currentSnapshot);
}

updateClock();
poll();
setInterval(updateClock, 1000);
setInterval(poll, pollMs);

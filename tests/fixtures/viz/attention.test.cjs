"use strict";
// Execute the real attention renderer against a minimal deterministic DOM.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { execFileSync } = require("node:child_process");
class Node {
  constructor(tag) { this.tagName = tag; this.children = []; this.attributes = {}; this.dataset = {}; this.hidden = false; this._text = ""; }
  set textContent(value) { this._text = String(value); this.children = []; }
  get textContent() { return this._text + this.children.map((node) => node.textContent).join(""); }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this._text = ""; this.children = nodes; }
  setAttribute(key, value) { this.attributes[key] = value; }
  focus() { document.activeElement = this; }
  querySelectorAll(selector) {
    const descendants = this.children.flatMap((node) => [node, ...node.querySelectorAll("*")]);
    if (selector === "*") return descendants;
    if (selector === "[data-focus-key]") return descendants.filter((node) => node.dataset.focusKey);
    return descendants.filter((node) => node.className === selector.slice(1));
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0]; }
}
const nodes = new Map(["#attention-list", "#attention-panel", "#attention-count"].map((id) => [id, new Node("div")]));
const document = { createElement: (tag) => new Node(tag), querySelector: (id) => nodes.get(id), querySelectorAll: (selector) => [...nodes.values()].flatMap((node) => node.querySelectorAll(selector)) };
const root = path.resolve(__dirname, "../../..");
const source = fs.readFileSync(path.join(root, "share/viz/app.js"), "utf8");
const extract = (name) => {
  const start = source.indexOf(`function ${name}(`);
  assert.ok(start >= 0, `${name} exists`);
  return source.slice(start, source.indexOf("\nfunction ", start + 1));
};
const context = vm.createContext({ document, URLSearchParams, URL });
vm.runInContext(source.slice(source.indexOf("const list ="), source.indexOf("const connectionNote =")) +
  "\nlet portfolio; let attentionItemCount=0; let agentsViewActive=false;\n" +
  ["titleCase", "projectRecord", "normalizedTasks", "statusTone", "chip", "taskKey", "hashForTask", "stableFocusKey", "restoreFocus", "deliveryStatus", "displayTaskTitle", "readyPrUrl", "renderAttention", "detailSection", "taskDetails"].map(extract).join("\n"), context);
const snapshot = JSON.parse(execFileSync(process.execPath, [path.join(__dirname, "attention.mjs")], { encoding: "utf8" }));
context.input = snapshot.portfolio;
vm.runInContext("portfolio=input; renderAttention();", context);
const cards = nodes.get("#attention-list").children;
assert.equal(cards.length, 3, "only explicit unanswered questions and recorded ready PRs render; agent work stays in task details");
assert.equal(nodes.get("#attention-count").textContent, `${cards.length} actionable`);
assert.equal(nodes.get("#attention-panel").hidden, false);
for (const card of cards) {
  assert.equal(card.tagName, "article");
  const [link, details] = card.children;
  assert.equal(link.tagName, "a");
  assert.ok(link.href.startsWith("#task=") || link.href === "https://example.invalid/pull/42");
  assert.match(link.attributes["aria-label"], /^(Answer needed|PR ready for review): /);
  assert.equal(link.children[1].className, "attention-title");
  assert.equal(link.children[2].className, "attention-reason");
  assert.equal(details.tagName, "details", "native disclosure supports keyboard access");
  assert.equal(details.children[0].tagName, "summary");
  assert.equal(details.children[1].tabIndex, 0, "full text scroll region supports keyboard access");
  assert.equal(details.children[1].attributes.role, "region");
  assert.equal(details.children[1].children[0].textContent, link.children[1].textContent, "full title preserved");
  assert.equal(details.children[1].children[1].textContent, link.children[2].textContent, "full action/question preserved");
}
assert.equal(cards[0].children[0].children[0].textContent, "PR ready for review");
assert.equal(cards[0].children[0].children[1].textContent, "Repair attention cards", "derive meaningful label from legacy scaffold");
assert.equal(cards[0].children[0].href, "https://example.invalid/pull/42", "ready PR links to the recorded PR");
assert.equal(cards[0].children[0].target, "_blank");
assert.equal(cards[0].children[0].rel, "noreferrer");
assert.equal(cards[0].children[0].children[2].textContent, "Review and merge if satisfied.");
assert.ok(cards[0].children[1].textContent.includes(snapshot.portfolio.tasks[0].title), "complete original scaffold remains accessible");
assert.equal(cards[1].children[0].children[0].textContent, "Answer needed");
assert.equal(cards[1].children[0].children[1].textContent, "Should the rollout include archived projects?");
assert.equal(cards[1].children[0].children[2].textContent, "Reply in main orchestrator chat.");
context.task = snapshot.portfolio.tasks[3];
assert.equal(vm.runInContext("displayTaskTitle(task)", context), "Repair attention cards", "flattened scaffold produces meaningful label");
context.task = snapshot.portfolio.tasks[1];
assert.equal(vm.runInContext("displayTaskTitle(task)", context), "Implement task 2", "unbroken title falls back to recorded brief outcome");
context.task = { id: "fallback", title: "You are a worker. " + "x".repeat(200) };
assert.equal(vm.runInContext("displayTaskTitle(task)", context), "fallback", "instruction blob falls back to task identity");
context.renderExecution = () => new Node("section");
context.renderWorkflow = () => new Node("section");
context.renderDecisions = () => null;
context.renderEvidence = () => new Node("section");
const taskDetail = vm.runInContext("taskDetails(task)", context);
assert.ok(taskDetail.textContent.includes(context.task.title), "full original remains available in expandable task details even without attention");
const unsafe = cards.find((card) => card.textContent.includes("<img"));
assert.ok(unsafe);
assert.equal(unsafe.children[0].children[1].children.length, 0, "untrusted HTML remains text");
assert.ok(unsafe.children[1].textContent.includes("This is untrusted literal text."));
const params = new URLSearchParams(cards[1].children[0].href.slice(1));
assert.equal(params.get("task"), snapshot.portfolio.tasks[2].key);
assert.equal(params.get("project"), snapshot.portfolio.tasks[2].project.id);
// A recorded readiness flag and actual absolute safe PR URL are both required.
context.task = { evidence: { review_queue: { state: "ready", pr_ready: true, pr_url: "https://example.invalid/pull/42" } } };
assert.equal(vm.runInContext("readyPrUrl(task)", context), "https://example.invalid/pull/42");
for (const url of ["", null, "javascript:alert(1)", "data:text/html,unsafe", "/pull/42", "invalid URL"]) {
  context.task.evidence.review_queue.pr_url = url;
  assert.equal(vm.runInContext("readyPrUrl(task)", context), null, `reject non-PR URL ${url}`);
}
for (const state of ["ready", ...snapshot.portfolio.tasks.slice(1).map((task) => task.evidence.review_queue.state)]) {
  context.task.evidence.review_queue = { state, pr_ready: false, pr_url: "https://example.invalid/pull/42" };
  assert.equal(vm.runInContext("readyPrUrl(task)", context), null, `do not infer readiness from ${state}`);
}
context.task.evidence.review_queue = { state: "review-findings", pr_ready: true, pr_url: "https://example.invalid/pull/42" };
assert.equal(vm.runInContext("readyPrUrl(task)", context), null, "inconsistent non-ready state stays task detail");
context.task.evidence.review_queue = { state: "ready", pr_url: "https://example.invalid/pull/42" };
assert.equal(vm.runInContext("readyPrUrl(task)", context), null, "PR presence and state alone do not invent readiness");
const originalDetails = cards[0].children[1];
originalDetails.open = true;
originalDetails.children[1].scrollTop = 144;
originalDetails.children[1].focus();
vm.runInContext("portfolio.tasks.reverse(); renderAttention();", context);
const refreshed = nodes.get("#attention-list").children.find((card) => card.children[1].dataset.attentionKey === originalDetails.dataset.attentionKey).children[1];
assert.equal(refreshed.open, true, "open disclosure survives reordered polling updates");
assert.equal(refreshed.children[1].scrollTop, 144, "full-text scroll survives updates");
assert.equal(document.activeElement, refreshed.children[1], "keyboard focus survives updates");
assert.equal(nodes.get("#attention-list").children.filter((card) => card.children[1].open).length, 1, "only the matching disclosure remains open");
vm.runInContext("agentsViewActive=true; renderAttention();", context);
assert.equal(nodes.get("#attention-panel").hidden, true);
vm.runInContext('agentsViewActive=false; portfolio={schema:"mx-portfolio.v1",tasks:[]}; renderAttention();', context);
assert.equal(nodes.get("#attention-panel").hidden, true);
assert.equal(nodes.get("#attention-list").children.length, 0);
console.log("attention renderer: bounded-layout hooks, full text, actions, safe text, exact routing and keyboard disclosure passed");

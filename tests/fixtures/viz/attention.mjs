#!/usr/bin/env node
// Legacy titles and human actions for deterministic DOM and isolated browser checks.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const snapshot = JSON.parse(execFileSync(process.execPath, [fileURLToPath(new URL("portfolio.mjs", import.meta.url)), "5"], { encoding: "utf8" }));
const legacy = "You are an persistent implementer sub-agent. Your current worker assignment defines your role.\n\n# Task\nRepair attention cards\n\n# Charter\nRepair attention cards\n\n# Definition of done\nKeep all original evidence.\n".repeat(40);
const states = ["ready", "needs-checks", "blocked-by-dependencies", "review-findings", "review-not-run", "stale-revision", "freshness-unknown", "publication-failed", "merged"];
const template = snapshot.portfolio.tasks[0];
while (snapshot.portfolio.tasks.length < states.length) {
  const ordinal = snapshot.portfolio.tasks.length + 1;
  snapshot.portfolio.tasks.push({ ...structuredClone(template), id: `task-${ordinal}`, key: `/mx/root#task:task-${ordinal}`, children: [] });
}
for (const [index, task] of snapshot.portfolio.tasks.entries()) {
  task.decisions = [];
  task.evidence.review_queue = { state: states[index], pr_ready: index === 0, pr_url: index === 0 ? "https://example.invalid/pull/42" : "" };
  task.title = index === 0 ? legacy : index === 1 ? "unbroken".repeat(200) : index === 3 ? legacy.replaceAll("\n", " ") : `Attention task ${index + 1}`;
}
snapshot.portfolio.tasks[2].decisions = [
  { id: "long", question: "Should the rollout include archived projects?", reason: "Keep the rollout within the agreed scope. ".repeat(200) },
  { id: "unsafe", question: '<img src=x onerror="window.__attentionInjected=true">', reason: "This is untrusted literal text." },
  { id: "resolved", state: "resolved", question: "Resolved question" },
  { id: "answered", answer: "yes", question: "Answered question" },
];
for (const [id, prUrl] of [["report-only", ""], ["unsafe-pr", "javascript:alert(1)"]]) {
  const task = structuredClone(template);
  Object.assign(task, { id, key: `/mx/root#task:${id}`, title: id === "report-only" ? "Completed research report" : "Unsafe PR URL fixture", decisions: [], children: [] });
  task.evidence.review_queue = { state: "ready", pr_ready: true, pr_url: prUrl };
  snapshot.portfolio.tasks.push(task);
}
snapshot.portfolio.counts.records = snapshot.portfolio.tasks.length;
snapshot.portfolio.counts.shown = snapshot.portfolio.tasks.length;
snapshot.portfolio.counts.tasks = snapshot.portfolio.tasks.filter((task) => task.role !== "sub-orchestrator").length;
process.stdout.write(`${JSON.stringify(snapshot)}\n`);

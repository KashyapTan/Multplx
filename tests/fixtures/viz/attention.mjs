#!/usr/bin/env node
// Legacy titles and human actions for deterministic DOM and isolated browser checks.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const snapshot = JSON.parse(execFileSync(process.execPath, [fileURLToPath(new URL("portfolio.mjs", import.meta.url)), "5"], { encoding: "utf8" }));
const legacy = "You are an persistent implementer sub-agent. Your current worker assignment defines your role.\n\n# Task\nRepair attention cards\n\n# Charter\nRepair attention cards\n\n# Definition of done\nKeep all original evidence.\n".repeat(40);
const states = ["ready", "needs-checks", "blocked-by-dependencies", "review-findings", "review-not-run"];
for (const [index, task] of snapshot.portfolio.tasks.entries()) {
  task.decisions = [];
  task.evidence.review_queue = { state: states[index % states.length] };
  task.title = index === 0 ? legacy : index === 1 ? "unbroken".repeat(200) : index === 3 ? legacy.replaceAll("\n", " ") : `Attention task ${index + 1}`;
}
snapshot.portfolio.tasks[2].decisions = [
  { id: "long", question: "Choose a rollout scope. ".repeat(200), reason: "Keep the rollout within the agreed scope." },
  { id: "unsafe", question: '<img src=x onerror="window.__attentionInjected=true">', reason: "This is untrusted literal text." },
  { id: "resolved", state: "resolved", question: "Resolved question" },
  { id: "answered", answer: "yes", question: "Answered question" },
];
process.stdout.write(`${JSON.stringify(snapshot)}\n`);

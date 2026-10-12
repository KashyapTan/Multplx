#!/usr/bin/env node
// Actual Rust snapshot/service/browser integration using an owned synthetic home.
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const runtime = path.join(root, "target/release/mx");
const fixture = mkdtempSync(path.join(tmpdir(), "mx-viz-service-browser-"));
const output = process.env.MX_VIZ_BROWSER_OUTPUT || "/tmp/mx-viz-dashboard-service-browser";
mkdirSync(output, { recursive: true });
const home = path.join(fixture, "home");
const child = path.join(fixture, "child");
const fakebin = path.join(fixture, "fakebin");
mkdirSync(fakebin);
for (const directory of [home, child]) {
  for (const name of ["state", "data", "config", "projects", "bin"]) mkdirSync(path.join(directory, name), { recursive: true });
  writeFileSync(path.join(directory, "AGENTS.md"), "# Synthetic fixture\n");
  writeFileSync(path.join(directory, "data/backlog.md"), "# Backlog\n");
}
writeFileSync(path.join(child, ".mx-agent-home"), "parent\n");
const commit = "a".repeat(40);
const pr = "https://github.com/example/dashboard/pull/7";
const model = (id, owner, state, options = {}) => ({
  schema_version: 2, task_id: id, role: "implementer", artifact: "implementation", persistent: false, private_home: false,
  parent_id: "root", root_id: "root", owner_home: owner, owner_state: path.join(owner, "state"), parent_state: path.join(owner, "state"), parent_home: owner,
  persistent_home: null, home_allocation: null, accepted_brief_digest: "synthetic", accepted_brief_path: null,
  runtime: { provider: "tmux", session_id: null, endpoint: `fixture:mx-${id}` }, attempt: { id: `attempt-${id}`, generation: 1, brief_revision: 1 }, accepted_brief_revision: 1,
  briefs: [{ revision: 1, scope: id === "parent" ? "Coordinate useful descendant work" : `Repair ${id} presentation`, acceptance_criteria: ["Fixture only"], source_artifacts: [], reason: "fixture" }],
  prior_attempts: [], retained_executions: [], native_observations: [], assignments: [{ generation: 1, role: "implementer", brief_revision: 1, reason: "fixture" }],
  schedule: { priority: 0, dependencies: [], state, waiting_condition: null, decisions: [] }, project: null, allocation: null, domain: null, owning_coordinator: "root", transfers: [],
  delivery: { current_commit: null, history: [] }, legacy_unknown: false, ...options,
});
const publish = (task) => {
  task.delivery = { current_commit: commit, history: [{ evidence_id: `evidence-${task.task_id}`, attempt_id: task.attempt.id, attempt_generation: 1, brief_revision: 1, commit,
    accepted_scope: task.briefs[0].scope, checks: [{ name: "fixture", outcome: "passed", summary: "Synthetic check", artifact: null }], review: { outcome: "passed", summary: "Synthetic review", artifact: null }, limitations: [], pr_url: pr, outcome: "published", observed_at: "2026-10-12T12:00:00Z" }] };
  return task;
};
const writeTask = (task, status) => {
  writeFileSync(path.join(task.owner_home, "state", `${task.task_id}.meta`), `schema_version=2\nbackend=tmux\nwindow=fixture:mx-${task.task_id}\nkind=${task.persistent ? "daemon" : "delivery"}\nmode=delivery\nworktree=${task.owner_home}\ncanonical_model=${JSON.stringify(task)}\n`);
  writeFileSync(path.join(task.owner_home, "state", `${task.task_id}.status`), `${status}: synthetic fixture\n`);
};
writeTask(publish(model("merged", home, "completed")), "done");
writeTask(model("parent", home, "completed", { persistent: true, persistent_home: child }), "done");
const childTask = model("nested", child, "waiting-human");
childTask.schedule.decisions = [{ id: "rollout", task_id: "nested", brief_revision: 1, workflow_revision: null, question: "Should this fixture include archived projects?", answer: null }];
writeTask(childTask, "blocked");
writeTask(model("reported", home, "runnable"), "done");
writeTask(model("historical", home, "running"), "working");
writeFileSync(path.join(fakebin, "gh"), `#!/bin/sh\nprintf '%s\\n' '{"url":"${pr}","state":"MERGED","title":"Repair presentation","headRefOid":"${commit}"}'\n`, { mode: 0o755 });
writeFileSync(path.join(fakebin, "tmux"), '#!/bin/sh\ncase "$1" in list-windows) printf "%s\\n" mx-merged mx-parent mx-nested mx-reported mx-historical ;; display-message) printf "%s\\n" "%fixture" ;; esac\n', { mode: 0o755 });
const env = { ...process.env, PATH: `${fakebin}:${process.env.PATH}`, MX_HOME: home, MX_STATE_OVERRIDE: path.join(home, "state"), MX_ROOT_OVERRIDE: root, MX_RUST_SOURCE_ROOT: root, MX_RUST_BIN: runtime, MX_DATA_OVERRIDE: path.join(home, "data"), MX_CONFIG_OVERRIDE: path.join(home, "config"), MX_PROJECTS_OVERRIDE: path.join(home, "projects"),
  MX_VIZ_PORT: "27890", MX_VIZ_IDLE_SECS: "60", MX_VIZ_POLL_MS: "250", MX_VIZ_REFRESH_SECS: "1", MX_VIZ_FORGE_REFRESH_SECS: "10" };
for (const key of ["MX_VIZ_SNAPSHOT_BIN", "MX_VIZ_FIXTURE", "MX_VIZ_DOCTOR_BIN", "MX_VIZ_TIMELINE_BIN", "MX_TASK_ID", "MX_HARNESS", "MX_BACKEND", "MX_REPORT_STATE_OVERRIDE", "CODEX_THREAD_ID", "CLAUDECODE"]) delete env[key];
const before = readFileSync(path.join(home, "state/merged.meta"), "utf8");
let browser;
try {
  const base = execFileSync(runtime, ["viz", "serve"], { env, encoding: "utf8" }).trim().split("\n").at(-1);
  let payload;
  for (let attempt = 0; attempt < 20; attempt++) {
    const response = await fetch(new URL("api/state", base));
    if (response.ok) { payload = await response.json(); break; }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  assert.ok(payload, "Rust service must publish a synthetic canonical snapshot");
  writeFileSync(path.join(output, "debug-state.json"), JSON.stringify(payload, null, 2));
  assert.equal(payload.snapshot.portfolio.schema, "mx-portfolio.v1");
  assert.equal(payload.forge_observations[0].state, "merged");
  const merged = payload.snapshot.portfolio.tasks.find((task) => task.id === "merged");
  const nested = payload.snapshot.portfolio.tasks.find((task) => task.id === "nested");
  assert.equal(merged.completion.kind, "implementation-complete");
  assert.equal(merged.observation.quality, "historical");
  assert.ok(nested, "registered standing-worker child is in the actual Rust portfolio");
  assert.ok(payload.snapshot.descendants.records.some((row) => row.parent_task_key.endsWith("task:parent") && row.validated_home === child));
  const { chromium } = await import(process.env.MX_PLAYWRIGHT_MODULE);
  browser = await chromium.launch({ headless: true });
  const errors = [];
  const results = { schema: "mx-viz-service-browser-results.v1", source: "actual Rust service and system-snapshot; synthetic home and forge command", layouts: {} };
  for (const [layout, viewport] of Object.entries({ desktop: { width: 1440, height: 900 }, mobile: { width: 390, height: 844 } })) {
    const page = await browser.newPage({ viewport });
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto(base);
    await page.locator("#attention-list").filter({ hasText: "Should this fixture include archived projects?" }).waitFor();
    assert.equal(await page.locator('#task-history-list [data-task-id$="task:merged"]').count(), 1);
    assert.match(await page.locator('#task-list [data-task-id$="task:reported"]').textContent(), /Awaiting parent verification/);
    assert.equal(await page.locator('#task-reconciliation-list [data-task-id$="task:historical"]').count(), 1);
    await page.locator("#agents-view-button").click();
    assert.equal(await page.locator('#agents-tree [data-agent-card-key$="task:parent"]').count(), 1);
    assert.equal(await page.locator('#agents-tree [data-agent-card-key$="task:nested"]').count(), 1);
    assert.equal(await page.locator("#agents-unresolved-panel").isVisible(), false, "exact validated descendant parent is connected");
    await page.evaluate(() => { document.activeElement?.blur(); window.scrollTo({ top: 0, behavior: "instant" }); });
    await page.screenshot({ path: path.join(output, `service-agents-${layout}.png`), fullPage: true });
    await page.locator("#tasks-view-button").click();
    await page.evaluate(() => { document.activeElement?.blur(); window.scrollTo({ top: 0, behavior: "instant" }); });
    await page.screenshot({ path: path.join(output, `service-dashboard-${layout}.png`), fullPage: true });
    results.layouts[layout] = { actual_rust_schema: true, merged_history: true, worker_report_active: true, historical_reconciliation: true, nested_human_question: true, exact_descendant_graph: true };
    await page.close();
  }
  assert.deepEqual(errors, []);
  assert.equal(readFileSync(path.join(home, "state/merged.meta"), "utf8"), before, "forge refresh must not mutate canonical publication state");
  writeFileSync(path.join(output, "service-browser-results.json"), JSON.stringify(results, null, 2) + "\n");
  console.log(JSON.stringify(results));
} finally {
  if (browser) await browser.close();
  try { execFileSync(runtime, ["viz", "stop"], { env, stdio: "ignore" }); } catch {}
  rmSync(fixture, { recursive: true, force: true });
}

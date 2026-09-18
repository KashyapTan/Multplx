#!/usr/bin/env node
// Optional browser acceptance runner for the read-only dashboard.
// It requires an already-running fixture-backed viz server and a temporary Playwright install.

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const playwrightModule = process.env.MX_PLAYWRIGHT_MODULE;
const fixtureFile = process.env.MX_VIZ_FIXTURE_FILE;
const base = process.env.MX_VIZ_BROWSER_URL;
const output = process.env.MX_VIZ_BROWSER_OUTPUT;
const expectedArtifactText = process.env.MX_VIZ_EXPECT_ARTIFACT_TEXT;
if (!playwrightModule || !fixtureFile || !base || !output) {
  process.stderr.write("set MX_PLAYWRIGHT_MODULE, MX_VIZ_FIXTURE_FILE, MX_VIZ_BROWSER_URL, and MX_VIZ_BROWSER_OUTPUT\n");
  process.exit(2);
}
const { chromium } = await import(playwrightModule);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const generator = path.join(root, "tests/fixtures/viz/portfolio.mjs");
const canonicalTwenty = JSON.parse(execFileSync(process.execPath, [generator, "20"], { encoding: "utf8" }));
mkdirSync(output, { recursive: true });
const originalFixture = readFileSync(fixtureFile);
let fixtureRestored = false;
const restoreFixture = () => {
  if (fixtureRestored) return;
  writeFileSync(`${fixtureFile}.new`, originalFixture);
  renameSync(`${fixtureFile}.new`, fixtureFile);
  fixtureRestored = true;
};
process.on("exit", restoreFixture);
const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const publish = (count, marker = "") => {
  const value = JSON.parse(execFileSync(process.execPath, [generator, String(count)], { encoding: "utf8" }));
  value.browser_marker = marker;
  writeFileSync(`${fixtureFile}.new`, `${JSON.stringify(value)}\n`);
  renameSync(`${fixtureFile}.new`, fixtureFile);
};
const percentile = (values, fraction) => [...values].sort((a, b) => a - b)[Math.ceil(values.length * fraction) - 1];
const browser = await chromium.launch({ headless: true });
const results = { schema: "mx-viz-browser-results.v1", scales: {}, errors: [], interaction_samples_ms: [] };

if (expectedArtifactText) {
  const artifact = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  await artifact.goto(base, { waitUntil: "domcontentloaded" });
  const firstArtifactRow = artifact.locator(".task-row").first();
  await firstArtifactRow.locator(":scope > summary").click();
  const brief = firstArtifactRow.locator(".evidence-row").filter({ hasText: "Brief" }).locator("a").first();
  await brief.click();
  await artifact.locator("#detail-dialog[open] #dialog-body").filter({ hasText: expectedArtifactText }).waitFor();
  results.canonical_artifact = {
    source_url: await brief.getAttribute("href"),
    previewed: true,
    scriptless: await artifact.locator("#detail-dialog iframe").count() === 0,
  };
  if (!results.canonical_artifact.source_url?.startsWith("/artifact/ref/") || !results.canonical_artifact.scriptless) {
    throw new Error("canonical brief did not use the exact reference route and scriptless Markdown preview");
  }
  await artifact.close();
}

for (const count of [0, 1, 5, 10, 20]) {
  publish(count, `scale-${count}`);
  await sleep(3200);
  for (const [layout, viewport] of Object.entries({ wide: { width: 1440, height: 900 }, narrow: { width: 390, height: 844 } })) {
    const page = await browser.newPage({ viewport });
    page.on("console", (message) => { if (message.type() === "error") results.errors.push(`${count}/${layout}: ${message.text()}`); });
    page.on("pageerror", (error) => results.errors.push(`${count}/${layout}: ${error.message}`));
    await page.goto(base, { waitUntil: "domcontentloaded" });
    await page.locator("#task-summary").filter({ hasText: new RegExp(`· ${count} tasks ·`) }).waitFor();
    const measured = await page.evaluate(() => ({
      width: innerWidth,
      horizontal_overflow: document.documentElement.scrollWidth > innerWidth,
      workspace_columns: getComputedStyle(document.querySelector(".workspace")).gridTemplateColumns.split(" ").length,
      task_rows: document.querySelectorAll(".task-row").length,
      side_position: getComputedStyle(document.querySelector(".side-column")).position,
    }));
    if (measured.horizontal_overflow) throw new Error(`${count}/${layout} has horizontal overflow`);
    if (layout === "narrow" && (measured.workspace_columns !== 1 || measured.side_position !== "static")) throw new Error(`${count} narrow layout did not collapse`);
    if (layout === "wide" && measured.workspace_columns < 2) throw new Error(`${count} wide layout lost its side rail`);
    await page.screenshot({ path: path.join(output, `scale-${count}-${layout}.png`), fullPage: true });
    results.scales[`${count}_${layout}`] = measured;
    await page.close();
  }
}

publish(20, "interaction-base");
await sleep(3200);
const interaction = await browser.newPage({ viewport: { width: 1440, height: 900 } });
await interaction.goto(base, { waitUntil: "domcontentloaded" });
await interaction.locator("#task-summary").filter({ hasText: /· 20 tasks ·/ }).waitFor();
const first = interaction.locator(".task-row > summary").first();
await first.click();
await interaction.locator("#search").fill("Portfolio task 1");
await interaction.locator("#search").focus();
publish(20, "meaningful-update");
await sleep(6000);
results.stable_interaction = await interaction.evaluate(() => ({
  search_value: document.querySelector("#search").value,
  search_focused: document.activeElement === document.querySelector("#search"),
  first_expanded: document.querySelector(".task-row")?.open === true,
  deep_link: location.hash,
}));
if (!results.stable_interaction.search_focused || !results.stable_interaction.first_expanded) throw new Error("focus or expansion did not survive a meaningful update");
await interaction.locator("#search").fill("");
await first.focus();
await first.press("ArrowDown");
results.keyboard_next_task = await interaction.evaluate(() => document.activeElement?.dataset?.focusKey || null);

const readBindings = () => interaction.evaluate(() => [...document.querySelectorAll(".task-row")].map((row) => ({
  key: row.dataset.taskId,
  identity: row.querySelector(".task-identity")?.textContent || "",
  evidence: [...row.querySelectorAll(".evidence-row")].map((item) => item.textContent.trim()),
})));
const baselineBindings = await readBindings();
const baselineByKey = new Map(baselineBindings.map((binding) => [binding.key, binding]));
const sameStrings = (actual, expected, context) => {
  const left = [...actual].sort();
  const right = [...expected].sort();
  if (JSON.stringify(left) !== JSON.stringify(right)) throw new Error(`${context}: ${JSON.stringify(left)} != ${JSON.stringify(right)}`);
};
const projectChecks = [];
for (const project of canonicalTwenty.portfolio.projects) {
  const expected = canonicalTwenty.portfolio.tasks.filter((task) => task.project.id === project.id);
  await interaction.locator("#project-filter").selectOption(project.id);
  await interaction.waitForFunction((count) => document.querySelectorAll(".task-row").length === count, expected.length);
  const visible = await readBindings();
  sameStrings(visible.map((binding) => binding.key), expected.map((task) => task.key), `project ${project.id} task keys`);
  for (const binding of visible) {
    if (!binding.identity.includes(project.path)) throw new Error(`project ${project.id} lost checkout path for ${binding.key}`);
    if (JSON.stringify(binding.evidence) !== JSON.stringify(baselineByKey.get(binding.key)?.evidence)) {
      throw new Error(`project ${project.id} changed evidence binding for ${binding.key}`);
    }
  }
  projectChecks.push({ id: project.id, display_name: project.display_name, path: project.path, records: visible.length });
}
await interaction.locator("#project-filter").selectOption("");
await interaction.waitForFunction((count) => document.querySelectorAll(".task-row").length === count, baselineBindings.length);
const restoredBindings = await readBindings();
if (JSON.stringify(restoredBindings) !== JSON.stringify(baselineBindings)) throw new Error("clearing the project filter changed task path or evidence bindings");
const duplicateDisplays = projectChecks.filter((project) => project.display_name === "Console");
if (duplicateDisplays.length !== 2 || duplicateDisplays[0].path === duplicateDisplays[1].path) {
  throw new Error("duplicate project display names did not retain distinct checkout paths");
}
const projectDepths = projectChecks.map((project) => project.path.split("/").filter(Boolean).length);
if (new Set(projectDepths).size !== projectChecks.length) throw new Error(`project repository depths are not distinct: ${projectDepths.join(",")}`);
results.project_filters = {
  projects: projectChecks,
  duplicate_display_paths_distinct: true,
  distinct_repository_depths: projectDepths,
  bindings_restored_after_clear: true,
};

const verifyFilter = async (selector, value, expectedKeys, label) => {
  await interaction.locator(selector).selectOption(value);
  await interaction.waitForFunction((count) => document.querySelectorAll(".task-row").length === count, expectedKeys.length);
  sameStrings((await readBindings()).map((binding) => binding.key), expectedKeys, label);
  await interaction.locator(selector).selectOption("");
};
await verifyFilter("#role-filter", "implementer", canonicalTwenty.portfolio.tasks.filter((task) => task.role === "implementer").map((task) => task.key), "role filter");
await verifyFilter("#status-filter", "working", canonicalTwenty.portfolio.tasks.filter((task) => task.state === "working").map((task) => task.key), "state filter");
await verifyFilter("#priority-filter", "p20", canonicalTwenty.portfolio.tasks.filter((task) => task.priority === 20).map((task) => task.key), "priority filter");
results.parallel_filters = { role: "implementer", state: "working", priority: "p20", exact_bindings: true };
await interaction.close();

const overlap = await browser.newPage({ viewport: { width: 1000, height: 700 } });
let activeStateRequests = 0;
let maximumActiveStateRequests = 0;
let completedStateRequests = 0;
await overlap.route("**/api/state", async (route) => {
  activeStateRequests += 1;
  maximumActiveStateRequests = Math.max(maximumActiveStateRequests, activeStateRequests);
  await sleep(3000);
  try { await route.continue(); } finally { activeStateRequests -= 1; completedStateRequests += 1; }
});
await overlap.goto(base, { waitUntil: "domcontentloaded" });
await sleep(7200);
results.nonoverlapping_poll = { maximum_simultaneous_state_requests: maximumActiveStateRequests, completed_state_requests: completedStateRequests };
if (maximumActiveStateRequests !== 1 || completedStateRequests < 1) throw new Error("browser polling overlapped state requests");
await overlap.close();

const hidden = await browser.newPage({ viewport: { width: 1000, height: 700 } });
await hidden.addInitScript(() => {
  const actualSetTimeout = window.setTimeout.bind(window);
  window.__mxTimeoutDelays = [];
  window.__mxHidden = false;
  Object.defineProperty(document, "hidden", { configurable: true, get: () => window.__mxHidden });
  window.__mxSetHidden = (value) => {
    window.__mxHidden = value;
    document.dispatchEvent(new Event("visibilitychange"));
  };
  window.setTimeout = (callback, delay, ...args) => {
    window.__mxTimeoutDelays.push(Number(delay));
    return actualSetTimeout(callback, delay, ...args);
  };
});
await hidden.goto(base, { waitUntil: "domcontentloaded" });
await hidden.locator("#task-summary").filter({ hasText: /· 20 tasks ·/ }).waitFor();
await hidden.evaluate(() => { window.__mxTimeoutDelays.length = 0; window.__mxSetHidden(true); });
const hiddenDelay = await hidden.evaluate(() => Math.max(...window.__mxTimeoutDelays));
results.hidden_poll = { method: "injected document.hidden state", scheduled_delay_ms: hiddenDelay };
if (hiddenDelay < 15000) throw new Error(`hidden polling scheduled after only ${hiddenDelay}ms`);
await hidden.close();

let firstState = null;
const freshDeadline = Date.now() + 10000;
while (Date.now() < freshDeadline) {
  const candidate = await fetch(new URL("api/state", base));
  if (candidate.headers.get("x-multplx-cache") === "fresh" && candidate.headers.get("x-multplx-refresh") === "idle") {
    firstState = candidate;
    break;
  }
  await candidate.arrayBuffer();
  await sleep(100);
}
if (!firstState) throw new Error("service did not reach a fresh idle observation before the 304 age check");
const firstEtag = firstState.headers.get("etag");
const firstAge = Number(firstState.headers.get("x-multplx-observation-age-ms"));
await firstState.arrayBuffer();
await sleep(120);
const unchangedState = await fetch(new URL("api/state", base), { headers: { "If-None-Match": firstEtag } });
const unchangedAge = Number(unchangedState.headers.get("x-multplx-observation-age-ms"));
results.unchanged_age = { status: unchangedState.status, before_ms: firstAge, after_ms: unchangedAge, advanced: unchangedAge > firstAge };
if (unchangedState.status !== 304 || !(unchangedAge > firstAge)) throw new Error("304 did not preserve an advancing observation age");

writeFileSync(`${fixtureFile}.new`, "{invalid fixture\n");
renameSync(`${fixtureFile}.new`, fixtureFile);
let failedRefresh = null;
const failureDeadline = Date.now() + 15000;
while (Date.now() < failureDeadline) {
  const response = await fetch(new URL("api/state", base), { headers: { "If-None-Match": firstEtag } });
  failedRefresh = {
    status: response.status,
    cache: response.headers.get("x-multplx-cache"),
    refresh: response.headers.get("x-multplx-refresh"),
    error: response.headers.get("x-multplx-refresh-error"),
  };
  await response.arrayBuffer();
  if (failedRefresh.cache === "stale" && failedRefresh.refresh === "failed" && failedRefresh.error) break;
  await sleep(250);
}
if (failedRefresh?.cache !== "stale" || failedRefresh?.refresh !== "failed" || !failedRefresh?.error) {
  throw new Error(`refresh failure did not remain visible: ${JSON.stringify(failedRefresh)}`);
}
results.failed_refresh = {
  status: failedRefresh.status,
  cache: failedRefresh.cache,
  refresh: failedRefresh.refresh,
  error_header_present: true,
};
const failedPage = await browser.newPage({ viewport: { width: 1000, height: 700 } });
await failedPage.goto(base, { waitUntil: "domcontentloaded" });
await failedPage.locator("#state-banner").filter({ hasText: "Refresh failed. Showing the last good snapshot" }).waitFor();
results.failed_refresh.browser_banner_visible = true;
await failedPage.close();
writeFileSync(`${fixtureFile}.new`, originalFixture);
renameSync(`${fixtureFile}.new`, fixtureFile);
fixtureRestored = true;

for (let sample = 0; sample < 20; sample += 1) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  let dataFinishedAt = null;
  page.on("response", async (response) => {
    if (response.url().endsWith("/api/state")) { await response.finished(); dataFinishedAt = performance.now(); }
  });
  await page.goto(base, { waitUntil: "domcontentloaded" });
  await page.locator("#task-summary").filter({ hasText: /· 20 tasks ·/ }).waitFor();
  if (dataFinishedAt === null) throw new Error(`sample ${sample} lacked a completed state response`);
  results.interaction_samples_ms.push(Number((performance.now() - dataFinishedAt).toFixed(3)));
  await page.close();
}
results.interaction_after_data_ms = {
  samples: 20,
  p50: percentile(results.interaction_samples_ms, 0.5),
  p95: percentile(results.interaction_samples_ms, 0.95),
  max: Math.max(...results.interaction_samples_ms),
};
results.browser = await browser.version();
await browser.close();
if (results.errors.length) throw new Error(results.errors.join("; "));
writeFileSync(path.join(output, "browser-results.json"), `${JSON.stringify(results, null, 2)}\n`);
process.stdout.write(`${JSON.stringify(results.interaction_after_data_ms)}\n`);

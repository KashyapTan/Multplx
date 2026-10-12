#!/usr/bin/env node
// Isolated synthetic browser regression. No operational home or forge access.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { dashboardFixture } from "./fixtures/viz/dashboard.mjs";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = process.env.MX_VIZ_BROWSER_OUTPUT || "/tmp/mx-viz-dashboard-browser";
const { chromium } = await import(process.env.MX_PLAYWRIGHT_MODULE);
mkdirSync(output, { recursive: true });
const server = createServer((request, response) => {
  const name = request.url === "/" ? "index.html" : request.url.replace("/assets/", "");
  if (!["index.html", "app.js", "app.css", "agents-graph.js"].includes(name)) { response.writeHead(404); response.end(); return; }
  response.setHeader("Content-Type", name.endsWith("js") ? "text/javascript" : name.endsWith("css") ? "text/css" : "text/html");
  response.end(readFileSync(path.join(root, "share/viz", name), "utf8").replace("__MX_VIZ_POLL_MS__", "250"));
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const browser = await chromium.launch({ headless: true });
const errors = [];
const results = {};
try {
  for (const [layout, viewport] of Object.entries({ desktop: { width: 1440, height: 900 }, mobile: { width: 390, height: 844 } })) {
    const page = await browser.newPage({ viewport });
    page.on("pageerror", (error) => errors.push(error.message));
    const payload = dashboardFixture();
    await page.route("**/api/state", (route) => route.fulfill({ contentType: "application/json", body: JSON.stringify(payload), headers: { "X-Multplx-Cache": "fresh", "X-Multplx-Observation-Age-Ms": "0" } }));
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.locator("#task-summary").filter({ hasText: "7 active shown" }).waitFor();
    await page.keyboard.press("Tab");
    assert.equal(await page.locator(".skip-link").evaluate((node) => document.activeElement === node), true);
    await page.locator(".skip-link").evaluate((node) => node.blur());
    assert.equal(await page.locator("#task-history").getAttribute("open"), null);
    assert.equal(await page.locator("#task-reconciliation").getAttribute("open"), null);
    assert.equal(await page.locator("#task-list .task-row").count(), 7);
    assert.equal(await page.locator("#task-history-list .task-row").count(), 2);
    assert.equal(await page.locator("#task-reconciliation-list .task-row").count(), 2);
    assert.equal(await page.locator("#attention-list .attention-card").count(), 2);
    assert.equal(await page.locator(".attention-item").first().getAttribute("href"), "https://example.invalid/pull/1");
    assert.match(await page.locator(".attention-item").first().textContent(), /Build fantasy rankings and player comparison/);
    assert.match(await page.locator(".attention-item").nth(1).textContent(), /Should archived projects be included.*main orchestrator chat/s);
    assert.equal(await page.locator(".side-column details[open]").count(), 0);
    const mismatch = page.locator('#task-list [data-task-id$="task:mismatch"]');
    assert.match(await mismatch.locator(":scope > summary").textContent(), /In progress/);
    assert.doesNotMatch(await mismatch.locator(":scope > summary").textContent(), /idle|fresh|unknown|\/example/);
    await mismatch.locator(":scope > summary").click();
    assert.match(await mismatch.textContent(), /Agent availability.*No process observed/s);
    await mismatch.locator(".technical-details > summary").click();
    assert.match(await mismatch.textContent(), /Task ID.*mismatch.*Project checkout.*\/example\/dashboard/s);
    await mismatch.locator(":scope > summary").click();
    await page.locator("#search").fill("player statistics");
    assert.equal(await page.locator("#task-list .task-row").count(), 1);
    await page.locator("#clear-filters").click();
    await page.locator("#status-filter").selectOption("running");
    assert.equal(await page.locator("#task-list .task-row").count(), 2);
    await page.locator("#clear-filters").click();
    await page.locator("#task-history > summary").click();
    assert.equal(await page.locator("#task-history").getAttribute("open"), "");
    await page.locator("#task-history > summary").click();
    await page.evaluate(() => { document.activeElement?.blur(); window.scrollTo({ top: 0, behavior: "instant" }); });
    await page.screenshot({ path: path.join(output, `dashboard-${layout}.png`), fullPage: true });
    await page.locator("#agents-view-button").click();
    assert.equal(await page.locator("#agent-history").getAttribute("open"), null);
    assert.equal(await page.locator("#agents-tree [data-agent-card-key$='task:old-parent']").count(), 1, "completed parent preserves active child context");
    assert.equal(await page.locator("#agents-tree [data-agent-card-key$='task:finished']").count(), 0);
    assert.equal(await page.locator("#agents-tree [data-agent-card-key$='task:active-child']").count(), 1);
    await page.locator("[data-agent-collapse-key$='task:old-parent']").click();
    assert.equal(await page.locator("#agents-tree [data-agent-card-key$='task:active-child']").isVisible(), false);
    await page.locator("#agent-search").fill("current schedule");
    assert.equal(await page.locator("#agents-tree [data-agent-card-key$='task:active-child']").isVisible(), true);
    await page.locator("#agent-search").fill("");
    await page.locator("#agents-expand").click();
    await page.locator("#agents-root").click();
    await page.evaluate(() => { document.activeElement?.blur(); window.scrollTo({ top: 0, behavior: "instant" }); });
    await page.screenshot({ path: path.join(output, `agents-${layout}.png`), fullPage: true });
    // Every rendered descendant remains reachable in the graph's bounded scroll container.
    const lastCard = page.locator("#agents-tree [data-agent-card-key]").last();
    await lastCard.focus();
    await lastCard.evaluate((node) => node.scrollIntoView({ block: "nearest", behavior: "instant" }));
    assert.equal(await lastCard.isVisible(), true);
    assert.equal(await lastCard.evaluate((node) => {
      const card = node.getBoundingClientRect(); const view = document.querySelector("#agents-viewport").getBoundingClientRect();
      return card.bottom <= view.bottom + 1 && card.top >= view.top - 1;
    }), true);
    await page.locator("#agent-history > summary").click();
    await page.locator("#agent-history [data-agent-card-key$='task:finished']").click();
    assert.equal(await page.locator("#task-history").getAttribute("open"), "");
    assert.equal(await page.locator("#task-history [data-task-id$='task:finished']").getAttribute("open"), "");
    // Programmatic refresh: reopening a recorded PR restores human action; merging removes only its review action.
    payload.forge_observations[1].state = "open";
    await page.locator("#attention-count").filter({ hasText: "3 actionable" }).waitFor();
    payload.forge_observations[1].state = "closed";
    await page.locator("#attention-count").filter({ hasText: "2 actionable" }).waitFor();
    payload.forge_observations[0].state = "merged";
    await page.locator("#attention-count").filter({ hasText: "1 actionable" }).waitFor();
    assert.match(await page.locator(".attention-item").textContent(), /Should archived projects be included/);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
    assert.equal(overflow, false);
    results[layout] = { active: 7, history: 2, uncertain: 2, nested_context: true, filters: true, disclosures: true, forge_refresh: true, status_separation: true, overflow: false };
    await page.close();
  }
  assert.deepEqual(errors, []);
  writeFileSync(path.join(output, "dashboard-browser-results.json"), JSON.stringify(results, null, 2) + "\n");
  console.log(JSON.stringify(results));
} finally { await browser.close(); server.close(); }

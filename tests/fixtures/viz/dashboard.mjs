// Synthetic portfolio shaped like the reported clutter; contains no private records.
export function dashboardFixture() {
  const home = "/example/workspace";
  const root = `root-home:${home}`;
  const task = (id, title, state, extra = {}) => ({
    id, key: `${home}#task:${id}`, parent_id: root, root_id: root,
    owner: { home, parent_id: root }, title, state, role: "implementer", children: [],
    project: { id: "demo", display_name: "Example dashboard", path: "/example/dashboard" },
    freshness: { status: "fresh" }, observation: { state, quality: "current", agent_alive: false },
    decisions: [], dependencies: [], sessions: [], brief: {}, evidence: {}, ...extra,
  });
  const review = (url) => ({ delivery: { commit: "example-head" }, review_queue: { state: "ready", pr_ready: true, pr_url: url } });
  const tasks = [
    task("FANTASY_UI_1012", "FANTASY_UI_1012", "completed", { brief: { scope: "Build fantasy rankings and player comparison" }, evidence: review("https://example.invalid/pull/1") }),
    task("old-parent", "Completed feature coordinator", "completed", { role: "sub-orchestrator", children: [`${home}#task:active-child`] }),
    task("active-child", "Repair the current schedule", "running", { parent_id: `${home}#task:old-parent`, owner: { home, parent_id: `${home}#task:old-parent` } }),
    task("finished", "Delivered startup command", "completed", { sessions: [{ session_id: "retained-1", current: false }, { session_id: "retained-2", current: false }], evidence: review("https://example.invalid/pull/2") }),
    task("question", "Choose rollout scope", "completed", { decisions: [{ id: "scope", question: "Should archived projects be included?" }], evidence: review("https://example.invalid/pull/3") }),
    task("uncertain", "Earlier attempt needs observation", "runnable", { freshness: { status: "unknown" }, observation: { state: "unknown", quality: "unknown" } }),
    task("reported", "Validate player comparisons", "done", { completion: { kind: "worker-reported-done", parent_verified: false } }),
    task("mismatch", "Implement team box scores", "running", { observation: { state: "idle", quality: "current", agent_alive: false } }),
    task("long", "You are a worker. " + "scaffold ".repeat(400), "working", { brief: { scope: "Restore useful player statistics" }, summary: "Keep the original calculations and expose their sample size." }),
    task("sparse", "", "unknown", { brief: {} }),
    task("blocker", "Resolve missing service access", "completed", { dependencies: [{ task_id: "service", blocking: true, reason: "Service credentials need reconciliation" }] }),
  ];
  return {
    snapshot: { mx_home: home, roots: { state: `${home}/state` }, domains: { records: [], complete: true },
      portfolio: { schema: "mx-portfolio.v1", tasks, counts: { records: tasks.length }, freshness: { status: "fresh" } } },
    forge_observations: [
      { url: "https://example.invalid/pull/1", state: "open", freshness: "fresh", head: "example-head" },
      { url: "https://example.invalid/pull/2", state: "merged", freshness: "fresh", head: "example-head" },
      { url: "https://example.invalid/pull/3", state: "merged", freshness: "fresh", head: "example-head" },
    ], artifacts: [{ label: "Old session transcript", kind: "transcript", url: "/artifact/old.txt" }],
  };
}

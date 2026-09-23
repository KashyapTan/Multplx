"use strict";

const assert = require("node:assert/strict");
const { normalize, layout } = require("../../../share/viz/agents-graph.js");

const home = "/mx/root";
const task = (id, ownerHome = home, parent = "root", extra = {}) => ({
  key: `${ownerHome}#task:${id}`,
  id,
  title: id,
  role: "implementer",
  state: "queued-request",
  parent_id: parent,
  owner: { home: ownerHome },
  sessions: [],
  ...extra,
});
const snapshot = (tasks = [], domains = [], options = {}) => ({
  schema: "mx-system-snapshot.v1",
  mx_home: home,
  portfolio: {
    schema: "mx-portfolio.v1",
    tasks,
    counts: { records: tasks.length, truncated: 0 },
    freshness: { status: "fresh", partial: false, reasons: [] },
    ...options.portfolio,
  },
  domains: { complete: true, records: domains },
});
const coordinator = (id, taskHome, runtimeHome, parent = "root") => task(id, taskHome, parent, {
  role: "sub-orchestrator",
  domain_id: `domain-${id}`,
});
const domain = (id, taskHome, runtimeHome) => ({
  domain_id: `domain-${id}`,
  coordinator: {
    id,
    qualified_id: `${taskHome}#task:${id}`,
    owner_home: taskHome,
    runtime_home: runtimeHome,
    validated_home: runtimeHome,
    endpoint: { backend: "tmux", exists: true },
  },
});

{
  const graph = normalize(snapshot());
  assert.equal(graph.root.kind, "root");
  assert.equal(graph.root.state, "unknown");
  assert.equal(graph.root.session, "session not observed");
  assert.deepEqual(graph.nodes, []);
  assert.equal(layout(graph).order.length, 1);
}

{
  const graph = normalize(snapshot([task("one"), task("two")]));
  assert.deepEqual(graph.root.children.map((node) => node.id), ["one", "two"]);
  assert.equal(layout(graph).order.length, 3);
  assert.equal(graph.nodes[0].session, "not started");
}

{
  const coordinatorHome = "/mx/root/coordinator-a";
  const coord = coordinator("coordinator-a", home, coordinatorHome);
  const worker = task("worker", coordinatorHome, "coordinator-a");
  const graph = normalize(snapshot([coord, worker], [domain("coordinator-a", home, coordinatorHome)]));
  const coordinatorNode = graph.nodes.find((node) => node.id === "coordinator-a");
  assert.equal(coordinatorNode.parentKey, graph.rootKey);
  assert.deepEqual(coordinatorNode.children.map((node) => node.id), ["worker"]);
  assert.equal(graph.unresolved.length, 0);
}

{
  const homeA = "/mx/root/coord-a";
  const homeB = "/mx/root/coord-b";
  const tasks = [
    coordinator("coordinator-a", home, homeA),
    coordinator("coordinator-b", home, homeB),
    task("same-worker", homeA, "coordinator-a"),
    task("same-worker", homeB, "coordinator-b"),
  ];
  const graph = normalize(snapshot(tasks, [domain("coordinator-a", home, homeA), domain("coordinator-b", home, homeB)]));
  const duplicates = graph.nodes.filter((node) => node.id === "same-worker");
  assert.equal(new Set(duplicates.map((node) => node.key)).size, 2);
  assert.equal(graph.unresolved.length, 0);
  assert.deepEqual(graph.nodes.filter((node) => node.id.startsWith("coordinator-")).map((node) => node.children[0].home), [homeA, homeB]);
}

{
  const unknownParent = task("orphan", home, "missing-coordinator");
  const descendant = task("descendant", home, "orphan");
  const graph = normalize(snapshot([unknownParent, descendant]));
  assert.match(graph.unresolved.find((node) => node.id === "orphan").unresolved, /parent not found/);
  assert.equal(graph.unresolved.find((node) => node.id === "orphan").children[0].id, "descendant");
  assert.deepEqual(new Set(layout(graph).order), new Set([graph.rootKey, ...graph.nodes.map((node) => node.key)]));
}

{
  const a = task("cycle-a", home, "cycle-b");
  const b = task("cycle-b", home, "cycle-a");
  const graph = normalize(snapshot([a, b]));
  assert.equal(graph.unresolved.length, 2);
  assert.ok(graph.unresolved.every((node) => /cyclic/.test(node.unresolved)));
  assert.equal(layout(graph).order.length, 3);
}

{
  const conflicted = task("wrong-key", home, "root", { key: "/elsewhere#task:wrong-key" });
  const unowned = { ...task("unowned"), owner: null, key: null };
  const duplicate = task("dup");
  const graph = normalize(snapshot([conflicted, unowned, duplicate, task("dup")]));
  assert.ok(graph.unresolved.some((node) => /conflicts with owner home/.test(node.unresolved)));
  assert.ok(graph.unresolved.some((node) => /owner home unavailable/.test(node.unresolved)));
  assert.ok(graph.unresolved.filter((node) => node.id === "dup").every((node) => /duplicate qualified/.test(node.unresolved)));
  assert.equal(layout(graph).order.length, graph.nodes.length + 1);
}

{
  const coord = coordinator("coordinator-a", home, "/mx/root/coordinator-a");
  const mismatchedDomain = domain("coordinator-a", home, "/mx/root/coordinator-a");
  mismatchedDomain.coordinator.owner_home = "/mx/foreign";
  const graph = normalize(snapshot([coord], [mismatchedDomain]));
  assert.match(graph.unresolved[0].unresolved, /domain identity conflicts/);
  assert.equal(graph.root.children.length, 0);
}

{
  const coord = coordinator("coordinator-a", home, "/mx/root/coordinator-a");
  const mapping = domain("coordinator-a", home, "/mx/root/coordinator-a");
  const duplicateMapping = { ...mapping, coordinator: { ...mapping.coordinator } };
  const graph = normalize(snapshot([coord], [mapping, duplicateMapping]));
  assert.match(graph.unresolved[0].unresolved, /conflicting coordinator domain mappings/);
  assert.equal(graph.root.children.length, 0);
}

{
  const tasks = Array.from({ length: 500 }, (_, index) => task(`leaf-${index}`, home, "root"));
  const graph = normalize(snapshot(tasks, [], { portfolio: { counts: { records: 500, truncated: 0 } } }));
  const first = layout(graph);
  const second = layout(graph);
  assert.equal(first.order.length, 501);
  assert.deepEqual(first.order, second.order);
  assert.equal(graph.root.children.length, 500);
}

{
  const graph = normalize({ mx_home: home, portfolio: { schema: "unsupported", tasks: [] } });
  assert.equal(graph.root.state, "unknown");
  assert.equal(graph.partial, true);
  assert.ok(graph.partialReasons.some((reason) => /domain projection/.test(reason)));
}

process.stdout.write("agent graph normalization and layout fixtures passed\n");

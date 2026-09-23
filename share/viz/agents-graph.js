"use strict";

(function exposeAgentGraph(global) {
  const text = (value) => typeof value === "string" && value.trim() ? value : null;
  const rows = (value) => Array.isArray(value) ? value : [];
  const keyFor = (home, id) => `${home}#task:${id}`;

  function normalize(snapshot) {
    const rootHome = text(snapshot?.mx_home);
    const rootKey = `main-orchestrator:${rootHome || "unknown-home"}`;
    const rootRef = rootHome ? `root-home:${rootHome}` : null;
    const root = {
      key: rootKey,
      id: "Main orchestrator",
      kind: "root",
      role: "main orchestrator",
      home: rootHome,
      state: "unknown",
      freshness: text(snapshot?.portfolio?.freshness?.status) || "unknown",
      session: "session not observed",
      issues: rootHome ? [] : ["root home identity unavailable"],
      task: null,
      order: -1,
      parentKey: null,
      children: [],
    };
    const portfolio = snapshot?.portfolio || {};
    const sourceTasks = rows(portfolio.tasks);
    const sourceCounts = new Map();
    for (const task of sourceTasks) {
      const id = text(task?.id) || "unknown task";
      const home = text(task?.owner?.home);
      const rawKey = text(task?.key) || (home ? keyFor(home, id) : null);
      if (rawKey) sourceCounts.set(rawKey, (sourceCounts.get(rawKey) || 0) + 1);
    }

    const domains = rows(snapshot?.domains?.records);
    const coordinatorByKey = new Map();
    for (const domain of domains) {
      const coordinator = domain?.coordinator;
      const key = text(coordinator?.qualified_id);
      if (key) {
        if (!coordinatorByKey.has(key)) coordinatorByKey.set(key, []);
        coordinatorByKey.get(key).push(domain);
      }
    }

    const nodes = [];
    const sourceKeys = new Map();
    const homeIds = new Map();
    sourceTasks.forEach((task, order) => {
      const id = text(task?.id) || "unknown task";
      const home = text(task?.owner?.home);
      const sourceKey = text(task?.key) || (home ? keyFor(home, id) : null);
      const repeated = sourceKey && sourceCounts.get(sourceKey) > 1;
      const duplicateIndex = repeated ? (sourceKeys.get(sourceKey)?.length || 0) + 1 : 0;
      const key = repeated ? `${sourceKey}#ambiguous:${duplicateIndex}` : sourceKey || `unqualified-task:${order}:${id}`;
      const current = rows(task?.sessions).find((session) => session?.current === true
        && (text(session?.endpoint) || text(session?.session_id)));
      const retained = rows(task?.sessions).some((session) => session?.current !== true);
      const domainRows = coordinatorByKey.get(sourceKey) || [];
      const candidateDomain = domainRows.length === 1 ? domainRows[0] : null;
      const candidateCoordinator = candidateDomain?.coordinator;
      const domainMatchesTask = candidateCoordinator
        && text(candidateCoordinator.id) === id
        && text(candidateCoordinator.qualified_id) === sourceKey
        && text(candidateCoordinator.owner_home) === home;
      const domain = domainMatchesTask ? candidateDomain : null;
      const state = text(task?.state) || text(task?.state?.state) || text(task?.state?.status)
        || text(task?.current_state?.state) || text(task?.current_state?.status) || "unknown";
      const provider = text(current?.provider)
        || text(domain?.coordinator?.endpoint?.backend)
        || text(domain?.coordinator?.endpoint?.provider)
        || (retained ? text(rows(task?.sessions).find((session) => text(session?.provider))?.provider) : null);
      let session = "none recorded";
      let sessionDetails = "no current session observed";
      if (current) {
        session = text(current.state) || "state unknown";
        sessionDetails = `current-attempt reference recorded (${session})`;
      } else if (domain?.coordinator?.endpoint?.exists === true) {
        session = "endpoint observed";
        sessionDetails = `${provider || "provider unknown"} endpoint observed`;
      } else if (retained) {
        session = "retained reference";
        sessionDetails = "retained session only; current session not observed";
      }
      else if (task?.intake || ["queued-request", "accepted-request", "waiting-external", "queued", "parked"].includes(state)) {
        session = "not started";
        sessionDetails = "accepted or queued without a recorded session";
      }
      const node = {
        key,
        sourceKey,
        id,
        title: text(task?.title) || id,
        kind: "task",
        role: text(task?.role) || "assignment",
        home,
        state,
        freshness: text(task?.freshness?.status) || "unknown",
        provider,
        session,
        sessionDetails,
        task,
        domain,
        order,
        parentKey: null,
        children: [],
        unresolved: null,
        issues: [],
      };
      if (repeated) node.issues.push("duplicate qualified task identity");
      if (text(task?.key) && home && task.key !== keyFor(home, id)) {
        node.issues.push("qualified task identity conflicts with owner home and task ID");
      }
      if (domainRows.length > 1) node.issues.push("conflicting coordinator domain mappings");
      if (candidateDomain && !domainMatchesTask) node.issues.push("coordinator domain identity conflicts with task owner");
      if (!home) node.issues.push("owner home unavailable");
      nodes.push(node);
      if (sourceKey) {
        if (!sourceKeys.has(sourceKey)) sourceKeys.set(sourceKey, []);
        sourceKeys.get(sourceKey).push(node);
      }
      if (home) {
        const alias = `${home}\u0000${id}`;
        if (!homeIds.has(alias)) homeIds.set(alias, []);
        homeIds.get(alias).push(node);
      }
    });

    const byKey = new Map(nodes.map((node) => [node.key, node]));
    const runtimeOwners = new Map();
    for (const node of nodes) {
      const coordinator = node.domain?.coordinator;
      const runtimeHome = text(coordinator?.runtime_home);
      const validatedHome = text(coordinator?.validated_home);
      if (node.role === "sub-orchestrator" && runtimeHome && runtimeHome === validatedHome
          && text(coordinator.id) === node.id
          && text(coordinator.qualified_id) === node.sourceKey
          && text(coordinator.owner_home) === node.home) {
        if (!runtimeOwners.has(runtimeHome)) runtimeOwners.set(runtimeHome, []);
        runtimeOwners.get(runtimeHome).push(node);
      }
    }

    function parentFor(node) {
      if (!node.home) return { issue: "owner home unavailable" };
      const task = node.task;
      const reference = text(task?.parent_id) || text(task?.owner?.parent_id) || text(task?.owner?.coordinator);
      if (!reference) {
        if (rootHome && node.home === rootHome && text(task?.root_id) === rootRef) return { key: rootKey };
        return { issue: "parent owner unavailable" };
      }
      if (rootRef && reference === rootRef) return { key: rootKey };
      const qualified = sourceKeys.get(reference);
      if (qualified) return qualified.length === 1 ? { key: qualified[0].key } : { issue: "ambiguous qualified parent" };
      const local = homeIds.get(`${node.home}\u0000${reference}`);
      if (local) return local.length === 1 ? { key: local[0].key } : { issue: "ambiguous same-home parent" };
      if (reference === "root" || reference === `root-home:${node.home}`) {
        if (rootHome && node.home === rootHome) return { key: rootKey };
        const owners = runtimeOwners.get(node.home);
        if (owners?.length === 1) return { key: owners[0].key };
        if (owners?.length > 1) return { issue: "ambiguous validated coordinator runtime home" };
      }
      if (reference.startsWith("root-home:")) {
        const referencedHome = reference.slice("root-home:".length);
        const owners = runtimeOwners.get(referencedHome);
        if (owners?.length === 1 && node.home === referencedHome) return { key: owners[0].key };
        if (owners?.length > 1) return { issue: "ambiguous validated coordinator runtime home" };
      }
      const runtimeParents = runtimeOwners.get(node.home);
      if (runtimeParents?.length === 1
          && (reference === runtimeParents[0].id || reference === runtimeParents[0].sourceKey)) {
        return { key: runtimeParents[0].key };
      }
      if (runtimeParents?.length > 1
          && runtimeParents.some((owner) => reference === owner.id || reference === owner.sourceKey)) {
        return { issue: "ambiguous validated coordinator runtime owner" };
      }
      return { issue: `parent not found: ${reference}`, reference };
    }

    for (const node of nodes) {
      if (node.issues.length) {
        node.unresolved = node.issues[0];
        continue;
      }
      const parent = parentFor(node);
      if (parent.issue) {
        node.unresolved = parent.issue;
        continue;
      }
      node.parentKey = parent.key;
    }

    const visiting = new Map();
    const visited = new Set();
    const cycleKeys = new Set();
    function visit(key, chain) {
      if (visited.has(key)) return;
      if (visiting.has(key)) {
        for (const cycleKey of chain.slice(visiting.get(key))) cycleKeys.add(cycleKey);
        return;
      }
      visiting.set(key, chain.length);
      chain.push(key);
      const node = byKey.get(key);
      if (node?.parentKey && node.parentKey !== rootKey) visit(node.parentKey, chain);
      chain.pop();
      visiting.delete(key);
      visited.add(key);
    }
    for (const node of nodes) visit(node.key, []);
    for (const key of cycleKeys) {
      const node = byKey.get(key);
      node.parentKey = null;
      node.unresolved = "cyclic parent relationship";
    }

    const rootChildren = [];
    const unresolved = [];
    for (const node of nodes) {
      if (node.unresolved) {
        unresolved.push(node);
      } else if (node.parentKey === rootKey) {
        rootChildren.push(node);
      } else {
        const parent = byKey.get(node.parentKey);
        if (!parent) {
          node.parentKey = null;
          node.unresolved = `parent not found: ${node.parentKey}`;
          unresolved.push(node);
        } else parent.children.push(node);
      }
    }
    root.children = rootChildren;
    const sortTree = (parent) => {
      parent.children.sort((left, right) => Number(right.role === "sub-orchestrator") - Number(left.role === "sub-orchestrator")
        || left.order - right.order || left.key.localeCompare(right.key));
      for (const child of parent.children) sortTree(child);
    };
    sortTree(root);
    unresolved.sort((left, right) => left.order - right.order || left.key.localeCompare(right.key));

    const total = Number.isSafeInteger(portfolio?.counts?.records) ? portfolio.counts.records : nodes.length;
    const truncated = Number.isSafeInteger(portfolio?.counts?.truncated) ? portfolio.counts.truncated : Math.max(0, total - nodes.length);
    const freshness = portfolio?.freshness || {};
    const domainsComplete = snapshot?.domains?.complete === true;
    const partial = freshness.partial === true || truncated > 0 || !domainsComplete;
    return {
      root,
      rootKey,
      nodes,
      unresolved,
      total,
      truncated,
      partial,
      partialReasons: [...new Set([
        ...rows(freshness.reasons).filter((reason) => typeof reason === "string"),
        ...(truncated ? [`${truncated} assignment${truncated === 1 ? "" : "s"} omitted by the portfolio limit`] : []),
        ...(!domainsComplete ? ["domain projection incomplete or unavailable"] : []),
      ])],
    };
  }

  function layout(graph) {
    const depth = new Map();
    const order = [];
    const pending = [{ node: graph.root, depth: 0 }];
    for (const node of graph.unresolved) pending.push({ node, depth: 1 });
    for (const node of graph.nodes) {
      if (node.unresolved && !graph.unresolved.includes(node)) pending.push({ node, depth: 1 });
    }
    while (pending.length) {
      const { node, depth: currentDepth } = pending.shift();
      if (depth.has(node.key)) continue;
      depth.set(node.key, currentDepth);
      order.push(node.key);
      for (const child of node.children) {
        pending.push({ node: child, depth: currentDepth + 1 });
      }
    }
    for (const node of graph.nodes) {
      if (depth.has(node.key)) continue;
      pending.push({ node, depth: 1 });
      while (pending.length) {
        const { node: current, depth: currentDepth } = pending.shift();
        if (depth.has(current.key)) continue;
        depth.set(current.key, currentDepth);
        order.push(current.key);
        for (const child of current.children) pending.push({ node: child, depth: currentDepth + 1 });
      }
    }
    return { depth, order };
  }

  const api = { normalize, layout };
  global.MxAgentsGraph = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);

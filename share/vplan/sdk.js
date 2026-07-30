(() => {
  "use strict";

  const COMMENT_BLOCK_ID = "vplan-comments";
  const token = document.querySelector('meta[name="vplan-token"]')?.content || "";
  const overlayNodes = new Set();
  const queue = [];
  let existing = [];
  let selected = null;
  let hovered = null;
  let submitting = false;

  function normalizeText(value, limit = 320) {
    const normalized = String(value || "").replace(/\s+/g, " ").trim();
    return normalized.length > limit ? `${normalized.slice(0, limit - 1)}…` : normalized;
  }

  function escapeIdentifier(value) {
    if (window.CSS?.escape) {
      return window.CSS.escape(value);
    }
    return value.replace(/[^a-zA-Z0-9_-]/g, (character) => `\\${character}`);
  }

  function isOverlayNode(node) {
    return node instanceof Element && (node.closest("[data-vplan-ui]") || overlayNodes.has(node));
  }

  function selectorFor(element) {
    if (element.id && document.querySelectorAll(`#${escapeIdentifier(element.id)}`).length === 1) {
      return `#${escapeIdentifier(element.id)}`;
    }
    const parts = [];
    let current = element;
    while (current && current !== document.body && current !== document.documentElement) {
      let part = current.tagName.toLowerCase();
      if (current.parentElement) {
        const siblings = [...current.parentElement.children];
        const position = siblings.indexOf(current) + 1;
        part += `:nth-child(${position})`;
      }
      parts.unshift(part);
      current = current.parentElement;
    }
    return `body > ${parts.join(" > ")}`;
  }

  function nearestHeading(element) {
    const section = element.closest("section, article, main, aside");
    const localHeading = section?.querySelector("h1, h2, h3, h4, h5, h6");
    if (localHeading) {
      return normalizeText(localHeading.textContent, 200);
    }
    const headings = [...document.querySelectorAll("h1, h2, h3, h4, h5, h6")];
    let nearest = "";
    for (const heading of headings) {
      const relation = heading.compareDocumentPosition(element);
      if (relation & Node.DOCUMENT_POSITION_FOLLOWING || heading.contains(element)) {
        nearest = normalizeText(heading.textContent, 200);
      }
    }
    return nearest;
  }

  function selectedTextFor(element) {
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0 || selection.isCollapsed) {
      return "";
    }
    const range = selection.getRangeAt(0);
    const common =
      range.commonAncestorContainer.nodeType === Node.ELEMENT_NODE
        ? range.commonAncestorContainer
        : range.commonAncestorContainer.parentElement;
    if (!common || (!element.contains(common) && !common.contains(element))) {
      return "";
    }
    return normalizeText(selection.toString(), 4096);
  }

  function setSelected(element) {
    if (!(element instanceof Element) || isOverlayNode(element)) {
      return;
    }
    selected?.element.classList.remove("vplan-selected-target");
    const selectionText = selectedTextFor(element);
    selected = {
      element,
      selector: selectorFor(element),
      anchor_text: selectionText || normalizeText(element.textContent, 4096),
      nearest_heading: nearestHeading(element),
      kind: selectionText ? "text selection" : "element",
    };
    element.classList.add("vplan-selected-target");
    renderTarget();
    panel.hidden = false;
    commentInput.focus();
  }

  function parseExisting() {
    const block = document.getElementById(COMMENT_BLOCK_ID);
    if (!block) {
      return [];
    }
    try {
      const parsed = JSON.parse(block.textContent || "[]");
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  }

  function findTextRange(element, snippet) {
    const needle = normalizeText(snippet, 4096);
    if (!needle) {
      return null;
    }
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    const nodes = [];
    let combined = "";
    while (walker.nextNode()) {
      const node = walker.currentNode;
      const text = node.nodeValue || "";
      if (!text) {
        continue;
      }
      nodes.push({ node, start: combined.length, end: combined.length + text.length });
      combined += text;
    }
    const directIndex = combined.indexOf(snippet);
    const normalizedIndex = directIndex >= 0 ? directIndex : combined.indexOf(needle);
    if (normalizedIndex < 0) {
      return null;
    }
    const endIndex = normalizedIndex + (directIndex >= 0 ? snippet.length : needle.length);
    const startNode = nodes.find((entry) => normalizedIndex >= entry.start && normalizedIndex < entry.end);
    const endNode = [...nodes].reverse().find((entry) => endIndex > entry.start && endIndex <= entry.end);
    if (!startNode || !endNode) {
      return null;
    }
    const range = document.createRange();
    range.setStart(startNode.node, normalizedIndex - startNode.start);
    range.setEnd(endNode.node, endIndex - endNode.start);
    return range;
  }

  function removePositionedOverlays() {
    document.querySelectorAll(".vplan-existing-pin, .vplan-range-highlight").forEach((node) => {
      overlayNodes.delete(node);
      node.remove();
    });
  }

  function addRangeHighlights(range, resolved) {
    for (const rect of range.getClientRects()) {
      if (rect.width < 1 || rect.height < 1) {
        continue;
      }
      const marker = document.createElement("span");
      marker.className = "vplan-range-highlight";
      marker.dataset.vplanUi = "true";
      marker.style.left = `${rect.left + window.scrollX}px`;
      marker.style.top = `${rect.top + window.scrollY}px`;
      marker.style.width = `${rect.width}px`;
      marker.style.height = `${rect.height}px`;
      if (resolved) {
        marker.style.opacity = "0.35";
      }
      document.body.append(marker);
      overlayNodes.add(marker);
    }
  }

  function positionExistingPins() {
    removePositionedOverlays();
    existing.forEach((comment, index) => {
      let element;
      try {
        element = document.querySelector(comment.selector);
      } catch {
        element = null;
      }
      if (!element) {
        return;
      }
      const range = comment.anchor_text ? findTextRange(element, comment.anchor_text) : null;
      if (range) {
        addRangeHighlights(range, Boolean(comment.resolved));
      }
      const rect = (range?.getBoundingClientRect() || element.getBoundingClientRect());
      const pin = document.createElement("button");
      pin.type = "button";
      pin.className = "vplan-existing-pin";
      pin.dataset.vplanUi = "true";
      pin.dataset.resolved = String(Boolean(comment.resolved));
      pin.textContent = String(index + 1);
      pin.title = comment.comment || "Review comment";
      pin.style.left = `${rect.right + window.scrollX}px`;
      pin.style.top = `${rect.top + window.scrollY}px`;
      pin.addEventListener("click", () => {
        panel.hidden = false;
        document.querySelector(`[data-existing-index="${index}"]`)?.scrollIntoView({
          behavior: "smooth",
          block: "nearest",
        });
      });
      document.body.append(pin);
      overlayNodes.add(pin);
    });
  }

  function createCard(comment, index, kind) {
    const card = document.createElement("article");
    card.className = kind === "existing" ? "vplan-comment-card" : "vplan-queue-card";
    card.dataset.vplanUi = "true";
    if (kind === "existing") {
      card.dataset.existingIndex = String(index);
      card.dataset.resolved = String(Boolean(comment.resolved));
    }
    const heading = document.createElement("div");
    heading.className = "vplan-card-heading";
    heading.textContent =
      comment.nearest_heading || comment.selector || (kind === "existing" ? `Comment ${index + 1}` : "Queued comment");
    const body = document.createElement("p");
    body.className = "vplan-card-body";
    body.textContent = comment.comment;
    const anchor = document.createElement("div");
    anchor.className = "vplan-card-anchor";
    anchor.textContent = normalizeText(comment.anchor_text, 180) || comment.selector;
    card.append(heading, body, anchor);
    if (kind === "queued") {
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "vplan-queue-remove";
      remove.textContent = "Remove";
      remove.addEventListener("click", () => {
        queue.splice(index, 1);
        renderQueue();
      });
      card.append(remove);
    }
    return card;
  }

  function renderExisting() {
    existingList.replaceChildren();
    if (existing.length === 0) {
      const empty = document.createElement("p");
      empty.className = "vplan-help";
      empty.textContent = "No persisted comments yet.";
      existingList.append(empty);
      return;
    }
    existing.forEach((comment, index) => existingList.append(createCard(comment, index, "existing")));
  }

  function renderTarget() {
    if (!selected) {
      targetSummary.dataset.ready = "false";
      targetSummary.textContent = "Click an artifact element or select text, then click its owning element.";
      addButton.disabled = true;
      return;
    }
    targetSummary.dataset.ready = "true";
    const heading = selected.nearest_heading ? ` under “${selected.nearest_heading}”` : "";
    targetSummary.textContent = `${selected.kind}${heading}: ${normalizeText(selected.anchor_text, 180) || selected.selector}`;
    addButton.disabled = commentInput.value.trim().length === 0;
  }

  function renderQueue() {
    queueList.replaceChildren();
    queue.forEach((comment, index) => queueList.append(createCard(comment, index, "queued")));
    if (queue.length === 0) {
      const empty = document.createElement("p");
      empty.className = "vplan-help";
      empty.textContent = "No comments queued.";
      queueList.append(empty);
    }
    toggle.dataset.count = String(queue.length);
    confirmButton.disabled = submitting || queue.length === 0;
    confirmButton.textContent = submitting ? "Saving…" : `Confirm & Save (${queue.length})`;
  }

  function createPanel() {
    const toggleButton = document.createElement("button");
    toggleButton.id = "vplan-review-toggle";
    toggleButton.type = "button";
    toggleButton.dataset.vplanUi = "true";
    toggleButton.dataset.count = "0";
    toggleButton.textContent = "Review";

    const reviewPanel = document.createElement("aside");
    reviewPanel.id = "vplan-review-panel";
    reviewPanel.dataset.vplanUi = "true";
    reviewPanel.innerHTML = `
      <header class="vplan-panel-header" data-vplan-ui>
        <strong>vplan review</strong>
        <button type="button" aria-label="Close review panel" data-close>×</button>
      </header>
      <div class="vplan-panel-scroll" data-vplan-ui>
        <p class="vplan-help">Click an element to anchor a comment. Select text first when the exact words matter.</p>
        <div class="vplan-target-summary" data-target-summary data-ready="false"></div>
        <textarea id="vplan-comment-input" placeholder="What should change?" maxlength="20000"></textarea>
        <button type="button" class="vplan-action vplan-action-primary" data-add disabled>Add to queue</button>
        <h2 class="vplan-section-title">Queued</h2>
        <div class="vplan-queue-list" data-queue-list></div>
        <h2 class="vplan-section-title">Persisted history</h2>
        <div class="vplan-comment-list" data-existing-list></div>
      </div>
      <footer class="vplan-panel-footer" data-vplan-ui>
        <p id="vplan-confirm-status" role="status"></p>
        <button type="button" id="vplan-confirm" class="vplan-action" disabled>Confirm & Save (0)</button>
      </footer>
    `;
    document.body.append(toggleButton, reviewPanel);
    overlayNodes.add(toggleButton);
    overlayNodes.add(reviewPanel);
    return { toggleButton, reviewPanel };
  }

  const { toggleButton: toggle, reviewPanel: panel } = createPanel();
  const targetSummary = panel.querySelector("[data-target-summary]");
  const commentInput = panel.querySelector("#vplan-comment-input");
  const addButton = panel.querySelector("[data-add]");
  const queueList = panel.querySelector("[data-queue-list]");
  const existingList = panel.querySelector("[data-existing-list]");
  const confirmButton = panel.querySelector("#vplan-confirm");
  const confirmStatus = panel.querySelector("#vplan-confirm-status");

  existing = parseExisting();
  renderExisting();
  renderTarget();
  renderQueue();
  positionExistingPins();

  toggle.addEventListener("click", () => {
    panel.hidden = !panel.hidden;
  });
  panel.querySelector("[data-close]").addEventListener("click", () => {
    panel.hidden = true;
  });
  commentInput.addEventListener("input", renderTarget);
  addButton.addEventListener("click", () => {
    const comment = commentInput.value.trim();
    if (!selected || !comment) {
      return;
    }
    queue.push({
      id: `c-${Date.now().toString(36)}-${crypto.getRandomValues(new Uint32Array(1))[0].toString(36)}`,
      selector: selected.selector,
      anchor_text: selected.anchor_text,
      nearest_heading: selected.nearest_heading,
      comment,
      ts: new Date().toISOString(),
      resolved: false,
    });
    commentInput.value = "";
    selected.element.classList.remove("vplan-selected-target");
    selected = null;
    renderTarget();
    renderQueue();
  });
  confirmButton.addEventListener("click", async () => {
    if (submitting || queue.length === 0) {
      return;
    }
    submitting = true;
    confirmStatus.textContent = "Saving comments into the artifact…";
    renderQueue();
    try {
      const response = await fetch("/confirm", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Vplan-Token": token,
        },
        body: JSON.stringify({ comments: queue }),
      });
      const result = await response.json().catch(() => ({}));
      if (!response.ok) {
        throw new Error(result.error || `server returned HTTP ${response.status}`);
      }
      confirmStatus.textContent = `Saved ${result.saved} comment${result.saved === 1 ? "" : "s"}. This review is closing.`;
      confirmButton.textContent = "Saved";
      queue.length = 0;
      setTimeout(() => {
        panel.querySelector(".vplan-panel-scroll").replaceChildren(
          Object.assign(document.createElement("p"), {
            className: "vplan-help",
            textContent: "Comments are saved in the HTML file. You can close this tab.",
          }),
        );
      }, 200);
    } catch (error) {
      submitting = false;
      confirmStatus.textContent = `Save failed: ${error.message}`;
      renderQueue();
    }
  });

  document.addEventListener(
    "mouseover",
    (event) => {
      const element = event.target;
      if (!(element instanceof Element) || isOverlayNode(element)) {
        return;
      }
      if (hovered && hovered !== selected?.element) {
        hovered.classList.remove("vplan-hover-target");
      }
      hovered = element;
      if (hovered !== selected?.element) {
        hovered.classList.add("vplan-hover-target");
      }
    },
    true,
  );
  document.addEventListener(
    "mouseout",
    (event) => {
      const element = event.target;
      if (element instanceof Element && element !== selected?.element) {
        element.classList.remove("vplan-hover-target");
      }
    },
    true,
  );
  document.addEventListener(
    "click",
    (event) => {
      const element = event.target;
      if (!(element instanceof Element) || isOverlayNode(element)) {
        return;
      }
      if (element.closest("a, button, input, select, textarea, summary, [contenteditable='true']")) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      setSelected(element);
    },
    true,
  );

  let positionFrame = 0;
  const schedulePinPosition = () => {
    cancelAnimationFrame(positionFrame);
    positionFrame = requestAnimationFrame(positionExistingPins);
  };
  window.addEventListener("scroll", schedulePinPosition, { passive: true });
  window.addEventListener("resize", schedulePinPosition);
})();

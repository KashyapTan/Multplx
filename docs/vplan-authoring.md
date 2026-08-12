# Authoring vplan artifacts

Use a vplan artifact when a plan, comparison, diagram, table, report, code review, or other structured response is easier to understand visually than as prose.
Use plain chat for a yes-or-no decision.
Task-linked artifacts live at `data/<id>/plan.html` and start from `bin/mx-vplan.sh new data/<id>/plan.html`.
That stable command is Rust-backed by default; this authoring contract and the frozen browser assets are unchanged by the runtime port.
The artifact is broker-authored and maintainer-facing.
Actors may supply evidence, but the broker owns the review surface and the unresolved-decision return path.

This guide is the single authoring owner for vplan artifacts.
It adapts the visual hierarchy, overflow, design selection, and seven playbook rules from the upstream [Lavish authoring guide](https://github.com/kunchenguid/lavish-axi/blob/main/skills/lavish/SKILL.md) and [playbook catalog](https://github.com/kunchenguid/lavish-axi/blob/main/src/playbooks.js) to Multplx's offline, one-shot review loop.
[`vplan.md`](vplan.md) owns commands, review mechanics, persistence, and run records.

## Workflow

1. Verify the current-state claims against the codebase, product, or source material before writing the artifact.
2. Create the artifact from the seed template with `bin/mx-vplan.sh new data/<id>/plan.html`.
3. Choose every applicable playbook below before shaping the page because one artifact often combines a plan, comparison, table, and diagram.
4. Replace the seed content with a self-contained explanation whose first screen makes the goal and review target obvious.
5. Use relative paths for local images, CSS, fonts, and scripts, and never use root-relative or external asset URLs.
6. Inspect the file directly and through the review server at narrow and wide widths before asking the maintainer to review it.
7. Serve it with `bin/mx-vplan.sh review data/<id>/plan.html` and do not edit it while its run record exists.
8. After confirmation, read the persisted feedback with `bin/mx-vplan.sh comments data/<id>/plan.html`, update the plan, and set addressed comments to `"resolved": true` in the JSON block rather than deleting them.
9. Re-serve for another round when the changed structure benefits from visual confirmation.
10. Before treating the review as complete, follow `decision-hold-lifecycle` for every unresolved maintainer decision the review exposed.

## Design direction

Choose the design direction in this priority order and stop at the first source that applies.

1. Match the maintainer's stated look or named design system.
2. Otherwise inspect the project the artifact describes and reuse its CSS tokens, component library, typography, spacing, and brand assets.
3. When neither source yields a system, use the vendored vplan seed template.

If the artifact previews a product interface, render that interface in the product's own design system even when Multplx is running from another repository.
State which design source you used when presenting the artifact.
Do not introduce CDN styling, remote fonts, hosted scripts, or third-party rendering dependencies.

## Visual hierarchy and layout safety

- Lead with the goal, decision, risk, or next action that matters most.
- Prefer sections, cards, tables, diagrams, annotated snippets, and aligned comparisons over long prose.
- Show existing interface state with an embedded screenshot when the real UI can be captured read-only.
- Reserve prose for rationale, tradeoffs, caveats, and facts that cannot be shown.
- Give typography, spacing, color, and layout a deliberate point of view.
- Use `minmax(0, 1fr)` for every grid track that can contain variable-width content.
- Put `min-width: 0` on flex and grid children so nested content can shrink.
- Wrap, truncate, scroll, or otherwise contain long paths, URLs, symbols, badges, and monospace status text deliberately.
- Put wide tables in an overflow container instead of letting the page overflow.
- Keep images, diagrams, video, SVG, and canvas content within the available width.
- Use color with text or shape rather than as the only status signal.
- Keep every review target large and distinct enough to click precisely.

## Diagram playbook

Use this playbook for relationships, flows, states, sequences, and architecture.

- Lead with the question the diagram answers instead of the implementation detail that produced it.
- Use Mermaid when automatic node placement and edge routing matter more than prose-heavy nodes.
- Use CSS, SVG, or positioned HTML when each node needs detailed prose, code, or controls.
- Use a small overview diagram followed by detailed module cards for large systems.
- Keep the first visual to the core relationship and move dense evidence and file references below it.
- Separate topology from detail so a complex overview stays readable.
- Prefer top-down flow for multi-step diagrams unless a short linear sequence reads better left to right.
- Quote Mermaid labels that contain punctuation or code-like names.
- Match Mermaid's theme variables to the page palette before rendering.
- Do not hand-build a flow from flexbox boxes and improvised arrows.
- Do not present unverified architecture claims as facts.
- Label uncertain relationships as questions so the maintainer can annotate them directly.

## Table playbook

Use this playbook when records share fields and the maintainer needs to compare evidence quickly.

- Start with a short summary of what the rows prove or require.
- Group columns by the decision they support, such as identity, evidence, status, and action.
- Keep raw details available while making the primary status visible without reading every cell.
- Use semantic table markup for tabular data.
- Use cards instead when records have different shapes or require long explanations.
- Put counts, risk levels, or status summaries above the table when they change how the rows should be read.
- Protect long paths, URLs, symbols, and prose from overflow on narrow screens.
- Make individual rows easy annotation targets.
- Do not paste a terminal table into HTML.
- Do not hide the important conclusion below a large undifferentiated grid.

## Comparison playbook

Use this playbook for options, tradeoffs, and current-versus-target behavior.

- Name the decision at the top.
- Use before-and-after panels when the same system changes over time.
- Use aligned option cards for mutually exclusive directions.
- Use a scorecard only when the criteria are explicit and genuinely comparable.
- Show concrete behavior or artifact shape for each side instead of abstract pros and cons.
- Keep corresponding details aligned so differences are visible without hunting.
- Make each option's cost as visible as its benefit.
- Separate primary tradeoffs from secondary notes.
- End with a recommendation only when the evidence supports one.
- Surface assumptions that would change the recommendation.
- Do not make every option appear equally recommended when one is clearly preferred.

## Plan playbook

Use this playbook for a product plan, technical design, implementation proposal, or PRD.

- Start with the goal, verified current state, and desired behavior.
- Describe the proposed approach through its high-level decisions before listing file changes.
- Make the plan self-contained enough for another developer to implement.
- Verify every codebase claim before presenting it as fact.
- Prefer a faithful interface mock when describing a frontend experience.
- Include failure modes, migration concerns, compatibility effects, and verification.
- List genuine remaining risks and open questions at the end.
- Present multiple open options through the comparison playbook.
- Remove a resolved open question by updating the plan's main content.
- Do not produce a page that contains only ambiguity and omits the actual proposal.

## Code playbook

Use this playbook whenever the artifact includes source, patches, diffs, or before-and-after code.

- Put the file path, language, and reason for inspection immediately before each code surface.
- Keep claims next to the lines, paths, or annotations that prove them.
- Group multi-file changes by user-facing area or task instead of repository order.
- Use an aligned split view when careful side-by-side comparison matters and width allows.
- Use a unified view when space is tight, changes are mostly additive, or mobile reading matters.
- Use the seed template's styled `pre` blocks and plain diff classes because vplan artifacts cannot depend on an external diff library.
- Wrap long lines unless horizontal alignment is essential to the review.
- Show only the relevant range when an unrelated full file would hide the point.
- Do not render code as a screenshot.
- Make files, hunks, and relevant lines distinct annotation targets.

## Input playbook

Use this playbook when the maintainer can select, tune, triage, or edit a structured choice faster in the artifact than in prose.

- Make the question, option meanings, and next action visible together.
- Use native radios, checkboxes, selects, text inputs, textareas, buttons, labels, and disclosure controls.
- Keep reversible selection state local until the maintainer explicitly queues the final answer.
- Show selected state separately from queued state.
- Queue one clear comment for the final answer instead of one comment per intermediate change.
- Use the vplan comment panel's queue and Confirm and Save action as the persistence boundary.
- Make the queued text specific enough for the broker to act without a follow-up question.
- Keep controls accessible and readable on narrow screens.
- Do not require interaction for information the maintainer only needs to read.

## Slides playbook

Use this playbook only when the maintainer explicitly asks for a deck, presentation, talk, or paced walkthrough.

- Use a scrolling page for detailed reference material or dense evidence.
- Plan the narrative before writing slide markup.
- Open with the point, build context, show evidence, and close with the decision or next action.
- Keep one idea per slide when the artifact has a narrative arc.
- Vary composition so consecutive slides do not look like repeated cards.
- Keep text sparse and let visuals carry the explanation.
- Use large type, strong alignment, and deliberate whitespace.
- Make navigation and screen-size assumptions explicit.
- Do not convert every explainer into slides by default.

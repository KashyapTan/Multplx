# Multplx
Multplx is a personal agent coordination system built around a broker, independent actors, persistent daemons, and maintainer-owned decisions.

The root folder, GitHub repository, and project name are all `Multplx`.
Never hardcode a local absolute checkout path in code or documentation.

## Vision
Port the read-only upstream reference under `firstmate/` into a standalone system tailored for the maintainer.
Preserve proven safety and lifecycle behavior while replacing rank-coded language and adding the features defined in `plans/index.html`.
The root implementation must stand on its own rather than depending on the reference checkout.
While the port is incomplete, `example_agents.md` is the non-auto-loaded broker-contract template.
Update `example_agents.md` whenever a plan changes broker behavior or documentation, and do not promote it to the auto-loaded `AGENTS.md` name until the port's definition of done is satisfied and the maintainer approves activation.

## Workflow

### Read More Than Less
Always read all relevant and connected files before writing new code. It is always better to over-read than to miss context.

### Freedom and Direction
You are extremely knowledgeable — don't be afraid to use that. If you have concerns, suggestions, or improvements, 
raise them. Discussion and clarification lead to the best possible outcome.

### Planning
Enter plan mode for non-trivial tasks. Get the correct info and details before executing. 
For trivial tasks this is unnecessary — don't over-engineer.

### Actors for Information Gathering
Route parallel read-only exploration to actors when it keeps the main reasoning context focused.
Keep work in the primary context when its reasoning is required for the final decision.

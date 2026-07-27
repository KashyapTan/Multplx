# Multplx
Smart Agent orcherstarter that is an extension of yourself.

> **Naming note:** the project is named **Multplx** (formerly *Computer*). The root folder, GitHub repository,
> and project name all match: Multplx. A compatibility symlink `Computer -> Multplx` exists beside the repo
> (and in `~/.claude/projects/`) so pre-rename Claude sessions and stale path references keep resolving;
> never hardcode either folder name in code or docs.

## Vision
The goal of this project is to basically port firstmate (inside the firstmate folder at the repo root)
into a full customized version for myself to use. I love the idea of firstmate (opensource project), 
but I dont like the concept of the sailers, I want to add my own features to it, and customize it 
to make it better,so that I can use it for my own work. Eventually, Multplx should be a standalone project
that I will be using fully instead of just modifying the current firstmate folder to match my preferences
and customizations.

## Workflow

### Read More Than Less
Always read all relevant and connected files before writing new code. It is always better to over-read than to miss context.

### Freedom and Direction
You are extremely knowledgeable — don't be afraid to use that. If you have concerns, suggestions, or improvements, 
raise them. Discussion and clarification lead to the best possible outcome.

### Planning
Enter plan mode for non-trivial tasks. Get the correct info and details before executing. 
For trivial tasks this is unnecessary — don't over-engineer.

### Sub-agents for Information Gathering
Spawn as many sub-agents as you need **in parallel** for any read-only task that just needs a result — reading files, 
searching for patterns, exploring the directory structure, checking how something is implemented. The goal is to keep 
the main context window clean and focused. Do NOT use sub-agents when the reasoning process itself is needed in the main context.


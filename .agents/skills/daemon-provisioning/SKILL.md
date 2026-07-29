---
name: daemon-provisioning
description: >-
  Agent-only reference for persistent daemon setup and retirement.
  Use when creating, seeding, validating, launching, recovering, handing backlog to, pushing inherited local material into, or retiring a daemon home, or when editing data/daemons.md.
  Covers home leases, transactional seeding, project clone restrictions, daemon harness pins, inherited local-material push, idle charter, handoff helper, and teardown safety.
user-invocable: false
metadata:
  internal: true
---

# daemon-provisioning

Use this reference before creating, seeding, validating, launching, handing backlog to, recovering, pushing inherited local material into, or retiring a persistent daemon, and before editing `data/daemons.md`.

Keep the always-inline routing rules in `AGENTS.md` authoritative: route by natural-language `scope:`, local-only projects stay with the main broker, and daemons are idle by default.

## Routing table

`data/daemons.md` has one parser-compatible line per persistent daemon:

```markdown
- <id> - <one-sentence charter summary> (home: <absolute-home-path>; scope: <natural-language responsibility>; projects: <project-a>, <project-b>; added <date>)
```

Each registry entry stays concise and single-line: the summary is one sentence naming the durable charter, `scope:` is the natural-language intake responsibility, `projects:` is the non-exclusive clone list, and any extra prose is limited to genuinely domain-specific hard rules that change routing or safety for that daemon.
The `home:` path points to the seeded home containing `data/charter.md`; no extra registry pointer field is needed.
The home-seeded `data/charter.md` is the sole owner of boilerplate idle-by-default behavior, the normal delegation lifecycle, and standard escalation contracts, so point to that charter rather than restating those contracts in the registry entry.
The `scope:` field is used during intake.
The `projects:` field is a non-exclusive clone list, not ownership.

## Charter and seed

Scaffold a daemon charter with:

```sh
bin/mx-brief.sh <id> --daemon {<project>...|--no-projects}
```

The scaffold writes a charter brief instead of a task brief.
Set `MX_DAEMON_CHARTER='<charter>'` to fill the charter text and `MX_DAEMON_SCOPE='<scope>'` when the routing scope differs.
If you scaffold without `MX_DAEMON_CHARTER`, replace the `{TASK}` placeholder before seeding.
Pass `--no-projects` instead of a project list to scaffold a project-less charter for a domain whose subject is the Multplx repo itself, whose home is a broker worktree and whose actors take pooled worktrees of the same repo.
`--no-projects` is mutually exclusive with a project list, and omitting both still fails loudly, so an accidental omission is never mistaken for a deliberate project-less seed.
Re-seeding a populated home as project-less is refused non-destructively when the home contains project clones or `data/projects.md` entries.
Retire or clean that home first, and re-scaffold a stale project-bearing charter with `--no-projects` before seeding.
Keep custom charter text focused on the persistent responsibility, available project clones, and genuinely domain-specific hard rules.
The scaffolded charter, later copied to `data/charter.md`, owns the standard lifecycle and escalation wording.
Preserve the generated charter sections unless the domain genuinely needs a hard rule.

Provision the persistent home and registry entry after the charter is filled:

```sh
bin/mx-home-seed.sh <id> <home|-> {<project>...|--no-projects}
```

Pass `--no-projects` in the project position to seed the project-less home described above; the same mutual-exclusion and fail-loud-on-omission rules apply.
It may only seed a home with no project clones or project-registry entries, and refuses conversion of populated homes without changing them.
`-` durably leases a fresh broker worktree via `treehouse get --lease` under the daemon id.
The lease survives with no live process and is never recycled by later `treehouse get` or `prune`.
The slot stays reserved across restarts until the lease is released.
Release happens only on explicit retirement or seed rollback, never on routine restart or recovery.

`bin/mx-home-seed.sh` copies the charter into the daemon home as `data/charter.md`.
It also writes the required `.mx-daemon-home` identity marker, which is gitignored and must remain in place for home validation.
`bin/mx-spawn.sh --daemon` launches it through the daemon harness path, resolving `config/daemon-harness` -> `config/actor-harness` -> the primary's own harness unless an explicit per-spawn harness override is passed.

`config/daemon-harness` may also pin a concrete model and effort for the daemon agent, in the SAME file rather than a new one: the format is a single whitespace-separated line `<harness> [<model>] [<effort>]`, with only the first non-empty, non-comment line parsed.
A bare `<harness>` (today's format, e.g. `claude`) behaves exactly as before - harness only, no model/effort flag - so this is fully backward-compatible.
`bin/mx-harness.sh daemon-model` and `bin/mx-harness.sh daemon-effort` print the optional 2nd/3rd tokens (empty when absent, or when the file is absent/`default`/harness-only); they read only `config/daemon-harness`, never `config/actor-harness`, which stays a bare adapter name.
For a `--daemon` spawn, `bin/mx-spawn.sh` populates `MODEL`/`EFFORT` from those tokens only when the harness itself came from the daemon config path for that spawn.
An explicit per-spawn `--harness` flag, positional harness arg, or raw launch command starts clean on model and effort too, unless the caller also passes explicit `--model` or `--effort`.
When the file's tokens do apply, an explicit per-spawn `--model` or `--effort` flag always wins over the file's token for that axis.
Because this resolves from the file on every spawn, the pin is durable across every respawn (recovery, `/updatemultplx`, restart) exactly like the harness axis itself - e.g. `config/daemon-harness` containing `claude opus` keeps a daemon pinned to Opus even if the primary's own default model later changes.
This is daemon-only: actor/scout model resolution is untouched by this file.

This section is the single owner of the daemon sync and inherited-local-material propagation contract; `AGENTS.md` sections 3 and 4 point here.
Before launch, `mx-spawn.sh --daemon` locally fast-forwards the home to the primary Multplx checkout's current default-branch commit when it is safe; dirty, diverged, or in-flight homes launch unchanged with a warning.
The locked session-start bootstrap sweep runs the same guarded fast-forward for every live daemon home, discovered from `state/<id>.meta` records with `kind=daemon` (`data/daemons.md` only backfills `home=` for older records).
That no-fetch path is a purely local fast-forward of tracked files, never an origin fetch, and it never touches the gitignored operational dirs, so a daemon's backlog, projects, and in-flight work are never disturbed; a linked worktree advances immediately, while a standalone clone that lacks the target receives broker updates through `/updatemultplx`'s origin refresh.
The same launch and the same locked bootstrap sweep also propagate the primary's declared inherited local material: `config/actor-dispatch.json`, `config/actor-harness`, `config/backlog-backend`, `config/herdr-presentation-spaces`, and the one shared maintainer-preference file `data/maintainer-shared.md`.
Because these paths are gitignored, that propagation is a separate, primary-authoritative copy independent of the tracked-files fast-forward: it re-converges every live home whether or not its tracked files advanced, and it touches only the declared items.
Propagation failures warn without blocking daemon launch or session-start continuation, and the destination keeps whatever safely validated state the helper left behind.
Inheritance copies the literal `config/actor-harness` file, so a daemon's own actors use the primary's actor harness only when it names a concrete adapter such as `codex`; an unset or `default` value has nothing concrete to inherit, and the daemon's own actors fall back to the daemon's own or detected harness instead.
`config/daemon-harness` is not inherited because it is only the primary's knob for launching daemon agents.
`data/maintainer-shared.md` is main-authoritative in the primary home and read-only in daemon homes.
Its primary file header must state that the file is main-authoritative, read-only in daemon homes, must not be edited there, and that new maintainer-preference discoveries are routed to the main broker through marked status or a document pointer.
Every propagation point converges the daemon copy to the primary bytes; when the primary file is absent, any existing daemon copy is quarantined and removed so absence converges too.
The helper rejects unsafe directories, symlinked or nonordinary source or destination artifacts, and hardlinked destination files.
Between propagation runs, the daemon copy is filesystem read-only; the helper may make its owned destination writable only around a guarded update and restores read-only mode on success, unchanged bytes, and recoverable failure paths.
Before replacing divergent daemon bytes, the helper hash-compares source and destination, quarantines the daemon-local version to a collision-safe private dated sibling file, and emits a `DAEMON_SYNC:` diagnostic naming the home and quarantine artifact.
Never copy any daemon `data/maintainer-shared.md` back into the primary.
Keep each home's `data/maintainer.md` domain-local.
After first propagation to an existing home, trim that home's local `data/maintainer.md` by hand to domain-specific content plus pointers to `data/maintainer-shared.md`; do not automate or silently delete private content.
Keep every `data/learnings.md` fully local by maintainer decision; route system-general machinery facts into tracked documentation through the normal Multplx repo path rather than inventing shared learnings propagation.
No AGENTS.md reread nudge is needed at spawn or respawn because the agent reads instructions fresh on launch; only the bootstrap sweep's running-home instruction-surface advance needs that AGENTS.md re-read.
Bootstrap reports successful AGENTS.md re-read sends as `BOOTSTRAP_INFO:` and only emits `NUDGE_DAEMONS:` when that send fails and needs retry.
A separate, literal-content config reread is required whenever inherited `config/*` material changes under an already-running daemon.
After each successful allowlisted config write, both the locked bootstrap convergence path and mid-session `bin/mx-config-push.sh` use the shared propagation report to build one per-home generation-specific private instruction file from the validated destination post-write bytes for only the allowlisted config items that actually changed for that home (`config/actor-dispatch.json`, `config/actor-harness`, `config/backlog-backend`, `config/herdr-presentation-spaces`), in deterministic allowlist order.
Each changed path is printed with clear begin/end delimiters and the destination file's full exact new bytes unparsed, or the explicit token `ABSENT` when propagation removed the destination copy.
The instruction uses only minimal framing that these are defaults/rules and do not remove judgment; it never includes SHA values, selected profiles, parsed summaries, or any other generated interpretation.
`data/maintainer-shared.md` is not a config file and is never inlined into this instruction file or message.
Homes whose allowlisted config files were all unchanged receive no config-reread message when no retry is pending.
Different homes may receive different changed-file sets based on their pre-push destination bytes.
Delivery uses the existing routed daemon path (`mx-send`) with only a single-line `CONFIG_REREAD: <absolute generation-specific instruction path>` pointer; a failed instruction publication retains the generated exact bytes in a bounded private retry queue when possible, legacy retry reports remain recoverable, a failed publication or retry-marker write retains the exact generation until it can be delivered, a failed send records a per-generation durable retry marker when possible, and all failures surface a concrete `CONFIG_REREAD:` diagnostic without claiming the live agent already re-read the values.
The propagation, generation publication, and pointer-delivery sequence holds one per-home inheritance lock, so concurrent mid-session pushes cannot deliver an older generation after a newer one.
A newly launched or relaunched daemon already reads its files at launch, so its pending config-reread generations are discarded or quarantined after cleanup failure and it needs no redundant live-agent config nudge unless propagation changes files after launch.
Quarantined pre-relaunch generations are retained in bounded private history, and cleanup skips creating an empty quarantine generation.
Successfully delivered generations are retained only within a bounded per-home state history, while pending generations remain until delivery succeeds or a launch supersedes them.
These config values remain defaults and rules only; they must not harden `mx-spawn` to reject a deliberate runtime choice that differs from the configured defaults.
For already-live daemons, use `bin/mx-config-push.sh` to push a mid-session inherited local-material change without running the tracked-file fast-forward.
It uses the same live-home discovery and propagation helper as bootstrap, reports each item as `pushed`, `unchanged`, `skipped`, or `error`, and follows the config-reread contract above for changed or pending generations.
`bin/mx-home-seed.sh` refuses to copy a missing or placeholder charter.

Direct seed without a preexisting brief requires `MX_DAEMON_CHARTER`.
Run `bin/mx-home-seed.sh validate` when checking registry integrity; it refuses duplicate ids, duplicate homes, and nested or overlapping homes.

Seeding is transactional.
If validation, cloning, no-mistakes initialization, or registry update fails, generated briefs, new homes, new project clones, and registry edits are rolled back.

Daemon project lists may include `no-mistakes` and `direct-PR` projects only.
`local-only` projects stay with the main broker.
For `no-mistakes` projects, seeding initializes only projects newly cloned into a daemon home and refuses to mutate a preexisting clone that is not already initialized.

## Backlog handoff

Apply `AGENTS.md` section 10's work-items-only backlog contract before creation or handoff.
When a daemon is created for a domain, existing main-backlog items that fall under its scope should become its work instead of staying stranded in the main backlog.
Scope-matching is broker's judgment against the daemon's natural-language scope, not a keyword rule.
Read `data/backlog.md`, pick queued items that fit the new scope, and move them with:

```sh
bin/mx-backlog-handoff.sh <daemon-id> <item-key>...
```

After seeding, run this handoff for the new daemon's in-scope queued items.
The helper resolves and validates the daemon home from `data/daemons.md`, then routes the item move through the owned backlog library, which moves each named item - and a whole connected set, blocker plus dependents, atomically - from the main `data/backlog.md` into the daemon home's `data/backlog.md`.
This routed path remains required when `config/backlog-backend=manual`, which controls only routine broker backlog edits.
It moves each queued item's whole block - the `- [ ] <id> ...` header plus every following two-or-more-space-indented body line and blank separator, up to the next item or column-0 section heading - byte-exact under the same section, treating an indented `## ...` line as body rather than a section boundary, so neither the header nor its body is duplicated or orphaned.
It refuses a selected item with a single-space or tab-indented continuation rather than risk leaving content orphaned in the main backlog.
It accepts in-scope `## Queued` entries only and refuses `## In flight` and historical `## Done` entries.
Done records stay with their home for pruning or archiving.
It is idempotent; an item already in the daemon backlog is skipped.
It refuses any destination that is not a genuine seeded Multplx home with safe operational directories and a matching `.mx-daemon-home` marker, so a move can never land in a project.
Do not hand off `local-only` items.

## Recovery

For `kind=daemon` meta with no window, treat the daemon as a dead persistent agent and respawn it with:

```sh
bin/mx-spawn.sh <id> --daemon
```

Use the recorded `home=` in meta.
If meta is missing but `data/daemons.md` still registers the daemon, respawn from the registry entry and its persistent on-disk home.
Respawn re-resolves the daemon harness from current config, uses the same guarded pre-launch sync, and re-propagates inherited local material, so recovered daemons converge inherited config items and shared maintainer preferences whenever their home validates; tracked-file sync remains guarded separately.
If the daemon is already running and only inherited local material changed, prefer `bin/mx-config-push.sh` over respawning.

Do not reconstruct a daemon's whole tree from the main home.
The main broker reconciles only direct reports.
Each daemon is a broker in its own home, so it runs recovery on startup and reconciles its own actors.
A daemon's recovery reconciles only work that is already its own and then idles.
It never initiates a survey or audit during recovery.

## Retirement and teardown

A daemon is persistent by default.
An empty queue is healthy and does not trigger teardown.
Run `bin/mx-teardown.sh <id>` for `kind=daemon` only when the maintainer or main broker explicitly decides to retire that persistent daemon.

The safety check is the daemon's own home.
Teardown refuses while its `state/*.meta` contains in-flight work.
When safe, teardown kills the direct tmux window, removes the `data/daemons.md` route, clears the main home metadata, and removes the retired daemon home.
Removing a leased home releases its durable treehouse lease via `treehouse return`, so the pool slot is freed for reuse rather than left leased forever.
A plain-clone home with no pool slot is simply removed.
If `treehouse return` fails for a leased home, teardown stops with state intact rather than raw-removing the directory and hiding a held lease.

With `--force`, teardown is the explicit discard path.
It kills child windows, discards child work and state inside the daemon home, removes the route, releases the lease, and removes the retired daemon home.
Never use `--force` unless the maintainer explicitly said to discard the work.

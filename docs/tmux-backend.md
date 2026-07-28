# tmux runtime backend

tmux is Multplx's verified reference runtime backend and the fully supported baseline for daemon homes.
[`configuration.md`](configuration.md#runtime-backend-configbackend--mx_backend) owns shared backend selection and metadata semantics.

## Setup

Install tmux with `brew install tmux` or your platform package manager.
The universal harness and toolchain requirements are in [`configuration.md`](configuration.md#toolchain).

tmux is the hard default when no explicit setting or runtime auto-detection selects another backend.
Select it explicitly with local `config/backend` containing `tmux`, with `MX_BACKEND=tmux` for one launch, or by asking Multplx to use tmux.
An explicit selection is also the opt-out from Herdr or cmux runtime auto-detection.

No provisioning is required before the first task.

## Watching the actors

For the best visible experience, launch the primary harness inside a tmux session:

```sh
tmux new -s broker
```

Actors tasks become windows in that session.
`tmux display-message -p '#S'` prints its name.
If the primary harness runs outside tmux, Multplx creates or reuses a detached session named `broker`:

```sh
tmux attach -t broker
```

Each task window is named `mx-<id>`.

```sh
tmux list-windows -t <session-name>
tmux select-window -t <session-name>:mx-<id>
```

Typing into an attached task window is authoritative direct intervention.
Routine supervision does not require attachment: `bin/mx-peek.sh <id>` captures a bounded tail and `MX_HOME=<home> bin/mx-send.sh <id> '<text>'` steers the recorded endpoint.

Verify setup by spawning a small task and confirming its `mx-<id>` window appears in the selected session.

## Current behavior and safety

A target-existence check proves only that the pane exists.
The deeper tmux agent-liveness probe first verifies exact window membership, then reads `#{pane_current_command}` to distinguish a running harness process from a bare idle shell.
It classifies recognized Claude and Codex process names as `alive`, common shells as `dead`, an authoritatively absent window as `missing`, unreadable state as `unreadable`, and every other process as `ambiguous`.
Only `dead` and `missing` authorize recovery because a false dead result could launch a duplicate agent.

Pi runs through a generic `node` process name and cannot be attributed confidently from the tmux foreground-process field.
An existing Pi pane is therefore reported as ambiguous rather than auto-healed, while an authoritatively missing Pi window can be relaunched safely.
This is the active tmux liveness limitation.

Agent liveness and composer safety are separate checks.
The shared classifier in `bin/mx-composer-lib.sh` accepts a shell glyph as an empty agent composer only inside a verified bordered composer.
A bare shell prompt is `unknown`, so away-mode escalation is never injected into a dead shell.

`bin/mx-tmux-lib.sh` owns exact type-and-submit mechanics.
It types a message once and retries Enter only until the composer clears.
A cleared composer is the positive delivery acknowledgement; text left in the composer remains `pending`, and `mx-send.sh` reports the failure instead of retyping.

There is one busy-queue exception.
Some harness TUIs accept Enter mid-turn as a queued message but leave its text visible until the turn completes.
After the normal retry budget, a provably busy pane is accepted as queued, while an idle pane remains `pending` as a genuine swallowed Enter.
`tests/mx-tmux-submit-busy.test.sh` covers busy and idle panes with both pending and cleared composers.

## Limits and regression entry points

- tmux is the reference path and supports daemon homes.
- Existing Pi agent-process liveness is inconclusive, while an authoritatively missing Pi window can trigger recovery.
- The busy-queue exception is tmux-specific; Herdr retains its separately documented gap.

```sh
tests/mx-backend-tmux-smoke.test.sh
tests/mx-tmux-submit-busy.test.sh
tests/mx-bootstrap.test.sh
```

[`verification/runtime-backends.md`](verification/runtime-backends.md#tmux) records the active foreground-process and submit evidence.

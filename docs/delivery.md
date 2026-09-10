# Least-privilege delivery

Multplx separates a completed local change from the credentialed act that sends it to GitHub.
Broker, actor, daemon, and validation-gate sessions do not hold remote-write credentials and never push, open a pull request, or merge one.
`bin/mx-deliver.sh` is the only remote-delivery entrypoint and directly enters the Rust review-delivery boundary.
Its remote-delivery operation runs from the maintainer's shell or a separately credentialed scheduler.
Local `prepare` is available to actors, while local `approve` belongs only to the accepted authority under [AGENTS.md](../AGENTS.md#validate).

[Back to the documentation index](README.md).

## Delivery handoff

The ordinary local validation path writes `state/<id>.ready-to-push` only after it has validated a clean local branch.
An exact `validation.waive-gate` grant may instead create a version-2 handoff that says `validation=waived`, binds the consumed request and exact SHA, and leaves the failed gate unchanged.
The typed exact line schema and inert private-file parsing are owned by `multplx-domain::review_delivery`, while `bin/mx-deliver-lib.sh` preserves the sourced-function gate and body-rendering ABI for its remaining callers.
The handoff pins the task, worktree, `mx/<id>` branch, base branch, validation provenance, approval state, PR title, and exact approved commit.
For `direct-PR`, the owned `prepare` operation writes a version-3 pending handoff with the supplied summary and an explicit full-gate-not-run label; it creates neither a gate run nor a waiver.

Delivery reparses the record without sourcing it and re-verifies all of the following before any network write:

- The handoff and task metadata are private regular files on the state device.
- The task metadata still names the same worktree.
- The worktree is clean, is on the recorded branch, and its current HEAD is the approved commit.
- Deep-review provenance requires the matching private passed gate, or a valid consumed exact-SHA waiver; direct-PR provenance requires task metadata still declaring `kind=delivery` and `mode=direct-PR`.
- Approval is exactly `approved`.

The push uses the approved object ID as the source of an explicit refspec.
This guarantees that a newer local commit cannot be pushed through a check-to-push race.
After the push, the service opens the pull request with a deterministic summary and truthful validation provenance, records the canonical URL through `mx-pr-check.sh`, and moves the handoff to `state/<id>.delivered`.
A stale worktree, branch, SHA, or gate binding moves the handoff to `state/<id>.ready-to-push.stale` and requires preparation or validation again under the recorded mode.
A pending or malformed record stays in place and causes a nonzero exit.

There is no runtime implementation selector.
The stable shell filename is an exec-only adapter and cannot redirect review, record, branch, poll, or remote operations to a retained shell body.

Teardown refuses while a ready handoff exists, including after a partial delivery made the commit reachable on the remote.
This keeps the source worktree available until PR creation and state recording finish.

## Credential scope

Agent launches remove ambient GitHub token variables, GitHub CLI configuration, SSH-agent access, interactive credential prompts, and default SSH identities.
They also overlay `origin` with a non-writable push URL for the lifetime of the agent process without changing the repository's stored remote.
The default actor posture has no GitHub token.
When authenticated private-repository reads are necessary, `MX_AGENT_GH_TOKEN` may carry a fine-grained token whose permissions are enforced remotely as read-only.
It must never have contents-write or pull-request-write permission.

Launch the primary broker from an equivalently uncredentialed environment.
Use a dedicated OS account or an isolated empty `GH_CONFIG_DIR`, leave GitHub token variables and `SSH_AUTH_SOCK` unset, and do not make a write-capable Git credential helper available to that process.
The delivery entrypoints refuse known agent-session markers as a backstop, but the operator-owned process boundary is what keeps the primary broker from possessing a credential in the first place.

The delivery context may use the maintainer's ordinary keychain-backed `gh` configuration.
A scheduler may instead pass exactly one of:

```sh
MX_DELIVERY_GH_TOKEN=... bin/mx-deliver.sh
MX_DELIVERY_GH_CONFIG_DIR=/absolute/private/gh-config bin/mx-deliver.sh
```

The service removes ambient `GH_TOKEN`, `GITHUB_TOKEN`, enterprise-token variants, agent read tokens, and agent-session markers from every git and GitHub subprocess.
It maps only the explicit delivery credential when one is configured.
No credential belongs in the repository, a task worktree, a state record, a generated brief, or a scheduler plist.

The service credential should be repository-scoped and grant only `contents:write` and `pull_requests:write` for the intended repositories.
Remote merge uses `bin/mx-pr-merge.sh` from the same non-agent context and remains subject to the configured merge authority.
An exact `delivery.merge-red` alternate binds the canonical PR URL, head SHA, and failed-check set before it invokes the credentialed `--admin` merge path, and records the outcome as maintainer-directed rather than green.
Local-only projects are unchanged and use `bin/mx-merge-local.sh` without a remote credential.

## Scheduler examples

A scheduler is optional.
Explicit maintainer invocation remains the default.
Use the operating system credential store or an isolated `gh` config directory rather than putting a token in the job definition.

For cron, a keychain-unlocking wrapper outside every repository can run:

```cron
*/5 * * * * /absolute/private/bin/run-mx-delivery
```

That private wrapper should obtain the credential from the platform credential store and then execute the absolute Multplx `bin/mx-deliver.sh` path.

For launchd, keep the plist free of tokens and set `ProgramArguments` to the same private wrapper.
Set `WorkingDirectory` to the Multplx home only for predictable logs; delivery records already carry and verify their exact worktree paths.
Capture stdout and stderr so a refused record or credential failure is visible.

## Choose and complete a delivery mode

Ask the broker, for example, “Use deep-review for demo and fix the login bug,” “Use direct-PR for demo, with yolo off,” or “Keep demo local-only and enable +yolo for routine decisions.”
For one task, say “Use direct-PR for this Multplx change only, with yolo off.”
The broker records standing choices in private `data/projects.md`; it passes explicit task overrides to both brief and launch commands when the request is task-specific.
For registered clones under `projects/demo`, valid registry rows are:

```text
- demo [deep-review] - Application
- demo [direct-PR +yolo] - Application
- demo [local-only] - Application
```

Use only one row per project, choosing one mode; append `+yolo` independently to any mode when authorized.
Omitted mode defaults to deep-review and omitted `+yolo` means off.
Yolo does not skip validation, supply agent credentials, or relax destructive and security-sensitive boundaries.
Self-repo tasks default to deep-review/off without a registry entry; explicit `--mode` and `--yolo on|off` apply per task.
Pass identical overrides to `bin/mx-brief.sh task demo` and `bin/mx-spawn.sh task projects/demo`; consult their help/header for flags.
Scouts retain the selected authority for later promotion but produce knowledge only; daemons record daemon/off and resolve their own child tasks separately.
Existing task metadata is not migrated when registry preferences change, and conflicting relaunch overrides are refused.

Verify the project default with `bin/mx-project-mode.sh demo`, then check the spawned task's `mode=` and `yolo=` fields in `state/task.meta` or `bin/mx-system-snapshot.sh --json`.
The recorded task values govern its lifecycle.

| Mode | Worker completion | Approved landing |
| --- | --- | --- |
| deep-review | Commit clean `mx/task`, then run `bin/mx-deep-review.sh task --intent-file /absolute/brief.md` until passed | Approve the handoff and run credentialed delivery below; merge the PR only under the configured authority |
| direct-PR | Commit clean `mx/task`, then run `bin/mx-deliver.sh prepare task --sha FULL_SHA --summary 'Describe the change and actual verification'` | Approve the handoff and run credentialed delivery below; the PR explicitly says the full gate did not run |
| local-only | Commit clean `mx/task`, ensure it fast-forwards the default branch, and report ready | After configured merge approval, broker runs `bin/mx-merge-local.sh task`; no remote or handoff is required |

For either remote mode, inspect the pending handoff and exact commit before approving from a non-agent maintainer shell with the correct `MX_HOME`:

```sh
bin/mx-deliver.sh approve task --sha FULL_SHA
bin/mx-deliver.sh task
```

An authorized broker may also run local `approve` under the existing explicit or standing authority; implementation workers and gate sessions cannot approve their own handoff.
Approval rechecks the exact clean worktree and validation provenance and does not change project mode or yolo.
The separate service rechecks them before pushing; later HEAD, branch, or worktree changes invalidate delivery.
After PR review and explicit merge authorization, the non-agent shell runs `bin/mx-pr-merge.sh task https://github.com/OWNER/REPO/pull/NUMBER`.
Local landing also refuses a dirty primary checkout, a wrong default branch, a dirty actor worktree, or a branch that cannot fast-forward.

## Review execution bounds

`bin/mx-deep-review.sh --help` owns the supported round, attempt, and wall-clock overrides.
Each configured command defaults to 300 seconds and each headless invocation to 1800 seconds; positive `MX_DEEP_REVIEW_COMMAND_TIMEOUT_SECONDS` and `MX_DEEP_REVIEW_AGENT_TIMEOUT_SECONDS` override them independently.
Round and structured-output attempt limits remain separate count bounds, not a global task deadline.
A timeout kills the invocation's process group, preserves failure diagnostics, and never produces a successful handoff or automatic waiver.
The process-group boundary covers ordinary descendants; subprocesses that deliberately detach into another session are outside this cleanup guarantee.
The default configuration keeps broad regression in CI and uses focused local verification.

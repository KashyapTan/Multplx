# Least-privilege delivery

Multplx separates a completed local change from the credentialed act that sends it to GitHub.
Broker, actor, daemon, and validation-gate sessions do not hold remote-write credentials and never push, open a pull request, or merge one.
`bin/mx-deliver.sh` is the only remote-delivery entrypoint and selects the Rust review-delivery boundary by default.
It runs from the maintainer's shell or a separately credentialed scheduler, never from an agent session.

[Back to the documentation index](README.md).

## Delivery handoff

The ordinary local validation path writes `state/<id>.ready-to-push` only after it has validated a clean local branch.
An exact `validation.waive-gate` grant may instead create a version-2 handoff that says `validation=waived`, binds the consumed request and exact SHA, and leaves the failed gate unchanged.
The typed exact line schema and inert private-file parsing are owned by `multplx-domain::review_delivery`, while `bin/mx-deliver-lib.sh` preserves the source-compatible gate and body-rendering ABI during the rollback window.
The handoff pins the task, worktree, `mx/<id>` branch, base branch, gate run, approval state, PR title, and exact approved commit.

Delivery reparses the record without sourcing it and re-verifies all of the following before any network write:

- The handoff and task metadata are private regular files on the state device.
- The task metadata still names the same worktree.
- The worktree is clean, is on the recorded branch, and its current HEAD is the approved commit.
- The gate run is private and records the same approved commit plus its summary and risk assessment, or the handoff carries a valid consumed exact-SHA validation waiver.
- Approval is exactly `approved`.

The push uses the approved object ID as the source of an explicit refspec.
This guarantees that a newer local commit cannot be pushed through a check-to-push race.
After the push, the service opens the pull request with a deterministic summary and risk section, records the canonical URL through `mx-pr-check.sh`, and moves the handoff to `state/<id>.delivered`.
A stale worktree, branch, SHA, or gate binding moves the handoff to `state/<id>.ready-to-push.stale` and requires validation again.
A pending or malformed record stays in place and causes a nonzero exit.

The implementation selector is `MX_REVIEW_DELIVERY_IMPLEMENTATION`.
It defaults to `rust`, and `legacy` is accepted only before an invocation begins any review, record, branch, poll, or remote operation.

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

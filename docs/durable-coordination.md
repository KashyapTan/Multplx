# Durable coordination

Multplx uses one filesystem-backed coordination path for watcher events and terminal-client requests.
The owning orchestrator remains the only process allowed to claim or acknowledge work in its home.
There is no separate peer messaging service, and a recorded home does not imply that Multplx can attach to the owner's native conversation.

## Wake inbox

Watcher events first enter `state/.wake-queue` and move into stable `state/wake-inbox/wake-*.json` receipts when the owner claims them.
Each receipt preserves the original five-field wake row, a stable event ID, the exact claiming process lifetime, its disposition history, and acknowledgement time.
Heartbeat noise may coalesce before inbox delivery; signal, stale, and check transitions keep separate event IDs even when their keys match.

Use:

```text
mx wake claim [--limit N]
mx wake list [--unfinished]
mx wake disposition EVENT handled --detail TEXT
mx wake disposition EVENT superseded --detail TEXT
mx wake disposition EVENT waiting --detail TEXT --condition TEXT (--trigger ID | --recheck-at EPOCH)
mx wake disposition EVENT follow-up --detail TEXT --follow-up OPERATION_ID
mx wake ack EVENT
mx wake resume EVENT --trigger ID
mx wake resume EVENT --trigger recheck
mx wake recover
mx wake pending
```

`claim` returns JSON lines and establishes one handler.
The compatibility command `bin/mx-wake-drain.sh` prints the original rows, but that display is only a claim.
Record a durable disposition before `ack`.
`follow-up` accepts only a validated receipt in the owning state: a recoverable transition, routed request, shared message, or spawn-action receipt whose recorded home matches the owner.
Missing receipts, receipts from another home, and copied spawn actions with a foreign owner state are rejected before acknowledgement.
An item acknowledged as `waiting` remains in the unfinished count; resume it with the named trigger, or let the next owner claim resume a due periodic recheck, which preserves the earlier waiting disposition in history.
A replacement owner may recover a claim only after the former exact process lifetime is gone.
Read-only and foreign sessions can list or count retained work but cannot claim, recover, disposition, resume, or acknowledge it.

## Terminal-client requests

`mx request connection` reports the canonical home and state path, the verified current owner when present, and the detected provider.
`endpoint: null` and `attachment: unavailable` mean that no validated native conversation endpoint is recorded.
Durable submission is still available in that state.

Submit one independently retryable item with:

```text
mx request submit \
  --batch BATCH_ID \
  --request REQUEST_ID \
  --task TASK_ID \
  --client CLIENT_ID \
  --project PROJECT_ID \
  --checkout CHECKOUT_ID \
  --start STARTING_REVISION \
  --brief BRIEF_REVISION \
  --scope TEXT \
  [--depends TASK_ID]... \
  [--artifact PATH] \
  [--parent-task TASK_ID --parent-home ABSOLUTE_PATH] \
  [--attempt ATTEMPT_ID --generation N]
```

Project, checkout, and starting revision must exactly match an existing registered checkout.
Parent task/home and attempt/generation are paired fields.
Unknown, repeated singleton, or dangling options fail before acceptance.
The receipt stores a shared `MessageEnvelope` with the same request, route, attempt, brief, correlation, summary, and artifact identity used by task reports and messaging.

Request state advances through separate facts:

1. `submit` writes matching outbox and authoritative inbox receipts: accepted and delivered.
2. Wake publication links the request to a stable wake event; startup or the next claim reconciles a crash between acceptance and publication.
3. `mx request acknowledge REQUEST_ID` records that the verified current owner accepted responsibility.
4. `mx request response REQUEST_ID --id RESPONSE_ID --summary TEXT [--artifact PATH]` records a response.
5. `mx request complete REQUEST_ID --id COMPLETION_ID --summary TEXT [--artifact PATH]` records completion evidence after a response.

Every retry must repeat the same immutable project, route, and result payload bindings for its stable ID.
The recipient process lifetime may change through the explicit adoption path after restart.
Owner adoption requires the prior exact lifetime to be fenced and the replacement to own the home session; prior owners remain in receipt history.

## Partial multi-project retry

Give each item its own request ID even when several items share a batch:

```text
mx request submit --batch rollout-7 --request rollout-7-api --task api \
  --client terminal-2 --project api-project --checkout api-checkout \
  --start 31f0... --brief 4 --scope "Update the API"

mx request submit --batch rollout-7 --request rollout-7-web --task web \
  --client terminal-2 --project web-project --checkout web-checkout \
  --start 8ac1... --brief 2 --scope "Update the web client" \
  --depends api

mx request submit --batch rollout-7 --request rollout-7-docs --task docs \
  --client terminal-2 --project docs-project --checkout docs-checkout \
  --start f90d... --brief 1 --scope "Publish migration guidance"
```

If the second item is interrupted after acceptance, repeat only `rollout-7-web` with the exact same arguments.
The existing receipt is returned and its missing wake notification is reconciled without accepting a duplicate task.
The API and docs items retain their own project contexts and progress independently.

When a claimed request is ready for launch, pass its stable request ID to `mx spawn ... --request-id REQUEST_ID`.
Capacity admission begins at dispatch time; accepting or acknowledging an inbox item does not reserve a slot.

## Task-scoped messages

`mx-send.sh` preserves literal, unmarked human transport behavior.
When a canonical task sends with `MX_TASK_ID`, or the text is marked operational input, a canonical task selector uses the same shared `MessageEnvelope` contract.
Multplx validates the destination task/home, parent, current attempt, accepted brief, sender, recipient, and correlation before transport, writes `state/message-outbox/MESSAGE_ID.json`, and advances it from pending to delivered only after the backend confirms submission.
The matching pending-reply receipt retains the same stable correlation and route context.
Repeating a marked delivery with its existing correlation reuses that identity; conflicting route or message content fails.

These receipts prove local acceptance and backend delivery state.
They do not grant merge authority, prove that the recipient acted, or invent an endpoint for a native provider that records none.
Use the response and completion facts from the authoritative task report path for those later states.

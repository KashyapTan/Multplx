---
name: updatemultplx
description: Perform a requested safe fast-forward update and refresh instructions in registered homes.
user-invocable: true
metadata:
  internal: true
---

# Update Multplx

Use `bin/mx-update.sh --help` and run the updater only when an update is requested.
It updates the runtime and registered homes through guarded fast-forwards; dirty, diverged, offline and wrong-branch targets are retained and reported.
Do not force, stash, reset or update user project checkouts as part of this operation.
Follow its `reread-broker` and `nudge-daemons` results for changed instruction surfaces and registered live endpoints.
Send refresh pointers through `bin/mx-send.sh` with the correct active home; transport delivery is not proof that instructions were read.
[Configuration](../../../docs/configuration.md) owns home settings and [persistent operations](../persistent-subagents/SKILL.md) describes inherited material.
During this redesign, do not activate partial instructions in real homes; [porting.md](../../../porting.md#implementation-sequence) owns release sequencing.

---
upstream_repo: @UPSTREAM_REPO@
fork_point: @FORK_POINT@
last_reviewed: @LAST_REVIEWED@
status: active
retired_reason:
---

# Fixture upstream record

<!-- mx-upstream-map:start -->
| Upstream path glob | Class | Multplx counterpart or reason |
| --- | --- | --- |
| `bin/fm-watch.sh` | relevant | Fixture retained watcher. |
| `tests/fm-watch.test.sh` | relevant | Fixture retained watcher test. |
| `bin/fm-x-*.sh` | deleted | Fixture removed relay. |
| `bin/fm-pr-glab.sh` | deleted | Fixture removed provider-only path. |
| `docs/release-notes.md` | irrelevant | Fixture release-note noise. |
<!-- mx-upstream-map:end -->

## Completed review log

<!-- mx-upstream-log:start -->
_No completed upstream review has been recorded._
<!-- mx-upstream-log:end -->

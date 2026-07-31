# Upstream review input

- Upstream repository: @UPSTREAM_URL@
- Diff range: `@BASE_SHA@..@HEAD_SHA@`
- Upstream HEAD: `@HEAD_SHA@`
- Commits: 5
- Relevant commits: 1
- Flagged commits: 1
- Mechanically skipped commits: 3
- Paths needing mapping: 1

## Relevant changes

### `@RELEVANT_SHA_SHORT@` fix watch race

- Paths: bin/fm-watch.sh (relevant)

#### Change metadata and diff

```diff
commit @RELEVANT_SHA@
Author:     Upstream Fixture <upstream@example.invalid>
AuthorDate: Wed Jul 1 00:00:01 2026 +0000
Commit:     Upstream Fixture <upstream@example.invalid>
CommitDate: Wed Jul 1 00:00:01 2026 +0000

    fix watch race
---
 bin/fm-watch.sh | 1 +
 1 file changed, 1 insertion(+)

diff --git a/bin/fm-watch.sh b/bin/fm-watch.sh
new file mode 100644
index 0000000..ec5627f
--- /dev/null
+++ b/bin/fm-watch.sh
@@ -0,0 +1 @@
+watch v1
```

## Flagged changes

### `@FLAGGED_SHA_SHORT@` add unmapped area

- Paths: docs/new-area.md (flag)

#### Change metadata and diff

```diff
commit @FLAGGED_SHA@
Author:     Upstream Fixture <upstream@example.invalid>
AuthorDate: Wed Jul 1 00:00:05 2026 +0000
Commit:     Upstream Fixture <upstream@example.invalid>
CommitDate: Wed Jul 1 00:00:05 2026 +0000

    add unmapped area
---
 docs/new-area.md | 1 +
 1 file changed, 1 insertion(+)

diff --git a/docs/new-area.md b/docs/new-area.md
new file mode 100644
index 0000000..80a617a
--- /dev/null
+++ b/docs/new-area.md
@@ -0,0 +1 @@
+unmapped
```

## Paths needing mapping

- `docs/new-area.md`

## Mechanically skipped

- `@DELETED_SHA_SHORT@` change removed relay - bin/fm-x-poll.sh (deleted)
- `@IRRELEVANT_SHA_SHORT@` update release notes - docs/release-notes.md (irrelevant)
- `@GLAB_SHA_SHORT@` fix removed provider - bin/fm-pr-glab.sh (deleted)

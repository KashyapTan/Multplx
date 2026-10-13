# Repository workspace grouping follow-up

This extends the [original transcript issue ledger in PR 64](https://github.com/KashyapTan/Multplx/blob/6acac1b/TRANSCRIPT_ISSUES_2026-10-12.md), with [PR 65](https://github.com/KashyapTan/Multplx/pull/65) retaining its separate dashboard scope.
Neither earlier PR is changed by this follow-up.

| Issue | Confirmed behavior | Resolution |
| --- | --- | --- |
| HERDR-01 | Related standing workers and their nested children appear in separate Herdr workspaces because the adapter derives placement from each private home; the optional presentation flag can fragment tasks further. | Fresh Git launches group separate tabs by canonical repository identity and validated root operational context, preserving private ownership, routes and lifetimes; unrelated repositories remain separate. |

The actual configured home was inspected only for presence of `config/herdr-presentation-spaces`; it was absent, confirming home-based placement as the immediate cause.
No private operational state, live workspace or existing endpoint was migrated.
The explicit `home` layout remains available for legacy presentation behavior, with the default requiring no configuration changes.

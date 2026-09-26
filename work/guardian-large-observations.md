# Complete Guardian observations

Classification: core compatibility for the existing optional Guardian review UI.
Native review policy and execution remain owned by Codex.

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Upstream commit `9c4879f3a5bfdc7fb5401b7d3ebcdc9a77c27aa2`, which is an
ancestor of that cutoff, preserves complete reviewed actions and uses native
request budgets instead of truncating action arguments.

Decodex previously rejected Guardian observations above 256 KiB in both its
adapter and database. The decoder returned no review, so a valid large native
action could disappear before persistence. Generic large approval request
storage did not cover this separate observation path.

The adapter and database now use the existing 8 MiB native message bound from
`decodex_core::MAX_NATIVE_MESSAGE_BYTES`. Complete event JSON, including action
suffixes, stays attached to the original thread, turn, review and process owner.
No schema migration or execution-policy change is required.

The public review list retains its existing limits: at most eight rows and
128 KiB per page. Details above the 96 KiB display limit remain withheld with an
explanation, and the UI cannot approve a review whose complete details are not
shown. Complete large-action browsing in the desktop is not provided by this
change. Retention does not itself submit approval or wake work.

Validation uses a 300 KB Unicode command with an exact required suffix. The
adapter regression failed at decode before the change. The updated tests cover
complete decode and supported action conversion, database reopen, the native
notification-to-coordinator path, unchanged no-wake/no-auto-approval behavior,
bounded withheld display details and rejection above the native message limit.
These fixtures do not establish live-provider Guardian execution or signed
large-review desktop acceptance.

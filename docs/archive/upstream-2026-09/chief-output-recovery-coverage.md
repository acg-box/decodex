> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore Chief output recovery assertions

Restore two sets of assertions from the preserved `chief/tests.rs` snapshot.
The existing native plan test covers provider history, but does not replace the
local output store's finality and kind checks. The database output test alone
does not prove that the coordinator routes native revert events to that owner.

- Restore `live_plan_finality_and_kind_survive_restart`: reject a foreign turn,
  bound a long Unicode draft, replace it with the completed plan, reopen the
  database, ignore a late draft and reject a conflicting output kind. Keep the
  final plan text, kind and non-truncated state.
- Restore the removed output assertions in
  `native_revert_retires_exact_thread_requests_without_replies_or_replay`:
  preserve output for a foreign process generation and foreign thread, then
  clear it for the exact reverted thread. Reopen the database and confirm it is
  still absent. Retain the original pending-request retirement and no-reply
  assertions.

These are coverage repairs for core output correctness. No production code or
database schema changes. The complete shared `chief/tests.rs` review remains
open until its other differences are mapped. Signed desktop acceptance remains
separate. Automations remain paused.

## Validation

The Chief test group passes 159 tests with no failures and 15 existing opt-in
tests ignored in 77.76 seconds. Both restored cases pass. The log is
`/tmp/decodex-chief-output-coverage.log`.
Strict stable runtime Clippy passes all features and targets in 12.98 seconds;
the log is `/tmp/decodex-chief-output-coverage-clippy.log`.
The preserved source hash matches the audit register. Formatting and diff checks
pass. These results prove restored local coverage, not native desktop acceptance.

> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore observed native checklists

The installed native process emitted two `turn/plan/updated` notifications, but
Decodex saved no checklist record. The restored native fixture failed with zero
records where one latest record was required. Restore the complete observation,
storage, public history and desktop path from the preserved baseline.

This is the native `update_plan` checklist, not a proposed plan from Plan mode.
At fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`app-server-protocol/src/protocol/v2/turn.rs` defines its thread, turn, explanation
and step states. `app-server/src/bespoke_event_handling.rs` explicitly distinguishes
it from Plan-mode updates. `core/src/tools/handlers/plan.rs` rejects `update_plan`
in Plan mode. Those source contracts were inspected.

## Storage and display

Use the existing SQLite inbox observation owner. Append a bounded, resolved,
non-waking record for a changed checklist under the exact running thread and
turn. Keep repeated identical updates idempotent, preserve A/B/A changes and show
only the latest observation per thread and turn. Retain earlier stored records;
the history query and desktop cached-page projection hide superseded entries.
After the existing update limit, show an explicit unavailable-later-states notice.

Restore literal escaping, credential filtering, bounded step previews and the
recorded-observation label. The desktop shows the same latest checklist in saved
and native conversation views. It does not infer task completion from checklist
states. Missed disconnected updates remain unavailable: native history does not
replay these notifications on the qualified executable.

The same inherited activity owner again retains a delayed MCP completion only
when an exact saved terminal-turn receipt establishes its historical association.
Wrong thread, unknown turn and mismatched payload identities cannot attach it to
the current dispatch. Saving the receipt does not wake work or change its turn.

No schema migration, backfill, second database owner or real-user database write
is required. Database schema 48 and protocol 2.93 remain unchanged.

## Provenance and reconciliation

The unmodified checklist renderer and native fixture originated in baseline
`2ffa385c3b49efe6a4109de0fd7353fb64abd2c5`; their files were unchanged in the preserved
working tree, so the 357-file change snapshot did not contain copies. Recover them
from Git, not from a new design. Extend the native fixture only with SQLite reopen
and public-history projection checks. Its opt-in native setup remains isolated.

Restore `database/src/chief/tests/activity.rs` byte-for-byte. Reconcile the complete
`database/src/chief_output.rs` diff: restore checklist and delayed-MCP behavior;
retain current output-change notifications, prompt/model pending guards and the
PR1511 rule that a late completion cannot erase fallback output before complete
native history is available. Keep all current hidden-journal exclusions in the
shared transcript query. Shared runtime and desktop files stay open for their
other inherited differences. Close only these two database file rows.

## Evidence and limits

Three database activity tests, three runtime projection tests and the rendered
saved/native-view checklist test pass. The installed native fixture fails before
the fix and passes afterward, including store reopen, public history, no wake,
no notification replay and exactly three synthetic inference requests across the
fixture lifecycle. It uses temporary homes and a loopback provider, without real
credentials or account/configuration changes.

Qualified binary: `codex-cli 0.158.0-alpha.2`, SHA-256
`c3e30211bd454da70ceb4d9cbc2e05fe6466812ab05c311c3bbff6addeb14202`, from the
ChatGPT application bundle. Strict database/runtime/GPUI lint passes with all
features and targets. The full database suite passes 162 unit tests and 7 restart
integration tests. The full desktop suite passes 519 tests with 5 opt-in tests
ignored. Signed desktop visual acceptance remains separate.

Recorded checklist presentation is optional for the user's removal review.
If removed, remove its projector, receipt writer and display together while
preserving native Plan-mode history and other activity receipts. Native checklist
execution remains upstream-owned. Automations remain paused.

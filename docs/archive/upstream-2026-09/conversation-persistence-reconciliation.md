> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Conversation persistence reconciliation

## Scope and authority

The database uses versioned migrations under database/migrations. The ordered
MIGRATIONS registry and recorded hashes in database/src/migrations.rs own
reconstruction and upgrade. SqliteStore is the application write owner.
This audit changes tests and review records only. No SQL history, schema version,
runtime store or existing user database changes.

Compare the complete inherited conversation_restart.rs test file and
0036_native_reasoning_effort.sql snapshot. The snapshot starts at
2ffa385c3b49efe6a4109de0fd7353fb64abd2c5. Close only these two inherited rows.
The larger conversation store, runtime, protocol and desktop reviews remain open.

## Migration mapping

| Inherited requirement | Current registered owner |
| --- | --- |
| Preserve request identity, original input, directory, model and service tier | Migration 36, nullable_conversation_effort, copies these fields when it rebuilds the request table. Its upgrade fixture compares all original columns and checks foreign keys. |
| Accept native effort names instead of a fixed local enumeration | Migration 36 accepts nonempty values up to 128 bytes and allows NULL for native inheritance. Store validation also rejects control characters. |
| Preserve source account, source revision and review-required state | Migration 46, initial_model_source, adds the source fields with paired identity, positive revision and boolean review constraints. Its upgrade fixture preserves old nullable requests and repeated migration state. |
| Keep application writes valid | create_conversation and review_initial_model_settings both use validate_initial_execution. Empty, oversized and control-bearing values are rejected before persistence. |

The old snapshot migration is not registered under its original filename.
Do not insert or replay it over the current version 36. Its non-null/default-high
policy is superseded by explicit native inheritance. Its SQL control-character
checks are not present in the current nullable table; the application write owner
enforces that rule. This is a documented validation-owner difference, not a claim
of identical SQL constraints or raw-SQL write behavior.

## Complete restart-test mapping

The original long conversation restart scenario and its fixtures remain intact
apart from Option-wrapped explicit effort values. It retains same-thread
continuation, exact history, process/source identity and no duplicate dispatch.
The source-routing and routing-successor tests moved within the file and retain
their assertions.

Current additions verify nullable effort, exact creation-receipt readback and
changed source-bound creation replays. The native-settings projection no longer
contains the old duplicated original-directory field; its test reads the original
request from the request owner and compares the complete request after restart.

Restore the missing advertised_reasoning_efforts_survive_create_review_and_reopen
case. It retains the original four native effort strings, creation and explicit
review, cold readback and invalid-control rejection. Only the current Option
types are adapted. Keep the newer nullable-effort case.

Run the revision-fenced review scenario for both inherited settings and explicit
low effort plus Flex. Both retain the race with one winner, cold idempotent replay,
unchanged input/directory, source identity, rejection after routing, and no turn
admission or process start from confirmation alone.

## Validation and limits

All eight conversation_restart integration tests pass. The two migration
fixtures pass from registered schema versions 35 and 45 to current version 48.
Strict stable Rust Clippy passes for every database feature and target.

Logs:
- /tmp/decodex-conversation-persistence-audit.log
- /tmp/decodex-effort-migration-audit.log
- /tmp/decodex-source-migration-audit.log
- /tmp/decodex-conversation-persistence-clippy.log

These are disposable SQLite fixtures through the current migration and store
owners. They do not qualify every historical schema, production-sized migration
locks, a manually modified database or signed desktop behavior.

Saved request integrity, source checks and no duplicate dispatch are core
correctness. The ordinary conversation workbench and optional model controls
remain separate product choices. This batch restores evidence for current
behavior; it does not add an execution engine or enable automations.

## Complete conversation store reconciliation

The complete inherited `database/src/conversations.rs` diff was reviewed. Restore
its read transaction around the initial conversation selection and all subsequent
projection queries. Without that transaction, a second SQLite connection can
commit between queries and produce an old revision with a new title.

A deterministic test uses SQLite's existing profile callback only in the test
build. After the production metadata SELECT, a second connection commits a title
and revision update. Before restoration, the returned projection mixes revision 1
with the new title. After restoration, both list and exact-ID reads return the
original snapshot; a subsequent read returns the committed new title and revision.
The probe is unregistered before its context is dropped. No dependency, production
hook, lock abstraction or timing-dependent retry loop is added.

Also restore the original warning-history regression unchanged. It records the
same status item twice, reopens the store and proves one warning remains while
the user's turn stays active at its original revision.

The other whole-file differences retain their current owners:

- Initial and stored request effort is nullable. Explicit effort still rejects
  empty, oversized and control-character strings; model validation stays bounded.
  Account-source revision validation remains, with generalized invalid-input text.
- The creation-receipt reader checks the exact command and conversation without
  reserving or replaying a creation. It proves local creation, not native execution.
- Initial model-source fields and replay checks remain in the creation transaction.
  The routing-successor copy now reads nullable effort; original input is retained.
- The positive non-submission regression adds both acknowledged/unacknowledged
  session states and rejects foreign thread evidence. The original rollback,
  durable reopen and idempotency assertions remain. A refusal does not invent a
  native turn or change unrelated session evidence.
- Module order and SQL formatting changes do not introduce another persistence
  owner. Other inherited source remains unchanged.

This closes the conversation-store file row. It does not close the complete
migration registry, runtime conversation owner or ordinary desktop acceptance.
All tests use disposable databases. The signed desktop must be rebuilt before
accepting this production restoration.

Validation: the deterministic mixed-snapshot regression fails before restoration
in `/tmp/decodex-conversation-snapshot-before.log`. Final source passes 13
conversation unit tests, eight restart integration tests and strict database Clippy
for all features and targets. Logs: `/tmp/decodex-conversation-snapshot-after.log`,
`/tmp/decodex-conversation-snapshot-restart.log` and
`/tmp/decodex-conversation-snapshot-clippy.log`. The read method and warning test
match the preserved source bytes; its snapshot hash is verified.

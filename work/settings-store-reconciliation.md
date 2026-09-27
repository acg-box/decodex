# Permission and plugin store reconciliation

Compare the complete preserved and current diffs for:

- `database/src/chief_permissions.rs`
- `database/src/chief_plugins.rs`
- `crates/decodex-runtime/src/chief_plugins.rs`

The inherited behavior has a current owner. No source, schema, protocol or product
policy change is needed for this reconciliation. Close only these three rows.

## Permission reservations

Keep exact work, thread, generation and observation identity. The current store
also rejects overlapping prompt and model edits. The old inline model-recovery
query moved to `chief_models::pending`, which checks `chief_model_legacy::pending`
before the current journal. A queued or unknown old result remains unresolved;
only the existing rejection, target observation or reconciliation settles it.
The guard was not removed.

Current native settings support named permission profiles while a task runs.
Retain that qualified behavior instead of restoring the older builtin-only
restriction. The existing test still rejects dispatch-before-acknowledgment,
preserves the active turn, retains unknown state across reopen and prevents replay.
The added A/B/A observation test preserves revisions without filling transcript
pages. See [native qualification](native-permission-controller-qualification.md).

## Plugin reservations and service

Keep the current idle/running reservation, mutual exclusion with permission,
model and prompt edits, and the shared legacy-model guard. The extracted ID
validator retains all previous request limits and also rejects malformed observed
IDs before they can settle a receipt. Sorting still compares the complete target
set. Existing tests preserve restart, publication-before-acknowledgment, immutable
outcomes, competing reservations and rejection of stale reviews.

The service persists current configured facts before review, including explicit
invalidation when facts are unavailable. Its publication identity includes the
settings and history revisions. The extracted catalog reader still verifies the
exact thread, absolute cwd and bounded inventory. Read and write guards reject
source changes; one reservation precedes one native write. Known pre-send queue
and size failures are rejected. An uncertain response remains unknown and cannot
be retried automatically. Other plugin exclusions remain intact.

At fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`, the
`thread_settings_update` test distinguishes replacement, omission, null and empty
plugin lists without inference. That source was read, not executed in this batch.
The installed-native controller evidence is documented in
[native plugin qualification](native-plugin-controller-qualification.md).

## Evidence and limits

All three source files are byte-identical to the checklist validation revision
`791b732218fe283e07bd5190d544bc20e9ae3a9e`. Its complete database run passed 162
unit tests and seven restart tests. This includes all eight permission/plugin
store tests and the legacy-model pending/reopen cases. Log:
`/tmp/decodex-checklist-all-database.log`.

The fresh focused service run passed all three permission, plugin and model
ownership scenarios, including queued, rejected and unknown outcomes, exact
source checks, restart and no replay. Log:
`/tmp/decodex-settings-store-service.log`. No new test duplicates
the existing ownership fixtures. This record does not close the broader legacy
model-history display, automatic fallback policy, shared connector filtering or
signed desktop acceptance. The selectors remain optional for the user's removal
review; retaining them requires their ownership and no-replay rules. Automations
remain paused.

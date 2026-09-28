> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile native bridge and model-setting test owners

Read both complete preserved diffs: `account_launch/chief_process.rs` and
`account_launch/chief_process_model_settings_tests.rs`. Verify their snapshot
hashes. Fixed upstream cutoff:
`595cc91e8cbb1c2ca822d0311dcf12709410c582`.

## Bridge scope

The bridge retains process I/O, pending response identity, frame handling and
private callback consumption. The shared native test module has crate visibility
so other tests can use the same isolated process fixture.

Configuration writes still require one recognized App link, App exposure, voice
or Hook edit. Thread settings still require a recognized model, recovery,
permission or plugin selection. Turn edits retain their separate live scope.
Restore missing test cases for plugin changes mixed with model/config/policy
fields, plugin requests sent as shared config writes, permission changes mixed
with sandbox/config, and versioned App leaf edits. Keep malformed/escaped key
paths, missing versions, relative file targets, wrong merge strategy, invalid
values, extra edits and explicit removal covered. Existing Hook and live-turn
scope tests remain.

The inherited bridge also restricted the exact parameter shape of three read
methods. The current bridge admits `permissionProfile/list`, `config/read` and
`configRequirements/read` by method. This is an explicit difference, not retained
raw-frame equivalence. The permission adapter validates absolute bounded cwd,
fixed page size and bounded unique cursors; App/voice configuration readers
construct scoped requests, and model-default inspection sends an empty
requirements request. Native protocol validation remains authoritative. Do not
restore duplicate bridge read-parameter validation or claim that arbitrary raw
read frames receive the old local rejection.

Additional admitted methods serve current owners: Hook inspection, App metadata,
MCP resources, explicit App UI tool calls, prompt revert and task unload. The App
UI adapter validates the target and arguments, binds a live history guard and
requires a confirmed durable operation in its caller. Method admission alone is
not user consent. Goal inspection remains admitted; goal set/clear remain denied.
The allow/deny test and separate native goal fixtures replace the old redundant
goal call inside the usage/resources fixture.

## Model test mapping

The two model read tests move to `chief_model_settings_tests.rs`; their complete
function bodies match the originals. They retain exact source-change, null,
missing metadata and foreign-thread assertions. Live model publication retains
capability, disabled/unsupported choice, changed source, durable unknown outcome
and no-retry checks. Current live-choice inspection adds source-bound options.

The removed task-model writer is replaced by `chief_models`. Its service tests
cover queued without observation, queued with current observation, native
rejection, lost transport and uncertain live errors. They retain receipts after
database reopen, reject repeat reviews, reject changed source identities and
reject an A-B-A settings revision. Exact request equality excludes service-tier
or global configuration writes. Manual account identity and revision are saved.
Legacy uncertain receipts still block a new mutation and remain readable.

The installed-native task model test covers the explicit-effort path in ordinary
and Plan mode. The current turn keeps its model, a later turn uses the selected
model after restart, another task retains its defaults, the service tier remains
unchanged and config bytes remain identical. It distinguishes queued response
from observed target settings and checks the persisted process identity.

## Evidence and limits

On this source, six model-setting tests and two model-service tests pass with no
skips. The installed-native task-model test passes with no skips against Codex
`0.158.0-alpha.2.1` in 1.25 seconds. Logs:
`/tmp/decodex-model-settings-owner.log`, `/tmp/decodex-model-service-owner.log`,
and `/tmp/decodex-native-task-model-owner.log`. The native binary SHA-256 is
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.

Twenty bridge tests and the separate plugin-scope test pass with no skips. Logs:
`/tmp/decodex-bridge-owner-tests.log` and `/tmp/decodex-bridge-plugin-owner.log`.
Only test assertions change; no production behavior changes in this batch.
Strict stable runtime Clippy passes all features and targets in 12.21 seconds;
the log is `/tmp/decodex-bridge-owner-clippy.log`. Snapshot hash, formatting and
diff checks pass.

Close these two source rows after the complete mappings. This does not close
separate Flex or child MCP qualification limits, prove all optional consumers
useful for Decodex, or establish signed desktop acceptance. Native compatibility
and no-replay behavior are core; optional settings and integration controls remain
subject to the user's removal review. Automations remain paused.

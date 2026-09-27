# Preserve model request history

Restore historical model receipts through the current model owner. The panel now
distinguishes a manual selection from automatic fallback and retains the original
request response after a later native observation or process reconciliation.
A historical target is separate from the task's current configured model.

## Evidence and ownership

`database/src/chief_model_history.rs` is a read-only part of `chief_models`.
It reads the latest attempt by event ID across `model_selection` and preserved
`model_recovery` records. The read requires the exact current task, native thread
and process owner. Original payloads, event IDs and response records remain
unchanged. No schema migration or second mutation owner is added.

The projection retains the requested model and effort, manual/automatic origin,
original response, matching native observation and post-restart reconciliation.
It maps legacy `uncertain` to the current `unknown` vocabulary. An observed target
does not replace an unknown RPC result. Reconciliation does not assert that an old
request succeeded. Full historical attempt details remain in the original saved
payloads; the current service exposes the fields required by the model panel.

The runtime binds receipt identity and content into the model review token.
Current source checks, pending-edit checks and mutation eligibility stay in the
existing owner. Reads do not send settings, replay input or make an old operation
retryable. The UI keeps current settings separate from the last request and
shows the restart qualification when reconciliation exists.

Local protocol 2.94 adds the receipt fields. The effective `last_outcome` and
pending state remain separate from the original response, so current clients do
not have to infer edit eligibility from historical metadata. Exact version
negotiation requires a matching service and desktop application.

## Inherited mapping

| Inherited file | Current owner and retained behavior |
| --- | --- |
| `database/src/chief_model_recovery_status.rs` | `database/src/chief_model_history.rs`: exact owner-bound historical read, original response, target observation and reconciliation. Saved attempt details and evidence references remain immutable. |
| `crates/decodex-protocol/src/chief_task_model_selection.rs` | `crates/decodex-protocol/src/chief_models.rs`: explicit selection with current source review and complete historical receipt metadata. Current flat action fields retain model/effort-only scope and distinguish preserved effort from explicit native `none`. |

The old writable recovery APIs are not restored. Their remaining whole-file
reconciliation stays open, as do shared runtime and desktop module rows.

## Classification and acceptance

Validation covers 15 database model regressions, two service fixtures and six
rendered/socket model-panel tests. The service fixture reads a historical target
that differs from current native settings and sends no native mutation. The socket
fixture loses the command reply, reads the retained receipt and sends exactly one
command. A rendered automatic-reconciliation case keeps old delivery unconfirmed.
All 164 protocol tests and the exact protocol/bundle-identity check pass. Update
the stale 2.91/2.92/2.93 test expectations to the exact 2.94 contract; preserve the
publication-instance and credential-negative assertions.
Strict Clippy passes for database, protocol, runtime and GPUI with all features
and targets. A final service regression also proves that a changed historical
receipt invalidates an earlier review without sending a native write. Logs are
`/tmp/decodex-model-history-database.log`,
`/tmp/decodex-model-history-service-history-change.log`,
`/tmp/decodex-model-history-gpui-qualified.log`,
`/tmp/decodex-model-history-protocol-final.log` and
`/tmp/decodex-model-history-service-final-clippy.log`.

Accurate saved-history interpretation is core correctness while model controls
or recovery records exist. The explicit model control and automatic fallback
policy remain optional product behaviors. Removing one does not authorize
deleting stored receipts or weakening pending-operation guards.

Signed desktop visual acceptance remains part of the final integration scope.
Rendered fixture tests are not installed-application acceptance. This batch does
not change an installed application, live account, native binary or automation.

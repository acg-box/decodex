# Preserve unresolved legacy model requests

The current model owner recognized `model_selection` events but ignored preserved
`model_recovery` events. A database reopened with an unresolved old request could
therefore admit new work. The regression test reproduces that failure before the
fix and passes after it.

## One current owner

Keep the current model journal as the mutation owner. Its pending check now also
reads the old event vocabulary. Model, permission, plugin and dispatch guards
share that decision. The model service displays a preserved request as reserved,
queued or unknown. It does not send that request again or create a replacement
reservation. Permission and plugin controls use the same pending check.

Read old reservations and their result, observation and reconciliation records
without changing their IDs, keys or payload bytes. No schema migration, data
backfill or second database entrypoint is required. The existing versioned SQLite
owner remains authoritative. The compatibility reader accepts the pre-upgrade
manual and automatic request shapes, including an omitted manual-review field.
It writes only new evidence when the current observation owner can settle an old
request.

## Confirmation and recovery

The current owner must supply complete, transport-current model facts. Historical
facts cannot settle a request. The original process can record target observation
only with the original current account revision and matching model and effort.
Automatic requests also require the requested tier. Manual model/effort edits
preserve whatever known tier native Codex reports. Missing tier data stays unknown.
Rejected requests cannot acquire a later success observation.

A replacement process cannot settle an old request while the old process death is
unknown. After confirmed death, a complete observation from the admitted new owner
records reconciliation. That releases the pending guard without claiming that the
old write succeeded. Stale owners, incomplete facts and unavailable accounts do
not authorize reconciliation. No retry is sent.

## Verification boundary

Validation passes: all 160 database unit tests and seven restart tests; the three
current model, permission and plugin service tests; the new legacy service case;
and strict database/runtime Clippy with all features and targets. After sharing
the pending-state projection helper, both current and legacy model service cases
and strict lint pass again.

The focused regression covers reopen and dispatch. Owned-process fixtures cover
manual and automatic requests with absent, queued and uncertain responses; native
publication versus history; tier preservation; changed account revision; rejection;
foreign sources; confirmed process death; replacement ownership; unchanged old
payload bytes; one evidence record; and reopen after reconciliation.

The service fixture reads an old uncertain request through the real model service,
shows it as pending, rejects another model change, disables permission changes,
and records zero native writes. Tests use disposable stores and retained local
wire fixtures. They do not read or migrate the user's live database.

This fixes compatibility for unresolved old records. It does not restore the old
automatic fallback producer or qualify every historical recovery display. Those
whole-file dispositions remain open. Signed desktop acceptance remains open.
Automations remain paused.

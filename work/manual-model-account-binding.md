# Bind manual model confirmation to the reviewed account

The inherited model owner stored the account and revision for both automatic
fallback and manual selection. The current shared owner retained those fields
only for automatic fallback. Runtime source checks protected the immediate send,
but the durable manual receipt could accept a later publication after the account
revision changed.

Record an optional `manual_source` with new manual attempts in the existing model
journal. The runtime supplies the account and revision it reviewed. The reservation
transaction checks that account against the retained process binding, current
revision and enabled account state. A later publication in the original process
must pass the same check before it can confirm the target.

For a new source-bound manual attempt, a replacement process can reconcile the
unknown request after confirmed process death. Even matching current settings do
not confirm delivery of the old request. Automatic fallback keeps its existing
account/banner context and the same reconciliation rule.

The optional field is omitted from old payloads. Existing records, stable request
keys and original responses stay unchanged. Old unbound manual records retain
their established interpretation; do not backfill account evidence that was not
recorded. Automatic and manual source contexts cannot both own one attempt.
No table, migration, wire-protocol field or second mutation owner is added.

## Evidence

With account identity recorded but not yet enforced, the regression changes the
account revision after reservation, then publishes the requested target. It
incorrectly returns `target_observed` instead of retaining `queued`. Preserve the
failure in `/tmp/decodex-manual-model-source-before.log`.

The corrected database tests verify stale account/revision rejection, matching
source confirmation, old payload compatibility and manual/automatic reconciliation
without an invented success. Service fixtures check that the real manual producer
saves its reviewed account context. Only disposable SQLite fixtures are changed.

Final validation passes 18 database model tests and two service fixtures. The
broader runtime model run passes 23 tests, with 16 opt-in tests ignored. Strict
database/runtime Clippy passes with all features and targets. Logs:
`/tmp/decodex-manual-model-source-database-final.log`,
`/tmp/decodex-manual-model-source-service-final.log`,
`/tmp/decodex-manual-model-source-runtime.log` and
`/tmp/decodex-manual-model-source-clippy-final.log`.

This restores core source correctness for the optional model controls. See the
[complete writer and test mapping](model-owner-reconciliation.md). Shared settings
observers and signed desktop acceptance remain open. No installed application,
live account or maintenance automation is changed.

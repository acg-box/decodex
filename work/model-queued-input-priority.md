# Keep queued execution choices ahead of model edits

The inherited `database/src/chief_model_recovery.rs` reservation checked queued
explicit execution choices for both automatic and manual model changes. The
current owner retained that check only inside automatic recovery eligibility.
As a result, a manual model/effort edit could reserve while an undelivered user
input already specified its own model or reasoning effort.

Move the existing query into the shared reservation transaction. Both manual
selection and automatic fallback now respect that queued choice. Keep the exact
existing predicate: an unresolved, undelivered user message with a nonempty
execution object. Empty execution settings do not block an edit. Do not consume,
rewrite, dispatch or drop the queued input.

The regression fails before the fix: an idle manual edit incorrectly returns a
reservation for a queued model choice. After the fix, it passes six cases across
idle/running tasks and model/effort/empty execution settings. All 16 database
model regressions and strict database Clippy pass. Logs:
`/tmp/decodex-model-queued-input-before.log`,
`/tmp/decodex-model-queued-input-after.log` and
`/tmp/decodex-model-queued-input-clippy.log`.
All 23 runtime model regressions also pass; 16 opt-in tests are ignored and are
not counted as passes. Log: `/tmp/decodex-model-queued-input-runtime.log`.

This is core ordering correctness for retained model controls. It adds no new
policy, journal, schema, native request or UI control. The full legacy owner and
test-file audit remains open; this fix does not close an inherited-file row.

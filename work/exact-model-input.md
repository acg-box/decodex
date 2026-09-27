# Exact model input recovery

Classification: optional model control restored from the inherited snapshot.
The existing model menu had catalog buttons but no field to apply an exact model
ID to an existing task. Restore that field and the original explicit apply
handler. Keep it in the model menu instead of the retired advanced-defaults form.
The field remains available when catalog discovery is unavailable.

Editing text alone does not create a next-message override. Apply trims the input,
uses the existing bounded `ConversationModel` validator and calls the same
`select_composer_option` path as catalog selection. Invalid input preserves the
previous choice. The existing task owner, draft persistence, effort reconciliation
and revision-bound acceptance rules remain unchanged. Apply does not start a turn.
A model ID is not a claim of provider availability or account access.

The fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582`
defines `TurnStartParams.model` as an optional string in
`codex-rs/app-server-protocol/src/protocol/v2/turn.rs`. Its `turn_start.rs`
fixtures supply explicit model strings. Those upstream tests were read, not run.
The experimental schema generated from installed Codex 0.158.0-alpha.2.1 also
accepts a string or null. Decodex retains its existing model-label validation;
this recovery does not expand accepted characters or change native dispatch.

## Complete execution-intent file mapping

Verify the original snapshot SHA-256 and compare the complete file:

- Revision tracking, saved choices, per-owner updates, action capture and
  revision-bound acceptance retain their original behavior.
- Restore `apply_exact_model_button` with its original validation and action.
- Keep current creation-intent and inherited-effort handling. These additions
  prevent native creation defaults from replacing an explicit user choice.
- Keep both inherited regression cases: an effort-only change does not apply
  display defaults, and a late acceptance cannot clear a newer choice. The
  provider-defined effort case now uses the public composer selection method
  before the same wire round trip. Steering retains the next-message choice.

The complete execution-intent file is reconciled. The larger composer, controls,
surface and creation-input reviews remain separate. In particular, this recovery
covers existing-task next-message input; new-task freeform input still needs its
own disposition. Do not use one button test to close the entire desktop scope.

## Qualification boundary

A rendered GPUI regression first fails because the apply button is absent. It
then clicks the restored control without a model catalog, verifies the trimmed
model on the owned next-message action, excludes another task and confirms no
submission. A second click with blank input preserves the earlier choice.
All 14 composer tests, both execution-intent regressions and strict all-feature,
all-target GPUI Clippy pass. Signed desktop interaction remains unqualified. The previous signed package
predates this UI change and is not acceptance evidence for the restored field.

Removing this optional field and button does not require removing the native
catalog, task-scoped execution overrides or saved-state correctness. Keep the
shared draft and no-replay owners for other consumers. Automations remain paused.

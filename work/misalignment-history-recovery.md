# Reconcile old provider precautions from native history

After another client had continued a paused native thread, Decodex could retain
its old precaution and refuse ordinary input. Restore the inherited read-only
reconciliation: require the exact old turn and a later turn in complete native
history, all required items, an unchanged question revision and a live thread
lifecycle. Clear only the same saved precaution on an idle, unchanged binding.
Recheck source guards within the database transaction. Never restore retired
input, start a turn or interpret history as a new continuation request.

## Voice retirement and storage

A precaution that stopped voice must survive later history and restart until an
explicit continuation is acknowledged. Record this cause before requesting the
native stop. A storage failure must not prevent the stop request or retain local
microphone authority. Keep the session if saving fails. Preserve the newer
transcript-tail persistence behavior while restoring the durable cause.

Migration 48 adds the inherited `retired_voice` flag. Existing records have an
unknown cause and are conservatively marked as retired. New records derive the
flag from the existing open-call owner. Later updates on the same thread retain
it. This is a forward migration under the existing versioned SQLite owner;
previous migration files and receipts are unchanged. Tests apply it only to
private temporary databases. No installed user database was migrated here.

## Evidence and remaining scope

The initial runtime regression failed: complete later history left the old
precaution present. Coverage includes current-only history, a later turn,
voice retirement, a missing original turn and a missing required item. It checks
read-only requests and no replay of retired input. Storage coverage checks
changed identities, invalidated source guards, rollback, explicit continuation
and reopen. Voice cases include saved-cause failure and lost native stop replies.
The upgrade fixture preserves old evidence, rejects invalid flags, checks all
unrelated schema and re-runs migration and verification.

The complete storage file and migration match the preserved snapshot; the
migration is registered as 48 rather than overwriting historical version 34.
Shared runtime files still have other unresolved inherited differences. In
particular, the live-review identity used for explicit continuation requires
separate reconciliation. This batch does not close R08, R10 or final native and
signed desktop acceptance.

## Validation results

- The restored regression failed before the production fix.
- All 194 selected Chief runtime tests pass; seven existing external tests remain
  ignored. This includes voice storage failure, lost-stop and transcript-tail
  recovery as well as all five new history scenarios.
- The full database run passed 148 of 150 tests. The remaining two expected
  the changed table to retain its old schema. After updating only that table's
  exception, both focused tests pass. The dedicated migration test verifies
  its exact retained data, flag constraints and all unrelated schema.
- Strict database and runtime lint passes with all features and targets.

The fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582` publishes
misalignment findings on terminal error notifications in
`codex-rs/app-server/src/bespoke_event_handling.rs`. Native history remains the
execution record. This change restores Decodex's local projection recovery;
it does not create a replacement execution or approval policy.

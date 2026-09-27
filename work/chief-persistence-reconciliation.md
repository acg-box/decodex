# Reconcile shared Chief persistence

The complete inherited diffs of `database/src/chief.rs` and
`database/src/chief_process.rs` were reviewed. This batch changes no persistence
source, migration, schema, account or runtime policy.

## Events, dispatch and input

Both work-event and transcript queries now exclude internal settings, model,
permission, plugin, hook, App, prompt-edit and voice-handoff journal records from
conversation pages. Filtering occurs before the page limit. The SQL does not
delete these records; exact event and canonical journal readers retain them.
The existing activity-completion, latest-plan and unresolved-steer rules remain.
This changes the visible projection, not original evidence storage.

Dispatch admission adds pending model selection and prompt-edit checks to the
existing idle/thread/permission/plugin checks. New user messages, question answers
and steering inputs cannot enter while an edit is unresolved. Canonical prompt
input references must match work, thread, edit receipt and digest, and the receipt
must be in `draft_restored`. Legacy plain text retains its existing route.

The added live-question test distinguishes first live arrival from replay and
retains that provenance after restart. Changed recovered question content loses
its original live-arrival flag. Skip and explicit resolution remain separate
from observation. The added request-payload and dispatch-refusal test modules
register current tests without removing the original modules.

## Process admission and retained observations

Changing an account now checks the entire owned family for an unreleased prompt
edit as well as non-idle dispatch. The existing process-death and affinity checks
remain. An unresolved edit cannot move to a different account; recovery uses the
same account and positive old-process death before a new owner can reconcile it.

The old model-recovery test module maps to current `model_fallback`, `model_legacy`
and `models` suites under the canonical model owner. See
[model ownership](model-owner-reconciliation.md) and
[model history](model-selection-history.md). Existing permission, plugin,
auth-recovery, Guardian, native-turn, warning and usage modules stay registered.
App, hook, prompt-edit and App UI tests are additive.

The added Guardian fixture retains a complete large Unicode action after reopen,
without an approval state, and rejects content beyond the native frame bound.
The voice fixture now records explicit transcript completion and checks bounded
call history, partial captions, unknown legacy completeness, exact work/thread
scope and reopen. These are persisted observations, not native execution claims.

All other inherited source in these two files remains unchanged. The current files
are also byte-identical to the versions covered by the recent 174-test database
run recorded in [database root reconciliation](database-root-reconciliation.md).
That broader result does not replace the focused evidence below.

## Verification boundary

Fresh focused fixtures use disposable databases. They cover canonical input and
prompt-edit exclusion/recovery, large Guardian content, live question provenance,
voice persistence and legacy journal pagination. No real account, notification,
voice session or approval is exercised. Close only these two inherited file rows.
Conversation/migration review, signed desktop acceptance and native lifecycle
qualification retain separate entries. Automations stay paused.

Validation passes 12 tests: seven prompt-edit cases, one large Guardian case,
one live-question provenance case, two voice-related cases and one legacy model
journal pagination case. Logs: `/tmp/decodex-chief-persistence-prompt.log`,
`/tmp/decodex-chief-persistence-guardian.log`,
`/tmp/decodex-chief-persistence-provenance.log`,
`/tmp/decodex-chief-persistence-voice.log` and
`/tmp/decodex-chief-persistence-history.log`. Both original snapshot hashes match.

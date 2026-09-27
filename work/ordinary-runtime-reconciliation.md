# Reconcile ordinary runtime and positive non-submission

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete inherited/current conversation.rs diff and both runtime and
database conversation/non_submission.rs diffs. This batch changes no code.

## Shared ordinary runtime

| Complete difference | Current owner and disposition |
| --- | --- |
| Configured settings modules | Add execution_overrides and model_settings; model_catalog becomes private to its parent. Existing runtime methods retain the application boundary. |
| Nullable effort | Keep native inheritance distinct from a literal effort spelling in runtime settings and local sessions. |
| Initial source and creation identity | Share creation_identity between creation and exact receipt reads. Source-less requests retain the original fingerprint; current source-bound requests include a domain marker and positive account revision. |
| Explicit later-turn choices | Carry optional per-field overrides through initial/fallback starts, resumed sessions and turn dispatch. Omitted intent preserves legacy explicit behavior; explicit false means inherit. |
| Resume | Delegate to the same inherit_native_settings owner without restoring stale creation model, tier, instructions or directory. |
| User turn requests | Retain exact client message identity, apply the shared overrides and mark the explicit user trigger. |
| Provider attempt ID | Extract the existing derived UUID expression into ordinary_provider_attempt_id so outcome readback uses the same identity. |
| Native refusal failure | If positive non-submission cannot be finalized, retain the ambiguous session and attempt the existing unknown-state transition. Never infer retry authority. |
| Retained Chief process directory | Retain its actual process cwd and return it only for the matching live generation. Canonical relative media uses that owner, not a different child-thread cwd. |
| Process initialization | Chief admission uses initialize_chief_turns; ordinary admission retains initialize_ordinary_turns. Keep each existing request-capability contract. |
| Active model read | WorkerCommand::ModelSettings performs an exact ordinary read, forwards every interleaved event and delivers terminal completion once. Idle queries restore their owned state even when a client disconnects. |
| Error and outcome types | HistoryChanged and refusal matches reorder without losing cases. Add the current closed auth-source/credential-conflict errors to the existing account failure mapping. |
| Tests | Add creation receipt identity coverage and the retained process cwd fixture field. No inherited function or test is removed by these changes. |

The source-bound fingerprint differs from the preserved unmerged prototype, which
appended only account/revision. This is explicit, not byte-equivalent. Main before
source-review restoration commit 9001d0457 had no initial_model_source field on
CreateConversation and passed None to storage. The current source-bound creation
and readback use one shared fingerprint; source-less identity remains unchanged.
No claim is made that a receipt produced by the old unmerged prototype can be
read under the newer source-bound format. No fallback replay is introduced.

## Positive non-submission chain

The runtime helper retains the exact provider-attempt/request/key/thread evidence
and positive receipt source. Its current additions reload the durable session
revisions after the evidence transaction, require the same session/thread, and
publish HistoryChanged before manual recovery. The admitted process stays owned
until explicit recovery. Evidence or readback failure remains uncertain.

The database helper now requires PositiveNonSubmissionReceipt as well as
NotSubmitted. It requires an active matching session and input, no streaming
output and no competing potentially submitted attempt. A zero-row update is an
error, so the evidence transaction rolls back instead of silently succeeding.
The exact status history item and conversation revision remain in that transaction.
Import cleanup and SQL formatting are the other differences. See
[the feature qualification](ordinary-native-non-submission.md).

## Evidence and limits

Eighteen conversation runtime tests pass, including source-bound settings,
interleaved terminal events, all override combinations, bounded closing retry,
creation fingerprint and exact process retirement. The separate source-bound
creation receipt test passes. The database non-submission test passes for both
acknowledgement states, wrong-thread rejection, atomic rollback and reopen.

All three original hashes match. No production or test source changes; no new
Clippy run is needed for this documentation mapping. Close these three complete
file rows only. This does not qualify every historical prototype receipt, native
provider version, ordinary desktop recovery surface or final signed lifecycle.
Those boundaries remain in the acceptance register. Automations stay paused.

> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore recovery evidence and reconcile the Chief coordinator

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete 863-line inherited/current chief.rs diff. Two removed recovery
paths require restoration; other changes map to the current owners below.

## Reproduced recovery losses

The native resume API can omit turns when excludeTurns is true. The fixed
upstream thread processor and thread-resume test explicitly preserve an empty
turn list in this mode. The local coordinator fixture previously returned full
history anyway, masking a lost usage-replay baseline. Make the fixture honor the
request. The existing usage recovery regression then fails because recovered
context is absent.

Restore the inherited asynchronous expect_usage_replay helper. If a matching
resume response contains no turns, read the latest turn identity through the
existing bounded history adapter. Discard the result if the history revision
changes. Use the returned identity for usage-baseline validation and await the
same helper in deferred resume recovery. Do not resend input; reuse the existing history adapter and its bounds.

The terminal history path had also lost observe_terminal_item. A completed plan
readback did not replace its cached partial text, so stale partial output survived
database reopen. Extend the existing missing/complete/empty readback regression
with a complete plan and answer. Before repair it finds one stale partial entry
instead of zero. Restore the helper, including final text observation, steer
receipts, asynchronous questions and subagent activity. Adapt final text replacement
to require a nonempty item ID and nonempty text. Old id-less result records remain
valid terminal evidence, and an empty readback cannot erase a retained partial.
The first restoration exposed those existing compatibility cases; keep their
assertions and fix the helper rather than weakening the tests.

## Complete coordinator mapping

| Difference | Retained owner and disposition |
| --- | --- |
| Configuration | Nullable manager and worker effort distinguish native inheritance from an explicit value. Keep the worker's Medium creation default and validate extensible effort strings. Creation defaults do not overwrite existing native tasks. |
| Native initialization | for_chief advertises the form extension served by the retained request consumer. Ordinary readers retain the narrower initialization. Existing realtime startup uses the native feature configuration. |
| Error outcomes | InputNotSent carries the durable refusal reason instead of separate drain/provider error variants. Local pre-write refusal and exact native refusal classification remain distinct from ambiguous effects. The host maps proven refusal to retained-draft rejection. |
| Terminal and usage recovery | Restore the two inherited helpers described above. Keep exact thread/turn matching, native completion evidence, uncertain usage after disconnection and capacity eligibility checks. |
| Task settings | native_settings owns exact resume parameters and current permission/plugin/model persistence. The old model-settings module is replaced by the shared owners. Permission inspection refreshes current configured facts before it exposes a review, rather than depending on a completion-only cached idle observation. |
| Request responses | Keep live pending-request identity, exact work/thread and native connection checks. Installation uses its exact native request guard; other responses use the original method and parameters. Transport consumes the guard once before writing. Native resolution removes the pending mapping; acknowledgement follows the response. |
| Dispatch identity | Canonical prompt input and asynchronous answers retain their original task thread instead of allowing a tool-upgrade fork. Stored canonical input must match its thread, content hash and edit receipt. |
| Dispatch ordering | Combine question/history and settings guards. Validate the complete turn request before external injection. Track whether injection was attempted; only a proven pre-effect refusal releases ordinary claims. An uncertain injection or native result remains unknown. |
| Trigger and execution | Use native user/retry/automation/goal trigger values. Application continuations remain tool output. Explicit model, effort and tier overrides use the shared helper; omitted fields inherit. |
| Steering | Proven stale history, queue refusal and oversized requests release the corresponding unsent claim. Acknowledged or ambiguous native effects retain their separate receipt handling. |
| Notifications | Ignore textless asynchronous-question items as live text. Settings observations, reasoning, questions and pending file evidence retain their dedicated owners. File evidence handles completion/unload/revert within its connection-aware observer and releases committed evidence. |
| Native turns and unload | observe_native_turn moves to native_turns with its dispatch-paused and exact ownership checks. observe_unloaded_thread owns loaded state and pending recovery invalidation. |
| Scheduled follow-up | Recover deferred resumes and questions before dispatching due evidence. Fresh input retains priority over capacity continuation; a changed selection supersedes its old retry. This does not enable the paused upstream maintainer. |
| Presentation and exports | Main replaces the root title. Prompt-edit review and voice handoff expose their existing shared owners. Field order changes do not create a new runtime authority. |

The optional prompt editor and settings selectors remain separate product
decisions. Their removal must preserve existing receipt, history, native ownership
and no-replay contracts. The coordinator remains the existing service owner.

## Validation boundary

The two before-fix failures are local native-protocol fixtures. The final Chief
suite passes 158 tests with 15 existing opt-in cases ignored. Strict stable runtime
Clippy passes for all features and targets. The initial direct restoration failed
three sparse-history compatibility tests; the final run retains and passes all
three after the helper adaptation. Original snapshot hashes and git diff --check
pass. Closing this coordinator
file does not close the full inherited tests.rs comparison, shared host/application
review, installed-native qualifications or final signed desktop acceptance.
Automations remain paused.

> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore plugin wait time and reconcile the shared protocol client

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete inherited/current client.rs diff and the complete
chief_execution.rs and client_request.rs helper diffs.

## Reproduced wait-time regression

SetTaskPlugin had lost both entries in the inherited 65-second command budget.
The current plugin service can spend up to 30 seconds inspecting native state
and eight seconds awaiting the native write. The ordinary client's five-second
receive timeout could report an uncertain outcome before the original reply.

An isolated local socket test receives one exact plugin command and its receipt,
then returns success after six seconds. Before the fix, the expected accepted
result fails. Restore SetTaskPlugin in both the outer command and inner transport
budget selections. The same test now passes and checks that no second connection
is made. No retry is added and other command budgets stay unchanged. This is a
compatibility fix for the existing optional selector, not a new plugin feature.

## Complete client mapping

| Difference | Current owner and disposition |
| --- | --- |
| Prompt media and preflight | Resolve relative local media only from the exact work/thread directory. Validate native input and complete turn envelope using current model settings and explicit overrides, without changing history. |
| Canonical upload | Stage bounded UTF-8 fragments under the complete upload identity and action-derived command key. Read durable progress; unknown/non-progress never permits a blind retry or model submission. |
| Prompt edit pages and send status | Retain phase/review/receipt/turn/item/size/offset across pages; require exact status identity. The read path neither confirms edits nor sends input. |
| Recap and native descendants | Add read-only observations through existing typed service queries. The getters do not start inference or create local execution ownership. |
| Voice, exposure, model settings, plugin and permission reads | Existing implementations move within the file. Keep local-profile checks, source checks where present, bounded deadlines and socket closure. |
| Task model review | task_model_selection and its older DTO/query move to model_selection and the current model-review owner. chief_models.rs calls the current method. This is a local API change, not native capability loss. |
| Live reviewer wrapper | live_reviewer was a wrapper over live_settings. Current chief_live_settings.rs uses the shared reader with model choices included; reviewer and model controls share that observation. |
| Native goal | goal_state becomes native_goal(work, thread), with exact work/thread validation and a check on the nested goal thread. The new owner has explicit unavailable/unsupported/disabled states distinct from an authoritative empty goal. |
| App connection and hooks | Add saved-app and hook reads beside the retained pending-request app-setting read. Native configuration and service review tokens retain mutation authority. |
| App UI | Add exact source, bounded document/receipt chunks, pending-call and review reads. Verify echoed request, fingerprints, offsets and capacity before returning content. Confirmation remains a separate action. |
| Output observation | One retained connection carries bounded revision-based output queries. Verify server/version/query/work, coalesce snapshots and cancel when the receiver closes. No start/resume/replay. |
| Steer receipts | Move the existing exact submission-identity check without weakening it. |
| Action routing and timeouts | Route all added actions to their exact local owner. Preserve existing special budgets; restore the missing plugin entry. App UI confirmation retains its separately longer deadline. |
| Tests | Add prompt/timeline modules, output cancellation, page-content hash and App UI source/chunk checks. Move the input-receipt test with its current DTO fields; retain its rejection cases. Restore the removed native-goal wire coverage against the current API. |

The native-goal regression exercises authoritative empty, a present goal, wrong
work, wrong outer thread, wrong nested goal thread, unavailable, unsupported and
disabled results. It asserts a read-only exact query and no reconnect. It restores
the inherited empty-goal intent without reviving the obsolete local query shape.

The receipt fixture's restored voice_session_id and protocol 2.95 come from
merged PR1630. The current tree includes them; this audit does not undo that fix.

## Execution and request helpers

chief_execution.rs preserves nullable reasoning instead of wrapping it in Some.
Its common apply_to_native_turn helper writes only explicit model/effort/tier
choices. Explicit tier takes precedence over legacy Fast; omitted fields inherit.
The shared helper is consumed by Chief turns and prompt preflight. The creation
wire test distinguishes null inheritance from literal none/high values.

client_request.rs accepts the composed approval-envelope bound instead of only
one native message's bound. After complete contiguous pages arrive, it hashes
(event, work, method, content) and rejects content mismatch. Exact ownership,
method whitelist, 8 KiB UTF-8 page size, offset/total/cursor checks and unavailable
handling remain. The wire regression now also rejects tampered final content.

## Validation and limits

After rebasing onto merged PR1630, all 167 protocol library tests pass and strict
stable protocol Clippy passes for every feature and target. The delayed plugin
case is a real before-fix failure and after-fix pass; native-goal coverage uses
eight response cases. No live account, native configuration or model request is
used. Original hashes match all three source rows; git diff --check passes.

Close only these three complete file dispositions. Shared wire/Chief DTO and
runtime owners remain separate reviews. Installed-native version limitations,
final signed desktop acceptance and the user's optional-feature subtraction
review remain open. Automations remain paused.

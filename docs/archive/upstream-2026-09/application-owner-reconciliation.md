> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile the shared application projection

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
The complete inherited application diff was read against main `a058dd53c`.
The preserved snapshot hash matches the register. Subsequent review restores
output filtering, account-link metadata, removed coverage and the detail limit.
This note closes the source comparison for `application.rs` only.

## Owner and behavior mapping

| Inherited difference | Retained owner and disposition |
| --- | --- |
| Query function reordering | The service remains the local query owner. `query_guardian_detail` joins `query_guardian`; archive and installation queries join `query_native_lifecycle`. Each arm calls the same dedicated owner and returns a typed unavailable result if that owner is absent. |
| Goal and model query names | `query_chief_goal` becomes `query_native_goal`, which reads the native goal for the exact work/thread pair. `query_task_model_selection` becomes `query_model_selection`. No local goal executor replaces the native owner. |
| History query | `query_live_chief_history` becomes `query_history`. It retains paged history and adds weather enrichment through the host. Weather is optional presentation. |
| Ordinary recovery queries | `query_ordinary_recovery` routes turn outcomes and creation receipts to their existing durable owners. Model review checks pre-session state, no native session, no admitted turn, and the saved request before and after the catalog read. It does not submit the request. |
| Conversation projection | `pre_session_recovery` becomes `pre_session_presentation`, which returns state and recovery action together. Native settings attach to the completed summary, including pre-session summaries. Optional reasoning effort and account/revision/process identity pass through the protocol validator. Archived records have an explicit archived result; invalid identity or revision stays unavailable. |
| Conversation publication | History changes emit `ConversationHistoryChanged` on the conversation stream. Streaming output keeps its distinct text event and turn identity. Execution settings preserve explicit overrides through their existing request owner. |
| New query routes | Recap, prompt input, App UI and native-agent queries delegate to the owners reviewed in [the host comparison](chief-host-reconciliation.md). App UI receipts use the durable receipt owner; resource and action queries use the live host. Missing owners do not manufacture live state. |
| Selected requests | The public query requires the live host. It rechecks the same durable request and live native connection after a file-detail read. Saved file changes use their recorded evidence. Scoped tests preserve background and child ownership without pretending a disconnected public query is live. |
| Request content | Restore native connector/link metadata and both executor variants. Page digests bind the complete selected content, including enriched diffs. Changed content and resolved requests invalidate continuation. See [approval recovery](application-approval-recovery.md). |
| History and output | Completed-message identities use turn and item pairs. Incomplete output remains readable. Capacity pending, cancellation and exhaustion have distinct notices. A normal interruption is a stop, not a failure. Async question provenance and other-client resolution retain their saved owners. Restore the credential filter, unknown-kind exclusion and hidden internal voice provenance described in [output recovery](application-projection-recovery.md). |
| Settings projection | Preserve the existing settings DTO and add its optional automatic-recap preference. This preference does not activate the maintainer automation. |
| Removed test helper | `render_chief_history_for_auth_test` has no call site in the preserved Rust snapshot. It only filters the retained test renderer by `auth_recovery`; no production owner is removed. The interruption test is renamed to assert the current normal-stop behavior. All other removed approval and model-review coverage is restored. |

## File-detail limit

Restore the inherited failure branch in `attach_file_approval_detail`: if the
complete enriched request exceeds the protocol envelope limit, return unavailable.
Do not return the original request with its newly read file evidence silently
removed. A synthetic oversized detail previously yielded an available request
containing only its reason. The restored branch makes that request unavailable
through the page projection.

This is a local boundary test, not evidence that an installed native executable
can generate such a detail. The current native reader bounds its response and
projected text; their limits reduce reachability. No transport limit is increased
and no additional validation layer is added.

## Classification and evidence limits

Request identity, complete file evidence, source-bound model recovery and output
correctness are core behavior for existing consumers. Weather, recap, settings
pickers, App UI and native-agent presentation remain optional product surfaces.
Their current routing is accounted for; this comparison does not decide that the
user should retain them. Removal must preserve their native or durable owner
where another consumer still uses it.

The failing-before boundary test is recorded in
`/tmp/decodex-application-detail-before.log`. All 53 application tests pass with four existing opt-in tests ignored
(`/tmp/decodex-application-detail-after.log`). Strict stable runtime Clippy passes
for all features and targets (`/tmp/decodex-application-detail-clippy.log`). The earlier approval and output batches retain their own evidence.
Closing this source row does not close native version qualifications, other
shared files, the optional-feature inventory or final signed desktop acceptance.
Automations remain paused.

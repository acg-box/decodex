> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile the shared desktop surface and capture tool

Read the complete inherited/current chief_surface.rs difference (2004 lines) and
workbench_visual_capture.rs difference (504 lines). Preserve the fixed upstream
cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582` and the existing owner boundaries.

## Request readback repair

The old pending-event and source checks had been removed from load_request.
Completion checked only selected work. Extract the existing completion body and
reproduce the defect with a pending event resolved before its reply: the old body
still installs the reply. The compiled regression fails in
`/tmp/decodex-request-source-before.log`.

Capture the existing profile epoch, runtime source and complete pending event.
At completion, require the same service, selected work and still-pending event.
Reject disconnected state. An ordinary snapshot refresh remains valid because
it does not change these owners; the generic refresh generation is not a service
identity. This avoids discarding valid long reads on every polling cycle.
The production completion test covers unchanged state, polling, resolution,
event replacement, runtime replacement, profile replacement, selection and
disconnect. It simulates completion state, not a live provider approval.

## Whole surface mapping

| Inherited behavior | Current owner or explicit presentation change |
| --- | --- |
| Module and field registration | Goal and task-model panels move to chief_native_goal and chief_models; plugins keep one module. New prompt, recap, hook, native-child, output-stream and weather fields use their existing dedicated owners. No inherited test function is removed: all 16 remain. |
| Input and creation | Shared creation defaults replace the literal model default. Nullable reasoning and per-field intent reach both public start fields. Resource inputs move into initialization; exact model input stays separate from observed settings. |
| History and notices | Exact question scope, identical-result short circuit and bounded refresh timing retain current history. Voice captions reconcile through their restored owner. Output streaming binds work/turn/item; native receipts and local progress both retain capacity cancellation. |
| Commands and drafts | Existing durable draft fences precede dispatch; canceled queued commands retain their original input. Interrupt uses its independent exact-turn owner even during ordinary send uncertainty. Recap and prompt reviews invalidate when new input changes their source. |
| Rebinding and failure | Profile binding resets dedicated observations and preserves recovered drafts. Snapshot invalidation clears source-bound panels. Transient read failures keep the last confirmed view for two attempts, while explicit unavailability is stale immediately. This is a current status policy, not proof of a live connection. |
| Detail and settings entrypoints | Agent settings now host resources, integrations, voice, usage, native goal and task settings. The transcript hosts conversation, requests, prompt editing and recap. The old standalone ChiefPreferences wrapper is replaced by the composer settings popover. |
| Inspection and relationships | Compact inspection retains work status, parent, due time and a combined work/thread copy reference. It no longer displays separate raw judgment/execution rows or a thread-only copy button. Restore Requires, Required by and Coordinates links inside this existing overlay: the scoped graph alone cannot expose dependencies outside its current scope. |
| History navigation | Paused-follow proximity triggers prefetch instead of a Load earlier button. Cursor progress, retry delay and message-position anchoring retain the current owner. Failed older-page reads back off instead of replacing global feedback. |
| Rendering | Response filtering, weather copy text, selectable notices and billion-token formatting use retained presentation owners. Streaming text replaces repeated Markdown rebuilds. Working animation and compact external-writer wording are presentation differences, not execution claims. |

Relationship restoration uses the existing snapshot and navigation helper. It
introduces no data owner or persistent state. The rendered regression includes a
dependency outside the selected graph scope, a reverse dependency and coordinated
children. Existing inspection geometry verification checks that the overlay does
not resize or scroll the transcript. These checks are not signed desktop approval.

## Capture tool mapping

The original read-only snapshot, selection, history, request, Guardian and evidence
file logic moves into read_service_projection. The original composer-send and
steer-receipt probes remain. Explicit command flags still require an explicit root;
new automatic recap, App UI and media probes also require the native fixture marker.
Layout-only App UI and integration fixtures reject service-source combinations.

Added probes retain distinct claims: automatic recap waits for a ready receipt;
App UI checks no execution before confirmation and a completed browser round trip;
media checks the native preview result. Their bounded loops and evidence files do
not qualify real microphone audio, ordinary installed-app lifecycle or all live
connectors. Additional creation-default and panel-preference modules support the
same production surface. No original probe is removed or silently enabled.

Close only these two file comparisons after validation. Native incompatibilities,
the overall review register and the final signed desktop artifact remain separate
work. Automations remain paused, including after completion.

Validation: the eight-case completion test and both inspection tests pass. The
complete desktop binary suite passes 541 tests with five explicit opt-in skips
in 14.27 seconds. Strict all-feature/all-target GPUI Clippy passes in 5.39 seconds,
including compilation of the capture target. Logs: /tmp/decodex-request-source-final.log,
/tmp/decodex-inspection-relations-tests.log, /tmp/decodex-shared-surface-suite.log
and /tmp/decodex-shared-surface-clippy.log. No installed account or provider was used.

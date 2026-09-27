# Reconcile shared native settings observation

The complete inherited `database/src/chief_task_settings.rs` and
`crates/decodex-runtime/src/chief/task_model_settings.rs` are implemented through
`chief_settings_observations` and the coordinator's `native_settings` module.
The current runtime delegates model, permission and plugin persistence to their
existing owners. Do not restore another writable settings cache.

## Database owner

The shared immediate transaction retains the inherited exact thread-to-work
binding, generation ownership, source digest validation, object-only projection,
explicit unavailable marker and source/value deduplication. Reads use an owned
transaction snapshot. A-B-A identities include the previous event ID. Stored
observations are resolved records and do not create wake work. Publication and
response-only methods remain separate; only publication calls receipt observers,
including when the observation itself is deduplicated.

The model event kind is now `native_task_models`; permission and plugin kinds
retain their names. Current model consumers obtain transport-current facts before
reading this projection. Old `native_task_settings` records are not promoted to
new source evidence. The observer does not rewrite them. Both old and current
private settings kinds are excluded before visible history pagination. Preserved
model request receipts have their separate compatibility reader.

The current shared projection limit is 64 KiB. The inherited model-only 4 KiB
limit is no longer a separate branch. The production model projection still has
only model, provider, effort and tier, with bounded strings supplied by
`NativeTaskModelSettings`; it does not accept arbitrary instruction payloads.
Permission and plugin projections keep the shared 64 KiB limit.

## Coordinator owner and consumers

| Inherited responsibility | Current owner |
| --- | --- |
| Accept exact complete resume settings | `hydrate_dispatch_thread` and `hydrated_thread_matches` retain exact thread and bounded model/effort checks, including explicit null. The native transport hydrates only the matching guarded resume response. |
| Project model, permission and plugin facts | `persist_task_settings` delegates to the three `persist_current` functions. Each reads current transport facts and a live guard; missing facts invalidate the durable projection. Only projected typed fields are serialized. |
| Observe settings notifications without creating work | `observe_settings_notification` retains the capacity-selection cancellation check, then persists current transport facts. It cannot let a stale queued payload replace newer wire state. |
| Observe configuration after start, resume or joining a native turn | The coordinator start path, dispatch hydration, resume recovery and native-turn join paths call `persist_task_settings`. Service model, permission and plugin inspectors also persist current facts before constructing a review. |
| Retain configured permission facts at turn completion | The transport retains configured facts while a turn is active and refreshes the idle guard only for the matching completed turn. Current persistence reads configured facts, rather than depending on an idle-only cache. Effective settings changes arrive through their own native publication. The old extra completion callback is not a second settings authority. |
| Enforce ownership and validity | The shared database owner rejects invalid source identifiers and refuses foreign or ambiguous bindings. Native metadata remains bounded and source checked; a caller cannot turn historical response data into a new publication. |

The five merged coordinator wire tests exercise all three projections, exact and
incomplete resume replies, stale queued payloads, privacy, empty and null plugin
lists, foreign generations, same-source deduplication, new wire revisions, A-B-A
transitions, reopen, no native requests and no wake events. The permission receipt
test separately demonstrates that historical `thread/read` facts cannot confirm a
pending selection. Database model and receipt tests qualify publication-only
settlement and legacy compatibility.

Validation evidence: the five tests and strict runtime Clippy passed in
`/tmp/decodex-settings-response-final.log` and
`/tmp/decodex-settings-response-clippy.log`. These implementation and test files
are unchanged by this documentation-only reconciliation.

Close only these two complete shared-settings file rows after checking their
original snapshot and current owner hashes. Broad coordinator/history files,
remaining UI acceptance and signed desktop validation stay open. This is core
observation and saved-state correctness; removing an optional control must not
remove it. No production code, schema, installed binary or automation changes.

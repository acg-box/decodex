# Reconcile the inherited task model owner

The complete inherited database writer, its 586-line regression module and the
293-line manual service producer were compared with the current model owner.
Earlier batches restored account binding, queued-input precedence, ordinary
Reserve exclusion, immutable history and legacy automatic occurrence identity.
This batch strengthens the remaining concurrency and restart qualification. It
does not add a production writer, product control, migration or native workaround.

## Writer and producer mapping

| Inherited responsibility | Retained owner |
| --- | --- |
| Reserve before sending; one pending operation; exact source and settings event | `database/src/chief_models.rs` uses the immediate SQLite transaction, current ownership and latest complete model observation. It checks the reviewed manual account or automatic account/banner context, queued explicit input, conflicting settings edits, unresolved prompt edits and misalignment. |
| Preserve stable occurrence identity across reopen and restart | Manual keys bind the reviewed token. The runtime token binds task, thread, generation, account/revision, history, saved settings and catalog. Automatic keys bind the original eight-part occurrence; the legacy prefix is also checked before reservation. New request IDs, process IDs or account refreshes cannot replay that occurrence. |
| Record the native response once without overwriting confirmation | `finish_chief_model_selection` matches the reservation ID, key and exact serialized attempt, then appends one result. `chief_model_history` reads original response and observation separately. |
| Confirm a target only from current complete native facts | The shared settings transaction calls the model observer only for a publication. Source, account revision, model/provider/effort and known tier are checked. Automatic fallback requires its exact expected tier; manual partial edits preserve the tier. |
| Reconcile after the original process is confirmed dead | Preserved legacy requests and new source-bound requests retain unknown original delivery even if the new owner's settings match. A ready current owner, complete facts and an available account are required. A response-only record does not authorize reconciliation. |
| Inspect catalog choices and issue an explicit task-default update | `crates/decodex-runtime/src/chief_models.rs` owns read and write. It compares saved and transport-current facts, filters Reserve, binds the review to the exact source and catalog, rechecks before reservation and send, and uses `ThreadModelSelection`. The payload does not reset provider, tier, collaboration mode or instructions. |
| Retain historical manual or automatic receipts in the service | The shared history reader supplies original response, target observation and restart-reconciliation provenance. Current configuration and historical requested configuration remain distinct. Pending receipts cannot authorize another edit. |

The old service converted a received response to uncertain when the source changed
after the RPC. The current service preserves a received queue acknowledgment as
the original response. That acknowledgment still does not prove application:
new manual requests store their reviewed account revision, and a later publication
must match it. The UI reports submission and independently reads saved state. A
changed source cannot confirm the old target or authorize replay.

Older current-format manual records that lack account provenance retain their
established interpretation; no account evidence is invented for them. Their
history still identifies restart reconciliation. This is separate from preserved
legacy records, which already carry account identity, and from new manual records,
which always receive source provenance from the production producer.

## Complete inherited regression mapping

| Original scenario | Current evidence |
| --- | --- |
| Single use across clients, reopen and an uncertain acknowledgment | `model_reservation_survives_crash_and_blocks_dispatch_until_native_confirmation` now opens two independent SQLite stores. Different review tokens race, so the result cannot be explained only by a duplicate-key constraint or one in-memory connection mutex. One reservation wins, survives reopen and retains an immutable unknown response. |
| Recheck account, revision, thread, generation, settings and queued manual choices | Source-bound automatic/manual fixtures, latest-event rejection, owned-generation fixtures and the queued model/effort matrix cover these gates. |
| Rejection consumes the occurrence but permits a new occurrence | Current model-key regressions and `completed_legacy_fallback_cannot_replay_in_the_current_journal` check current and preserved keys, immutable response and distinct banner admission. |
| Separate target publication from acknowledgment and replay | Automatic tier and source fixtures reject incomplete or different facts; current history and legacy fixtures retain the original response and release only the pending fence. |
| Response-only facts cannot settle a request | Database fixtures keep the receipt unresolved after saved response facts; the publication path can settle even when the same observation row is deduplicated. |
| Historical target and response survive later configuration changes | The shared legacy/current history fixtures and service receipt fixture distinguish the requested target from current configuration, preserve unknown response and enforce exact task ownership. |
| An unset expected tier requires explicit null | The preserved automatic unset-tier fixture rejects missing, malformed and different tiers before accepting explicit null. Shared model fact parsing retains the same requirement for current requests. |
| Same-model effort edits share serialization and require publication | The queued-input matrix permits an explicit same-model effort change while idle or running, and mixed-owner tests serialize model, permission and plugin edits. Source/legacy manual fixtures distinguish queued response from observation and retain spent review identities. |
| Restart reconciliation never claims old delivery succeeded | The source-bound fixture now covers manual and automatic requests with both matching and different new-owner models. It checks disabled-account refusal, unknown response, reconciliation provenance and subsequent permission admission. Legacy and older unbound-format fixtures retain their distinct compatibility rules. |

All 22 database model tests and strict database Clippy pass. The strengthened
checks use disposable SQLite files only. Logs:
`/tmp/decodex-model-owner-qualified.log` and
`/tmp/decodex-model-owner-qualified-clippy.log`.

Close only the three complete inherited writer, manual producer and regression
file rows. The shared settings observer, shared test registration and remaining
history consumers require their separate file reviews. This is not completion
of R03 or signed desktop acceptance. Native child-input and Flex restart limits
remain separate open acceptance items.

The optional surfaces are manual model controls and automatic fallback policy.
Persistence, original response history, source validation and no-replay handling
are core correctness for existing saved state. A removal review must preserve
those contracts even if a control or automatic policy is removed.

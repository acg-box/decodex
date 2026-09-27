# Reconcile shared wire and Chief message contracts

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete inherited/current wire.rs and chief.rs diffs. The current
comparison includes the merged PR1630 voice receipt and protocol 2.95 changes.
This batch restores tests; it does not add production protocol fields or actions.

## Shared query and command envelope

| Difference | Current disposition |
| --- | --- |
| Automatic recap preference | Add the opt-in desktop field, default false. Omitted updates preserve its value. This preference is separate from scheduled maintenance, which stays paused. |
| Prompt input and history edit queries/results | Bind upload, send status, directory and review pages to their original identities; retain read/confirm/send separation in the existing service owners. |
| Recap, hooks and saved app settings | Add typed observations alongside the retained voice, exposure, pending app, plugin and permission queries. Reordering existing variants does not change their wire names. |
| Model review query/result | Replace GetChiefTaskModelSelection/ChiefTaskModelSelection with GetChiefModelSelection/ChiefModelSelection and the current model-review owner. |
| Native goal query/result | Replace GetChiefGoal/ChiefGoal with GetChiefNativeGoal/ChiefNativeGoal, requiring the exact native thread as well as local work. Preserve explicit unavailable states rather than inventing an empty goal. |
| App UI queries/results | Separate document/source reads, pending-call discovery, callback review and receipt pages. Execution is a reviewed Chief action, not a query. |
| Native descendants and live output | Add bounded read-only descendant inspection and revision-based output observation. No second agent execution owner. |
| Ordinary recovery queries | Add original turn-outcome and creation-receipt reads, plus configured model settings. Wire validation requires canonical source IDs and valid original account revision/message where applicable. |
| Creation source boxing | Box InitialModelSource inside CreateConversation; serialization remains the same. Restore its source-coordinate roundtrip/rejection test. |
| Ordinary turn overrides | Optional explicit field choices preserve the legacy all-explicit payload when absent. |
| History notification | Add ConversationHistoryChanged; application publishes it and the client lifecycle invalidates the affected conversation history. |
| Account rejection labels | Add closed source-account-unknown and credential-conflict outcomes from the existing AccountService error projection; no raw credentials or backend errors enter the DTO. |

All existing CommandPayload variants remain. The only removed query/result names
are the two model/goal pairs above. Their current application dispatch branches
were inspected. GetChiefLiveReviewer now requires include_models in its exact
current shape; the old optional default is not retained. Activity-detail cursor
omission is now accepted as None. These are explicit local contract differences,
not a claim that old payloads are byte-compatible. Exact-current negotiation
rejects older clients; the golden fixtures use 2.95.

## Chief actions and presentation

The sole removed Chief action name is SetPermissionProfile, replaced by
SelectPermissions with the same task/thread/review/profile intent and current
service permission owner. Existing SetTaskModel now carries model plus optional
effort rather than an older selection wrapper. Live reviewer uses ChiefReviewer.
Current service branches consume these shapes; they do not bypass native policy.

Add the delivered prompt-upload/edit/acknowledgement/send, recap, saved-app, hook,
App UI acknowledgement/confirmation and explicit native-agent input actions.
These are references to existing feature owners, not new adoption in this batch.
Configured starts accept partial execution overrides, and absent/null effort
inherits native configuration. Full explicit legacy execution values retain
their current decoded meaning.

ChiefRequestText now uses the composed approval-envelope bound. History entries
add exact native source, turn and weather observations. Displayed exited status
does not imply success. The voice receipt identity and its original legacy-wire
test are restored by PR1630. Public live-message kind uses snake_case instead of
the inherited camelCase; this is covered by the exact local protocol boundary.
It labels only public response/plan/summary text and creates no execution authority.
Misalignment review identity includes its source connection. Catalog program
metadata remains informational. ChiefOutputResult adds a coalesced observation
with explicit work ownership and service-lifetime revision.

## Restored coverage and acceptance

Restore initial_model_source_roundtrips_and_rejects_invalid_coordinates from the
preserved snapshot. Its only adaptation is Box::new for the current field. The
four cases retain valid roundtrip, zero/negative revision and noncanonical-account
rejection. Restore the inherited 2.47 case beside legacy/future versions in the
exact-current query/command/event gate test. The two model-review tests moved
within wire.rs; their original cases remain. The recap omission test is additive.

All 166 protocol library tests pass on this branch, including the restored case.
Strict stable Clippy passes for all protocol features and targets. No production
source behavior changed, and no live configuration or account was touched.
Snapshot hashes match both full source rows; git diff --check passes.

Close only wire.rs and chief.rs full-file dispositions. This does not close
shared runtime/application/desktop reviews, version-specific installed-native
limitations, signed application acceptance or optional-feature decisions.
Automations remain paused.

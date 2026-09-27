# Reconcile inherited large approvals

This maps the complete preserved `work/large-user-approval.md` from the manual
catch-up to current owners. The original snapshot and its dated evidence remain
preserved. The fixed upstream is `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
This document closes the inherited document and shared reviewer fixture rows.
It does not close approval acceptance.

## Implementation mapping

| Inherited requirement | Current disposition |
| --- | --- |
| Immutable complete approval payload and compact inbox scans | `database/src/chief_request_payload.rs` and registered migrations 39/40. Exact event reads hydrate the stored payload. Transaction failure leaves no compact-only receipt. See [storage](large-native-approval-storage.md). |
| Native 8 MiB message bound and composed file evidence | `MAX_NATIVE_MESSAGE_BYTES` remains 8 MiB. `MAX_APPROVAL_ENVELOPE_BYTES` permits two bounded native parts plus 64 KiB metadata. The store checks each part, request kind, file identity and metadata. See [live file evidence](live-native-file-approvals.md). |
| Complete 8 KiB UTF-8 request pages | [Page transport](large-native-approval-pages.md) binds event, work, method, digest and offsets. Assembly has a total deadline and exposes no partial approval. The later composed-envelope change supersedes that batch's original 8 MiB assembly bound. |
| Explicit large permission and offered policy decisions | [Decision handling](large-native-approval-decisions.md) reconstructs the selected original response. Free-form input keeps its separate limit. No automatic approval or replay is added. |
| Reader navigation and stale controls | The existing GPUI reader supports mouse and keyboard sections and rejects controls from an old request revision. Unlike the inherited design, current explicit approval does not require a mandatory visit to every page. This is a recorded product difference, not equivalent behavior. |
| Pending file diff before native history | `chief/file_changes.rs` retains exact connection/thread/turn/item evidence within 32 items and 32 MiB. The coordinator releases it after commit, not after a failed write. Duplicate requests reuse the immutable receipt; completion, thread lifecycle and connection close release transient state. |
| Child and background approvals | Current projection uses retained native ownership and a live request guard, not the parent's active turn alone. `respond_pending_event` rechecks owner and connection and consumes the exact original request guard. Offline history cannot authorize a reply. |

## Native Guardian sections

The original document also records native-owned behavior. Keep these distinctions:

- Policy-template substitution and parent-tool isolation map to the retained
  large-action native fixture. The installed-native qualification in
  [Guardian evidence](native-guardian-evidence-qualification.md) includes the
  complete action, substituted template, unsupported parent-only tool and denial.
  This does not add a local template editor or qualify every MCP tool boundary.
- Answer retention across automatic compaction and cold restart maps to the
  retained `chief_process_native_guardian_evidence_tests.rs` fixture. Its unique
  answer, original restriction and opaque checkpoint assertions remain. It does
  not qualify incompatible hashes, pending-review migration races or all child
  propagation. Native Codex owns these internal policies.
- The historical shutdown probe demonstrated EOF exit only. It did not establish
  entry into a retry sleep, tracker completion, Linux proxy cancellation or late
  cached-allow races. Keep these limits open in R08/R10; do not upgrade that old
  observation to current lifecycle acceptance.

## Evidence and remaining acceptance

Fresh current-source checks pass three database request-payload tests and two
runtime file-approval tests. They exercise atomic failure, compact reads, exact
cold readback, changed-content rejection, bounded composed evidence, retained
file evidence after write failure, duplicate delivery and one-shot decline.
The GPUI large-test selection passes three tests; two cover approval reader
navigation/stale controls and permission dispatch. The third covers composer undo
and is not approval evidence. Logs:

- `/tmp/decodex-approval-reconciliation-storage.log`
- `/tmp/decodex-approval-reconciliation-files.log`
- `/tmp/decodex-approval-reconciliation-reader.log`

These fixtures use disposable storage and simulated transport. The rendered
permission test proves dispatch, not a service acknowledgement. This audit does
not rerun installed-native Guardian cases or claim signed approval acceptance.

The old source-queue position, schema 41, protocol 2.59, unmerged status and bundle
path are historical. Current registered migrations and local protocol own the
contract. The signed 0658 artifact's limited acceptance does not cover approval
inspection, accept/decline, stale resolution, reconnect or uncertain replies.
Those paths, maximum-size page latency and broader R08/R10/R12 remain open.
Automations stay paused, including after manual completion.

## Shared reviewer fixture

The complete diff of
`account_launch/chief_process_native_reviewer_store.rs` was also reviewed.
The inert account, capability attestation, process-generation binding, task/thread
binding and source-key setup remain unchanged. Explicit imports replace a glob,
`ChiefAppReviewer` becomes `ChiefReviewer`, and `observe_model` moves without a
behavior change. Reviewer publication still rejects a stale source and duplicate
review, reads the durable applied receipt after reopening, leaves no pending
local event and rejects a completed native target.

Every original child test module remains registered: model settings, permissions,
voice tails, plugins, warnings, live models, task models and App exposure. Some
move within the file or change the local module name. Added outcome, service,
App UI, hook and voice-start modules do not replace those original modules.
New helper methods use existing permission, model, plugin and hook service owners.
They check native readback or saved hook trust, reject a reused review and retain
the relevant durable receipt checks.

This reconciles the shared fixture source, not all features that use it. The
fixture seeds synthetic admission and inert credential rows. It does not qualify
real credential enrollment, kernel process admission or complete desktop behavior.
Native cases keep their explicit opt-in requirement and their separate evidence
records. No live native case is inferred from a skipped test.

Fresh shared-fixture validation passes 22 tests with 14 explicit native skips in
`/tmp/decodex-reviewer-store-reconciliation.log`. The original module-path set is
a subset of the current set. Source comparison and existing native qualification
records establish the retained helper mapping; the 22-test run alone does not
qualify the skipped native cases. No production or test source changes are made
in this reconciliation batch.

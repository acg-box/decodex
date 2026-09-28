> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore complete Guardian review details

Large saved reviews could not be inspected in Decodex. The summary withheld the
action above its display limit and offered no detail reader. Restore the inherited
8 KiB UTF-8 page query, bound to the saved row and digest. The reader uses the
existing observation store and sends no native request. No second output store or
approval policy is added.

The desktop shows the complete action and findings over successive pages. It
shows the explicit approval control only after contiguous pages reach the end.
Skipped pages cannot complete review. Changed evidence discards prior detail
state. Existing task, connection, pending-command and native approval checks stay
in force. Submission acknowledgment does not claim execution.

The restored restart test also exposed a separate loss: the decoder rejected a
completed review when its rationale exceeded 64 KiB. The saved state then remained
in progress. Remove that unsupported field limit and retain the existing total
native-message bound. Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`
exposes `GuardianApprovalReview.rationale` as `Option<String>` in
`app-server-protocol/src/protocol/v2/item.rs`; it declares no 64 KiB field limit.
The complete long action and rationale now survive wire receipt and storage
restart and can be reconstructed exactly through bounded pages.

## Reconciliation

Restore the complete protocol and desktop detail-reader behavior. Retain the
current denial wording. Restore the runtime reader and compact action projection;
retain long ASCII and Unicode coverage as paging tests and keep secret/unknown
content ineligible for approval. Restore all four other missing coordinator
regressions: absent assessment after review failure, late review cancellation,
complete large approval requests and foreign paths. Keep current cold-resume and
expanded-frame checks. The adapter retains its newer complete-action test and
adds long findings to that case.

The shared client, wire, exports and application modules remain open for their
other inherited differences. Local protocol advances from 2.92 to 2.93 because
this batch adds the detail query/result and summary field. Database schema 48 and
native APIs do not change.

## Verification scope

The initial restored long-review test failed because the completion was dropped;
the log is preserved. After the decoder fix, 9 adapter tests, 4 protocol tests and
19 runtime tests pass. The runtime test reconstructs complete multilingual and
escaped text after reopening SQLite, rejects wrong work/digest/boundary requests,
and invalidates old pages after a conflicting writer. Three native opt-in tests
are excluded from that runtime count.

Four desktop tests and strict adapter/protocol/runtime/GPUI lint with all features
and targets pass. The restarted reader test also passes through the actual
application query dispatcher. Signed desktop acceptance remains separate. Guardian presentation is optional for the user's
removal review; native enforcement remains upstream-owned. If the presentation
is retained, complete evidence and explicit approval are required. Automations
remain paused.

# Preserve complete file approval history

The native-history fallback for file approvals called a detail projector that
stopped at 24 KiB. The general activity reader had separate complete pagination,
but this approval path received only the shortened result. The inherited long
file-diff test had also been reduced to a short string.

Restore the original complete projector with its existing 8 MiB bound. Keep
bounded presentation in the existing page function. Restore the original long
Unicode diff/suffix assertion and the complete-before-request-paging test. Both
fail before the fix: the suffix is missing and the result is marked truncated.
The final projection preserves the suffix without changing native request params,
request liveness, approval identity or decision authority.

The service uses this fallback only when saved pending-file evidence is absent.
It rechecks the live request before attaching the exact detail to the approval.
Saved pending-file evidence, the composed-envelope limit and request pagination
retain their existing owners. This is a compatibility fix for an existing path,
not a new approval mode.

## Complete inherited file mapping

The complete `chief_detail.rs` diff was reviewed:

- Restore the removed nine-case `activity_detail_rejects_changed_or_missing_source`
  transport test unchanged. `read_bound` still checks account, process, account
  revision, history revision, thread, work and source presence after native reads.
- Tool detail extraction moves to `chief_tool_detail.rs`. Existing server/tool,
  text results and errors remain. Resource/media descriptions, structured results
  and failure tails are additive. Standalone function output uses the shared parts
  owner so all text blocks remain available without media or encrypted bodies.
- App context is descriptive native metadata. The current `Link:` label replaces
  `Connected account link:` and the shared extractor can display supplied context
  on dynamic tool items too. Neither path derives account identity from arguments.
  Extend the current context test with a forged argument link and assert it is
  absent. The old MCP-only display restriction is not claimed as unchanged.
- Web detail tests move to their current module. Query deduplication and URL/pattern
  content remain; find output order changes. Empty results, missing results and
  opaque future result fields now have distinct text. The common redaction and
  complete UTF-8 pagination remain. The moved long-patch test checks changed source,
  content, invalid offsets and a final suffix.
- Image detail retains the path and explicitly says that executor identity and
  image bytes are unavailable from that record. It does not open the image or
  infer a local execution environment.
- A test-only unbound reader is added. Production activity reads retain their
  source-bound paginated owner. Complete file reads use the restored projector.

All other inherited file source remains unchanged. Close only this detail-file
row after validation. The shared application, adapter, request UI and full signed
approval acceptance retain separate entries.

## Evidence and limits

The restored regressions fail before the fix in
`/tmp/decodex-file-approval-complete-before.log`. Tests use simulated native
history and isolated storage. They do not prove signed desktop accept/decline,
real provider execution or maximum-size page latency. No user file, account,
credential or live approval is changed by this batch.

Final validation passes all 15 detail tests and strict runtime Clippy for all
features and targets. The file-approval integration selection passes five tests
with one explicit installed-native skip; one passing detail case overlaps the
15-test group. Logs: `/tmp/decodex-file-approval-complete-after.log`,
`/tmp/decodex-file-approval-complete-clippy.log` and
`/tmp/decodex-file-approval-complete-integration.log`. The complete projector and
source-identity regression match the original bytes. A new signed artifact is
required for acceptance of this production change.

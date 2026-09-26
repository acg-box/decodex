# Bind explicit continuation to current live findings

A saved precaution could expose continuation text after reconnect without current
native evidence. History now retains the explanation but exposes continuation
only when the active native connection has the exact matching live findings.
The review ID binds those findings to the live review identity. The host and
coordinator both reject an old ID before submitting the continuation.

Use the existing guarded native request writer for the final `turn/start`.
A native revert queued during history validation invalidates the guard before
any write. A known unsent stale-history result releases the local submission
claim while retaining the precaution. A lost response retains the unresolved
claim and cannot be replayed. Remote rejection uses fixed user text instead of
copying a provider error that can contain private input.

Current native model and effort inheritance remain unchanged. A detail-free
historical terminal record can match only after the current live details have
already matched the saved review. Existing GUI acknowledgment is bound to the
review ID and requires a separate second click; no new navigation or UI action
is introduced. The wire shape is unchanged.

## Evidence

The restored public-history regression first failed because saved-only evidence
still exposed a continuation. Focused tests cover read-only saved history,
stale confirmation IDs, exact live success, changed findings, native rejection,
uncertain delivery, a revert queued during history reads, and cold reconnect.
The fixture now keeps its notification receiver alive and delivers review changes
through the actual transport observer before the corresponding RPC reply.

An isolated native test passes with installed Codex `0.158.0-alpha.2`. It uses a
private temporary HOME and CODEX_HOME, an isolated database, and a loopback
Responses provider with synthetic data. Native shutdown is awaited. The provider
returns the exact streamed policy-violation shape in fixed upstream commit
`595cc91e8cbb1c2ca822d0311dcf12709410c582`, test
`codex-rs/app-server/tests/suite/v2/misalignment_policy.rs`.

The real native process emits the failed turn and live findings. Exactly one
provider request exists before explicit confirmation; a stale ID creates none.
A valid confirmation produces exactly one further request containing the
continuation text and completes. A duplicate confirmation creates no request.
This verifies the installed native protocol path with a synthetic provider; it
does not claim a live provider's authorization decision or signed desktop
acceptance.

This flow uses the adapter restored in PR1557. Shared source-file reconciliation
and final acceptance remain open until their other inherited differences and
required runtime/desktop cases are verified.

## Local validation results

All 194 selected Chief runtime tests pass; nine external tests are ignored in
that run, including the separately executed native qualification above. The
saved-history regression and focused continuation cases pass. The rendered GUI
check for second-click confirmation, stale findings and missing continuation
also passes. Strict protocol/runtime lint passes with all features and targets.
These checks do not replace normal signed-app visual acceptance.

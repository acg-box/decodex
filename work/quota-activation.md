# Automatic weekly quota activation

General settings contains one switch, **Auto-activate weekly quota**, on by default. Existing installations also receive the enabled default when migration 30
is first applied. A manually disabled preference stays disabled across restarts.
The existing daemon observer checks every 15 seconds. A future weekly reset means a
countdown exists and no request is needed. An expired reset permits one request.
Percentages do not decide activation. Missing quota, disabled accounts, missing
credentials, and a currently exhausted five-hour window block the request.

The request uses the existing Rust HTTP client and AccountService credential lock.
Before sending model input, a short-lived attested native process reads the selected
account's workspace route and managed requirements. It performs no model request
and is shut down after discovery. The same account credential lock covers discovery
and the direct HTTP request. If no attested profile is available, activation is
skipped while account health observation continues. A discovery failure makes no
model request and uses the existing rejection backoff.

The direct request uses the native backend origin, account-routing header and
residency requirement. It sends `gpt-5.6-sol`, low reasoning, one short message, no
tools, `store: false`, and streaming enabled. It creates no Decodex conversation.
The credential owner handles token refresh. The HTTP client does not follow
redirects or retry the request. This feature does not promise zero provider-side
retention. Account/profile/reset APIs retain their separate backend owner.

SQLite migration 30 adds the preference and one reservation row per account. The
daemon reserves before sending, so concurrent checks, restarts, and toggling the
preference do not replay an attempt. A positive `response.completed` event marks
completion. A connection failure or HTTP 4xx permits another attempt after 15 minutes. Other network errors, 5xx,
truncated streams, and shutdown after reservation leave the outcome unknown and do
not trigger immediate retry. This deliberately favors avoiding duplicate requests;
a request that never arrived can miss that cycle.

A completed or ambiguous request suppresses that exact reset indefinitely. Only a
newer reported reset starts another cycle. Older snapshots cannot roll the cycle
back. Re-reading the same expired timestamp only checks status; it does not resend.
On first enable, a future timestamp also skips activation. No inferred seven-day
fallback or precise balance is needed.

## Provider evidence

Reference: `openai/codex` main commit
`abbdde95b593594c4daa2677392dbfd1dd4ccb8b`:

- `codex-rs/model-provider-info/src/lib.rs`: ChatGPT Codex base URL.
- `codex-rs/codex-api/src/common.rs`: Responses request fields.
- `codex-rs/codex-api/src/endpoint/responses.rs`: HTTP POST and SSE transport.
- `codex-rs/codex-api/tests/sse_end_to_end.rs`: completion event and response identity.

The original qualification inspected CLI `0.155.0-alpha.9.2`. Its generated
`v2/GetAccountRateLimitsResponse.json` declares integer `usedPercent` and nullable
`resetsAt`; it does not provide a precise unused-cycle flag. The current route adapter additionally depends on native account and requirement
reads from the attested binary. At fixed upstream
`595cc91e8cbb1c2ca822d0311dcf12709410c582`, workspace routing is owned by
`app-server/src/request_processors/account_processor/workspace_routing.rs`.
Upstream source is not evidence that every account accepts the selected model or
that quota reset behavior is fixed.

The small completion reader recognizes only bounded SSE data events and positive
completion. No existing streaming SSE decoder is present in this runtime. Adding
an upstream agent runtime or a dependency for this one event would widen the change.
Tests cover chunk boundaries, CRLF, malformed data, truncated streams, byte limits,
and HTTP outcomes. Revisit this reader if upstream changes event framing or adds a
required transport; replace it with a shared decoder if another runtime caller needs SSE.

## Validation

Use the database and runtime activation tests, desktop-settings controller tests,
the local database gate, and the GPUI check. Tests use disposable databases and a
loopback HTTP fixture. They must not enable the feature in the user's database.
Protocol 2.43 keeps the new settings projection within the exact-version client gate.

### Local validation scope

Database tests cover default enablement, upgrade preservation, reset timestamps,
rounded percentages, concurrency, restart, manual disablement, and rejection backoff.
The loopback HTTP tests cover completion, connection failure, and ambiguous results.
The protocol and runtime libraries, GPUI settings controller and surface, strict
Clippy, architecture scripts, and local database gate are checked on the PR revision.
No real-account activation, app installation, or live settings change was performed.

## Current inherited-consumer review

The complete `account_api/activation.rs` diff retains the original activation
body, quota eligibility and durable no-replay behavior. Its differences add the
native route policy and a production-client redirect/retry regression. Eight
focused runtime tests pass; two opt-in native tests are not run in this batch.
No real-account activation, preference change or automation enablement occurs.
See [consumer reconciliation](core-consumer-reconciliation.md).

Weekly quota activation predates this upstream scan. Its existing preference is
separate from the paused upstream-maintainer automation. The scan's core change
is preserving native routing restrictions if this product feature is retained.
Do not classify the whole quota feature as a newly adopted upstream capability.

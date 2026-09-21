# Automatic weekly quota activation

General settings contains one switch, **Auto-activate weekly quota**, on by default. Existing installations also receive the enabled default when migration 30
is first applied. A manually disabled preference stays disabled across restarts.
The existing daemon observer checks every 15 seconds. A future weekly reset means a
countdown exists and no request is needed. An expired reset permits one request.
Percentages do not decide activation. Missing quota, disabled accounts, missing
credentials, and a currently exhausted five-hour window block the request.

The request uses the existing Rust HTTP client and AccountService credential lock.
It calls the ChatGPT Codex Responses endpoint with `gpt-5.6-sol`, low reasoning,
one short message, no tools, `store: false`, and streaming enabled. It does not start
a process, create a Codex home, or create a Decodex conversation. The credential owner
handles token refresh. This feature does not promise zero provider-side retention.

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

Installed CLI inspected: `0.155.0-alpha.9.2`. Its generated
`v2/GetAccountRateLimitsResponse.json` declares integer `usedPercent` and nullable
`resetsAt`; it does not provide a precise unused-cycle flag. The feature does not
depend on that binary at runtime. Upstream main is a wire reference, not evidence
that every account accepts the selected model or that quota reset behavior is fixed.

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

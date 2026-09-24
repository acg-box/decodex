# Activate a weekly window that has not started

## Observed failure

The local activation preference was enabled. Repeated read-only database samples
showed a weekly reset that moved forward with observation time. The remaining
interval stayed close to seven days. Its activation record remained idle.
The old trigger required an expired timestamp, so this window could never qualify.

## Change

Keep a durable observation anchor. If samples at least 30 seconds apart both put
the reset approximately seven days ahead, and the reset advances, reserve one
activation request. Allow 15 seconds for timestamp rounding and request latency.
Do not use rounded usage percentages to identify this state.

Keep the reservation while the reset continues to move. A real countdown can
establish the next expiry. Preserve the preference, account revision check,
other-window quota checks, rejection backoff, and suppression after an ambiguous
send. Migration 31 adds the observation time without deleting existing attempts.
Reset Card refreshes use the same observation path as natural resets.

The request still uses the existing non-persistent Responses adapter. This change
does not create a conversation or change the request payload.

## Upstream reference

Reviewed official `openai/codex` main at
`53446f90a56692dede3c8f413e8d486a6adb77b5`:
`codex-rs/codex-backend-openapi-models/src/models/rate_limit_window_snapshot.rs`
defines `reset_at`, `reset_after_seconds`, and `limit_window_seconds`.
`codex-rs/backend-client/src/types.rs` imports that provider model.
The observed floating timestamp is local runtime evidence, not a claim that
upstream specifies this activation policy.

Generated schemas from installed `codex-cli 0.155.0-alpha.16.3` expose
`RateLimitWindow.resetsAt`. This patch uses the existing direct API adapter;
it does not require a new app-server capability.

## Regression coverage

- A floating reset activates once, including across a restart.
- A fixed future reset does not activate.
- A completed or ambiguous request stays suppressed while the timestamp moves.
- A rejected request retains its backoff while the timestamp moves.
- Existing idle records can activate after upgrade; reserved records cannot replay.
- A real countdown permits activation at the next expiry.

Source tests do not prove installation or live provider activation.

## Local acceptance

- The database suite passed: 76 tests. Strict database Clippy and the isolated
  database gate passed with schema 31.
- The signed application build and bundle tests passed. The bundle test rejected
  its intentional ABI mismatch fixture as expected.
- Installed `/Applications/Decodex.app` and launched its bundled service.
- The previously idle target produced a completed activation receipt. Its reset
  stopped moving at `1790838600000000` microseconds. Remaining time decreased from
  604787 to 604771 seconds while displayed usage still rounded to zero.
- The conversation count remained 40 before and after activation.
- The architecture suite has an existing failure: its quickstart assertion
  requires the words `bundled SQLite`, which are absent from the unchanged
  generated OpenWiki page. This patch does not change that page or assertion.
- After restarting the installed app, the attempt timestamp and completed receipt
  stayed unchanged. The reset stayed fixed and remaining time reached 604745
  seconds. The conversation count stayed 40.

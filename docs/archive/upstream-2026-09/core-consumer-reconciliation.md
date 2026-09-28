> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Native configuration and route consumers

Close two inherited source-file reviews after complete preserved/current diff
comparison. No production source, schema, protocol or preference changes occur.

## Chief CLI

`apps/decodex-cli/src/chief.rs` keeps explicit model, turn, source and idempotency
identity checks. The extracted wire-identity helper has the same bounds. Omitted
reasoning effort now inherits native configuration; explicit effort still maps to
the same value. Keep the added inheritance test and the updated explicit-value
assertion. All eight Chief CLI tests pass.

The high-level request client assembles pages before returning. If an incomplete
page reaches the command, the CLI returns a nonzero error and does not expose it
as a complete request. Successful and unavailable request behavior is retained.
Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582` declares the optional
model and reasoning fields in `app-server-protocol/src/protocol/v2/turn.rs`.

## Quota activation route

`account_api/activation.rs` retains the quota decision, durable reservation, exact
account revision, bounded SSE completion and single model request. It now reads
native routing through `account_launch/activation_policy.rs` under the same account
credential lock. The longer credential window covers discovery plus the request;
it does not extend the request's 60-second timeout. Missing native attestation
skips activation without suppressing health observations. Failed discovery sends
no model request and retains rejection backoff.

The production HTTP client rejects redirect/retry behavior. The new regression
checks that 307 and 503 responses create no second connection. All eight focused
activation and policy tests pass; two opt-in native tests remain ignored in this
run. No real account or provider is used.

Update [quota activation](quota-activation.md) to remove stale claims that this
path starts no native process or has no native runtime dependency. The existing
weekly activation feature predates this scan. Native route correctness is core
for retaining that consumer, not a newly enabled product feature.

## Evidence and remaining work

- `/tmp/decodex-core-consumer-cli.log`: eight tests pass.
- `/tmp/decodex-core-consumer-activation.log`: eight pass, two ignored.
- Fixed upstream workspace-routing processor and optional-turn fields were read.

Close only the CLI Chief and runtime activation file rows. Shared protocol,
bootstrap, core exports, other consumers and signed desktop acceptance keep their
own dispositions. No installed-native, real-account or full product acceptance is
claimed by this documentation batch. Automations remain paused.

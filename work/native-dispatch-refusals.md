# Native Chief dispatch refusals

Classification: core compatibility for existing Chief input and capacity recovery.
This change does not implement provider policy locally or authorize another send.

## Native evidence

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
`app-server/src/error_code.rs` defines the draining invalid-request response.
`request_processors/turn_processor.rs` returns it for native NotSubmitted admission.
`config_manager.rs` defines the changed managed-provider requirement, and
`request_processors/config_errors.rs` wraps it as an invalid configuration request.

The shared adapter classifier accepts only error code -32600 and the two exact
native messages. A different code, prefix or suffix is not positive refusal
proof. Private error data is not used. The request client owns reply correlation;
a missing response remains unknown.

## Current owners

Chief uses the existing InputNotSent result and durable refusal transaction only
when no earlier external-context injection occurred. Original user input remains
readable for manual action. Async answers keep the existing exact-event release
path. An injected update or lost response retains its uncertain dispatch fence.
No automatic resend, provider fallback or credential mutation is added.

Migration 38 updates the existing capacity-transition trigger. A claimed retry
can be cancelled only with a resolved refusal receipt for its exact work and retry
identity. It accepts the two native reasons and the three existing local refusal
reasons: stale settings, oversized request and full local queue. Earlier applied
migration files and checksums are preserved.

A refused continuation restores the original failed turn's delivery identity.
The cancellation cannot return to pending. The input did reach the original turn;
it must not be relabeled as if that original request was never sent.

## Validation and limits

Targeted native-message classification and Chief refusal tests cover direct input,
prior injection, lost response, input retention after reopen, one attempted send,
no automatic retry and original capacity-delivery preservation. Database tests
cover all five refusal causes, exact resolved receipt identity, wrong work/retry,
unknown reasons, upgrade history and cancellation without replay.

An isolated installed Codex 0.155.0-alpha.16.4 fixture changed synthetic enterprise
requirements through a loopback configuration backend. The retained thread was
refused without another inference request. After restoring requirements, a manual
request succeeded on the same thread. It observed three bundle reads and two total
inference requests. This validates the native policy contract, not a production
policy change or signed desktop recovery interaction.

The ordinary conversation adaptation is documented separately in
[Ordinary native non-submission](ordinary-native-non-submission.md). This Chief
batch alone did not close that audit item. Large approvals and optional product
scopes remain separate. Scheduled automation stays paused.

Local validation: 123 database tests passed; the exact-message adapter test, two
Chief drain/ambiguity tests and the refused-capacity continuation test passed.
The earlier refusal regression selection passed eight tests (one opt-in skip).
Strict adapter/database/runtime Clippy passed across all targets and features.

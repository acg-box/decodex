# Guardian approval frame preflight

An observed denial can fit the native notification limit while its converted
approval request exceeds the request limit. For example, a write-stdin action
with a non-ASCII working directory expands when the core action requires a path
URI. The previous flow reserved a durable submission before the transport
rejected the oversized frame. The service then reported an unknown outcome.

Use `AppServerClient::preflight_request` in `core_denial_event` before returning
a submittable event. This existing owner measures the full JSON-RPC envelope
with the longest request ID. The runtime already converts before thread resume,
latest-turn checks and the durable claim. Oversized conversions now fail there.
Keep the complete observed review, the native 8 MiB bound, and exact action data.
Do not retry or execute the denied action.

At fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`ThreadApproveGuardianDeniedActionParams` contains `threadId` and the serialized
core assessment event. The installed Codex `0.158.0-alpha.2` schema exposes the
same request. The adapter preserves this contract and uses the existing local
transport limit.

## Validation

All 9 adapter and 12 runtime Guardian tests pass. Strict lint for both crates
passes with all features and targets.

Both regressions failed before the fix. The inherited adapter case retains a
review that cannot fit the complete approval frame. The runtime case delivers a
valid notification below the native limit whose path conversion exceeds that
limit. It requires a known rejection, no outgoing RPC, no approval reservation,
and unchanged saved evidence. Existing successful approval, large review,
conflict, supersession and lost-reply cases remain in the focused Guardian suite.

The complete inherited `guardian/approval.rs` differs only in use of the shared
preflight owner and whitespace. This closes that file's review. The broad runtime
test file and final native/desktop acceptance remain separate.

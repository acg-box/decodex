# Live misalignment review guard

Restore transport-local continuation evidence from the preserved implementation.
Only live error or terminal notifications can populate this bounded observation
cache. Persisted errors and history reads cannot create a live review identity.
The existing guarded request writer checks the captured identity immediately
before it writes, including notifications queued ahead of the request.

New input, a new turn, history changes, thread closure and disconnection invalidate
old evidence. A detail-free terminal notification for the same failed turn retains
the detailed error already observed. An unrelated settings-operation error does
not erase that review. Re-observing identical findings after invalidation creates
a different identity; another connection cannot reuse the old identity.

The fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582` publishes live
misalignment details through terminal error handling in
`codex-rs/app-server/src/bespoke_event_handling.rs`. This adapter observes that
native contract; it does not decide whether a denied action can execute.

## Validation and scope

All 217 adapter tests pass; eight existing external tests remain ignored.
The restored cases cover queued invalidation before a write, no output on a stale
request, a detail-free terminal event, unrelated errors, disconnect, repeated
identical findings and cross-connection identity. Strict adapter lint passes with
all features and targets. Existing model, plugin, permission and history guards
retain their current owners and behavior.

The complete `app_server_client/live_reviews.rs` matches the verified preserved
snapshot. The shared transport files remain open in the inherited review. This
batch restores the adapter foundation only: runtime publication, explicit user
confirmation and continuation still need to consume the live review identity.
It does not establish end-to-end continuation or signed desktop acceptance.

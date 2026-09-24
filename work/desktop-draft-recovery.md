# Desktop draft recovery

## Delivered boundary

Editable Chief text, attachments, task references, and asynchronous question editors belong to the exact service profile. Disconnecting keeps the editors. Returning to a profile restores its in-memory inputs. A late command result cannot clear a different profile or newer text.

Next-message model, effort, and tier choices belong to the conversation and carry a revision. SendConfigured contains only explicit changes. Acceptance clears only the captured revision. Steering keeps next-message choices. Full legacy execution objects remain readable. Protocol version: 2.47.

The local store and document types are groundwork for cold recovery. The store uses private files, revision comparison, a writer lock, atomic replacement, and a bounded checked payload. Recovery documents preserve service/thread ownership and uncertain command identities. These APIs are tested, but the desktop does not yet save or load these documents in production.

## Upstream evidence

Reference: openai/codex 595cc91e8cbb1c2ca822d0311dcf12709410c582, codex-rs/app-server-protocol/src/protocol/v2/turn.rs. TurnStartParams has optional model and effort overrides. An omitted service tier differs from an explicit standard tier. This batch preserves that distinction in queued message options; it does not establish complete native settings recovery.

## Verification

- Core storage suite: 88 passed, including corrupt/oversized data, stale writers, concurrent writers, private paths, and reopen.
- Protocol library: 106 passed, including draft recovery and partial/legacy message settings.
- Runtime library: 481 passed, 8 ignored. Partial settings preserve existing values; explicit standard clears the requested tier.
- Chief desktop tests: 117 passed, 1 ignored. Input ownership, question restoration, explicit settings, and late acceptance are covered.
- Repository strict Clippy: core, protocol, runtime, and desktop passed.

These are source and test results. They do not prove signed desktop quit/relaunch acceptance.

## Next boundary

Connect document capture and background publication to the desktop. Persist the exact command identity before RPC dispatch. Keep both copies when another window writes. Retain the original submitted input when a failure arrives after later edits. Add explicit recovery controls and flush on native quit. Then verify cold reopen and signed desktop behavior before claiming durable recovery.

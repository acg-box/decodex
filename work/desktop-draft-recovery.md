# Desktop draft recovery

## Delivered boundary

Editable Chief text, attachments, task references, and asynchronous question editors belong to the exact service profile. Disconnecting keeps the editors. Returning to a profile restores its in-memory inputs. A late command result cannot clear a different profile or newer text.

Next-message model, effort, and tier choices belong to the conversation and carry a revision. SendConfigured contains only explicit changes. Acceptance clears only the captured revision. Steering keeps next-message choices. Full legacy execution objects remain readable. Protocol version: 2.47.

The desktop captures profile-owned text, files, task references, explicit conversation settings, and question editors in a private revisioned store. It waits for publication of the exact command identity and original input before RPC dispatch. The original in-flight copy remains available if later edits are saved before a reply. Acceptance removes that exact copy; a known failure can retain it beside newer input. Unknown delivery blocks automatic replay after reopening.

A concurrent writer produces a visible conflict. The user can keep both copies, restore a copy for its exact service, export it, or confirm removal of a copy with known delivery. Uncertain copies cannot be removed. Empty edits are saved as edits. Native quit waits for publication, rechecks the latest input, and cancels termination if publication fails. The AppKit bridge adds the missing termination callback without replacing GPUI lifecycle methods.

## Upstream evidence

Reference: openai/codex 595cc91e8cbb1c2ca822d0311dcf12709410c582, codex-rs/app-server-protocol/src/protocol/v2/turn.rs. TurnStartParams has optional model and effort overrides. An omitted service tier differs from an explicit standard tier. This batch preserves that distinction in queued message options; it does not establish complete native settings recovery.

## Verification

- Desktop binary tests: 286 passed, 5 ignored. This includes cold reopen with an in-flight original and later edit, accepted-copy cleanup, profile changes before dispatch, busy/conflicting writers, restored question inputs, export, recovery button clicks, and quit flush/recheck.
- Repository strict Clippy passed for all desktop targets and features.
- The preceding input-ownership batch passed 88 core, 106 protocol, 481 runtime, and 117 Chief desktop tests.

These are source and test results. The AppKit callback test uses an isolated delegate class. It is not signed desktop quit/relaunch acceptance.

## Remaining acceptance and integration

Build and test a fresh signed desktop with isolated fixture storage. Verify the real menu, Dock, and keyboard quit paths, cancelled quit on conflict, relaunch recovery, and export. Do not run capture fixtures against the user's draft store.

Exact native steering receipt reconciliation is a separate pending integration. Restored uncertain commands remain blocked and are never replayed. Complete the audit of new-task configuration drafts and task-setting presentation; this batch persists explicit conversation settings, not all setup defaults.

> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Native-admitted turn recovery

## Problem and scope

The preserved Chief path for native-admitted turns was absent. A real installed
Codex goal started a new turn, but Decodex kept the bound task idle with no active
turn. The restored native lifecycle test reproduced that mismatch before the fix.

This repairs an existing conversation consumer. Codex owns goal state, scheduling,
limits and turn admission. Decodex records the observed turn and its history. The
optional goal display is a separate removal choice. This change does not enable
goals, create a production goal, resume paused goals or enable maintenance.

## Retained ownership

Handle `turn/started` only for the exact bound work and current ready process.
The existing database transaction records a resolved observation and moves an
idle task to the observed running turn. It does not mark pending local input as
delivered. Reject retired owners, competing turns, uncertain local dispatch and
previously recorded native turns. A newly observed native turn cancels a pending
local capacity retry through the shared cancellation owner.

On reconnect, inspect owned idle tasks and reconcile their latest native turn
under the current history guard. Do not create local input or replay a turn.
For an unloaded thread, read the goal through the existing typed native getter.
Only an exact active goal permits hydration. Resume uses the canonical native
settings parameters and observation owner; startup defaults stay out of the
request. Codex can then continue that already-active goal under its own limits.

A native-only turn has no locally acknowledged execution selection. Do not create
a local capacity retry for it. Local input submitted after an automatic turn, or
while a native continuation is already active, retains its own delivery receipt.

## Provenance and adaptation

The database observer, its two tests and the native lifecycle test already exist
with identical bytes in pre-scan base `2ffa385c3b49efe6a4109de0fd7353fb64abd2c5`
and preserved PR1378 commit `4e370c07464ea3528ed1334fd6ce75fcc5ca595a`. These four
paths are outside the 360-row delta. Restore the three database files exactly.
The lifecycle test uses a shared isolated Responses fixture extracted from the
preserved permission fixture; it does not require unrelated permission controls.

The two delta files are `chief/native_turns.rs` and
`chief/tests/native_goal_recovery.rs`. Adapt them to the existing native resume,
settings-publication and typed goal owners. Restore missed-event/revert coverage
and the original repeated-recovery assertion: only native history reads and no
duplicate result. The capacity fixture now exposes only turns already started;
its previous future-turn list falsely represented a newer native turn during
recovery. Its three-attempt, model, context and no-input-replay assertions remain.

At fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`ext/goal/src/extension.rs` restores goal runtime on thread resume.
`ext/goal/src/runtime.rs` checks active status and native admission under the goal
state permit. `ext/goal/tests/goal_extension_backend.rs` covers active-goal resume
accounting. Keep these responsibilities native.

## Verification boundaries

The installed 0.158.0-alpha.2 fixtures use private native homes, temporary stores
and loopback Responses services. They cover autonomous turns, separate pending
input, missed start/completion recovery, duplicate start rejection, input during
ongoing continuation, cold active-goal restoration and native budget stopping.
Native children are shut down before cleanup. These fixtures use no live account
or cloud model and do not establish signed desktop acceptance.

Database tests cover generation rotation, ready ownership, replay after reopen,
capacity supersession and preservation of uncertain local input. Runtime checks
cover exact goal status/identity, reverted history and no local retry for a
native-only capacity failure. Shared recovery and desktop acceptance remain open.

---
type: Reference
title: "Agent coordination and native conversations"
description: "Local work ownership, native conversation dispatch, explicit branches and recovery."
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.7.0
    at: 2026-10-03T17:31:35.485Z
sources:
  - id: openwiki-source-787f8ad27b8519fbed2bd039
    resource: repo://crates/decodex-codex/src/app_server_client/thread_fork.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
  - id: openwiki-source-52d3ef4824079f66e9063566
    resource: repo://crates/decodex-runtime/src/agent/native_subagents.rs
  - id: openwiki-source-2a0e86d8a9789b05a13deccc
    resource: repo://crates/decodex-runtime/src/agent/prompt_edit.rs
  - id: openwiki-source-58c0167d31c3e40dc89591ab
    resource: repo://database/src/agent_fork.rs
  - id: openwiki-source-51332b5dcd4b194b62fec905
    resource: repo://database/src/agent_guardian.rs
  - id: openwiki-source-e1fc2e0e81623c7dd7ff5417
    resource: repo://database/src/agent_process.rs
generated: { by: "codex", at: "2026-09-29T13:52:19.644Z" }
---

# Agent coordination and native conversations

Agent is the user's primary agent for general-purpose work: discussion, research, writing, planning, analysis, and software. It may handle simple work itself or organize workers and subordinate Agents. The user does not have to operate a delegation protocol.

## Owners and flow

The GPUI composer sends typed commands to the service-owned `AgentHost`. The host persists accepted input under a stable command identity. `AgentCoordinator` binds work items to native Codex threads, dispatches exact turns, observes results, and records decisions in SQLite. Codex owns native thread history and provider execution; Decodex owns work relationships and presentation.

`agent/instructions.md` defines the behavioral policy. A worker finishing is evidence for the Agent to assess, not automatic parent-goal completion. Dependencies require accepted, resolved work. Review starts after an artifact exists; a review does not grant execution permission. Continuation can invalidate previous acceptance.

## Persistence and recovery

Work, inbox events, dependencies, dispositions, process bindings, usage and pending requests survive service restart. Stable source IDs deduplicate external evidence. Unknown dispatch is not proof that no work started: reconcile exact thread/turn history instead of replaying.

Existing Agent threads retain their identity and native capabilities; resumption does not fork an older thread merely to attach a new tool set. Prior-boot process death requires positive kernel evidence. Optional metadata failures do not close the shared transport.

When another client owns the conversation, sending is unavailable. The host rejects input when this state is known, without queuing a message. A race can leave previously accepted but unsent input; that input is retained as a user-decision record, not automatically resent when ownership becomes available. Availability checks do not create turns. Archive restoration is a separate, explicit desired-state operation.

## Inbox ownership and native queues

Keep the Decodex inbox as the dispatch owner for Decodex work. A native user-message queue is not a replacement for the work scheduler. Do not write the same input to both queues.

Before dispatch, Decodex checks dependencies, work state and native thread ownership. It selects the execution settings, records the dispatch intent, then calls `turn/start`. The inbox also contains async question answers and external work evidence. These records have different input roles and recovery rules. A queued user message cannot represent all of them.

The native `thread/queue` API stores user input for a native thread. Its start request does not carry Decodex dependencies, account selection or the local dispatch receipt. The upstream queue deletes an item after native admission reports `Started`. This deletion does not commit the Decodex receipt. See the [native queue implementation at the reviewed revision](https://github.com/openai/codex/blob/a397079287e6638b39dda329835350d93222681f/codex-rs/ext/queue/src/service.rs#L365-L445). This is source evidence, not a claim that Decodex has adopted or qualified that queue.

The current decision is to retain the existing inbox and direct native turn admission. There is no data migration and no second native enqueue operation. Native conversations and their existing queues remain native-owned; this decision does not remove their data.

A future change must define a cutover before moving pending messages. Stop local dispatch for the exact source, reconcile every uncertain send against native history, and map each eligible user input to one stable native submission ID. Keep async answers and application evidence under their existing owners. Delete a local pending record only after the exact native submission or admitted turn is observed. On rollback, reconcile native admission before restoring local dispatch. Do not replay uncertain input or treat queue deletion as proof of delivery. Such a change must also preserve dependency checks and per-message execution settings; the current native queue API is not a drop-in replacement.

## Native child requests and approvals

Native child requests resolve through verified thread-spawn ancestry to the owning local work item, with cycle and depth bounds. A fork alone does not establish child authority. Child approvals retain the child's native identity; child ownership does not grant local manager-tool authority.

Guardian observations are durable review evidence. They neither authorize execution nor wake work by themselves. Approval UI must answer the exact pending request and preserve uncertainty when acknowledgment is unavailable.

## Explicit conversation branches

A user can create a native branch before an input or after its completed turn. This is a separate action from a same-thread history edit. The source conversation keeps its history. Codex owns the new thread and copied context; `deferGoalContinuation` prevents the copied Goal from continuing as part of fork creation.

Decodex reserves a new local work item before the native request. It saves the acknowledged native ID before a separate history read. A lost creation reply stays uncertain and cannot authorize another fork. Recovery of a known ID reads its exact source and prefix without starting a model turn.

The branch is a sibling of its source, or a child of personal Main when Main is the source. It keeps the source role, workspace and recorded tool version. It does not copy pending inbox work or dependency edges. An acknowledged fork has an explicit ownership record that permits it to use the source process. Other subordinate managers do not gain that authority. A branch does not create another personal Main or silently upgrade native tools.

Before-input branches use the existing canonical input and draft handback owner. After-turn branches do not prefill the selected input. See [conversation recovery](../workflows/conversations-and-recovery.md) for the user actions and uncertain-result behavior.

## Verification and navigation

Run focused tests in `agent/tests.rs`, `agent/tests/native_subagents.rs`, `agent/tests/archive.rs` and `agent_host.rs`. Distinguish fixture tests from live provider tests and retained historical receipts.

See [Desktop workspace](desktop-workspace.md), [Subscription voice](../integrations/subscription-voice.md), and [Runtime architecture](runtime-architecture.md).

## Current workflow documentation

Read [conversation recovery](../workflows/conversations-and-recovery.md), [settings](../workflows/models-and-settings.md) and [approvals](../workflows/approvals-and-native-ownership.md) for the consolidated contracts. Native execution remains in Codex; completion reports remain evidence for the Agent to assess.

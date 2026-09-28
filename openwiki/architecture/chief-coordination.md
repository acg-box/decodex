---
type: Reference
title: "Agent coordination and native conversations"
description: "Chief coordination and native conversations"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T02:09:19.063Z
sources:
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
  - id: openwiki-source-52d3ef4824079f66e9063566
    resource: repo://crates/decodex-runtime/src/agent/native_subagents.rs
  - id: openwiki-source-51332b5dcd4b194b62fec905
    resource: repo://database/src/agent_guardian.rs
generated: { by: "codex", at: "2026-09-28T02:09:19.063Z" }
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

## Native child requests and approvals

Native child requests resolve through verified thread-spawn ancestry to the owning local work item, with cycle and depth bounds. A fork alone does not establish child authority. Child approvals retain the child's native identity; child ownership does not grant local manager-tool authority.

Guardian observations are durable review evidence. They neither authorize execution nor wake work by themselves. Approval UI must answer the exact pending request and preserve uncertainty when acknowledgment is unavailable.

## Verification and navigation

Run focused tests in `agent/tests.rs`, `agent/tests/native_subagents.rs`, `agent/tests/archive.rs` and `agent_host.rs`. Distinguish fixture tests from live provider tests and retained historical receipts.

See [Desktop workspace](desktop-workspace.md), [Subscription voice](../integrations/subscription-voice.md), and [Runtime architecture](runtime-architecture.md).

## Current workflow documentation

Read [conversation recovery](../workflows/conversations-and-recovery.md), [settings](../workflows/models-and-settings.md) and [approvals](../workflows/approvals-and-native-ownership.md) for the consolidated contracts. Native execution remains in Codex; completion reports remain evidence for the Agent to assess.

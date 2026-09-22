---
type: Reference
title: "Chief coordination and native conversations"
description: "Chief coordination and native conversations"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-e63babef303ce03a74424170
    resource: repo://crates/decodex-runtime/src/chief_host.rs
  - id: openwiki-source-6582bce2d15b511be3bc71bc
    resource: repo://crates/decodex-runtime/src/chief.rs
  - id: openwiki-source-e525513ca8a9d5ce4fbb5336
    resource: repo://crates/decodex-runtime/src/chief/instructions.md
  - id: openwiki-source-4536c93a216e276ffcbc8c87
    resource: repo://crates/decodex-runtime/src/chief/native_subagents.rs
  - id: openwiki-source-27b1e25fd29a5d33d90e1cfe
    resource: repo://database/src/chief_guardian.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Chief coordination and native conversations

Chief is the user's primary agent for general-purpose work: discussion, research, writing, planning, analysis, and software. It may handle simple work itself or organize workers and subordinate Chiefs. The user does not have to operate a delegation protocol.

## Owners and flow

The GPUI composer sends typed commands to the service-owned `ChiefHost`. The host persists accepted input under a stable command identity. `ChiefCoordinator` binds work items to native Codex threads, dispatches exact turns, observes results, and records decisions in SQLite. Codex owns native thread history and provider execution; Decodex owns work relationships and presentation.

`chief/instructions.md` defines the behavioral policy. A worker finishing is evidence for the Chief to assess, not automatic parent-goal completion. Dependencies require accepted, resolved work. Review starts after an artifact exists; a review does not grant execution permission. Continuation can invalidate previous acceptance.

## Persistence and recovery

Work, inbox events, dependencies, dispositions, process bindings, usage and pending requests survive service restart. Stable source IDs deduplicate external evidence. Unknown dispatch is not proof that no work started: reconcile exact thread/turn history instead of replaying.

Existing Chief threads retain their identity and native capabilities; resumption does not fork an older thread merely to attach a new tool set. Prior-boot process death requires positive kernel evidence. Optional metadata failures do not close the shared transport.

When another client owns the conversation, sending is unavailable. The host rejects input when this state is known, without queuing a message. A race can leave previously accepted but unsent input; that input is retained as a user-decision record, not automatically resent when ownership becomes available. Availability checks do not create turns. Archive restoration is a separate, explicit desired-state operation.

## Native child requests and approvals

Native child requests resolve through verified thread-spawn ancestry to the owning local work item, with cycle and depth bounds. A fork alone does not establish child authority. Child approvals retain the child's native identity; child ownership does not grant local manager-tool authority.

Guardian observations are durable review evidence. They neither authorize execution nor wake work by themselves. Approval UI must answer the exact pending request and preserve uncertainty when acknowledgment is unavailable.

## Verification and navigation

Run focused tests in `chief/tests.rs`, `chief/tests/native_subagents.rs`, `chief/tests/archive.rs` and `chief_host.rs`. Distinguish fixture tests from live provider tests and retained historical receipts.

See [Desktop workspace](desktop-workspace.md), [Subscription voice](../integrations/subscription-voice.md), and [Runtime architecture](runtime-architecture.md).

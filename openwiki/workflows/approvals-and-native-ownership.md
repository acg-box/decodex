---
type: Reference
tags: [decodex, architecture]
title: "Approvals and native request ownership"
description: "Complete request evidence, native child ownership, Guardian observations and explicit decisions."
sources:
  - id: openwiki-source-52d3ef4824079f66e9063566
    resource: repo://crates/decodex-runtime/src/agent/native_subagents.rs
  - id: openwiki-source-51332b5dcd4b194b62fec905
    resource: repo://database/src/agent_guardian.rs
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.6.0
    at: 2026-09-29T06:24:17.023Z
---


# Approvals and native request ownership

## Authority

Codex enforces native permissions, tool policy and provider execution. Decodex presents complete evidence and sends an explicit decision for the current request. A rendered panel, saved setting or Guardian observation is not execution permission.

Native child requests resolve through verified thread-spawn ancestry to the local owner. The resolver bounds traversal and rejects cycles or mismatched parents. A fork alone does not establish child authority, and inspecting a child does not grant it local manager-tool authority.

## Complete evidence before a decision

Large approvals use bounded pages and durable payload records rather than a clipped preview as decision input. The selected request, turn, connection, account and process must still match the reviewed evidence. Read-only history may survive resolution; it cannot authorize a new response to an old request.

Guardian evidence is durable and can contain conflicts. It does not wake work or mark a task complete. Explicit continuation for a misalignment finding must match the current finding and checkpoint. A stale callback cannot dismiss a newer block.

## Saved configuration versus request response

Task permission/reviewer settings and App connection settings are configuration controls. Saving one does not implicitly approve a pending tool call. Shared Hook/App configuration receipts distinguish saved, overridden, rejected and unknown outcomes. Uncertainty triggers readback, not automatic replay.

## Boundaries and tests

Verify child browser-auth and user-input handoff against the installed binary. Successful ancestry or permission tests do not establish complete child interaction. General login policy and native execution admission have separate owners.

Start with `agent/native_subagents.rs`, `agent/guardian.rs`, `agent_guardian.rs`, the large-request payload owner and the corresponding native tests. See [tools and integrations](../integrations/tools-plugins-and-apps.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).

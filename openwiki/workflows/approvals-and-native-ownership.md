---
type: Reference
tags: [decodex, architecture]
title: "Approvals and native request ownership"
description: "Complete request evidence, native child ownership, Guardian observations and explicit decisions."
sources:
  - id: openwiki-source-c6ac7d6b2fdeb458faab2c3a
    resource: repo://apps/decodex-gpui/src/agent_permissions.rs
  - id: openwiki-source-6a1c55b61be8ac251a493046
    resource: repo://apps/decodex-gpui/src/agent_portfolio.rs
  - id: openwiki-source-40db4dc21bafde1429326458
    resource: repo://crates/decodex-core/src/permissions.rs
  - id: openwiki-source-eccf08613126ff810078b520
    resource: repo://crates/decodex-protocol/src/agent.rs
  - id: openwiki-source-5bc08ad67fcdb104dfcde7bf
    resource: repo://crates/decodex-runtime/src/agent_permissions.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-a4a2658e06fa2cf48ff4589e
    resource: repo://crates/decodex-runtime/src/agent/global_guardian.rs
  - id: openwiki-source-a7886792ad3e47b8e1afb7fa
    resource: repo://crates/decodex-runtime/src/agent/guardian_details.rs
  - id: openwiki-source-1bebc823fa8b6426acb4d5e2
    resource: repo://crates/decodex-runtime/src/agent/native_settings.rs
  - id: openwiki-source-52d3ef4824079f66e9063566
    resource: repo://crates/decodex-runtime/src/agent/native_subagents.rs
  - id: openwiki-source-51332b5dcd4b194b62fec905
    resource: repo://database/src/agent_guardian.rs
  - id: openwiki-source-d1279ad243eb550afec4d0e6
    resource: repo://database/src/guardian_settings.rs
generated: { by: "codex", at: "2026-10-28T08:28:06.287Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-30T17:36:00.319Z
---


# Approvals and native request ownership

## Authority

Codex enforces native permissions, tool policy and provider execution. Decodex presents complete evidence and sends an explicit decision for the current request. A rendered panel, saved setting or Guardian observation is not execution permission.

Native child requests resolve through verified thread-spawn ancestry to the local owner. The resolver bounds traversal and rejects cycles or mismatched parents. A fork alone does not establish child authority, and inspecting a child does not grant it local manager-tool authority.

## Complete evidence before a decision

Large approvals use bounded pages and durable payload records rather than a clipped preview as decision input. The selected request, turn, connection, account and process must still match the reviewed evidence. Read-only history may survive resolution; it cannot authorize a new response to an old request.

Guardian evidence is durable and can contain conflicts. It does not wake work or mark a task complete. Explicit continuation for a misalignment finding must match the current finding and checkpoint. A stale callback cannot dismiss a newer block.

## Conversation permissions

Settings > Agents defines defaults for new conversations. The composer shield controls the selected conversation. Both use the same native presets: Ask for approval, Approve for me, Full access and Read-only. Speed stays in the model menu. Changing defaults does not rewrite existing conversations.

Ask for approval sends eligible native requests to the human. An Agent cannot impersonate the answer. Approve for me routes eligible requests to the native reviewer, which can allow or deny the action. Full access selects the native unrestricted profile; Read-only selects the inspection profile. Native filesystem, network, tool and managed policy remain authoritative.

The service reviews the exact thread, current permission facts and allowed profiles before reserving one native selection. A queued update is not proof of application; native readback supplies the effective result. Unconfirmed writes are not retried automatically. Managed restrictions remain visible.

Changes apply through native thread settings. Active turns and pending approvals keep native ownership. Native children inherit their parent's settings; independent conversations use their own settings. Several Works in one conversation do not gain separate sandboxes. Independent Collaboration does not transfer the other Agent's execution authority.

Global Guardian controls only optional Review Details. It does not overwrite conversation permissions. Saved permission, Hook and App connection settings remain configuration controls with separate observed outcomes. See [models and settings](models-and-settings.md).

## Optional Review Details

The selected runtime exposes private reviewer records without adding them to parent canonical history. The observer verifies the private Guardian source, exact reviewer thread/turn and owned parent ancestry. It retains only native assistant output and item-free terminal metadata. Raw reasoning frames are excluded.

The runtime does not expose an exact action Review ID for those private records. The UI states Action association unavailable and never associates the latest action by timing. A record does not grant permission, complete Work, clear the parent's active turn, start a turn or replay an action. A private interruption remains private lifecycle evidence.

Retention is bounded to 128 records at 64 KiB each. The snapshot includes the newest 32 short previews; the exact selected record is read separately. Raw-event configuration takes effect when a native source loads. The selected protocol cannot enable a raw subscription through resume. After a completed review, the service can instead recover exposed output from native history: it checks up to 32 same-directory private candidates and five turns each, verifies the parent, and retains no raw reasoning. Reconnect gaps remain explicit. Turning observation off stops new records without deleting saved review history or deciding approvals. Review Details is diagnostic output, not a complete private reasoning transcript.

## Boundaries and tests

The [Guardian contract](../decisions/project-agent-work.md#global-guardian-decision) keeps private observation separate from execution policy. Synthetic fixtures cover native permission readback and resume, private reviewer identity, parent-history isolation and opt-out. Earlier global-switch qualification remains historical evidence. Local tests cover bounds, stale receipts, protocol and UI controls. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md#project-agent-work-and-global-guardian) for actual counts and limits.

Child browser-auth and user-input handoff still need their own exact native interaction evidence; ancestry and permission tests do not establish all interactions. The later [source-built acceptance](../testing/upstream-acceptance-boundaries.md#source-built-real-provider-acceptance) exercises a real provider in one controlled scenario. It does not establish installed-app behavior or general model judgment.

Start with `agent/native_subagents.rs`, `agent/guardian.rs`, `agent/global_guardian.rs`, the large-request payload owner and their native tests. See [tools and integrations](../integrations/tools-plugins-and-apps.md).

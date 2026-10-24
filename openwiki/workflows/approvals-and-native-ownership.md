---
type: Reference
tags: [decodex, architecture]
title: "Approvals and native request ownership"
description: "Complete request evidence, native child ownership, Guardian observations and explicit decisions."
sources:
  - id: openwiki-source-6a1c55b61be8ac251a493046
    resource: repo://apps/decodex-gpui/src/agent_portfolio.rs
  - id: openwiki-source-f05abb1636e0c1ff279607a6
    resource: repo://crates/decodex-codex/src/app_server_client/guardian_settings.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-a4a2658e06fa2cf48ff4589e
    resource: repo://crates/decodex-runtime/src/agent/global_guardian.rs
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
  - id: openwiki-source-52d3ef4824079f66e9063566
    resource: repo://crates/decodex-runtime/src/agent/native_subagents.rs
  - id: openwiki-source-51332b5dcd4b194b62fec905
    resource: repo://database/src/agent_guardian.rs
  - id: openwiki-source-d1279ad243eb550afec4d0e6
    resource: repo://database/src/guardian_settings.rs
generated: { by: "codex", at: "2026-10-24T21:11:31.338Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-24T21:11:31.338Z
---


# Approvals and native request ownership

## Authority

Codex enforces native permissions, tool policy and provider execution. Decodex presents complete evidence and sends an explicit decision for the current request. A rendered panel, saved setting or Guardian observation is not execution permission.

Native child requests resolve through verified thread-spawn ancestry to the local owner. The resolver bounds traversal and rejects cycles or mismatched parents. A fork alone does not establish child authority, and inspecting a child does not grant it local manager-tool authority.

## Complete evidence before a decision

Large approvals use bounded pages and durable payload records rather than a clipped preview as decision input. The selected request, turn, connection, account and process must still match the reviewed evidence. Read-only history may survive resolution; it cannot authorize a new response to an old request.

Guardian evidence is durable and can contain conflicts. It does not wake work or mark a task complete. Explicit continuation for a misalignment finding must match the current finding and checkpoint. A stale callback cannot dismiss a newer block.

## Global Guardian

Guardian has one global Review preference: Ask Me or Automatic Review. It also has one optional global Review Details setting. The desktop has no editable per-Agent reviewer override. A retained legacy per-Agent write request is rejected explicitly. Native filesystem, network, tool, profile and managed policy remain separate authority.

Ask Me sends eligible native approval requests to the human. An Agent cannot impersonate the answer. Automatic Review routes eligible requests to the native reviewer, which can allow or deny the action. Full Access remains a distinct native permission choice; setting `approval_policy=never` alone does not broaden permissions. Independent Collaboration does not transfer the other Agent's execution authority.

The service records desired global settings and per-source application receipts separately. It writes a reviewer-only `thread/settings/update` for the exact loaded source. The acknowledgment records publication, not application. Actual native readback determines Applied, Pending, Overridden, Unsupported, Rejected or Unknown. Unknown publication is not automatically retried. Managed native restrictions remain visible.

Thread preference changes apply to subsequent turns. Active turns, pending human approvals and previously captured child policy keep their native owner. Changing Guardian does not answer an existing approval or restart the action. Native Subagents follow actual inherited native rules. Several Works in one conversation do not gain separate enforced permission sandboxes.

Saved permission, Hook and App connection settings remain configuration controls. Their receipts distinguish saved, overridden, rejected and unknown. Uncertain writes require source-bound readback. See [models and settings](models-and-settings.md).

## Optional Review Details

The selected runtime exposes private reviewer records without adding them to parent canonical history. The observer verifies the private Guardian source, exact reviewer thread/turn and owned parent ancestry. It retains only native assistant output and item-free terminal metadata. Raw reasoning frames are excluded.

The runtime does not expose an exact action Review ID for those private records. The UI states Action association unavailable and never associates the latest action by timing. A record does not grant permission, complete Work, clear the parent's active turn, start a turn or replay an action. A private interruption remains private lifecycle evidence.

Retention is bounded to 128 records at 64 KiB each. The snapshot includes the newest 32 short previews; the exact selected record is read separately. New observation settings require a native source load, and reconnect gaps remain explicit. Turning observation off stops new records without deleting saved review history or deciding approvals. Review Details is diagnostic output, not a complete private reasoning transcript.

## Boundaries and tests

The [global Guardian contract](../decisions/project-agent-work.md#global-guardian-decision) is implemented and qualified with the exact selected CLI and helpers in synthetic fixtures. Tests cover Project idempotency, Ask Me approval preservation during a global switch, native effective readback, unchanged sandbox and approval policy, private reviewer identity, parent-history isolation and opt-out. Local tests cover bounds, stale receipts, protocol and UI controls. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md#project-agent-work-and-global-guardian) for actual counts and limits.

Child browser-auth and user-input handoff still need their own exact native interaction evidence; ancestry and permission tests do not establish all interactions. This change does not establish real-account, installed-app or complete model judgment acceptance.

Start with `agent/native_subagents.rs`, `agent/guardian.rs`, `agent/global_guardian.rs`, the large-request payload owner and their native tests. See [tools and integrations](../integrations/tools-plugins-and-apps.md).

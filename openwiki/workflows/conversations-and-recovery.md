---
type: Reference
tags: [decodex, architecture]
title: "Conversation input, history and recovery"
description: "Input delivery, draft recovery, native history editing, recap and rendering boundaries."
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T02:19:36.307Z
sources:
  - id: openwiki-source-3362ac542db6e3212a621c47
    resource: repo://apps/decodex-gpui/src/agent_recap_automatic.rs
  - id: openwiki-source-2a0e86d8a9789b05a13deccc
    resource: repo://crates/decodex-runtime/src/agent/prompt_edit.rs
  - id: openwiki-source-c29a139ba78abf924bfd2cc2
    resource: repo://docs/archive/upstream-2026-09/signed-draft-acceptance.md
  - id: openwiki-source-8d4b61fd83ed007c18390abe
    resource: repo://docs/archive/upstream-2026-09/upstream-feature-decisions.md
generated: { by: "codex", at: "2026-09-28T02:19:36.307Z" }
---


# Conversation input, history and recovery

## Owners and normal input

The desktop composer stores editable client drafts. The service accepts a typed command with a stable identity, persists its input and dispatch state, and submits through the retained native Codex connection. Queue acceptance is not proof of provider execution. Thread, turn, account and process identities remain attached to observations and receipts.

The Agent composer and the separate ordinary History workbench share recovery mechanisms but are different consumers. Normal startup does not expose the ordinary workbench as a new navigation destination.

## Drafts and uncertain outcomes

`agent_draft_storage`, `agent_draft_recovery` and `ClientDraftStore` preserve unsent input and recovered copies. Shared-store conflicts retain the local edit and the competing saved copy; explicit recovery and export make the distinction visible. Quit can be cancelled while the conflict remains unresolved.

Persist input before dispatch. A lost response can mean that an operation ran. Check the exact saved receipt or native history before changing its state; do not resend an uncertain command. A later observation from another task, account or generation cannot settle it. Native closing-thread recovery resumes the existing conversation and keeps delivery evidence.

## Edit an earlier prompt (O03)

Review the exact native item and complete input, including attachment and mention identity. The service confirms the selected history boundary before one `thread/revert` request. Revert removes the selected and later turns from conversation history; it does **not** undo workspace file changes. The thread identity and native settings remain native-owned.

The service records the attempt before the write. Success or a lost reply requires readback; recovery does not issue the mutation again. Projection refresh and durable desktop draft handback must finish before normal input resumes. The restored edit is not submitted automatically. **Send edited input** is a separate explicit action.

## History and rendering

Current native timeline readers preserve complete selected items through paging. Large approvals, long tool output, unfinished answers, native image references and public reasoning must not become fabricated complete text. Optional math, Mermaid, weather and rich previews keep source access and literal fallback. A historical opaque media reference does not prove the underlying media is currently available.

## Recap and questions

Manual recap (O01) is retained. Automatic recap (O02) is a separate preference and defaults off; it is unrelated to scheduled upstream maintenance. Recap generation uses the native task and exact publication identity. A foreground result does not qualify a long background eligibility timer.

Asynchronous questions keep explicit answers and Skip. The separately retained nonblocking timeout policy (O16) can produce an empty answer after its grace/countdown conditions; it is not a general permission to answer questions for the user.

## Evidence and extension boundary

The [signed draft record](../../docs/archive/upstream-2026-09/signed-draft-acceptance.md) contains scoped restart, conflict, cancelled-Quit and export observations for its named artifact. It does not qualify every input surface or current installed build. Start code investigation with `agent/prompt_edit.rs`, `agent_prompt_edit.rs`, the draft owners and their focused tests. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md) and [Agent coordination](../architecture/chief-coordination.md).

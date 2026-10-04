---
type: Reference
title: "Conversation input, history and recovery"
description: "Explicit native branches, same-thread edits, canonical drafts, complete export and uncertain result recovery."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-5eddd1d872eed61f8ba7e703
    resource: repo://apps/decodex-gpui/src/agent_draft_storage.rs
  - id: openwiki-source-d701c906e7fcc9ab2d0917bc
    resource: repo://apps/decodex-gpui/src/agent_inspection.rs
  - id: openwiki-source-0a913db08f7e3fac688dbce5
    resource: repo://apps/decodex-gpui/src/agent_prompt_confirm.rs
  - id: openwiki-source-b2084dffd07b4229957a0f94
    resource: repo://apps/decodex-gpui/src/agent_prompt_edit.rs
  - id: openwiki-source-5e245e8cc4db92f2dbe2ba47
    resource: repo://apps/decodex-gpui/src/agent_prompt_fork.rs
  - id: openwiki-source-3362ac542db6e3212a621c47
    resource: repo://apps/decodex-gpui/src/agent_recap_automatic.rs
  - id: openwiki-source-5606e779f593d160176b0a27
    resource: repo://apps/decodex-gpui/src/agent_transcript.rs
  - id: openwiki-source-dc292a3cdb3064363ab29907
    resource: repo://crates/decodex-codex/src/app_server_client/temporary_structured.rs
  - id: openwiki-source-5a9a9e8bb72a23939f23f6f6
    resource: repo://crates/decodex-runtime/src/agent_transcript.rs
  - id: openwiki-source-2a0e86d8a9789b05a13deccc
    resource: repo://crates/decodex-runtime/src/agent/prompt_edit.rs
generated: { by: "codex", at: "2026-10-03T17:31:35.485Z" }
verified:
  - by: openwiki/0.7.0
    at: 2026-10-03T17:31:35.485Z
---

# Conversation input, history and recovery

## Owners and normal input

The desktop composer stores editable client drafts. The service accepts a typed command with a stable identity, persists its input and dispatch state, and submits through the retained native Codex connection. Queue acceptance is not proof of provider execution. Thread, turn, account and process identities remain attached to observations and receipts.

The Agent composer and the separate ordinary History workbench share recovery mechanisms but are different consumers. Normal startup does not expose the ordinary workbench as a new navigation destination.

## Drafts and uncertain outcomes

`agent_draft_storage`, `agent_draft_recovery` and `ClientDraftStore` preserve unsent input and copies needed for conflict or uncertain-operation recovery. They are not a permanent archive of accepted edits. Shared-store conflicts retain the local edit and the competing saved copy; explicit recovery and export make the distinction visible. Quit can be cancelled while the conflict remains unresolved.

Persist input before dispatch. A lost response can mean that an operation ran. Check the exact saved receipt or native history before changing its state; do not resend an uncertain command. A later observation from another task, account or generation cannot settle it. Native closing-thread recovery resumes the existing conversation and keeps delivery evidence.

## Edit or branch at an earlier input

Review the exact native item and complete input, including image, skill and mention identity. The confirmation panel offers three explicit choices:

| Choice | Native history | Draft |
| --- | --- | --- |
| **Edit in new branch** | Create a fork before the selected input; keep the original conversation unchanged | Restore the edited canonical input on the new branch |
| **Branch after this turn** | Copy the selected completed turn and all earlier turns; keep the original unchanged | Open the branch without prefilling that input |
| **Remove turns from original and keep draft** | Revert the original before the selected input, removing it and all later turns | Keep the edited draft on the original thread |

A branch before the first input is an empty-prefix native fork, not an unrelated replacement conversation. Fork creation defers copied Goal continuation. Opening a branch does not send input or start a model turn. Neither a fork nor a same-thread revert undoes workspace file changes.

The desktop saves the exact choice, destination and edited draft before dispatch. The service reserves the operation before one native write. It saves a returned fork ID before a separate prefix read. If creation acceptance is unknown, retain the draft and read its receipt; do not create another branch. Known fork recovery only reads native identity and history. The source keeps its normal history and has no branch-draft input fence.

For a before-input branch or same-thread edit, projection refresh and durable draft handback must finish before that destination can accept normal input. Images and skill references stay in the canonical input path. **Send edited input** remains a separate explicit action. Ordinary composer drafts remain independent. Resuming a saved confirmation retains its original choice rather than asking for a new destination.

## Finish or cancel an edit

Cancel edit removes the exact saved draft when no confirmation, native receipt, pending handback, fork or send exists. Once a native operation may have run, closing the editor retains its identity for recovery. It does not cancel a confirmed history mutation or make an unknown send safe to repeat.

An accepted edited-input send removes its matching draft without creating a recovered archive copy. A completed after-turn branch removes the finished source edit operation; a before-input branch keeps the canonical draft on its new destination until the user sends it. Explicit Fork is the way to preserve the original conversation while continuing elsewhere.

Unfinished edit entries, Refresh conversation and Show saved local records live in Details. The normal timeline shows the active editor or one unresolved-edit notice rather than a list of saved drafts. Existing unsent drafts remain available for explicit recovery or discard; this change does not batch-delete them.

## History and rendering

Current native timeline readers preserve complete selected items through paging. Large approvals, long tool output, unfinished answers, native image references and public reasoning must not become fabricated complete text. Optional math, Mermaid, weather and rich previews keep source access and literal fallback. A historical opaque media reference does not prove the underlying media is currently available.

Complete Markdown export reads the full native conversation through its history owner. The service transfers bounded chunks tied to the exact source and export token, then releases the temporary document. The desktop can save it to a file or copy it. **Loaded excerpt** is a distinct action and must not be described as a complete export when older history is not loaded.

## Recap and questions

Manual recap is available. Automatic recap is a separate preference and defaults off; it is unrelated to scheduled upstream maintenance. Recap generation uses the native task and exact publication identity. A foreground result does not qualify a long background eligibility timer.

Recap uses a temporary native thread with tools disabled. For absent or built-in permission profiles, it selects the native `:read-only` default explicitly, so a managed workspace default cannot override the read-only request. An explicit custom profile keeps its own restrictions. The adapter checks the returned profile or sandbox and the ephemeral-thread flag before it starts inference.

Asynchronous questions keep explicit answers and Skip. The separately retained nonblocking timeout policy can produce an empty answer after its grace/countdown conditions; it is not a general permission to answer questions for the user.

## Evidence and extension boundary

Verify restart, conflict, cancelled Quit and draft export against the actual app artifact; a historical receipt does not qualify the current installed build. Start code investigation with `agent/prompt_edit.rs`, `agent_prompt_edit.rs`, the draft owners and their focused tests. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md) and [Agent coordination](../architecture/chief-coordination.md).

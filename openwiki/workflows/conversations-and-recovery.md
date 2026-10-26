---
type: Reference
title: "Conversation input, history and recovery"
description: "Explicit native branches, same-thread edits, canonical drafts, complete export and uncertain result recovery."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-6512b631b67649d924c16ba3
    resource: repo://apps/decodex-gpui/src/agent_archive.rs
  - id: openwiki-source-bf03f317e64cfefd11f6ea83
    resource: repo://apps/decodex-gpui/src/agent_attached_branches.rs
  - id: openwiki-source-258277a596f7d23ef310055b
    resource: repo://apps/decodex-gpui/src/agent_draft_merge.rs
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
  - id: openwiki-source-56b561d7e6dd906d218aa9d1
    resource: repo://apps/decodex-gpui/src/agent_read_state.rs
  - id: openwiki-source-3362ac542db6e3212a621c47
    resource: repo://apps/decodex-gpui/src/agent_recap_automatic.rs
  - id: openwiki-source-912dbe8f46e7ec6026887bbe
    resource: repo://apps/decodex-gpui/src/agent_sample.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-d57edbe5de23ebee985fc88b
    resource: repo://apps/decodex-gpui/src/agent_timeline_inputs.rs
  - id: openwiki-source-5606e779f593d160176b0a27
    resource: repo://apps/decodex-gpui/src/agent_transcript.rs
  - id: openwiki-source-5d032c6be684964aa2c401fa
    resource: repo://apps/decodex-gpui/src/agent_workspace.rs
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-ecdf4586908bb6226955607a
    resource: repo://crates/decodex-codex/src/app_server_client/history_item.rs
  - id: openwiki-source-830a575f82404942511da59b
    resource: repo://crates/decodex-codex/src/app_server_client/prediction.rs
  - id: openwiki-source-dc292a3cdb3064363ab29907
    resource: repo://crates/decodex-codex/src/app_server_client/temporary_structured.rs
  - id: openwiki-source-787f8ad27b8519fbed2bd039
    resource: repo://crates/decodex-codex/src/app_server_client/thread_fork.rs
  - id: openwiki-source-caeefdea0805519b59a469aa
    resource: repo://crates/decodex-protocol/src/desktop_drafts.rs
  - id: openwiki-source-c01ab8da738707861c64c143
    resource: repo://crates/decodex-protocol/src/desktop_message_delivery.rs
  - id: openwiki-source-95a7168e447de14b4b758f16
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_archive_tests.rs
  - id: openwiki-source-9e21e877b2d7ad96b0a4d637
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_capacity_tests.rs
  - id: openwiki-source-4a57fba4bea69171318c6323
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_fork_tests.rs
  - id: openwiki-source-720a745d503e0e60ebcad0c5
    resource: repo://crates/decodex-runtime/src/agent_detail.rs
  - id: openwiki-source-3d25cb6558fde84d70520ac2
    resource: repo://crates/decodex-runtime/src/agent_read_state.rs
  - id: openwiki-source-72323573c4abb11c31ffbeec
    resource: repo://crates/decodex-runtime/src/agent_sample.rs
  - id: openwiki-source-5a9a9e8bb72a23939f23f6f6
    resource: repo://crates/decodex-runtime/src/agent_transcript.rs
  - id: openwiki-source-f4bc7020783c49a911b7fb29
    resource: repo://crates/decodex-runtime/src/agent_visibility.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
  - id: openwiki-source-2a0e86d8a9789b05a13deccc
    resource: repo://crates/decodex-runtime/src/agent/prompt_edit.rs
  - id: openwiki-source-88b81a215b1bac061dad80ec
    resource: repo://crates/decodex-runtime/tests/native_qualification/model_errors.py
  - id: openwiki-source-c6bea677adcde563202b65b9
    resource: repo://crates/decodex-runtime/tests/native_qualification/response_identity.py
  - id: openwiki-source-58c0167d31c3e40dc89591ab
    resource: repo://database/src/agent_fork.rs
  - id: openwiki-source-7cbe85f6b9884923ffd5cbfa
    resource: repo://database/src/agent/tests/input_retry.rs
  - id: openwiki-source-28af4781ad8d9558410630da
    resource: repo://database/src/works.rs
generated: { by: "codex", at: "2026-10-26T14:39:55.302Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-26T15:20:45.937Z
---

# Conversation input, history and recovery

## Owners and normal input

The desktop composer stores editable client drafts. The service accepts a typed command with a stable identity, persists its input and dispatch state, and submits through the retained native Codex connection. Queue acceptance is not proof of provider execution. Thread, turn, account and process identities remain attached to observations and receipts.

The Agent composer and the separate ordinary History workbench share recovery mechanisms but are different consumers. Normal startup does not expose the ordinary workbench as a new navigation destination.

## Persistent Agent and Work context

Each user-created Agent has one current native conversation. It can own several Works and act as Project Lead without creating a permanent component manager. The Assistant is optional. Current Work requirements and authorized Materials are read before assignment, recovery and assessment; a compacted summary alone is not the authority. Different Work records do not create independent native directories or sandboxes within that conversation.

The canonical tool revision uses Work business commands and native Subagents. Ordinary input retains the original native conversation and its immutable tool set. A new local tool revision does not replace that conversation. The runtime checks the capabilities of the bound native context before it uses versioned tools. Old parent identifiers do not grant another persistent Agent input or private-history authority.

Work Results and command receipts are separate from the conversation's latest reply. Restored pending evidence remains awaiting the owner assessment. A requirement change rejects old completion; a pause stays effective after restart. Native continuation and exact terminal readback remain native-owned, with V1 native wait and qualified V2 idle wake limits in the [acceptance record](../testing/upstream-acceptance-boundaries.md).

## Drafts and uncertain outcomes

`agent_draft_storage` and `ClientDraftStore` keep the current editable input under its original conversation owner. A shared-store conflict merges current edits and unresolved delivery records. It does not create alternative whole-editor copies or a separate recovery manager. The current draft format is version 11.

An exact local message has its own delivery state. A positively unsent message shows **Retry** and **Dismiss** through the existing message UI. Retry preserves the original input and its attachments; it does not replace text that the user entered later. Repeated retries refer to one successor event. An **Unconfirmed** message offers **Check status**: an unknown outcome is not safe to resend. Queue acceptance remains separate from native execution.

Persist input before dispatch. A lost response can mean that an operation ran. Check the exact saved receipt or native history before changing its state; do not resend an uncertain command. A later observation from another task, account or generation cannot settle it. Native closing-thread recovery resumes the existing conversation and keeps delivery evidence.

## Native archive changes

Codex owns archive state. A confirmed archive removes non-root bound work from the active list and graph, while the service keeps its stored history and original relationships. The Assistant entry remains present and carries its archive state. Active descendants stay visible under their nearest visible ancestor. See [archive synchronization](../architecture/chief-coordination.md#archive-synchronization).

When a refreshed active snapshot removes the selected conversation or its composer owner, the desktop stops voice input and saves the draft under its original work and thread identity. Normal navigation restores the next conversation's own draft. If no conversation remains active, sending stays unavailable; a retained draft cannot start a replacement conversation or move to the Assistant. A failed snapshot read does not trigger this transition.

After Codex restores the original thread, a positive active-list observation restores its original relationships through the normal background refresh. An application restart is not required. Opening that conversation restores its saved draft under the original work and thread IDs. Native child tabs follow the same complete-list rule and retain their own draft when removed from the active view. An archive refresh does not delete accepted input or replay an uncertain command.

## Edit or branch at an earlier input

Review the exact native item and complete input, including image, skill and mention identity. The confirmation panel offers four explicit choices:

| Choice | Native history | Draft |
| --- | --- | --- |
| **Edit in new branch** | Create a fork before the selected input; keep the original conversation unchanged | Restore the edited canonical input on the new branch |
| **Branch after this turn** | Copy the selected completed turn and all earlier turns; keep the original unchanged | Open the branch without prefilling that input |
| **Attached branch after this turn** | Attach a persistent native branch containing the completed-turn prefix to the exact source conversation | Open the branch without prefilling or sending input |
| **Remove turns from original and keep draft** | Revert the original before the selected input, removing it and all later turns | Keep the edited draft on the original thread |

A branch before the first input is an empty-prefix native fork, not an unrelated replacement conversation. Ordinary branches defer copied Goal continuation. An attached branch starts without a copied native Goal; the source Goal remains unchanged. Opening any branch does not send input or start a model turn. Neither a fork nor a same-thread revert undoes workspace file changes.

The desktop saves the exact choice, destination and edited draft before dispatch. The service reserves the operation before one native write. It saves a returned fork ID before a separate prefix read. If creation acceptance is unknown, retain the draft and read its receipt; do not create another branch. Known fork recovery only reads native identity and history. The source keeps its normal history and has no branch-draft input fence.

An attached branch retains the exact source Agent as its local parent and keeps its Project, role, provider and tool revision. In the source Agent's preferences, open **Attached branches**, then **Open branch** to continue a verified active branch. Navigation preserves the source draft. The list reads the newest 100 saved Decodex attached-branch receipts and discloses truncation; it does not adopt branches created outside Decodex. Refresh is explicit.

An uncertain creation stays unconfirmed. Keep the saved choice and recover it instead of creating another branch. An acknowledged identity can require saved-choice recovery before history is ready. If a branch is archived, restore it in Codex and refresh the list. Native archive and parent-deletion rules remain authoritative. This workflow adds no automatic follow-up, scheduling, result merge or deletion control. The [attached-branch acceptance record](../testing/upstream-acceptance-boundaries.md#persistent-attached-branches) separates executed fixtures from installed-app acceptance.

For a before-input branch or same-thread edit, projection refresh and durable draft handback must finish before that destination can accept normal input. Images and skill references stay in the canonical input path. **Send edited input** remains a separate explicit action. Ordinary composer drafts remain independent. Resuming a saved confirmation retains its original choice rather than asking for a new destination.

## Finish or cancel an edit

Cancel edit removes the exact saved draft when no confirmation, native receipt, pending handback, fork or send exists. Once a native operation may have run, closing the editor retains its identity for recovery. It does not cancel a confirmed history mutation or make an unknown send safe to repeat.

An accepted edited-input send removes its matching draft without creating a recovered archive copy. A completed after-turn branch removes the finished source edit operation; a before-input branch keeps the canonical draft on its new destination until the user sends it. Explicit Fork is the way to preserve the original conversation while continuing elsewhere.

Unfinished edit entries, Refresh conversation and Show saved local records live in Details. The normal timeline shows the active editor or one unresolved-edit notice rather than a list of saved drafts. The current editor and unresolved edit operations retain their own identities; there is no saved-copy list.

## History and rendering

Current native timeline readers preserve complete selected items through paging. Large approvals, long tool output, unfinished answers, native image references and public reasoning must not become fabricated complete text. Optional math, Mermaid, weather and rich previews keep source access and literal fallback. A historical opaque media reference does not prove the underlying media is currently available.

Complete Markdown export reads the full native conversation through its history owner. The service transfers bounded chunks tied to the exact source and export token, then releases the temporary document. The desktop can save it to a file or copy it. **Loaded excerpt** is a distinct action and must not be described as a complete export when older history is not loaded.

## Terminal model errors and item identity

A native `modelNotFound` result is terminal. Its provider message and typed classification remain available in live results, stored history and a cold read; Decodex does not turn it into a capacity retry. Capacity recovery requires its own exact failed-turn evidence. A changed model or later user request is a separate action.

Native history also keeps one nonempty item identity when a provider assistant message omits its ID. The source-access and cold-read probes inspect the existing result without another inference request. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md#terminal-errors-and-external-context) for the exact artifact and transport limits.

## Read state and exact item details

Task preferences shows the native read state for the current root conversation and explicit read/unread actions. Refresh does not acknowledge a result. A mark is bound to the reviewed source and native receipt revision; source drift, an unavailable receipt or a stale revision requires a fresh read rather than an automatic retry. Native child conversations and the Dock's local status are separate.

Expanded tool and file details can read the exact native history item. Unsupported item-read methods fall back to the existing full-thread path; unrelated errors do not. This does not replace complete transcript export or add a bulk media loader.

## Recap and questions

Manual recap is available. Automatic recap is a separate preference and defaults off; it is unrelated to scheduled upstream maintenance. Recap generation uses the native task and exact publication identity. A foreground result does not qualify a long background eligibility timer.

Recap requests an isolated temporary native thread with tools disabled. For absent or built-in permission profiles, it selects the native `:read-only` default explicitly, so a managed workspace default cannot override the read-only request. An explicit custom profile keeps its own restrictions. The adapter checks the returned profile or sandbox and the ephemeral-thread flag before it starts inference. The retained native qualification still observed a model-required `apply_patch` entry: it verified environment denial, not removal of every tool name from the model catalog.

Asynchronous questions keep explicit answers and Skip. The separately retained nonblocking timeout policy can produce an empty answer after its grace/countdown conditions; it is not a general permission to answer questions for the user.

For parent escalation and exact event receipts, use [Agent coordination](../architecture/chief-coordination.md#native-contributions-and-independent-collaboration). In Ask separately, **Close** hides the panel; **Cancel answer** requests cancellation of its exact native sampling request. A late result or uncertain cancellation keeps its original request identity. This does not cancel the parent conversation.

## Explicit prediction capability

The adapter can create one ephemeral prediction fork of a loaded parent, with the native model, provider, environment, context, tools and permissions inherited. Its bounded structured run has a 30-second deadline and exact-thread cleanup; cancellation before inference makes no model request. It does not send to the parent, modify a draft, schedule background predictions or add a suggestion UI. Callers must authorize inherited tool use. It must not replace the tool-free recap path.

## Evidence and extension boundary

Verify restart, concurrent edits, failed-message retry and uncertain-message status against the actual app artifact; a historical receipt does not qualify the current installed build. Start code investigation with `agent/prompt_edit.rs`, `agent_prompt_edit.rs`, the draft owners and their focused tests. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md) and [Agent coordination](../architecture/chief-coordination.md).

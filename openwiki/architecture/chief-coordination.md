---
type: Reference
title: "Project, Agent and Work coordination"
description: "Local work ownership, native conversation dispatch, explicit branches and recovery."
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-25T02:09:34.237Z
sources:
  - id: openwiki-source-c76550e85b25876b339f9255
    resource: repo://apps/decodex-gpui/src/agent_context_settings.rs
  - id: openwiki-source-1e0b3343cd024646618a1f95
    resource: repo://apps/decodex-gpui/src/agent_native_agents.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-d0b9aaf952250307abc4342e
    resource: repo://crates/decodex-codex/src/app_server_client/archive.rs
  - id: openwiki-source-2b08587e9b26e203156110c8
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences.rs
  - id: openwiki-source-787f8ad27b8519fbed2bd039
    resource: repo://crates/decodex-codex/src/app_server_client/thread_fork.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-f4bc7020783c49a911b7fb29
    resource: repo://crates/decodex-runtime/src/agent_visibility.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-a5bf72500fe69a895fca0dac
    resource: repo://crates/decodex-runtime/src/agent/coordination.md
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
  - id: openwiki-source-52d3ef4824079f66e9063566
    resource: repo://crates/decodex-runtime/src/agent/native_subagents.rs
  - id: openwiki-source-2a0e86d8a9789b05a13deccc
    resource: repo://crates/decodex-runtime/src/agent/prompt_edit.rs
  - id: openwiki-source-30c7edf894a639091ecbafc6
    resource: repo://crates/decodex-runtime/src/agent/tests/external_context.rs
  - id: openwiki-source-d511cef2848dd5f6968572ae
    resource: repo://crates/decodex-runtime/src/agent/tool_migration.rs
  - id: openwiki-source-b1dd4c8ab85f342314daa6f1
    resource: repo://crates/decodex-runtime/src/agent/work.rs
  - id: openwiki-source-a09c082db4ad1473c4d1e557
    resource: repo://crates/decodex-runtime/src/application.rs
  - id: openwiki-source-58c0167d31c3e40dc89591ab
    resource: repo://database/src/agent_fork.rs
  - id: openwiki-source-51332b5dcd4b194b62fec905
    resource: repo://database/src/agent_guardian.rs
  - id: openwiki-source-e1fc2e0e81623c7dd7ff5417
    resource: repo://database/src/agent_process.rs
  - id: openwiki-source-0f0fc00c258a3ddad6935fcc
    resource: repo://database/src/agent_tool_migration.rs
  - id: openwiki-source-28af4781ad8d9558410630da
    resource: repo://database/src/works.rs
  - id: openwiki-source-4e05748daab41d1c9c2f54dd
    resource: repo://database/src/works/collaboration.rs
  - id: openwiki-source-c0eb738b42368debe7ef7545
    resource: repo://database/src/works/commands.rs
generated: { by: "codex", at: "2026-10-24T21:19:56.719Z" }
---

# Project, Agent and Work coordination

Project, Agent and Work are separate durable objects. An Agent has one current native conversation. Assistant and Project Lead are responsibilities of an existing Agent. The user can work directly with the Project Lead from discussion through delivery. One Agent can own Work in several Projects. There is no fixed L0/L1/L2 hierarchy or mandatory personal router.

The [accepted contract](../decisions/project-agent-work.md) defines the shared vocabulary and minimum state rules. The default model tools are `work_read` and `work_manage`. Only the user creates persistent Agents. Native Codex supplies bounded Subagents; Decodex does not create a second temporary executor. The former managed-worker creation, continuation and goal tools are retired from the default tool surface and refused if an older conversation still advertises them. Compatibility storage preserves genuine old execution and history.

## Owners and flow

The desktop sends typed commands to the service. SQLite owns Project membership, Agent responsibilities, Work requirements, holds, Results, assessments and Collaboration Requests. Codex owns native threads, turns, Subagents, messages, waits, context, Goals, permissions and enforcement. Work commands persist business decisions; they do not spawn or message native helpers.

A Work has one accountable Agent, immutable requirements revisions and explicit completion criteria. The owner can perform it directly or use native helpers. `work_read` supplies current scoped evidence. `work_manage` uses a stable command ID and exact content for retries. The service verifies authority from the actual delivered user decision, then applies the command transactionally. A contributor's request is application evidence, not a new user instruction.

The evidence chain is Work -> requirements -> issued instruction -> execution -> Result -> assessment. Native evidence keeps exact thread, turn and item identities. Mutable files keep their digest and execution-directory boundary. Identity checks establish provenance; the owner still judges whether the Result meets the requirements. Native completion, transport delivery, acceptance, a Guardian Review and a read receipt do not complete Work.

Project membership does not grant private conversation access. Agent and granted Material scope is applied in SQL before bounded reads. A native conversation still has its actual execution directory and Project. Associating Work with another Project does not switch that directory, change permissions or create a sandbox. Use a separate execution context when hard isolation is required.

## Native contributions and independent Collaboration

The owner saves a contribution intent before native dispatch, uses the returned `nativePrompt`, then binds the observed direct native call. Recovery requires exact parent turn/call and child evidence. V1 uses its native tools and Result history. V2 uses matched delegation identities and root-turn evidence; canonical child history does not contain inbound Agent messages. Missing or ambiguous evidence stays Unknown. Nested Code Mode attribution is not qualified and does not justify a duplicate spawn.

The selected native backend comes from native model metadata and configuration. A feature flag alone does not establish V1 or V2. Qualified V2 can wake its idle owner from native completion. V1 uses the native wait tool for required Results. Neither path needs Ultra or a local polling controller.

Independent Agents cooperate through scoped Collaboration Requests in the existing durable inbox. Sending can queue while the recipient is busy. Delivery is distinct from acceptance; acceptance requires the exact delivered turn. A returned contribution remains pending until the owner accepts it or requests repair. Repair keeps the same request and adds a new immutable instruction. Required requests must be handled before Work completion. Cancellation affects that request; late evidence remains history.

Questions first stay within the assignment and existing authority. A Subagent uses native messages and waits. An independent Agent uses the scoped request. The owner escalates only missing decisions or authority to the user. Ask Me approvals remain human-owned. Agent conversations stay available after individual Work completes.

## Persistence and recovery

Work, instructions, inbox delivery, requirements, Results, assessments and command receipts survive restart. A retry with the same command ID and exact content returns its committed receipt before mutable external evidence is checked again. Unknown native effects are reconciled from exact history. No missing acknowledgment authorizes replay, a second spawn or optimistic completion.

Changes preserve earlier instructions and Results. An old Result cannot complete a newer requirements revision. A hold blocks new Decodex contributions and completion. It does not roll back or automatically stop an already-running native action, Goal or child. Native interruption remains native-owned.

A tool-surface change uses the durable migration journal only after the source and descendants are quiescent and native inspection is available. It saves a positively known replacement before retiring the old source, verifies settings after resume and requires a positive archive receipt before switching. Uncertain creation or retirement stays parked. Previous thread history remains available read-only; its old parent identifiers grant no input authority.

Known external ownership rejects input before enqueueing it. A race can leave accepted but unsent input; that input remains a user-decision record and needs a fresh explicit send when ownership returns. One Agent's external writer does not block unrelated Agents. Prior-boot process death still requires positive kernel evidence; optional metadata failures do not close the shared transport.

## Inbox and native queues

The existing inbox owns Decodex user input, async answers and peer application evidence. These retain different provenance and recovery rules. Decodex validates dispatch state, dependencies and native ownership, records intent, then submits the exact native turn. It does not write the same input to a second native queue.

Native user queues remain native-owned. They do not carry all local dependency, account, application-evidence and delivery semantics. A future queue cutover would need explicit reconciliation of uncertain admission before any pending input moves. No such cutover is part of this implementation.

The native context preference panel still edits `features.retain_client_tool_outputs`. Compatibility delegation records can request bounded retention for `work_instruction`; wake and retry evidence does not request it. Native retention does not replace durable Work records or their owner.

## Archive synchronization

Codex owns the archive state. The service observes the exact native thread IDs bound to retained local work. It searches active threads first, then searches archived threads for the remaining IDs. Both lists include every provider and source kind. A positive archived observation hides non-root work; a positive active observation restores it. The personal Agent entry remains present. An absent ID, an empty title or a completed status does not authorize deletion.

The reader orders native pages by update time and stops when it finds every requested ID. It does not need to read the entire archive. Native thread metadata supplies working-directory filters when every binding can be read. Otherwise, the reader scans the complete catalogs. Transport frame limits, per-page deadlines, and retained-ID and cursor budgets bound the operation. Valid positive observations remain usable if a later page fails or times out. IDs without positive evidence retain their previous visibility.

Archive observations refresh in the background on a five-second interval. Only one refresh runs at a time within a native process generation; a changed set of bindings can request an earlier refresh. Work snapshots use confirmed observations without waiting for the native read. A new process starts with no inherited observations, and a late result from an earlier process cannot replace them. The service does not write a second archive flag into SQLite.

The active snapshot supplies the work list and relationship graph. Archived non-root work is hidden. The personal Agent remains as the coordination entry point, with its archive state available to the input gate. Filtering removes incident relationships, pending-event entries and context references from the view. Hidden conversation records leave the active Project view. Live descendants remain visible under their nearest visible ancestor. Stored work and the original hierarchy remain intact. A later positive active observation restores visibility without an application restart.

Native child lists explicitly include all providers and exclude archived threads. Only a complete valid list replaces the previous list. When a child leaves that list, the desktop saves its draft and closes its affected tabs. Removal of one local owner clears only that owner's child state. Other open conversations keep their connection, input and view state.

An unused local draft has no native archive state. Use the explicit service-owned cleanup command in [Commands and validation](../operations/commands-and-validation.md#discard-an-unused-local-draft). The service rejects cleanup when the target contains real work or references.

## Native child requests and approvals

Native child requests resolve through verified thread-spawn ancestry to the owning local work item, with cycle and depth bounds. A fork alone does not establish child authority. Child approvals retain the child's native identity; child ownership does not grant local manager-tool authority.

Guardian observations are durable review evidence. They neither authorize execution nor wake work by themselves. Approval UI must answer the exact pending request and preserve uncertainty when acknowledgment is unavailable.

## Explicit conversation branches

A user can create a native branch before an input or after its completed turn. This is a separate action from a same-thread history edit. The source conversation keeps its history. Codex owns the new thread and copied context; `deferGoalContinuation` prevents the copied Goal from continuing as part of fork creation.

Decodex reserves a new local work item before the native request. It saves the acknowledged native ID before a separate history read. A lost creation reply stays uncertain and cannot authorize another fork. Recovery of a known ID reads its exact source and prefix without starting a model turn.

The branch is an independent user-created conversation. Legacy source relationships remain compatibility evidence, not a mandatory business hierarchy. It keeps the source role, Project association and recorded tool version. It does not copy pending inbox work or dependency edges. An acknowledged fork has an explicit ownership record that permits it to use the source process. Independent Agents do not gain that authority from an old parent identifier. A branch does not create another Assistant or silently upgrade native tools.

Before-input branches use the existing canonical input and draft handback owner. After-turn branches do not prefill the selected input. See [conversation recovery](../workflows/conversations-and-recovery.md) for the user actions and uncertain-result behavior.

## Verification and navigation

Run focused tests in `agent/tests.rs`, `agent/tests/native_subagents.rs`, `agent/tests/archive.rs`, `agent_host.rs` and `agent_visibility.rs`. The desktop archive tests in `agent_native_agents.rs` cover unrelated conversations, retired tabs and retained drafts. Distinguish fixture tests from live provider tests and retained historical receipts.

See [Desktop workspace](desktop-workspace.md), [Subscription voice](../integrations/subscription-voice.md), and [Runtime architecture](runtime-architecture.md).

## Current workflow documentation

Read [conversation recovery](../workflows/conversations-and-recovery.md), [settings](../workflows/models-and-settings.md) and [approvals](../workflows/approvals-and-native-ownership.md) for the consolidated contracts. Native execution remains in Codex; completion reports remain evidence for the Agent to assess.

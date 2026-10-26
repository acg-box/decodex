---
type: Reference
title: "Project, Agent and Work coordination"
description: "Local work ownership, native conversation dispatch, explicit branches and recovery."
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-26T14:39:55.302Z
sources:
  - id: openwiki-source-bf03f317e64cfefd11f6ea83
    resource: repo://apps/decodex-gpui/src/agent_attached_branches.rs
  - id: openwiki-source-d9c9b48c73ee4a2adc90f01c
    resource: repo://apps/decodex-gpui/src/agent_collaboration_cases.rs
  - id: openwiki-source-c76550e85b25876b339f9255
    resource: repo://apps/decodex-gpui/src/agent_context_settings.rs
  - id: openwiki-source-1e0b3343cd024646618a1f95
    resource: repo://apps/decodex-gpui/src/agent_native_agents.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-0d053d4de2f668168f982629
    resource: repo://apps/decodex-gpui/src/agent_usage_evidence.rs
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-d0b9aaf952250307abc4342e
    resource: repo://crates/decodex-codex/src/app_server_client/archive.rs
  - id: openwiki-source-2b08587e9b26e203156110c8
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences.rs
  - id: openwiki-source-787f8ad27b8519fbed2bd039
    resource: repo://crates/decodex-codex/src/app_server_client/thread_fork.rs
  - id: openwiki-source-991553c8d747a54ecf5a9376
    resource: repo://crates/decodex-core/src/work.rs
  - id: openwiki-source-1b314e86ab2f1adbc0ede4c8
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_project_sync_tests.rs
  - id: openwiki-source-725813d500ec103482924456
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_project_work_tests.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-1477615a268800361379ce21
    resource: repo://crates/decodex-runtime/src/agent_initial_settings.rs
  - id: openwiki-source-f4bc7020783c49a911b7fb29
    resource: repo://crates/decodex-runtime/src/agent_visibility.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-a5bf72500fe69a895fca0dac
    resource: repo://crates/decodex-runtime/src/agent/coordination.md
  - id: openwiki-source-a7886792ad3e47b8e1afb7fa
    resource: repo://crates/decodex-runtime/src/agent/guardian_details.rs
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
  - id: openwiki-source-1bebc823fa8b6426acb4d5e2
    resource: repo://crates/decodex-runtime/src/agent/native_settings.rs
  - id: openwiki-source-52d3ef4824079f66e9063566
    resource: repo://crates/decodex-runtime/src/agent/native_subagents.rs
  - id: openwiki-source-419a87794b421322302c7d9e
    resource: repo://crates/decodex-runtime/src/agent/project.rs
  - id: openwiki-source-2a0e86d8a9789b05a13deccc
    resource: repo://crates/decodex-runtime/src/agent/prompt_edit.rs
  - id: openwiki-source-30c7edf894a639091ecbafc6
    resource: repo://crates/decodex-runtime/src/agent/tests/external_context.rs
  - id: openwiki-source-a7b97dd347fbe2f630692d12
    resource: repo://crates/decodex-runtime/src/agent/tests/native_agent_wake.rs
  - id: openwiki-source-4f59431f3f6264a65208bdcb
    resource: repo://crates/decodex-runtime/src/agent/work_delivery.rs
  - id: openwiki-source-b1dd4c8ab85f342314daa6f1
    resource: repo://crates/decodex-runtime/src/agent/work.rs
  - id: openwiki-source-3c62f617dc1d15daed2c93e4
    resource: repo://crates/decodex-runtime/src/application_collaboration.rs
  - id: openwiki-source-a09c082db4ad1473c4d1e557
    resource: repo://crates/decodex-runtime/src/application.rs
  - id: openwiki-source-58c0167d31c3e40dc89591ab
    resource: repo://database/src/agent_fork.rs
  - id: openwiki-source-51332b5dcd4b194b62fec905
    resource: repo://database/src/agent_guardian.rs
  - id: openwiki-source-e1fc2e0e81623c7dd7ff5417
    resource: repo://database/src/agent_process.rs
  - id: openwiki-source-fc5ad70343bb20c4e9d12540
    resource: repo://database/src/agent_process/attachments.rs
  - id: openwiki-source-7c1fe4a0f054521cb1d94e08
    resource: repo://database/src/agent/collaboration.rs
  - id: openwiki-source-d1279ad243eb550afec4d0e6
    resource: repo://database/src/guardian_settings.rs
  - id: openwiki-source-2b705b409434252a7417e708
    resource: repo://database/src/usage_evidence_tests.rs
  - id: openwiki-source-8c78df4b382026886ac50894
    resource: repo://database/src/usage_evidence.rs
  - id: openwiki-source-28af4781ad8d9558410630da
    resource: repo://database/src/works.rs
  - id: openwiki-source-4e05748daab41d1c9c2f54dd
    resource: repo://database/src/works/collaboration.rs
  - id: openwiki-source-c0eb738b42368debe7ef7545
    resource: repo://database/src/works/commands.rs
generated: { by: "codex", at: "2026-10-26T14:39:55.302Z" }
---

# Project, Agent and Work coordination

Project, Agent and Work are separate durable objects. An Agent has one current native conversation. Assistant and Project Lead are responsibilities of an existing Agent. The user can work directly with the Project Lead from discussion through delivery. One Agent can own Work in several Projects. There is no fixed L0/L1/L2 hierarchy or mandatory personal router.

The [accepted contract](../decisions/project-agent-work.md) defines the shared vocabulary and minimum state rules. The default model tools are `work_read` and `work_manage`. Only the user creates persistent Agents. Native Codex supplies bounded Subagents; Decodex does not create a second temporary executor. The former managed-worker creation, continuation and goal tools are retired from the default tool surface and refused if an older conversation still advertises them. Durable conversation records preserve native history.

## Owners and flow

The desktop sends typed commands to the service. SQLite owns Project membership, Agent responsibilities, Work requirements, holds, Results, assessments and Collaboration Requests. Codex owns native threads, turns, Subagents, messages, waits, context, Goals, permissions and enforcement. Work commands persist business decisions; they do not spawn or message native helpers.

A new independent conversation can derive its initial settings from its actual first pending user input. An Assistant is not required. Explicit model and effort choices are retained. A bound conversation or uncertain native effect never uses this bootstrap path. After native creation, Decodex materializes the thread name before Project reconciliation.

Several independent conversations can share one admitted native process. Each Agent attaches only after that process has created or resumed its exact native thread. The binding preserves the process account, generation and live-owner fences; Project membership alone grants no process authority.

A Work has one accountable Agent, immutable requirements revisions and explicit completion criteria. The owner can perform it directly or use native helpers. `work_read` supplies current scoped evidence. `work_manage` uses a stable command ID and exact content for retries. The service verifies authority from the actual delivered user decision, then applies the command transactionally. A contributor's request is application evidence, not a new user instruction.

The evidence chain is Work -> requirements -> issued instruction -> execution -> Result -> assessment. Native evidence keeps exact thread, turn and item identities. Mutable files keep their digest and execution-directory boundary. Identity checks establish provenance; the owner still judges whether the Result meets the requirements. Native completion, transport delivery, acceptance, a Guardian Review and a read receipt do not complete Work.

Project membership does not grant private conversation access. Agent and granted Material scope is applied in SQL before bounded reads. A native conversation still has its actual execution directory and Project. Associating Work with another Project does not switch that directory, change permissions or create a sandbox. Use a separate execution context when hard isolation is required.

Before dispatch, Decodex resumes the exact retained native conversation and reconciles its registered Project. An unassigned conversation can join that native Project through `thread/metadata/update`. Existing matching membership is reused. Conflicting or unavailable membership and changed roots park that Agent's input for inspection; other queued Agents can continue. The operation keeps the conversation, execution directory and native permissions. Native catalog and membership readback do not establish the official desktop's saved-Project or sidebar presentation.

## Native contributions and independent Collaboration

The owner saves a contribution intent before native dispatch. A new helper uses the returned `nativeTaskName` as its native task name and `nativePrompt` as its message. Direct native calls retain their existing attribution. Visible child input can prove the exact prompt, sender, recipient, ancestry and receiving turn; one unique parent activity then completes the binding.

Native V2 can encrypt the child input. In that case, Decodex can verify the prepared task name, native parent metadata, root turn and unique parent activity. It records this as task-identity proof, not plaintext verification. Source history can recover a named assignment after a missed input event. The legacy unnamed encrypted Work bindings remain Unknown; their prompts are not reconstructed.

The control acceptance follow-up recovered the native owner and exact encrypted delivery for the old venue and lodging helpers. Their own returned messages identified the expected Work contributions. This corroborates the result association, but it does not prove that the encrypted input equals the prepared instruction. The follow-up did not write inferred Work bindings. See `retained_project_agent_work.controls_followup` in the [retained evidence](../../automations/upstream/retained-evidence.json).

An exact `work_read` request can inspect up to five recent parent turns and use native child history to recover an existing binding. It does not spawn, resend or run a polling controller. Result evidence must match the recorded receiving turn or verified root-turn assignment. Script text, quoted messages and timing do not establish dispatch. V1 retains its native prompt and activity path.

The selected native backend comes from native model metadata and configuration. A feature flag alone does not establish V1 or V2. Qualified V2 can wake its idle owner from native completion. V1 uses the native wait tool for required Results. Neither path needs Ultra or a local polling controller.

V2 exposes collaboration directly to the model by default. Nested V2 qualification uses the supported native `multi_agent_v2.non_code_mode_only=false` setting. Decodex does not change that default or add a second executor. The retained exact-artifact tests distinguish V1, permitted V2 nesting and default direct-only behavior.

Independent Agents cooperate through scoped Collaboration Requests in the existing durable inbox. Sending can queue while the recipient is busy. Delivery is distinct from acceptance; acceptance requires the exact delivered turn. A returned contribution remains pending until the owner accepts it or requests repair. Repair keeps the same request and adds a new immutable instruction. Required requests must be handled before Work completion. Cancellation affects that request; late evidence remains history.

Questions first stay within the assignment and existing authority. A Subagent uses native messages and waits. An independent Agent uses the scoped request. The owner escalates only missing decisions or authority to the user. Ask Me approvals remain human-owned. Agent conversations stay available after individual Work completes.

## Read-only collaboration evidence and usage

The existing collaboration history shows locally retained answers beside their exact native question. The join uses the Agent, native thread and question ID. A reply receipt can prove saved text, acknowledged delivery and the receiving turn's terminal outcome. None of these facts resolves the question or completes Work. A reply made through another client can lack a retained body; a question tombstone then remains an unknown disposition rather than an invented answer.

Independent contribution history uses the request and immutable instruction identities. A repair creates another cycle. Earlier cycles keep their own sources and do not borrow the current cycle's acceptance or owner assessment. Native admission observations append evidence in the existing inbox without rewriting the original request or waking an Agent. The display adds no automatic escalation or acceptance policy.

**Actual usage** in Agent preferences and Work details reads retained native turn receipts. Agent totals count each Agent/thread/turn identity once across retained bindings. Work includes exact Result, native delivery and peer admission references across retained requirements. Several recorded Work references make a turn shared; that turn is excluded from linked-turn totals. A linked turn can also contain other activity. Missing receipts and unobserved native-child usage remain unknown.

The reader sums known whole-turn input and output deltas, and separately sums recorded native duration. It does not sum cumulative thread counters or convert provider amounts into a bill. Recorded durations can overlap and are not Work elapsed time. Bounded source examples identify the underlying turns; the aggregate still covers all retained matching receipts. See the [qualification limits](../testing/upstream-acceptance-boundaries.md#selected-read-only-views).

## Persistence and recovery

Work, instructions, inbox delivery, requirements, Results, assessments and command receipts survive restart. A retry with the same command ID and exact content returns its committed receipt before mutable external evidence is checked again. Unknown native effects are reconciled from exact history. No missing acknowledgment authorizes replay, a second spawn or optimistic completion.

Changes preserve earlier instructions and Results. An old Result cannot complete a newer requirements revision. A hold blocks new Decodex contributions and completion. It does not roll back or automatically stop an already-running native action, Goal or child. Native interruption remains native-owned.

Ordinary input resumes the retained native conversation. A newer local tool revision does not create or replace a thread. Native history and the thread's recorded tool capabilities remain bound to that context. No context-upgrade journal is maintained.

Known external ownership rejects input before enqueueing it. A race can leave accepted but unsent input; that input remains a user-decision record and needs a fresh explicit send when ownership returns. One Agent's external writer does not block unrelated Agents. Prior-boot process death still requires positive kernel evidence; optional metadata failures do not close the shared transport.

## Inbox and native queues

The existing inbox owns Decodex user input, async answers and peer application evidence. These retain different provenance and recovery rules. Decodex validates dispatch state, dependencies and native ownership, records intent, then submits the exact native turn. It does not write the same input to a second native queue.

Native user queues remain native-owned. They do not carry all local dependency, account, application-evidence and delivery semantics. A future queue cutover would need explicit reconciliation of uncertain admission before any pending input moves. No such cutover is part of this implementation.

The native context preference panel still edits `features.retain_client_tool_outputs`. Compatibility delegation records can request bounded retention for `work_instruction`; wake and retry evidence does not request it. Native retention does not replace durable Work records or their owner.

## Archive synchronization

Codex owns the archive state. The service observes the exact native thread IDs bound to retained local work. It searches active threads first, then searches archived threads for the remaining IDs. Both lists include every provider and source kind. A positive archived observation hides non-root work; a positive active observation restores it. The personal Agent entry remains present. An absent ID, an empty title or a completed status does not authorize deletion.

The reader orders native pages by update time and stops when it finds every requested ID. It does not need to read the entire archive. Native thread metadata supplies working-directory filters when every binding can be read. Otherwise, the reader scans the complete catalogs. Transport frame limits, per-page deadlines, and retained-ID and cursor budgets bound the operation. Valid positive observations remain usable if a later page fails or times out. IDs without positive evidence retain their previous visibility. Attached native threads are hidden from ordinary `thread/list`. Their exact `attachedToThreadId` query supplies archive membership, including empty forks. A failed metadata or attachment-filter read leaves the observation unknown.

Archive observations refresh in the background on a five-second interval. Only one refresh runs at a time within a native process generation; a changed set of bindings can request an earlier refresh. Work snapshots use confirmed observations without waiting for the native read. A new process starts with no inherited observations, and a late result from an earlier process cannot replace them. The service does not write a second archive flag into SQLite.

The active snapshot supplies the work list and relationship graph. Archived non-root work is hidden. The personal Agent remains as the coordination entry point, with its archive state available to the input gate. Filtering removes incident relationships, pending-event entries and context references from the view. Hidden conversation records leave the active Project view. Live descendants remain visible under their nearest visible ancestor. Stored work and the original hierarchy remain intact. A later positive active observation restores visibility without an application restart.

Native child lists explicitly include all providers and exclude archived threads. Only a complete valid list replaces the previous list. When a child leaves that list, the desktop saves its draft and closes its affected tabs. Removal of one local owner clears only that owner's child state. Other open conversations keep their connection, input and view state.

An unused local draft has no native archive state. Use the explicit service-owned cleanup command in [Commands and validation](../operations/commands-and-validation.md#discard-an-unused-local-draft). The service rejects cleanup when the target contains real work or references.

## Native child requests and approvals

Native child requests resolve through verified thread-spawn ancestry to the owning local work item, with cycle and depth bounds. A fork alone does not establish child authority. Child approvals retain the child's native identity; child ownership does not grant local manager-tool authority.

Guardian observations are durable review evidence. They neither authorize execution nor wake work by themselves. Approval UI must answer the exact pending request and preserve uncertainty when acknowledgment is unavailable.

Review Details can retain exposed native assistant output. The selected native protocol supports raw-event subscription at thread creation, but not on resume. After a completed review, bounded history recovery checks the newest 32 private candidates in the same directory and up to five turns per candidate. It verifies the native parent before retaining output. Raw reasoning is excluded. The protocol does not supply an exact action Review association; this remains explicit. Missing records remain possible, and these observations never authorize or replay an action.

## Explicit conversation branches

A user can create a native branch before an input, after its completed turn, or as an attached branch after that turn. This is a separate action from a same-thread history edit. The source conversation keeps its history. Codex owns the new thread and copied context. Ordinary forks defer copied Goal continuation. Attached forks use `attachToSource`, start without the source Goal snapshot and leave the original Goal with its original conversation.

Decodex reserves a new local work item before the native request. It saves the acknowledged native ID before a separate history read. A lost creation reply stays uncertain and cannot authorize another fork. Recovery of a known ID reads its exact source and prefix without starting a model turn.

An ordinary branch is an independent user-created conversation. An attached branch keeps its exact source Agent as its local parent. It is a persistent Agent conversation, not a native Subagent. Both forms keep the source role, Project association and recorded tool version. It does not copy pending inbox work or dependency edges. An acknowledged fork has an explicit ownership record that permits it to use the source process. Independent Agents do not gain that authority from an old parent identifier. A branch does not create another Assistant or silently upgrade native tools.

Before-input branches use the existing canonical input and draft handback owner. After-turn branches do not prefill the selected input. The **Attached branches** list shows retained Decodex creation receipts, including unresolved outcomes. Opening a ready branch uses the existing conversation controls and preserves the source draft. Closing a tab keeps the branch; native Codex owns archive, restore and parent-deletion safeguards. See [conversation recovery](../workflows/conversations-and-recovery.md) for the user actions and uncertain-result behavior.

## Verification and navigation

Run focused tests in `agent/tests.rs`, `agent/tests/native_subagents.rs`, `agent/tests/archive.rs`, `agent_host.rs` and `agent_visibility.rs`. The desktop archive tests in `agent_native_agents.rs` cover unrelated conversations, retired tabs and retained drafts. Distinguish fixture tests from live provider tests and retained historical receipts.

See [Desktop workspace](desktop-workspace.md), [Subscription voice](../integrations/subscription-voice.md), and [Runtime architecture](runtime-architecture.md).

## Current workflow documentation

Read [conversation recovery](../workflows/conversations-and-recovery.md), [settings](../workflows/models-and-settings.md) and [approvals](../workflows/approvals-and-native-ownership.md) for the consolidated contracts. Native execution remains in Codex; completion reports remain evidence for the Agent to assess.

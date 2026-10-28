---
type: Reference
title: "Retained upstream product capabilities"
description: "Current native integration, account and conversation scope, with excluded duplicate surfaces."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-bf03f317e64cfefd11f6ea83
    resource: repo://apps/decodex-gpui/src/agent_attached_branches.rs
  - id: openwiki-source-d9c9b48c73ee4a2adc90f01c
    resource: repo://apps/decodex-gpui/src/agent_collaboration_cases.rs
  - id: openwiki-source-c76550e85b25876b339f9255
    resource: repo://apps/decodex-gpui/src/agent_context_settings.rs
  - id: openwiki-source-9c68c30b2ff24ddf9a764d74
    resource: repo://apps/decodex-gpui/src/agent_memory.rs
  - id: openwiki-source-b2084dffd07b4229957a0f94
    resource: repo://apps/decodex-gpui/src/agent_prompt_edit.rs
  - id: openwiki-source-5e245e8cc4db92f2dbe2ba47
    resource: repo://apps/decodex-gpui/src/agent_prompt_fork.rs
  - id: openwiki-source-56b561d7e6dd906d218aa9d1
    resource: repo://apps/decodex-gpui/src/agent_read_state.rs
  - id: openwiki-source-01379a7fb49ab2d638863891
    resource: repo://apps/decodex-gpui/src/agent_skills.rs
  - id: openwiki-source-0d053d4de2f668168f982629
    resource: repo://apps/decodex-gpui/src/agent_usage_evidence.rs
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-02bd3cfc9d4db9d1d1d8fd59
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences_tests.rs
  - id: openwiki-source-2b08587e9b26e203156110c8
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences.rs
  - id: openwiki-source-ecdf4586908bb6226955607a
    resource: repo://crates/decodex-codex/src/app_server_client/history_item.rs
  - id: openwiki-source-830a575f82404942511da59b
    resource: repo://crates/decodex-codex/src/app_server_client/prediction.rs
  - id: openwiki-source-787f8ad27b8519fbed2bd039
    resource: repo://crates/decodex-codex/src/app_server_client/thread_fork.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-4a57fba4bea69171318c6323
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_fork_tests.rs
  - id: openwiki-source-a0063c7b07a1bc990ee9af6c
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process.rs
  - id: openwiki-source-b2869e6da778a74cb6afc667
    resource: repo://crates/decodex-runtime/src/account_launch/process.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-e2f4e298ab0a4c683b92158d
    resource: repo://crates/decodex-runtime/src/account_service/personal_access_token.rs
  - id: openwiki-source-4e8482c0f2dbe370858a1b11
    resource: repo://crates/decodex-runtime/src/agent_context_settings_native_tests.rs
  - id: openwiki-source-720a745d503e0e60ebcad0c5
    resource: repo://crates/decodex-runtime/src/agent_detail.rs
  - id: openwiki-source-d8df4be72e86f8bd3d65cce8
    resource: repo://crates/decodex-runtime/src/agent_plugins.rs
  - id: openwiki-source-3d25cb6558fde84d70520ac2
    resource: repo://crates/decodex-runtime/src/agent_read_state.rs
  - id: openwiki-source-8b32cad13ab2428dd54bd986
    resource: repo://crates/decodex-runtime/src/agent_skills.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-a4a2658e06fa2cf48ff4589e
    resource: repo://crates/decodex-runtime/src/agent/global_guardian.rs
  - id: openwiki-source-a7886792ad3e47b8e1afb7fa
    resource: repo://crates/decodex-runtime/src/agent/guardian_details.rs
  - id: openwiki-source-419a87794b421322302c7d9e
    resource: repo://crates/decodex-runtime/src/agent/project.rs
  - id: openwiki-source-e32adebfd6d3bf27dc186bad
    resource: repo://crates/decodex-runtime/src/agent/tests/auth_recovery.rs
  - id: openwiki-source-30c7edf894a639091ecbafc6
    resource: repo://crates/decodex-runtime/src/agent/tests/external_context.rs
  - id: openwiki-source-4f59431f3f6264a65208bdcb
    resource: repo://crates/decodex-runtime/src/agent/work_delivery.rs
  - id: openwiki-source-b1dd4c8ab85f342314daa6f1
    resource: repo://crates/decodex-runtime/src/agent/work.rs
  - id: openwiki-source-2da6601c3f30e806c504e991
    resource: repo://crates/decodex-runtime/src/host_credentials.rs
  - id: openwiki-source-003f00974f49a9f106bbcbea
    resource: repo://database/schema.sql
  - id: openwiki-source-58c0167d31c3e40dc89591ab
    resource: repo://database/src/agent_fork.rs
  - id: openwiki-source-fc5ad70343bb20c4e9d12540
    resource: repo://database/src/agent_process/attachments.rs
generated: { by: "codex", at: "2026-10-27T21:12:13.971Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-28T02:37:26.121Z
---

# Product scope and native ownership

Decodex supplies a local workspace, account policy, work relationships and readable evidence. Codex supplies native conversation and tool execution. Adopt native capabilities through the admitted protocol rather than building a parallel execution system.

## Implemented coordination and Guardian

The [Project, Agent and Work contract](project-agent-work.md) is the accepted and implemented contract for general-purpose collaboration. It defines shared English names for code and UI, an optional Assistant, existing Agents acting as Project Leads, one owner per Work, native Subagents, and scoped Collaboration Requests between independent Agents. It does not use fixed L0/L1/L2 business levels. The current ownership boundary is documented below.

Global Guardian controls and native private Review Details are now selected product scope. They are no longer awaiting a decision on whether to implement them. The target has no per-Agent Guardian override; actual native requirements and per-source effective settings remain authoritative. Review Details are diagnostic observation, not permission or Work acceptance. Implementation, exact-artifact qualification and a separate local installed development acceptance are recorded in the retained evidence index. The installed scenario qualifies two Projects and four Works, independent-Agent input and approval ownership, Pause/Resume, restart recovery, global Guardian and the Workspace/Full Access sequence. Official Codex desktop Project presentation remains unobserved. A controlled scenario does not establish general model quality. This decision does not enable a native flag, broaden Full Access, or select unrelated historical optional proposals.

The default coordination surface uses `work_read` and `work_manage`. Only users create persistent Agents. Native Subagents own temporary execution. Managed-worker/manager creation and old goal/dependency tools are retired from new conversations and refused in legacy schemas. Native history and recorded tool declarations stay bound to their original conversations.

Global Guardian changes the thread preference for subsequent turns and reports per-source receipts. Existing active captures and pending human approvals retain native ownership. Optional private details retain bounded assistant output and terminal metadata without raw reasoning. Exact action associations are unavailable in the selected runtime and remain explicit. This does not select the separate native Guardian V3 experiment or add a per-Agent override.

The current schema and protocol 4.0 store the business model directly. Native Projects provide catalog identity and directory context; Decodex retains Work and responsibility associations. A native Project entry does not prove synchronization with desktop saved Projects. Work association is not a new execution sandbox or automatic directory switch.

The retained-conversation repair reconciles unassigned native Project membership before input and preserves conflicts for inspection. The current schema retains actual native child-input receipts for Work attribution when native configuration permits nested V2 Code Mode. Recovery joins exact evidence without replay. V1 nesting remains supported; V2 keeps its direct-only default. These are repairs to retained integration contracts. Native Project identity and membership and Decodex Project presentation passed in the separate installed qualification. Official desktop saved-Project/sidebar presentation remains unobserved. See the [canonical contract](project-agent-work.md#qualification-and-evidence-limits).

The lifecycle follow-up preserves this scope. The schema records independent Agent ownership on a shared admitted native process and distinguishes visible input from native task-name proof for encrypted assignments. Guardian history recovery uses supported native reads after restart and preserves missing action associations. These changes repair retained behavior; they add no new reviewer or helper executor. The later installed qualification also repaired independent Agents that lacked conversation controls or sent input to the first Agent. It records the exact development artifact, preserved data scope and three unexplained complete-suite cleanup markers separately from the earlier full-workspace pass. Isolated controls passed; they do not establish the cleanup cause.

## Selected read-only views

The selected scope includes three views in the existing desktop:

- **Native memory summary:** Read and copy the current loaded root conversation's native summary. Codex selects the memory version and availability. Decodex adds no memory writer, automatic retention policy or implicit resume.
- **Collaboration history:** Retain exact local question replies, their delivery and receiving-turn outcome, plus independent contribution cycles and explicit owner assessments. A reply or returned contribution does not establish resolution or complete Work. Missing external reply bodies remain unknown.
- **Actual usage:** Read known whole-turn token deltas and recorded duration for a persistent Agent or retained Work references. Shared Work references, missing counters and unobserved child usage remain explicit. Linked turns can contain other activity; recorded duration is not Work elapsed time and these values are not a bill.

These views are available through local protocol 4.0 and the same source owners. Their reusable regression identities, exact-artifact checks and limits are in `selected_read_views` in the [retained evidence index](../../automations/upstream/retained-evidence.json). Installed-app acceptance remains separate.

## Current optional decisions

Items 1–3 are delivered through the selected read-only views above. The user selected **item 4, persistent attached branches**. An explicit completed-turn branch uses native attachment, retains the exact source relationship and can be reopened from Agent preferences. Creation sends no input and copies no native Goal. The child-visible history starts empty, while native inference retains the reviewed parent context. Source turn IDs preserve boundary provenance. Native attachment requires paginated source history; the public branch flow already checks this. It adds no automatic follow-up, scheduling, result merge or deletion policy. The retained `retained_attached_forks` evidence identifies its regressions and qualification limits.

**Item 5, the native Guardian V3 experiment, waits for upstream maturity. Item 6, a custom managed configuration server, is deferred by the user.** Neither is selected for implementation. Existing global Guardian and native managed requirements keep their current scope. There is no outstanding new decision in this six-item list.

## Current integration boundary

| Area | Decodex responsibility |
| --- | --- |
| Conversations | Preserve native identity, canonical inputs, drafts and complete history export. Explicit native branches keep the original conversation; same-thread edits remain a separate choice. |
| Agent coordination | Keep one accountable owner per Work, current requirements, exact Results, assessment and scoped independent Collaboration. Native Subagents remain native execution. |
| Tools and connections | Show observed capabilities and origin, and select enabled native skills for input. Configure plugin installation and connections per account in Codex. Discovery does not install or enable tools. |
| Rich output | Render supported Markdown and structured content. The embedded MCP HTML viewer and its callback execution path are retired. |
| Approvals and recovery | Use exact current request identity and native enforcement. Unknown outcomes require reconciliation, not automatic replay. |
| Accounts | Admit only native `plus`, `pro`, `prolite`, `promax` and `team` plans. Keep multi-account routing and native credential ownership. Imported ChatGPT PAT accounts use their verified identity without synthetic OAuth expiry or refresh tokens. |
| Voice | Retain the subscription dictation and live paths described in the voice integration page. Source support is not an installed-app acceptance result. |

The bridge admits installed-plugin observations but rejects local plugin list/read/install/reconcile management and MCP configuration reload. Historical plugin observations remain readable; these records do not restore removed management controls.

Provider authentication recovery recording, formerly O24, is also retired. Saved recovery history remains readable without generating new notifications or outgoing work.

The desktop does not need a duplicate plugin marketplace, plugin publishing workflow or embedded browser to use these native capabilities. A local skill picker selects an already enabled skill; it does not reproduce plugin setup.

## Retained native additions

Tool and file-change details use exact native thread, turn and item identities. Only an explicit unsupported-method response permits the older history-read fallback. This detail path does not replace bulk transcript paging.

Eligible root conversations expose native read receipts in task preferences. Read and unread marks require the reviewed native revision and local source identity. Reading does not acknowledge a result, and these receipts do not replace Dock handoff or attention state.

The client also exposes a bounded, explicitly invoked ephemeral prediction fork. It inherits the loaded parent's context, tools and permissions. It is not a tool-free recap, an automatic background job, or a composer suggestion interface. Its caller must own authorization and event routing. The isolated recap path remains separate.

Native launch policy requests full-fork prefix preservation, model-specific subagent context defaults and Agent message wakeups. The retained description-first argument is accepted as a removed native feature, so current tool ordering stays native-owned. The native configuration still selects Code Mode hosting transport.

Background compaction and retention of delegation instructions are configurable native additions. Task preferences > Context defaults reads and writes the same native configuration used by manual `config.toml` edits. Explicit off values remain valid; saving a default does not restart loaded conversations. The panel distinguishes a saved user value from a project override.

The compatibility path requests retention only for `work_instruction` outputs. This covers initial delegation and explicit follow-up instructions while keeping application tool authority. Wakes, capacity retries and arbitrary external results remain unselected. Codex owns the retention decision and budget.

The context panel does not expose a switch to hide native collaboration tools, and launch policy does not force that native option. Replacing the model's native collaboration surface remains a separate product decision. Persistent Decodex work records and native subagent execution keep their current owners.

See [Runtime architecture](../architecture/runtime-architecture.md#context-defaults), [Agent coordination](../architecture/chief-coordination.md) and [Acceptance boundaries](../testing/upstream-acceptance-boundaries.md).

## Adoption and evidence

Inspect official upstream source and the schema generated by the installed binary before adding an adapter. Separate upstream capability, local implementation, automated checks and installed-app acceptance. A retained DTO or database receipt alone does not establish an active UI or writer.

The previous numbered optional-feature table was a dated review, not permanent permission to preserve every feature. Its original decisions remain in Git history. Keep this page focused on current boundaries and use focused integration pages for behavior.

See [Tools and integrations](../integrations/tools-plugins-and-apps.md), [Agent coordination](../architecture/chief-coordination.md), [Subscription voice](../integrations/subscription-voice.md) and [Acceptance boundaries](../testing/upstream-acceptance-boundaries.md).

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
  - id: openwiki-source-1bf529ea3f1649ac9a28334a
    resource: repo://apps/decodex-gpui/src/agent_conversation_tabs.rs
  - id: openwiki-source-77ad5024b567271a0f618fd7
    resource: repo://apps/decodex-gpui/src/agent_list_branch.rs
  - id: openwiki-source-89a6e0bcef9377afc01425eb
    resource: repo://apps/decodex-gpui/src/agent_list_status_tests.rs
  - id: openwiki-source-02f8a02cdfa0616877cdad62
    resource: repo://apps/decodex-gpui/src/agent_list_status.rs
  - id: openwiki-source-9c68c30b2ff24ddf9a764d74
    resource: repo://apps/decodex-gpui/src/agent_memory.rs
  - id: openwiki-source-b2084dffd07b4229957a0f94
    resource: repo://apps/decodex-gpui/src/agent_prompt_edit.rs
  - id: openwiki-source-5e245e8cc4db92f2dbe2ba47
    resource: repo://apps/decodex-gpui/src/agent_prompt_fork.rs
  - id: openwiki-source-0f9840ddf9e841a062aa905a
    resource: repo://apps/decodex-gpui/src/agent_prompt_suggestions.rs
  - id: openwiki-source-56b561d7e6dd906d218aa9d1
    resource: repo://apps/decodex-gpui/src/agent_read_state.rs
  - id: openwiki-source-296a92a9cefecb19f0fc1f95
    resource: repo://apps/decodex-gpui/src/agent_sample_tests.rs
  - id: openwiki-source-309d7ec654ab4d531c906212
    resource: repo://apps/decodex-gpui/src/agent_sample_ui.rs
  - id: openwiki-source-01379a7fb49ab2d638863891
    resource: repo://apps/decodex-gpui/src/agent_skills.rs
  - id: openwiki-source-5606e779f593d160176b0a27
    resource: repo://apps/decodex-gpui/src/agent_transcript.rs
  - id: openwiki-source-0d053d4de2f668168f982629
    resource: repo://apps/decodex-gpui/src/agent_usage_evidence.rs
  - id: openwiki-source-3fa6d6c49e3542409542045b
    resource: repo://apps/decodex-gpui/src/agent_work_browser.rs
  - id: openwiki-source-59ed67d26d1c310707a365e5
    resource: repo://apps/decodex-gpui/src/component_plugins.rs
  - id: openwiki-source-682f90c6d08ed3207257f1d5
    resource: repo://apps/decodex-gpui/src/conversations.rs
  - id: openwiki-source-a16804a567889414868e5d67
    resource: repo://apps/decodex-gpui/src/settings_surface.rs
  - id: openwiki-source-5afd606185b0f01d8fc4f429
    resource: repo://apps/decodex-gpui/src/shell_diagnostics.rs
  - id: openwiki-source-2986b39185cca5c00a29ad1d
    resource: repo://apps/decodex-gpui/src/shell_status.rs
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-3127fd4c7b2aefcc9cea7961
    resource: repo://automations/upstream/retained-contracts.json
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
  - id: openwiki-source-e379b8a714d71f87371ae328
    resource: repo://crates/decodex-components/src/plugins.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-0d29990b5fd295fa89a495ad
    resource: repo://crates/decodex-protocol/src/model_catalog.rs
  - id: openwiki-source-84719462dc44acb0bf71023b
    resource: repo://crates/decodex-runtime/src/account_api.rs
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
  - id: openwiki-source-5bc08ad67fcdb104dfcde7bf
    resource: repo://crates/decodex-runtime/src/agent_permissions.rs
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
generated: { by: "codex", at: "2026-10-30T15:17:49.972Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-30T20:00:55.995Z
---

# Product scope and native ownership

Decodex supplies a local workspace, account policy, work relationships and readable evidence. Codex supplies native conversation and tool execution. Adopt native capabilities through the admitted protocol rather than building a parallel execution system.

## Implemented coordination and Guardian

The [Project, Agent and Work contract](project-agent-work.md) is the accepted and implemented contract for general-purpose collaboration. It defines shared English names for code and UI, an optional Assistant, existing Agents acting as Project Leads, one owner per Work, native Subagents, and scoped Collaboration Requests between independent Agents. It does not use fixed L0/L1/L2 business levels. The current ownership boundary is documented below.

Native permissions and private Review Details are selected product scope. Each conversation owns its permission mode; the global Guardian control only enables optional review records. Review Details are observation, not permission or Work acceptance. The retained contract catalog records an earlier installed scenario with two Projects, four Works, independent-Agent input and approval ownership, Pause/Resume, restart recovery and a Workspace/Full Access sequence. That historical qualification does not establish the current installed artifact or general model quality. Official Codex desktop Project presentation remains unobserved.

The default coordination surface uses `work_read` and `work_manage`. Only users create persistent Agents. Native Subagents own temporary execution. Managed-worker/manager creation and old goal/dependency tools are retired from new conversations and refused in legacy schemas. Native history and recorded tool declarations stay bound to their original conversations.

Permission changes use the selected native conversation's supported profile and reviewer settings. Existing child captures and pending human approvals retain native ownership. Optional private details retain bounded assistant output and terminal metadata without raw reasoning. Exact action associations are unavailable in the selected runtime and remain explicit. This does not select the separate native Guardian V3 experiment.

The current schema and protocol 4.3 store the business model directly. Native Projects provide catalog identity and directory context; Decodex retains Work and responsibility associations. A native Project entry does not prove synchronization with desktop saved Projects. Work association is not a new execution sandbox or automatic directory switch.

The retained-conversation repair reconciles unassigned native Project membership before input and preserves conflicts for inspection. The current schema retains actual native child-input receipts for Work attribution when native configuration permits nested V2 Code Mode. Recovery joins exact evidence without replay. V1 nesting remains supported; V2 keeps its direct-only default. These are repairs to retained integration contracts. Native Project identity and membership and Decodex Project presentation passed in the separate installed qualification. Official desktop saved-Project/sidebar presentation remains unobserved. See the [canonical contract](project-agent-work.md#qualification-and-evidence-limits).

The lifecycle follow-up preserves this scope. The schema records independent Agent ownership on a shared admitted native process and distinguishes visible input from native task-name proof for encrypted assignments. Guardian history recovery uses supported native reads after restart and preserves missing action associations. These changes repair retained behavior; they add no new reviewer or helper executor. The later installed qualification also repaired independent Agents that lacked conversation controls or sent input to the first Agent. It records the exact development artifact, preserved data scope and three unexplained complete-suite cleanup markers separately from the earlier full-workspace pass. Isolated controls passed; they do not establish the cleanup cause.

## Selected read-only views

The selected scope includes three views in the existing desktop:

- **Native memory summary:** Read and copy the current loaded root conversation's native summary. Codex selects the memory version and availability. Decodex adds no memory writer, automatic retention policy or implicit resume.
- **Collaboration history:** Retain exact local question replies, their delivery and receiving-turn outcome, plus independent contribution cycles and explicit owner assessments. A reply or returned contribution does not establish resolution or complete Work. Missing external reply bodies remain unknown.
- **Actual usage:** Read known whole-turn token deltas and recorded duration for a persistent Agent or retained Work references. Shared Work references, missing counters and unobserved child usage remain explicit. Linked turns can contain other activity; recorded duration is not Work elapsed time and these values are not a bill.

These views are available through local protocol 4.3 and the same source owners. Their reusable regression identities, exact-artifact checks and limits are in `selected_read_views` in the [stable retained contract catalog](../../automations/upstream/retained-contracts.json). Installed-app acceptance remains separate.

## Current optional decisions

In the earlier six-item review, items 1–3 are delivered through the selected read-only views above. The user selected **item 4, persistent attached branches**. An explicit completed-turn branch uses native attachment, retains the exact source relationship and can be reopened from Agent preferences. Creation sends no input and copies no native Goal. The child-visible history starts empty, while native inference retains the reviewed parent context. Source turn IDs preserve boundary provenance. Native attachment requires paginated source history; the public branch flow already checks this. It adds no automatic follow-up, scheduling, result merge or deletion policy. The retained `retained_attached_forks` evidence identifies its regressions and qualification limits.

The user also selected multiple Side chats in the conversation header. The source and open Side chats share a horizontal tab strip. **+** reviews the latest completed source turn before **Create side chat** confirms an attached fork. Switching retains each draft and history position. Closing hides a tab without deleting the native branch; Agent preferences can reopen it. This presentation reuses the selected attachment boundary and adds no runtime or scheduling policy.

**Ask separately** remains a one-shot question. Direct composer, completed-response and text-selection actions open its shared popover. It preserves the main draft and reading position and does not create a Side chat. See [conversation presentation](../architecture/conversation-presentation.md).

**Item 5, the native Guardian V3 experiment, waits for upstream maturity** ([issue #2563](https://github.com/acg-box/decodex/issues/2563)). **Item 6, a custom managed configuration server, is deferred by the user** ([issue #2564](https://github.com/acg-box/decodex/issues/2564)). Neither is selected for implementation. Existing global Guardian and native managed requirements keep their current scope. There is no outstanding new decision in this six-item list.

## Approved desktop refinements

The later manual review approved original items **2, 3, 6, 7, 9, 10, 12 and 15**. These are delivered in [PR 2537](https://github.com/acg-box/decodex/pull/2537):

| Original item | Retained capability |
| --- | --- |
| 2 | Reviewed native branching from a locked conversation, with source and draft preservation. |
| 3 | Bounded structured math, including aligned equations, boxed models and grouped scripts. |
| 6 | Host hour-cycle and time-zone formatting for reset and expiry labels. |
| 7 | Clickable prose URLs with complete destinations and literal code handling. |
| 9 | Native V3 public reasoning summaries supplied as quiet voice context. |
| 10 | A call stays with its owner while the user browses other conversations. |
| 12 | Unsent question drafts return to the composer after exact terminal turn evidence. |
| 15 | Rich streaming previews withhold unfinished link destinations until closure, newline or completion. |

**Original items 13 and 31, board-only native messaging and the related coordination choice, remain undecided** in [issue 2539](https://github.com/acg-box/decodex/issues/2539). The current native collaboration surface stays available. These eight approvals do not select other historical proposals.

The [retained contracts](../../automations/upstream/retained-contracts.json) and [validation index](../../automations/upstream/retained-evidence.json) record source references and qualified limits. The implementation used focused desktop, storage and wire tests. It did not install or release an application or add a new live-provider or physical-audio qualification.

## Model retirement and native catalogs

The user selected **original item 28**: use the current native catalog default after an explicit model retirement deadline. An upgrade suggestion or a missing entry alone does not establish retirement. New Agent settings and idle loaded Agent recovery use the existing native settings and receipt paths. The desktop no longer cycles through a fixed model or reasoning-effort list when a current catalog is unavailable.

The native model catalog already refreshes automatically; this work does not add another model directory, poller or provider. See [model retirement](../workflows/models-and-settings.md#native-model-retirement) for the preserved settings and validation limits, and `retained_model_retirement_defaults` in the [retained contracts](../../automations/upstream/retained-contracts.json).

Diagnostic-log upload (**original item 24**) is excluded at the user's request to protect privacy. Terminal queue-navigation keys, copy-on-select and additional semantic range-copy gestures (**items 16, 17 and 25**) are not selected. Clipboard latency (**item 22**) requires a reproduced defect before maintenance work starts. Existing desktop copy behavior remains supported.

## Next-message suggestions and current choices

The user selected **original item 26** with automatic generation, controlled by **Settings > Agents > Conversation assistance**: a newly completed answer in the selected independent root conversation requests one suggestion when its composer is empty. Opening saved history, repeated refreshes and an existing draft do not trigger inference. The composer menu retains **Suggest next message…** for an explicit request. Press **Tab** in the focused empty message field to place the current suggestion in the composer as one undoable edit. Other fields, Shift-Tab and nonempty drafts keep normal focus navigation. Later edits are preserved, and acceptance does not send a message. The switch defaults on. Disabling prevents new automatic requests, and re-enabling does not replay answers completed while disabled. The feature uses the existing native sampler and conversation refresh; it adds no separate poller. See [the suggestion workflow](../workflows/conversations-and-recovery.md#next-message-suggestions) and `retained_prompt_suggestions` in the [retained contracts](../../automations/upstream/retained-contracts.json).

**Original items 19 and 69**, authenticated remote execution, remain deferred in [issue 1693](https://github.com/acg-box/decodex/issues/1693). **Item 21** retains Decodex account management: independent account observations and reset-credit requests use the existing account API client. This decision does not make those operations inherit native `application.network` policy.

**Item 27 is deferred until the final research review**, tracked in [issue #2562](https://github.com/acg-box/decodex/issues/2562). Do not implement or enable deferred mailbox preemption from this review. The user rejected the proposed schema-size risk indication for **item 30**; its diagnostic gap remains tracked in [issue 2546](https://github.com/acg-box/decodex/issues/2546). Do not add that indication or automatic budget tuning. **Item 29** is selected as a per-server switch in **Settings > Agents > Native defaults**. It edits only an existing user-configured server’s native `startup_readiness`: `catalog` when on, `connection` when off. A valid native cache can satisfy startup; tool execution still needs a live connection. The configuration roundtrip qualifies the setting boundary, not MCP cache performance or installed-app behavior. No global enablement is selected.

## Selected conversation improvements

The current selected desktop scope includes these original review items:

| Item | Retained behavior |
| --- | --- |
| 39 | One Decodex cloud identity for work, loading and the welcome entrance, including menu-bar loading. |
| 42 | A color-only working-text sweep with a static reduced-motion presentation. |
| 50 | Reconnect the same live, saved, never-dispatched follow-up slot. Changed sources, uncertain delivery, pending requests and compaction keep their existing fences. |
| 65 | Find in loaded retained conversation text, including folded activity and the current live revision, with explicit earlier-history loading. |
| 78 | Native child model and effort with distinct resolved-creation, requested-legacy and current-thread provenance. |

The user also approved these bounded additions:

| Item | Retained behavior |
| --- | --- |
| 35 | Locate an exact known item in the main conversation. Reuse loaded content; read an unloaded same-turn window through native anchors and cursors. Keep the return action in the header and omit per-message nearby-history buttons. |
| 43 | Show the native-retained command output line count while output is collapsed. |
| 67 | Find conversations with the same registered native resource type and identity, including archive filtering and paging. No historical upload or PDF scan is selected. |
| 72 | Show existing provider cost estimates in the conversation header and Menu Bar, with source account, unknown values and explicit refresh. No account total or local pricing engine is selected. |

Item 67 uses the current shared native store. Exact current local conversations can open; hidden archived, replaced or unmanaged owners provide a copied native reference. This does not adopt external conversations or create new associations. **Item 71** is deferred in [issue 2549](https://github.com/acg-box/decodex/issues/2549). **Item 77** is not selected. Items 57 and 64 add no speaker or channel configuration.

**Item 36** is covered by the existing Decodex warning system. Exact unchanged notices stay dismissed until their source changes or recovers; opening Settings does not reset dismissal. This does not select a new native warning viewer or pin controls.

**Items 32, 33, 37, 40, 52, 54, 56 and 81 are excluded.** Codex owns its app-server self-diagnostics. Decodex diagnoses its own product and integration failures. The standalone Codex startup-configuration UI, query and dedicated qualification helper are retired. Diagnostic export and upload capabilities are not selected. Ordinary conversation Markdown export and normal error or warning presentation remain retained.

The manual proposal records have current dispositions. Deferred research does not authorize implementation, experimental activation, a new setting or daily maintenance debt. Microphone selection does not select independent output-device or input-channel controls. The [current contracts](../../automations/upstream/retained-contracts.json) retain approvals, deferrals, exclusions and duplicates. The [qualification index](../../automations/upstream/retained-evidence.json) separates executed Decodex tests, source inspection, unchanged runtime identities and installed-app limits. The automation remains paused until explicit user authorization resumes it.

## Conversation list and remaining manual scope

**Item 59 is delivered** in [PR 2559](https://github.com/acg-box/decodex/pull/2559). A locally managed conversation row offers **Branch…**. The action opens that exact source, reads its latest completed native turn and reuses the existing explicit after-turn confirmation. Source and unrelated drafts survive navigation and cancellation. Creation sends no input. Native inventory rows without a local managed source gain no adoption path. The focused client checks do not establish installed-app or live-provider acceptance.

**Item 74 is implemented** as independent status filters in the existing conversation browser and Project-grouped Work list. Each view saves its selection through the existing host-local presentation preference owner and offers **All statuses** to clear it. The filter reuses displayed status facts and combines with Project and search filters. Exact row opening, source drafts and Work details retain their existing owners. It adds no model grouping, command center, execution policy or native runtime change. Five focused GPUI tests and affected static checks passed; physical app restart, installed UI and live-provider acceptance remain separate. The [portable evidence](../../automations/upstream/retained-evidence.json) records the source and limits, and native maintenance state owns merge reconciliation.

The following native options remain deferred research:

| Item | Scope | Tracking |
| --- | --- | --- |
| 49 | Guardian selects root evidence around native child handoffs. | [Issue 2556](https://github.com/acg-box/decodex/issues/2556) |
| 66 | Fresh native Subagent client-tool inheritance and upstream readiness. | [Issue 2553](https://github.com/acg-box/decodex/issues/2553) |
| 68 | Native Guardian decision-comparison readiness. | [Issue 2554](https://github.com/acg-box/decodex/issues/2554) |
| 70 | Native managed-worktree tools, considered with LayerFS research. | [Issue 2555](https://github.com/acg-box/decodex/issues/2555) |
| 79 | Structured JSON evidence for native Guardian review. | [Issue 2557](https://github.com/acg-box/decodex/issues/2557) |

Items 44 and 82 are not selected. Item 46 shares the retired parser scope of item 38. Item 58 adds no separate next-message suggestion feature. The ignored native async-classifier override in item 62 must not become an ineffective setting; it is distinct from the retained background-compaction control.

Conditional research on [account-safe cross-process MCP sharing](https://github.com/acg-box/decodex/issues/2524) and [a supported complete critical-path interface](https://github.com/acg-box/decodex/issues/2525) also retains its existing issues. These records preserve deferred scope; they do not authorize implementation or add daily maintenance debt.

## Native compatibility maintenance

Keep native execution, enforcement and algorithms in Codex. Decodex maintains its actual launch, configuration, custom-tool routing, authorization, protocol and presentation consumers. Native source inclusion alone does not establish configured activation, supported client behavior or artifact qualification.

| Item | Retained Decodex boundary |
| --- | --- |
| 55 | Keep launch environment and verified helpers correct; native Codex restores login-shell PATH. |
| 60 | Preserve the native model source and configuration boundary; retain one model-context catalog owner. |
| 73 | Keep supported client-tool routing, callback content and authorization if strict Code Mode exposure is explicitly selected later. |
| 75 | Check supported native/model capability and the current client-tool discovery boundary if ranked search is explicitly selected later. |
| 76 | Preserve real parent/root turn attribution where the supported caller contract requires it. Work relationships do not establish native turn causality. |

Items 45 and 48 also retain native ownership. These decisions select no replacement executor or search algorithm and enable no experiment. Source inspection has not reproduced an item-76 defect; retain the narrow caller-attribution check without claiming scenario qualification. Run affected Decodex checks when a concrete integration change requires proof.

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

The desktop does not need a duplicate plugin marketplace, plugin publishing workflow or embedded browser to use these native capabilities. A local skill picker selects an already enabled skill. The explicit [EGM Component](../integrations/elephant-goldfish.md) controls invoke the original native plugin lifecycle; they do not reopen generic plugin management through the Agent bridge.

## Retained native additions

Tool and file-change details use exact native thread, turn and item identities. Only an explicit unsupported-method response permits the older history-read fallback. This detail path does not replace bulk transcript paging.

Eligible root conversations expose native read receipts in task preferences. Read and unread marks require the reviewed native revision and local source identity. Reading does not acknowledge a result, and these receipts do not replace Dock handoff or attention state.

The client also exposes a bounded, explicitly invoked ephemeral prediction fork. It inherits the loaded parent's context, tools and permissions. It is not a tool-free recap or an automatic background job. Its caller must own authorization and event routing. Next-message suggestions use the separate native sampling path described above; the isolated recap path remains separate.

Native launch policy requests full-fork prefix preservation, model-specific subagent context defaults and Agent message wakeups. The retained description-first argument is accepted as a removed native feature, so current tool ordering stays native-owned. The native configuration still selects Code Mode hosting transport.

Background compaction and retention of delegation instructions are configurable native additions. Settings > Agents > Native defaults and Agent preferences > Context defaults read and write the same native configuration used by manual `config.toml` edits. Explicit off values remain valid; saving a default does not restart loaded conversations. The panel distinguishes a saved user value from a project override.

The compatibility path requests retention only for `work_instruction` outputs. This covers initial delegation and explicit follow-up instructions while keeping application tool authority. Wakes, capacity retries and arbitrary external results remain unselected. Codex owns the retention decision and budget.

The context panel does not expose a switch to hide native collaboration tools, and launch policy does not force that native option. Replacing the model's native collaboration surface remains a separate product decision. Persistent Decodex work records and native subagent execution keep their current owners.

See [Runtime architecture](../architecture/runtime-architecture.md#context-defaults), [Agent coordination](../architecture/chief-coordination.md) and [Acceptance boundaries](../testing/upstream-acceptance-boundaries.md).

## Adoption and evidence

Inspect official upstream source and the schema generated by the installed binary before adding an adapter. Separate upstream capability, local implementation, automated checks and installed-app acceptance. A retained DTO or database receipt alone does not establish an active UI or writer.

The previous numbered optional-feature table was a dated review, not permanent permission to preserve every feature. Its original decisions remain in Git history. Keep this page focused on current boundaries and use focused integration pages for behavior.

See [Tools and integrations](../integrations/tools-plugins-and-apps.md), [Agent coordination](../architecture/chief-coordination.md), [Subscription voice](../integrations/subscription-voice.md) and [Acceptance boundaries](../testing/upstream-acceptance-boundaries.md).

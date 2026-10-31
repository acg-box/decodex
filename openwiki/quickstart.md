---
type: Reference
title: "OpenWiki quickstart"
description: "Task entrypoints for current native conversations, account routing, product scope and evidence."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-d700ef551f46158044378d8f
    resource: repo://apps/decodex-cli/src/lib.rs
  - id: openwiki-source-4e2d2760a1fe5d904918afb7
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ConversationEstimatesView.swift
  - id: openwiki-source-bf03f317e64cfefd11f6ea83
    resource: repo://apps/decodex-gpui/src/agent_attached_branches.rs
  - id: openwiki-source-3708e6cf9324a8ff1f6c39ae
    resource: repo://apps/decodex-gpui/src/agent_collaboration_tests.rs
  - id: openwiki-source-2a6989c7fa25b9681df71f95
    resource: repo://apps/decodex-gpui/src/agent_collaboration.rs
  - id: openwiki-source-c76550e85b25876b339f9255
    resource: repo://apps/decodex-gpui/src/agent_context_settings.rs
  - id: openwiki-source-1bf529ea3f1649ac9a28334a
    resource: repo://apps/decodex-gpui/src/agent_conversation_tabs.rs
  - id: openwiki-source-77ad5024b567271a0f618fd7
    resource: repo://apps/decodex-gpui/src/agent_list_branch.rs
  - id: openwiki-source-02f8a02cdfa0616877cdad62
    resource: repo://apps/decodex-gpui/src/agent_list_status.rs
  - id: openwiki-source-c6ac7d6b2fdeb458faab2c3a
    resource: repo://apps/decodex-gpui/src/agent_permissions.rs
  - id: openwiki-source-6a1c55b61be8ac251a493046
    resource: repo://apps/decodex-gpui/src/agent_portfolio.rs
  - id: openwiki-source-0eea3c2bab20e4a9d8e3d80a
    resource: repo://apps/decodex-gpui/src/agent_prompt_confirm_wire_tests.rs
  - id: openwiki-source-b2084dffd07b4229957a0f94
    resource: repo://apps/decodex-gpui/src/agent_prompt_edit.rs
  - id: openwiki-source-309d7ec654ab4d531c906212
    resource: repo://apps/decodex-gpui/src/agent_sample_ui.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-2fb9e466c6aafdf4f4950bc1
    resource: repo://apps/decodex-gpui/src/agent_timeline_find.rs
  - id: openwiki-source-51a4755f3c4ddd78511e5c8e
    resource: repo://apps/decodex-gpui/src/agent_tree.rs
  - id: openwiki-source-40c9c6a55485557343853517
    resource: repo://apps/decodex-gpui/src/agent_usage_estimates.rs
  - id: openwiki-source-3fa6d6c49e3542409542045b
    resource: repo://apps/decodex-gpui/src/agent_work_browser.rs
  - id: openwiki-source-5d032c6be684964aa2c401fa
    resource: repo://apps/decodex-gpui/src/agent_workspace.rs
  - id: openwiki-source-a2f254020dd8eaabd6366c91
    resource: repo://apps/decodex-gpui/src/session_settings.rs
  - id: openwiki-source-0813b2e35e2133631c0aa4db
    resource: repo://apps/decodex-gpui/src/settings_components.rs
  - id: openwiki-source-37be5bb3dc64279420e57b31
    resource: repo://apps/decodex-gpui/src/settings_native.rs
  - id: openwiki-source-a16804a567889414868e5d67
    resource: repo://apps/decodex-gpui/src/settings_surface.rs
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-7dcbb082d2502f1ec4386c39
    resource: repo://automations/upstream/prompts/maintainer.md
  - id: openwiki-source-3127fd4c7b2aefcc9cea7961
    resource: repo://automations/upstream/retained-contracts.json
  - id: openwiki-source-787f8ad27b8519fbed2bd039
    resource: repo://crates/decodex-codex/src/app_server_client/thread_fork.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-4a57fba4bea69171318c6323
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_fork_tests.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-ff8abff5a675874c627f1969
    resource: repo://crates/decodex-runtime/src/account_service/tests/plan_scope.rs
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-003f00974f49a9f106bbcbea
    resource: repo://database/schema.sql
  - id: openwiki-source-dc8958560b1230967470835f
    resource: repo://database/src/schema.rs
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
generated: { by: "codex", at: "2026-10-31T14:39:55.131Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-31T14:39:55.131Z
---

# OpenWiki quickstart

Decodex is a local workspace above Codex app-server. The primary Agent can discuss work, act directly, or coordinate other agents. Codex owns native conversation execution. Decodex owns local work relationships, account policy, recovery and presentation. “Chief” in older filenames and records is the former Agent name.

## Project, Agent and Work

The [canonical contract](decisions/project-agent-work.md) is implemented: Project provides continuing context, each user-created Agent has one current native conversation, and Work records an outcome with one accountable Agent, current requirements and evidence. Assistant is optional. Project Lead is a responsibility of an existing Agent. The same Agent can handle several Works.

Agents use native Subagents for bounded execution and scoped Collaboration Requests for independent peers. Native Codex owns execution, context, permissions and continuation. Decodex owns durable business relationships, evidence, coordination and presentation. The default Work tools do not create another executor, and no part of this contract requires Ultra.

Work and Guardian fit the existing desktop. Expand Work to inspect Materials, Collaboration, Results and assessment. The composer shield selects native conversation permissions; Settings > Agents defines defaults for new conversations. Guardian keeps optional bounded Review Details and does not override conversation permissions. Review records with no exact action association stay explicitly unassociated.

The current SQLite schema preserves native input receipts, independent process ownership and exact assignment evidence. Conversations can start without an Assistant. Native Project reconciliation, assignment recovery and Guardian detail recovery keep native execution in Codex. See the [acceptance boundaries](testing/upstream-acceptance-boundaries.md) for historical source-built, installed-app and online voice qualifications. Those records do not qualify a later artifact.

**Settings > Agents** groups background compaction, delegation-instruction retention, per-server cached MCP catalogs and automatic next-message suggestions. Native defaults use the selected connected Agent’s project and the same native configuration as Agent preferences. See [retained switches](workflows/models-and-settings.md#find-the-retained-switches) for their scope and other existing settings.

## Memory, collaboration and usage views

- **Agent preferences > Memory summary** reads and copies the loaded native summary on request. Native Codex selects the version; empty and unavailable states stay explicit. See [models and settings](workflows/models-and-settings.md#read-a-native-memory-summary).
- The existing **Collaboration** panel retains exact local question replies, delivery outcomes and independent contribution/repair cycles. A reply or completed receiving turn does not imply resolution. See [coordination](architecture/chief-coordination.md).
- **Actual usage** in Agent preferences and Work details shows known native token deltas and duration with source coverage. Shared or missing Work attribution stays explicit; this is not billing or complete Work elapsed time.

These views preserve unknown coverage and resolution limits. Their retained qualification is in [the acceptance boundary](testing/upstream-acceptance-boundaries.md#selected-read-only-views). Runtime selection, source-review state, installed artifacts and schedules remain separate.

The conversation browser and Work list have independent saved **status filters**, combined with the existing Project and search filters. Choose **All statuses** to clear a status choice without changing a draft or Work state. See [list filtering](workflows/conversations-and-recovery.md#filter-existing-lists-by-status).

## Side chats and separate questions

The conversation header holds the source and its open Side chats as tabs. Use **+** to review a Side chat from the latest completed source turn, then confirm **Side chat**. Each tab keeps its own draft and reading position. Closing a tab retains the native branch; reopen it from **Agent preferences → Side chats**.

For one quick question, use **Ask separately** in the composer toolbar, the question action beside a completed response, or **Ask about selection**. The popover preserves the main draft. See [the conversation workflow](workflows/conversations-and-recovery.md#open-side-chats-in-tabs) for both forms and their native boundaries.

## Find and reconnect

Use **Cmd-F** or the conversation header's search control to find loaded conversation text, including folded activity. **Load earlier and search** extends the existing history window; shortened output remains incomplete. The current match keeps its text revision during refresh and resize. See [Find and saved-send recovery](workflows/conversations-and-recovery.md#find-conversation-text) for keyboard ownership and the live never-dispatched reconnect slot. Ordinary offline drafts and uncertain deliveries are not automatically sent.

Choose **Show in conversation** in a tool detail to locate a loaded message or read its unloaded native history in the main transcript. The history window has native earlier/later pages and a fixed **Return to conversation** action. **Related conversations** in Resources uses registered resource identities, archive filtering and native paging. It opens existing local conversations or copies an unmanaged native reference. It does not scan historical uploads. See [history and resource lookup](workflows/conversations-and-recovery.md#read-items-near-a-known-message).

Reported dollars appear beside **Context** in the composer; **Conversation estimates** in Menu Bar retains explicit refresh and source-account labels. Missing dollar values stay hidden. The Menu Bar list covers up to eight recent locally managed conversations, not account-wide spending. See [estimate scope](workflows/models-and-settings.md#conversation-cost-estimates).

## Branch from a list row

Choose **Fork** on an existing locally managed conversation row to review its latest completed native turn. Confirm **Fork** to use the ordinary after-turn branch path. Source drafts stay preserved, and creation sends no input. Unavailable native history and unmanaged rows cannot provide this shortcut. See [the list workflow](workflows/conversations-and-recovery.md#branch-from-the-conversation-list) and [its qualification](testing/upstream-acceptance-boundaries.md#conversation-list-branch-qualification).

If a conversation uses a different provider from the current configuration, **Fork** reviews a new branch on the current provider. The original history and draft remain, and creation sends no input. See [provider recovery](workflows/conversations-and-recovery.md#continue-after-a-provider-change).

## Persistent attached branches

When reviewing an earlier message, choose **Side chat** to create a persistent native branch from its completed-turn prefix. The source history, draft and Goal remain unchanged. The child view starts empty while native inference retains the parent context. Creation requires paginated source history, sends no input and copies no native Goal. Reopen a verified branch through **Agent preferences > Side chats > Open**. See [conversation recovery](workflows/conversations-and-recovery.md#edit-or-branch-at-an-earlier-input) and the [exact-artifact qualification](testing/upstream-acceptance-boundaries.md#persistent-attached-branches).

## Start by task

| Task | Read first |
| --- | --- |
| Change Project/Agent/Work contracts, recovery, global Guardian or canonical names | [Project, Agent and Work contract](decisions/project-agent-work.md) |
| Understand service, clients and storage | [Runtime architecture](architecture/runtime-architecture.md) |
| Trace native helper execution, peer Collaboration, question escalation or recovery | [Agent coordination](architecture/chief-coordination.md) |
| Change conversation layout, loading or motion | [Conversation presentation](architecture/conversation-presentation.md), [Desktop workspace](architecture/desktop-workspace.md) |
| Change input, branches, drafts, read state, export or recap | [Conversations and recovery](workflows/conversations-and-recovery.md) |
| Change model, effort, Goal, search or native launch defaults | [Models and settings](workflows/models-and-settings.md) |
| Handle an approval or provider question | [Approvals and ownership](workflows/approvals-and-native-ownership.md) |
| Install, update or remove original EGM | [Elephant and Goldfish component](integrations/elephant-goldfish.md) |
| Use native skills or understand plugin and connection ownership | [Tools and integrations](integrations/tools-plugins-and-apps.md) |
| Change dictation, live voice or voice preferences | [Subscription voice](integrations/subscription-voice.md) |
| Import OAuth/PAT accounts or diagnose routing and activation | [Accounts and routing](operations/accounts-and-routing.md) |
| Diagnose storage | [Local database](operations/local-database.md) |
| Decide which upstream capabilities belong here | [Product scope](decisions/upstream-product-scope.md) |
| Review an upstream change | [Upstream maintenance](operations/codex-upstream-autopilot.md) |
| Choose checks and understand their limits | [Commands](operations/commands-and-validation.md), [acceptance boundaries](testing/upstream-acceptance-boundaries.md) |
| Improve tests without weakening contracts | [Test quality](testing/test-quality.md) |
| Maintain documentation | [Wiki maintenance](operations/wiki-maintenance.md) |

## Current boundaries

- `decodex serve` is the service and SQLite product-state owner. GPUI and CLI use typed local clients. The bundled helper is `decodex`, not `decodexd`.
- The protocol crate’s `CURRENT_VERSION`, `database/schema.sql` and `database/src/schema.rs` define the exact current local format. New stores start with that schema; startup does not run a historical migration chain.
- The Agent graph presents work relationships; the right Collaboration panel keeps recorded exchanges and pending response requests visible across chats. Expanding an exchange preserves the active chat and draft. Historical Factory diagrams do not define the current desktop.
- Account admission accepts native `plus`, `pro`, `prolite`, `promax` and `team` plans. OAuth and PAT use the same plan scope; native identity, quota and entitlement remain authoritative. See [Accounts](operations/accounts-and-routing.md).
- Account Route is synchronous and service-owned. Shared-auth liveness, exact source identity and readback govern completion. Same-account refresh adopts a valid non-older native winner instead of restoring a losing token.
- Unknown submission outcomes require exact receipts or native history. They do not authorize replay. Unavailable conversations keep readable history.
- Generic plugin management and embedded MCP HTML widgets are retired. The explicit EGM component uses the native Codex plugin lifecycle through Settings > Components. Codex owns configuration and execution; Decodex retains tool observations, native forms and readable history.
- The checked-in automation portfolio keeps the upstream maintainer **PAUSED** during manual catch-up and until explicit resume approval. The daily cadence is retained. This is configuration evidence, not scheduler readback. Documentation generation does not enable automations, install an app or publish a release.

## Build and evidence

Use stable Rust and the repository-owned commands. macOS packaging uses the selected Xcode installation or an explicit `DEVELOPER_DIR`. A unit test, native fixture, signed desktop check, merge and installation prove different things. Keep version-specific limits visible.

Maintain documentation through OpenWiki. Consolidate by current system behavior and reader tasks; do not recreate a page for each retired document. Previous plans and revision-specific receipts remain in Git history. They are not a current work queue.

---
type: Reference
title: "OpenWiki quickstart"
description: "Task entrypoints for current native conversations, account routing, product scope and evidence."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-d700ef551f46158044378d8f
    resource: repo://apps/decodex-cli/src/lib.rs
  - id: openwiki-source-bf03f317e64cfefd11f6ea83
    resource: repo://apps/decodex-gpui/src/agent_attached_branches.rs
  - id: openwiki-source-3708e6cf9324a8ff1f6c39ae
    resource: repo://apps/decodex-gpui/src/agent_collaboration_tests.rs
  - id: openwiki-source-2a6989c7fa25b9681df71f95
    resource: repo://apps/decodex-gpui/src/agent_collaboration.rs
  - id: openwiki-source-6a1c55b61be8ac251a493046
    resource: repo://apps/decodex-gpui/src/agent_portfolio.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-51a4755f3c4ddd78511e5c8e
    resource: repo://apps/decodex-gpui/src/agent_tree.rs
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
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
generated: { by: "codex", at: "2026-10-26T14:39:55.302Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-27T06:15:00.380Z
---

# OpenWiki quickstart

Decodex is a local workspace above Codex app-server. The primary Agent can discuss work, act directly, or coordinate other agents. Codex owns native conversation execution. Decodex owns local work relationships, account policy, recovery and presentation. “Chief” in older filenames and records is the former Agent name.

## Project, Agent and Work

The [canonical contract](decisions/project-agent-work.md) is implemented: Project provides continuing context, each user-created Agent has one current native conversation, and Work records an outcome with one accountable Agent, current requirements and evidence. Assistant is optional. Project Lead is a responsibility of an existing Agent. The same Agent can handle several Works.

Agents use native Subagents for bounded execution and scoped Collaboration Requests for independent peers. Native Codex owns execution, context, permissions and continuation. Decodex owns durable business relationships, evidence, coordination and presentation. The default Work tools do not create another executor, and no part of this contract requires Ultra.

Work and Guardian fit the existing desktop. Expand Work to inspect Materials, Collaboration, Results and assessment. Guardian has global Ask Me or Automatic Review and optional bounded Review Details; Full Access stays a separate native permission choice. Review records with no exact action association stay explicitly unassociated.

The current SQLite schema preserves native input receipts, independent process ownership and exact assignment evidence. Conversations can start without an Assistant. Native Project reconciliation, assignment recovery and Guardian detail recovery keep native execution in Codex. See the [acceptance boundaries](testing/upstream-acceptance-boundaries.md) for historical source-built, installed-app and online voice qualifications. Those records do not qualify a later artifact.

## Memory, collaboration and usage views

- **Agent preferences > Memory summary** reads and copies the loaded native summary on request. Native Codex selects the version; empty and unavailable states stay explicit. See [models and settings](workflows/models-and-settings.md#read-a-native-memory-summary).
- The existing **Collaboration** panel retains exact local question replies, delivery outcomes and independent contribution/repair cycles. A reply or completed receiving turn does not imply resolution. See [coordination](architecture/chief-coordination.md).
- **Actual usage** in Agent preferences and Work details shows known native token deltas and duration with source coverage. Shared or missing Work attribution stays explicit; this is not billing or complete Work elapsed time.

These views preserve unknown coverage and resolution limits. Their retained qualification is in [the acceptance boundary](testing/upstream-acceptance-boundaries.md#selected-read-only-views). Runtime selection, source-review state, installed artifacts and schedules remain separate.

## Persistent attached branches

When reviewing an earlier message, choose **Attached branch after this turn** to create a persistent native branch from its completed-turn prefix. The source history, draft and Goal remain unchanged. Creation sends no input and copies no native Goal. Reopen a verified branch through **Agent preferences > Attached branches > Open branch**. See [conversation recovery](workflows/conversations-and-recovery.md#edit-or-branch-at-an-earlier-input) and the [exact-artifact qualification](testing/upstream-acceptance-boundaries.md#persistent-attached-branches).

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
- Local plugin management and embedded MCP HTML widgets are retired. Codex owns configuration and native execution; Decodex retains tool observations, native forms and readable history.
- The checked-in automation portfolio marks the upstream maintainer **ACTIVE**. This is configuration evidence, not scheduler readback. Documentation generation does not enable automations, install an app or publish a release.

## Build and evidence

Use stable Rust and the repository-owned commands. macOS packaging uses the selected Xcode installation or an explicit `DEVELOPER_DIR`. A unit test, native fixture, signed desktop check, merge and installation prove different things. Keep version-specific limits visible.

Maintain documentation through OpenWiki. Consolidate by current system behavior and reader tasks; do not recreate a page for each retired document. Previous plans and revision-specific receipts remain in Git history. They are not a current work queue.

---
type: Reference
title: "Retained upstream product capabilities"
description: "Current native integration, account and conversation scope, with excluded duplicate surfaces."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-b2084dffd07b4229957a0f94
    resource: repo://apps/decodex-gpui/src/agent_prompt_edit.rs
  - id: openwiki-source-5e245e8cc4db92f2dbe2ba47
    resource: repo://apps/decodex-gpui/src/agent_prompt_fork.rs
  - id: openwiki-source-56b561d7e6dd906d218aa9d1
    resource: repo://apps/decodex-gpui/src/agent_read_state.rs
  - id: openwiki-source-01379a7fb49ab2d638863891
    resource: repo://apps/decodex-gpui/src/agent_skills.rs
  - id: openwiki-source-ecdf4586908bb6226955607a
    resource: repo://crates/decodex-codex/src/app_server_client/history_item.rs
  - id: openwiki-source-830a575f82404942511da59b
    resource: repo://crates/decodex-codex/src/app_server_client/prediction.rs
  - id: openwiki-source-dc292a3cdb3064363ab29907
    resource: repo://crates/decodex-codex/src/app_server_client/temporary_structured.rs
  - id: openwiki-source-a0063c7b07a1bc990ee9af6c
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process.rs
  - id: openwiki-source-e2f4e298ab0a4c683b92158d
    resource: repo://crates/decodex-runtime/src/account_service/personal_access_token.rs
  - id: openwiki-source-720a745d503e0e60ebcad0c5
    resource: repo://crates/decodex-runtime/src/agent_detail.rs
  - id: openwiki-source-d8df4be72e86f8bd3d65cce8
    resource: repo://crates/decodex-runtime/src/agent_plugins.rs
  - id: openwiki-source-3d25cb6558fde84d70520ac2
    resource: repo://crates/decodex-runtime/src/agent_read_state.rs
  - id: openwiki-source-8b32cad13ab2428dd54bd986
    resource: repo://crates/decodex-runtime/src/agent_skills.rs
  - id: openwiki-source-e32adebfd6d3bf27dc186bad
    resource: repo://crates/decodex-runtime/src/agent/tests/auth_recovery.rs
  - id: openwiki-source-2da6601c3f30e806c504e991
    resource: repo://crates/decodex-runtime/src/host_credentials.rs
generated: { by: "codex", at: "2026-10-10T06:49:11.265Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-10T06:49:11.265Z
---

# Product scope and native ownership

Decodex supplies a local workspace, account policy, work relationships and readable evidence. Codex supplies native conversation and tool execution. Adopt native capabilities through the admitted protocol rather than building a parallel execution system.

## Current integration boundary

| Area | Decodex responsibility |
| --- | --- |
| Conversations | Preserve native identity, canonical inputs, drafts and complete history export. Explicit native branches keep the original conversation; same-thread edits remain a separate choice. |
| Agent coordination | Keep one accountable owner, durable dispositions and links to the work actually reviewed. Native subagents remain native execution. |
| Tools and connections | Show observed capabilities and origin, and select enabled native skills for input. Configure plugin installation and connections per account in Codex. Discovery does not install or enable tools. |
| Rich output | Render supported Markdown and structured content. The embedded MCP HTML viewer and its callback execution path are retired. |
| Approvals and recovery | Use exact current request identity and native enforcement. Unknown outcomes require reconciliation, not automatic replay. |
| Accounts | Keep multi-account routing and native credential ownership. Imported ChatGPT PAT accounts use their verified identity without synthetic OAuth expiry or refresh tokens. |
| Voice | Retain the subscription dictation and live paths described in the voice integration page. Source support is not an installed-app acceptance result. |

The bridge admits installed-plugin observations but rejects local plugin list/read/install/reconcile management and MCP configuration reload. Historical plugin observations remain readable; these records do not restore removed management controls.

Provider authentication recovery recording, formerly O24, is also retired. Saved recovery history remains readable without generating new notifications or outgoing work.

The desktop does not need a duplicate plugin marketplace, plugin publishing workflow or embedded browser to use these native capabilities. A local skill picker selects an already enabled skill; it does not reproduce plugin setup.

## Retained native additions

Tool and file-change details use exact native thread, turn and item identities. Only an explicit unsupported-method response permits the older history-read fallback. This detail path does not replace bulk transcript paging.

Eligible root conversations expose native read receipts in task preferences. Read and unread marks require the reviewed native revision and local source identity. Reading does not acknowledge a result, and these receipts do not replace Dock handoff or attention state.

The client also exposes a bounded, explicitly invoked ephemeral prediction fork. It inherits the loaded parent's context, tools and permissions. It is not a tool-free recap, an automatic background job, or a composer suggestion interface. Its caller must own authorization and event routing. The isolated recap path remains separate.

Native launch policy enables description-first tool ordering, full-fork prefix preservation and model-specific subagent context defaults. The native configuration still selects Code Mode hosting transport. Decodex does not add a universal retain flag to submitted application tool outputs. See [Runtime architecture](../architecture/runtime-architecture.md) and [Acceptance boundaries](../testing/upstream-acceptance-boundaries.md).

## Adoption and evidence

Inspect official upstream source and the schema generated by the installed binary before adding an adapter. Separate upstream capability, local implementation, automated checks and installed-app acceptance. A retained DTO or database receipt alone does not establish an active UI or writer.

The previous numbered optional-feature table was a dated review, not permanent permission to preserve every feature. Its original decisions remain in Git history. Keep this page focused on current boundaries and use focused integration pages for behavior.

See [Tools and integrations](../integrations/tools-plugins-and-apps.md), [Agent coordination](../architecture/chief-coordination.md), [Subscription voice](../integrations/subscription-voice.md) and [Acceptance boundaries](../testing/upstream-acceptance-boundaries.md).

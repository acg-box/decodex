---
type: Reference
title: "Native integrations, automations and auxiliary tools"
description: "Native integrations, automations and auxiliary tools"
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-f5d4f5acdb41fe7c4e985337
    resource: repo://automations/decodex/README.md
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-7dcbb082d2502f1ec4386c39
    resource: repo://automations/upstream/prompts/maintainer.md
  - id: openwiki-source-2b364d575520def19b215f0b
    resource: repo://crates/decodex-pro-bridge/src/events.rs
  - id: openwiki-source-e3c81936641ddff1a8714ac6
    resource: repo://crates/decodex-pro-bridge/src/lib.rs
  - id: openwiki-source-e9d609e612bb7e44111ec4b1
    resource: repo://crates/decodex-runtime/src/agent_integrations.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-3edc2f094826ee56ddec6b73
    resource: repo://crates/decodex-runtime/src/ragflow.rs
  - id: openwiki-source-3f60f3a3aae45b3e3b355d3e
    resource: repo://database/src/assistant.rs
generated: { by: "codex", at: "2026-11-03T16:31:59.854Z" }
verified:
  - by: openwiki/0.7.3
    at: 2026-11-03T16:41:22.743Z
---


# Native integrations, automations and auxiliary tools

## Agent integrations

The service reads MCP status, installed plugins and installed Apps independently from native app-server. It reads the selected thread before and after the query and requires the same absolute working directory and live settings guard. Unsupported, unavailable and capacity-exceeded results remain distinct. A failed optional inventory does not prove that no tools are installed.

Plugin installation and connection configuration belong to Codex. Decodex retains read-only observations, native forms, resource links and task references. Inventory is not installation authority. UI clients do not copy credentials, directly edit native configuration, or substitute host files for account-owned state.

## Automation boundary

`automations/portfolio.toml` defines desired configuration for one managed native role: the upstream Maintainer. It has an explicit `ACTIVE` override and retains its daily cadence. Pause it during manual catch-up; resume it only with explicit user authorization. The Content Manager and Xurl Publisher schedules and prompts are retired. These values do not establish live scheduler status. Desired configuration does not prove host registration or execution, and a documentation refresh must not resume a paused host task. The retired upstream Reviewer and Health roles are not active requirements. Agent follow-up events are a separate service behavior.

```sh
python3 automations/decodex/scripts/config/render_automation_plan.py --json
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo +stable make test-automations
```

Do not assume an OpenWiki scheduled workflow exists from an old setup comment. See [Wiki maintenance](../operations/wiki-maintenance.md).

## Pro research component

The optional Pro research component uses Decodex's Rust bridge, Chrome extension and native host. Requests retain stable IDs, immutable input and send receipts. A completion subscription supplies advisory update hints; the saved request receipt remains authoritative after a lost or repeated hint. Waiting for that local connection makes no model call and cannot send a research question. The Agent Inbox carries the resulting completion evidence to its original requester for assessment.

The implemented [Assistant lifecycle](../decisions/assistant-lifecycle.md) uses the existing Inbox and host tick for independent user-delegated duties. Optional RAGFlow knowledge and Memory use a separate scoped HTTP adapter and direct-user configuration. These duties do not register a repository maintenance schedule or create a generic event bus. Pro request receipts retain their existing owner.

## Auxiliary product boundaries

Radar owns bounded research artifacts, source bundles and local evidence retention. Publisher owns candidate records, reservations, xurl effects, budgets and readback. Neither owns the application database, Agent threads, or account selection.

The retained static-site source is outside the current upstream-maintenance scope; the site is retired pending a separate redesign. Its source presence does not imply a live publication obligation. Local caches under `.agent/automations/`, build outputs, account state and credentials are not public site inputs.

`Decodex.app` is the sole GUI. Its Swift menu bar and native client library are in-process components; the local helper is `decodex serve`. See [Runtime architecture](../architecture/runtime-architecture.md), [Radar and Publisher contracts](radar-publisher-contracts.md), and [Subscription voice](subscription-voice.md).

For tool observations, native forms and the retired local management boundary, use [Tools and connections](tools-plugins-and-apps.md).

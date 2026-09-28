---
type: Reference
title: "OpenWiki quickstart"
description: "Task-oriented entrypoints for current Decodex architecture, workflows, product scope and historical evidence."
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T02:19:36.307Z
sources:
  - id: openwiki-source-d700ef551f46158044378d8f
    resource: repo://apps/decodex-cli/src/lib.rs
  - id: openwiki-source-ec2c431b14759817413ba09e
    resource: repo://apps/decodex-gpui/src/agent_graph.rs
  - id: openwiki-source-51a4755f3c4ddd78511e5c8e
    resource: repo://apps/decodex-gpui/src/agent_tree.rs
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
  - id: openwiki-source-e7e2b18dcd23b3b9fac7753b
    resource: repo://docs/archive/upstream-2026-09/migration-map.tsv
  - id: openwiki-source-8d4b61fd83ed007c18390abe
    resource: repo://docs/archive/upstream-2026-09/upstream-feature-decisions.md
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
generated: { by: "codex", at: "2026-09-28T02:19:36.307Z" }
---


# OpenWiki quickstart

Decodex is a local workspace above Codex app-server. The primary Agent can discuss work, act directly, or coordinate other agents. Codex owns native conversation execution. Decodex owns local work relationships, account policy, recovery and presentation. “Chief” in older filenames and records is the former Agent name.

## Start by task

| Task | Read first |
| --- | --- |
| Understand service, clients and storage | [Runtime architecture](architecture/runtime-architecture.md) |
| Trace Agent coordination and native children | [Agent coordination](architecture/chief-coordination.md) |
| Change conversation presentation | [Desktop workspace](architecture/desktop-workspace.md) |
| Change input, drafts, history editing or recap | [Conversations and recovery](workflows/conversations-and-recovery.md) |
| Change model, effort or task settings | [Models and settings](workflows/models-and-settings.md) |
| Handle an approval or provider question | [Approvals and ownership](workflows/approvals-and-native-ownership.md) |
| Change MCP, plugins, App widgets or hooks | [Tools and integrations](integrations/tools-plugins-and-apps.md) |
| Change dictation, live voice or voice preferences | [Subscription voice](integrations/subscription-voice.md) |
| Diagnose account selection or activation | [Accounts and routing](operations/accounts-and-routing.md) |
| Diagnose storage | [Local database](operations/local-database.md) |
| Decide which upstream capabilities belong here | [Product scope](decisions/upstream-product-scope.md) |
| Review an upstream change | [Upstream maintenance](operations/codex-upstream-autopilot.md) |
| Choose checks and understand their limits | [Commands](operations/commands-and-validation.md), [acceptance boundaries](testing/upstream-acceptance-boundaries.md) |
| Maintain documentation | [Wiki maintenance](operations/wiki-maintenance.md) |
| Find old PR, scan or acceptance evidence | [September 2026 archive](../docs/archive/upstream-2026-09/README.md) |

## Current boundaries

- `decodex serve` is the service and SQLite product-state owner. GPUI and CLI use typed local clients. The bundled helper is `decodex`, not `decodexd`.
- This source revision uses exact local protocol **2.97** and SQLite schema **49**. Read the protocol constant and migration owner when updating these values.
- The Agent graph presents dependencies and reports; the separate tree presents parent ownership. Historical Factory diagrams do not define the current desktop.
- Account Route is synchronous and service-owned. Shared-auth liveness, exact source identity and readback govern completion. Same-account refresh adopts a valid non-older native winner instead of restoring a losing token.
- Unknown submission outcomes require exact receipts or native history. They do not authorize replay. Unavailable conversations keep readable history.
- The user retained O01–O23 and O25–O26. O24 records no new AWS/Bedrock recovery notices; saved notices remain readable.
- The upstream maintainer is configured **PAUSED**. Documentation generation does not enable it, install an app or publish a release.

## Build and evidence

Use stable Rust and the repository-owned commands. macOS packaging uses the selected Xcode installation or an explicit `DEVELOPER_DIR`. A unit test, native fixture, signed desktop check, merge and installation prove different things. Keep version-specific limits visible.

The dated archive replaces `work/` as historical evidence. It is not another current product manual or an unfinished work queue. Old temporary test paths can have been removed after their results were retained.

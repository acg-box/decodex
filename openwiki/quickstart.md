---
type: Reference
title: "OpenWiki quickstart"
description: "OpenWiki quickstart"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-d700ef551f46158044378d8f
    resource: repo://apps/decodex-cli/src/lib.rs
  - id: openwiki-source-477d041b92b25547bc39e55d
    resource: repo://apps/decodex-gpui/src/chief_graph.rs
  - id: openwiki-source-a78ea5fe51f1eae9468e41e0
    resource: repo://apps/decodex-gpui/src/chief_tree.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# OpenWiki quickstart

Decodex is a local general-purpose multi-agent workspace above Codex app-server. The user normally talks to Chief. Chief can discuss or act directly, organize workers and subordinate Chiefs, assess results, and retain review records. Codex owns native execution; Decodex owns product coordination, account policy and presentation.

## Start by task

| Task | Read first | Source owner |
| --- | --- | --- |
| Understand execution and persistence | [Runtime architecture](architecture/runtime-architecture.md) | CLI `serve`, runtime bootstrap, SQLite |
| Change Chief organization, requests or recovery | [Chief coordination](architecture/chief-coordination.md) | `chief.rs`, `chief_host.rs`, `database/src/chief.rs` |
| Change conversation UI, graph/tree, glass or settings | [Desktop workspace](architecture/desktop-workspace.md) | GPUI shell, Chief workspace, native panels |
| Change account login or routing | [Account lifecycle](specs/account-lifecycle-authority.md), [Login](specs/account-login-authority.md) | AccountService and login manager |
| Diagnose local state | [Database operations](operations/local-database.md) | SQLite store and service |
| Change speech input | [Subscription voice](integrations/subscription-voice.md) | Dictation gateway, realtime Chief, Swift media host |
| Redeem/reset quota | [Reset Cards](operations/reset-cards.md), [Activation](operations/quota-activation.md) | Account API and durable operations |
| Build and test | [Commands and validation](operations/commands-and-validation.md) | Makefile.toml and macOS scripts |
| Maintain Codex compatibility | [Upstream adaptation](operations/codex-upstream-autopilot.md) | Native automation portfolio |
| Maintain evidence/public content | [Auxiliary tools](integrations/plugins-automations-and-auxiliary-tools.md) | Radar, Publisher, Astro site |
| Refresh this Wiki | [Wiki maintenance](operations/wiki-maintenance.md) | OpenWiki page/Claim lifecycle |

## Current boundaries

- `decodex serve` is the sole service and product-state owner. There is no current `decodexd` executable.
- SQLite at `~/.decodex/server/decodex.sqlite3` is the only normal product store. Current schema is 30 and local protocol is exactly 2.43.
- GPUI and CLI use typed local clients. The app's Swift libraries and attached glass windows do not create additional product authorities.
- Chief's agent tree represents ownership; its graph represents dependencies and reports. The old Factory Program/Domain lens is historical.
- Known unavailable conversations reject sending. History remains readable. An uncertain provider outcome is not safe to replay.
- Account Route is synchronous and service-owned. Older Pending-route DTOs and timer explanations are superseded.
- Reset Cards require explicit redemption; weekly quota activation is a separate configurable minimal request.
- Voice dictation produces an editable streaming draft with final correction; live voice attaches to the native Chief thread.

## Build and acceptance

Use stable Rust and a full selected Xcode installation. Packaging honors `DEVELOPER_DIR` but does not require `Xcode-beta.app`. Run focused checks first, then the applicable repository gate. Unit tests, signed packaging, visible UI acceptance, live provider behavior, merge and installation are different evidence.

## Historical material

The decisions, private-artifact archive and dated evidence pages preserve original receipts and rationale. They are not live setup instructions. Do not follow old disposable-database instructions, restore retired repository/GitHub orchestration, or infer that an old schema/protocol number is current.

[SQLite decision](decisions/sqlite-local-product.md) remains applicable. [Adaptive Program design](decisions/adaptive-program-extension-architecture.md) retains design context; Program persistence and built-in projections remain distinct from today's Chief UI.

An old setup comment is not proof of automatic Wiki updates. Check the maintenance page and actual host/workflow state.

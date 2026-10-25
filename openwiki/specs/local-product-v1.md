---
type: Reference
title: "Current local product contract"
description: "Current desktop, service, native conversation and account authority boundaries."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-98e7b23c4cc276d20fcb4649
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/AccountControlViews.swift
  - id: openwiki-source-acf49c93c3e80379f0023c71
    resource: repo://apps/decodex-gpui/src/accounts.rs
  - id: openwiki-source-3708e6cf9324a8ff1f6c39ae
    resource: repo://apps/decodex-gpui/src/agent_collaboration_tests.rs
  - id: openwiki-source-2a6989c7fa25b9681df71f95
    resource: repo://apps/decodex-gpui/src/agent_collaboration.rs
  - id: openwiki-source-2ee1164d01a09b38a208f1ac
    resource: repo://apps/decodex-gpui/src/agent_markdown.rs
  - id: openwiki-source-15ee466228bdb01892c9678b
    resource: repo://apps/decodex-gpui/src/agent_text_selection.rs
  - id: openwiki-source-51a4755f3c4ddd78511e5c8e
    resource: repo://apps/decodex-gpui/src/agent_tree.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-ff8abff5a675874c627f1969
    resource: repo://crates/decodex-runtime/src/account_service/tests/plan_scope.rs
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
  - id: openwiki-source-a09c082db4ad1473c4d1e557
    resource: repo://crates/decodex-runtime/src/application.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-bdc10e06c0058b26a9bde83d
    resource: repo://database/migrations/0056_project_agent_work.sql
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
  - id: openwiki-source-dd24c2ff3c2515a21892e312
    resource: repo://database/src/program_cycles.rs
generated: { by: "codex", at: "2026-10-24T21:19:56.719Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-25T20:12:58.607Z
---

# Current local product contract

This page describes the implemented local-product boundary. The accepted [Project, Agent and Work contract](../decisions/project-agent-work.md) defines the implemented architecture, shared English vocabulary and global Guardian controls. Its synthetic native and local qualification is recorded separately from installed-app acceptance. Legacy execution readers retain some old names without imposing a business hierarchy.

## Product and storage

Decodex is a local general-purpose agent workspace. A user-created Agent has one current native conversation and can own several Works. It uses native Subagents for temporary execution and scoped Collaboration Requests for independent peers. Ordinary native Conversations remain a separate execution path. The service is `decodex serve`; SQLite is the sole normal durable store. The embedded migration ledger defines its schema version. `CURRENT_VERSION` in the protocol crate defines the exact client/service protocol.

Clients do not read SQLite, provider credentials or Codex auth files. The signed app contains one GUI executable, one unified service helper and native libraries. Attached glass windows are presentation components, not new state owners.

## Native execution and history

Codex app-server owns provider execution and native thread history. The runtime binds exact account, process generation, thread and turn identities before accepting effects. It re-observes native state rather than assuming that another client cannot change a thread. A missing, interrupted or unknown result does not authorize replay.

Agent retains work relationships, event dispositions, dependencies, reviews and follow-up obligations in SQLite. A returned contribution requires assessment against current Work requirements before completion. Normal recovery preserves exact native identities. A separate quiescent tool-set migration retains previous history and requires positive replacement/retirement receipts; optional metadata errors stay isolated from the transport.

## User interaction

The conversation is primary. The graph represents work relationships; the right Collaboration panel keeps recorded exchanges and pending response requests visible across chats. Expanding an exchange preserves the active chat and draft. Current archive/ownership failures appear within the selected conversation. Known external ownership rejects new messages rather than queuing them for later. Archive restore is explicit and verified by exact native identity/readback.

Markdown text selection shares one document across the blocks of a rendered message. Response metadata separates duration from optional usage details. Dictation updates the draft with partial and final transcript revisions; live voice attaches to the native Agent thread.

## Account effects

Account admission accepts only native `plus`, `pro`, `prolite`, `promax` and `team` plans for OAuth and PAT credentials. Native identity, model entitlement and observed quota remain separate facts. See [account operations](../operations/accounts-and-routing.md).

Route is one synchronous service-owned command. It locks routing and account state, derives current revisions, checks liveness/shared-source identity, refreshes when required and commits an authoritative projection. The retired Pending-route shape and 100 ms retry loop are not current requirements.

Shared-auth writes use exact-source compare-and-swap and readback. Passive observation requires a stable two-poll source and an exact known stored bundle; it does not import a different bundle. A refresh can adopt a valid concurrent Codex winner without a second provider call or loser writeback. Generation-bound callbacks require a strictly newer same-provider credential and an active original generation.

Desktop controllers apply results only for the exact active session and command. They never kill or restart external Codex to make a route succeed. Account ordering, enablement, logout and explicit recovery remain service-owned.

Reset Card redemption is explicit and durably one-attempt. Weekly activation is separately configurable and deduplicates expired or observed floating reset windows. Neither UI animation nor a timeout proves a successful provider effect.

## Retained and retired scope

Historical Program cycles and their Domain Pack projections remain readable through compatibility queries; they do not define a current execution scheduler. The historical Factory graph is not the current UI. Built-in repository/PR/check-run orchestration and the private-artifact lane are retired; old evidence must not reactivate them.

No historical disposable-database instruction applies to user data. Ordered migrations, compatibility refusal and preserved recovery evidence are the current boundary.

Conversation branches and canonical input handback are described in [conversation recovery](../workflows/conversations-and-recovery.md). Imported native PAT credentials are described in [account operations](../operations/accounts-and-routing.md).

## Verification map

Use [Agent coordination](../architecture/chief-coordination.md), [Desktop workspace](../architecture/desktop-workspace.md), [Account lifecycle](account-lifecycle-authority.md), [Login](account-login-authority.md), [Subscription voice](../integrations/subscription-voice.md) and [Commands and validation](../operations/commands-and-validation.md). Tests, signed packaging, live behavior and merged delivery are separate claims.

## Project, Agent and Work

Current coordination uses separate Project, persistent Agent and Work objects with native Subagents, scoped independent Collaboration and global Guardian. Assistant and Project Lead are responsibilities of existing Agents. Direct schema56/protocol3 migration preserves actual old execution and history; historical Factory plans do not restore a second executor. Read the [canonical contract](../decisions/project-agent-work.md) and its explicit qualification limits.

---
type: Reference
title: "Current local product contract"
description: "Current local product contract"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-98e7b23c4cc276d20fcb4649
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/AccountControlViews.swift
  - id: openwiki-source-acf49c93c3e80379f0023c71
    resource: repo://apps/decodex-gpui/src/accounts.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-268229e2b9f21dae93c32513
    resource: repo://crates/decodex-protocol/src/wire.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Current local product contract

## Product and storage

Decodex is a local general-purpose agent workspace. Chief is the primary conversation and can organize workers and subordinate Chiefs. Ordinary native Conversations remain a separate execution path. The service is `decodex serve`; SQLite is the sole normal durable store, at schema version 30. The exact local protocol is 2.43.

Clients do not read SQLite, provider credentials or Codex auth files. The signed app contains one GUI executable, one unified service helper and native libraries. Attached glass windows are presentation components, not new state owners.

## Native execution and history

Codex app-server owns provider execution and native thread history. The runtime binds exact account, process generation, thread and turn identities before accepting effects. It re-observes native state rather than assuming that another client cannot change a thread. A missing, interrupted or unknown result does not authorize replay.

Chief retains work relationships, event dispositions, dependencies, reviews and follow-up obligations in SQLite. A worker result requires assessment before parent completion. Existing native threads are preserved during recovery; optional metadata errors are isolated from the main transport.

## User interaction

The conversation is primary. The tree represents ownership and the graph represents work relationships. Current archive/ownership failures appear within the selected conversation. Known external ownership rejects new messages rather than queuing them for later. Archive restore is explicit and verified by exact native identity/readback.

Markdown text can be selected per block. Response metadata separates duration from optional usage details. Dictation updates the draft with partial and final transcript revisions; live voice attaches to the native Chief thread.

## Account effects

Route is one synchronous service-owned command. It locks routing and account state, derives current revisions, checks liveness/shared-source identity, refreshes when required and commits an authoritative projection. The retired Pending-route shape and 100 ms retry loop are not current requirements.

Shared-auth writes use exact-source compare-and-swap and readback. Passive following requires stable metadata and same-account non-older rotations. A refresh can adopt a valid concurrent Codex winner without a second provider call or loser writeback. Generation-bound callbacks require a strictly newer same-provider credential and an active original generation.

Desktop controllers apply results only for the exact active session and command. They never kill or restart external Codex to make a route succeed. Account ordering, enablement, logout and explicit recovery remain service-owned.

Reset Card redemption is explicit and durably one-attempt. Weekly activation is separately configurable and deduplicates expired unchanged resets. Neither UI animation nor a timeout proves a successful provider effect.

## Retained and retired scope

Program cycles and compiled-in Domain Pack projections remain storage/runtime mechanisms. The historical Factory graph is not the current UI. Built-in repository/PR/check-run orchestration and the private-artifact lane are retired; old evidence must not reactivate them.

No historical disposable-database instruction applies to user data. Ordered migrations, compatibility refusal and preserved recovery evidence are the current boundary.

## Verification map

Use [Chief coordination](../architecture/chief-coordination.md), [Desktop workspace](../architecture/desktop-workspace.md), [Account lifecycle](account-lifecycle-authority.md), [Login](account-login-authority.md), [Subscription voice](../integrations/subscription-voice.md) and [Commands and validation](../operations/commands-and-validation.md). Tests, signed packaging, live behavior and merged delivery are separate claims.

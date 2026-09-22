---
type: Reference
title: "Account lifecycle authority"
description: "Account lifecycle authority"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-acf49c93c3e80379f0023c71
    resource: repo://apps/decodex-gpui/src/accounts.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Account lifecycle authority

## Owners

`AccountService` is the sole coordinator for durable account lifecycle and credential effects. SQLite owns account records, revisions, operation journals, credential records and routing control. Secret-bearing values stay inside zeroizing service adapters. GPUI, CLI and menu-bar presentation consume bounded credential-negative results.

Login enrollment and reauthentication enter through the singleton login manager and install through AccountService. Account profile and quota observations are distinct from authentication readiness. A missing five-hour window, an unknown observation and an exhausted window are different facts.

## Synchronous Route

Current `RouteAccount` names the target account. The service reserves a command receipt, locks routing and the account, and derives the current revision fences. It verifies enablement and credentials, checks exact shared-auth state, refreshes the relevant lineage when necessary, projects with compare-and-swap/readback, and commits selection through the same command.

External auth-owning Codex liveness or shared-source drift can reject a cross-account switch. The service does not kill external applications. The former Pending Route receipt, 100 ms polling policy and bounded process-list DTO are retired and must not be reintroduced from old Wiki text.

Desktop controllers accept only results matching generation, server identity, protocol, command and idempotency key. An unmatched or lost result cannot become success merely because the account row looks changed.

## Refresh convergence

Passive shared-auth following requires a stable two-poll source. Only a known same-account, non-older credential can be adopted. For a refresh of the exact projected family, Decodex mirrors its successor conditionally; if Codex wins the race with a valid non-older bundle, Decodex adopts that winner without another provider refresh or losing-token writeback.

A generation-bound refresh callback may return a registry successor only for the same provider, a strictly newer credential, a non-older account revision and a still-active generation. Invalid or uncertain bindings fail closed. Credentials and raw auth responses must not be logged or sent through UI projections.

## Recovery and controls

Enable/disable, order, selection, logout and manual recovery are service-owned operations. Durable state must survive restart; a timeout after a provider effect is not a fresh attempt authorization. Route, ordinary conversation account continuity, Chief account rotation, Reset Card redemption and weekly activation have different state owners and must not be conflated.

Current startup preserves SQLite migrations and user state. Historical disposable-store instructions do not apply. See [Login authority](account-login-authority.md), [Database operations](../operations/local-database.md), [Chief coordination](../architecture/chief-coordination.md) and [Reset Cards](../operations/reset-cards.md).

## Tests

Use AccountService, shared-auth coordinator, account-login and SQLite lifecycle tests for exact revision, refresh-race, receipt and restart behavior. Live authentication, provider quota and actual application ownership require separate acceptance evidence.

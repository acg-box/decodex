---
type: Reference
title: "Account lifecycle authority"
description: "Service-owned account effects, synchronous routing and exact credential refresh convergence."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-230c7c1843682509316cd745
    resource: repo://apps/decodex-cli/tests/account_route_process.rs
  - id: openwiki-source-acf49c93c3e80379f0023c71
    resource: repo://apps/decodex-gpui/src/accounts.rs
  - id: openwiki-source-f02ae41e2bd15dda85dc86b9
    resource: repo://codex-runtime.lock.json
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-82e4f987f46e34e31621081e
    resource: repo://crates/decodex-runtime/src/auth_projection.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-63bea5f3704fcd7e4b161192
    resource: repo://database/src/account_lifecycle.rs
  - id: openwiki-source-dc8958560b1230967470835f
    resource: repo://database/src/schema.rs
generated: { by: "codex", at: "2026-11-23T03:02:45.946Z" }
verified:
  - by: openwiki/0.9.0
    at: 2026-11-23T03:02:45.946Z
---

# Account lifecycle authority

## Owners

`AccountService` is the sole coordinator for durable account lifecycle and credential effects. SQLite owns account records, revisions, operation journals, credential records and routing control. Secret-bearing values stay inside zeroizing service adapters. GPUI, CLI and menu-bar presentation consume bounded credential-negative results.

Login enrollment and reauthentication enter through the singleton login manager and install through AccountService. Account profile and quota observations are distinct from authentication readiness. A missing five-hour window, an unknown observation and an exhausted window are different facts.

## Synchronous Route

The caller sends `RouteAccount` with a target account ID. The service reserves a command receipt, locks routing and the target, and reads their revisions. The target must be enabled, have usable credentials and qualify for new work. For an enrolled source, the service can save a valid native credential successor even after the source plan changes. It uses the existing refresh journal and credential store for this write.

If the shared OAuth sign-in belongs to a removed or unknown account, the command returns `shared_login_replacement_required` with a display label and confirmation. OAuth files with tokens and no `auth_mode` use the same check. A second command can send `replace_shared_login` with that confirmation and a new idempotency key. The digest binds the shared file version to the target ID, target revision and routing revision. The service stores both command results through its existing receipt mechanism.

The service checks for a live native credential owner and verifies the exact shared file version before a cross-account switch. It replaces the local sign-in with compare-and-swap, verifies the result and commits the selection. A conflicting owner or changed file stops the switch. The upstream OAuth logout can revoke tokens. Route uses local projection and leaves other account credentials intact. [Pinned Codex revision](https://github.com/openai/codex/commit/61cb947f057155b705dda922739663fe2c37cf65)

Desktop controllers accept only results matching generation, server identity, protocol, command and idempotency key. An unmatched or lost result cannot become success merely because the account row looks changed.

## Refresh convergence

Passive shared-auth observation requires a stable two-poll source and one known account with the exact stored credential bundle. A different bundle is reported as unmanaged; this observation does not import it. For a refresh of the exact projected family, Decodex mirrors its successor conditionally; if Codex wins the race with a valid non-older bundle, Decodex adopts that winner without another provider refresh or losing-token writeback.

A generation-bound refresh callback may return a registry successor only for the same provider, a strictly newer credential, a non-older account revision and a still-active generation. Invalid or uncertain bindings fail closed. Credentials and raw auth responses must not be logged or sent through UI projections.

## Recovery and controls

Enable/disable, order, selection, logout and manual recovery are service-owned operations. Durable state must survive restart; a timeout after a provider effect is not a fresh attempt authorization. Route, ordinary conversation account continuity, Agent account rotation, Reset Card redemption and weekly activation have different state owners and must not be conflated.

An explicit logout can recover from a refresh that was rejected by the provider or from access rejected after refresh. It reuses the exact recovery-operation link used by verified reauthentication. Preparation and commit require the same account revision and credential binding. The failed refresh keeps its original reason and becomes superseded only after the logout commits. Pending effects and ambiguous provider outcomes remain blocked.

Startup verifies the exact current SQLite schema and keeps user state. It does not apply a historical migration chain or reset an older store. See [Login authority](account-login-authority.md), [Database operations](../operations/local-database.md), [Agent coordination](../architecture/chief-coordination.md) and [Reset Cards](../operations/reset-cards.md).

For the desktop and CLI recovery steps and the native PAT import procedure, see [account routing and recovery](../operations/accounts-and-routing.md).

## Tests

Use AccountService, shared-auth coordinator, account-login and SQLite lifecycle tests for exact revision, refresh-race, receipt and restart behavior. The route process test switches away from a downgraded source, removes it, and checks the confirmation flow against an isolated service. It verifies that a change of sign-in invalidates an earlier confirmation and that a repeated command returns its saved result. The rejected-refresh logout regression covers direct and command execution, durable command replay, and startup reconciliation. The ambiguous-refresh regression verifies that logout leaves credentials intact. Live authentication, provider quota and actual application ownership require separate acceptance evidence.

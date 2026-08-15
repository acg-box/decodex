---
type: "Evidence"
title: "Adaptive Factory Spine V1 Evidence"
description: "Implementation, restart, protocol, GPUI, and dogfood evidence for the first bounded Program feedback cycle."
tags: [adaptive-factory, program, ontology, graph, sqlite, gpui, evidence]
openwiki:
  roles: [testing, architecture, workflow]
  change_kinds: [lifecycle, public-api, validation]
  source_paths: [crates/decodex-core/src/program.rs, database/migrations/0005_adaptive_factory_spine.sql, database/src/program_cycles.rs, crates/decodex-protocol/src/program_cycle.rs, crates/decodex-runtime/src/application.rs, crates/decodex-runtime/src/quick_task.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs, apps/decodex-gpui/src/shell.rs]
  test_paths: [database/src/program_cycles.rs, database/tests/quick_task_restart.rs, crates/decodex-runtime/tests/bootstrap_doctor.rs, crates/decodex-runtime/tests/websocket_protocol.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs, apps/decodexd/tests/signal_shutdown.rs]
  invariants: [SQLite is the only Program authority.; A Program WorkItem uses the ordinary Quick Task execution path.; A Review requires positive terminal provider evidence.; A derived causal graph has no scheduling authority.; Unknown provider outcomes never authorize automatic replay.; One V1 Program cycle owns exactly one WorkItem.]
  validation_commands: [python3 scripts/vnext/local_database_gate.py, python3 -m unittest tests/scripts/test_vnext_architecture.py, DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer cargo test -p decodex-core -p decodex-protocol -p decodex-database -p decodex-runtime -p decodex-gpui -p decodexd --all-targets --features decodex-gpui/visual-capture --no-fail-fast, DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer cargo clippy -p decodex-core -p decodex-protocol -p decodex-database -p decodex-runtime -p decodex-gpui -p decodexd --all-targets --features decodex-gpui/visual-capture -- -D warnings]
---

# Adaptive Factory Spine V1 Evidence

Status: implemented and locally verified.

Date: 2026-08-15.

This page contains no credential, account identity, Conversation identity, provider
thread identity, or provider Turn identity.

## Delivered boundary

The first Adaptive Factory slice implements this fixed causal chain:

```text
Program -> Signal -> Claim -> Proposal -> Objective -> WorkItem
        -> Codex Quick Task -> Evidence -> Program Review
```

Migration 0005 owns normalized tables for the semantic identities, WorkItem execution
binding, Evidence, and Review. Schema version 5 has five immutable migration digests and
39 tables. The Program create and Review operations are aggregate transactions. They do
not expose a general workflow or graph mutation API.

Protocol V2.2 adds bounded Program create and Review commands, list and aggregate
queries, and one complete Program change event. GPUI retains one presentation-neutral
Program controller. The Factory renders a Program pulse, causal graph, node inspector,
relative causal timeline, Evidence, Review controls, and navigation to the bound Codex
Conversation from the same aggregate.

Starting a Program WorkItem calls the existing Quick Task path with one optional exact
WorkItem cause. The Conversation and WorkItem binding commit in one SQLite transaction.
Routing Decision, RuntimeSession, ProcessGeneration, ProviderAttempt, and history remain
the execution owners. V1 adds no scheduler, provider client, or second worker engine.

## Safety evidence

- Program creation refuses duplicate semantic identities and idempotency drift.
- Quick Task creation binds at most one exact ready WorkItem in its existing
  Conversation transaction.
- Review creation requires the exact running WorkItem and positive terminal provider
  evidence for its bound Conversation.
- Review creation writes deterministic Evidence, external Evidence, classification,
  rationale, WorkItem completion, Objective state, and Program revision atomically.
- SQLite reopen retains the full cycle. Exact command replay returns the recorded result
  and does not create a second semantic chain.
- ProviderAttempt uncertainty keeps the existing no-automatic-replay boundary.
- A daemon signal-test fixture uses a short macOS temporary root so the staging socket
  stays within the kernel Unix-socket path limit. It also uses an empty private `PATH`,
  so transport lifecycle tests do not snapshot or hash an ambient Codex installation.
  SIGINT, SIGTERM, and stale-socket recovery after SIGKILL pass.

## Local validation

The affected-package all-target test command passes on stable Rust with the Xcode Beta
Metal toolchain. Strict Clippy passes with warnings denied. The vNext architecture suite
passes 10 tests. The schema-5 local database gate passes WAL, `quick_check`,
foreign-key, migration-digest, and exact 39-table inventory checks. The isolated daemon
signal suite passes all three process-level cases without depending on the size or
presence of an installed Codex executable.

The deterministic GPUI capture uses a complete closed-cycle projection with nine
semantic and runtime nodes and 11 causal relations. The production binary does not use
this fixture. It exists only behind the `visual-capture` feature.

## Native isolated restart dogfood

A staged signed Decodex app and a current daemon used an owner-private isolated root. The
operator created one Program through the native GPUI intake. Fresh accessibility
readback exposed one Program selector, the five pre-execution nodes, the authoritative
causal graph, and the matching causal timeline. A read-only database check found exactly
one Program.

The daemon and GPUI then stopped and restarted against the same root. A fresh retained
session reopened the same named Program and the same five-node causal projection. No
Conversation existed in this isolated root, so the restart could not replay a provider
request.

## Real Codex closed-cycle dogfood

The current user database was copied with SQLite online backup before migration. The
backup passed `integrity_check` at schema version 3. The current daemon then migrated the
live database to schema version 5 without changing the existing ProviderAttempt count.

The signed native GPUI created one Program with an explicit purpose, non-goal, review
policy, sourced Signal, Claim, Proposal, finite Objective, and one WorkItem. Starting the
WorkItem created one ordinary Quick Task and exactly one bound ProviderAttempt. Codex
performed a read-only repository inspection. The Turn settled with positive provider
evidence and identified the schema-5 migration and GPUI Program projection paths.

The operator attached one deterministic validation Evidence record and one external
Codex Evidence record, then recorded `capability_progress`. SQLite readback showed the
Objective as `achieved`, the WorkItem as `done`, two Evidence rows, and one Review. The
Factory showed the complete nine-node causal graph and matching timeline, and its
Conversation action opened the exact bound Quick Task.

The daemon and GPUI were then stopped and restarted. Before and after restart, the
database contained 43 total ProviderAttempts and exactly one ProviderAttempt bound to
this Program. The reviewed Program and bound Conversation reopened without creating a
new request.

This dogfood pass also exposed one stale in-memory projection after the first WorkItem
binding. The GPUI controller now retains the exact expected Conversation and invalidates
the selected Program only after the corresponding Quick Task publication. A focused
regression test covers returning to Factory after that publication. The rebuilt signed
app reopened the complete reviewed cycle.

## Scope retained for later milestones

This evidence does not claim a public Extension SDK, Domain Pack loader, MCP action
gateway, dynamic multi-agent topology, general WorkItem board, graph database, ontology
language, cross-project scheduler, remote worker, or consequential external action.

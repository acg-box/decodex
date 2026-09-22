---
type: Reference
title: "Historical Adaptive Factory Spine V1 evidence"
description: "Historical Adaptive Factory Spine V1 evidence"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-477d041b92b25547bc39e55d
    resource: repo://apps/decodex-gpui/src/chief_graph.rs
  - id: openwiki-source-dd24c2ff3c2515a21892e312
    resource: repo://database/src/program_cycles.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Current status

The one-cycle August receipt below remains historical. Program records still have a SQLite owner, but the old Factory UI and Quick Task source paths are not current desktop entrypoints. The active workspace uses Chief conversations, an ownership tree, and a dependency graph. This refresh does not rerun the recorded dogfood or extend its acceptance to current builds.

See [current architecture](../architecture/runtime-architecture.md) and [validation commands](../operations/commands-and-validation.md).

---

## Preserved evidence

# Adaptive Factory Spine V1 Evidence

Status: historical implemented baseline; superseded by Repeatable Program Loop V1.

Date: 2026-08-15.

This page contains no credential, account identity, Conversation identity, provider
thread identity, or provider Turn identity.

This page records the original one-cycle milestone and its V2.2/schema-5 evidence. The
current V2.3/schema-6 behavior is in
[Repeatable Program Loop V1 evidence](repeatable-program-loop-v1.md).

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

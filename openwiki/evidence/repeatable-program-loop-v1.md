---
type: "Evidence"
title: "Repeatable Program Loop V1 Evidence"
description: "Implementation, restart, replay, GPUI, and three-cycle live evidence for manual sequential Program continuation."
tags: [adaptive-factory, program, ontology, graph, sqlite, gpui, evidence]
openwiki:
  roles: [testing, architecture, workflow]
  change_kinds: [lifecycle, public-api, validation]
  source_paths: [database/migrations/0006_repeatable_program_loop.sql, database/src/program_cycles.rs, crates/decodex-protocol/src/program_cycle.rs, crates/decodex-protocol/src/wire.rs, crates/decodex-runtime/src/application.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs]
  test_paths: [database/src/program_cycles.rs, crates/decodex-protocol/src/wire.rs, crates/decodex-runtime/src/application.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs]
  invariants: [SQLite is the only Program authority.; Continuation binds the exact predecessor Review and Program revision.; One Program has at most one unreviewed cycle.; One Review has at most one successor Signal.; Continued WorkItems use the ordinary Quick Task provider path.; Unknown provider outcomes never authorize automatic replay.; GPUI derives cycle order from accepted causal lineage.]
  validation_commands: [python3 scripts/vnext/local_database_gate.py, python3 -m unittest tests/scripts/test_vnext_architecture.py, DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer cargo test -p decodex-core -p decodex-protocol -p decodex-database -p decodex-runtime -p decodex-gpui -p decodexd --all-targets --features decodex-gpui/visual-capture --no-fail-fast, DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer cargo clippy -p decodex-core -p decodex-protocol -p decodex-database -p decodex-runtime -p decodex-gpui -p decodexd --all-targets --features decodex-gpui/visual-capture -- -D warnings]
---

# Repeatable Program Loop V1 Evidence

Status: implemented and locally verified.

Date: 2026-08-15.

This page contains no credential, account identity, Conversation identity, provider
thread identity, or provider Turn identity.

## Delivered boundary

One active reviewed Program can append one finite next cycle:

```text
prior Review -> Signal -> Claim -> Proposal -> Objective -> WorkItem
             -> Codex Quick Task -> Evidence -> next Review
```

Migration 0006 adds one nullable predecessor Review reference to a Signal. The root
Signal has no predecessor. Each later Signal has one exact predecessor Review. Partial
unique indexes permit one root Signal per Program and one successor Signal per Review.
SQLite triggers reject cross-Program lineage. The migration is additive and does not
rewrite the first cycle.

`ContinueProgram` is one aggregate V2.3 command. It uses the command envelope's positive
expected revision instead of adding a second revision field. One immediate SQLite
transaction verifies the active Program, exact terminal Review, and absence of an
unreviewed cycle. It appends the next five semantic records and advances the Program
revision. Replay returns the existing receipt. Stale, duplicate, non-sequential, and
parallel input is rejected.

The command does not run the WorkItem. The operator starts it through the ordinary Quick
Task command. Existing Routing Decision, RuntimeSession, ProcessGeneration,
ProviderAttempt, account affinity, and no-replay rules remain the only execution path.
A Review still requires positive terminal provider evidence plus deterministic and
external Evidence.

Runtime reconstructs each cycle from its causal Review-to-Signal lineage. GPUI derives
`C1`, `C2`, and later labels from ordered Signal boundaries. It shows all retained
cycles, marks the current cycle, and opens the exact Conversation for the current
WorkItem. SQLite remains the semantic authority; the graph remains a projection.

## Deterministic evidence

Focused persistence tests cover a successful continuation, idempotent replay before and
after SQLite reopen, stale revision refusal, parallel unreviewed-cycle refusal, and
explicit abandonment of an unresolved prior Objective. Protocol tests cover V2.3
validation and stable wire fixtures. Runtime tests cover ordered two-cycle projection.
GPUI tests cover the exact revision and payload, two cycle boundaries, the `Continues`
edge, and current WorkItem conversation selection.

The local database gate reports schema version 6, six exact migration digests, WAL,
`quick_check`, foreign-key integrity, the predecessor lineage column, and 39 tables. The
vNext architecture suite passes 10 tests. Strict Clippy passes with warnings denied for
all affected packages. The complete affected-package and native visual checks are the
terminal acceptance commands listed in this page's metadata.

## Three-cycle live dogfood

The current user database was copied with SQLite online backup before migration from
schema version 5. The owner-private backup passed `integrity_check`. The live baseline
contained one reviewed Program cycle and 43 total ProviderAttempts.

Cycle 2 appended one successor to the exact first Review. Its bound read-only Codex
Quick Task created ProviderAttempt 44 and reached `succeeded` with positive terminal
evidence. A client connection ended after provider acceptance. Readback found the same
Conversation and attempt, so the probe did not resend. It completed the existing cycle
with an evidence-backed `capability_progress` Review. The Program reached revision 4
with two Signals, two WorkItems, and two Reviews.

The daemon then stopped cleanly and was started again through its LaunchAgent. The V2.3
doctor readback reported all eight required core checks as Ready. Before the next cycle,
the Program remained at revision 4 and ProviderAttempt count remained 44. Restart did
not add a semantic entity, Conversation, or attempt.

Cycle 3 appended one successor to the exact second Review. Its bound read-only Codex
Quick Task created ProviderAttempt 45 and reached `succeeded`. The final Review produced
Program revision 6 with exactly three Signals, three WorkItems, and three Reviews. The
two added cycles therefore created exactly two provider attempts. All three cycles kept
one Program identity and immutable predecessor history.

## Finding and next boundary

The result supports a small sequential Program aggregate. A separate Cycle table,
scheduler, graph database, or Manager process was not required. Exact lineage plus one
aggregate command was enough to make the long-lived Program operational.

This evidence does not validate automatic continuation, background stewardship,
dynamic multi-agent execution, a general WorkItem board, MCP actions, a public Extension
SDK, a registry, or a programmatic plugin host. Its next bounded pressure test is now
implemented. The Development and Paper Investment result is recorded in the
[Built-in Domain Pack Pressure Test V1 evidence](builtin-domain-pack-pressure-test-v1.md).

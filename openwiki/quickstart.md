---
type: "Reference"
title: "OpenWiki Quickstart"
description: "Entry point for Decodex local-product architecture, SQLite authority, Quick Task execution, Adaptive Factory Programs, operations, and validation."
tags: [repository, local-product, navigation]
openwiki:
  roles: [repository, architecture, workflow]
  change_kinds: [navigation]
  source_paths: [crates/decodex-runtime/src/quick_task.rs, crates/decodex-protocol/src/quick_task.rs, database/src/lib.rs]
---

# OpenWiki Quickstart

Decodex is a local agent factory above Codex app-server. Codex remains the execution
kernel for one thread. Decodex owns the product state and coordination that become hard
when one engineer operates many threads: account capacity, routing, durable conversation
lineage, process fencing, provider-attempt safety, history, and later graph-based factory
views.

The current milestone uses one bundled SQLite database. The fixed database is
`~/.decodex/server/decodex.sqlite3`. The `database/` workspace owns its schema,
migrations, credential table, adapters, transfer tool, and restart tests. `decodexd` is
the only normal reader and writer. GPUI and CLI remain same-UID protocol clients.

This authority supersedes the former server-database reset and the intermediate redb
credential-vault target. The server adapter is removed. redb remains linked only by the
one-shot transfer tool.

## Start here

- [SQLite local-product decision](decisions/sqlite-local-product.md): why the desktop
  product uses SQLite now and when a server database can be reconsidered.
- [Local product V1 contract](specs/local-product-v1.md): supported data, ownership,
  execution invariants, deferred capabilities, and acceptance conditions.
- [Local database operations](operations/local-database.md): initialization, validation,
  installation, one-shot account transfer, rollback retention, and checks.
- [SQLite implementation evidence](evidence/sqlite-local-product.md): current automated
  evidence and signed live acceptance.
- [Historical account lifecycle contract](specs/account-lifecycle-authority.md): retained
  domain semantics whose former server-store and redb ownership is superseded.
- [ProcessGeneration authority](specs/process-generation-authority.md): durable pre-spawn
  fences and positive-only process evidence.
- [ProviderAttempt authority](specs/provider-attempt-authority.md): one external turn
  attempt, positive-only outcome evidence, and replay prohibition.
- [Execution coordination](specs/execution-coordinator-authority.md): the small stateless
  sequencer used by Quick Task.
- [Adaptive Program and extension architecture](decisions/adaptive-program-extension-architecture.md):
  the implemented repeatable Program loop and built-in Domain Packs, plus the accepted
  direction for dynamic agents and bounded external actions.
- [Repeatable Program Loop V1 evidence](evidence/repeatable-program-loop-v1.md): the
  continuation contract, restart and replay proof, repeated GPUI projection, and live
  three-cycle dogfood record.
- [Built-in Domain Pack Pressure Test V1 evidence](evidence/builtin-domain-pack-pressure-test-v1.md):
  immutable Pack binding, namespaced domain projections, frozen Treasury provenance,
  fail-before-attempt capability checks, GPUI evidence, and two-domain live restart proof.
- [Adaptive Factory Spine V1 evidence](evidence/adaptive-factory-spine-v1.md): the exact
  historical first-cycle boundary, causal projection, restart proof, and local dogfood
  record.

For the current app-server freshness boundary and Quick Task controls, start with the
[local product contract](specs/local-product-v1.md) and its [SQLite evidence](evidence/sqlite-local-product.md).
The contract covers exact selected-thread read/archive reconciliation, the deliberately
lossy history boundary, request-scoped model/effort/Fast settings, and the
`RestoreProcessReadiness` pre-effect rejection.

## Current usable slices

The supported product path is one ordinary multi-turn Quick Task:

```text
user message
-> account route
-> RuntimeSession and Codex thread
-> fenced ProcessGeneration
-> fenced ProviderAttempt
-> assistant history and positive terminal evidence
-> exact app-server retirement and positive death evidence
-> daemon restart
-> later user message on the same Codex thread
```

The SQLite authority persists accounts, credentials, routing controls and quota facts,
conversation and Turn history, RuntimeSession thread binding, ProcessGeneration state,
ProviderAttempt state, and exact command receipts. Missing or stale quota data means
unknown capacity; it is not fabricated exhaustion. A current known depleted fact still
blocks that account.

The runtime keeps the mature Codex app-server protocol and safety harness. It does not
replace Codex with a new agent kernel. It preserves exact account binding, pre-spawn
fencing, one dispatch authorization, positive-only terminal evidence, and restart-safe
ambiguity handling. One Conversation keeps its initially selected account even when the
global routing default changes. Independent Conversations can use different accounts.
Only one non-dead app-server generation can use an account at one time. After positive
turn terminal evidence, the runtime retires that process before it publishes `Ready`. A
later Turn starts a fresh process generation and rehydrates the same account and Codex
thread. An idle completed Conversation does not reserve the account process slot.

The Factory also supports one manually repeated Adaptive Program loop:

```text
Program -> Signal -> Claim -> Proposal -> Objective -> WorkItem
        -> Codex Quick Task -> Evidence -> Review
        -> Signal -> Claim -> Proposal -> Objective -> WorkItem -> ...
```

SQLite owns the Program charter and every semantic identity. A Program WorkItem starts
through the ordinary Quick Task path and binds the resulting Conversation in the same
transaction. The Factory derives one causal graph and timeline from the authoritative
aggregate. It does not use a graph database or a second execution engine. A Review can
close only after the bound WorkItem has positive terminal provider evidence and the
user supplies deterministic and external Evidence. An explicit continuation binds the
exact prior Review and Program revision. It appends one finite next cycle and permits at
most one unreviewed cycle. No timer or automatic continuation exists.

Each new Program selects one of two built-in declarative Domain Packs. The Development
Pack derives Repository, Change, and Validation entities from the existing Program
aggregate. The Paper Investment Pack derives Asset, Thesis, and Scenario entities from
one embedded official U.S. Treasury June 2025 fixture. A legacy Program can receive one
revision-fenced Pack binding. The binding is immutable after creation. `decodexd`
validates the exact built-in Pack version and digest and derives stable entity identities
on readback. GPUI renders both Packs with the same host-owned graph, cards, inspector,
capability state, evidence, timeline, and Conversation controls.

The Pack contract adds vocabulary and a projection. It does not add a scheduler, worker,
graph store, arbitrary executable extension, live market feed, or action gateway. A
Program WorkItem must have one exact Pack that grants `codex.quick_task` before it can
enter the existing Quick Task runtime. Missing, unknown, drifted, and undeclared Pack
authority stops before a ProviderAttempt can exist.

## Deferred product surfaces

ManagedRepository, the general WorkItem board, Reset Card consumption, execution-decision
queries, automation, ManagedRun, remote workers, and multi-machine deployment are not
partially ported. Their current protocol surfaces remain typed unavailable where a
surface exists. They do not activate a legacy storage fallback.

Ontology and graph engineering are present as the small closed Program vocabulary,
derived causal projection, and two bounded namespaced Domain Pack projections above
proven conversation/runtime facts. A general ontology language, graph editor, graph
database, dynamic agent topology, and cross-Program planning remain deferred.

The accepted [adaptive Program and extension architecture](decisions/adaptive-program-extension-architecture.md)
defines the delivered first spine and the later product order. It keeps SQLite,
app-server, ProcessGeneration, ProviderAttempt, and positive-evidence owners as the
base. It does not make deferred extension or multi-agent surfaces partially available.

## Repository map

- `database/` owns bundled SQLite, immutable ordered migrations, database adapters, and
  local persistence tests.
- `database/transfer/` is the separate one-shot redb-to-SQLite upgrade tool. Normal
  daemon startup does not link redb.
- `crates/decodex-core/` owns mechanism-neutral domain types and fixed local paths.
- `crates/decodex-codex/` owns Codex app-server and direct provider contracts.
- `crates/decodex-runtime/` owns service composition, account/process/provider services,
  Quick Task orchestration, the built-in Pack registry, and bounded Program/domain
  projections.
- `crates/decodex-protocol/` owns the owner-only same-UID client protocol.
- `database/src/program_cycles.rs` owns the atomic Program aggregate and Review rules.
- `crates/decodex-runtime/domain_packs/` owns the two exact built-in declarative manifests.
- `crates/decodex-runtime/fixtures/` owns frozen offline Pack data and source metadata.
- `apps/decodexd/` is the only server composition root.
- `apps/decodex-cli/` and `apps/decodex-gpui/` are protocol-only clients.

## First commands

```sh
cargo run -p decodexd -- --version
cargo run -p decodex-cli -- status
cargo run -p decodex-cli -- doctor --output json
cargo run -p decodex-cli -- account list
cargo run -p decodex-gpui
python3 scripts/vnext/local_database_gate.py
python3 -m unittest tests/scripts/test_vnext_architecture.py
cargo make check
```

The hidden database commands used by the installer and the local gate are:

```sh
decodexd initialize-local-database --root ROOT
decodexd validate-local-database --root ROOT
```

## Safety rules

- Keep one normal product-state authority. Do not add dual-write or runtime fallback.
- Do not let a client open SQLite, the credential table, the retired redb file, or Codex
  authentication files.
- Keep the database owner-private (`0600`) and its directories owner-only (`0700`).
- Never emit credential values through logs, commands, protocol payloads, tests, or
  reports.
- Do not share the SQLite file over a network filesystem.
- Keep migrations ordered, embedded, immutable after release, and transactional.
- Preserve retained rollback data, the redb source, and Keychain records until a verified
  rollback-window decision authorizes deletion.
- Treat multi-machine deployment as a later server-mode architecture, not a generic
  database abstraction added to the desktop product now.

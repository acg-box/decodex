---
type: "Architecture Decision"
title: "SQLite Local-Product Decision"
description: "Accepted decision for a single-daemon desktop product using bundled SQLite as the normal Decodex product-state authority, with a separate one-shot redb transfer boundary."
tags: [architecture, decision, sqlite, storage]
openwiki:
  roles: [architecture, repository, operations]
  change_kinds: [persistence, migration, installation]
  source_paths: [Cargo.toml, database/src/lib.rs, database/src/migrations.rs, database/transfer/src/main.rs, crates/decodex-runtime/src/bootstrap.rs]
  symbols: [SqliteStore, ServiceBootstrap]
  test_paths: [database/tests/quick_task_restart.rs, database/transfer/tests/transfer.rs, scripts/vnext/local_database_gate.py]
  invariants: ["decodexd is the sole normal product-state writer.", "Clients use the daemon protocol and never open SQLite directly.", "The retired redb vault is accessed only by the one-shot transfer tool."]
  validation_commands: ["cargo make test-local-database", "cargo test -p decodex-database --all-targets", "cargo test -p decodex-database-transfer"]
---

# SQLite Local-Product Decision

Status: accepted and implemented.

Date: 2026-08-14.

## Decision

The first usable Decodex product is a local desktop application with one daemon writer.
It uses bundled SQLite as its only normal product-state authority. The schema and its
evolution owner live under `database/`.

No separate database server is packaged, supervised, configured, or contacted by a normal install.
redb is not a runtime credential store. It is linked only by the separate one-shot
account transfer executable. Keychain is not required for normal account credentials.

## First principles

Decodex is not another coding-agent model. Codex app-server already supplies the thread
execution kernel. Decodex exists to amplify it into a manageable factory when a person
has more active threads, accounts, dependencies, evidence, and follow-up work than they
can reliably track in a chat list.

The first product deployment has these facts:

1. One user owns one local Decodex root.
2. One `decodexd` process owns product mutations.
3. GPUI, menu bar, and CLI clients communicate through one same-UID protocol.
4. Multi-machine workers are a possible future product, not a current requirement.

Under these facts, a separate server database adds packaging, bootstrap, roles, sockets,
supervision, credentials, and failure modes without adding useful concurrency or remote
authority. SQLite is the smaller complete mechanism.

## Consequences

- The fixed path is `~/.decodex/server/decodex.sqlite3`; it is not a user-configured
  endpoint.
- One serialized `rusqlite::Connection` owns transactions. Synchronous work runs through
  bounded blocking tasks.
- The database uses bundled SQLite, WAL, `synchronous=FULL`, foreign keys, a bounded busy
  timeout, private cache, `NOFOLLOW`, and owner-private files.
- Credentials use a narrow table in the same physical database. Secret-bearing access is
  limited to the daemon credential adapter and zeroizing memory values.
- The schema has an ordered migration ledger. Fresh install and upgrade execute the same
  embedded migrations in `BEGIN IMMEDIATE` transactions and verify migration digests.
- There is no backend trait, pool, dual-write system, or storage-engine switch.
- A future multi-machine product must define a separate server-mode authority and a data
  transfer boundary. It must not make the desktop database a shared network file.

## Transfer decision

Existing account state is valuable because multi-account capacity is a core factory
requirement. The upgrade therefore moves the exact credential-negative account snapshot
and secret bundles into SQLite once.

The old running daemon supplies account, quota, and routing facts without credentials.
After a graceful stop, a signed transfer tool opens the fixed retired redb vault read
only, validates exact account and credential bindings, and commits one SQLite transaction.
The transfer is idempotent and value-suppressing. It does not accept an arbitrary source
path and does not delete retained rollback data, the redb vault, or Keychain records.

## Scope control

This decision does not port every planned factory domain. The first proof is one real
Quick Task conversation that returns an assistant response and continues on the same
Codex thread after daemon restart. Deferred domains return typed unavailable responses
instead of keeping a legacy backend alive.

---
type: "Operations Guide"
title: "Local Database Operations"
description: "Operator workflow for Decodex bundled SQLite initialization, validation, signed macOS installation, one-shot redb transfer, rollback retention, and focused acceptance checks."
tags: [operations, sqlite, installation, migration, recovery]
openwiki:
  roles: [operations, repository, testing]
  change_kinds: [persistence, installation, recovery]
  source_paths: [database/src/lib.rs, database/src/migrations.rs, database/transfer/src/main.rs, apps/decodexd/src/main.rs, scripts/macos/install_decodex_local_service.py, scripts/vnext/local_database_gate.py]
  symbols: [SqliteStore, initialize_local_database, validate_local_database, TransferOutcome]
  test_paths: [database/tests/quick_task_restart.rs, database/transfer/tests/transfer.rs, tests/scripts/test_install_decodex_local_service.py, tests/scripts/test_vnext_architecture.py]
  invariants: ["The daemon owns the fixed SQLite database and clients never open it.", "Migrations are embedded, ordered, transactional, and immutable after release.", "The retired redb source is read only and retained for rollback until an explicit acceptance decision.", "Validation is read only and fails closed on unsafe or incompatible database state."]
  validation_commands: ["python3 scripts/vnext/local_database_gate.py", "python3 -m unittest tests/scripts/test_install_decodex_local_service.py", "cargo test -p decodex-database --all-targets", "cargo test -p decodex-database-transfer"]
---

# Local Database Operations

Consult this page when changing local persistence, installer behavior, schema migrations, transfer safety, or database validation. The current product uses one bundled SQLite database; it does not require a separate database server.

## Fixed paths and ownership

- Product database: `~/.decodex/server/decodex.sqlite3`
- Retired transfer source: `~/.decodex/server/credentials.redb`
- Local protocol socket: `~/.decodex/server/decodex.sock`

`database/` is the schema and schema-evolution owner. `database/migrations/` contains immutable ordered SQL; `database/src/migrations.rs` applies and attests the embedded sequence; domain modules own typed transactions; and credentials remain in the narrow owner-private credential table. `decodexd` is the only normal reader and writer. The database is not supported on a network filesystem.

## Initialize and validate

The explicit daemon commands are:

```sh
decodexd initialize-local-database --root ROOT
decodexd validate-local-database --root ROOT
```

Initialization creates or upgrades the fixed database through embedded migrations and is idempotent for the exact current schema. Validation is read only at the product level and checks application identity, migration order and digests, schema inventory, foreign keys, WAL and synchronous settings, integrity, and foreign-key consistency. Normal `decodexd serve` performs the same validation before retaining product availability and does not use redb or a legacy server store.

The migration seam is additive and transactional. Never edit a shipped migration: add the next numbered migration and focused upgrade/restart coverage. A schema change that crosses the signed app, daemon, CLI, or FFI boundary must also preserve the artifact cohort and receive package-facing validation.

## Fresh macOS installation

Stage one signed, team-consistent service set:

```sh
DECODEX_LOCAL_SERVICE_SIGN_IDENTITY="Developer ID Application: Example (TEAMID)" \
  scripts/macos/stage_decodex_local_service.sh
```

The stage contains `decodexd`, `decodex`, and `decodex-database-transfer`. The installer verifies signatures, team identity, fixed binary identifiers, and daemon/CLI artifact-cohort agreement before stopping a running service. It creates owner-only directories, initializes and validates SQLite, installs a LaunchAgent invoking `decodexd serve` from `~/.decodex`, starts the daemon, and performs doctor/account-list readback through the installed CLI.

It does not create server roles or databases, resolve a database password, or install a network database service.

## One-shot account transfer

When SQLite is absent but the retired redb vault exists, the installer treats transfer as one upgrade boundary:

1. Ask the old daemon for a bounded credential-negative account snapshot.
2. Stop the old service gracefully.
3. Send that snapshot to signed `decodex-database-transfer` on stdin.
4. Open the fixed redb path read only with no-follow and owner checks.
5. Require the exact account set and validate payload fingerprints and bindings.
6. Insert accounts, credentials, quota facts, routing, and one transfer ledger row in one `BEGIN IMMEDIATE` transaction.
7. Revalidate SQLite and perform credential-negative readback.

An exact retry returns `replayed`; a different source or non-fresh target fails closed. Output contains only outcome, count, digest, and source-retained facts. The tool does not delete the redb file or other rollback sources.

## Rollback retention and validation

Retain the redb source and any pre-upgrade private database backup until live account inventory, first response, daemon restart, later response, and an accepted observation window are complete. Deletion requires a separate operator decision.

```sh
decodexd artifact-cohort
decodex --output json artifact-cohort
python3 scripts/vnext/local_database_gate.py
python3 -m unittest tests/scripts/test_vnext_architecture.py
python3 -m unittest tests/scripts/test_install_decodex_local_service.py
cargo test -p decodex-database --all-targets
cargo test -p decodex-database-transfer
cargo test -p decodexd
```

The leading `a` in the preceding block is not a command; use the corrected command below when copying it:

```sh
python3 -m unittest tests/scripts/test_install_decodex_local_service.py
```

Use `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer` for the full GPUI workspace gate on the current development host when the default Command Line Tools selection lacks Metal.

See [Runtime architecture](../architecture/runtime-architecture.md) for startup ownership and [SQLite local-product decision](../decisions/sqlite-local-product.md) for the product rationale.

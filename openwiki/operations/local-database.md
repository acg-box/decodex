# Local Database Operations

Status: current operator and validation workflow.

## Fixed paths

- Product database: `~/.decodex/server/decodex.sqlite3`
- Retired transfer source: `~/.decodex/server/credentials.redb`
- Local protocol socket: `~/.decodex/server/decodex.sock`

The product database path is derived from the validated Decodex root. Configuration does
not contain a database endpoint.

## Initialize and validate

The installer uses these hidden commands:

```sh
decodexd initialize-local-database --root ROOT
decodexd validate-local-database --root ROOT
```

Initialization creates or upgrades the fixed database with embedded ordered migrations.
It is idempotent for an exact current database. Validation is read-only at the product
level and verifies the application ID, migration sequence and digests, exact schema
inventory, foreign keys, WAL, full synchronous mode, quick integrity result, and foreign
key check.

Normal `decodexd serve` opens and verifies the same database. It does not start or contact
a database server.

## Fresh installation

Build one signed, team-consistent local-service set before installation:

```sh
DECODEX_LOCAL_SERVICE_SIGN_IDENTITY="Developer ID Application: Example (TEAMID)" \
  scripts/macos/stage_decodex_local_service.sh
```

The stage contains `decodexd`, `decodex`, and `decodex-database-transfer`. The daemon
and transfer tool use the fixed identifiers checked by the installer. An ad-hoc
signature is rejected because it has no TeamIdentifier.

The macOS installer:

1. verifies signed binaries and the expected team;
2. creates owner-only directories and configuration;
3. initializes and validates SQLite;
4. installs a LaunchAgent that invokes `decodexd serve` directly;
5. starts the daemon; and
6. runs doctor and account-list readback.

It does not install PostgreSQL, create roles or databases, manage a socket directory, or
resolve a database password.

## Existing account transfer

If SQLite is absent and the retired redb vault exists, the installer treats this as one
upgrade boundary:

1. It asks the old running daemon for the bounded credential-negative account list.
2. It gracefully stops the old service.
3. It sends that snapshot on stdin to the signed `decodex-database-transfer` tool.
4. The tool opens only the fixed redb path with read-only and no-follow checks.
5. It requires the exact same account set and validates every payload fingerprint and
   binding.
6. It inserts accounts, credentials, quota facts, routing, and one transfer ledger row in
   one `BEGIN IMMEDIATE` transaction.
7. It revalidates SQLite and performs credential-negative readback.

An exact retry returns `replayed`. A different source or a non-fresh target fails closed.
Output contains only the outcome, count, digest, and source-retained fact.

## Rollback retention

The installer and transfer tool do not delete the old PostgreSQL cluster, redb file, or
Keychain records. They are inert rollback sources. Delete them only after a separate
decision confirms live account inventory, first response, daemon restart, later response,
and an accepted observation window.

## Validation commands

```sh
python3 scripts/vnext/local_database_gate.py
python3 -m unittest tests/scripts/test_vnext_architecture.py
python3 -m unittest tests/scripts/test_install_decodex_local_service.py
cargo test -p decodex-database --all-targets
cargo test -p decodex-database-transfer
cargo test -p decodexd
```

Use `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer` for the complete GPUI
workspace gate on the current development host because the default Command Line Tools
selection does not expose the Metal compiler.

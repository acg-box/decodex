---
type: "Reference"
title: "Local Database Operations"
openwiki_generated: true
sources:
  - id: openwiki-source-35eca69c3013fea5ba400887
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/DecodexNativeCompatibility.swift
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-268229e2b9f21dae93c32513
    resource: repo://crates/decodex-protocol/src/wire.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-a09c082db4ad1473c4d1e557
    resource: repo://crates/decodex-runtime/src/application.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-a89c2fe187b4f7bf37dc206d
    resource: repo://database/migrations/0009_durable_account_route.sql
  - id: openwiki-source-7fe11c5074beaf147aac2be4
    resource: repo://database/migrations/0010_pending_account_route_progress.sql
  - id: openwiki-source-63bea5f3704fcd7e4b161192
    resource: repo://database/src/account_lifecycle.rs
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
generated: { by: "codex", at: "2026-08-27T10:25:21.174Z" }
verified:
  - by: openwiki/0.4.2
    at: 2026-08-27T10:25:21.174Z
---

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

Schema version 8 adds two nullable self-references to the existing account-operation
journal. They bind one verified reauthentication to one targetless refresh ambiguity and
record the successful supersession without changing the old operation's recovery phase or
reason. The migration is additive, preserves existing rows, and replaces the unsettled
operation indexes atomically. A binary rollback across this schema boundary must retain a
matching pre-upgrade private database backup; an older daemon does not accept a newer
migration ledger.

Schema version 9 adds one nullable, credential-negative `request_json` column to command
receipts. Only a reserved `route_account` receipt must contain this value. The partial index
supports bounded startup recovery. Schema version 10 adds nullable `progress_json` for decoding
pending Route progress and a unique partial index for at most one reserved receipt. Pending is a
current accepted handoff state when an external Codex process may still own the old refresh-token
family or when the exact shared-auth source is not stable enough to replace. The daemon retains the
original request, account and routing revision fences, and idempotency receipt. Startup and the
bounded background recovery loop reclaim that same receipt only after re-reading those fences and
the current external Codex liveness state. Creating Pending immediately wakes the recovery loop.
While any Pending Route exists, the loop checks every 100 milliseconds; without Pending work it
uses a one-second idle cadence. A long Pending state therefore means that liveness still observes
an auth-owning Codex process or another readiness fence, not that the timer is slow. Decodex does
not terminate or restart Codex.

Pending carries one closed operator reason. A concrete process wait lists at most eight positive
PIDs, identifies each as ChatGPT or Codex, and states whether the normal shared auth home is proved
or unknown. Official ChatGPT/Codex bundle executables always block. A standalone Codex CLI is
ignored only when same-UID process metadata proves that its effective `CODEX_HOME` is an existing
canonical directory distinct from the normal shared home. If macOS withholds environment metadata,
the CLI remains a visible `auth home unknown` blocker. Other reasons name account readiness,
shared-source stability or availability, process-observation availability, or atomic projection
readback. Operators should quit the listed process or repair the displayed readiness boundary;
they should not wait for a longer timer.

After a Route projects an account, ordinary same-account refreshes remain journaled account
operations. A successful Decodex refresh conditionally mirrors its successor to the exact shared
auth source. If Codex rotates that source first, Decodex imports the valid non-older Codex bundle as
the winner through a new deterministic credential rotation; it does not write the losing refresh
token back to `auth.json`. Receipts and progress remain credential-negative throughout this
convergence.

Schema version 11 adds the singleton `desktop_settings` table. `decodexd` is its only
reader and writer. The positive revision guards the **Show Decodex in the menu bar**
preference; GPUI reads and changes it only through protocol 2.11.

The account-login restoration repair adds no schema migration. The current protocol uses exact
artifact cohort 7. On startup, the daemon can compensate only the exact pre-repair
`StoreApplied` enrollment collision described by the account-lifecycle contract; it deletes the
proved orphan credential and cancels that operation. Installation therefore upgrades the signed
daemon, CLI, and GPUI application as one cohort while retaining the pre-install database rollback
copy. The application bundle contains no native client library because GPUI links the protocol
client directly.

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

1. verifies signed binaries, the expected team, and exact daemon/CLI artifact-cohort
   agreement before it stops the running service;
2. creates owner-only directories and configuration;
3. initializes and validates SQLite;
4. installs a LaunchAgent that invokes `decodexd serve` directly with
   `~/.decodex` as its stable working directory;
5. starts the daemon; and
6. runs doctor and account-list readback through the installed CLI, which proves the
   running daemon uses the same protocol 2.11 and artifact cohort 7.

It does not install former server store, create roles or databases, manage a socket directory, or
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

The installer and transfer tool do not delete the old former server store cluster, redb file, or
Keychain records. They are inert rollback sources. Delete them only after a separate
decision confirms live account inventory, first response, daemon restart, later response,
and an accepted observation window.

## Validation commands

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

Use `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer` for the complete GPUI
workspace gate on the current development host because the default Command Line Tools
selection does not expose the Metal compiler.

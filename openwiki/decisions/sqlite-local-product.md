# SQLite Local-Product Decision

Status: accepted and implemented.

Date: 2026-08-14.

## Decision

The first usable Decodex product is a local desktop application with one daemon writer.
It uses bundled SQLite as its only normal product-state authority. The schema and its
evolution owner live under `database/`.

PostgreSQL is not packaged, supervised, configured, or contacted by a normal install.
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

Under these facts, a PostgreSQL server adds packaging, bootstrap, roles, sockets,
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
- There is no backend trait, pool, dual-write system, or SQLite/PostgreSQL switch.
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
path and does not delete the PostgreSQL cluster, redb vault, or Keychain records.

## Scope control

This decision does not port every planned factory domain. The first proof is one real
Quick Task conversation that returns an assistant response and continues on the same
Codex thread after daemon restart. Deferred domains return typed unavailable responses
instead of keeping PostgreSQL alive.

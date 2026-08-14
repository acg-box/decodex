# Decodex database

This directory is the sole schema and schema-evolution owner for the local Decodex
product database.

The first product mode has one daemon and one bundled SQLite database at
`~/.decodex/server/decodex.sqlite3`. Clients use the daemon protocol. They do not open
this database. The database is not a generic storage abstraction and is not supported
on a network file system.

## Ownership

- `migrations/` contains immutable, ordered SQL migrations.
- `src/migrations.rs` applies and attests the embedded migration sequence.
- Domain modules own bounded typed reads and transactions.
- Large content stays in the content-addressed blob store.
- Credential values stay in `account_credentials`. General account reads do not select
  this table or return its payload.

Never edit a shipped migration. Add the next numbered migration and a focused upgrade
test. A migration must be transactional and safe to retry after process termination.

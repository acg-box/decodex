# vNext storage feasibility proof

This M0 spike proves the storage choices required by XY-1264. It is not the complete
`decodexd` schema, a v0.2 importer, or a compatibility path.

Run the proof on the intended macOS host:

```sh
cargo make test-vnext-storage-proof
```

The runner resolves one PostgreSQL distribution, creates a temporary cluster with data
checksums enabled and TCP disabled, creates databases only inside that cluster, and
removes the cluster on exit. It never connects to the machine's ordinary PostgreSQL
socket or databases. `--keep` is a diagnostic escape hatch when invoking `proof.py`
directly; the operator must remove the printed temporary directory afterward.

## Selected implementation contract

- PostgreSQL 18.x with `pgcrypto` is the sole product-state database. The Rust stack is
  `tokio-postgres 0.7.18`, `deadpool-postgres 0.14.1`, and `refinery 0.9.2`. Refinery
  embeds ordered forward migrations and records checksums. Destructive down migrations
  are not accepted. SQLx was rejected for this gate because its migration macro pulled a
  SQLite linkage version incompatible with the existing v0.2 `rusqlite` workspace.
- The local daemon owns a dedicated per-user cluster. It uses a Unix socket and does not
  expose PostgreSQL on TCP. A future remote UI connects to authenticated `decodexd`,
  never to PostgreSQL. The pool is bounded; the proof prewarms two connections and caps
  at 32.
- Mutations carry expected revisions and idempotency keys. Lease acquisition and outbox
 claiming are atomic PostgreSQL operations. Outbox delivery is at least once. After an
 ordinary failure, only the owning worker may record the error and reschedule with a
 delay. After an ambiguous side effect, the worker reconciles a receipt and authoritative
 readback before retrying. Exactly-once side effects are not claimed.
- Large bytes live at `blobs/sha256/<first-two-hex>/<sha256>`. PostgreSQL owns their
  metadata and lifecycle. Auth is checked before a range read; byte length and SHA-256
  are checked before bytes are trusted. Deletion removes bytes only after retention is
  due and retains a PostgreSQL tombstone.
- The UI cache is keyed by server/schema/entity revision or content hash, capped by both
  bytes and entry count, disposable, and rebuilt from PostgreSQL. SQLite remains allowed
  only behind this cache boundary; this proof needs only filesystem entries.

## Installation and local ownership

PostgreSQL is an explicit external runtime prerequisite, not embedded or forked. For
development, `postgres`, `initdb`, `pg_ctl`, `psql`, `pg_dump`, and `pg_restore` must
resolve from the same PostgreSQL 18 distribution (Nix or Homebrew is acceptable). The
bootstrap resolves the real binary directory and matching `share/postgresql` directory,
explicitly enables data checksums, then verifies checksums are `on`, server major 18, and
`pgcrypto` before accepting the cluster.

The packaged app must perform the same preflight and provide an actionable PostgreSQL 18
installation/configuration step when it fails; it must not silently use another global
service or downgrade the version. XY-1267 should add the daemon bootstrap command and
configuration field for the resolved binary directory. Package installation itself is
owned by the later packaging gate.

The vNext local layout is:

```text
~/.decodex/
  config.toml
  logs/
  backups/
  blobs/sha256/
  cache/
  postgres/data/
  run/postgres/       # Unix socket and daemon-owned transient files
```

Directories and files containing product state are daemon-user-only. Credentials stay
in the host credential vault. PostgreSQL stores account metadata and health only.
`~/.codex` remains Codex-owned shared configuration and rollout/thread continuity; vNext
does not place PostgreSQL, blobs, cache, logs, or Decodex configuration below it.

## Backup, restore, and rollback

Before a migration, stop product mutations and take a custom-format dump:

```sh
pg_dump -Fc -d decodex -f ~/.decodex/backups/decodex-before-V1.dump
decodexd storage migrate
pg_dump -Fc -d decodex -f ~/.decodex/backups/decodex-after-V1.dump
pg_restore --list ~/.decodex/backups/decodex-after-V1.dump
```

`decodexd storage migrate` is the required future public command; the proof invokes the
same embedded migrator directly. Restore never overwrites the suspect database. Restore
to a new database, validate migration and row/integrity counts, then change the daemon
target while mutations remain stopped:

```sh
createdb decodex_recovery
pg_restore --exit-on-error --single-transaction --no-owner \
  -d decodex_recovery ~/.decodex/backups/decodex-after-V1.dump
psql -X -qAt -d decodex_recovery \
  -c "SELECT version, name, checksum FROM refinery_schema_history ORDER BY version"
```

Rollback uses the same new-database procedure with the pre-migration dump. Keep the
failed database until diagnosis is complete; remove it only after recovered daemon and
integrity readback are accepted.

## Maintenance and failures

The proof runs `VACUUM (ANALYZE) decodex.outbox`, `REINDEX TABLE decodex.outbox`, and
checks `pg_stat_user_tables.last_analyze`.

| Failure | Detection | Required response |
| --- | --- | --- |
| Wrong PostgreSQL major/distribution or missing `pgcrypto` | bootstrap/migration fails before acceptance | install/configure the accepted PostgreSQL 18 distribution; do not weaken the version check |
| Migration failure | refinery returns an error and preserves migration checksum history | keep mutations stopped; restore the pre-migration dump into a new database |
| Corrupt database/operator mistake | dump, restore, or integrity readback fails | preserve the suspect cluster; restore the latest verified dump into a new cluster/database |
| Expired daemon lease | `expires_at` is in the past | a new holder atomically replaces it and increments revision |
| Crash after outbox claim | expired `in_flight` row | reclaim with incremented attempt; reconcile external receipt/readback before side-effect retry |
| Missing/tampered blob | missing file, byte-count mismatch, or SHA-256 mismatch | report unavailable/corrupt; regenerate only from authoritative provenance |
| Cache damage/overflow | schema/hash mismatch or either cap exceeded | evict or delete and rebuild from PostgreSQL/blob metadata |
| Disk pressure | volume monitoring reaches stop threshold | evict cache first, apply accepted blob retention, stop mutations before exhaustion |

Timings include debug-build and local scheduling effects and are feasibility evidence,
not production latency budgets. XY-1300 owns later performance/fault budgets.

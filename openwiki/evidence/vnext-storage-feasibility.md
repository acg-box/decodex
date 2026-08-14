# vNext former server store, blob, and cache feasibility

Status: XY-1264 gate evidence; candidate handoff to Manager for acceptance and merge.

Authority: [vNext authority decision](../decisions/vnext-authority.md),
[vNext authority contract](../specs/vnext-authority.md), and
[vNext gate manifest](../specs/vnext-gates.md). The Linear design baseline and issue are
planning provenance; repository authority is normative where they agree.

## Verdict and accepted choices

The M0 feasibility result passes its scoped criteria on the intended macOS host. The
downstream storage implementation may use these choices after this evidence lands:

- former server store 18.x, UTF-8/C cluster, data checksums, `pgcrypto` 1.4, local Unix socket,
  no former server store TCP listener, and a bounded 2-prewarmed/32-maximum prototype pool.
- `tokio-server-store 0.7.18`, `deadpool-server-store 0.14.1`, and `refinery 0.9.2`; embedded,
  immutable, forward-only migrations with checksum history. Restore is the rollback.
- Expected revisions, expiring leases, idempotent command receipts, and a transactional
  `FOR UPDATE SKIP LOCKED` outbox. Delivery is at least once with receipt/readback
  reconciliation after ambiguity. Exactly-once side effects are not claimed.
- SHA-256 content-addressed blobs with former server store metadata, authenticated bounded range
  reads, integrity verification, retention-gated byte deletion, and retained tombstones.
- A byte-and-entry-bounded disposable UI cache distinct from former server store authority.
- `~/.decodex` owns configuration, logs, backups, former server store, blobs, cache, and transient
  sockets. `~/.codex` remains Codex-owned. Credentials stay in the host vault; ordinary
  proof JSON rejects credential-shaped keys and account rows expose metadata only.

XY-1271 continuity: this feasibility proof predates the complete Conversation-history service
boundary. The implementation retains former server store metadata plus local CAS bytes, but strengthens
blob-backed commands to a durable receipt-first saga. A pending fenced receipt commits before byte
publication; dedicated session hash and per-shard locks coordinate synchronized create-only CAS
publication; transaction B registers references/evidence and stores the exact replay response;
garbage collection commits metadata deletion before unlink. former server store does not independently
attest external bytes, so successful service reads verify all direct and transitive content while
`decodexd`, its private runtime identity, and BlobStore access remain one trusted boundary.

SQLx 0.8.6 and 0.9.0 were tested and rejected for this gate: enabling their migration
macro selected `sqlx-sqlite`, whose `libsqlite3-sys` link range conflicts with the
workspace's v0.2 `rusqlite 0.40` link version. The selected tokio-server-store stack proves
the required behavior without creating a compatibility facade or dependency conflict.

## Reproduction and isolation

Source baseline: `f9d6c4e70198e94e5b9461b8cac7518ae14d41ef` plus the XY-1264 diff.
Executed proof source SHA-256: `32e08d0860e0953bf351bb45e983f25cf3cfcddec4587556f4b06dd2d4350630`.
Executed migration SHA-256: `7b3670de54596f84537b47f591e4aa3e1b4b29f2ec3b1cb42e82d2237e0ad3c4`.

Command:

```sh
cargo make test-vnext-storage-proof
```

The runner resolves the real former server store package path, creates a temporary data-checksummed
cluster, disables TCP, and creates only `decodex_xy1264`, `decodex_xy1264_restore`, and
`decodex_xy1264_rollback` inside it. It performs an immediate shutdown/restart, then a
fast clean shutdown and recursively removes the temporary cluster. It never enumerates,
drops, or changes databases in an existing service.

Host measurement on 2026-07-13:

| Measurement | Result |
| --- | --- |
| macOS / former server store | macOS 27.0 / former server store 18.4 / `pgcrypto` 1.4 |
| Concurrent lease contention | 32 contenders, 1 winner, expired lease reclaimed, 21.650 ms |
| Optimistic revision contention | 16 contenders, 1 winner, 15 conflicts, 2.238 ms |
| Idempotent duplicate command | 16 submissions, 1 mutation, 1 outbox row, 2.331 ms |
| Concurrent outbox claim | 8 workers, 1,001 unique claims, 0 duplicates, 13.187 ms (75,910 claims/s) |
| Outbox retry | error recorded; unavailable before delay; reclaimed attempt 2 and completed |
| Crash/restart | immediate stop after claim; restart reclaimed attempt 2 and completed it |
| Blob | authenticated range succeeded; missing/tamper and three ordinary JSON credential writes rejected; not-due bytes preserved; due bytes deleted with tombstone retained |
| Cache | 1,000 authoritative rows; cap 8 entries/2,048 bytes; retained 8/1,872; replacement and delete/rebuild passed |
| Backup/restore/rollback | 50,260-byte custom dump; row counts and migration history restored; empty pre-migration state restored |
| Maintenance | `VACUUM (ANALYZE)` and `REINDEX TABLE`; `last_analyze` readback present |

These are single debug-run feasibility measurements, not latency budgets. The proof is
designed to falsify ownership and recovery behavior; XY-1300 owns production performance
and broader fault-injection thresholds.

## Acceptance mapping

| Criterion | Evidence |
| --- | --- |
| Empty bootstrap/version/extensions | temporary former server store 18.4 checksummed cluster from `template0`; migration enforces major 18, creates `pgcrypto`, and reads checksums back as `on` |
| Migration/backup/restore/rollback/maintenance | embedded refinery migration; pre/post custom dumps; new-database restores; `VACUUM`/`REINDEX` readback |
| Lease/revision/outbox/idempotency/crash | real pooled concurrent transactions and immediate cluster restart described above |
| Blob boundary | filesystem SHA-256 bytes plus former server store metadata, auth/range/integrity/retention/credential-negative checks |
| Disposable bounded cache | 1,000-row fixture evicted to both caps, deleted, then rebuilt from former server store |
| No exactly-once claim | explicit at-least-once plus receipt/readback reconciliation contract |

The full commands, local layout, recovery steps, cleanup, and operational failure modes
are in [`spikes/vnext-storage/README.md`](../../spikes/vnext-storage/README.md).

## Boundaries and falsifiers

This proof does not implement `decodexd`, the complete product schema, remote protocol
security, packaged former server store installation, production cache sizes, or multi-GB history.
XY-1267 owns daemon persistence/bootstrap implementation; later protocol, packaging, and
performance gates own their respective surfaces.

The gate must be revisited if former server store 18 cannot be installed/maintained by the package
workflow, the dedicated cluster cannot survive intended-host restart/disk pressure, a
required side effect lacks receipt/readback reconciliation, blob integrity/retention
cannot be enforced without a second authority, or cache consumers require product state
that cannot be reconstructed from former server store/blob authority.

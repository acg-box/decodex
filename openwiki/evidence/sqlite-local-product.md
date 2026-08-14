# SQLite Local-Product Evidence

Status: accepted implementation and signed live-cutover evidence.

Date: 2026-08-14.

This page contains no credential value, email address, provider-account identifier, or
credential fingerprint.

## Implemented evidence

- `database/` owns one bundled SQLite connection, immutable V1 and V2 migrations, digest ledger,
  exact schema inventory verification, WAL, full synchronous mode, foreign keys,
  integrity checks, no-follow open, and owner-private file creation.
- Unit tests cover initialization, reopen, migration tamper refusal, exact credential
  compare-and-swap, foreign-key refusal, file mode, symlink refusal, account lifecycle,
  routing, quota uncertainty, and command replay.
- `database/tests/quick_task_restart.rs` uses only public APIs. It persists initial account
  routing, RuntimeSession and Codex-thread binding, a ready ProcessGeneration, one
  authorized ProviderAttempt, assistant history, positive provider evidence, and atomic
  terminalization. It then reopens SQLite, reserves a later Turn, and proves a SameThread
  plan on the original account and Codex thread. It admits one fresh rehydrated
  ProcessGeneration only from the active, acknowledged source RuntimeSession and exact prior
  terminal evidence. It also proves that an exact completed process-admission receipt plus an
  absent target generation is durable pre-effect evidence. Replaying the old attempt leaves
  exactly one dispatch intent.
- `database/transfer` tests a real redb fixture, exact import, exact replay, SQLite
  readback, owner-private mode, and byte-for-byte source retention.
- Installer tests cover fresh install, direct LaunchAgent composition, transfer ordering,
  bounded subprocesses, signature checks, account-count readback, and source retention.
- Runtime and daemon focused checks compile without PostgreSQL. Daemon signal tests cover
  SIGINT, SIGTERM, exact socket cleanup, and stale-socket recovery after SIGKILL. In the full
  nextest run, these real cold-start tests reserve all global test threads while retaining their
  20-second startup bound.

## Signed live acceptance

On 2026-08-14, the daemon, CLI, and transfer binaries built from the final source were signed by
one Apple Development Team and installed atomically. Strict signature verification passed for
the three fixed executable identifiers. The installer then reported:

- SQLite active as the local database;
- six accounts available after the one-shot transfer;
- the transfer was not repeated during final-source reinstall;
- the retired PostgreSQL directory and redb vault retained; and
- the direct `decodexd serve` LaunchAgent running.

A fresh real Quick Task conversation against that final installed set completed one Codex
app-server turn, restarted `decodexd`, and completed a later turn that recalled a nonce from the
first turn. The probe reported one preserved RuntimeSession and one Codex thread. Independent
read-only SQLite queries found:

- four completed Turns: two user and two assistant;
- 48 ordered history items;
- one InitialThread and one SameThread continuation plan, each with a distinct operation and
  idempotency key;
- two distinct succeeded ProviderAttempts, each with exact terminal evidence; and
- two distinct ProcessGenerations: the pre-restart generation dead and the current generation
  ready.

The exact Codex thread value and all credential-bearing values were suppressed. The failed
pre-dispatch probe and the later acceptance-unknown probe remain durable diagnostic evidence and
were not automatically retried.

An additional new conversation was started while the selected account still owned the prior
conversation's ready ProcessGeneration. Before the repair, this case returned
`AcceptanceUnknown` despite having no target generation or ProviderAttempt. After the repair, the
same live condition returned the explicit `RestoreProcessReadiness` rejection. Read-only SQLite
evidence showed one failed user Turn, no active Turn, no Codex thread, no target
ProcessGeneration, and no ProviderAttempt. The one-live-generation-per-account policy remains;
cross-conversation process multiplexing is deferred.

## Final repository gates

One complete `cargo make check` run finished successfully on the final source. It included:

- exact npm 11.17.0 installation, lock provenance and signature checks, zero high-level npm
  vulnerabilities, site build, and Astro diagnostics with zero errors, warnings, or hints;
- all-feature, all-target workspace compilation, Rust formatting, Taplo formatting, and strict
  Clippy checks for all 12 active Rust packages;
- the schema-V2 local database gate with both immutable migration digests, WAL, foreign keys,
  integrity checks, owner-private mode, and the exact 28-table inventory;
- 833 passed nextest tests with three declared skips, including the globally isolated
  full-daemon signal tests; and
- architecture-contract and real CLI/daemon diagnostic tests.

The final Rust advisory scan reports zero vulnerabilities. Its information-only result remains
the pre-existing baseline of four unmaintained and two unsound transitive packages. Dependency
inspection confirms that normal `decodexd` composition contains neither PostgreSQL nor redb;
redb is present only in the separate one-shot transfer executable.

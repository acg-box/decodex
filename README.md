<div align="center">

# Decodex

Local agent factory above Codex app-server.

[![License](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![GitHub tag (latest by date)](https://img.shields.io/github/v/tag/acg-box/decodex)](https://github.com/acg-box/decodex/tags)

</div>

Decodex is not another coding model or a replacement for Codex. Codex app-server is the
execution kernel for one thread. Decodex adds the durable product state and coordination
needed when one engineer manages many conversations, accounts, dependencies, gates, and
follow-up actions.

The current milestone is intentionally small: one real local Quick Task can select an
account, start or continue a Codex thread, persist its execution facts, survive a daemon
restart, and continue without duplicate dispatch.

## Current architecture

- `decodexd` is the sole product-state and side-effect owner.
- A bundled SQLite database at `~/.decodex/server/decodex.sqlite3` is the only normal
  product store.
- `database/` owns migrations, schema verification, storage APIs, transfer tooling, and
  restart tests.
- Account credentials are stored in a narrow owner-private SQLite table. They are
  available only to the daemon credential adapter and never enter protocol output.
- Codex app-server remains the provider runtime. One RuntimeSession binds one exact
  account and Codex thread.
- GPUI and CLI are same-UID Unix WebSocket clients. They do not open SQLite, credential
  files, or Codex authentication files.

Normal startup does not require a separate database server, redb, or Keychain. A one-shot tool
can import the existing account pool from the retired redb vault during upgrade. It opens
the source read only and leaves all rollback sources intact.

## Supported product slice

```text
user message
-> account route
-> RuntimeSession and Codex thread
-> fenced ProcessGeneration
-> fenced ProviderAttempt
-> assistant history and positive terminal evidence
-> daemon restart
-> later user message on the same Codex thread
```

The database persists account lifecycle and routing state, credentials, quota facts,
Conversation and Turn history, runtime-session binding, process-generation fences,
provider attempts, positive evidence, and command receipts.

Missing or stale quota evidence means unknown capacity, not exhaustion. A current known
depleted observation still blocks that account. Fixed routing keeps the selected account;
balanced routing prefers known available capacity and then follows configured order
through unknown accounts.

Account affinity is conversation-scoped. The first route binds one account to the
RuntimeSession and Codex thread. Later turns keep that account even if the global routing
default changes. A different conversation can select a different account. If the bound
account is depleted, this milestone stops for explicit recovery; it does not silently
replace the account and discard provider cache affinity.

## Deferred surfaces

This milestone does not partially port every proposed factory feature. ManagedRepository,
WorkItem board persistence, Reset Card consumption, execution-decision projections,
automation, ManagedRun, ontology and graph projections, remote workers, and multi-machine
coordination are deferred. Implemented protocol calls return typed unavailable results;
they do not start a legacy storage fallback.

Ontology and graph engineering remain central to the direction of Decodex. They will be
projections over proven Goals, tasks, threads, artifacts, claims, dependencies, gates,
and evidence. They are not a second speculative execution engine.

## Workspace

- `database/`: SQLite authority and one-shot account transfer.
- `crates/decodex-core/`: mechanism-neutral domain types and fixed local paths.
- `crates/decodex-codex/`: Codex app-server contracts.
- `crates/decodex-runtime/`: daemon service composition and Quick Task orchestration.
- `crates/decodex-protocol/`: bounded same-UID protocol.
- `apps/decodexd/`: daemon composition root.
- `apps/decodex-cli/`: diagnostic and product command client.
- `apps/decodex-gpui/`: desktop client.
- `openwiki/`: current product, architecture, operations, and evidence authority.

## Development

The active Rust toolchain is stable. The repository uses a separately pinned formatter
because its style options are newer than stable rustfmt.

```sh
python3 scripts/vnext/local_database_gate.py
python3 -m unittest tests/scripts/test_vnext_architecture.py
cargo test -p decodex-database --all-targets
cargo test -p decodex-database-transfer
cargo test -p decodexd
cargo make check
```

These checks run locally. This repository does not keep a tracked GitHub Actions CI
workflow; future Actions are limited to tag/release publication. The active vNext CLI
does not provide repository commit, landing, or Git-hook commands. Use the reviewed
Git/GitHub workflow for those actions, with exact base/head object IDs and authoritative
merge readback where required.

On the current macOS development host, use the Xcode beta developer directory for the
complete GPUI gate because the default Command Line Tools selection does not include the
Metal compiler:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer cargo make check
```

Start with the [OpenWiki quickstart](openwiki/quickstart.md) for the normative contract,
operations, safety rules, and current evidence.

## License

Decodex is licensed under GPLv3. See [LICENSE](LICENSE).

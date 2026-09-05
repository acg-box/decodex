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

The current milestone is intentionally small: one real local Conversation can select an
account, start or continue a Codex thread, persist its execution facts, survive a service
restart, and continue without duplicate dispatch.

## Current architecture

- `decodex serve` is the sole product-state and side-effect owner. The same `decodex`
  executable also provides the short-lived CLI commands.
- A bundled SQLite database at `~/.decodex/server/decodex.sqlite3` is the only normal
  product store.
- `database/` owns migrations, schema verification, storage APIs, transfer tooling, and
  restart tests.
- Account credentials are stored in a narrow owner-private SQLite table. They are
  available only to the service credential adapter and never enter protocol output.
- Codex app-server remains the provider runtime. One RuntimeSession binds one exact
  account and Codex thread.
- GPUI and ordinary CLI commands are same-UID Unix WebSocket clients. They do not open SQLite, credential
  files, or Codex authentication files.
- The GPUI product is the sole macOS GUI and is packaged as `Decodex.app`. The bundle
  contains a signed `Contents/Helpers/decodex` for local profiles and the native Swift menu-bar
  presentation as an in-process dynamic library. It contains no second app or UI process.
- A local app session starts `Contents/Helpers/decodex serve --parent-fd ...` when no
  service is available. It reuses an exact-version service and reports
  `service_version_mismatch` for any other version. The app-user command at
  `~/.local/bin/decodex` is a symlink to the bundled helper, never a copied second binary.
  The standalone local-service installer instead places one regular `decodex` executable at
  that path for pure CLI and LaunchAgent operation. These installation modes are mutually
  exclusive; neither installer adds coexistence machinery. Running `decodex` without a
  subcommand displays help, and serving is always explicit.
- **Show Decodex in the menu bar** is a service-owned product preference. **Launch Decodex
  at login** is an independent macOS `SMAppService.mainApp` preference and is not stored in
  SQLite. Closing the main window hides Decodex and retains the protocol session, native
  menu bar, and app-owned service. **Quit Decodex** stops the app and only its owned service.

Normal startup does not require a separate database server, redb, or Keychain. A one-shot tool
can import the existing account pool from the retired redb vault during upgrade. It opens
the source read only and leaves all rollback sources intact.

## Supported product slice

```text
GPUI Conversation action                    apps/decodex-gpui/src/conversations.rs
-> exact-current Conversation protocol      crates/decodex-protocol/src/{conversation,wire}.rs
-> Decodex Conversation service             crates/decodex-runtime/src/{application,conversation}.rs
-> SQLite revision/idempotency authority    database/src/{conversations,command}.rs
-> RuntimeSession + ProcessGeneration       database/src/{runtime_sessions,process_generations}.rs
-> ProviderAttempt safety                   database/src/provider_attempts.rs
-> Codex app-server thread/start + turn/start
                                            crates/decodex-codex/src/conversation.rs
-> exact thread/history/event readback      runtime + protocol + GPUI
```

The Program path has no second execution engine. `database/src/program_cycles.rs` owns the
persisted Program aggregate and its Conversation binding. The service derives the Program graph
and timeline, and `apps/decodex-gpui/src/{programs,program_graph,factory_surface}.rs` presents
them. A Codex link is absent until SQLite readback supplies the exact provider thread ID; a
Decodex Conversation UUID is never substituted. Provider thread identities are opaque, limited
to the SQLite-compatible 512-byte boundary, and percent-encoded as exactly one deep-link path
segment.

The database persists account lifecycle and routing state, credentials, quota facts,
Conversation and Turn history, runtime-session binding, process-generation fences,
provider attempts, positive evidence, and command receipts.

Missing or stale quota evidence means unknown capacity, not exhaustion. A current known
depleted observation still blocks that account. Fixed routing keeps the selected account;
balanced routing prefers known available capacity and then follows configured order
through unknown accounts.

Account Route is one synchronous, fail-fast operation under one service-local mutex. It
returns `codex_is_running` immediately, without changing authentication or routing, when
ChatGPT or Codex is open. Otherwise it validates the target credential, safely persists any
required refresh successor, rechecks process and source state, atomically replaces
`~/.codex/auth.json`, verifies exact readback, and only then commits the fixed account in
SQLite. It never creates Pending state, waits for an app to exit, or hot-switches a running
Codex process. Refresh ambiguity becomes `credential_needs_login`; it is never blind retry
authority. Startup can reconcile only the narrow case in which the auth write completed but
the SQLite active-account commit did not.

Account affinity is conversation-scoped. The first route binds one account to the
RuntimeSession and Codex thread. Later turns keep that account even if the global routing
default changes. A different conversation can select a different account. If the bound
account is depleted, this milestone stops for explicit recovery; it does not silently
replace the account and discard provider cache affinity.

## Removed and deferred surfaces

The unsupported WorkItem board protocol and UI, and the static Coordinator/Agent/Review/Replay
Factory preview, are deleted. Their old command names are not part of protocol 2.14 and fail
decoding. The fake Execution Decision query and projection are also removed from the protocol and
runtime. ManagedRepository, Reset Card execution, automation, ManagedRun, remote workers, and
multi-machine coordination remain deferred without a public fake workflow or legacy storage
fallback.

Ontology and graph engineering remain central to the direction of Decodex. They will be
projections over proven Goals, tasks, threads, artifacts, claims, dependencies, gates,
and evidence. They are not a second speculative execution engine.

## Persistence compatibility

This cutover adds one ordered migration that terminalizes legacy reserved `route_account`
receipts as `interrupted_by_upgrade` and removes their replayable request/progress state. It
preserves accounts, credentials, routing data, conversations, and immutable Program Pack bindings.
The local protocol accepts one exact version; build commit and package version remain diagnostics,
not a second compatibility scheme.
The compatibility allowlist is limited to persisted/internal bytes that existing databases or
Pack digests already own:

- the `quick_task_requests` table, `quick_task_admission_key` column, and migration identity/file
  `quick_task_execution_controls` / `0003_quick_task_execution_controls.sql`;
- persisted command-operation discriminators containing `quick_task` in existing receipts and
  process-generation evidence;
- the immutable built-in Pack capability literal `codex.quick_task`.

These names are not product, UI, protocol, or Rust API concepts.

## Workspace

- `database/`: SQLite authority and one-shot account transfer.
- `crates/decodex-core/`: mechanism-neutral domain types and fixed local paths.
- `crates/decodex-codex/`: Codex app-server contracts.
- `crates/decodex-runtime/`: service composition and Conversation orchestration.
- `crates/decodex-protocol/`: bounded same-UID protocol.
- `apps/decodex-cli/`: the `decodex` composition root, explicit service command, diagnostics,
  and product command client.
- `apps/decodex-gpui/`: the only desktop GUI and `Decodex.app` packaging source.
- `openwiki/`: current product, architecture, operations, and evidence authority.

## Development

The active Rust toolchain is stable. The repository uses a separately pinned formatter
because its style options are newer than stable rustfmt.

```sh
python3 scripts/vnext/local_database_gate.py
python3 -m unittest tests/scripts/test_vnext_architecture.py
cargo test -p decodex-database --all-targets
cargo test -p decodex-database-transfer
cargo test -p decodex-cli --all-targets
DECODEX_APP_SIGN_IDENTITY="4EBCADF6B4D513E45CE33EC6934C08DBB0F03D7F" \
DECODEX_APP_SIGN_TEAM_IDENTIFIER="4N949UKQ55" \
  scripts/macos/test_decodex_app_stage.sh
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

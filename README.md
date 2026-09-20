<div align="center">

# Decodex

Local agent factory above Codex app-server.

[![License](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![GitHub tag (latest by date)](https://img.shields.io/github/v/tag/acg-box/decodex)](https://github.com/acg-box/decodex/tags)

</div>

Decodex is not another coding model or a replacement for Codex. Codex app-server is the
execution runtime for independent threads. Decodex adds the durable product state and coordination
needed when one engineer manages many conversations, accounts, dependencies, gates, and
follow-up actions.

The personal Chief coordinates goals through independent Codex threads. It receives
worker and automation results, requests repairs in the original worker thread, and
reports decisions to the user. SQLite preserves work relationships and obligations
across service restarts. Ordinary Conversations remain available for direct work.
See [Chief refactor status](CHIEF_REFACTOR.md) for acceptance evidence and open gaps.

## Working with Chief

The Chief tab uses the same local service as the other app surfaces. Select an
explicit model, reasoning effort, working directory and execution policy before
starting. Workers use the same model with medium effort. The account-owned process
remains alive across turns; a worker result can wake the original Chief later.
Chief process admission uses account readiness and current quota observations.
The five-hour limit is optional: confirmed absence is distinct from an unknown
window or a failed query. Accounts are not classified by plan name. Known current
exhaustion still blocks admission. A newly enrolled account can require a quota
refresh before its first Chief process starts.

The CLI exposes the same operations:

```sh
decodex chief status
decodex chief start --root-id personal-chief --model MODEL --effort high --cwd /absolute/project --read-only "Coordinate this goal"
decodex chief send --root-id personal-chief "Report the remaining decisions"
decodex chief ingest --work-id WORK_ID --source-event-id SOURCE_EVENT_ID "Automation result"
decodex chief request --event-id EVENT_ID
```

Use a stable source event ID for automation results. Repeated delivery of the same
event does not create another inbox item. Requests need an explicit answer; the
Chief does not automatically grant execution approvals. An uncertain dispatch
stays visible and must not be blindly retried. Follow-up checks run while the local
service is running; this does not promise wakeups after the app and service exit.
Answer work decisions in the Chief conversation. The Chief records a new decision
receipt linked to that reply; it does not rewrite the original evidence. Execution
approval requests remain separate and require an explicit response to the exact
pending request. Work details and dependencies are available below the conversation.
Goal completion is also an explicit Chief judgment with related evidence; resolved
workers do not automatically mark the parent goal complete.

The Chief retains its selected account process while the service runs. Existing
account-capacity rules still apply to ordinary Conversations that require a separate
process. The work snapshot is bounded to 100 items, 500 dependencies and 100 pending
events; exceeding the bound produces an explicit capacity result, not partial data.

## Current architecture

- `decodex serve` is the sole product-state and side-effect owner. The same `decodex`
  executable also provides the short-lived CLI commands.
- A bundled SQLite database at `~/.decodex/server/decodex.sqlite3` is the only normal
  product store.
- `database/` owns migrations, schema verification, storage APIs, transfer tooling, and
  restart tests.
- Account credentials are stored in a narrow owner-private SQLite table. They are
  available only to the service credential adapter and never enter protocol output.
- Codex app-server remains the provider runtime. Chief threads share a retained,
  account-bound process. The service owns its lifecycle and correlated event stream;
  one worker finishing does not terminate peer threads. Ordinary Conversations retain
  their existing RuntimeSession account and thread bindings.
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
User -> Chief conversation -> same-UID service -> durable inbox
                                      |
                                      v
                         Chief thread <-> independent worker threads
                                      |
                         result / decision / next check
                                      |
                              user-facing briefing
```

The Chief model chooses its decomposition and checks. Code owns durable state,
message correlation, scheduling and actual permissions, not a mandatory sequence
of planning and review roles. A Codex link is absent until readback supplies the exact provider thread ID; a
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

The active Factory renderer and fixed Program/Review workflow are retired. Public
Program mutation commands and new-conversation Program binding are removed; hiding
navigation is not the retirement boundary. Historical Program queries and lineage
remain readable, and released migrations and recorded data remain intact.

The unsupported WorkItem board, static Coordinator/Agent/Review/Replay preview and
fake Execution Decision projection remain removed. Remote workers, multi-machine
coordination and a general automation-authoring product are not part of this slice.
Automation result intake and due checks are supported through Chief.

Ontology and graph engineering remain central to the direction of Decodex. They will be
projections over proven Goals, tasks, threads, artifacts, claims, dependencies, gates,
and evidence. They are not a second speculative execution engine.

Managed Repository orchestration is retired. Decodex does not own repository
allocation, Git registration, worktree preparation, or commit state machines.
Chief and ordinary Conversations continue to use explicit working directories.
Repository revision evidence remains available to context and supervised validation.
The diagnostic report no longer includes a managed-repository component. This change
uses exact local protocol version 2.41; update the app and service together.

The unused built-in GitHub PR/check-run write-and-verification layer is also
retired. Chief uses task-authorized tools when GitHub work is requested; Decodex
does not impose a native PR delivery workflow. This removal does not change Radar,
Publisher, or repository maintenance automation.

## Persistence compatibility

Migration 0013 adds Chief work, inbox, process bindings and saved settings. Migration
0014 preserves quota facts while adding explicit optional-window absence. Prior
account-route upgrade history remains unchanged. Accounts, credentials, routing
data, conversations and historical Program Pack bindings remain readable.
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

Start with the [OpenWiki quickstart](openwiki/quickstart.md) for the repository index.
Generated pages can lag this refactor; current source, tests and
[Chief delivery evidence](CHIEF_REFACTOR.md) take precedence for the new workflow.

## License

Decodex is licensed under GPLv3. See [LICENSE](LICENSE).

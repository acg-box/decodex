# OpenWiki Quickstart

Decodex is a Rust workspace for repo-native coding-agent orchestration: a `decodex` CLI/runtime, retained Linear issue lanes, local operator control, MCP access, a macOS account-pool app, an Astro public site, an installable Decodex Codex plugin, and auxiliary Radar/Publisher tooling. The root workspace manifest describes the package as "Decodex runtime, static site, and local operator tooling" (`Cargo.toml`). The README summarizes the product as "Repo-native agent orchestration, retained lanes, and local operator control" (`README.md`).

OpenWiki is the repo-local project knowledge surface for agents and maintainers. Runtime authority lives in source, project contracts, tests, manifests, and local runtime state; OpenWiki explains where to start and what to watch before editing.

## Start here

- [Runtime architecture](architecture/runtime-architecture.md): process topology, CLI bootstrap, app-server runs, operator HTTP/MCP, and state ownership.
- [Design rationale](decisions/design-rationale.md): why Decodex keeps loop graphs internal, autonomy authority typed, MCP/skills split, the site static, and Radar/Publisher bounded.
- [Lane Authority v2](decisions/lane-authority-v2.md): accepted target architecture for project binding, canonical lane authority, effect orchestration, migration, and telemetry. This target is not current runtime behavior until its checkpoints land.
- [Drift audits](evidence/drift-audits.md): public-safe evidence notes, current MCP remote-control watched claims, reverse checks, validation commands, and stop conditions.
- [Runtime operator workflows](workflows/runtime-operator-workflows.md): project registry, run/serve/status, lane control, recovery, intake, commit/land, accounts, and MCP workflows.
- [Contracts and data](specs/contracts-and-data.md): project config, `WORKFLOW.md`, SQLite state, Decision Contracts, Program Intake, tracker tools, review lifecycle, and commit messages.
- [Runtime contracts](specs/runtime-contracts.md): runtime state ownership, project/`WORKFLOW.md` contracts, leases/attempts, app-server protocol, tracker writeback, evidence/privacy, and recovery boundaries.
- [Runtime lifecycle](specs/runtime-lifecycle.md): lane authority, app-server protocol, tracker tools, evidence, loop runtime, review lifecycle, and autonomy control-plane boundaries.
- [Lane Authority v2 target contract](specs/lane-authority-v2.md): target records, transitions, migration rules, scenario matrix, and checkpoint gates.
- [Lane Authority v2 gate manifest](specs/lane-authority-v2-gates.md): normative scenario ids, commands, expected assertions, fixture paths, and evidence requirements for C0-C7.
- [Lane Authority v2 effect registry](specs/lane-authority-v2-effects.md): exhaustive runtime-owned mutation kinds, reconciliation and compensation rules, and adapter enforcement.
- [Lane Authority v2 checkpoints](evidence/lane-authority-v2-checkpoints.md): durable anti-drift record, review objections, validation evidence, and C0-C7 advancement state.
- [Commands and validation](operations/commands-and-validation.md): task runner, tests, targeted checks, status publishing, app/site/Radar/Publisher validation.
- [Operator runbooks](operations/operator-runbooks.md): lane-control recovery, review handoff recovery, release readiness, GitHub operations, and control-plane workflows.
- [Plugins, automations, and auxiliary tools](integrations/plugins-automations-and-auxiliary-tools.md): installable plugin lifecycle, hook guardrails, automation sync, Radar, Publisher, native App, and site boundaries.
- [Radar, Publisher, and site contracts](integrations/radar-publisher-site.md): Radar artifacts, upstream review, release deltas, social publishing, site contract, and retention.
- [Radar Publisher contracts](integrations/radar-publisher-contracts.md): artifact contracts, upstream handoff, control-plane candidates, Publisher reservations, static-site boundary, retention, and stop conditions.

## Repository map

- `apps/decodex/` builds the `decodex` CLI/runtime package (`apps/decodex/Cargo.toml`). Its public bootstrap is `apps/decodex/src/lib.rs`; the CLI surface is `apps/decodex/src/cli.rs`.
- `apps/radar/` is the Radar auxiliary tool for upstream review queues, release deltas, artifact validation, signal rendering, and bundle generation (`apps/radar/README.md`, `apps/radar/src/lib.rs`).
- `apps/decodex-publisher/` validates and reserves Decodex-owned social artifacts (`apps/decodex-publisher/README.md`, `apps/decodex-publisher/src/lib.rs`).
- `apps/decodex-app/` is a native macOS UI over local Decodex account-pool state and may launch `decodex serve` when no default local server is available (`apps/decodex-app/README.md`).
- `site/` is the static Astro product site; it must not depend on live daemon state (`site/package.json`, `openwiki/integrations/plugins-automations-and-auxiliary-tools.md`).
- `plugins/decodex/` contains the installable Decodex plugin, narrow routing skills, and lifecycle guardrail hooks (`plugins/decodex/.codex-plugin/plugin.json`).
- `automations/decodex/` and `automations/radar/` contain portable Codex App automation source; live machine-local configs are generated from these manifests (`automations/decodex/README.md`, `automations/radar/README.md`).
- `scripts/` contains repo maintenance helpers including plugin sync and macOS app staging.
- `tests/` currently contains Python tests for script-level plugin sync behavior (`tests/scripts/test_sync_installable_plugins.py`). Most Rust tests live in each app crate under `src/**/tests*`.

## Runtime in one minute

`apps/decodex/src/main.rs` only calls `decodex::run()`. The library bootstrap initializes error reporting, daily file tracing under `~/.codex/decodex/logs`, a panic-abort hook, and then runs `Cli::parse().run()` (`apps/decodex/src/lib.rs`). The CLI supports project registry, run/serve, status, lane control, MCP, intake, recovery, commit/land, account pool, app launch, probe, and validation status publishing (`apps/decodex/src/cli.rs`).

Local runtime state is under `~/.codex/decodex`: global config, `accounts.jsonl`, `projects/`, `logs/`, `agent-evidence/`, and `runtime.sqlite3` (`apps/decodex/src/runtime/paths.rs`). Project contracts are outside checkouts under `~/.codex/decodex/projects/<service-id>/` and use fixed `project.toml` plus `WORKFLOW.md` files (`apps/decodex/src/config/service.rs`, `decodex.example.toml`).

`decodex run` opens the runtime store, resolves/registers a project config, loads workflow policy, checks tracker backoff, and dispatches through `orchestrator::run_configured_cycle` (`apps/decodex/src/orchestrator/entrypoints/run.rs`). `decodex serve` is the long-running local control plane; each daemon tick reconciles active children, idle recovery, post-review orchestration, archive backlog, and due child spawns (`apps/decodex/src/orchestrator/daemon.rs`).

## First commands

Use these as discovery and validation entrypoints:

```sh
decodex --help
decodex project list
decodex status --live
decodex run --dry-run --explain
decodex serve --listen-address 127.0.0.1:8192
decodex mcp serve --transport stdio
cargo make check
```

For a source checkout without installed binaries, run Rust commands through Cargo, for example `cargo run -p decodex -- status`. For a targeted Rust gate, prefer `cargo check --all-features --all-targets --workspace` or `cargo nextest run --workspace --all-targets --all-features` (`Makefile.toml`, `openwiki/operations/commands-and-validation.md`).

## Authority and safety rules

- Do not read `.env` files or live secret-bearing config. `decodex.example.toml` is the redacted setup model and uses credential environment-variable names, not token values.
- Do not hand-edit `~/.codex/decodex/runtime.sqlite3`, runtime DB rows, hidden child process state, Linear labels, GitHub merges, or internal Program graph ids to simulate lifecycle controls.
- Use `decodex commit` and `decodex land` for Decodex-owned commit/landing authority; the installable plugin hook blocks raw `git commit` and `gh pr merge` inside Decodex scope (`plugins/decodex/scripts/decodex_lifecycle_hook`).
- Treat Linear and GitHub as collaboration mirrors. The local runtime SQLite store is the single-machine source of truth for leases, attempts, protocol summaries, private execution events, Program Intake, review lifecycle records, run control, and connector backoff (`openwiki/specs/contracts-and-data.md`, `apps/decodex/src/state/sqlite_store/schema.rs`).
- For project knowledge work, update OpenWiki directly and keep it aligned with source, tests, and manifests.

## Recent development context

Recent history shows active work around lifecycle hook guardrails, automation reasoning effort, baseline canonicalization before dispatch, local validation landing status gates, and review/landing lifecycle hardening. The important current themes are: keep Decodex-owned lifecycle actions behind typed commands, protect baseline canonicalization before normal/program/retry dispatch, preserve local validation evidence for landing, and avoid letting stale knowledge or plugin skills become runtime authority.

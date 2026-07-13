# OpenWiki Quickstart

Decodex is cutting over to the accepted vNext agent-workspace architecture. The active
Rust workspace currently contains the ownership skeleton and explicitly unavailable
composition roots; it does not provide the old Linear/SQLite runtime. The v0.2 source is
preserved under `apps/decodex/` as frozen provenance and is excluded from the active
workspace. Radar, Publisher, the static site, plugins, and automation source remain
outside the runtime rewrite until their owners adopt them.

OpenWiki is the repo-local project knowledge surface for agents and maintainers. Runtime authority lives in source, project contracts, tests, manifests, and local runtime state; OpenWiki explains where to start and what to watch before editing.

## Start here

- [Runtime architecture](architecture/runtime-architecture.md): process topology, CLI bootstrap, app-server runs, operator HTTP/MCP, and state ownership.
- [Design rationale](decisions/design-rationale.md): why Decodex keeps loop graphs internal, autonomy authority typed, MCP/skills split, the site static, and Radar/Publisher bounded.
- [vNext authority decision](decisions/vnext-authority.md): the accepted product, ownership, state-authority, cutover, and delivery decision for the rebuild.
- [vNext authority contract](specs/vnext-authority.md): normative entities, runtime boundaries, protocol, account continuity, non-goals, and migration contract for later implementation.
- [vNext gate manifest](specs/vnext-gates.md): ordered feasibility and implementation gates, downstream issue ownership, and decision-changing falsifiers.
- [XY-1262 Codex runtime proof](evidence/vnext-codex-runtime-proof.md): shared-home, ownership, schema, collaboration, cross-account, fallback, crash, and typed-quota evidence for the Codex feasibility gate.
- [Lane Authority v2](decisions/lane-authority-v2.md): superseded historical target retained as architecture and incident provenance; C1-C7 are frozen and must not be implemented.
- [Drift audits](evidence/drift-audits.md): public-safe evidence notes, current MCP remote-control watched claims, reverse checks, validation commands, and stop conditions.
- [v0.2 freeze receipt](evidence/v0.2-freeze.md): exact trusted tag, cold-config and automation inventory, frozen legacy work, preserved incident evidence, cleanup ownership, and the unresolved SQLite-backup gap.
- [GPUI feasibility evidence](evidence/gpui-feasibility.md): pinned XY-1263 candidate/toolchain, macOS runtime and package probes, bounded-history measurements, and the accessibility no-go that freezes downstream UI work.
- [Runtime operator workflows](workflows/runtime-operator-workflows.md): project registry, run/serve/status, lane control, recovery, intake, commit/land, accounts, and MCP workflows.
- [Contracts and data](specs/contracts-and-data.md): current v0.2 project config, SQLite, Decision Contract, Program Intake, tracker, review, and commit behavior; superseded for vNext target work.
- [Runtime contracts](specs/runtime-contracts.md): current v0.2 state, app-server, tracker, evidence/privacy, and recovery contracts; superseded for vNext target work.
- [Runtime lifecycle](specs/runtime-lifecycle.md): current v0.2 lane, app-server, tracker, review, and autonomy lifecycle; superseded for vNext target work.
- [Lane Authority v2 target contract](specs/lane-authority-v2.md), [effect registry](specs/lane-authority-v2-effects.md), [gate manifest](specs/lane-authority-v2-gates.md), and [checkpoint ledger](evidence/lane-authority-v2-checkpoints.md): superseded provenance only, not active implementation authority.
- [Commands and validation](operations/commands-and-validation.md): task runner, tests, targeted checks, status publishing, app/site/Radar/Publisher validation.
- [Operator runbooks](operations/operator-runbooks.md): lane-control recovery, review handoff recovery, release readiness, GitHub operations, and control-plane workflows.
- [Plugins, automations, and auxiliary tools](integrations/plugins-automations-and-auxiliary-tools.md): installable plugin lifecycle, hook guardrails, automation sync, Radar, Publisher, native App, and site boundaries.
- [Radar, Publisher, and site contracts](integrations/radar-publisher-site.md): Radar artifacts, upstream review, release deltas, social publishing, site contract, and retention.
- [Radar Publisher contracts](integrations/radar-publisher-contracts.md): artifact contracts, upstream handoff, control-plane candidates, Publisher reservations, static-site boundary, retention, and stop conditions.

## Repository map

- `crates/decodex-core/` owns pure domain/application authority contracts and ports; it has no dependencies.
- `crates/decodex-protocol/` owns the vNext version and loopback-only endpoint contract shared with clients.
- `crates/decodex-postgres/` owns the PostgreSQL product-state adapter boundary; its production implementation remains unavailable until XY-1267.
- `crates/decodex-codex/` owns the shared-normal-`~/.codex` adapter boundary; runner behavior remains unavailable until XY-1270.
- `crates/decodex-runtime/` owns `decodexd` service assembly and is the only library owner that composes protocol and infrastructure adapters.
- `apps/decodexd/`, `apps/decodex-cli/`, and `apps/decodex-gpui/` are composition roots. The client roots depend only on the protocol crate; the GPUI binary remains a disabled print-and-exit stub while XY-1263 is failed.
- `apps/decodex/` is the frozen v0.2 package. It remains in Git for provenance but is excluded from Cargo workspace membership and must not be used by vNext.
- `apps/radar/` is the Radar auxiliary tool for upstream review queues, release deltas, artifact validation, signal rendering, and bundle generation (`apps/radar/README.md`, `apps/radar/src/lib.rs`).
- `apps/decodex-publisher/` validates and reserves Decodex-owned social artifacts (`apps/decodex-publisher/README.md`, `apps/decodex-publisher/src/lib.rs`).
- `apps/decodex-app/` is a native macOS UI over local Decodex account-pool state and may launch `decodex serve` when no default local server is available (`apps/decodex-app/README.md`).
- `site/` is the static Astro product site; it must not depend on live daemon state (`site/package.json`, `openwiki/integrations/plugins-automations-and-auxiliary-tools.md`).
- `plugins/decodex/` contains the installable Decodex plugin, narrow routing skills, and lifecycle guardrail hooks (`plugins/decodex/.codex-plugin/plugin.json`).
- `automations/decodex/` and `automations/radar/` contain portable Codex App automation source; live machine-local configs are generated from these manifests (`automations/decodex/README.md`, `automations/radar/README.md`).
- `scripts/` contains repo maintenance helpers including plugin sync and macOS app staging.
- `tests/scripts/test_vnext_architecture.py` enforces the exact vNext dependency graph, client isolation, and exclusion-with-preservation of the legacy package.

## Runtime in one minute

`apps/decodexd` composes the PostgreSQL and Codex adapter boundaries through
`decodex-runtime` and serves the typed V1 protocol at loopback-only
`ws://127.0.0.1:49152/v1/ws`. It opens no database, repository, or Codex process. The
protocol supports current/previous-minor negotiation, typed command receipt/result and
event envelopes, bounded snapshots/queues/wire text, fixed-capacity in-lifetime
idempotency, cursor resume, and snapshot fallback. The `decodex` and GPUI roots compile
against `decodex-protocol` only
and still report their unsupported or disabled state.

No vNext product state is persisted yet. PostgreSQL is the accepted future product-state
authority, `~/.codex` remains Codex-owned shared continuation state, and the accepted
Decodex-owned target is `~/.decodex`. XY-1267 and XY-1268 own those implementations.
The legacy `~/.codex/decodex` SQLite/config layout is frozen provenance, not a vNext
input or fallback.

There is no active scheduling, CLI operation, account routing, Codex adapter, PostgreSQL
store, authenticated HTTP artifact path, remote binding, or GPUI product behavior in
this slice. Authentication and TLS are disabled; loopback refusal is the enforced
network boundary until the later remote-security gate.

## First commands

Use these as discovery and validation entrypoints:

```sh
cargo run -p decodexd
cargo run -p decodex-cli
cargo run -p decodex-gpui
cargo make test-vnext-architecture
cargo make check
```

`decodexd` starts the loopback protocol service and runs until stopped. The client and
GPUI binaries report foundation/unavailability state and exit. For a targeted Rust gate,
prefer
`cargo check --all-features --all-targets --workspace` or
`cargo nextest run --workspace --all-targets --all-features` (`Makefile.toml`,
`openwiki/operations/commands-and-validation.md`).

## Authority and safety rules

- Do not read `.env` files or live secret-bearing config. `decodex.example.toml` is the redacted setup model and uses credential environment-variable names, not token values.
- Do not route vNext through `apps/decodex`, legacy SQLite, Linear lanes, or the legacy operator transport.
- Use `decodex commit` and `decodex land` for Decodex-owned commit/landing authority; the installable plugin hook blocks raw `git commit` and `gh pr merge` inside Decodex scope (`plugins/decodex/scripts/decodex_lifecycle_hook`).
- PostgreSQL becomes vNext product-state authority only when its owning gate lands. Until then, unavailable is the only supported service state; there is no fallback authority.
- For project knowledge work, update OpenWiki directly and keep it aligned with source, tests, and manifests.

## Recent development context

XY-1265 established compile-time ownership and composition. XY-1266 implements the
loopback protocol foundation; XY-1267 owns PostgreSQL-backed product state and durable
transactions, and XY-1268 owns the API-only CLI/path cutover. Codex behavior, account
routing, remote security, HTTP artifacts, and GPUI product work remain with their later
owners and gates.

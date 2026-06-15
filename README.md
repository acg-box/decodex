<div align="center">

# Decodex

Repo-native agent orchestration, upstream Codex radar, and public publishing.

[![License](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Language Checks](https://github.com/hack-ink/decodex/actions/workflows/language.yml/badge.svg?branch=main)](https://github.com/hack-ink/decodex/actions/workflows/language.yml)
[![Release](https://github.com/hack-ink/decodex/actions/workflows/release.yml/badge.svg)](https://github.com/hack-ink/decodex/actions/workflows/release.yml)
[![GitHub tag (latest by date)](https://img.shields.io/github/v/tag/hack-ink/decodex)](https://github.com/hack-ink/decodex/tags)
[![GitHub last commit](https://img.shields.io/github/last-commit/hack-ink/decodex?color=red&style=plastic)](https://github.com/hack-ink/decodex)
[![GitHub code lines](https://tokei.rs/b1/github/hack-ink/decodex)](https://github.com/hack-ink/decodex)

</div>

## Feature Highlights

- Rust CLI and runtime for repo-native retained coding-agent lanes.
- Natural-language-first loop-runtime contract with research/decision promotion,
  internal execution-program state, and normal Linear issue lanes.
- Native macOS app for Decodex Codex account-pool management.
- Explicit project registry under `~/.codex/decodex/projects/<service-id>/`.
- Local operator listener with a dashboard at `/` and `/dashboard`, WebSocket
  snapshot/control traffic at `/dashboard/control`, Decodex App snapshot/account
  APIs under `/api/`, and `GET /livez` for liveness.
- Static Astro site that publishes curated Decodex Radar and Publisher output.
- Deterministic GitHub upstream Radar pipeline for review queues, change bundles,
  release deltas, rendered signal entries, and content validation.
- Repo-local Radar skills for upstream Codex triage, code analysis, release analysis,
  signal drafting, and X publishing.
- Publisher workflow for checked-in upstream reviews, impact classification, curated
  public signals, and automated low-frequency X publication records for
  `@decodexspace`.
- Installable Decodex plugin with reusable agent-facing skills for planning,
  manual CLI, automation, commit, land, and labels.
- Repository documentation split by question type into spec, runbook, reference, and
  decision lanes.

## Status

Prototype / in active development.

This repository now integrates the former runtime repository and the static signal site
into one Decodex workspace. The static site remains the public surface by default.
Runtime and operator behavior does not become a public site backend just because both
surfaces live in one workspace.

Supported runtime host targets are macOS and Linux. Windows remains unsupported for the
runtime.

## App identity

- Product display name: **Decodex**.
- Runtime CLI binary: **decodex**.
- Public site: <https://decodex.space>.
- Lower-case `decodex` remains only for stable technical identifiers such as the
  repository slug, Cargo package name, CLI binary, plugin package, config keys, and
  schema names.

## Workspace posture

- `apps/decodex/` owns the Rust package that builds the `decodex` CLI and runtime.
- `apps/decodex-app/` owns the native macOS app that manages Decodex
  Codex accounts through the bundled Rust app helper.
- `site/` owns the Astro static site and checked-in public content.
- `apps/decodex/src/radar.rs` owns Rust Radar queue, release-delta, bundle, render,
  validation, backfill, and ledger commands.
- `scripts/github/` owns the automation-only Codex AI analysis helper and shared
  schema support for that helper.
- `artifacts/github/` owns checked-in review queues, upstream reviews, GitHub bundles,
  impact records, and editorial analysis drafts.
- `artifacts/archive/` owns checked-in recovery manifests for cold Radar batches stored
  as GitHub Release assets.
- `artifacts/social/` owns checked-in Publisher publication records and generated-media
  evidence.
- `plugins/decodex/` owns the installable Decodex plugin and reusable agent-facing
  skills.
- `dev/skills/` owns repository-development skills for Radar analysis and Publisher
  publishing. They are not packaged with the installable Decodex plugin.
- `docs/` remains the authoritative documentation surface.

Runtime authority stays in `apps/decodex/src/`, the registered project contracts under
`~/.codex/decodex/projects/<service-id>/`, and the governing specs under `docs/spec/`.
Public site authority stays in `site/`, `apps/decodex/src/radar.rs`,
`artifacts/github/`, and the site/content specs.

Historical Radar trace is local by default. `decodex radar refresh-upstream-queue`
writes `.decodex/radar.sqlite3` and refreshes `upstream_review_queue/v1` so every
inspected upstream commit can be tracked before AI review decides whether it deserves
Decodex follow-up, public content, or only ledger trace.

## Runtime platform support

- The Decodex runtime contract is Unix-only: macOS and Linux.
- Windows is outside the runtime contract.
- Current Codex/app-server compatibility is capability-gated and recorded in
  [`docs/spec/app-server.md`](docs/spec/app-server.md).
- The public site is static and deploys through GitHub Pages.
- Starting `decodex serve` without its `--config` option schedules enabled projects
  from the explicit registry only. Operator and App snapshots still expose active
  runtime DB-backed attempts for disabled projects, because disabling a project pauses
  future dispatch rather than deleting visibility or ownership. It does not scan Codex
  history, repo-local config files, or currently open worktrees to infer projects.

## Usage

### Runtime CLI

From the workspace root:

```sh
cargo run -p decodex --bin decodex -- --help
cargo run -p decodex --bin decodex -- probe stdio://
cargo run -p decodex --bin decodex -- project list
cargo run -p decodex --bin decodex -- status
cargo run -p decodex --bin decodex -- status --live
cargo run -p decodex --bin decodex -- diagnose --json
cargo run -p decodex --bin decodex -- maintenance prune --dry-run
cargo run -p decodex --bin decodex -- lane steer <ISSUE> --run-id <RUN_ID> --expected-turn-id <TURN_ID> --message <TEXT>
cargo run -p decodex --bin decodex -- research compile --intent "research X"
cargo run -p decodex --bin decodex -- research compile --input research-design-run.json
cargo run -p decodex --bin decodex -- research promote <CONTRACT_ID>
cargo run -p decodex --bin decodex -- intake goal --project decodex <CONTRACT_ID> --dry-run
cargo run -p decodex --bin decodex -- intake goal --project decodex <CONTRACT_ID> --apply
cargo run -p decodex --bin decodex -- intake issues --project decodex XY-1 XY-2 --dry-run
cargo run -p decodex --bin decodex -- intake issues --project decodex XY-1 XY-2 --apply
cargo run -p decodex --bin decodex -- radar refresh-upstream-queue
cargo run -p decodex --bin decodex -- radar refresh-release-delta
cargo run -p decodex --bin decodex -- radar validate
cargo run -p decodex --bin decodex -- run --dry-run
cargo run -p decodex --bin decodex -- serve --listen-address 127.0.0.1:8192
```

Project-scoped commands accept `--config <PROJECT_DIR>` after the subcommand when the
operator wants to override registry-based project resolution for that command.
`decodex status` first tries to reuse the default local operator listener's published
`GET /api/operator-snapshot` when the snapshot is recent, covers the requested project,
and has at least the requested `--limit`. JSON output marks this as
`"status_source": "operator_snapshot_cache"` and includes `snapshot_age_seconds`.
If that cache is missing, stale, mismatched, or too small, the command falls back to a
direct local runtime snapshot and reports `status_cached_snapshot_unavailable` in
`warning_details`. Use `decodex status --live` when the operator needs fresh
Linear/GitHub readback before acting; `--live` bypasses the cached snapshot path and
marks JSON output as `"status_source": "live_observers"`. Use the Accounts API refresh
path, such as `GET /api/accounts?refresh=1`, when the operator needs fresh ChatGPT
account usage probes.
`decodex serve` uses hardcoded scheduler cadences: the local control-plane loop
publishes snapshots every 15 seconds, and Linear-backed queue/status scans run at
most every 5 minutes per project unless an operator or agent requests an explicit
scan with `POST /api/linear-scan`. Persisted Execution Programs do not wait for that
ordinary queue-label scan: the runtime keeps the Program graph in local state,
refreshes only the mapped Linear issue facts needed for readiness, and directly
dispatches ready DAG nodes with `program` dispatch mode.

`decodex research compile` is the native Decodex research/design entrypoint. It
accepts minimal natural-language intake or a structured research/design JSON packet,
then persists a `decodex.decision_contract/1` payload in local runtime SQLite. The
Decodex research method frames the question first, records evidence, compares
realistic options, forms a challenge-ready judgment, resolves skeptic objections, and
then ends as `decision_ready`, `not_decision_ready`, `blocked`, or
`needs_human_decision`. A compiled contract is latent and cannot queue work, mutate
tracker state, set goals, or authorize implementation. `decodex research promote`
records explicit acceptance for a stored contract; only promoted contracts may later
feed issue shaping or internal Execution Program readiness.

`decodex intake goal` materializes a promoted Decision Contract. `--dry-run` prints
the proposed normal Linear issues, dependencies, conflict domains, and dispatch plan
without mutating Linear or local Program Intake rows. `--apply` creates or updates the
generated normal Linear issue briefs and persists the internal Execution Program plus
contract/program links in runtime SQLite. Apply does not run implementation or apply
queue labels; the persisted Program becomes eligible for direct graph dispatch on the
next scheduler pass. If the contract is still
latent, needs a decision, or lacks issue-shaping authority, intake stops before
creating executable work.

`decodex intake issues` materializes a supplied batch of existing Linear issues into
local Program Intake state. `--dry-run` prints the deterministic ready/held/blocked
report without mutation. `--apply` persists the local Program Intake Plan, Execution
Program, and issue mappings. It never applies or removes service queue labels; ready
mapped nodes are dispatched directly by the Program scheduler instead of being
converted into queued-label work.

### Install from Source

```sh
git clone https://github.com/hack-ink/decodex
cd decodex

cargo install --path apps/decodex --force
decodex --version
```

### Project contracts

Project contracts are managed outside checkouts under
`~/.codex/decodex/projects/<service-id>/` with fixed filenames:

- `project.toml` for service paths and credential environment-variable names
- `WORKFLOW.md` for execution policy

The redacted template for a project config lives at `decodex.example.toml`.
Phase-scoped app-server goals are mandatory for retained lane execution. Decodex
rejects a connected Codex app-server that lacks required `thread/goal/*` methods
instead of falling back to ordinary continuation.
When a project enables `[codex.accounts]`, the shared ChatGPT account pool is
`~/.codex/decodex/accounts.jsonl`; it is global Decodex state, not a project-local
file, and project configs do not own an account-pool path override. Set
`[codex.accounts].fixed_account` in `~/.codex/decodex/config.toml` to pin all new
account-pool runs to one account. When that global selector is absent, Decodex balances
new runs across the pool. The operator dashboard Accounts UI writes and clears the same
global selector; project configs do not pin specific accounts. Account display-name
rerolls are also global Decodex state under `[codex.account_names.offsets]` in
`~/.codex/decodex/config.toml` so the operator dashboard and Decodex App show the same
privacy-preserving names. Client-only presentation preferences such as theme, sorting,
and whether identities are hidden remain local to each UI. Usage probes also read
Codex profile token stats for local Accounts displays. Bounded seven-day account
usage estimates are kept in
`~/.codex/decodex/account-usage-history.jsonl`; the file stores daily percentage
snapshots plus non-secret capacity weights for local display and no token material.
Refresh authentication failures mark the account `auth_failed` in `accounts.jsonl`;
Decodex will not select or manually activate that account again until it is re-logged
or replaced.
To switch the account used by the Codex CLI itself, run
`decodex account use <selector>` or use the Decodex App row action; this overwrites
`$CODEX_HOME/auth.json` or `~/.codex/auth.json` from the matching `accounts.jsonl`
entry. Later account-pool token refreshes also update that Codex auth target when it
currently contains the same account id.

`decodex diagnose --json` writes the local agent evidence index under
`~/.codex/decodex/agent-evidence/<service-id>/` and prints the same handoff index for
repair agents.

`decodex maintenance prune` defaults to the same read-only report as
`decodex maintenance prune --dry-run`. Add `--apply` to rotate
oversized local logs and agent-evidence event streams, prune old backup files, compact
old terminal-run protocol events after preserving their summary, and checkpoint the
SQLite WAL. `decodex serve` also runs the auto-safe subset at startup and periodically
while it is polling, including 14-day retention for rotated logs and agent-evidence
event streams plus 14-day protocol-event compaction for terminal unowned runs after
the compact summary is preserved. If the runtime database is busy or candidate
detection fails, serve logs a warning and continues polling.

## Static Site

The public site is an Astro static site under `site/`. It renders checked-in content and
generated JSON artifacts, then deploys through GitHub Pages.

The public site owns:

- Codex signal cards
- release-delta presentation
- continuous Radar status presentation
- static assets and public page rendering

The public site does not own:

- retained-lane scheduling
- tracker writes
- local operator state
- app-server orchestration
- the operator dashboard served by `decodex serve`

The static-site boundary is recorded in `docs/decisions/static-public-site.md`. GitHub
Pages setup for `https://decodex.space` lives in `docs/runbook/github-pages-deploy.md`.

## Upstream Radar Pipeline

The upstream Codex Radar path starts deterministic and becomes editorial only after
Codex automation reviews source evidence:

- `decodex radar refresh-upstream-queue` records every observed recent upstream
  commit, resolves PRs when possible, and refreshes
  `artifacts/github/review-queue/openai-codex-latest.json`.
- `dev/skills/README.md` routes the repo-local Radar and editorial instructions. They
  are not part of the installable Decodex plugin distribution.
- `decodex radar bundle build` builds normalized GitHub bundles under
  `artifacts/github/bundles/` when a queued subject needs full source context.
- `decodex radar backfill-release-range` fills release-window gaps before a release
  or prerelease summary, but daily Radar still starts from the commit stream.
- `docs/spec/upstream-review.md` records the queue and AI review boundary.
- `docs/spec/upstream-impact.md` records how upstream Codex changes are classified for
  public signals and Control Plane follow-up work.
- `decodex radar render-signal` renders reviewed analysis drafts into site content.
- `decodex radar validate` validates the published signal collection and checked Radar
  artifact contracts.
- `decodex radar refresh-upstream-queue`, `decodex radar refresh-release-delta`,
  `decodex radar bundle validate`, `decodex radar ledger ...`, `decodex radar
  render-signal`, `decodex radar backfill-release-range`, and `decodex radar
  validate` provide the Rust-owned command surface for deterministic queue refresh,
  release-delta refresh, bundle validation, local ledger maintenance, signal
  rendering, release-window backfill, and checked Radar artifact validation.
- `docs/spec/social-publishing.md` and
  `docs/runbook/social-publishing-workflow.md` govern automated low-frequency X
  publication for `@decodexspace`.
- `.github/workflows/refresh-upstream-radar.yml` refreshes deterministic upstream
  queue metadata every six hours.
- `.github/workflows/refresh-release-delta.yml` refreshes release and prerelease
  checkpoint metadata every hour.
- `.github/workflows/deploy-pages.yml` publishes the Astro site to GitHub Pages on
  pushes to `main`.

The governing workflow lives at `docs/runbook/local-github-signal-workflow.md`.

## Operator Dashboard

`decodex serve` owns the local operator listener. It serves the operator dashboard from
`GET /` and `GET /dashboard`; published snapshots, active-run updates, and local
dashboard controls flow through the `/dashboard/control` WebSocket. The HTTP surface is
kept to dashboard pages/assets, `GET /livez`, and the local account-control API used by
Decodex App.

For dashboard UI development, use one mock operator dashboard server for both the
browser dashboard and Decodex App preview:

```sh
node dev/operator-dashboard-mock.mjs --listen-address 127.0.0.1:57399
node dev/operator-dashboard-mock.mjs --listen-address 127.0.0.1:57399 --use-codex-auth
```

That single mock listener serves `GET /dashboard`, `GET /api/accounts`, and the
dashboard authority WebSocket at `ws://127.0.0.1:57399/dashboard/control`. When
previewing Decodex App against the mock, point the App at the same base URL with
`DECODEX_APP_SERVER_URL=http://127.0.0.1:57399`; do not start a second mock server for
the App. This environment variable is authoritative: when it is set, Decodex App
connects only to that server and reports an error instead of falling back to the
default `127.0.0.1:8192` runtime.

```sh
DECODEX_APP_SERVER_URL=http://127.0.0.1:57399 open -n target/decodex-app/Decodex.app
```

Use hidden `decodex serve --dev --listen-address <ADDR>` only when
developing local account/app snapshot APIs against real runtime state while explicitly
avoiding scheduler activity. Dev mode deliberately does not register projects, poll
Linear, dispatch work, or accept `--config`. Decodex App's normal
fallback server is ordinary `decodex serve --listen-address 127.0.0.1:8192`; the CLI
owns the default scheduler cadences. App launch connects to an
existing live default listener instead of starting a duplicate server only when
`DECODEX_APP_SERVER_URL` is unset. For dashboard and App preview UI work, prefer the
single mock server above.

The dashboard semantics and local-vs-external state boundary live in
`docs/reference/operator-control-plane.md`.

## Development

Repo-native validation is owned by `Makefile.toml`.

Runtime checks follow the Decodex task structure:

```sh
cargo make check
cargo make fmt
cargo make lint
cargo make test
```

Whole-workspace checks include runtime, static-site, and content validation:

```sh
cargo make checks
```

Static-site/content checks are available separately:

```sh
cargo make decodex-checks
```

## Workspace Layout

The tracked workspace currently keeps:

- `apps/decodex/` as the Rust package that builds the `decodex` CLI and runtime
- `site/` as the Astro static site for the public Decodex signal surface
- `scripts/github/` as the deterministic GitHub collection, normalization, render, and
  validation script surface
- `artifacts/github/` as checked-in GitHub bundle and analysis artifacts
- `plugins/decodex/` as the canonical installable Decodex plugin source
- `dev/skills/` as repo-development Radar analysis and Publisher publishing skills that
  are not packaged with the installable Decodex plugin
- `docs/spec/` as the normative runtime, workflow, site, and content contract lane
- `docs/runbook/` as the operator procedures, validation sequences, deployment steps,
  and content workflow lane
- `docs/reference/` as the current repository and artifact surface map lane
- `docs/decisions/` as the durable design-rationale lane
- `docs/research/` as legacy or supporting machine-authored research artifacts; current
  Decodex research authority flows through runtime-local Decision Contracts
- `dev/` as local development helpers outside `dev/skills/`, such as the operator
  dashboard mock server
- `assets/` as generated Decodex App icon source notes, Icon Composer foreground,
  generated `.icns`, and menu bar template assets
- `.github/` as CI, release, Pages deployment, and content-refresh workflows

Generated or local-only directories such as `target/`, `site/dist/`, `site/.astro/`,
`.worktrees/`, `.workspaces/`, and `.codex/` are not part of the tracked repository
structure. For the authoritative layout and ownership map, read
`docs/reference/workspace-layout.md`.

## Documentation

- Product and development overview: this `README.md`
- Unified documentation router: `docs/index.md`
- Natural-language loop-runtime contract: `docs/spec/loop-runtime.md`
- Normative specs: `docs/spec/index.md`
- Procedural runbooks: `docs/runbook/index.md`
- Current implementation references: `docs/reference/index.md`
- Durable design rationale: `docs/decisions/index.md`
- Documentation policy and placement rules: `docs/policy.md`

## Support Me

If you find this project helpful and would like to support its development, you can buy me a coffee!

Your support is greatly appreciated and motivates me to keep improving this project.

- **Fiat**
    - [Ko-fi](https://ko-fi.com/hack_ink)
    - [Afdian](https://afdian.com/a/hack_ink)
- **Crypto**
    - **Bitcoin**
        - `bc1pedlrf67ss52md29qqkzr2avma6ghyrt4jx9ecp9457qsl75x247sqcp43c`
    - **Ethereum**
        - `0x3e25247CfF03F99a7D83b28F207112234feE73a6`
    - **Polkadot**
        - `156HGo9setPcU2qhFMVWLkcmtCEGySLwNqa3DaEiYSWtte4Y`

Thank you for your support!

## Appreciation

We would like to extend our heartfelt gratitude to the following projects and contributors:

- The Rust community for their continuous support and development of the Rust ecosystem.

## Additional Acknowledgements

- TODO

<div align="right">

### License

<sup>Licensed under [GPL-3.0](LICENSE).</sup>

</div>

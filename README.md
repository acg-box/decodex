<div align="center">

# Decodex

Local-first agent workspace orchestration.

[![License](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![GitHub tag (latest by date)](https://img.shields.io/github/v/tag/hack-ink/decodex)](https://github.com/hack-ink/decodex/tags)
[![GitHub last commit](https://img.shields.io/github/last-commit/hack-ink/decodex?color=red&style=plastic)](https://github.com/hack-ink/decodex)
[![GitHub code lines](https://tokei.rs/b1/github/hack-ink/decodex)](https://github.com/hack-ink/decodex)

</div>

## vNext foundation status

The active Rust workspace is the product-incomplete vNext foundation. `decodexd` now
serves the versioned structured-JSON WebSocket protocol at
`~/.decodex/server/decodex.sock`. The local profile permits only the exact configured
effective UID. Both client and server verify kernel peer credentials for every connection.
The WebSocket route remains `/v1/ws`, and the handshake URI does not dial TCP. The server
accepts only exact-current protocol V2.0,
publishes bounded snapshots and resumable ordered events, deduplicates commands for one
server lifetime in a fixed-capacity ledger, and disconnects clients whose bounded
outbound queue fills. It loads the typed `~/.decodex/config.toml`, retains the stable
server-host identity, and makes PostgreSQL product state available only after the
least-privilege runtime identity verifies the exact PostgreSQL 18 catalog and configured
authority without running DDL. A separate explicit operator command can install the one
canonical `schema.sql` transactionally on an empty target through a schema-owner identity;
normal serve never resolves or retains that identity. Runtime verification requires safe
trigger and function ownership, an effective `origin` replication role, a closed exact-signature
inventory with canonical metadata and source for every runtime-callable function, intact
retention triggers with no additional trigger, rule, or policy execution path,
dependency-based extension-control closure, and USAGE-only identity sequences across the
login role and every SET-reachable role. The operator must also pin the expected PostgreSQL
Unix-peer UID; descriptor-pinned socket metadata and kernel peer credentials are verified before
the runtime identity authenticates. Missing, malformed, unsafe, unreachable,
authentication-failed, or incompatible configuration remains typed unavailable with no fallback.
ProductStore, Quick Task, and ManagedRepository readiness are independent. Protocol V2.0
retains bounded read-only doctor/status, Conversation-history, and immutable execution-decision
queries outside mutation receipts and adds account lifecycle, the shared Reset Card service,
and an independent bounded account-profile query. V1.x and other V2 minor revisions are
refused before application payload handling. Doctor reads live-revalidate the retained
PostgreSQL endpoint and exact current authority without DDL or repinning.
The active `decodex status` and `decodex doctor` commands are API-only clients of that
V2.0 query. They select the active or `--profile NAME` typed profile without echoing its
name, pin the stable server identity before accepting a snapshot or report, and emit human text or
`--output json` under `decodex/cli-diagnostics/1`. Exit status is 0 only when all checks
are ready, 1 for a complete report containing unavailable or unknown checks, and 2 for a
closed client/configuration/protocol failure, including an incomplete current component set.
`decodex account list` reads the canonical account registry. `decodex account profile`
reads one bounded per-account profile and redacts email unless `--include-email` is set.
`decodex fast-mode status` reads the current Codex Fast mode setting.
`decodex fast-mode set --enabled BOOL` atomically updates only
`[features].fast_mode` in `~/.codex/config.toml`. It creates no backup and no second
configuration authority.
The `decodex reset-card list`,
`use`, and `status` commands are thin clients of the common daemon service. The public contract
uses a vNext account UUID, exact revision,
and grant/expiry descriptor. Accounts in `available` or `depleted` state are admitted.
The Rust service and CLI support the repository's macOS and Linux runtime hosts. Only
the native SwiftUI client is macOS-specific.
Reset-card operations currently accept only a local profile. The client rejects a
remote profile before it opens a connection because the repository has no
authenticated remote reset-card transport. Account and inventory JSON bind results to
the selected profile name and stable server UUID. Later list, use, and status calls can
retain that authority with global `--profile NAME --expected-server-id UUID` options.
The caller creates and persists the `use` idempotency key before it invokes the CLI;
the CLI never substitutes or generates that key.
`decodexd` alone reads credentials, starts the Codex app-server, resolves and persists
the opaque credit ID, performs the effect, and reconciles fresh state. Restart recovery
uses the same exact ID and idempotency key. It never rematches another card. The Codex
schema must advertise both `account/rateLimits/read` and
`account/rateLimitResetCredit/consume`.

The local namespace uses a persistent single-link lock, fixed staging socket, and
same-directory descriptor-relative publication. The runtime owns all session and command
tasks in one set and directly owns daemon service futures. Shutdown closes new Reset Card
provider work, settles already registered work, and empties the task set before
identity-checked cleanup, listener close, and lock release. `decodexd` maps SIGINT and
SIGTERM to this graceful path; a later start safely recovers a provably stale socket after
SIGKILL. Codex conversation dispatch, remote or cross-UID transport, application PKI,
and GPUI product behavior are still unavailable until their owning slices land.
The frozen v0.2 source remains under
`apps/decodex/` as provenance and is excluded from the active Cargo workspace; it is not
a compatibility runtime.

## Frozen v0.2 runtime reference

All runtime, operator, CLI, configuration, and path claims below that describe
retained lanes, Linear, SQLite, `apps/decodex/`, `~/.codex/decodex`, or
`decodex serve` are frozen v0.2 reference material regardless of present-tense wording
or heading level. They are not active vNext capability, authority, aliases, or fallback
behavior.

### Feature highlights at freeze

- Rust CLI and runtime for repo-native retained coding-agent lanes.
- Natural-language-first loop-runtime contract with accepted decision intake,
  internal execution-program state, and normal Linear issue lanes.
- Objective-driven project autonomy design with first-class Objective Contracts,
  typed signals, non-executable proposals, and normal Program Intake execution.
- Native macOS app for Decodex Codex account-pool management.
- Explicit project registry under `~/.codex/decodex/projects/<service-id>/`.
- Local operator listener with WebSocket snapshot/control traffic at
  `/dashboard/control`, Decodex App snapshot/account APIs under `/api/`, and
  `GET /livez` for liveness.
- Static Astro site for the public Decodex product surface and app download entry.
- Installable Decodex agent plugin for runtime planning, operations, commit, and
  landing workflows.
- GitHub CodeQL workflow for required repository code-scanning rules.
- OpenWiki-backed project knowledge under `openwiki/`, split by architecture,
  workflows, contracts, operations, and integrations.

## Frozen v0.2 status

Prototype / in active development.

This repository owns the Decodex runtime, native app, static site, installable plugins,
Radar auxiliary tooling, and repo-local automation source under `automations/radar`
and `automations/decodex`. Recurring Codex App automation execution and private
generated state stay outside tracked source; generated Radar artifacts must stay under
`.agent/automations/radar/cache`, while Publisher social artifacts stay under
`.agent/automations/decodex/cache/social`.

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

- `crates/decodex-core/`, `crates/decodex-protocol/`, `crates/decodex-postgres/`,
  `crates/decodex-codex/`, and `crates/decodex-runtime/` are the five active vNext
  library owners.
- `apps/decodexd/`, `apps/decodex-cli/`, and `apps/decodex-gpui/` are the active vNext
  composition roots. `decodexd` owns the same-UID Unix WebSocket server; the CLI is the
  bounded diagnostic and reset-card service client, while GPUI still reports disabled
  capability.
- `apps/decodex/` preserves the frozen v0.2 package outside the active Cargo workspace.
- `apps/radar/` owns the standalone Radar auxiliary tool for upstream evidence,
  release-delta, signal rendering, validation, and local ledger workflows.
- `apps/decodex-publisher/` owns the standalone Publisher boundary for content
  evidence, xurl publication, exact readback, outcomes, and social validation.
- `apps/decodex-app/` owns the native macOS app. Its single account surface uses the
  bundled active CLI as a credential-negative client of `decodexd`; it does not bundle
  or start the frozen v0.2 helper or HTTP control plane.
- `site/` owns the Astro static product site and app download entry.
- `plugins/decodex/` owns Decodex runtime/operator lifecycle skills.
- `openwiki/` owns the repo-local project knowledge surface.
- `automations/portfolio.toml` is the only checked-in source for the exact five Codex
  App automations. Agents own upstream adaptation and content operation. Deterministic
  boundaries own signed commit, signed landing, and xurl effects. The automations do
  not use Decodex server.
- `automations/decodex/` contains compact portfolio validation, content prompts, and
  social schemas. `automations/radar/` contains optional reusable evidence tools and
  has no schedule.

No other product-specific mutation service is active beyond Reset Card. The protocol uses
bounded in-memory replay/idempotency state while the PostgreSQL adapter owns durable product-state
transactions when its explicit configuration verifies successfully. The stable
server-host identity is persisted under `~/.decodex`; stale or impossible cursors still
force snapshot fallback after restart. Shared `~/.codex` remains Codex-owned. OpenWiki explains
those contracts for maintainers and agents but is not a runtime input. Public site
authority stays in `site/`.

## Frozen v0.2 runtime platform support

- The Decodex runtime contract is Unix-only: macOS and Linux.
- Windows is outside the runtime contract.
- Current Codex/app-server compatibility is capability-gated and recorded in
  [`openwiki/specs/contracts-and-data.md`](openwiki/specs/contracts-and-data.md).
- The public site is static and deploys through GitHub Pages.
- Starting `decodex serve` without its `--config` option schedules enabled projects
  from the explicit registry only. Operator and App snapshots still expose active
  runtime DB-backed attempts for disabled projects, because disabling a project pauses
  future dispatch rather than deleting visibility or ownership. It does not scan Codex
  history, repo-local config files, or currently open worktrees to infer projects.

## Frozen v0.2 usage

### Runtime CLI

Use:

```sh
decodex --help
decodex app
decodex probe stdio://
decodex project list
decodex status
decodex status --live
decodex diagnose --json
decodex maintenance prune --dry-run
decodex lane steer <ISSUE> --run-id <RUN_ID> --expected-turn-id <TURN_ID> --message <TEXT>
decodex intake goal --project decodex <CONTRACT_ID> --dry-run
decodex intake goal --project decodex <CONTRACT_ID> --apply
decodex intake issues --project decodex XY-1 XY-2 --dry-run
decodex intake issues --project decodex XY-1 XY-2 --apply
decodex mcp serve --transport stdio
decodex mcp serve --transport streamable-http --listen-address 127.0.0.1:8193
decodex run --dry-run
decodex serve --listen-address 127.0.0.1:8192
```

Project-scoped commands accept `--config <PROJECT_DIR>` after the subcommand when the
operator wants to override registry-based project resolution for that command.
`decodex land --manual-authority --pr <URL>` is the non-issue landing exception: when
no `--config` is supplied, it uses the current Git checkout plus GitHub CLI credentials
from `GH_TOKEN`, `GITHUB_TOKEN`, or `gh auth token` and does not read or refresh the
project registry. Use `--config` when manual landing should use configured GitHub
credentials or workspace hooks; issue-authority landing still requires project config
for retained handoff, Linear closeout, runtime ledger, and cleanup policy.
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

`decodex intake goal` materializes an accepted Decision Contract. `--dry-run` prints
the proposed normal Linear issues, dependencies, conflict domains, and dispatch plan
without mutating Linear or local Program Intake rows. `--apply` creates or updates the
generated normal Linear issue briefs and persists the internal Execution Program plus
contract/program links in runtime SQLite. Apply does not run implementation or apply
queue labels; the persisted Program becomes eligible for direct graph dispatch on the
next scheduler pass. If the contract is not accepted, needs a decision, or lacks
issue-shaping authority, intake stops before creating executable work.

`decodex intake issues` materializes a supplied batch of existing Linear issues into
local Program Intake state. `--dry-run` prints the deterministic ready/held/blocked
report without mutation. `--apply` persists the local Program Intake Plan, Execution
Program, and issue mappings. It never applies or removes service queue labels; ready
mapped nodes are dispatched directly by the Program scheduler instead of being
converted into queued-label work.

`decodex mcp serve --transport stdio` starts the local MCP gateway for desktop and
CLI clients. `decodex mcp serve --transport streamable-http` serves the same gateway
over the Streamable HTTP `POST /mcp` endpoint, bound to `127.0.0.1:8193` by default
for operator-chosen local, tunnel, or relay access. Streamable HTTP validates browser
`Origin` headers against loopback or repeated `--allow-origin <ORIGIN>` values, issues
`Mcp-Session-Id` response headers on `initialize`, requires a known session for later
requests, returns ordinary JSON-RPC JSON responses, and switches to
`text/event-stream` framing when the client sends `Accept: text/event-stream`.
The session header is protocol state, not authorization. `--allow-origin` is CORS
trust, not authentication. Use `--bearer-token-env <ENV_VAR>` when a Streamable HTTP
listener is reachable beyond loopback or when exposing any profile above `observe`;
Decodex validates `Authorization: Bearer <token>` for `POST` and `DELETE` while still
allowing unauthenticated CORS preflight. Direct non-loopback listeners require both
`--allow-origin` and `--bearer-token-env`. The built-in bearer guard is Decodex's
minimum direct-listener boundary, not OAuth Protected Resource Metadata; OAuth or a
managed relay can still sit in front for broader MCP client interoperability.
The gateway advertises resources, resource templates, prompts, tools, logging
compatibility, and progress notifications. Resources expose runtime Decision Contract
readback, local status snapshots, remote-safe live
status/activity projections, current/recent status-window run event/protocol/child-agent
activity/progress diagnostics, PR/review-state readback, lane-inspect aliases, and
lane-control readback. The tool catalog is schema-bound and deliberately small. Local
stdio defaults to the `admin` capability profile; Streamable
HTTP defaults to `observe`. Both can be set with
`--capability-profile observe|plan|operate|admin`; `tools/list` filters by the active
profile and `tools/call` returns structured refusals for tools above it. Observe is
read-only. Plan exposes goal-intake and objective-driven autonomy tools: dry-run modes
validate or preview without tracker or Program Intake mutation, while apply modes
require explicit authority fields and return structured refusals when authority or
project context is missing. Autonomy plan tools can draft and accept Objective
Contracts, submit signals, compile or challenge proposals, and request proposal
acceptance without starting execution. Direct Objective Contract acceptance requires
human/operator authority; policy-backed acceptance fails closed until it is resolved
from trusted Decodex runtime authority state. Operate exposes
`decodex_lane_control`
as an inspect-first lane-control facade: `inspect` returns current preconditions,
`steer` and `interrupt` delegate through existing lane-control guards only with
current run/turn authority, and `manual_attention` or `retained_resume` refuse back to
their canonical tracker/runtime paths. Admin exposes `decodex_project_control` for
project status plus future-dispatch-only pause/resume with explicit authority; `scan`
refuses to the operator control loop. Stdio stdout is reserved for MCP JSON-RPC
messages; diagnostics and logs stay off stdout.

### Project contracts

Project contracts are managed outside checkouts under
`~/.codex/decodex/projects/<service-id>/` with fixed filenames:

- `project.toml` for service paths and credential environment-variable names
- `WORKFLOW.md` for execution policy

At freeze, the redacted project-config template occupied `decodex.example.toml`. The
current checked-in file is the vNext global `~/.decodex/config.toml` template and must
not be used as a compatibility template for this frozen flow.
Decodex autonomy is objective-driven project autonomy, not a hidden runtime repair
loop. `[autonomy]` defaults to latent-only: objective drafts, signal audits, and
proposal dry-runs may produce evidence, but unattended promotion or intake requires an
accepted Objective Contract version plus accepted project-policy authority. Project
config may reference those runtime authority records; it does not embed or replace
allowed signal kinds, allowed surfaces, cooldown, write budget, validation gates, or
review policy. Runtime-health checks are one signal adapter; they do not define the
autonomy product.
Phase-scoped app-server goals are mandatory for retained lane execution. Decodex
rejects a connected Codex app-server that lacks required `thread/goal/*` methods
instead of falling back to ordinary continuation.
When a project enables `[codex.accounts]`, the shared ChatGPT account pool is
`~/.codex/decodex/accounts.jsonl`; it is global Decodex state, not a project-local
file, and project configs do not own an account-pool path override. Set
`[codex.accounts].fixed_account` in `~/.codex/decodex/config.toml` to pin all new
account-pool runs to one account. When that global selector is absent, Decodex balances
new runs across the pool. Decodex App writes and clears the same global selector;
project configs do not pin specific accounts. Account display-name
rerolls are also global Decodex state under `[codex.account_names.offsets]` in
`~/.codex/decodex/config.toml` so Decodex App shows privacy-preserving names.
Client-only presentation preferences such as whether identities are hidden remain
local to each UI. Usage probes also read
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
oversized local logs and agent-evidence event streams, prune old backup files, delete
legacy Git askpass helper files older than one day from registered project worktree
roots, compact old terminal-run protocol events after preserving their summary, and
checkpoint the SQLite WAL. `decodex serve` also runs the auto-safe subset at startup
and periodically while it is polling, including 14-day retention for rotated logs and
agent-evidence event streams plus 14-day protocol-event compaction for terminal
unowned runs after the compact summary is preserved. If the runtime database is busy
or candidate detection fails, serve logs a warning and continues polling.

## Static Site

The public site is an Astro static site under `site/`. It renders the public Decodex
product surface and app download entry. External Codex automation owns publication to
GitHub Pages.

The public site owns:

- homepage rendering
- public static assets
- appcast download widget
- Astro build and type-check behavior

The public site does not own:

- retained-lane scheduling
- tracker writes
- local operator state
- app-server orchestration
- the local operator APIs served by `decodex serve`
- upstream monitoring or public publishing automation

The static-site boundary and GitHub Pages setup for `https://decodex.space`, including
the external automation boundary, are summarized in
[`openwiki/integrations/plugins-automations-and-auxiliary-tools.md`](openwiki/integrations/plugins-automations-and-auxiliary-tools.md).

## Frozen v0.2 operator listener

The excluded `apps/decodex/` v0.2 source still documents its historical HTTP operator
listener. It is not an active workspace member, is not packaged by the current macOS
App, and is not an account authority. The current App uses only the vNext CLI over the
same-UID Unix service.

## Development

Repo-native validation is owned by `Makefile.toml`.

Runtime checks follow the Decodex task structure:

```sh
cargo make check
cargo make fmt
cargo make lint
cargo make lint-fix
cargo make test
```

Use `lint` for the read-only lint gate and `lint-fix` for the canonicalizing lint
path used by registered Decodex workflow gates.

Sync installable Codex plugins with the guarded installer:

```sh
python3 scripts/config/sync_installable_plugins.py --apply --clean-repo-local-skills
```

This installs only `plugins/*` into `$CODEX_HOME/plugins/cache/hack-ink/*/<version>`.
Repo-local skills under `automations/*/skills/` are development and automation
inputs for this repository; they must not be installed into global
`$CODEX_HOME/skills`.

Node package type checks and builds are available separately:

```sh
cargo make check-node
cargo make build-node
```

## Workspace Layout

The tracked workspace currently keeps:

- `crates/decodex-*/` as the five active vNext library owners
- `apps/decodexd/`, `apps/decodex-cli/`, and `apps/decodex-gpui/` as active vNext
  composition roots
- `apps/decodex/` as frozen v0.2 provenance excluded from the active Cargo workspace
- `site/` as the Astro static site for the public Decodex product surface
- `plugins/decodex/` as the canonical installable Decodex plugin source
- `openwiki/` as the repo-local project knowledge and agent context surface
- `dev/` as local development helpers, such as the operator dashboard mock server
- `assets/` as generated Decodex App icon source notes, Icon Composer foreground,
  generated `.icns`, and menu bar template assets

Generated or local-only directories such as `target/`, `site/dist/`, `site/.astro/`,
`.worktrees/`, `.workspaces/`, and `.codex/` are not part of the tracked repository
structure. For an explanatory layout and ownership map, read
[`openwiki/quickstart.md`](openwiki/quickstart.md).

## OpenWiki

- Product and development overview: this `README.md`
- Agent and maintainer entrypoint: [`openwiki/quickstart.md`](openwiki/quickstart.md)
- Runtime architecture: [`openwiki/architecture/runtime-architecture.md`](openwiki/architecture/runtime-architecture.md)
- Operator workflows: [`openwiki/workflows/runtime-operator-workflows.md`](openwiki/workflows/runtime-operator-workflows.md)
- Contracts and data: [`openwiki/specs/contracts-and-data.md`](openwiki/specs/contracts-and-data.md)
- Runtime lifecycle: [`openwiki/specs/runtime-lifecycle.md`](openwiki/specs/runtime-lifecycle.md)
- Commands and validation: [`openwiki/operations/commands-and-validation.md`](openwiki/operations/commands-and-validation.md)
- Operator runbooks: [`openwiki/operations/operator-runbooks.md`](openwiki/operations/operator-runbooks.md)
- Plugins, automations, and auxiliary tools: [`openwiki/integrations/plugins-automations-and-auxiliary-tools.md`](openwiki/integrations/plugins-automations-and-auxiliary-tools.md)
- Radar, Publisher, and site contracts: [`openwiki/integrations/radar-publisher-site.md`](openwiki/integrations/radar-publisher-site.md)

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

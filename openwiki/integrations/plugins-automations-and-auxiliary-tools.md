# Plugins, Automations, And Auxiliary Tools

This page covers repository areas that support the runtime but are not the core scheduler. It is especially important because plugin and automation guardrails can affect future coding agents.

## Installable Decodex plugin

The installable plugin lives under `plugins/decodex/`. Its manifest describes a narrow scope: Decodex runtime and operator workflows for planning, ops, commit, and landing (`plugins/decodex/.codex-plugin/plugin.json`). It contains only Decodex-owned guidance and does not route, bundle, or manage companion plugins.

Core files:

- `plugins/decodex/.codex-plugin/plugin.json`: plugin metadata, display text, skills path.
- `plugins/decodex/skills/decodex/SKILL.md`: route Decodex work to planning, ops, commit, or land surfaces.
- `plugins/decodex/references/routing.md`: first reads and runtime/landing/MCP boundaries.
- `plugins/decodex/hooks/hooks.json`: PreToolUse hook registration.
- `plugins/decodex/scripts/decodex_lifecycle_hook`: Python guardrail for raw Git/GitHub commands in Decodex scope.

The plugin is not runtime authority. Runtime policy lives in source, project contracts, and runtime DB records; this page only explains those boundaries.

## Lifecycle hook guardrails

Recent git history added lifecycle hook guardrails. `hooks.json` registers a PreToolUse hook for Bash/Shell/exec tool use and calls the installed `decodex_lifecycle_hook` (`plugins/decodex/hooks/hooks.json`). The hook script:

- Parses command payloads from tool input.
- Splits shell segments and unwraps shell/env/command wrappers.
- Detects Decodex-owned scope by this repository shape or registered Decodex project repo/worktree paths.
- Blocks raw `git commit` inside Decodex scope and instructs use of `decodex commit`.
- Blocks raw `gh pr merge` inside Decodex scope and instructs use of `decodex land`.

Explicit GitHub selectors recognize canonical `acg-box/decodex` and the transferred
`hack-ink/decodex` redirect alias, so an old link cannot bypass the landing guard.

This protects high-risk history and landing surfaces from bypassing Decodex authority. Future changes should preserve scoped behavior: outside Decodex-owned paths, the hook should not become a generic Git policy.

Watchpoint: `hooks.json` currently hardcodes the plugin path version `0.2.0`, while `scripts/config/sync_installable_plugins.py` derives install version from root `Cargo.toml`. Tests currently assert the `0.2.0` install path (`tests/scripts/test_sync_installable_plugins.py`), so update hook path, manifest version, workspace version, and tests together during version changes.

## Plugin installation

The installer is `scripts/config/sync_installable_plugins.py`. It finds the repo root, reads the workspace version from root `Cargo.toml`, discovers plugins under `plugins/*/.codex-plugin/plugin.json`, and copies each plugin to:

```text
$CODEX_HOME/plugins/cache/acg-box/<plugin>/<version>
```

Each plugin manifest declares `package.include` and `package.exclude`. The sync
script materializes only that runtime package contract. Repository-only plugin
tests live under `tests/scripts/`, outside the physical runtime root. The sync
tests require every file under `plugins/*` to match its package contract.

Commands:

```sh
python3 scripts/config/sync_installable_plugins.py
python3 scripts/config/sync_installable_plugins.py --apply
python3 scripts/config/sync_installable_plugins.py --apply --clean-repo-local-skills
```

`--clean-repo-local-skills` removes global `$CODEX_HOME/skills/<skill>` entries only when they are exact copies of repo-local `automations/*/skills/*`; it refuses modified global skills (`scripts/config/sync_installable_plugins.py`, `tests/scripts/test_sync_installable_plugins.py`).

## Codex App automations

`automations/portfolio.toml` is the one checked-in portfolio authority. It declares
exactly five native tasks and their model, effort, schedule, status, execution
environment, prompt, and primary cwd. Live definitions remain machine-local and are
owned by the native Codex automation lifecycle.

Commands:

```sh
python3 automations/decodex/scripts/config/render_automation_plan.py --json
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo make test-automations
```

The renderer and evaluator do not write native state. The Manager applies full-field
updates only through native automation tools and reads each definition back. Native
`created_at` and `updated_at` metadata are required.

The upstream operating loop has three explicit owners:

- Maintainer researches official Codex changes, creates one deterministic branch and
  PR per upstream head, and uses one ephemeral Sol/max subagent in a temporary task
  worktree for implementation. It uses `decodex commit` for signed commits.
- Reviewer independently reviews and tests the exact GitHub PR head. It sends defects
  back through PR feedback and uses only `decodex land` for signed landing with exact
  base/head and merge-tree readback.
- Manager audits all five native definitions, upstream latency, PR outcomes, content
  and X results, repeated failure causes, and configuration drift. It archives only
  completed successful tasks through native task tools.

The content loop has two explicit owners:

- Content Manager researches official Codex sources and landed Decodex changes. It
  uses CodexRadar only as secondary editorial input and records at most one
  `decodex/content-evidence/1` candidate or no-op.
- Xurl Publisher performs the final quality decision and uses only Publisher
  `publish-next` or `observe-due`. Publisher alone invokes xurl and enforces exact
  account, daily limit, budget, uncertain-write, and readback boundaries.

None of these tasks uses Decodex server, runtime, queue, planner, or MCP. The Decodex
CLI is used only for commit and landing. All schedules use the primary checkout;
temporary worktrees are per-run resources. GitHub PRs, refs, signed commits, merge
readback, native task state, and Publisher X evidence are sufficient workflow state.

Do not copy full automation prompts into OpenWiki. Summarize boundaries and link to source files when a task needs details.

## Radar

Radar is an auxiliary Rust CLI for upstream evidence and artifact workflows (`apps/radar/README.md`, `apps/radar/src/lib.rs`). It owns:

- upstream review queue artifacts
- upstream impact artifacts
- analysis drafts
- signal entries
- release deltas
- control-plane upgrade candidates
- local Radar ledger workflows
- validation and bundle generation

Generated Radar state belongs under `.agent/automations/radar/cache` (`automations/radar/README.md`). Radar does not own Decodex runtime commands or social publishing artifacts.

Source entrypoints:

- `apps/radar/src/lib.rs`: CLI run bootstrap and module map.
- `apps/radar/src/cli.rs`: command parser.
- `apps/radar/src/artifact_validation.rs`: artifact validation.
- `apps/radar/src/operations.rs`: refresh/build/render/validate operations.
- `automations/radar/radar.toml`: Radar-owned cache paths.

## Decodex Publisher

`decodex-publisher` is the deterministic X boundary (`apps/decodex-publisher/README.md`, `apps/decodex-publisher/src/lib.rs`). It owns:

- `decodex/content-evidence/1`
- `social_publish_reservation/v1`
- `social_post/v1`
- `social_outcome/v1`
- high-level `publish-next` and `observe-due` workflows
- one-post-per-day and $1.25-per-month cost-ceiling enforcement
- xurl authorization, immutable attempts, budget ledger, and exact readback

Generated Publisher state belongs under `.agent/automations/decodex/cache/social`.
Publisher consumes direct source URLs, not private Radar lineage. All X reads and
writes use xurl. Browser control, X MCP, direct HTTP, and account switching are
outside this workflow. The normal publication ceiling is $0.030; a full post plus
24-hour and seven-day observations reserves at most $0.040. The per-lineage ceiling
is $0.060 so one interrupted identity read, one safe identity reconciliation, one
normal publication, and both observations can complete without weakening the
$1.25 monthly cap. Publisher writes an immutable call record before each paid xurl
operation. Restart recovery releases a reservation with no attempt, or terminalizes
a durable no-call attempt, even when a new run owns the recovery. Identity recovery
allows one extra read. Publication readback allows no more than five total calls,
and outcome observation allows no more than three reads. Exhausted read-only work
gets a terminal result so unrelated publication lineages and later observation
windows can continue. An unknown create result remains blocked only in its own
lineage and is never retried.
See [Radar Publisher contracts](radar-publisher-contracts.md) for reservation,
budget, and social artifact boundaries.

Example command:

```sh
decodex-publisher validate-social
```

## Native Decodex App

`apps/decodex-app/` is the current SwiftPM macOS menu-bar client
(`apps/decodex-app/README.md`). It:

- Reads the complete account skeleton from the common daemon service and renders one
  compact UUID-keyed row for every account in canonical routing order. Rows use
  independent cards with transparent gaps, compact quota meters, and single-line
  Reset Card controls. The overflow menu stores one panel-wide appearance choice:
  `Thin` by default, or the system `Liquid Glass` effect. The same menu keeps only
  `Refresh all`, that material selector, and Quit. A manual reload remains available
  during a background read and coalesces into the active reload cycle; it reads
  daemon-owned values and does not start provider work. Account mutations and Reset
  Card submissions still disable it. Observation synchronization keeps the last
  published values usable while a new cache read is in flight. Opening the panel
  presents the latest published values immediately and may ask the daemon for one
  coalesced priority observation; only the explicit `Refresh all` action enters the
  full-read lane.
- Reveals a compact trailing-edge reorder grip over an account card's padding while
  the pointer is over that card, so the grip does not reserve a layout column. A
  drag uses the stable list coordinate space and keeps the full-size card on a
  fixed vertical track. Crossing an adjacent card's center springs that card into
  the open slot, and release springs the dragged card into its final slot. A
  completed reorder, or the grip's accessible Move up or Move down action, sends
  one complete `set_account_order` request through the in-process native client
  with the current routing revision. The daemon-owned
  [Account Lifecycle Authority](../specs/account-lifecycle-authority.md#versioned-account-controls)
  remains canonical. The returned routing order replaces the immediate order, and a
  rejection restores the last authoritative order. Reordering is unavailable while
  an account control or Reset Card submission is in progress, when routing and
  account membership disagree, or when fewer than two accounts exist. The app does
  not persist a separate local order.
- Loads daemon-owned quota, Reset Card, and profile values concurrently for every row.
  One cold or failed account observation does not hide the other accounts. The
  [daemon account observer](../specs/account-lifecycle-authority.md#daemon-account-observation)
  starts provider observations for all ready accounts concurrently without a small global
  fan-out cap. A provider-unsupported quota duration stays a muted row-local fact and does
  not mark the account as failed.
- Uses Reset Cards from the common daemon service. The first click starts a
  five-second local confirmation. The second click ends the countdown, shows a busy
  indicator, persists a pending attempt, and submits one in-process Rust request with
  a vNext account UUID, exact revision, public grant/expiry descriptor, and one
  idempotency key. `decodexd` owns credentials, the direct ChatGPT backend API client, the
  opaque credit ID, the provider effect, and durable recovery. The Codex app-server is
  reserved for Quick Task execution and is not used for account health or Reset Card.
  If inventory advances during a skeleton
  read, the app queues one newer skeleton read instead of leaving the row in a
  checking state. During that reconciliation, the app keeps the last quota visible
  but does not expose Reset Cards from the old account revision. It holds an
  advanced inventory until the matching account skeleton arrives and then applies
  it without a duplicate daemon read. One per-account coordinator coalesces
  same-revision inventory calls. A use gate waits for any older daemon read and
  blocks fresh reads until the effect dispatch ends. A terminal use result starts
  bounded background reconciliation without holding the button busy. Internal contention,
  provider cleanup, an incomplete detail response, and a summary/detail count mismatch retain the
  same-revision last complete snapshot while the daemon performs its bounded retry. Only a typed
  local daemon transport loss shows
  `Connecting to Decodex…`. The next current inventory replaces the retained value
  directly, so the quota bar can animate to the restored value.
- Uses an in-process Rust protocol client for active vNext account, profile, Reset Card,
  routing, Codex projection, and Fast requests. The app does not start the CLI, a
  service, or a credential helper, and Swift does not inject credentials. Operators
  run the independently configured daemon service.
  The macOS source-install path provisions that service with
  `scripts/macos/install_decodex_local_service.py`; its Rust supervisor owns the
  PostgreSQL and daemon process generations.
- Uses one finite startup retry schedule for daemon transport and retryable row-scoped
  cold-cache results while the independently supervised observer warms. Permanent
  profile and Reset Card failures remain row-local. It does not retry consume, replace
  an idempotency key, or take service lifecycle ownership. Successful login replacement
  also uses bounded short-interval daemon readback so an old unauthorized observation
  cannot look like the result of the new login.
- Shows one compact status row for each retained attempt and checks durable status
  automatically. A nonterminal state or temporary read failure updates that row. A
  terminal result removes the row and shows the result message. The UI does not
  redispatch `use` or substitute a new key for that pending card.

The reset-card path is not the scheduler, project registry owner, credential owner,
app-server process owner, or runtime authority. Its bounded private journal retains at
most 64 credential-negative attempts and fails closed on malformed or unsafe storage:
recoverable entries remain available for status-only inspection, while new use is blocked.
There is one Swift account surface and one native Rust client library. The package has
no bundled CLI, legacy account store, helper, HTTP control plane, or dual UI. Slice-1
backend startup uses the clean
[Account Lifecycle Authority](../specs/account-lifecycle-authority.md) with no watcher,
credential environment projection, helper, mapping, or `:8192` service.
See [Runtime architecture](../architecture/runtime-architecture.md) for the shared
service flow and [Commands and validation](../operations/commands-and-validation.md)
for the Swift and staging checks.

Build/stage commands:

```sh
swift build --package-path apps/decodex-app -c release
apps/decodex-app/script/build_and_run.sh
scripts/macos/test_decodex_app_stage.sh
python3 -m unittest tests.scripts.test_install_decodex_local_service
```

## Static site

`site/` is an Astro static product site (`site/package.json`). It should remain static and must not depend on a live Decodex daemon unless a future Decodex decision changes that boundary. Keep that boundary summarized here and verify behavior from `site/src/`, `site/package.json`, and site build checks.

Commands:

```sh
npm --prefix site run check
npm --prefix site run build
npm --prefix site run dev
```

Use `site/README.md`, `site/src/`, `site/package.json`, and the site contract together when documenting current behavior. The README records the static-site boundary; source and build output remain the final check for rendered behavior.

## Generated and local-only paths

Do not treat these as source authority:

- `target/`
- `site/dist/`
- `site/.astro/`
- `site/node_modules/`
- `.worktrees/`
- `.decodex/`
- `.agent/automations/radar/cache/`
- `.agent/automations/decodex/cache/social/`
- `~/.codex/decodex/`
- local ignored scratch such as `large_tool_results/`

OpenWiki should document where generated state belongs, not copy generated artifacts.

## Change guidance

- Plugin routing or hook changes: inspect `plugins/decodex/`, update sync tests, and run `python3 -m unittest tests/scripts/test_sync_installable_plugins.py`.
- Automation manifest changes: run sync/evaluation scripts for both Decodex and Radar manifests.
- Radar artifact/schema changes: update Radar validation tests and relevant automation README boundaries.
- Publisher schema changes: update `apps/decodex-publisher/src/social_validation*` tests and social workflow docs.
- App changes: run SwiftPM or staging checks; remember the app shares the Rust account/control-plane state.
- Site changes: run Astro check/build; keep it static unless product authority changes.

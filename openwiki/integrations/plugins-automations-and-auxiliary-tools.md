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

This protects high-risk history and landing surfaces from bypassing Decodex authority. Future changes should preserve scoped behavior: outside Decodex-owned paths, the hook should not become a generic Git policy.

Watchpoint: `hooks.json` currently hardcodes the plugin path version `0.2.0`, while `scripts/config/sync_installable_plugins.py` derives install version from root `Cargo.toml`. Tests currently assert the `0.2.0` install path (`tests/scripts/test_sync_installable_plugins.py`), so update hook path, manifest version, workspace version, and tests together during version changes.

## Plugin installation

The installer is `scripts/config/sync_installable_plugins.py`. It finds the repo root, reads the workspace version from root `Cargo.toml`, discovers plugins under `plugins/*/.codex-plugin/plugin.json`, and copies each plugin to:

```text
$CODEX_HOME/plugins/cache/hack-ink/<plugin>/<version>
```

Each plugin manifest declares `package.include` and `package.exclude`. The sync
script materializes only that runtime package contract; source-only assets such
as `plugins/decodex/tests/` stay out of installed plugin cache entries.

Commands:

```sh
python3 scripts/config/sync_installable_plugins.py
python3 scripts/config/sync_installable_plugins.py --apply
python3 scripts/config/sync_installable_plugins.py --apply --clean-repo-local-skills
```

`--clean-repo-local-skills` removes global `$CODEX_HOME/skills/<skill>` entries only when they are exact copies of repo-local `automations/*/skills/*`; it refuses modified global skills (`scripts/config/sync_installable_plugins.py`, `tests/scripts/test_sync_installable_plugins.py`).

## Codex App automations

Automation source is checked in under:

- `automations/upstream/`: current standalone upstream maintenance, independent
  review/landing, health supervision, deterministic cursor/lease state, and policy.
- `automations/decodex/`: the current Content Manager and xurl Publisher
  definitions, shared config tooling, Publisher schemas, and skills.
- `automations/radar/`: reusable Radar evidence tooling and skills.

These are portable sources. Live Codex App configs are machine-local and owned by
the native automation lifecycle. Source manifests use relative paths and
`{repo_root}` placeholders. Runtime cwd is always the primary checkout owning
`main`: the plan renderer rejects linked-worktree runtime roots and the evaluator
treats any managed `.worktrees` cwd as a P0 failure. The renderer also refuses
configured private fragments such as absolute user-home paths, auth files, account
files, or runtime databases (`automations/decodex/README.md`).

Commands:

```sh
python3 automations/decodex/scripts/config/render_automation_plan.py --json
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/upstream/automations.toml
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/decodex/automations.toml
python3 -m unittest automations.upstream.tests.test_upstream_autopilot
```

The renderer cannot write scheduler state. Apply each planned definition only with
the Codex native automation lifecycle tool, then read it back. Codex App owns its
`created_at` and `updated_at` metadata. The rendered retirement list contains only
`decodex-x-browser-publisher`; Health deletes that exact obsolete definition and
verifies its absence without listing or changing unrelated definitions.

The upstream operating loop has three explicit owners:

- Upstream Maintainer polls every six hours, preserves a complete first-parent cursor, observes
  stable and prerelease tags, generates stable and experimental schemas from the exact
  installed Codex executable, and claims one change. It can edit and stage source.
  The checked-in state wrapper alone can run sandboxed tests, invoke
  `decodex commit --manual-authority`, push, and open a pull request.
- Upstream Reviewer runs in a separate task and context. It verifies the exact
  pull-request head, performs an independent code review, repeats sandboxed tests
  through the wrapper, requests bounded repairs, or lets the wrapper invoke
  `decodex land --manual-authority --pr` with the exact validated base and head
  object IDs. Decodex alone creates and pushes the signed merge, synchronizes
  `main`, and cleans the exact lane.
- Upstream Health verifies live config, two-hour observation freshness, cursor
  continuity, six-hour review SLA, lease expiry, retry budgets, and current schema
  evidence. It also reconciles the two content definitions and validates existing
  social artifacts without opening X.

The content loop has two explicit owners:

- Content Manager runs once per day. It uses official sources, internal Radar
  evidence, landed Decodex evidence, and bounded outcome records to produce one
  source-backed candidate or one justified quality skip.
- X Publisher runs three times per day. Each task processes at most one
  publication, due 24-hour outcome, or due seven-day outcome. It validates and reserves
  one candidate, delegates all X access to the pinned `decodex-publisher` xurl
  entrypoint, verifies the exact `decodexspace` identity and created post, and
  records one due 24-hour or seven-day outcome.
- The five fixed definitions total 12 `high` task wakes per day: 4 Maintainer, 2
  Reviewer, 2 Health, 1 Content Manager, and 3 Publisher. This is 360 wakes in 30
  days and 372 wakes in 31 days. The three Publisher windows do not change the
  one-post-per-day limit or the X API ceilings of $1.20 per 30 days, $1.24 per 31
  days, and $1.25 per calendar month.
- Upstream Health supervises all five managed definitions and queues a bounded
  `content_loop_degraded` code-improvement candidate when content validation, strategy,
  candidate handling, outcome collection, or xurl publication misses its service
  level.

None of these tasks uses Decodex server, MCP, Program Intake, Linear, or tracker state.
The installed Decodex CLI is used only for commit and landing. Scheduled cwds remain
the primary `main` checkout; temporary implementation and review worktrees are
per-run resources, not automation bindings. Upstream text is untrusted data and is
never executed. Candidate code runs only in the wrapper's credential-free,
external-network-denied macOS sandbox. Generated state is bounded, local-only, and
excludes prompt text, logs, credentials, private account identifiers, and personal
data. The xurl ledger stores only bounded operation metadata, response digests,
verified public post identity, and cost ceilings. Social and strategy records are
never committed or archived to Git. The managed portfolio contains only the three
upstream tasks plus `decodex-content-manager` and `decodex-xurl-publisher`.

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
- `automations/radar/radar.toml`: cache and handoff path contract.

## Decodex Publisher

`decodex-publisher` is an auxiliary publishing handoff CLI (`apps/decodex-publisher/README.md`, `apps/decodex-publisher/src/lib.rs`). It owns:

- `social_candidate/v1`
- `social_publish_reservation/v1`
- `social_post/v1`
- `social_outcome/v1`
- `social_strategy/v1`
- one serialized xurl publication and observation state machine
- one-post-per-day and $1.25-per-month cost-ceiling enforcement
- social artifact validation and reservation workflows

Generated Publisher state belongs under `.agent/automations/decodex/cache/social`.
Publisher consumes Radar handoff evidence, but must not refresh upstream state or
perform fresh upstream source analysis (`automations/decodex/README.md`). All X reads
and writes use the fixed xurl entrypoint. Browser control, X MCP, direct HTTP, and
account switching are outside this workflow. The normal publication ceiling is
$0.030; a full post plus 24-hour and seven-day observation lifecycle is $0.040.
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
  independent material cards with transparent gaps, compact quota meters, and
  single-line Reset Card controls.
- Loads quota windows and Reset Cards independently for each row. One slow or failed
  provider request does not hide the other accounts. A provider-unsupported quota
  duration stays a muted row-local fact and does not mark the account as failed.
- Uses Reset Cards from the common daemon service. The first click starts a
  five-second local confirmation. The second click first persists a pending attempt,
  then submits one in-process Rust request with a vNext account UUID, exact revision,
  public grant/expiry descriptor, and one idempotency key. `decodexd` owns credentials,
  app-server, the opaque credit ID, the provider effect, and durable recovery.
- Uses an in-process Rust protocol client for active vNext account, profile, Reset Card,
  routing, Codex projection, and Fast requests. The app does not start the CLI, a
  service, or a credential helper, and Swift does not inject credentials. Operators
  run the independently configured daemon service.
  The macOS source-install path provisions that service with
  `scripts/macos/install_decodex_local_service.py`; its Rust supervisor owns the
  PostgreSQL and daemon process generations.
- Retries only the bounded startup account-skeleton transport while the independently
  supervised service becomes ready. Row-scoped profile or Reset Card failures remain
  local until explicit refresh. It does not retry consume, replace an idempotency key,
  or take service lifecycle ownership.
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

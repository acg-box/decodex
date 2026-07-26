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
- `automations/decodex/`: shared config tooling plus reusable Publisher schemas and
  skills.
- `automations/radar/`: reusable Radar evidence tooling and skills.

These are portable sources. Live Codex App configs under `$CODEX_HOME/automations/*/automation.toml` are generated and machine-local. Source manifests use relative paths and `{repo_root}` placeholders. Runtime cwd is always the primary checkout owning `main`: the installer rejects linked-worktree runtime roots and the evaluator treats any managed `.worktrees` cwd as a P0 failure. The installer also refuses configured private fragments such as absolute user-home paths, auth files, account files, or runtime databases (`automations/decodex/README.md`).

Commands:

```sh
python3 automations/decodex/scripts/config/sync_automations.py
python3 automations/decodex/scripts/config/sync_automations.py --apply
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/upstream/automations.toml
python3 -m unittest automations.upstream.tests.test_upstream_autopilot
```

The current operating loop has three explicit owners:

- Upstream Maintainer polls hourly, preserves a complete first-parent cursor, observes
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
  evidence.

None of these tasks uses Decodex server, MCP, Program Intake, Linear, or tracker state.
The installed Decodex CLI is used only for commit and landing. Scheduled cwds remain
the primary `main` checkout; temporary implementation and review worktrees are
per-run resources, not automation bindings. Upstream text is untrusted data and is
never executed. Candidate code runs only in the wrapper's credential-free,
external-network-denied macOS sandbox. Generated state is bounded, local-only, and
excludes prompt text, logs, credentials, account identifiers, and personal data.

The obsolete Publisher, Radar review, release curator, retention, health evaluator,
daily review, Manager, and weekly growth manifests and prompts were deleted. They
cannot be installed or reactivated by the default sync path.

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
- social artifact validation and reservation workflows

Generated Publisher state belongs under `.agent/automations/decodex/cache/social`. Publisher consumes Radar handoff evidence, but must not refresh upstream state or perform fresh upstream source analysis (`automations/decodex/README.md`). See [Radar Publisher contracts](radar-publisher-contracts.md) for reservation and social artifact boundaries.

Example command:

```sh
decodex-publisher validate-social .agent/automations/decodex/cache/social/x
```

## Native Decodex App

`apps/decodex-app/` is a SwiftPM macOS app for the local account pool (`apps/decodex-app/README.md`). It:

- Lists stored accounts without token material.
- Pins future Decodex runs to one account or returns to balanced selection.
- Forces Codex to use a stored account by writing `auth.json`.
- Shows vNext reset cards from the common daemon service. The first click starts a
  five-second local confirmation. The second click first persists a pending attempt,
  then invokes the bundled `decodex-cli` with a vNext account UUID, exact revision,
  public grant/expiry descriptor, and one idempotency key. `decodexd` owns credentials,
  app-server, the opaque credit ID, the provider effect, and durable recovery.
- Runs isolated Codex device login and imports the resulting auth file. The App
  honors an explicit `CODEX_CLI_PATH` override; otherwise it resolves the login
  executable from the Codex macOS application registered with Launch Services,
  then falls back to a `codex` executable in the inherited `PATH`.
- Removes stored accounts from the local pool.
- Invokes the bundled `decodex-cli` for active vNext reset-card requests. The bundle
  also distributes `decodexd`, but Swift does not start it or inject credentials.
  Operators must run the independently configured daemon service. Separate bundled
  legacy `decodex` and `decodex-app-helper` executables remain for unrelated existing
  account UI; they have no vNext reset-card authority.
- Shows a `Resume` action for retained attempts. Resume checks durable status first and,
  only when the daemon reports `not_found`, may invoke `use` again with the same profile,
  server UUID, selection, and idempotency key. It never substitutes a new key for that
  pending card.

The reset-card path is not the scheduler, project registry owner, credential owner,
app-server process owner, or runtime authority. Its bounded private journal retains at
most 64 credential-negative attempts and fails closed on malformed or unsafe storage:
recoverable entries remain available for status-only inspection, while new use is blocked.
The separate legacy account path may still manage its existing local `decodex serve`
lifecycle. See [Runtime architecture](../architecture/runtime-architecture.md) for the
shared service flow and [Commands and validation](../operations/commands-and-validation.md)
for the direct Swift and staging checks.

Build/stage commands:

```sh
swift build --package-path apps/decodex-app -c release
apps/decodex-app/script/build_and_run.sh
scripts/macos/test_decodex_app_stage.sh
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

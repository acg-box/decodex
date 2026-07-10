# Plugins, Automations, And Auxiliary Tools

This page covers repository areas that support the runtime but are not the core scheduler. It is especially important because plugin and automation guardrails can affect future coding agents.

## Installable Decodex plugin

The installable plugin lives under `plugins/decodex/`. Its manifest describes a narrow scope: Decodex runtime and operator workflows for planning, ops, commit, and landing (`plugins/decodex/.codex-plugin/plugin.json`). It explicitly says generic repository work, OpenWiki-backed knowledge, and research/skeptic review belong to external installed plugins, not this Decodex plugin.

Core files:

- `plugins/decodex/.codex-plugin/plugin.json`: plugin metadata, display text, skills path.
- `plugins/decodex/skills/decodex/SKILL.md`: route Decodex work to planning, ops, commit, or land surfaces.
- `plugins/decodex/references/routing.md`: first reads and runtime/landing/MCP boundaries.
- `plugins/decodex/hooks/hooks.json`: PreToolUse hook registration.
- `plugins/decodex/scripts/decodex_lifecycle_hook`: Python guardrail for raw Git/GitHub commands in Decodex scope.

The plugin is not runtime authority. Runtime policy still lives in source, project contracts, runtime DB records, and OpenWiki/spec content.

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

- `automations/decodex/`: Publisher automation, public-publishing jobs, automation health audit jobs, social schemas, skills, and shared config tooling.
- `automations/radar/`: Radar upstream review, release checkpoint curation, artifact retention, GitHub/Codex analysis helpers, and Radar skills.

These are portable sources. Live Codex App configs under `$CODEX_HOME/automations/*/automation.toml` are generated and machine-local. Source manifests use relative paths and `{repo_root}` placeholders. Runtime cwd is always the primary checkout owning `main`: the installer rejects linked-worktree runtime roots and the evaluator treats any managed `.worktrees` cwd as a P0 failure. The installer also refuses configured private fragments such as absolute user-home paths, auth files, account files, or runtime databases (`automations/decodex/README.md`).

Commands:

```sh
python3 automations/decodex/scripts/config/sync_automations.py
python3 automations/decodex/scripts/config/sync_automations.py --apply
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/decodex/automations.toml
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/radar/automations.toml
python3 automations/decodex/scripts/operations/summarize_automation_effectiveness.py
```

The operating loop has explicit owners:

- Health Audit repairs live-config-only drift from validated canonical source and
  re-evaluates every managed automation. It never edits repo source.
- Daily Effectiveness Review independently measures the previous 24 hours and writes
  `automation_effectiveness_scorecard/v1` evidence.
- Automation Manager consumes that evidence, ranks fresh Radar opportunities, creates
  at most one qualified social candidate, closes operational incidents, and executes
  the active content experiment. Daily records measurements and recommends strategy
  changes; Weekly alone selects, modifies, continues, or stops experiments. Publisher
  remains the sole X writer.
- Weekly Growth Review compares consecutive seven-day windows and persists the next
  experiment. The deterministic scorecard treats missing, invalid, or expired active
  strategy and post-cutover Daily Manager coverage gaps as operational P1 evidence;
  an ACTIVE live config alone is not successful execution. Paid X MCP reads are
  bounded and used only when fresh outcome or benchmark evidence can change the
  decision.

Operational autonomy does not bypass repository authority. Prompt/live-config repair,
candidate selection, publishing, outcome learning, and strategy updates can close
automatically. Code, schema, runtime, PR, and landing changes still require normal
Decodex authority and are emitted as structured implementation handoffs.

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
- Runs isolated Codex device login and imports the resulting auth file.
- Removes stored accounts from the local pool.
- Connects to a default local `decodex serve` when available, otherwise starts bundled `decodex serve --listen-address 127.0.0.1:8192`.

The app is not the scheduler, project registry owner, or runtime authority. It is a native UI over Rust-owned account/control-plane state.

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

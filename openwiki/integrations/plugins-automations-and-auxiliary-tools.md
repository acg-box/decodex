---
type: "Reference"
title: "Automations And Auxiliary Tools"
description: "Boundaries for native automations, Radar, Publisher, the sole GPUI application, and static site."
tags: [automations, radar, publisher, gpui, site]
openwiki:
  roles: [integration, operations]
  change_kinds: [automation, desktop, publishing]
  source_paths: [automations/portfolio.toml, apps/radar/src/lib.rs, apps/decodex-publisher/src/lib.rs, apps/decodex-gpui/src/main.rs, site/package.json]
---

# Automations And Auxiliary Tools

These repository areas support Decodex but do not share `decodexd` product authority.

## Native Codex automations

`automations/portfolio.toml` is the checked-in portfolio authority. It defines the native
tasks, model, effort, schedule, status, execution environment, prompt, and primary
working directory. Live definitions remain machine-local and use the native automation
lifecycle.

```sh
python3 automations/decodex/scripts/config/render_automation_plan.py --json
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo make test-automations
```

The renderer and evaluator do not write native state. These tasks do not replace the
Decodex protocol, product database, Quick Task runtime, or app-server authority.

## Radar

Radar is an auxiliary Rust CLI for upstream evidence and artifact workflows. Source
entrypoints are `apps/radar/src/lib.rs`, `apps/radar/src/cli.rs`, and
`apps/radar/src/artifact_validation.rs`. Generated Radar state belongs under
`.agent/automations/radar/cache`.

Radar owns its evidence schemas, review queue, release deltas, bounded local ledger,
validation, and bundle generation. It does not own Decodex product commands, account
state, conversations, or social publication.

## Decodex Publisher

`decodex-publisher` is the deterministic social-publication boundary. It owns its content
evidence, reservations, publication attempts, budget ledger, and exact readback. Generated
Publisher state belongs under `.agent/automations/decodex/cache/social`.

```sh
decodex-publisher validate-social
```

Publisher consumes accepted source-backed content. It does not research product state or
join the `decodexd` service lifecycle.

## Decodex.app

`apps/decodex-gpui/` is the only macOS GUI source. It builds the `decodex-gpui`
executable and stages as `Decodex.app` with bundle identity `box.acg.decodex`.

The application is presentation-only:

- Accounts, quotas, routing, login, Quick Tasks, Programs, history, Health, and settings
  use typed protocol clients or retained-session controllers.
- The application never opens SQLite, account credentials, or Codex authentication files.
- The optional status item uses `NSStatusBar` inside the same `Decodex.app` process through
  the signed embedded `libDecodexMenuBar.dylib` host.
- The **Show Decodex in the menu bar** preference is read and changed through `decodexd`.
- The bundle contains no nested login-item app or helper UI; its signed embedded payloads are
  the local `decodexd` helper, the native client FFI library, and the menu-bar library.

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  cargo +stable test -p decodex-gpui --all-targets --features visual-capture
scripts/macos/test_decodex_app_stage.sh
```

The independently installed `decodexd` service remains the only core owner. The service
installer does not install or launch the GUI.

## Static site

`site/` is a static Astro product surface. It must build without a live Decodex daemon.

```sh
npm --prefix site run check
npm --prefix site run build
npm --prefix site run dev
```

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

## Change guidance

- Automation changes: run the renderer, evaluator, and automation tests.
- Radar changes: run Radar validation and package tests.
- Publisher changes: run Publisher validation and package tests.
- App changes: run GPUI tests, architecture tests, app staging, and a native launch check.
- Site changes: run the Astro check and build.

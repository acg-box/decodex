---
type: "Runbook"
title: "Commands And Validation"
description: "Current source, test, package, and runtime validation entrypoints for Decodex."
tags: [operations, validation, rust, macos, sqlite]
openwiki:
  roles: [operations, testing]
  change_kinds: [validation, runtime, desktop, packaging]
  source_paths: [Makefile.toml, scripts/vnext/local_database_gate.py, scripts/macos/stage_decodex_app.sh, scripts/macos/test_decodex_app_stage.sh, tests/scripts/test_vnext_architecture.py]
---

# Commands And Validation

Use the smallest command that proves the changed contract, then broaden for shared
runtime, protocol, database, or packaging changes. Use the stable Rust channel for every
build and test command.

## Active owner map

- `database/`: SQLite schema, migrations, adapters, and restart evidence.
- `crates/decodex-core/`: mechanism-neutral domain types, configuration, and paths.
- `crates/decodex-codex/`: Codex app-server contracts.
- `crates/decodex-runtime/`: daemon application services and product behavior.
- `crates/decodex-protocol/`: typed same-UID protocol and clients.
- `apps/decodexd/`: the only background service composition root.
- `apps/decodex-cli/`: supported protocol CLI.
- `apps/decodex-gpui/`: the only macOS GUI and `Decodex.app` packaging source.
- `apps/radar/` and `apps/decodex-publisher/`: independent auxiliary CLIs.
- `database/transfer/`: one-shot read-only account transfer.

## Focused database and architecture checks

```sh
python3 scripts/vnext/local_database_gate.py
python3 -m unittest tests/scripts/test_vnext_architecture.py
python3 -m unittest tests/scripts/test_account_login_architecture.py
cargo +stable test -p decodex-database
cargo +stable test -p decodex-protocol
cargo +stable test -p decodex-runtime
```

The local database gate builds a fresh owner-private root, runs daemon initialization and
validation, checks all migration digests and the exact table inventory, and proves the
normal runtime dependency boundary. It does not use a second database server or client
store.

## CLI and daemon checks

```sh
cargo +stable run -p decodexd -- --version
cargo +stable run -p decodex-cli -- status
cargo +stable run -p decodex-cli -- doctor --output json
cargo +stable test -p decodex-cli --all-targets
cargo +stable test -p decodexd --all-targets
```

The active CLI command inventory is `artifact-cohort` (hidden), `status`, `doctor`,
`reset-card`, `account`, and `fast-mode`. It does not own repository orchestration,
commit, landing, or service supervision.

## GPUI checks

The complete GPUI build needs an Xcode developer directory with the Metal compiler on
the current macOS development host. The signed app staging path additionally needs the
Apple signing identity named by `DECODEX_APP_SIGN_IDENTITY` and SwiftPM support for the
embedded menu-bar library.

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  cargo +stable test -p decodex-gpui --all-targets --features visual-capture
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  cargo +stable clippy -p decodex-gpui --all-targets --features visual-capture -- -D warnings
```

GPUI is a protocol-only client. Its persistent desktop-setting controller must not depend
on `decodex-database`, `rusqlite`, credentials, or provider engines.

## Native macOS application checks

```sh
DECODEX_APP_SIGN_IDENTITY="Apple Development: ..." \
  scripts/macos/stage_decodex_app.sh
scripts/macos/test_decodex_app_stage.sh
scripts/macos/run_decodex_gpui_accessibility_gate.swift --help
python3 -m unittest tests.scripts.test_install_decodex_local_service
```

`stage_decodex_app.sh` is the canonical release-shaped builder and requires a stable Apple
codesigning identity; ad-hoc signing is intentionally rejected. It builds `decodex-gpui`,
`decodexd`, the native client FFI, and the Swift menu-bar library, then signs one
`Decodex.app`. The stage test verifies the bundle name, display name, executable, identifier,
icon, signatures, one-app shape, embedded helper/library counts, and required ABI symbols.
It also proves that no nested login-item app is present. The former
`stage_decodex_gpui.sh` entrypoint was deleted and must not be used.

A native runtime acceptance run must also prove:

1. the main Decodex window belongs to the staged GPUI executable;
2. **Show Decodex in the menu bar** changes through `decodexd`;
3. one status item appears or disappears without a second process; and
4. the setting survives daemon and application restart.

## Repository gate

The repository task runner defines the broad gate:

```sh
cargo make check
```

When stable-only policy prevents a task-runner subcommand from using its configured
formatter toolchain, run the stable build, lint, test, Node, architecture, database, and
package checks separately and record the exact formatter gap. Do not override a build or
test with a numbered or non-stable Rust compiler.

## Radar, Publisher, automations, and site

```sh
cargo +stable test -p radar
cargo +stable test -p decodex-publisher
python3 automations/decodex/scripts/config/render_automation_plan.py --json
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
npm --prefix site run check
npm --prefix site run build
```

These surfaces have separate artifact authority. Their passing checks do not prove the
daemon, SQLite, protocol, or macOS application contracts.

## Completion checklist

- Run the focused regression that can fail for the changed behavior.
- Run architecture checks after ownership or packaging changes.
- Run a real build for every changed executable or bundle.
- Run `git diff --check` and review the complete diff.
- Reverse-scan removed names, paths, commands, app identities, tests, and documentation.
- State any unrun live UI, signing, installer, or repository-gate evidence exactly.

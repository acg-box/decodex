---
type: Reference
title: "Commands and validation"
description: "Commands and validation"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-c8b1a2a9f2113ec43d4066da
    resource: repo://Makefile.toml
  - id: openwiki-source-b7793decf9d7c9ba48e57e0f
    resource: repo://rust-toolchain.toml
  - id: openwiki-source-8341ffcf9eaf781ab2d51b69
    resource: repo://scripts/macos/compile_decodex_app_icon.sh
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Commands and validation

Use the smallest check that proves the changed contract. Broaden testing for shared runtime, protocol, database or packaging changes. The repository uses stable Rust for builds and tests; the formatter command is independently pinned in Makefile.toml.

## Source and runtime owners

| Owner | Responsibility |
| --- | --- |
| `database/` | SQLite schema, migration ledger, transactions and restart fixtures |
| `decodex-core` | Domain types, bounded identities, paths and pure policies |
| `decodex-codex` | Native app-server transport and adapters |
| `decodex-runtime` | Service application, account effects, conversations and Chief |
| `decodex-protocol` | Exact typed local wire contract and clients |
| `apps/decodex-cli` | Unified `decodex` commands and `serve` |
| `apps/decodex-gpui` | GPUI app, native Swift libraries and UI tests |
| `apps/radar`, `apps/decodex-publisher` | Independent research/publication tools |
| `site/` | Static Astro product site |

Do not use removed `apps/decodexd`, Factory capture binaries, or old server-store gates.

## Focused checks

```sh
cargo +stable test -p decodex-database --lib
cargo +stable test -p decodex-protocol --lib
cargo +stable test -p decodex-runtime --lib
cargo +stable test -p decodex-codex --lib
cargo +stable test -p decodex-gpui --bin decodex-gpui
cargo +stable test -p decodex-cli --all-targets
python3 scripts/vnext/local_database_gate.py
python3 -m unittest tests/scripts/test_vnext_architecture.py
python3 -m unittest tests/scripts/test_account_login_architecture.py
```

The database gate uses an isolated owner-private root. It is not permission to reset the user's database. Ignored live-provider tests require their documented account and effect prerequisites; do not enable every ignored test as a routine check.

## Diagnostics and macOS packaging

```sh
cargo +stable run -p decodex-cli -- --version
cargo +stable run -p decodex-cli -- status
cargo +stable run -p decodex-cli -- --output json doctor
xcode-select --print-path
xcodebuild -version
scripts/macos/stage_decodex_app.sh
scripts/macos/test_decodex_app_stage.sh
```

A full Xcode installation and its Metal toolchain are required for GPUI. The scripts use the selected developer directory unless `DEVELOPER_DIR` is set. No Beta-specific path is required. The canonical stage script requires valid configured signing authority and rejects ad-hoc signing. It builds one app with the unified helper, native-client FFI and Swift menu-bar library.

The staging test verifies payload counts, Info.plist, signatures, team identity, native ABI and a deliberately mismatched ABI fixture. Python reads piped entitlement bytes explicitly for compatibility. A passing package test is not live UI, speech, release notarization or installation acceptance.

## Repository and auxiliary checks

```sh
cargo make check
cargo make test-automations
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo +stable test -p radar
cargo +stable test -p decodex-publisher
npm --prefix site run check
npm --prefix site run build
```

Makefile.toml owns the complete gate and tool-specific formatter/linter settings. Use repository lockfiles and already-managed tools. A missing prerequisite is not a source defect and does not authorize arbitrary toolchain replacement.

## Evidence boundaries

Report source checks, unit tests, signed build, visual acceptance, provider acceptance, PR merge and installed release separately. For UI changes test focus, typing, selection, scrolling, panel transitions and task switching. Preserve failure output; screenshots that fail or return blank do not establish that the app itself is blank.

[Wiki maintenance](wiki-maintenance.md) describes the separate documentation lifecycle.

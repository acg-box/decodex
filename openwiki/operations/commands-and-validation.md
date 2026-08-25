---
type: "Validation Guide"
title: "Commands And Validation"
description: "Current narrow and aggregate validation commands for the Decodex SQLite daemon, Rust workspace, automation tooling, protocol clients, macOS installation, and local database transfer."
tags: [operations, testing, validation, rust, sqlite]
openwiki:
  roles: [operations, testing, repository]
  change_kinds: [testing, public-api, persistence, installation]
  source_paths: [Makefile.toml, apps/decodexd/src/main.rs, scripts/vnext/local_database_gate.py, tests/scripts/test_vnext_architecture.py, scripts/lint_rust_workspace.py]
  symbols: [test, test-headless, test-local-database, lint-rust, check-automations]
  test_paths: [tests/scripts/test_vnext_architecture.py, tests/scripts/test_install_decodex_local_service.py, database/tests/quick_task_restart.rs]
  invariants: ["Use the narrowest check that exercises the changed boundary.", "Database acceptance uses the SQLite local database gate rather than retired server-store commands.", "Workspace and package checks are conditional when a change crosses public, GPUI, packaging, or generated-artifact boundaries."]
  validation_commands: ["cargo make check", "cargo make test-local-database", "cargo make test-headless", "cargo make lint-rust-headless"]
---

# Commands And Validation

Consult this page before selecting checks for a source, protocol, database, automation, or macOS packaging change. `Makefile.toml` is the authority for implemented task names. Prefer focused checks; use aggregate tasks only when the changed boundary warrants them.

## Core task map

| Purpose | Command |
| --- | --- |
| Broad repository check | `cargo make check` |
| Headless aggregate without GPUI | `cargo make test-headless` |
| Sandboxed aggregate | `cargo make test-sandboxed` |
| Rust type check | `cargo make check-rust` |
| Rust tests | `cargo make test-rust` |
| Rust lint | `cargo make lint-rust`; headless: `cargo make lint-rust-headless` |
| Local database gate | `cargo make test-local-database` |
| Architecture contract tests | `cargo make test-vnext-architecture` |
| CLI diagnostics | `cargo make test-vnext-cli-diagnostics` |
| Automation portfolio tests | `cargo make test-automations` |
| Node/site checks | `cargo make check-node` and `cargo make build-node` |

`test` combines automation tests, gate-contract tests, local database validation, Rust tests, architecture tests, and CLI diagnostics. `check` additionally includes formatting, lint, build, and the selected test aggregate. `test-headless` and `lint-rust-headless` exclude only `decodex-gpui`; use them when Apple/Metal tooling is unavailable and the change does not touch GPUI.

## Boundary-first recipes

- **Database or migration:** run `cargo make test-local-database`, then `cargo test -p decodex-database --all-targets`; add `cargo test -p decodex-database-transfer` for transfer changes. Escalate to `cargo make check` when manifests, public exports, or installer artifacts change.
- **Runtime or protocol:** run the focused package test, `cargo make test-vnext-architecture`, and `cargo make test-vnext-cli-diagnostics` when daemon startup or wire behavior changes. Verify consumer imports and artifact-cohort agreement for shipped protocol changes.
- **Automation or scripts:** run `cargo make test-automations` and the focused Python unittest module. Use `cargo make check-automations` only when the diff avoids GPUI, Apple GPU, and packaging surfaces.
- **macOS app or service staging:** run `swift test --package-path apps/decodex-app`, `python3 -m unittest tests/scripts/test_install_decodex_local_service.py`, and the applicable staging script. A full workspace check is conditional on Rust/GPUI or signed artifact changes.
- **Site or publisher:** run the package-specific Rust tests or `npm --prefix site run check`; run the site build when Astro/content/build configuration changes.

## Local database acceptance

The canonical database checks are:

```sh
python3 scripts/vnext/local_database_gate.py
python3 -m unittest tests/scripts/test_vnext_architecture.py
cargo test -p decodex-database --all-targets
cargo test -p decodex-database-transfer
cargo test -p decodexd
```

These validate embedded SQLite migrations, integrity and restart behavior, transfer safety, and daemon integration. Do not use retired Postgres/server-store bootstrap, migration, or latest-schema commands as current acceptance. The current daemon commands are `initialize-local-database --root ROOT` and `validate-local-database --root ROOT`; normal serve startup validates but does not perform schema administration.

## Expensive checks and scope

Run `cargo make check` when the change crosses multiple workspace packages, GPUI/Metal, release packaging, or a public client boundary. Run signing/staging checks when installer scripts, artifact cohorts, binaries, or LaunchAgent behavior change. Do not run broad checks merely for documentation or isolated domain-module changes.

The repository has no tracked GitHub Actions CI workflow. Local validation evidence and Git/GitHub landing authority are separate concerns; the active CLI does not provide repository commit, landing, or Git-hook commands.

See [Local database operations](local-database.md) for installation and transfer sequencing and [Runtime architecture](../architecture/runtime-architecture.md) for ownership and lifecycle boundaries.

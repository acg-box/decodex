---
type: Reference
title: Build Test Run Entrypoints
description: Answers how to validate this repo, run checks, and find setup, build, test, run, automation, and Decodex command entrypoints.
status: active
authority: current_state
owner: docs
tags: [reference, build, test, run, validation, setup, automation, resources, entrypoints, agent, repo-memory]
source_refs: []
code_refs: [Makefile.toml, Cargo.toml, apps/decodex/Cargo.toml, site/package.json, apps/decodex-app/Package.swift, README.md]
related: [./workspace-layout.md, ./test-suite.md, ./docs-knowledge-map.md, ../policy.md, ../runbook/release-readiness.md, ../spec/okf-knowledge-layer.md]
drift_watch: [Makefile.toml, Cargo.toml, site/package.json, apps/decodex-app/Package.swift, cargo make --list-all-steps, cargo make check, cargo nextest list --workspace --all-targets --all-features, automations/decodex/scripts/config/sync_automations.py, automations/decodex/automations.toml, automations/radar/automations.toml]
last_verified: 2026-06-27
---

# Build Test Run Entrypoints

Purpose: Map the current repository commands for setup, building, testing, running,
and validating Decodex.

Read this when: You need the smallest current entrypoint for repo setup, local
validation, running checks, task-runner automation resources, or Decodex CLI usage.
This is the first reference for a new agent that needs to understand how repo setup
and command entrypoints fit together.

Not this document: The full test inventory, repository directory ownership, release
procedure, or runtime behavior contract.

Covers: Task-runner authority, primary validation gates, targeted command entrypoints,
source entrypoints, local prerequisites, and owner boundaries.

## Task Runner Authority

`Makefile.toml` owns repo-native task names. Use `cargo make` when an equivalent task
exists, and inspect `Makefile.toml` or run `cargo make --list-all-steps` when the exact
subcommands matter.

The root `check` task is the aggregate repository gate. It currently depends on:

- `build`
- `check-docs`
- `check-node`
- `check-rust`
- `fmt-check`
- `lint`
- `test`

This makes `cargo make check` the first command to reach for before claiming a broad
repository change is ready. For narrow documentation-only edits, `check-docs` is the
focused gate, but a broad ready/land claim should still consider the aggregate gate or
explain why a narrower check is sufficient.

## Primary Validation Gates

| Task | Command surface | Owns |
| --- | --- | --- |
| `cargo make check` | composite root task | Aggregate build, docs, node, Rust, format, lint, and test validation |
| `cargo make check-docs` | `decodex docs check` | Decodex docs OKF/profile validation |
| `cargo make check-rust` | `cargo check --all-features --all-targets --workspace` | Rust workspace type checking |
| `cargo make check-node` | `npm run check` in `site/` | Astro and TypeScript site checks |
| `cargo make fmt-check` | Rust nightly fmt plus Taplo check | Rust and TOML formatting |
| `cargo make lint` | Clippy plus `cargo vstyle curate` | Rust lint and vibe-style curation |
| `cargo make test` | `cargo nextest run --workspace --all-targets --all-features` | Default Rust test gate |
| `cargo make build` | `npm run build` in `site/` | Static site production build |

The SwiftPM macOS app under `apps/decodex-app/` is excluded from the root Cargo
workspace. Validate it with SwiftPM from that directory when a change touches the
native app surface.

## Targeted Commands

Use these commands when a change does not need the full aggregate gate:

```sh
decodex docs check
decodex docs graph
decodex docs find --tag validation
cargo check --all-features --all-targets --workspace
cargo nextest run --workspace --all-targets --all-features
cargo test -p decodex <filter>
npm --prefix site run check
npm --prefix site run build
```

Use these commands when invoking the runtime:

```sh
decodex --help
decodex status
decodex diagnose --json
decodex mcp serve --transport stdio
decodex serve --listen-address 127.0.0.1:8192
```

`README.md` remains the better source for the broad CLI usage list. This document owns
the repository-memory owner for validation and entrypoint selection.

Use these commands when installing or auditing repo-owned Codex app automations on a
new machine:

```sh
python3 automations/decodex/scripts/config/sync_automations.py --apply
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/decodex/automations.toml
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/radar/automations.toml
```

`sync_automations.py` installs live `$CODEX_HOME/automations/*/automation.toml` files
from repo manifests and prompts. The checked-in source stays portable: manifests use
relative paths and `cwd = "{repo_root}"`, while the installer resolves the current
clone path only in local Codex app config. It also refuses configured private prompt
fragments such as absolute user-home paths, auth files, account files, and runtime
databases before writing.

## Source Entrypoints

| Surface | Entrypoint |
| --- | --- |
| Runtime CLI | `apps/decodex/src/main.rs`, `apps/decodex/src/cli.rs`, `apps/decodex/src/lib.rs` |
| App helper binary | `apps/decodex/src/bin/decodex-app-helper.rs` |
| OKF/docs command behavior | `apps/decodex/src/docs_okf.rs`, `apps/decodex/src/cli.rs` |
| Rust package manifest | `apps/decodex/Cargo.toml` |
| Root workspace manifest | `Cargo.toml` |
| Static site scripts | `site/package.json` |
| Native macOS app package | `apps/decodex-app/Package.swift` |

## Local Prerequisites And Generated State

`site/node_modules/` must exist for the site `npm` tasks, but it is local dependency
state and is not tracked. Run `npm install` or `npm ci` in `site/` when a fresh
worktree lacks dependencies.

The following paths are generated or local-only and are not source entrypoints:

- `target/`
- `site/dist/`
- `site/.astro/`
- `.worktrees/`
- `.decodex/`

## Owner Boundary

This concept owns build, test, run, validation, setup, automation resources, and
automation entrypoints questions. Use [`./test-suite.md`](./test-suite.md) for test
inventory and placement standards, and [`./workspace-layout.md`](./workspace-layout.md)
for directory ownership boundaries.

When validation or setup questions are not discoverable from `docs/index.md`, lane
indexes, or nearby related links, treat that as an owner-navigation issue and add a
specific index entry or relationship link rather than duplicating this command list in
another document.

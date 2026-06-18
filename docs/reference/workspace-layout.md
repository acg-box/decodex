---
type: "Reference"
title: "Workspace Layout"
description: "Describe the current top-level repository surfaces and which concerns each one owns."
status: active
authority: current_state
owner: docs
tags: [reference]
last_verified: 2026-06-18
---
# Workspace Layout

Purpose: Describe the current top-level repository surfaces and which concerns each one
owns.

Read this when: You need to know where runtime code, static-site code, workflow policy,
and documentation topics currently live.

Not this document: The normative behavior contract, operator procedures, or durable
design rationale.

Covers: The repository surface map, ownership boundaries, and local directories that
should not be treated as repository source.

For build, test, run, setup, validation, and task-runner command entrypoints, read
[`./build-test-run.md`](./build-test-run.md).

## Top-level surfaces

| Path | Role |
| --- | --- |
| `apps/decodex/` | Rust package that builds the `decodex` CLI and runtime. Runtime, orchestration, tracker integration, app-server integration, operator HTTP, and local control-plane behavior live under `apps/decodex/src/`. |
| `apps/decodex-app/` | SwiftPM macOS app for local Decodex Codex account-pool management. It talks to the bundled `decodex-app-helper`, which links the Rust account service directly, and does not own runtime scheduling or operator dashboard state. |
| `site/` | Astro static site for the public Decodex product surface and app download entry. It is not backed by a live Decodex daemon and does not own upstream monitoring or public publishing automation. |
| `scripts/assets/` | Asset-generation helpers for checked-in app and tray icon assets. |
| `scripts/macos/` | macOS-only app packaging and local bundle verification helpers. |
| `plugins/decodex/` | Canonical installable Decodex plugin source and reusable agent-facing skills, including issue briefing, planning, manual CLI, automation, commit, land, and labels. |
| `docs/spec/` | Normative runtime, workflow, site, and content contracts. |
| `docs/runbook/` | Operator procedures, validation sequences, deployment steps, and content workflows. |
| `docs/reference/` | Current repository and artifact surface maps. |
| `docs/decisions/` | Durable rationale for repository-level design choices. |
| `docs/research/` | JSON research artifacts and supporting evidence. It does not own runtime authority, policy, current-state reference, or durable rationale until promoted into the matching primary docs lane. |
| `dev/` | Local development helpers, such as the operator dashboard mock server. |
| `assets/` | Shared static assets that are not owned by the Astro app's generated output. Decodex App icons live under `assets/app-icon/{source,composer,generated}/`; menu bar template assets live under `assets/tray-icon/{source,generated}/`; `scripts/assets/render_decodex_app_icons.swift` regenerates the icon set. |
| `Makefile.toml` | Repo-native task names and automation entrypoints. |
| `decodex.example.toml` | Redacted template for a project `project.toml`; live project contracts live under `~/.codex/decodex/projects/<service-id>/`. |

## Rust workspace

The root `Cargo.toml` is a workspace manifest. It does not define a root package.

`apps/decodex/Cargo.toml` is the only checked-in Rust package in this first integrated
layout. Use package-qualified Cargo commands only when validating source changes from
the workspace root:

```sh
cargo check -p decodex --all-features --all-targets
cargo build -p decodex
```

For Decodex CLI usage and the current aggregate validation gates, use
[`./build-test-run.md`](./build-test-run.md) instead of treating this layout reference
as command authority.

Do not add new runtime behavior to a root `src/` directory. If Decodex later needs
shared crates, add them under `packages/` and make the boundary explicit in this
reference document and the root workspace manifest.

## Static public site

`site/` remains the public, static Decodex surface. It owns:

- homepage rendering
- public static assets
- appcast download widget
- Astro build and type-check behavior

The site does not own:

- Decodex runtime scheduling
- local operator state
- tracker writes
- app-server orchestration
- live operator dashboard behavior
- upstream monitoring
- public publishing automation

Those runtime and operator surfaces stay in `apps/decodex/` and `docs/spec/`.

## Installable Codex surface

The installable Codex home surface, including `~/.codex/AGENTS.md`, is not a Decodex
runtime contract and is not tracked in this repository. Do not reintroduce checked-in
`.codex/AGENTS.md` content here to carry Decodex-specific policy.

Global agent guidance should stay portable. Decodex-specific runtime, workflow,
identity, review, landing, closeout, and cleanup policy belongs in `apps/decodex/src/`,
`docs/spec/`, the registered project `WORKFLOW.md`, project `project.toml`, runbooks,
or the Decodex plugin skill that owns a reusable method. The normative split is defined
by [`../spec/installable-agent-policy.md`](../spec/installable-agent-policy.md).

## Local Decodex home

Runtime state that belongs to the local operator, not to this repository, lives under
`~/.codex/decodex/`:

- `runtime.sqlite3` is the single-machine control-plane database for all registered
  projects. It owns run leases, attempts, private execution events, tracker/PR
  caches, retained PR state, retry state, and project registration.
- `agent-evidence/<service-id>/` stores local agent-readable diagnosis artifacts,
  including `handoff-index.json`, `events.jsonl`, `blockers/*.json`, and
  `runs/<yyyy-mm>/<run-id>/capsule.json`. This is a derived handoff view, not the
  runtime source of truth and not a public mirror.
- `accounts.jsonl` stores the optional shared ChatGPT account pool used for
  Codex app-server auth token injection and refresh.
- `account-usage-history.jsonl` stores bounded local usage percentages and non-secret
  capacity weights for the account pool display.
- `logs/` stores Decodex process logs. Logs are diagnostic text; structured execution
  evidence belongs in `runtime.sqlite3`.
- `projects/<service-id>/project.toml` stores the central service config for one
  registered project.
- `projects/<service-id>/WORKFLOW.md` stores that project's execution policy.
- Project discovery comes from explicit registration, not from scanning Codex history
  or repo-local config files.

`.decodex/` is ignored by Git and reserved for local-only runtime or agent state. Do
not introduce checked-in repository behavior that depends on repo-local `.decodex/`
files.

This local control-plane state chooses registered projects. Once a checkout is selected,
the matching project directory's `WORKFLOW.md` remains the execution contract for gates,
tracker routing, and policy.

## Boundary notes

- Runtime authority stays in `apps/decodex/src/`, the registered project contract under
  `~/.codex/decodex/projects/<service-id>/`, and the governing specs under
  `docs/spec/`.
- Public site authority stays in `site/` and the site spec.
- Reusable agent-facing Decodex usage instructions live under `plugins/decodex/`.
- `docs/runbook/`, `docs/reference/`, and `docs/decisions/` must not override runtime or
  workflow authority.
- `docs/research/` is a JSON research artifact lane. Current Decodex runtime research
  authority still flows through runtime-local Decision Contracts until accepted and
  promoted.

## Local-only and generated directories

These paths are intentionally ignored and should not be treated as tracked repository
structure:

- `target/`: Rust build products and local analysis artifacts
- `site/dist/`: Astro build output
- `site/.astro/`: Astro local cache
- `.worktrees/`: local Git worktree lanes
- `.workspaces/`: local clone-backed workspace lanes from older workflows
- `.codex/`: local agent/runtime state, except the app-local
  `apps/decodex-app/.codex/environments/environment.toml` run action config

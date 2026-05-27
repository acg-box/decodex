---
name: manual-cli
description: "Use for human-driven Decodex CLI workflows: project registration, probe, status, dry-run probes, live-run routing, commit, land, archive hygiene, and local operator inspection. Does not own retained-lane automation policy beyond routing to the automation skill."
---

# Manual CLI

## Goal

Use Decodex as a CLI assistant for human-driven development without taking over the
runtime-owned retained-lane lifecycle.

## Read First

- `README.md` for the current CLI shape.
- `Makefile.toml` before running repo-native checks.
- `docs/reference/operator-control-plane.md` when interpreting `status` or dashboard
  fields.
- `docs/runbook/linear-archive-hygiene.md` before archiving old terminal Linear issues.
- `docs/runbook/self-dogfood-pilot.md` for the full self-dogfood operator sequence.

## Command Surface

Use the installed `decodex` binary when the operator is working from an installed
runtime. Use `cargo run -p decodex --bin decodex -- ...` when developing this
repository itself.

Common manual checks and dry-run probes:

```sh
decodex probe stdio://
decodex project add "$HOME/.codex/decodex/projects/<service-id>"
decodex project list
decodex status
decodex run --dry-run
decodex archive-linear --repo-label repo:<name> --older-than-days 30
```

Development equivalents from the Decodex repo root:

```sh
cargo run -p decodex --bin decodex -- probe stdio://
cargo run -p decodex --bin decodex -- project add "$HOME/.codex/decodex/projects/<service-id>"
cargo run -p decodex --bin decodex -- status
cargo run -p decodex --bin decodex -- run --dry-run
cargo run -p decodex --bin decodex -- archive-linear --repo-label repo:<name> --older-than-days 30
```

Live `run` commands enter the runtime-owned automation path, even when an operator
starts one pass manually:

```sh
decodex run
decodex run <ISSUE>
cargo run -p decodex --bin decodex -- run
cargo run -p decodex --bin decodex -- run <ISSUE>
```

Before starting a live run, read the `automation` skill and the registered project's
`WORKFLOW.md`. Treat live `run` as orchestration, not as a status check.

Manual commit and landing are separate narrow workflows:

- Use `commit` before creating a Decodex-formatted local commit.
- Use `land` only when the user asks to land a human-driven PR through Decodex.

## Project Registration

- `project add` registers or refreshes a project directory in the local runtime DB.
- The project directory must contain `project.toml` and `WORKFLOW.md`.
- `project.toml` owns `[paths].repo_root`, service identity, and credential env-var
  names.
- Project-scoped commands without `--config` resolve through the explicit registry;
  they do not scan arbitrary checkouts for repo-local config files.

## Status and Dry Run

- Use `status` to inspect active lanes, queue state, review and landing state, recovery
  worktrees, and the run ledger.
- Use `run --dry-run` before live automation to validate project loading, issue
  discovery, eligibility, and worktree planning without tracker mutation.
- Use `probe stdio://` before relying on the Codex app-server boundary.
- Treat hidden `serve --dev` as isolated local-development infrastructure only. It
  serves dashboard, account, and app snapshot APIs, but it does not register projects,
  poll Linear, dispatch work, or accept `--config` or `--interval`. Decodex App's
  fallback server uses ordinary `serve` when no compatible local listener is already
  running.
- For `skills/list` app-server preflight output, enabled skills plus scan diagnostics
  are local evidence, not a lane blocker. Missing cwd coverage or zero enabled skills
  are blockers; inspect `first_error_path` and `first_error` before changing plugin or
  skill installs.

## Boundaries

- Do not treat `run --dry-run` as proof that a live run can complete tracker writes,
  PR handoff, or closeout.
- Do not hand-edit runtime DB state unless a runbook explicitly says to.
- Do not clean up retained automation worktrees from the side when `status` shows a
  live or recovery-owned lane.

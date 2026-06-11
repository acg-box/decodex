---
name: manual-cli
description: "Use when a human is driving Decodex CLI workflows: project registration, probe, status, dry-run checks, live-run routing, commit, land, archive hygiene, or local operator inspection."
---

# Manual CLI

## Goal

Use Decodex as a CLI assistant for human-driven work without accidentally taking over
runtime-owned retained-lane lifecycle.

## Read First

- `README.md` for current CLI shape.
- `Makefile.toml` before repository-native checks.
- `docs/reference/operator-control-plane.md` when interpreting `status` or dashboard
  fields.
- `docs/spec/lane-control.md` and `docs/runbook/lane-control-recovery.md` before
  interrupting, steering, retrying, resuming, relabeling, or escalating lanes.
- `docs/runbook/linear-archive-hygiene.md` before archiving old terminal Linear issues.
- `docs/runbook/self-dogfood-pilot.md` for the full self-dogfood operator sequence.

## Command Surface

Use installed `decodex` when operating an installed runtime. Use
`cargo run -p decodex --bin decodex -- ...` when developing this repository itself.

Common installed-runtime probes:

```sh
decodex probe stdio://
decodex project add "$HOME/.codex/decodex/projects/<service-id>"
decodex project list
decodex status
decodex status --live
decodex run --dry-run
```

Repo-development equivalents:

```sh
cargo run -p decodex --bin decodex -- probe stdio://
cargo run -p decodex --bin decodex -- status
cargo run -p decodex --bin decodex -- status --live
cargo run -p decodex --bin decodex -- run --dry-run
```

## Live Run Warning

`decodex run` and `decodex run <ISSUE>` enter runtime-owned automation, even when a
human starts one pass manually. Before live run, read the `automation` skill and the
registered project's `WORKFLOW.md`.

## Manual Lifecycle

- Use `commit` before creating a Decodex-formatted local commit.
- Use `land` only when the user asks to land a human-driven PR through Decodex.
- Use `run --dry-run` only as an intake/worktree-planning check. It does not prove live
  tracker writes, PR handoff, closeout, or app-server execution will succeed.
- Use `probe stdio://` before relying on the Codex app-server boundary.
- Use `POST /api/linear-scan` after label or issue-state changes when the scheduler
  should refresh before its next 5-minute Linear poll.

## Boundaries

- Do not hand-edit runtime DB state unless a runbook explicitly says to.
- Do not clean up retained automation worktrees from the side when status shows a live
  or recovery-owned lane.
- Do not kill hidden `_attempt` children or directly mutate Linear tracker state to
  simulate lane controls.
- Do not treat app-server `skills/list` diagnostics as blockers without checking
  whether cwd coverage and enabled skills are actually missing.

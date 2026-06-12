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
- Use `recover review-handoff diagnose` and then `recover review-handoff rebind` for
  retained PR handoff state drift; the live rebind owns marker refresh plus the narrow
  current-marker failure-state label/state repair described in the runbook.
- Use `recover review-handoff adopt` for a verified human-owned PR that was created
  from a managed Decodex worktree and should enter normal `decodex land --authority`
  closeout. Run dry-run first from the lane worktree, then rerun live only after it
  confirms active-label state, clean worktree, exact PR branch/head match, and green
  landable PR gates. If the active service label is missing, live adopt may restore it
  after all other validation passes and will report that in dry-run. Adopt may reuse an
  existing worktree mapping only when it points at the same current managed checkout; a
  stale mapping branch name is repaired during the successful adopt write. Do not use
  adopt when a retained review handoff marker already exists; use rebind or normal land
  there.
- Use `recover merged-closeout` when Decodex still reports stale retained attention
  but the PR was already merged, the tracker issue is completed, and no real retained
  patch remains. Run dry-run first with the merged PR URL; live recovery requires
  `--manual-authority` and writes `closeout` plus `cleanup_complete` only after it
  validates completed issue state, absent queue/active/attention labels, matching PR
  head branch, merge commit containment in `origin/<default-branch>`, and clean or
  absent retained worktree state.
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

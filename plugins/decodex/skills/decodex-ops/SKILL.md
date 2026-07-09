---
name: decodex-ops
description: Use when Decodex runtime operations, retained automation, human-driven CLI commands, or Decodex Linear labels affect intake, recovery, lane control, or operator readback without creating commits or landing PRs.
---

# Decodex Ops

Operate Decodex runtime and tracker surfaces without taking over commit or landing
authority. Read `../../references/routing.md` when Program Intake, retained lanes,
service labels, recovery, or lane-control details matter.

## Runtime Automation

- Confirm execution authority before tracker or runtime mutation.
- Inspect `decodex status` or `decodex lane inspect <ISSUE>` first.
- Treat status, dashboard, MCP, and Linear readbacks as projections of kernel-owned
  lifecycle state; do not use rendered strings as policy authority.
- Mutating recovery, retry, terminalization, post-review, or queue guardrail work
  must be justified by typed command intents or the owning recovery runbook.
- With MCP, use `decodex_observe` for readback, `decodex_lane_control` for
  inspect-first steer/interrupt, and `decodex_project_control` only for project
  status or future-dispatch pause/resume.
- Use Program Intake for accepted Program work and ordinary service labels only for
  non-Program issue intake.
- Treat `decodex:needs-attention` and `terminal_pending` as stop signals.

## Manual CLI

- Use `decodex ...` for human-driven Decodex commands.
- Treat `run --dry-run` as planning evidence, not proof of live writes or closeout.
- Before live `decodex run`, inspect the project `WORKFLOW.md` and runtime state.

## Review-Handoff Recovery

- For `missing_review_handoff_record`, start with
  `decodex recover review-handoff diagnose <ISSUE> --json`.
- If diagnosis proves a Decodex-owned retained lane PR and matching retained
  worktree, branch, and head lineage, dry-run
  `decodex recover review-handoff rebind <ISSUE> --pr <URL> --dry-run` before any
  live rebind.
- If diagnosis shows a human-owned PR takeover from the current managed worktree,
  dry-run `decodex recover review-handoff adopt <ISSUE> --pr <URL> --dry-run`
  before any live adopt.
- If a later issue and merged successor PR already landed the accepted repair, do
  not rebind the obsolete PR. Dry-run
  `decodex recover superseded-closeout <ISSUE> --pr <OLD_PR> --successor-issue <ISSUE> --successor-pr <MERGED_PR> --dry-run`
  before live superseded closeout. The obsolete issue must have no queue, active,
  or needs-attention labels or live runtime ownership, including non-terminal
  attempts such as `continuation_pending`, retained retry schedules on the
  worktree marker, and retained worktree markers that still show live process
  or active thread. Any retained protocol, activity, or progress evidence also
  blocks terminalization when the marker process is dead or no matching latest
  attempt row exists; explicit stale-lane recovery must clear or classify that
  evidence first. The
  successor issue must expose a Decodex ledger record for the exact successor
  PR head and merge commit. Live superseded
  closeout records retryable pending closeout authority before public Linear or
  GitHub projections, then records completed closeout authority before public
  cleanup projection and retained worktree cleanup. Closeout recovery retries
  any local ledger duplicate whose matching public Linear comment is absent.
- Merged closeout recovery records lifecycle authority before public Linear
  closeout or cleanup projections and only clears retained run/worktree state
  after required projections succeed or are confirmed already present.
- Do not infer PR lineage from branch names, PR titles, Linear comments, status
  summaries, or stale snapshots.

## Labels

- `decodex:queued:<service-id>`: ordinary issue intake candidate.
- `decodex:active:<service-id>`: runtime-owned active lane marker.
- `decodex:manual-only`: human opt-out.
- `decodex:needs-attention`: human-required stop.
- Read `<service-id>` from `project.toml`; do not guess it.
- Queue only issues startable under `WORKFLOW.md`.
- Require explicit execution intent or accepted project-policy authority before
  derived intake.

## Boundaries

Do not edit runtime DB rows, hidden children, Linear state, retained worktrees, or
service labels from the side. Do not create commits or land PRs from this skill; use
`commit` or `land` for those narrow authority surfaces.

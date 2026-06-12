---
name: automation
description: "Use when Decodex owns runtime automation: registered projects, serve, issue intake, retained lanes, tracker tools, review handoff, repair, landing, closeout, cleanup, or operator recovery."
---

# Automation

## Goal

Operate Decodex as the retained-lane control plane for automatic development.

Automation starts only after execution authority exists. Natural-language `research X`
may create a latent Decision Contract, but latent research must not dispatch retained
lanes, set Codex goals, mutate tracker state, or apply queue labels until a later
promotion request clearly accepts the contract.

## Read First

- Read `project.toml` and `WORKFLOW.md` under
  `~/.codex/decodex/projects/<service-id>/` before interpreting runtime policy.
- Read `README.md`, `docs/index.md`, and `Makefile.toml` when developing Decodex
  itself.
- Read `docs/spec/runtime.md`, `docs/spec/tracker-tools.md`,
  `docs/spec/post-review-lifecycle.md`, and `docs/reference/operator-control-plane.md`
  when the exact runtime/status contract matters.
- Read `docs/spec/lane-control.md` and `docs/runbook/lane-control-recovery.md` before
  interrupting, steering, retrying, resuming, relabeling, or escalating a lane.

## Core Workflow

1. Confirm authority: accepted/promoted Decision Contract, explicit human execution
   request, or normal issue brief that grants implementation authority.
2. Inspect current state with `decodex status`, `decodex status --json`, or
   `decodex lane inspect <ISSUE>` before mutating anything.
3. Queue only ready issues with `decodex:queued:<service-id>`. Use the `labels` skill
   for Decodex Linear labels.
   blocked, stale, paused, active, terminal, or unmapped internal nodes remain
   unqueued.
4. Request `POST /api/linear-scan` after creating or relabeling queued work when the
   scheduler should observe it before the next 5-minute poll.
5. Let retained lanes finish through runtime-owned review, repair, handoff, landing,
   closeout, and cleanup unless the operator explicitly moves the lane to a manual
   path.

## Immediate Commands

Installed runtime:

```sh
decodex probe stdio://
decodex project add "$HOME/.codex/decodex/projects/<service-id>"
decodex status
decodex run --dry-run
decodex run
decodex serve
```

Decodex repo development:

```sh
cargo run -p decodex --bin decodex -- probe stdio://
cargo run -p decodex --bin decodex -- status
cargo run -p decodex --bin decodex -- run --dry-run
cargo run -p decodex --bin decodex -- run
cargo run -p decodex --bin decodex -- serve
```

## Required Signals

- Intake starts from `decodex:queued:<service-id>` and active ownership uses
  `decodex:active:<service-id>`.
- `decodex:manual-only` opts out of automation.
- `decodex:needs-attention` is a human-required stop that automation must not silently
  retry. The only runtime-owned clear path is the explicit review-handoff rebind
  recovery where a current same-PR same-head marker proves stale failure-state drift.
- `recover review-handoff adopt` is an explicit human/operator manual takeover path,
  not an automation retry path. Use it only when a verified human-owned PR from a
  managed worktree should be adopted into the retained review/landing lifecycle. It may
  reuse an existing worktree mapping only when that mapping points at the same current
  checkout; a stale mapping branch name is repaired by the successful adopt write. If
  the active service label is missing, dry-run must report whether live adopt will
  restore it after validation. Adopt must not clear needs-attention or replace normal
  retained-lane rebind.
- `recover merged-closeout` is the explicit stale retained-attention reconciliation
  path after a PR was already merged and the issue is already completed. Use dry-run
  first; live recovery requires manual authority and writes `closeout` plus
  `cleanup_complete` only after PR lineage, origin/default containment, labels, and
  retained worktree safety checks pass.
- A lane is terminal only after exactly one terminal path is finalized:
  `review_handoff` or `manual_attention`.
- `phase = terminal_pending` means the agent already called terminal finalize and
  Decodex is finishing lifecycle writeback. Do not interrupt, requeue, or manually
  clear it from the side.

## Boundaries

- Do not expose graph ids, DAG edge editing, hidden goal ids, or queue-label mechanics
  as the ordinary user workflow.
- Do not directly edit runtime DB rows, kill hidden `_attempt` children, or mutate
  Linear state to simulate lane controls.
- Do not substitute manual `decodex land` for runtime-owned retained-lane landing
  unless the operator explicitly moves the lane to a human-driven landing path.
- Treat `skills/list` preflight diagnostics as evidence to inspect. Missing cwd
  coverage or zero enabled skills are blockers; unrelated installed-skill scan
  diagnostics alone are not.

---
type: Runbook
title: Orchestration Kernel Cutover
description: Defines the direct cutover sequence for replacing scattered Decodex lane lifecycle decisions with a single typed orchestration kernel.
status: active
authority: procedural
owner: runtime
tags: [runtime, orchestration, kernel, lane-control, refactor, validation]
source_refs: []
code_refs:
  - apps/decodex/src/orchestrator/kernel.rs
  - apps/decodex/src/orchestrator/kernel/action.rs
  - apps/decodex/src/orchestrator/kernel/command.rs
  - apps/decodex/src/orchestrator/kernel/decision.rs
  - apps/decodex/src/orchestrator/kernel/lane_control.rs
  - apps/decodex/src/orchestrator/kernel/post_review.rs
  - apps/decodex/src/orchestrator/kernel/state.rs
  - apps/decodex/src/orchestrator/lane_decision.rs
  - apps/decodex/src/orchestrator/status/run_projection/run/lane_control.rs
  - apps/decodex/src/orchestrator/status/queue.rs
  - apps/decodex/src/orchestrator/status/review_orchestration.rs
  - apps/decodex/src/orchestrator/retained_review_orchestration.rs
  - apps/decodex/src/orchestrator/run_cycle/project.rs
  - apps/decodex/src/orchestrator/daemon/spawn.rs
  - apps/decodex/src/orchestrator/types/dispatch.rs
related:
  - ../spec/owned-lane-policy.md
  - ../spec/lane-control-state.md
  - ../spec/loop-runtime.md
  - ../spec/post-review-lifecycle.md
  - ../spec/runtime.md
  - ../reference/build-test-run.md
drift_watch:
  - OwnedLaneAction
  - LaneObservation
  - OwnedLaneDecision
  - CommandIntent
  - LaneNextAction
  - PostReviewLaneDecision
  - lane_control_next_action
  - classify_queued_issue
  - review_lifecycle_records
  - cargo make check-rust
  - cargo make test
  - cargo make check-docs
last_verified: 2026-07-01
---

# Orchestration Kernel Cutover

Purpose: Execute the direct Decodex orchestration-kernel cutover without a production
shadow path.

Read this when: A lane is replacing scattered scheduler, retry, post-review,
lane-control, queue, and status lifecycle decisions with one typed kernel.

Not this document: The normative owned-lane action policy, the lane-control state
contract, or the broader runtime reconciliation rules. Those remain owned by the
linked specs.

Covers: Target architecture, checkpoints, required subagent reviews, validation gates,
and completion evidence.

## Target Shape

The runtime decision path must become:

```text
typed facts -> orchestration kernel -> owned-lane decision -> command intents + projections
```

The kernel is pure. It does not read or write SQLite, tracker state, GitHub, worktrees,
process state, files, sockets, or app-server sessions. Existing modules may collect
facts, execute typed command intents, or render compatibility projections, but they
must not independently decide lane lifecycle policy after their surface is cut over.

## Canonical Vocabulary

`OwnedLaneAction` is the only domain action vocabulary and must stay aligned with
`docs/spec/owned-lane-policy.md`:

- `continue`
- `wait_for_external_signal`
- `retry_automatically`
- `resume_retained_lane`
- `manual_intervention_required`
- `ready_to_land`

Operational names such as `run_repo_gate`, `cleanup_terminal`,
`forbidden_stale_or_ambiguous`, `needs_review_repair`, and `continue_owned_attempt`
are command intents, phase details, reasons, or compatibility projections. They are
not owned-lane action classes.

## Required Kernel Output

Each reducer result must carry:

- `decision_class`: one `OwnedLaneAction`.
- `policy_state`: typed policy status used for guards.
- `lane_state_axes`: ownership, liveness, policy, and terminalization projection.
- `command_intents`: zero or more idempotent side-effect requests.
- `projection_hints`: compatibility strings for status, dashboard, MCP, and public
  readback.
- `blockers`: private reason codes and public-safe summaries.

Every mutating command intent must include an idempotency key, required precondition
facts, and expected postcondition facts.

## Non-Goals

- Do not add a production old/new shadow reducer path.
- Do not preserve duplicated policy branches for compatibility.
- Do not make JSON compatibility fields authoritative.
- Do not infer post-review authority from branch names, current HEAD alone, Linear
  comments alone, or helper marker names when a lifecycle record is required.
- Do not move side effects into the kernel.

## Implementation Checklist

### Checkpoint 0: Ground Rules

- [x] Confirm the branch is not `main`.
- [x] Confirm work happens in an isolated `.worktrees/` checkout.
- [x] Confirm checked-in task authority from `Makefile.toml`.
- [x] Confirm routed identity with `git config --get codex.github-identity` and
  `git config --get codex.linear-workspace`.
- [x] Keep this checklist updated as checkpoints complete.

Exit gate:

- [x] `git status --short --branch` proves the intended branch and clean or owned
  changes.

### Checkpoint 1: Kernel Skeleton And Golden Oracle

- [x] Add `apps/decodex/src/orchestrator/kernel/` with typed modules for facts, state,
  action, decision, command intents, projection hints, and reason codes.
- [x] Add golden reducer tests for all six `OwnedLaneAction` classes.
- [x] Add contradiction tests proving incomplete or conflicting authority resolves to
  `manual_intervention_required`.
- [x] Add compatibility projection tests for the legacy strings still exposed by
  status/readback.

Exit gate:

- [x] Targeted Rust tests for the kernel pass.
- [x] A read-only scout or skeptic subagent reviews the kernel boundary and reports no
  blocker.

### Checkpoint 2: Attempt, Phase, And Repo-Gate Cutover

- [x] Replace `LaneNextAction` lifecycle authority with `OwnedLaneDecision` output.
- [x] Keep legacy lane-decision event JSON stable through compatibility projection.
- [x] Route child-exit retry scheduling through command intents.
- [x] Route phase acceptance and repo-gate failure decisions through the kernel.
- [x] Remove dead operational variants that no longer have callers.

Exit gate:

- [x] Runtime repo-gate and phase-goal tests pass.
- [x] A read-only subagent reviews the attempt/phase cutover and reverse-scans for
  remaining duplicate authority.

### Checkpoint 3: Queue, Dispatch, And Program Candidate Cutover

- [x] Split queue classification from loop-guardrail checkpoint mutation.
- [x] Represent guardrail observe/clear operations as command intents.
- [x] Preserve scheduler priority: retry, post-review, Program, normal queue.
- [x] Ensure `IssueDispatchMode` remains execution mode, not lifecycle authority.
- [x] Preserve retained post-review blocking of normal and Program intake.

Exit gate:

- [x] Intake, candidate selection, queue, and Program scheduler tests pass.
- [x] A read-only subagent reviews that status/queue projection is pure.

### Checkpoint 4: Post-Review And Landing Cutover

- [x] Move post-review decision authority to kernel facts and decisions.
- [x] Treat review request, acknowledgement probing, bounded resend, repair, landing,
  closeout, and cleanup as command intents or phase details.
- [x] Use `review_lifecycle_records` as durable post-review authority where required.
- [x] Fail closed when lifecycle authority is missing or contradictory.
- [x] Preserve degraded PR/worktree readback wait behavior.

Exit gate:

- [x] Review landing, post-review classification, closeout, and cleanup tests pass.
- [x] A read-only subagent reviews lifecycle-record authority and side-effect adapter
  boundaries.

### Checkpoint 5: Lane-Control, Status, And Projection Cutover

- [x] Make lane-control axes kernel-owned typed state projections.
- [x] Keep status, inspect, dashboard, MCP, and Linear mirror outputs as pure
  projections.
- [x] Preserve current serialized status fields unless an intentional migration is
  documented.
- [x] Remove scheduler dependence on status-derived raw liveness interpretation.

Exit gate:

- [x] Operator status, lane inspect, dashboard, and projection tests pass.
- [x] A read-only subagent reviews projection compatibility and scheduler boundaries.

### Checkpoint 6: Cleanup, Docs, And Final Verification

- [x] Delete or reduce superseded helpers to fact collectors, side-effect executors, or
  compatibility renderers.
- [x] Update specs, references, and runbooks that name changed action vocabulary or
  authority boundaries.
- [x] Update `docs/log.md`.
- [x] Run formatting for touched Rust/TOML files.
- [x] Run focused test suites for every touched surface.
- [x] Run the broad validation gate or document any unavailable part with evidence.
- [x] Run final subagent skeptic review against the completion claim.

Exit gate:

- [x] `cargo make check-docs` passes.
- [x] `cargo make check-rust` passes.
- [x] `cargo make test` passes, or all failures are proven unrelated and scoped.
- [x] Final subagent review has no unresolved blocker.
- [x] Completion audit maps every checklist item to current evidence.

## Completion Audit

Current checkpoint evidence:

- CP0: `git status --short --branch` shows `xy/orchestration-kernel-cutover` in
  `.worktrees/orchestration-kernel-cutover`.
- CP1-CP5: targeted checkpoint suites passed during the cutover, with read-only
  subagent reviews recorded after each checkpoint.
- CP6 cleanup: superseded lifecycle helpers are either removed from authority paths
  or reduced to fact collection, command-intent adapters, or compatibility projection.
- CP6 docs: `docs/spec/lane-control-state.md`,
  `docs/spec/owned-lane-policy.md`, `docs/spec/post-review-lifecycle.md`,
  `docs/runbook/index.md`, and `docs/log.md` were updated for the kernel-owned
  vocabulary and authority boundary.
- CP6 formatting: touched Rust files were formatted with the same nightly rustfmt
  used by `cargo make fmt-check`; full `cargo make fmt-check` still fails on broad
  pre-existing workspace formatting drift outside the touched files. The
  `fmt-check` failure set was captured from `/tmp/decodex-fmt-check.log` and
  compared against the cutover path list: 151 formatted-diff paths, zero
  intersections with the cutover paths. That unrelated churn was not accepted into
  this cutover.
- CP6 focused Rust tests:
  - `cargo test -p decodex kernel::lane_control --lib`: pass, 8 tests.
  - `cargo test -p decodex operator::status --lib`: pass, 264 tests.
  - `cargo test -p decodex post_review --lib`: pass, 97 tests.
  - `cargo test -p decodex operator::status::agent_evidence --lib`: pass, 12 tests.
  - `cargo test -p decodex operator::status::http::lane_control --lib`: pass,
    11 tests.
- CP6 broad validation:
  - `cargo make check-docs`: pass.
  - `cargo make check-node`: pass after `npm ci` restored `site/node_modules`.
  - `cargo make check-rust`: pass.
  - `cargo make test`: pass, 1620 passed, 1 skipped.
  - `cargo make lint`: fails on existing strict Clippy debt outside the cutover
    scope, including unused imports in app-server/status aggregation modules,
    redundant `serde_json` imports, a derivable default, and an existing long
    program-intake test. Branch-introduced Clippy findings were removed or scoped.
- CP6 final skeptic review: Hubble reported no blockers and accepted the completion
  claim under the runbook's "passed or explicitly unrelated evidence" standard.

## Completion Standard

The cutover is complete only when the old lifecycle decision branches have been
deleted or reduced to adapters, all mutating work flows through kernel command intents,
all read surfaces are projections, all listed validation gates have passed or have
explicit unrelated-failure evidence, and the final subagent review finds no unresolved
architecture blocker.

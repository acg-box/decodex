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
  - cargo make check
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

## Active Lifecycle Slimming Ledger

This ledger tracks the direct lifecycle-slimming follow-up. It is the durable
anti-drift checklist for the current work; the high-level goal only states the final
acceptance contract.

### Scope Lock

- Remove Self Check review mode and all Basic review behavior from active runtime,
  prompts, config, tests, and docs.
- Remove the agent-facing review checkpoint tool from normal handoff and repair
  flows.
- Keep Standard review as a runtime-owned harness, not as an agent-reported
  checkpoint.
- Keep Strict review and GitHub review semantics fail-closed.
- Keep current-head clean review checkpoint semantics, docs impact, PR/head/worktree
  lineage, and review-blocking dirty-worktree checks.
- Do not preserve old/new compatibility branches, legacy marker authority, or
  short-term dual logic.
- Treat status, dashboard, recovery, landing, and closeout as projections or
  side-effect adapters over the lifecycle kernel result.

### Slice Checklist

- [x] Slice 1: Delete Self Check and Basic review from active config, prompts, docs,
  and tests.
- [x] Slice 2a: Delete the agent-facing `issue_review_checkpoint` surface from normal
  app-server tool exposure.
- [x] Slice 2b: Stop requiring agents to produce a clean checkpoint before
  `issue_review_handoff` or `issue_review_repair_complete`.
- [x] Slice 2c: Preserve current-head clean review checkpoint validation for runtime
  post-review progression.
- [x] Slice 2d: Distinguish `handoff`, `repair`, and missing runtime checkpoint phases
  in post-review facts.
- [x] Slice 2e: Re-check review-blocking worktree dirtiness after a clean checkpoint
  before allowing landing progression.
- [x] Slice 2f: Add or confirm a runtime-owned Standard review checkpoint producer so
  Standard lanes cannot wait forever after the agent-facing checkpoint surface is
  removed.
- [x] Slice 2g: Run a fresh skeptic review for the Standard-review gate and close all
  blockers before marking Slice 2 complete.
- [x] Slice 3: Collapse handoff, orchestration, landing, repair, and closeout authority
  into the single `review_lifecycle_records` authority record.
- [x] Slice 4: Build one structured post-review facts builder and route all
  post-review classifiers through it.
- [x] Slice 5: Make the lifecycle kernel the only producer of post-review
  `next_action`; all callers must consume kernel output or command intents.
- [x] Slice 6: Delete old classifiers, marker compatibility, duplicated
  churn/retry/blocker state machines, and authority-like projection writers.
- [x] Slice 7: Update specs, runbooks, and operator docs to describe only the new
  lifecycle authority model.
- [ ] Slice 8: Run focused tests, reverse scans, broad validation, and final skeptic
  challenge before any ready/done claim.

### Current Independent Review Findings

- [x] Repair-phase checkpoint was incorrectly keyed as `handoff`; fixed by carrying
  actual checkpoint phase through facts and classifiers.
- [x] Clean checkpoint progression did not re-check review-blocking dirty worktree
  changes; fixed with a post-checkpoint dirty gate.
- [x] Runtime-owned Standard review producer is proven with focused tests: pending
  Standard lanes invoke a runtime-owned reviewer, clean handoff checkpoints unblock
  the next reconcile tick, and prior non-clean review findings force a repair-phase
  runtime checkpoint.
- [x] Runtime-owned Standard review producer failures are bounded: retry count is
  persisted in the retained lifecycle authority and three consecutive producer
  failures become durable manual attention instead of an endless pending loop.
- [x] Runtime-owned Standard review terminal statuses fail closed durably:
  `blocked`, `needs_architecture_review`, and unknown checkpoint statuses route to
  retained manual attention rather than silently waiting.
- [x] Runtime-owned Standard review is only invoked after landing gates are green;
  pending checks keep the lane waiting without spending a review run.
- [x] Strict review now composes both gates: GitHub Review pass is necessary but not
  sufficient; landing still waits for a runtime-owned clean checkpoint on the
  reviewed head.
- [x] Strict status/dashboard projection now composes both gates too: a GitHub Review
  pass with green landing gates still projects `runtime_standard_review_checkpoint_pending`
  until the current head has a runtime-owned clean checkpoint.
- [x] Strict review producer failures stay in the external-result phase and preserve
  the runtime retry count, so GitHub Review is not re-requested and bounded failure
  still escalates to manual attention.
- [x] Runtime checkpoint lookup is current-head and phase-aware: same-head handoff
  and repair checkpoints are resolved by current artifact evidence instead of a
  fixed phase priority that can hide newer clean evidence.
- [x] Lifecycle authority projection is proven as the terminal source for landing and
  closeout: admin merge, manual issue-authority landing, already-merged recovery,
  and closeout adapters now submit kernel evidence and persist authority envelopes.
- [x] The handoff and orchestration state-store adapters no longer seed fake
  lifecycle-authority rows with `sequence = 0` / `runtime_projection`; they submit
  lifecycle-kernel decisions and persist authority envelopes before updating
  retained review readback fields.
- [x] Retained review loading now starts from `review_lifecycle_records`, fails closed
  when the lifecycle authority is missing or lacks PR base lineage, and exposes the
  retained lane as `ReviewLifecycleRecord` rather than an orchestration marker.
- [x] Ordinary dispatch blocking, post-review status loading, strict/non-GitHub status
  classification, and retained closeout cleanup now read the lifecycle authority
  record first instead of using handoff/orchestration marker tables as the semantic
  source.
- [x] Runtime retained-review dispatch no longer parses `ReviewOrchestrationPhase`
  from persisted state. It consumes kernel-owned `next_action` values instead:
  request, ack wait, result wait, landing-gate wait, repair, landing readback,
  closeout, or manual attention.
- [x] Status/dashboard classification no longer parses old orchestration phases.
  It consumes the same lifecycle `next_action` projection, so runtime and operator
  readback now share the lifecycle-kernel action vocabulary.
- [x] Active retained-review reconciliation and execution-failure drift recovery now
  write lifecycle transitions through `record_review_lifecycle_transition` instead
  of constructing orchestration marker records as the authority write path.
- [x] Admin merge success now records `landed` as the final lifecycle authority
  state without writing a later `waiting_for_merge` orchestration projection over
  that terminal authority.
- [x] Agent tracker-tool persistence and explicit review-handoff recovery apply now
  write post-review transition state through `ReviewLifecycleTransitionInput`
  instead of constructing active legacy transition-marker authority writes.
- [x] Retained review command intents and command facts now use lifecycle-authority
  vocabulary (`SyncReviewLifecycleAuthority` /
  `ReviewLifecycleAuthorityCurrent`) instead of orchestration-marker vocabulary.
- [x] Retained reconciliation and execution-failure drift recovery helper modules
  now write retained lifecycle authority transitions directly; active marker-shaped
  helper names were removed from those runtime paths.
- [x] Review-handoff rebind validation and recovery diagnostics now consume
  `ReviewLifecycleRecord` directly instead of rebuilding handoff/orchestration
  marker compatibility records before checking PR, head, branch, and issue-state
  binding.
- [x] Prompt context and post-review status snapshots now carry
  `ReviewLifecycleRecord` authority records directly instead of projecting retained
  review readback through legacy handoff-marker fields.
- [x] Manual issue-authority landing context now carries
  `ReviewLifecycleRecord` through closeout ledger and lifecycle decision writes
  instead of converting the record back into a legacy handoff marker.
- [x] Tracker-tool handoff/repair completion and explicit review-handoff
  adopt/rebind recovery now create lifecycle authority with direct
  `ReviewLifecycleHandoffInput` plus transition input instead of constructing active
  legacy handoff-marker write adapters.
- [x] State test helpers now use authority-first lifecycle fixtures and the old
  `ReviewHandoffMarker` / `ReviewOrchestrationMarker` names, old upsert/read
  adapter names, and old handoff/orchestration marker helper names are absent from
  source, specs, operator docs, and plugin text outside this ledger.
- [x] Review-handoff recovery, repair-apply, terminal finalize, stale-repair, and
  runtime-failure tests now use lifecycle-authority names instead of old marker
  compatibility names in module names, function names, and assertion text.
- [x] Old Linear execution ledger is demoted to execution-log/audit/readback context:
  it no longer answers landed, closed, cleanup, or final lifecycle state.
- [x] `decodex/commit/2` is commit-local only: `change`, `authority`, and `impact`;
  landing, PR, source branch, closeout, and related metadata are rejected or kept out
  of the schema.

### Required Reverse Scans

Before completion, these scans must return no active old-authority paths except
intentional deleted-file diffs or documented historical text:

```sh
rg -n 'review_checkpoint_tool_specs|review_checkpoint_(reviewer|status|contract|checks|finding_routes|findings_array)_schema|review_cost_control_schema|non_empty_string_array_schema|require_clean_review_checkpoint|mod clean_checkpoint_gate' apps/decodex/src -S
rg -n 'Self Check|basic review|ReviewLevel::Basic|review_level.*basic|before PR handoff|before `issue_review_handoff`|before `issue_review_repair_complete`|Decodex exposes `issue_review_checkpoint`|exposes `issue_review_checkpoint`|Call .*issue_review_checkpoint' docs apps/decodex/src -g '!docs/runbook/orchestration-kernel-cutover.md' -g '!docs/log.md' -S
rg -n 'ReviewHandoffMarker|ReviewOrchestrationMarker|upsert_review_handoff_marker|review_handoff_marker|upsert_review_orchestration_marker|review_orchestration_marker|sample_review_handoff_marker|sample_review_orchestration_marker|seed_review_handoff_marker|seed_review_orchestration_marker|persisted_review_handoff_marker|persisted_review_orchestration_marker|handoff_marker|orchestration_marker|retained_handoff_marker|lifecycle_marker|review_orchestration_(branch|head|pr)_mismatch|review_orchestration_runtime|apply_review_orchestration_phase_classification' apps/decodex/src docs plugins -g '!docs/runbook/orchestration-kernel-cutover.md' -S
```

### Required Validation

- [x] `cargo test -p decodex lifecycle --all-features -- --test-threads=1`
- [x] `cargo test -p decodex standard_review_waits_for_runtime_review_checkpoint_before_landing --all-features -- --test-threads=1`
- [x] `cargo test -p decodex reconcile_post_review_orchestration_waits_for_runtime_standard_review_checkpoint --all-features -- --test-threads=1`
- [x] `cargo test -p decodex reconcile_post_review_orchestration_escalates_runtime_standard_review_checkpoint_failure_after_budget --all-features -- --test-threads=1`
- [x] `cargo test -p decodex reconcile_post_review_orchestration_routes_runtime_standard_review --all-features -- --test-threads=1`
- [x] `cargo test -p decodex reconcile_post_review_orchestration_routes_unknown_runtime_standard_review_status_to_attention --all-features -- --test-threads=1`
- [x] `cargo test -p decodex reconcile_post_review_orchestration_runs_runtime_standard_review_after_external_pass_before_admin_merge --all-features -- --test-threads=1`
- [x] `cargo test -p decodex reconcile_post_review_orchestration_escalates_strict_runtime_review_failure_without_restarting_external_request --all-features -- --test-threads=1`
- [x] `cargo test -p decodex runtime_review_checkpoint_status_for_head_prefers_current_same_head_handoff_artifact --all-features -- --test-threads=1`
- [x] `cargo test -p decodex reconcile_post_review_orchestration_skips_runtime_standard_review_while_landing_gates_pending --all-features -- --test-threads=1`
- [x] `cargo test -p decodex review_landing_orchestration::landing_fallbacks --all-features -- --test-threads=1`
- [x] `cargo test -p decodex landed_lineage_merge::admin_merge --all-features -- --test-threads=1`
- [x] `cargo test -p decodex reconcile_post_review_orchestration_runs_runtime_standard --all-features -- --test-threads=1`
- [x] `cargo test -p decodex runtime_review_ --all-features -- --test-threads=1`
- [x] `cargo test -p decodex build_post_review_lane_statuses_waits_for_runtime_checkpoint_after_strict_pass --all-features -- --test-threads=1`
- [x] `cargo test -p decodex review_lifecycle --all-features -- --test-threads=1`
- [x] `cargo test -p decodex review_landing_status_rows --all-features -- --test-threads=1`
- [x] `cargo test -p decodex review_landing_orchestration --all-features -- --test-threads=1`
- [x] `cargo test -p decodex review --all-features -- --test-threads=1`
- [x] `cargo test -p decodex review_policy --all-features -- --test-threads=1`
- [x] `cargo test -p decodex review_level --all-features -- --test-threads=1`
- [x] `cargo make check-docs`
- [x] `git diff --check`
- [ ] Final fresh skeptic review has no unresolved blocker.

Current hard-cutover evidence:

- [x] `cargo fmt --package decodex`
- [x] `cargo check -p decodex --all-features`
- [x] `cargo test -p decodex --all-features --no-run`
- [ ] `cargo test -p decodex --lib --all-features -- --test-threads=1`
- [ ] `cargo test -p decodex --all-features -- --test-threads=1`
- [x] `cargo run -p decodex --bin decodex -- docs check`
- [x] `git diff --check`
- [ ] Fresh read-only skeptic review reported PASS after blocker repair. The latest
  fresh skeptic review returned blockers for unresolved index state, stale broad
  validation, stale runbook evidence, and remaining marker-era test/docs wording;
  these must be repaired before this line can be checked.

## Archived Prior Kernel Cutover Context

The checkpoint history below is retained as background from the earlier
`xy/orchestration-kernel-cutover` branch. It is not the acceptance contract for the
active lifecycle slimming work above; the authoritative current checklist is the
Active Lifecycle Slimming Ledger.

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

- [x] `cargo make check` passes.
- [x] `cargo make check-rust` passes.
- [x] `cargo make test` passes, or all failures are proven unrelated and scoped.
- [x] Final subagent review has no unresolved blocker.
- [x] Completion audit maps every checklist item to current evidence.

## Archived Completion Audit

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
  - `cargo make check`: pass.
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

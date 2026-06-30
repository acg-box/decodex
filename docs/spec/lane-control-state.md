---
type: Spec
title: Lane-Control State Specification
description: Defines the Decodex lane-control state model used by scheduling, guards, cleanup, and operator projections.
status: active
authority: normative
owner: runtime
tags: [lane-control, runtime, scheduler]
source_refs: []
code_refs: [apps/decodex/src/orchestrator/status/mod.rs, apps/decodex/src/orchestrator/tests/operator/status/http/mod.rs, apps/decodex/src/agent/tracker_tool_bridge/tools.rs]
related: [lane-control.md, runtime.md]
drift_watch: [ownership_state, liveness_state, policy_state, terminalization_state, review_churn_exceeded]
last_verified: 2026-06-27
---

# Lane-Control State Specification

Purpose: Define the authoritative Decodex lane-control state model used by
scheduler decisions, policy guards, terminal cleanup, and operator projections.
Status: normative
Read this when: You are implementing or validating lane scheduling, current-lane
projection, review-policy stops, retained recovery, closeout, or dashboard status.
Not this document: The operator command sequence for steering or interrupting a lane.
Use [`lane-control.md`](./lane-control.md) for CLI/API controls and
[`runtime.md`](./runtime.md) for broader runtime reconciliation rules.
Defines: The lane control state axes, invariants, guard semantics, terminal barrier,
and projection rules that prevent liveness evidence from re-creating ownership.

## Resource Model

Each Decodex lane is a control-plane resource with four separate state axes:

- `ownership_state`: who, if anyone, is authorized to mutate the lane.
- `liveness_state`: what local process, app-server, or protocol evidence is visible.
- `policy_state`: whether review, retry, architecture, or authority rules allow the
  current strategy to continue.
- `terminalization_state`: whether finalization has started, retired run control, or
  completed cleanup.

Operator snapshots may include derived fields for readability, but scheduler
decisions and running-lane counts must be based on the state axes rather than inferred
from protocol activity.

## Canonical Values

`ownership_state` values:

- `pending`: eligible or waiting before an owned attempt starts.
- `leased_run`: Decodex owns the run lease and the active attempt may mutate the
  lane.
- `terminalizing`: Decodex is retiring run control, finishing writeback, archiving the
  app-server thread, or cleaning up an owned attempt.
- `retained_attention`: useful retained state exists but autonomous mutation is stopped
  until recovery or human attention resolves the blocker.
- `orphaned_live_thread`: liveness evidence remains after Decodex lost active ownership.
- `closed`: no active ownership remains and no retained recovery bucket is required.

`liveness_state` values:

- `unknown`: no useful live evidence exists.
- `process_alive`: the recorded child process is still alive.
- `thread_active`: app-server reports an active thread or active flags.
- `protocol_recent`: recent protocol work evidence exists without a live process.
- `not_running`: the owned process/thread is stopped or archived.
- `host_boot_mismatch`: the process marker belongs to a previous host boot.
- `late_protocol_activity`: protocol activity arrived after a terminal barrier.

`policy_state` values:

- `allowed`: no policy stop is active.
- `review_pending`: a required review checkpoint has not been recorded.
- `review_findings`: the latest review checkpoint has non-clean findings but remains
  within the repair budget.
- `review_churn_exceeded`: repeated non-clean findings exceeded the convergence budget
  and the current repair strategy must stop.
- `architecture_recovery_pending`: architecture recovery is the next autonomous path.
- `authority_boundary_required`: a boundary decision or human authority is required.
- `human_attention_required`: the lane is blocked for explicit operator attention.

`terminalization_state` values:

- `none`: no terminal barrier is active.
- `barrier_started`: a terminal transition has started.
- `run_control_retired`: the run-control channel is no longer authoritative.
- `thread_archive_requested`: the app-server thread is being archived or has been
  asked to stop.
- `cleanup_pending`: closeout or worktree cleanup remains.
- `cleanup_complete`: the lane is closed out and cleanup is complete.

## Invariants

- A lane counts as a running lane only when `ownership_state` is `leased_run`.
- Liveness evidence may update `liveness_state`, but it must not create or restore
  `leased_run` ownership.
- When a newer visible attempt for the same issue exists, older attempts with stale
  or protocol-only liveness evidence stay out of current-lane projection and
  current-attention counts. The older attempts remain available as recent/history
  evidence, but only the newest attempt in that issue lineage may become current.
- A retry or automatic continuation for the same issue must resume the latest
  unterminated phase-goal state from the immediately previous attempt. A validated
  or active `handoff_evidence` phase must not be reset to
  `implement_to_validation_ready` merely because the runtime created a new attempt.
- `run_lease=false` is incompatible with `ownership_state=leased_run`.
- Terminal attempt statuses such as `failed`, `interrupted`, `stalled`, or `succeeded`
  must not be promoted to `running` by live process, thread, or protocol evidence.
- Issue-scoped terminal Run Ledger outcomes are authoritative for old or unowned run
  inspect/status projection. If no run lease or other authoritative live owner remains,
  `decodex lane inspect` and operator status must project the final ledger outcome
  into `status`, `attempt_status`, `phase`, `ownership_state`, `liveness_state`, and
  `lane_control_next_action`. That projection must not overwrite a still-leased
  current attempt for the same issue.
- `policy_state=review_churn_exceeded` blocks further review-repair mutation for the
  same strategy until architecture recovery or human attention changes the policy
  state.
- Review checkpoints that would move the current phase to
  `review_churn_exceeded` must fail immediately after persisting the checkpoint
  evidence; the runtime must not wait for turn-end classification to enforce the
  stop.
- Review-repair external round counters are monotonic for a retained review lineage;
  there is no fourth-round reset path.
- Protocol events after a terminal barrier are retained as evidence and projected as
  `late_protocol_activity`; they do not change `ownership_state`.
- Merged pull request plus completed tracker issue plus no owned active attempt
  projects as cleanup or closed state, not as a running lane.

## Guard Semantics

Every mutating lane tool must consult lane control state before writing tracker,
worktree, review, closeout, or run-control data. A guard decision has three outcomes:

- `allow`: the tool may proceed.
- `deny_terminal`: the lane is terminalizing or closed; the tool must not mutate it.
- `deny_policy`: policy state requires architecture recovery or human attention before
  another repair or handoff mutation.

`issue_review_checkpoint` may record the checkpoint that triggers a policy stop, and
`issue_terminal_finalize` may record terminal retention/cleanup after the stop.
Progress, handoff, repair-complete, closeout, transition, label, and comment tools
must be fenced while `policy_state=review_churn_exceeded` remains current for the
run's review phase and repair strategy. Changing the lane HEAD does not clear this
guard; architecture recovery or human attention must explicitly change the policy
state.

Guard decisions are private runtime evidence. Public tracker projections may summarize
the stop reason, but raw lane evidence remains local unless an allowlisted lifecycle
projection renders it.

## Projection Rules

Operator status and dashboard views must show the four state axes and `next_action`.
Raw observation fields such as `status`, `phase`, and `execution_liveness` are
diagnostics; scheduler code must not infer ownership from them.

Examples:

```text
ownership_state: closed
liveness_state: late_protocol_activity
policy_state: allowed
terminalization_state: cleanup_complete
next_action: ignore_late_activity
```

```text
ownership_state: retained_attention
liveness_state: host_boot_mismatch
policy_state: allowed
terminalization_state: none
next_action: inspect_recovery_evidence
```

```text
ownership_state: leased_run
liveness_state: process_alive
policy_state: review_findings
terminalization_state: none
next_action: repair_review_findings
```

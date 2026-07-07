---
type: "Runbook"
title: "Lane-Control Recovery"
description: "Procedure for recovering Decodex lanes after control-plane interventions or ambiguous retained evidence."
status: active
authority: procedural
owner: automation
tags: [runbook]
code_refs: [apps/decodex/src/cli.rs, apps/decodex/src/recovery.rs, apps/decodex/src/recovery/stale_active_guidance.rs, apps/decodex/src/orchestrator/execution.rs, apps/decodex/src/orchestrator/status.rs]
drift_watch: [decodex recover ghost-lane, decodex recover stale-active, stale_active_release, run_stale_active_recovery, linear_active_label_present, ghost_lane_cleanup_audit_present, mcp_test_fixture_ghost_lane, runtime_recovery_required, runtime_recovery_blocked, authority_boundary_check, architecture_recovery_packet, architecture_recovery_started, architecture_recovery_terminal, loop_status]
last_verified: 2026-07-02
---
# Lane-Control Recovery

Goal: Give agents and operators a bounded recovery sequence after Decodex lane
interrupt, hard fallback, broad steer, task replacement, or ambiguous retained-lane
evidence.

Read this when: A lane-control request has returned, timed out, fallen back, changed
task content materially, or left unclear evidence about whether a retained lane should
resume, requeue, stop, or require human attention.

Inputs: Registered project id, issue identifier, run id, attempt number, current turn
id when available, control request result, `decodex status` or
`decodex status --json`, private evidence from `decodex evidence`, tracker state,
retained worktree state, and PR lineage when present.

Depends on: [`../spec/lane-control.md`](../spec/lane-control.md),
[`../spec/tracker-tools.md`](../spec/tracker-tools.md),
[`../reference/operator-control-plane.md`](../reference/operator-control-plane.md),
[`./recover-review-handoff.md`](./recover-review-handoff.md), the Decodex
`decodex-ops` skill, plus the registered project `project.toml` and
`WORKFLOW.md`.

Verification: The chosen path should cite the inspection evidence, the control outcome,
the retained worktree or PR lineage when relevant, and the supported Decodex command,
API, label skill, or issue-scoped tracker tool used for the next mutation.

## Recovery Principle

Lane control is not a shortcut around retained-lane lifecycle. `turn/steer` can carry
broad operator text, and `hard_interrupt_fallback` can stop a recorded child process
when explicitly requested, but recovery still has to preserve audit, lane identity,
workflow policy, and useful local work.

Do not directly kill hidden `_attempt` children, edit runtime DB rows, or mutate Linear
labels to simulate lane control. The normal paths are CLI/API lane controls, retained
retry/resume, explicit recovery commands, label-skill actions, issue-scoped tracker
tools, and manual attention. If an operator had to stop a process outside Decodex
controls for immediate host safety, treat the next state as ambiguous evidence until
the lane, private evidence, and worktree have been inspected.

## Inspect First

Run the smallest set of inspections that can prove the lane identity and current owner:

```sh
decodex lane inspect <ISSUE> --run-id <RUN_ID> --json
decodex status --json
decodex diagnose --json
decodex evidence <ISSUE> --run-id <RUN_ID> --attempt <N> --json
```

Use the local HTTP API only against the same trusted listener when CLI access is not
the active surface:

```sh
curl -sS 'http://127.0.0.1:8192/api/lane/inspect?projectId=<service-id>&issue=<ISSUE>&runId=<RUN_ID>'
```

Before mutating anything, confirm:

- project id and registered project path
- issue identifier, tracker state, and service-scoped labels
- branch, worktree, and whether the worktree is active, retained, queued-attention, or
  cleanup-only
- run id, attempt, thread id, current turn id, and process/protocol liveness
- control outcome such as accepted, rejected, timed out, failed, or
  `hard_interrupt_fallback`
- `run_lease_missing` rejections together with process, protocol, channel, branch,
  and retained worktree evidence when the lane still appears live
- private evidence and public lifecycle signal
- latest issue-level phase-goal evidence; an open phase such as `handoff_evidence`
  survives later empty failed-start attempts until terminal finalization, review
  completion, a decision request, blocked recovery, blocker checkpoint, or audited
  failed-start cleanup closes it
- latest `authority_boundary_check` private event when guardrail pressure, broad
  steer, hard fallback, ambiguous retained progress, or uncovered direction could
  change the accepted authority envelope
- PR URL, head branch, and head SHA when the lane has crossed review handoff

If these facts do not prove the requested lane, do not steer, interrupt, retry, resume,
or clean labels.

## Decision Tree

| Evidence after inspection | Agent decision | Supported next action |
| --- | --- | --- |
| Active lane still matches the issue, branch, run id, attempt, and turn. | Let the runtime continue or wait for the control result. | No label change. Use the next CLI/API control only when the operator explicitly asks. |
| Soft interrupt was accepted and the runtime is still resolving the attempt. | Wait for status, protocol activity, or evidence to settle. | Re-inspect; do not requeue or force-kill. |
| Soft control was rejected with `run_lease_missing`, but inspect/status still shows the same run id, attempt, branch, active channel, and live child process or protocol activity. | Treat the lane as degraded active execution, not cleanup-only state. | Re-inspect with `decodex lane inspect` or use `decodex lane interrupt <ISSUE> --run-id <RUN_ID> --force` only when the operator explicitly wants hard process fallback. |
| Forced interrupt after `run_lease_missing` reports no signalable process. | Treat force as non-mutating for the child process. | Inspect retained worktree and private evidence; do not claim the interrupt succeeded or clear attention labels. |
| Hard fallback reports `hard_interrupt_fallback`. | Treat it as an interrupted runtime event, not a graceful completion. | Inspect retained worktree and evidence; resume only if lineage is exact. |
| Queue status shows `reason = linear_active_label_present` and `attention_next_action = run_stale_active_recovery`, or `recover stale-active diagnose` reports `classification = stale_active_ownership`. | Treat it as tracker-present stale active ownership, not missing-issue ghost recovery and not review-handoff recovery. | Run `decodex recover stale-active diagnose <ISSUE> --json`; if it proves tracker issue present, active label present, no live process, no source-progress worktree state, no unmerged retained branch commits, no unavailable retained default-branch proof, no uninspectable worktree, no private source/review progress evidence, and no PR/review lineage or review-policy checkpoint under either issue id key, run `decodex recover stale-active release <ISSUE> --dry-run`, then rerun without `--dry-run`. A run lease or active shared claim is recoverable only when process identity proves the recorded child is gone, the run-activity marker run id and attempt match the latest leased run, the local lease belongs to that same project/run, and no external or incompatible shared claim is present. Dead-process runtime telemetry such as stale thread status, active local control-channel files, protocol events, child/protocol summaries, failed control attempts, implementation phase-goal rows, app-server no-diff loop guardrail checkpoints, no-progress harness outcomes, and probing checkpoints is recoverable only after process identity proves the recorded child is gone and the worktree/lineage/progress guards are clean. The release terminal-guards active and terminal-looking app-server attempts such as `failed` or `interrupted` before final label-release safety, clears only matching proven-dead local run leases, preserves a queue label if present, revalidates before mutation, repeats the run-lease/shared-claim guard and tracker-label readback before removing the active label, removes only `decodex:active:<service-id>`, and treats stale thread/turn ids alone as recoverable metadata only after all progress checks are clean. |
| A previous `recover stale-active release` stopped after local cleanup and `recover stale-active diagnose` now shows `terminal_guarded` or a terminal-looking app-server status such as `failed` or `interrupted`, inactive or never-published control channel, missing worktree/mapping, `stale_active_release` audit evidence, active label still present, and only protocol/activity-summary blockers. | Treat it as idempotent stale-active release reentry. | Re-run `decodex recover stale-active release <ISSUE> --dry-run`, then rerun without `--dry-run` if the report stays safe. Do not hand-edit the active label; the command must still repeat run-lease/shared-claim, review-lineage, and tracker-label guards before removing only the service active label. |
| `recover stale-active diagnose` reports `classification = stale_active_state_restore_pending` after a previous release removed the active label while preserving the queue label, and the issue remains in the configured in-progress state. | Treat it as idempotent startable-state restore for the same stale-active release. | Re-run `decodex recover stale-active release <ISSUE> --dry-run`, then rerun without `--dry-run` if the report stays safe. The command may restore only the configured first startable state and only when the same run/attempt `stale_active_release` audit, missing worktree/mapping, inactive control channel, no run lease/shared claim, and no review lineage are still proven. |
| `recover stale-active diagnose` reports blockers. | Preserve the lane and follow the diagnostic `next_action`; blocked stale-active rows are retained-progress, review-handoff, live/unsettled-runtime, or missing-evidence cases, not release candidates. | Inspect the named blockers. Do not clear Linear labels or runtime rows while a run lease, active shared claim, needs-attention label, live process, unknown runtime-marker process liveness, tracked or untracked non-runtime worktree changes, uninspectable worktree, unavailable retained default-branch proof, retained branch commits not reachable from the default branch, private source/review progress evidence, review-policy checkpoint, or PR/review lineage exists. For `private_progress_evidence_present`, `worktree_tracked_changes_present`, `worktree_unmerged_commits_present`, or retained non-git files, inspect `decodex evidence <ISSUE> --json` and the retained worktree from the report before deciding whether to resume, recover review handoff, route manual attention, or explicitly discard work. For review lifecycle, review-policy, or PR-lineage blockers, run `decodex recover review-handoff diagnose <ISSUE> --json` instead of stale-active release. |
| Live or fresh cached status shows `ownership_state = ghost_lane`, `policy_state = runtime_recovery_required`, and `lane_control_next_action = run_ghost_lane_recovery`. | Treat it as a missing-issue local runtime ghost, not review-handoff recovery. | Run `decodex recover ghost-lane diagnose <ISSUE> --json`; if it still proves missing tracker issue, missing worktree, no live process, and no PR/review lineage, run `decodex recover ghost-lane cleanup <ISSUE> --dry-run`, then rerun without `--dry-run`. Ordinary lanes must also prove no control-channel row or file, no private evidence, and no thread/protocol evidence. The historical PubFi MCP fixture is recoverable only when diagnose reports `classification = mcp_test_fixture_ghost_lane` with `mcp_test_fixture_private_control_evidence_present` and no blockers; its private evidence may contain only `source = mcp-test` lane-control request rows and fixture-matching `control_action` audit rows with `source = mcp-test` or `source = cli`. |
| Status or diagnose shows `ghost_lane_cleanup_audit_present` for a missing-issue ghost and no retained worktree, live process, PR lineage, review lifecycle, or mixed private evidence. | Treat the prior cleanup as idempotently complete. | Re-run live status. The lane should be absent from current and retained-attention views; ordinary intake may proceed. If the row still appears with `runtime_recovery_blocked`, inspect the named blocker instead of editing runtime SQLite. |
| Live or fresh cached status shows `runtime_recovery_blocked`, or `recover ghost-lane` reports blockers. | Preserve attention. | Inspect the blocker named by status or diagnose output; do not hand-edit runtime SQLite rows or clear the lane while a tracker issue, retained worktree, control-channel file, live execution signal, private evidence outside the allowed PubFi MCP fixture control rows, mixed private evidence, PR lineage, or review lifecycle record exists. |
| Retained worktree has useful local changes and lineage matches issue, branch, runtime evidence, and PR when present. | Resume or repair the same lane. | Use `decodex run <ISSUE>` when the registered workflow makes it eligible, or use the specific retained recovery runbook. |
| Review lifecycle record is missing or stale but the retained PR lane appears recoverable. | Diagnose before rebind. | Run `decodex recover review-handoff diagnose <ISSUE>` and follow [`recover-review-handoff.md`](./recover-review-handoff.md). |
| A later failed-start attempt has little or no evidence, but an earlier issue-level phase-goal continuation is still open. | Preserve retained lifecycle ownership. | Resume the open phase or use the matching recovery runbook; do not classify the lane as failed-start cleanup debt or clear the worktree mapping. |
| Queue label or tracker state was changed and the scheduler should observe it before the next poll. | Request a refresh, not a retry. | `POST /api/linear-scan` with `projectId`, or no body for all enabled projects. |
| Queue label should be added, removed, or interpreted. | Use service-scoped label policy. | Follow the `decodex-ops` skill; do not guess `<service-id>` or clear `needs-attention` before fixing the blocker. |
| Broad steer materially changes the objective or acceptance contract. | Preserve audit and resolve lifecycle explicitly. | Update and requeue the same issue, create a new issue/lane, or route the owned run to manual attention. |
| Operator wants a different issue or replacement task. | Treat as task replacement, not steer. | Stop or pause through supported controls as needed, then create/update/requeue through the supported lifecycle. |
| Status or Linear failure summary reports a loop guardrail reason. | Inspect the reason-specific evidence, Architecture Recovery Packet, and Authority Boundary Check. | Follow the loop guardrail recovery table below before clearing `decodex:needs-attention` or requeueing. |
| Authority Boundary Check policy is `auto_continue`. | Continue only if lane identity, ownership, and validation evidence still match. | Resume through the supported retained-lane path; keep the boundary-check event as private evidence. |
| Authority Boundary Check policy is `requires_enhanced_evidence`. | Continue recovery, but require stronger tests, review, migration, or operator evidence before handoff or landing. | Keep the evidence requirement visible in private evidence and status; do not treat it as a human approval gate by itself. |
| Authority Boundary Check policy is `block_landing`. | Continue only to restore or strengthen validation/review policy evidence. | Do not hand off or land until the blocked evidence standard is restored and recorded. |
| Authority Boundary Check policy is `requires_human_decision`. | Stop automatic recovery and preserve the durable decision request. | Keep or apply `decodex:needs-attention`, inspect the Linear decision request and private `authority_decision_request` evidence, then continue only after the issue, Decision Contract, or policy accepts, rejects, or revises the requested authority change. |
| Evidence is missing, contradictory, or would require guessing whether local work is safe to overwrite. | Stop automatic recovery. | Use manual attention with structured public blockers and keep private evidence local. |

## Loop Guardrail Recovery

Loop guardrails stop non-converging automation after three consecutive matching
observations. They preserve retained worktrees and private evidence; they do not mean
the operator should delete local progress to make the queue clean.

Current runtime guardrail handling is two-stage:

1. stop the current ineffective repair strategy and record a private
   `architecture_recovery_packet`
2. record or consume an Authority Boundary Check to decide whether autonomous
   architecture recovery may continue

If the boundary check policy allows autonomous recovery and recovery budget remains,
Decodex may record `architecture_recovery_started` and retry with a materially
different implementation strategy. `requires_enhanced_evidence` and `block_landing`
continue recovery only with their evidence restrictions preserved. If the policy is
`requires_human_decision`, or if recovery budget is exhausted, Decodex must keep or
apply manual attention with a typed reason such as `contract_boundary_required`,
`external_dependency_required`, or `architecture_recovery_exhausted`.

| Guardrail reason | Inspect first | Resume only after |
| --- | --- | --- |
| `validation_repeat` | The repeated validation failure, repo-gate output, retained worktree, prior repair attempts, Architecture Recovery Packet, and boundary policy. | Autonomous recovery may continue only when the policy is `auto_continue` and budget remains; otherwise a human fixes the cause or records new authority. |
| `no_effective_diff` | The retained worktree status, private retry evidence, whether any useful tracked delta exists, Architecture Recovery Packet, and boundary policy. | Autonomous recovery may continue only when evidence proves the next strategy is an engineering implementation change inside authority; otherwise a human identifies the next diff, resets intentionally, or updates authority. |
| `remaining_delta_unchanged` | The unchanged tracked delta, latest validation evidence, Architecture Recovery Packet, and boundary policy. | The next repair must be bounded, materially different, and inside authority; otherwise a human accepts/resets the patch or updates authority. |
| `review_churn` or `review_policy_exhausted` | Fresh-context review checkpoints, active/stop finding fingerprints, accepted findings, rejected findings, current head, Architecture Recovery Packet, and boundary policy. | A materially different implementation strategy may continue with `block_landing`; architecture/product direction changes require human authority. |
| `dependency_program_stale` | The open blocker issue, Execution Program readiness, and whether the dependency split is still correct. | Resolve the dependency, refresh/split the program, or update accepted Decision Contract authority; do not auto-recover as ordinary implementation work. |
| `uncovered_direction` | The missing requirement or decision gap named in public/private evidence. | An accepted Decision Contract captures the missing direction and the issue is updated or requeued from that authority. |
| `ambiguous_retained_progress` | Retained worktree diff, ownership markers, PR lineage if present, private evidence, and boundary policy. | A human chooses one path: resume same lane, finish manual repair, or reset/discard the retained patch explicitly. |

Before treating retained progress as human-owned, check the current run activity
marker. A `retry_kind` marker means retry scheduling still owns the run. A live
`current_operation=repo_gate` marker means the repository gate still owns the run.
For active phase-goal lanes, inspect the latest private progress and decision
evidence: no blockers or decision request means Decodex should recover the phase goal
and schedule continuation; concrete blockers or decision requests keep the normal
manual-attention path.

For every human-required guardrail stop, keep `decodex:needs-attention` until the
blocker above is resolved. If the issue returns to automation, request a Linear scan
or let the next scheduled scan observe the corrected tracker state; do not bypass the
guardrail with a manual retry that leaves the same evidence unchanged. Do not clear
manual attention for `architecture_recovery_exhausted` until a new accepted recovery
strategy, issue update, or Decision Contract update exists.

For authority-boundary decision requests, the supported resume sequence is:

1. Record the decision in the issue, accepted Decision Contract, or project policy.
2. Clear or preserve `decodex:needs-attention` according to that decision.
3. Requeue or resume through supported Decodex lifecycle controls only after the
   status/evidence tuple still matches the retained lane.

Do not resume by editing raw tracker records, mutating runtime SQLite directly, or
referring to internal graph ids as the operator-facing decision.

## Broad Steer Examples

Broad steer can be delivered by the runtime, but it does not erase lifecycle authority.

Example: an active lane is implementing "add lane-control guidance" and an operator
steers "ignore that and add dashboard retry buttons." The CLI/API may accept the steer
when the run id and expected turn id match. After the turn resolves, an agent must
inspect the diff and evidence. If the issue still has the old objective and the diff
now contains dashboard controls, do not hand off the PR as if the original issue was
satisfied. Preserve the steer audit and either create a replacement issue, update and
requeue the current issue through explicit lifecycle, or route manual attention.

Example: an operator steers "narrow this to docs only; do not touch Rust." If the issue
still accepts that scope and the resulting diff matches the same acceptance criteria,
the lane may continue after inspection. The agent should still cite the steer evidence
and ensure the review handoff summary does not imply unrequested runtime behavior
changed.

## Interrupt And Hard Fallback Examples

Example: `decodex lane interrupt XY-123 --run-id run-abc` reports a soft interrupt
request. Re-run `decodex lane inspect` or `decodex status --json`. If protocol
activity shows the same turn is still stopping, wait or inspect private evidence; do
not kill the child process from the side.

Example: `decodex lane steer XY-123 --run-id run-abc --expected-turn-id turn-1 ...`
or `decodex lane interrupt XY-123 --run-id run-abc` reports
`run_lease_missing` while `decodex lane inspect` still shows the same branch,
attempt, active channel, and live process/protocol state. Do not treat the lane as
cleanup-only and do not clear `decodex:needs-attention`. If the operator needs to stop
the child immediately, retry interrupt with `--force`; otherwise preserve the retained
worktree and use the private evidence readback to decide whether the lane can be
resumed or needs manual attention.

Example: `decodex lane interrupt XY-123 --run-id run-abc --force` reports
`hard_interrupt_fallback`. Inspect the retained worktree before retry. If the worktree
contains a partial patch that still belongs to `XY-123`, resume through
`decodex run XY-123` only when `WORKFLOW.md` eligibility, runtime evidence, branch,
and PR lineage still match. If the patch belongs to a replaced task or the issue state
is unclear, route manual attention.

## Label And Scan Rules

`POST /api/linear-scan` only asks the local listener to refresh Linear-backed intake and
status before the next scheduled poll. It does not start an attempt, retry a failed
lane, or change labels.

Keep `decodex:queued:<service-id>` when the issue is still intended for automation and
the scheduler simply needs to observe a changed state. Remove it only through the
Decodex ops skill when the issue should no longer be an intake candidate. Keep
`decodex:needs-attention` until the recorded blocker is resolved; clearing it is not a
recovery shortcut.

During an owned automation run, agents use issue-scoped tracker tools for progress,
review handoff, manual attention, and terminal finalization. Outside the owned run,
operators use the documented CLI/API controls and label procedures.

## Manual Attention Route

Use manual attention when:

- lane identity cannot be proven from current evidence
- retained work may be overwritten or discarded without a human decision
- broad steer or task replacement changed the issue authority
- hard fallback stopped a process but retained worktree state is unclear
- Linear labels, active ownership, or tracker state conflict with runtime evidence
- PR lineage cannot be validated after review handoff

The valid owned-agent path is:

1. request the configured `decodex:needs-attention` label through `issue_label_add`
2. call `issue_comment` with `kind = "manual_attention"` and structured public fields so Decodex can validate the blocker and apply the label
3. call `issue_terminal_finalize(path = "manual_attention")`

Keep host-local paths, private payloads, raw steer text, process diagnostics, account
details, and secrets out of the public Linear fields.

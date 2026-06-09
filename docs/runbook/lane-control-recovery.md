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
[`./recover-review-handoff.md`](./recover-review-handoff.md), the Decodex `automation`,
`manual-cli`, and `labels` skills, plus the registered project `project.toml` and
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
- `active_lease_missing` rejections together with process, protocol, channel, branch,
  and retained worktree evidence when the lane still appears live
- private evidence and public lifecycle signal
- PR URL, head branch, and head SHA when the lane has crossed review handoff

If these facts do not prove the requested lane, do not steer, interrupt, retry, resume,
or clean labels.

## Decision Tree

| Evidence after inspection | Agent decision | Supported next action |
| --- | --- | --- |
| Active lane still matches the issue, branch, run id, attempt, and turn. | Let the runtime continue or wait for the control result. | No label change. Use the next CLI/API control only when the operator explicitly asks. |
| Soft interrupt was accepted and the runtime is still resolving the attempt. | Wait for status, protocol activity, or evidence to settle. | Re-inspect; do not requeue or force-kill. |
| Soft control was rejected with `active_lease_missing`, but inspect/status still shows the same run id, attempt, branch, active channel, and live child process or protocol activity. | Treat the lane as degraded active execution, not cleanup-only state. | Re-inspect with `decodex lane inspect` or use `decodex lane interrupt <ISSUE> --run-id <RUN_ID> --force` only when the operator explicitly wants hard process fallback. |
| Forced interrupt after `active_lease_missing` reports no signalable process. | Treat force as non-mutating for the child process. | Inspect retained worktree and private evidence; do not claim the interrupt succeeded or clear attention labels. |
| Hard fallback reports `hard_interrupt_fallback`. | Treat it as an interrupted runtime event, not a graceful completion. | Inspect retained worktree and evidence; resume only if lineage is exact. |
| Retained worktree has useful local changes and lineage matches issue, branch, runtime evidence, and PR when present. | Resume or repair the same lane. | Use `decodex run <ISSUE>` when the registered workflow makes it eligible, or use the specific retained recovery runbook. |
| Review handoff marker is missing or stale but the retained PR lane appears recoverable. | Diagnose before rebind. | Run `decodex recover review-handoff diagnose <ISSUE>` and follow [`recover-review-handoff.md`](./recover-review-handoff.md). |
| Queue label or tracker state was changed and the scheduler should observe it before the next poll. | Request a refresh, not a retry. | `POST /api/linear-scan` with `projectId`, or no body for all enabled projects. |
| Queue label should be added, removed, or interpreted. | Use service-scoped label policy. | Follow the `labels` skill; do not guess `<service-id>` or clear `needs-attention` before fixing the blocker. |
| Broad steer materially changes the objective or acceptance contract. | Preserve audit and resolve lifecycle explicitly. | Update and requeue the same issue, create a new issue/lane, or route the owned run to manual attention. |
| Operator wants a different issue or replacement task. | Treat as task replacement, not steer. | Stop or pause through supported controls as needed, then create/update/requeue through the supported lifecycle. |
| Status or Linear failure summary reports a loop guardrail reason. | Stop automatic recovery and inspect the reason-specific evidence. | Follow the loop guardrail recovery table below before clearing `decodex:needs-attention` or requeueing. |
| Evidence is missing, contradictory, or would require guessing whether local work is safe to overwrite. | Stop automatic recovery. | Use manual attention with structured public blockers and keep private evidence local. |

## Loop Guardrail Recovery

Loop guardrails stop non-converging automation after three consecutive matching
observations. They preserve retained worktrees and private evidence; they do not mean
the operator should delete local progress to make the queue clean.

| Guardrail reason | Inspect first | Resume only after |
| --- | --- | --- |
| `validation_repeat` | The repeated validation failure, repo-gate output, retained worktree, and prior repair attempts. | The repair strategy changes, the validation cause is fixed manually, or the issue is routed to architecture/research review. |
| `no_effective_diff` | The retained worktree status, private retry evidence, and whether any useful tracked delta exists. | A human identifies a concrete next diff, commits/resets the retained work intentionally, or updates the issue scope. |
| `remaining_delta_unchanged` | The unchanged tracked delta and latest validation evidence. | The next repair is bounded and materially different, or the retained patch is accepted/reset manually. |
| `review_churn` or `review_policy_exhausted` | Fresh-context review checkpoints, accepted findings, rejected findings, and the current head. | A new repair strategy, architecture review, or manual decision is recorded. |
| `dependency_program_stale` | The open blocker issue, Execution Program readiness, and whether the dependency split is still correct. | The dependency is resolved, the program is refreshed/split, or a research/decision contract updates execution authority. |
| `uncovered_direction` | The missing requirement, decision, or research gap named in public/private evidence. | A research or Decision Contract captures the missing direction and the issue is updated or requeued from that authority. |
| `ambiguous_retained_progress` | Retained worktree diff, ownership markers, PR lineage if present, and private evidence. | A human chooses one path: resume same lane, finish manual repair, or reset/discard the retained patch explicitly. |

For every guardrail stop, keep `decodex:needs-attention` until the blocker above is
resolved. If the issue returns to automation, request a Linear scan or let the next
scheduled scan observe the corrected tracker state; do not bypass the guardrail with a
manual retry that leaves the same evidence unchanged.

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
`active_lease_missing` while `decodex lane inspect` still shows the same branch,
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
labels skill when the issue should no longer be an intake candidate. Keep
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

1. add the configured `decodex:needs-attention` label
2. call `issue_comment` with `kind = "manual_attention"` and structured public fields
3. call `issue_terminal_finalize(path = "manual_attention")`

Keep host-local paths, private payloads, raw steer text, process diagnostics, account
details, and secrets out of the public Linear fields.

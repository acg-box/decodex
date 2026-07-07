---
type: "Spec"
title: "Owned-Lane Policy"
description: "Define the authoritative fallback policy for Decodex-owned lanes, including when automation may continue, wait, retry, resume a retained lane, or must stop for human intervention. Status: normative Read this when: You need the decision matrix for manual intervention, automatic recovery, post-review waiting, retained-lane repair re-entry, or ready-to-land determination. Not this document: The low-level app-server contract, the full runtime state machine, the downstream `WORKFLOW.md` schema, or the operator step-by-step pilot procedure. Defines: The stable decision classes, authoritative signals, human-handoff boundaries, automatic-recovery prerequisites, and the ownership boundary between the runtime model and local workflow implementation."
status: active
authority: normative
owner: runtime
tags: [spec]
last_verified: 2026-06-16
---
# Owned-Lane Policy

Purpose: Define the authoritative fallback policy for Decodex-owned lanes, including when automation may continue, wait, retry, resume a retained lane, or must stop for human intervention.
Status: normative
Read this when: You need the decision matrix for manual intervention, automatic recovery, post-review waiting, retained-lane repair re-entry, or ready-to-land determination.
Not this document: The low-level app-server contract, the full runtime state machine, the downstream `WORKFLOW.md` schema, or the operator step-by-step pilot procedure.
Defines: The stable decision classes, authoritative signals, human-handoff boundaries, automatic-recovery prerequisites, and the ownership boundary between the runtime model and local workflow implementation.

## Design reference

- `openai/symphony` `README.md` and `SPEC.md` are the only external design standard for this policy.
- This policy follows Symphony's stable boundaries:
  - the service is a scheduler and runner, not a general-purpose workflow engine
  - work intake is tracker-backed
  - execution happens in isolated per-issue worktrees
  - workflow policy is repository-owned
  - observability and trust posture are explicit runtime concerns
- Decodex-local extensions such as `WORKFLOW.md [context.read_first]` and the installed workflow skills are execution-context inputs for a compliant implementation. They are not the core domain model of this policy, and they do not replace the primary `WORKFLOW.md` body defined by Symphony.

## Scope

This policy applies to one Decodex-owned lane at a time, from initial claim through review handoff, retained-lane recovery, landing readiness, and final cleanup.

This policy does not require one concrete implementation strategy. A compliant implementation may satisfy the policy through different local workflow tooling as long as the same decisions, signals, and safety boundaries remain true.

## Core principle

The policy is defined in terms of stable runtime decisions and authoritative signals, not in terms of specific local workflow helpers.

The runtime must decide only among these action classes:

- `continue`
- `wait_for_external_signal`
- `retry_automatically`
- `resume_retained_lane`
- `manual_intervention_required`
- `ready_to_land`

The runtime must not invent new action classes at execution time.

Review-request submission, acknowledgement probing, and bounded resend behavior are not separate action classes. They are orchestration behaviors that may happen while the lane remains in `wait_for_external_signal`.

## Authoritative signals

The runtime may use only the following signal groups to decide lane behavior:

- Tracker state:
  - issue workflow state
  - tracker labels
  - blocker state
  - explicit authority comments written during the run
- Retained-lane state:
  - worktree existence
  - guarded markers such as `.decodex-terminal-guarded`
  - activity markers such as `.decodex-run-activity`
  - current attempt and retry-budget bookkeeping
- Review state:
  - whether the recorded PR still matches the lane branch and validated head
  - whether the PR is still open, closed without merge, or merged
  - review approval or requested-change state
  - unresolved review-thread state
  - required-check state
  - mergeability
- Closeout state:
  - whether merge already happened
  - whether authoritative closeout has run
  - whether worktree and branch cleanup are still pending

If these signals disagree and the disagreement cannot be resolved without guessing operator intent, the runtime must choose `manual_intervention_required`.

## Action classes

### `continue`

`continue` means the current automation-owned path is still valid and should keep progressing without pausing for a new external signal.

Examples:

- the live lane is still running and remains eligible
- a merge already happened and the same automation-owned sequence should continue into closeout and cleanup

### `wait_for_external_signal`

`wait_for_external_signal` means the lane is healthy, but the next transition depends on an outside event rather than immediate new agent work.

Examples:

- the lane is in `In Review` and no actionable review feedback exists yet
- required checks are still running
- a review request was sent for the current PR head and the runtime is waiting for review activity or an implementation-defined acknowledgement window to elapse

### `retry_automatically`

`retry_automatically` means the current attempt failed in a retryable way and the runtime still owns the next attempt.

This action requires:

- remaining retry budget
- the issue still being active under retry policy
- no explicit human-attention signal

### `resume_retained_lane`

`resume_retained_lane` means the runtime may safely re-enter an existing retained worktree and continue work on the same owned lane instead of starting a fresh lane.

This action requires:

- the retained worktree still exists
- the retained worktree still matches the owned issue and branch
- the tracker or review signals clearly indicate the same lane should continue
- an explicit retained review-handoff lineage marker is still present when the lane already crossed into post-review ownership
- retained post-review re-entry can still prove the same PR lineage, even when the current
  repair attempt advances to a newer head on that same PR
- retained review-policy checkpoint state in runtime SQLite still matches the current phase when convergence counting must continue across re-entry

### `manual_intervention_required`

`manual_intervention_required` means automation must stop because continuing would require guessing, would violate policy, or would hide an unresolved blocker.

This action is mandatory when:

- retry budget is exhausted
- the agent explicitly requests human attention
- stalled reconciliation found retained tracked worktree changes that require
  operator recovery, or stalled retry budget is exhausted
- terminal signaling is missing or contradictory
- required labels cannot be applied and the runtime must fall back to a guarded non-startable state
- retained worktree, tracker state, or PR state disagree in a way that is not safely self-healing
- multiple retained lanes or worktree candidates would require guessing ownership or a surviving lane
- post-review signals are ambiguous enough that repair, wait, and land are all plausible

### `ready_to_land`

`ready_to_land` means the runtime may proceed into the landing sequence for an owned PR-backed lane.

This action requires:

- the lane already completed PR-backed review handoff
- required approvals are satisfied
- actionable review repair is no longer pending
- required checks are green
- the PR is mergeable under repository policy

`ready_to_land` is a decision boundary, not a guarantee that landing and closeout are already complete.

## Decision table

| Observed condition | Minimum authoritative signals | Required action | Automatic recovery allowed |
| --- | --- | --- | --- |
| Running lane remains eligible and activity is current | Current issue state is still active; no interrupting state transition; activity marker or session state still live | `continue` | Not applicable |
| Clean worker exit with remaining retry budget | Retry budget remains; issue is still active; no `decodex:needs-attention` label or equivalent human-attention signal | `retry_automatically` | Yes |
| Abnormal worker exit with remaining retry budget | Same as above, plus failure is classified as retryable | `retry_automatically` | Yes |
| App-server capability preflight method timeout with remaining retry budget | App-server initialized; capability preflight timed out; workflow retry budget remains; no hard config/model/provider/skills/plugin/MCP blocker was observed | `retry_automatically` | Yes |
| Stalled active run with no retained tracked changes and remaining retry budget | Issue still has active ownership; retry budget remains; worktree has no retained tracked changes requiring operator recovery | `retry_automatically` | Yes |
| Retry exhausted, explicit human-attention signal, or retained stalled partial progress | Retry budget exhausted, or `decodex:needs-attention`, or stalled-run evidence with retained tracked changes | `manual_intervention_required` | No |
| Human-attention label is unavailable but the failure path still must block redispatch | Failure path is human-required; label application failed; guarded retained marker recorded | `manual_intervention_required` | No |
| Retained non-terminal lane still matches issue, branch, and owned recovery intent | Retained worktree exists; issue still belongs to the owned lane; recovery signals are consistent | `resume_retained_lane` | Yes |
| `In Review` lane has no actionable review yet | PR still belongs to lane; no requested changes that require repair; checks or review are still pending | `wait_for_external_signal` | Yes, when new signal arrives |
| `In Review` lane now has actionable review repair work | PR still belongs to lane; actionable review feedback is present; retained lane remains reusable | `resume_retained_lane` | Yes |
| `In Review` lane has green checks, satisfied review, and is mergeable | PR still belongs to lane; approvals satisfied; unresolved blocking review work absent; checks green; mergeable | `ready_to_land` | Yes |
| Pre-PR independent review or post-review repair churn exceeded the configured convergence budget | Runtime-owned review checkpoints show repeated `findings` in the same phase crossed the configured limit; the current repair strategy no longer has a bounded low-risk patch path | `manual_intervention_required`, or `retry_automatically` only when an Architecture Recovery Packet plus Authority Boundary Check authorizes a materially different recovery strategy | Yes, only through the authorized Architecture Recovery Packet path while recovery budget remains. Review-policy churn may continue with `block_landing` while preserving the landing block. |
| Review-policy stop cannot recover autonomously | Runtime-owned review checkpoints identify `needs_architecture_review`, an Authority Boundary Check result outside the lane authority, insufficient recovery evidence, or exhausted recovery budget for the current head, phase, evidence, and stop class | `manual_intervention_required` | No direct retry; future decision follow-up may run only through a separate structured adapter contract |
| Merge already happened but closeout or cleanup is incomplete | Merged PR is authoritative; closeout or cleanup evidence is still missing | `continue` | Yes |
| Signals are contradictory or incomplete in a way that requires guesswork | Tracker, retained lane, review, or cleanup signals disagree materially | `manual_intervention_required` | No |

## Human-intervention boundaries

Automation must stop and require a human when any of these are true:

- the lane needs a decision that cannot be derived from authoritative signals alone
- continuing would rewrite or override evidence the operator should inspect first
- the runtime cannot prove that the retained worktree, PR, and tracker issue still belong to the same owned lane
- a required failure guard could not be applied cleanly
- the lane has already crossed the configured retry limit
- review repair convergence has crossed the architecture recovery boundary and
  recovery is outside authority, insufficiently evidenced, or exhausted

For review-policy churn, the runtime counts only structured review checkpoints:

- phase is `handoff` before the first PR-backed review handoff succeeds
- phase is `repair` during retained review-repair runs
- every `findings` checkpoint must carry at least one accepted independent-review
  finding and increments the repeat count only for the accepted finding fingerprints
  that are still active in the current phase
- a distinct accepted-finding fingerprint starts at repeat count one and does not
  inherit churn from an earlier, now-resolved finding
- `clean` does not stop the lane and resolves active finding records, resetting the
  compatibility `nonclean_rounds` readback to zero for the current phase
- `clean` may carry rejected or non-actionable reviewer comments, but those comments
  are not repair input
- recording `issue_review_handoff` or `issue_review_repair_complete` clears the retained review-policy state

Review-policy stops are also the only review failures eligible for a future
structured decision follow-up path. That path is not an additional action class and
does not by itself clear the current stop decision. `needs_architecture_review` and
`blocked` still enter `manual_intervention_required`, receive the configured
human-attention guard, and stay ineligible until the blocking signal is materially
cleared. Convergence-budget exhaustion for repeated accepted findings first follows
the loop-runtime architecture recovery boundary: a materially different engineering
strategy may continue only when the Authority Boundary Check policy allows autonomous
recovery and recovery budget remains. Review-policy churn may continue with
`block_landing` while preserving the landing block; otherwise the lane enters
`manual_intervention_required`.

Any future decision follow-up must use the same runtime decision class plus a separate
adapter contract. Free-form terminal comments, skill prose, old review memory, or stale
worktree state are not sufficient dispatch inputs. The minimum dispatch evidence is the
current lane head, review-policy head, review phase, normalized stop kind, normalized
error class, latest bounded-review evidence, issue/run identity, and PR URL when the
stop happened during retained repair.

`needs_architecture_review` and convergence-budget exhaustion share the same future
dispatch envelope but not the same question:

- architecture stop asks for an architecture decision before implementation continues
- convergence stop asks whether repeated findings should continue as repair work,
  become a redesign, or stop

`blocked` remains a human-required stop and is not a decision-follow-up input by
default.

Human intervention is not complete merely because a human observed the failure. Human intervention is complete only when the blocking signal is materially cleared.

Retained tracked worktree changes do not by themselves convert a retryable abnormal
exit into `manual_intervention_required`. When issue ownership still matches and
retry budget remains, the next retry must resume the same worktree and treat the
patch as recovery context. Retained partial progress becomes human-required only when
the failure is non-retryable, explicit human attention was requested, retry or
loop-recovery budget is exhausted, stalled reconciliation found retained tracked
changes requiring operator recovery, or authoritative ownership signals are
contradictory enough that continuing would require guessing.

Examples of materially cleared signals:

- `decodex:needs-attention` is removed and the issue is returned to a startable state
- the retained worktree is repaired or explicitly discarded and the runtime can safely choose the next lane action again
- review feedback is resolved or clarified enough that the runtime can classify the lane as wait, repair, or land without guessing

## Automatic-recovery prerequisites

Automatic recovery after a human-required or externally blocked stop is allowed only
when all of the following are true:

- the authoritative blocker was actually cleared, not merely commented on
- the lane still has a valid owned worktree or a clearly recoverable replacement path
- the tracker issue is in the expected active or handoff state for the recovery path
- the recovery action is one of the allowed action classes in this document

Automation must not resume solely because a daemon restarted, a timer elapsed, or a retained worktree still exists on disk.

## Ambiguous-case rules

### Flaky CI

- If required checks are still pending or have a retriable platform failure without new code changes, the runtime may stay in `wait_for_external_signal`.
- If the lane needs a new code change or an operator judgment about the failing signal, the runtime must use `manual_intervention_required` until a later implementation explicitly supports that retry class.

### Review-request lag or missing reaction

- Whether the current PR head needs a fresh review request is an orchestration decision, not a core domain state.
- Sending or re-sending a review request is an implementation action performed by the local workflow adapter for the current environment.
- A review request should be bound to the current PR head and should leave auditable evidence such as a platform-native review-request state change or an attributable request comment for that head.
- If the request was sent successfully and the lane is otherwise healthy, the runtime should remain in `wait_for_external_signal` rather than treating the lack of immediate review activity as a failure.
- If the implementation-defined acknowledgement window elapses with no reliable evidence that the request was accepted for the current head, the runtime may perform a bounded resend for that same head.
- If bounded resend is exhausted, or repeated requests still leave the runtime unable to determine whether review was requested successfully, the runtime must switch to `manual_intervention_required`.

### Dismissed security alerts

- A dismissed security alert is not, by itself, an open repair obligation.
- The runtime should treat the current required-check state and unresolved review state as authoritative, not stale alert shells alone.
- If the alert state and required-check state disagree materially, the runtime must require human intervention instead of guessing.

### Stale retained worktrees

- A retained worktree may be reused only when it still matches the owned issue and branch and there is no contradictory terminal evidence.
- If the retained worktree exists but ownership or branch identity is unclear, the runtime must require human intervention.
- Ordinary text or code conflicts on a clear authoritative lane are not manual-intervention events by themselves. The agent should reconcile inside that authoritative lane, rerun any canonical regeneration path required by the touched files, and then use the repo-native verification path to validate the result.
- If multiple retained lanes or `.worktrees/*` candidates conflict but one authoritative surviving lane is already clear, the agent may reconcile that conflict inside the authoritative lane as ordinary implementation work.
- Manual intervention is reserved for cases where the runtime or agent lacks a safe decision basis, including when multiple retained lanes or `.worktrees/*` candidates could plausibly own the same task and there is no authoritative surviving lane, when in-progress Git state or uncommitted work makes continued reconciliation unsafe, or when correctness depends on product or authorial intent that verification cannot adjudicate.

### Missing labels or partial closeout

- Missing failure labels require a guarded non-startable state and human repair before redispatch.
- Partial closeout keeps the lane automation-owned only when the merge result is already authoritative and the remaining steps are deterministic closeout or cleanup.
- If closeout authority is unclear, the runtime must require human intervention.

## Ownership boundary

This policy is normative about runtime decisions and safety boundaries.

This policy is not normative about the exact local helper names used to satisfy those decisions.

In particular:

- deciding whether a pre-PR independent-review gate is satisfied belongs to orchestration
- deciding whether the current PR head needs a fresh review request belongs to orchestration
- counting pre-PR independent-review churn and post-review repair churn belongs to orchestration
- deciding that the lane has crossed from bounded repair into architecture rethink or manual escalation belongs to orchestration
- executing the concrete review-request side effect belongs to the local workflow adapter
- retrying that side effect after missing acknowledgement is an orchestration policy decision implemented through the same local adapter
- the core runtime model should record only the resulting evidence and current decision class, not a permanent dependency on one helper name, one round counter implementation, or one comment syntax

One compliant local implementation may map the action classes in this document onto:

- retained linked Git worktrees for lane lifecycle
- saved execution plans for implementation authority
- explicit review request and review repair flows
- a separate PR landing step
- a separate closeout step
- a separate cleanup step

That mapping is informative. Future local workflow tooling may change as long as the same decision classes and safety boundaries remain true.

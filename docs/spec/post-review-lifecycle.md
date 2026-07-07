---
type: "Spec"
title: "Post-Review Lifecycle"
description: "Define the normative lifecycle for a Decodex-owned lane after a PR-backed `In Review` handoff, through review follow-up, landing, closeout, and cleanup. Status: normative Read this when: You need the authoritative post-`In Review` state model, transition rules, retry/manual-intervention boundaries, or follow-on implementation split for autonomous review follow-up and landing. Not this document: The low-level app-server protocol, the pre-review runtime handoff contract, the broader owned-lane fallback matrix, or local skill instructions. Defines: Post-`In Review` lane phases, phase-to-action-class mapping, authoritative signals, retry and cancellation rules, ownership boundaries, and the minimum follow-on implementation split."
status: active
authority: normative
owner: runtime
tags: [spec]
code_refs:
  - apps/decodex/src/orchestrator/kernel/post_review.rs
  - apps/decodex/src/orchestrator/kernel/command.rs
  - apps/decodex/src/orchestrator/status/post_review/classification.rs
  - apps/decodex/src/orchestrator/post_review_facts.rs
  - apps/decodex/src/orchestrator/kernel/lifecycle.rs
  - apps/decodex/src/orchestrator/retained_review_orchestration.rs
  - apps/decodex/src/orchestrator/retained_review_orchestration/phases.rs
  - apps/decodex/src/orchestrator/retained_review_orchestration/admin_merge.rs
  - apps/decodex/src/orchestrator/retained_review_orchestration/lifecycle_authority.rs
  - apps/decodex/src/orchestrator/execution_failure/review_handoff_drift.rs
  - apps/decodex/src/orchestrator/dispatch_policy.rs
  - apps/decodex/src/orchestrator/run_cycle_post_review.rs
  - apps/decodex/src/orchestrator/status/post_review.rs
  - apps/decodex/src/pull_request.rs
  - apps/decodex/src/state/review_records/lifecycle.rs
  - apps/decodex/src/state/review_records/lifecycle/authority.rs
drift_watch:
  - decodex/lifecycle-authority-record/1
  - decodex/lifecycle-event/1
  - decodex land --manual-authority --pr
  - review_handoff_state_transition_pending
  - review_lifecycle_records
  - issue_review_handoff
  - issue_terminal_finalize
  - IssueDispatchMode::Normal
  - IssueDispatchMode::Program
  - IssueDispatchMode::Retry
  - IssueDispatchMode::ReviewRepair
  - IssueDispatchMode::Closeout
last_verified: 2026-07-01
---
# Post-Review Lifecycle

Purpose: Define the normative lifecycle for a Decodex-owned lane after a PR-backed `In Review` handoff, through review follow-up, landing, closeout, and cleanup.
Status: normative
Read this when: You need the authoritative post-`In Review` state model, transition rules, retry/manual-intervention boundaries, or follow-on implementation split for autonomous review follow-up and landing.
Not this document: The low-level app-server protocol, the pre-review runtime handoff contract, the broader owned-lane fallback matrix, or local skill instructions.
Defines: Post-`In Review` lane phases, phase-to-action-class mapping, authoritative signals, retry and cancellation rules, ownership boundaries, and the minimum follow-on implementation split.

## Design reference

- `openai/symphony` `README.md` and `SPEC.md` are the only external design standard for this lifecycle.
- This lifecycle keeps Symphony's stable boundaries intact:
  - the service is a scheduler and runner, not a general-purpose workflow engine
  - workflow policy stays repository-owned
  - a successful worker run may end at a workflow-defined handoff state rather than `Done`
  - operator-visible observability and explicit trust posture remain runtime concerns
- Decodex-local extensions such as `WORKFLOW.md [context.read_first]` and the checked-in workflow skills are local execution constraints and adapter surfaces. They are not the core domain model of this lifecycle, and they do not replace the primary `WORKFLOW.md` body defined by Symphony.

## Relationship to other specs

- [`runtime.md`](./runtime.md) defines the current success and failure writeback boundary through PR-backed `In Review` handoff.
- [`linear-execution-ledger.md`](./linear-execution-ledger.md) defines the versioned
  Linear comment event-ledger schema used by post-review handoff, repair, landing,
  closeout, and cleanup records.
- [`owned-lane-policy.md`](./owned-lane-policy.md) defines the allowed action classes and the fallback policy for waiting, repair re-entry, landing readiness, automatic recovery, and manual intervention.
- [`review-orchestration.md`](./review-orchestration.md) defines the runtime-owned
  Decodex Review and GitHub Review loop, strict GitHub Review request and pass
  signals when that level is enabled, round accounting, and the rule that GitHub
  Review pass flows into Decodex-directed admin merge instead of a separate manual
  landing request.
- This document narrows those action classes into the specific post-`In Review` lane phases and transitions that Decodex must honor after review handoff succeeds.

## Core invariants

1. Post-`In Review` work remains part of the same Decodex-owned lane.
2. Post-`In Review` automation must keep using authoritative signals rather than chat memory, branch-name heuristics, or skill-name-specific states.
3. The tracker issue must remain in `In Review` until authoritative closeout transitions it to the workflow-defined `tracker.completed_state`, unless a human explicitly cancels the lane into a terminal tracker state first.
4. A later review-repair attempt must resume the retained lane for the same issue and PR head lineage; it must not silently open a fresh unrelated implementation lane.
5. Landing, closeout, and cleanup are deterministic tail stages of the same owned lane, not separate human-only ceremonies.
6. No phase in this lifecycle may depend on the permanent existence of a particular local helper name such as `review-request`, `review-repair`, `pr-land`, or `closeout`.

## Authoritative signals

Post-`In Review` classification may use only these signal groups:

- Tracker state:
  - issue workflow state
  - labels
  - blocker state
  - public Linear lifecycle comments as team-visible projections only
- Retained-lane state:
  - worktree existence
  - lane markers such as activity and guarded markers
  - current branch, validated head, and retry/churn bookkeeping
- Review state:
  - PR identity and current head
  - whether the PR is still open, closed without merge, or merged
  - review approval or requested-change state
  - unresolved review-thread state
  - required-check state
  - mergeability
- Closeout state:
  - whether merge already happened
  - whether closeout already ran
  - whether deterministic local cleanup remains pending

In the current runtime, the retained lane persists one `review_lifecycle_records` row
in the Decodex runtime database as the projection of a canonical lifecycle authority
record. Final lifecycle states are decided by the pure lifecycle kernel and written
only by the runtime state adapter. Linear execution ledger comments, tracker comments,
manual closeout receipts, and recovery audits are public projections or execution logs;
they must not answer whether a lane is finally `landed`, `closed`, or cleanup-final.

The authority record schema is `decodex/lifecycle-authority-record/1`. Its append-only
event envelope schema is `decodex/lifecycle-event/1`. The persisted record covers:
`schema_version`, `project_id`/`service_id`, `issue_id`, `subject_id`, `sequence`,
`phase`, `transition`, `previous_state`, `next_state`, `next_action`, `review_level`,
`review_gate_state`, `pr_url`, `base_branch`, `head_branch`, `validated_head_sha`,
`worktree_path`, `merge_commit`, `cleanup_state`, `authority`, `actor`,
`source_evidence_refs`, `idempotency_key`, `correlation_id`, `causation_id`, and
`decided_at`.

The lifecycle kernel is a pure decision function: it receives normalized facts and
evidence classification, then returns a `LifecycleDecision` plus authority-record
envelope. It must not read or write `StateStore`, SQLite, tracker, GitHub, Linear, or
worktree state. Runtime adapters collect facts, perform side effects such as GitHub
merge or tracker closeout, submit intent/readback evidence to the kernel, and
transactionally persist the authority projection plus lifecycle event.

When that exact lifecycle authority projection is missing, post-review ownership must
block as unresolved instead of rebinding from branch-name, current-head-only
heuristics, or Linear comments.
Historical `review_handoffs` and `review_orchestrations` tables are dropped during
runtime bootstrap without copying their rows, and must not be used as readback
authority. Operators must use explicit diagnose, rebind, or adopt recovery to create or
refresh a current lifecycle record.

Manual development remains supported. Humans may edit, test, commit, open PRs, run
issue-authority `decodex land`, and run explicit recovery. Issue-authority landing and
already-merged recovery still submit landing and closeout evidence through the
lifecycle kernel before the runtime projection becomes final. The non-issue
`decodex land --manual-authority --pr <URL>` path is the local receipt exception: it
does not require project registry or issue closeout and therefore does not create an
issue-authority final lifecycle record.

If these signals disagree and the disagreement cannot be resolved without guessing operator intent, the runtime must use `manual_intervention_required`.

## Explicit handoff recovery

`missing_review_handoff_record` is a fail-closed post-review state. The scheduler must
not infer a PR lineage from branch names, current heads, PR titles, or Linear comments,
and `decodex run` must not repair this state automatically.

If operator status sees private `review_completion_intent` plus
`issue_terminal_finalize(path = "review_handoff")` but no matching
`review_lifecycle_records` row, it must expose a deterministic pending writeback
reason such as `review_handoff_writeback_missing_lifecycle_authority`. That readback is
only a fail-closed recovery contract: recovery may proceed automatically only when the
exact private intent, PR URL, retained branch, local `HEAD`, and PR head still match.
Otherwise operators must use explicit diagnose, rebind, or adopt recovery.

Failure writeback must also respect this post-review boundary. If an execution failure
arrives after a retained review lifecycle record already binds the current issue,
branch, PR, and local HEAD lineage, Decodex may self-heal state drift by rebinding the
phase fields in that lifecycle record, clearing loop guardrail checkpoints for the
issue, and moving the tracker issue back to `tracker.success_state` when the issue had
drifted to `tracker.in_progress_state` or `tracker.failure_state`. This is not
implementation repair and must happen before retry/no-diff loop guardrails run. If the
retained lifecycle record is absent, unverified, or diverged, Decodex must stop with
`review_handoff_state_drift` or the existing `missing_review_handoff_record` posture
and require explicit recovery evidence rather than guessing a PR lineage.

Once a retained worktree and matching review lifecycle record already bind the issue
and branch, that lifecycle evidence is stronger than a queued/startable tracker state
or an ordinary retry signal. `Normal`, `Program`, and `Retry` dispatch must fail closed
for that issue while the tracker state is still startable or
`tracker.in_progress_state`, and operator intake must surface the blocked reason
`review_handoff_state_transition_pending`. Review repair dispatch, explicit
`review-handoff rebind` or `adopt`, post-review orchestration, landing, and closeout
remain the only owners of progress after the lifecycle record exists.

When the retained review lifecycle record exists but a direct PR-state read or local
worktree branch/head read fails, operator status must degrade the readback instead of
replacing the bound lane with a null-PR blocked state. The status row must keep the
issue identifier, retained branch, lifecycle PR URL, and lifecycle head SHA, and it may
expose warnings such as `readback_warning = "pull_request_state_read_failed"`,
`worktree_checkout_branch_read_failed`, or `worktree_head_read_failed` until the next
successful readback.
Retained orchestration must preserve degraded readback as a wait state in the
lifecycle record. It must not
convert `pull_request_state_read_failed`, `worktree_checkout_branch_read_failed`,
`worktree_head_read_failed`, or other `WaitForReview` classifications into passive
manual attention; only classifications whose decision is `Block` may add
`decodex:needs-attention`.

When Linear issue metadata readback is degraded by connector backoff, operator status
must still keep locally retained lifecycle rows visible with the lifecycle PR URL and
head SHA and must mark the row as tracker-readback degraded instead of presenting the
PR or code state as failed. If the lifecycle record is bound but
`decodex:active:<service-id>` is missing, recovery diagnosis must classify the state
as ownership drift. A bound lane already in `tracker.success_state` may ask the
operator to confirm or restore ownership before continuing the existing lifecycle. An
already-current same-PR same-head lane that drifted back to `tracker.in_progress_state`
or `tracker.failure_state` must instead point to the explicit
`review-handoff rebind --dry-run` path, because that path can validate the retained
worktree and PR lineage before restoring the active service label and completing the
issue-state transition.

The supported operator recovery surface is `decodex recover review-handoff`. This is a
break-glass recovery path for orphaned retained review lanes and stale lifecycle
heads after explicit manual repair or rebase. It is not part of the normal automation
success path.

- `diagnose` is read-only. It reports the project, issue, branch, worktree, local head,
  active automation label, existing PR URL when present, lifecycle handoff head,
  lifecycle phase head, PR base/head when readable, and the missing or mismatched
  lifecycle reason. A diagnostic may report a bound lifecycle record, active ownership
  drift, a missing lifecycle record, a pending issue-state transition, an unverified PR
  read, or a concrete field mismatch that requires explicit rebind.
- `adopt` is mutating and requires an explicit issue identifier plus PR URL. It is the
  supported manual takeover path for a human-owned PR that was created outside a
  runtime-retained lane but should now enter Decodex's normal retained review/landing
  lifecycle. It must run from the current managed lane worktree under the configured
  `worktree_root`, validate active automation ownership, reject opt-out and
  needs-attention stops, require the issue to be in `tracker.in_progress_state` or
  already in `tracker.success_state`, require a clean current checkout, require the
  current branch and `HEAD` to match the PR head branch and SHA, and require the PR to
  be open, non-draft, mergeable, green, free of pending review requests, and free of
  unresolved review threads. It may reuse an existing worktree mapping only when that
  mapping points at the current managed checkout; it must reject mappings to a
  different checkout and must reject any existing review lifecycle record for the
  current or previously mapped branch. Adopt rewrites the mapping to the current PR
  branch only after validation succeeds. Those already-bound lifecycle lanes belong to `rebind`
  or normal landing.
- `rebind` is mutating and requires an explicit issue identifier plus PR URL. It must
  validate the configured project, tracker issue state, active automation ownership,
  retained worktree branch, clean worktree, PR repository, PR base, PR head branch, PR
  head SHA, and open non-draft PR state before writing the lifecycle record.
  Existing-record refresh
  requires the workflow `tracker.success_state`. Partial normal handoff recovery may
  also accept the workflow `tracker.in_progress_state` when the lifecycle record is
  missing, or when an already-current lifecycle record exists but the issue state was
  not advanced, and the validated PR plus retained worktree prove the handoff lineage.
  If stale failure writeback already moved that already-current lifecycle lane back to
  `tracker.failure_state`, removed `decodex:active:<service-id>`, or applied
  `tracker.needs_attention_label`, explicit rebind may restore the active service
  label, clear needs-attention, and move the issue to `tracker.success_state` after
  the rebind audit succeeds. Decodex must reject this failure-state recovery for
  missing or stale lifecycle records because those still need explicit PR-lineage
  repair before tracker state can be trusted.
- If no review lifecycle record exists, `rebind` restores the missing lifecycle record
  from the validated PR and retained worktree. If a record already exists for the same
  branch and PR but its stored handoff head or phase head is stale, `rebind` may
  refresh that record to the validated PR head. It must reject an existing record for a
  different PR, and it must reject a current same-branch same-PR record as a no-op
  unless the issue is still in `tracker.in_progress_state` or `tracker.failure_state`
  and only the active-label repair, needs-attention repair, or success-state
  transition remains.
- A successful adopt writes a runtime worktree mapping for the current managed checkout,
  creates a local run attempt identity for the takeover, writes the same runtime DB
  review lifecycle record as normal `issue_review_handoff` needs,
  records a `review_handoff_adopt` audit event, and may move the issue from
  `tracker.in_progress_state` to `tracker.success_state` after the audit succeeds. It
  does not land the PR, queue follow-up work, or clear needs-attention. A subsequent
  `decodex land --authority <ISSUE> --pr <URL>` owns merge, tracker closeout, and
  cleanup through the normal issue-authority path.
- Explicit non-issue landing with `decodex land --manual-authority --pr <URL>` is not
  a project-registry operation. When no `--config` is provided, it derives repository
  context from the current Git checkout, uses `GH_TOKEN`, `GITHUB_TOKEN`, or
  `gh auth token`, keeps the normal PR/base/head/check gates, writes only the local
  manual land receipt, and skips runtime/Linear closeout. Passing `--config` may supply
  GitHub credentials and workspace hooks, but pure manual-authority landing must not
  refresh project registry state unless issue closeout is actually in scope.
- A successful rebind writes the same runtime DB review lifecycle record as normal
  `issue_review_handoff` needs, records a `review_handoff_rebind` audit event, and
  records whether active ownership or needs-attention labels were repaired. It does
  not land the PR, queue follow-up work, or substitute for healthy lanes' normal
  `issue_review_handoff` plus `issue_terminal_finalize(path = "review_handoff")`
  path. If either lifecycle marker write fails, the command must clear any partial
  handoff record before reporting failure. If any audit write fails after lifecycle
  record creation, the command must clear the new record and roll back any active
  service label restored by that rebind before reporting failure.
- Once a rebind or equivalent current lifecycle record exists for a retained lane, stale
  passive failure handling for the earlier `missing_review_handoff_record` observation
  must not move the issue back to the failure state or add `decodex:needs-attention`.
  The next scheduler/status pass must reclassify from the current lifecycle record
  instead of applying the obsolete missing-record writeback.

`cleanup_only` rows are outside this rebind surface. When operator status reports a
cleanup-only worktree with `provenance_source = "legacy_unknown"`, Decodex has only an
old local mapping and cannot prove PR or closeout lineage from the runtime store. The
operator path is: verify the tracker issue and PR terminal state, inspect the retained
checkout for local-only changes, run
`decodex recover legacy-closeout <ISSUE> --pr <MERGED_PR> --dry-run`, rerun with
`--manual-authority` only after validation passes, and only then remove the worktree.
That fallback must stay rarer than normal closeout, explicit rebind, or deterministic
legacy reconstruction from authoritative lifecycle records. Runtime recovery may
classify a retained worktree as `runtime_recovered` only after tracker, retained
lifecycle record, or closeout evidence proves a current owner; it must not silently
upgrade a terminal cleanup-only `legacy_unknown` row.

## Phase model

The post-`In Review` lifecycle is expressed in lane phases. These phases refine, but do not replace, the owned-lane action classes.

```mermaid
stateDiagram-v2
    [*] --> review_wait: review_handoff accepted
    review_wait --> review_repair: actionable review feedback
    review_repair --> review_wait: repaired head pushed\nfresh review requested
    review_wait --> ready_to_land: approvals + checks + mergeability satisfied
    ready_to_land --> review_wait: review or checks regress
    ready_to_land --> review_repair: actionable repair reappears
    ready_to_land --> landing: runtime starts clean-path merge
    ready_to_land --> review_repair: merge stops being a deterministic clean path
    landing --> closeout: merge authoritative
    closeout --> cleanup: tracker closeout complete
    cleanup --> [*]: lane state clean
```

At any phase, contradictory state or a non-self-healing merge failure must stop the lane in `manual_intervention_required` instead of guessing a next step. Exhausted repair or convergence budget first follows the owning review or loop guardrail policy: only an in-envelope Architecture Recovery Packet may continue autonomously; otherwise the lane stops as human-required.

| Lane phase | Required action class | Entry conditions | Exit conditions |
| --- | --- | --- | --- |
| `review_wait` | `wait_for_external_signal` | PR-backed `In Review` handoff succeeded for the current owned lane | Actionable review repair appears, landing becomes ready, human intervention becomes required, or cancellation is explicit |
| `review_repair` | `resume_retained_lane` | Actionable review feedback exists and the retained lane still belongs to the same issue and PR lineage | A new repaired head is pushed and review is re-requested for that head, human intervention becomes required, or cancellation is explicit |
| `ready_to_land` | `ready_to_land` | Required approvals are satisfied, blocking review work is absent, checks are green, the branch is up to date with base, the PR is cleanly mergeable, no unresolved Authority Boundary `requires_human_decision` or authority decision request remains, and no unresolved `requires_enhanced_evidence` or `block_landing` policy remains for the current head | Clean-path landing begins, signals fall back to wait or repair, or human intervention becomes required |
| `landing` | `continue` | The runtime has committed to executing the clean merge for the current lane | Merge is recorded, landing fails into a resumable deterministic tail step, or human intervention becomes required |
| `closeout` | `continue` | Merge already happened for the lane's authoritative anchor and tracker closeout has not yet completed | Tracker closeout succeeds, the lane blocks on contradictory closeout state, or human intervention becomes required |
| `cleanup` | `continue` | Either (a) merge and closeout are authoritative and only worktree or branch cleanup remains, or (b) explicit pre-merge cancellation is authoritative and only deterministic retained-lane cleanup remains | The retained worktree and lane branch state are clean, or cleanup blocks on conflicting local evidence |

`manual_intervention_required` is not a normal progress phase. It is the mandatory stop outcome whenever the owned-lane policy says automation must stop.

## Phase semantics

### `review_wait`

This is the default healthy state immediately after PR-backed review handoff.

While in `review_wait`:

- the tracker issue remains in `In Review`
- the retained lane remains reserved for the same issue and PR lineage
- missing immediate review activity is not, by itself, a failure
- review-request acknowledgement probing or bounded resend may happen as orchestration behavior without leaving `review_wait`
- before requesting external review, pending or unknown check readback waits for a
  later status tick, and red checks route to `review_repair`; neither case may apply
  `decodex:needs-attention` by itself

`review_wait` must not trigger code changes on its own.

### `review_repair`

`review_repair` means the runtime has enough authoritative evidence to re-enter the retained lane and address review feedback.

While in `review_repair`:

- the runtime must reuse the retained lane when it is still valid
- repair work must stay bound to the same issue, branch lineage, and PR
- the runtime must validate each GitHub Review claim against the codebase, tests, and requirements before changing code
- when a fresh-context runtime-owned `issue_review_checkpoint` artifact exists for
  the repair phase, repair work must operate on accepted findings from that
  checkpoint; rejected or non-actionable comments remain evidence, not repair scope
- the repaired head must pass the local pre-review gate before being pushed
- when `[codex].review` is `"standard"` or `"strict"`, the runtime records every
  repaired-head bounded Decodex Review result through `issue_review_checkpoint`
  after retained repair completion
- every addressed review thread must receive an in-thread reply for the repaired head
- only threads whose landed fix is verified on that repaired head may be resolved; pushback or clarification threads stay open
- once a new head is pushed and fresh review is requested on the same PR, the lane returns to `review_wait` for that new head

If the issue also uses `execution-state`, that overlay remains only durable execution memory inside the retained repair run. It may record task-local runtime progress for the same issue through `issue_progress_checkpoint`, but it does not decide lane phase transitions such as `review_wait`, `review_repair`, `ready_to_land`, or `closeout`.

In the current XY-174 slice, a retained repair run finishes by recording an explicit
`issue_review_repair_complete` action for the same PR URL, then finalizing the run with
the `review_repair` terminal path. After the local repository gate passes, Decodex
must push the validated local `HEAD` to the retained PR branch before applying that
completion. Push auth, refspec, and remote-rejection failures are structured
retained-review-repair push failures; they must stop before refreshing the retained
handoff row. After a successful push, applying completion must re-read the PR and
verify that the remote PR head matches the validated local `HEAD`. Only then may it
refresh the local runtime handoff row to the repaired PR head while keeping the
tracker issue in `In Review`; it does not re-run the original
`issue_review_handoff` state transition.
When `[codex].review` is `"standard"` or `"strict"`,
`issue_review_repair_complete` records the pushed repaired-head fact and then the
runtime-owned repair review gate must record a latest retained repair checkpoint that
is `clean` for the current repaired head before the lane may proceed on the clean
path. If repeated repair checkpoints stay in `findings` for three consecutive rounds,
the runtime must stop the current patch-on-patch strategy and run the architecture
recovery boundary check before either retrying with a materially different
implementation strategy or routing to human intervention. If the checkpoint reports
`needs_architecture_review` / `blocked`, the runtime must stop for human
intervention. When `[codex].review` is `"off"`, retained repair completion skips that
Decodex Review checkpoint requirement but still requires the repaired head to be
pushed and the configured repository validation gate to pass.
During the narrow interval after `issue_review_repair_complete` and
`issue_terminal_finalize(path = "review_repair")` are recorded but before the retained
`review_lifecycle_records` row has been refreshed to the repaired PR head, operator
status must treat the exact private completion intent plus current-head clean repair
checkpoint artifact as transitional writeback evidence. That transition may surface
`review_repair_writeback_missing_lifecycle_authority` or
`review_repair_writeback_stale_lifecycle_authority`, but it must not revive stale repair
findings, request a duplicate review checkpoint, or classify the repaired lane as a
new ordinary implementation run. The transition is valid only when the issue, branch,
run attempt, PR URL, PR head ref, PR head OID, local `HEAD`, and clean repair
checkpoint artifact all match.
Every retained repair completion, regardless of review level, also requires the latest
private `issue_progress_checkpoint` for the current run attempt to include parseable
`docs_impact` and a `head_sha` matching the repaired lane `HEAD`; `review_repair`
terminal finalization is invalid without that current-head docs-impact checkpoint.
The same completion also requires that the PR still belongs to the retained lane, points
at the repaired lane HEAD, and remains open and ready for fresh review.

If the retained lane is missing or no longer provably belongs to the same issue and PR lineage, the runtime must not invent a fresh lane silently. It must require human intervention or a separately defined recovery path.

Targeted issue dispatch must use the same post-review ownership classification as the
ordinary scheduler. When `decodex run <ISSUE>` resolves to a retained lane currently
classified as `needs_review_repair`, the runtime should enter retained review repair for
that issue rather than treating the request as ordinary queued intake or generic retry.
This targeted path is gated by the status-visible retained lane classification; an
ordinary `In Review` issue without a `needs_review_repair` post-review lane is not
promoted into repair only because it is service-owned. If the status-visible retained
repair lane belongs to a different issue than the targeted identifier, the runtime must
fail closed with a targeted retained repair mismatch instead of borrowing that lane or
falling through to generic retry.

### `ready_to_land`

`ready_to_land` is a decision boundary, not a merge event.

The runtime may classify the lane as `ready_to_land` only when the native clean-path merge is
already deterministic:

- the PR still belongs to the owned lane
- required approvals are satisfied
- actionable blocking review work is absent
- required checks are green
- the branch is already up to date with the base branch
- mergeability is affirmative and `mergeStateStatus` is `CLEAN`
- any earlier Authority Boundary `requires_human_decision` or authority decision
  request has been resolved through explicit issue, Decision Contract, or supported
  policy authority
- any earlier Authority Boundary `requires_enhanced_evidence` or `block_landing`
  policy has been cleared by a clean review checkpoint for the current lane head

If any of those signals becomes false again before landing starts, the lane must return to
`review_wait` or `review_repair` instead of forcing a merge. Cases that are not deterministic
clean tail work, including branch sync, conflict resolution, ambiguous mergeability,
repository-specific recovery, pre-receive-hook ambiguity, or red/unstable check interpretation,
must use the retained agent path before another runtime merge attempt.

### `landing`

`landing` begins only after `ready_to_land` was true and the runtime committed to the merge step.

While in `landing`:

- the runtime executes the repo-approved merge path
- merge policy for retained review landing is the fixed Decodex policy: require green checks, require an up-to-date base branch, preserve commit-level history, use merge commits, and never squash or rebase
- direct runtime execution is limited to the clean path; if the merge would require
  implementation-shaped work, the runtime re-enters the retained lane agent path instead of asking
  the agent to author the merge side effect

If merge succeeds, the lane progresses to `closeout`. If merge does not succeed and the cause is not self-healing, the runtime must require human intervention rather than guessing whether to retry.

### `closeout`

`closeout` begins after merge is authoritative and the tracker issue still needs the final completed-state transition or deterministic retained-lane tail work remains.

While in `closeout`:

- the merge anchor is authoritative
- operator-visible `pull_request_merged_closeout_pending` requires the PR merge state
  readback to agree with the retained handoff head. If one readback reports a merged PR
  while the direct PR merge readback still reports an open or mismatched head, Decodex
  must not dispatch closeout and must surface a contradictory readback blocker instead
  of guessing.
- tracker closeout is Linear-only in this slice
- the tracker issue transitions from `In Review` to the resolved `tracker.completed_state`
- the configured local repo-root default branch fast-forwards to the authoritative landed default-branch head before deterministic tail work is considered complete
- the retained lane may continue even after the issue is already in the resolved `tracker.completed_state` when deterministic closeout tail work or cleanup eligibility still remains pending
- a merged PR remains eligible for deterministic closeout when the local lane HEAD is the exact PR head, the GitHub merge commit, or a later local HEAD that contains the GitHub merge commit; that landed lineage must not be downgraded into generic manual attention while closeout or cleanup is still deterministic
- when deterministic closeout follows the same successful review handoff without any failed or interrupted closeout retry, the closeout record, run summary, and local run ledger reuse the review handoff `run_id` and `attempt_number`; later real closeout retries keep incrementing attempt numbers
- `issue_closeout_complete` and `closeout` terminal finalization require the latest
  private `issue_progress_checkpoint` for the current run attempt to include parseable
  `docs_impact` and a `head_sha` matching the current lane `HEAD`; review or closeout
  records do not substitute for that docs-impact checkpoint
- an issue that reaches the resolved `tracker.completed_state` before the PR is actually merged is contradictory state and must block automation instead of being treated as ready-to-land
- for manual land closeout, once merge and tracker closeout are authoritative, cleanup treats already-absent transient Decodex labels (`decodex:active:<service-id>`, `decodex:queued:<service-id>`, and the configured needs-attention label) as idempotent; this does not relax the requirement that active ownership must be present before landing starts
- for explicit `decodex land --manual-authority --pr <URL>` reruns from the repo-root default branch, recovery is allowed only after GitHub reports the PR as `MERGED`, the local default branch is current with `origin/<default>`, that default branch contains the PR merge commit, the landed lane branch/worktree cleanup is already complete, and no merged worktree cleanup debt remains; unmerged PRs still require the normal managed-lane readiness path

Successful post-merge closeout requires a machine-readable resolved `tracker.completed_state`. Repository workflow policy resolves that target either from the explicit `tracker.completed_state` field or, if that field is omitted, from an exact `"Done"` terminal-state default. Because `closeout` now validates this field during retained post-review completion, the runtime must stop for `manual_intervention_required` instead of guessing a post-merge tracker target whenever workflow policy cannot resolve a valid completed state.

If merge is authoritative but closeout fails due to a deterministic infrastructure problem with no contradictory state, the runtime may resume `closeout` later within the same owned lane. A terminal tracker state written during `closeout` does not by itself authorize worktree deletion while deterministic cleanup is still pending. If state is contradictory, the runtime must stop for human intervention.

### `cleanup`

`cleanup` is the final deterministic tail stage. It removes retained worktree and lane branch state only after one of these terminal ownership conditions is authoritative:

- successful landing-and-closeout: merge and closeout are already authoritative
- explicit pre-merge terminal cancellation: the tracker issue already reached a terminal cancellation state before merge, and no contradictory retained-lane evidence remains

`cleanup` must not begin while:

- review work is still pending for a lane whose owned progress has not been explicitly canceled
- neither successful landing-and-closeout nor explicit pre-merge terminal cancellation has ended owned-lane progress
- successful landing-and-closeout is the governing path but merge is not yet authoritative
- successful landing-and-closeout is the governing path and closeout is incomplete
- successful landing-and-closeout is the governing path and the local repo-root default branch is still behind the authoritative landed default-branch head

## Transition rules

1. `review_handoff` success in the runtime spec enters `review_wait`.
2. `review_wait -> review_repair` when authoritative review feedback requires a code change and the retained lane is still reusable.
3. `review_repair -> review_wait` after a repaired head is pushed and review is requested for that exact head.
4. `review_wait -> ready_to_land` when approvals, checks, and mergeability all satisfy repository policy for the current lane head.
5. `ready_to_land -> review_wait` when approvals, checks, or mergeability regress before merge starts and no actionable code change is required.
6. `ready_to_land -> review_repair` when actionable review repair reappears before merge starts.
7. `ready_to_land -> landing` when the runtime begins the merge step.
8. `landing -> closeout` when merge is authoritative for the lane's anchor.
9. `closeout -> cleanup` when the tracker closeout succeeds and only deterministic local cleanup remains.
10. `review_wait`, `review_repair`, or `ready_to_land` may transition directly to `cleanup` when the tracker issue reaches a terminal cancellation state before merge and only deterministic local cleanup remains.
11. `cleanup -> finished` when the retained worktree and lane branch state are clean.

At any phase, contradictory signals force `manual_intervention_required`. Exhausted
retry budgets also force the human-attention path. Exhausted repair/convergence
budgets first follow the owning review or loop guardrail policy: engineering strategy
changes may continue only through autonomous architecture recovery inside the
Authority Envelope; otherwise the lane becomes human-required.

## Failure, retry, and cancellation rules

### Review-request lag

- A missing immediate review response is not a failure by itself.
- The runtime may perform bounded resend for the same verified head if the implementation-defined acknowledgement window expires without reliable review-request evidence.
- Exhausting bounded resend without reliable request evidence forces `manual_intervention_required`.

### Review-repair failures

- Transport or runtime interruptions during `review_repair` may resume the same retained lane when the owned-lane policy still permits `resume_retained_lane`.
- Watch-level child failures in `review_repair` and deterministic post-review tail stages consume the same `execution.max_attempts` retry budget as normal issue execution. Once that budget is exhausted, the runtime must write the human-attention failure state, remove active automation ownership, and block the lane from further post-review redispatch.
- Operator status must report an exhausted retained post-review lane as `blocked` with a retry-budget reason instead of continuing to classify it as `needs_review_repair` or `ready_to_land`.
- If the lane's PR is already merged, exhausted local closeout, default-branch sync, or cleanup retries must be reported as `closeout_blocked` or `cleanup_blocked` with the retained PR URL from the runtime handoff row.
- Structural churn is not a generic retry case. If repair rounds exceed the configured convergence budget, the runtime must stop the current repair strategy and either enter architecture recovery under the Authority Envelope or require human intervention rather than patching indefinitely.
- Three consecutive non-clean fresh-context review rounds in the same phase are a
  review-churn guardrail. Public writeback may use `review_policy_exhausted` or the
  normalized loop reason `review_churn`, but the recovery rule is the same: inspect the
  repeated findings for the exact current head, the Architecture Recovery Packet, and
  the Authority Boundary Check before choosing a new repair strategy, architecture
  review, or manual resolution before requeueing.
- A repair batch that changes the head must return to `review_wait` for that new head instead of continuing downstream on stale review state.

### Landing and closeout failures

- `landing`, `closeout`, and `cleanup` are deterministic tail stages.
- If their authoritative preconditions are still satisfied, the runtime may resume the same stage later without reopening implementation.
- If merge, tracker state, or worktree ownership becomes contradictory, the runtime must stop for human intervention.

### Cancellation

Cancellation is not a separate owned-lane action class. It is an authoritative external outcome.

Examples:

- the issue is moved to `Canceled` or `Duplicate`
- the PR is closed without merge and the tracker issue is moved to `Canceled` or `Duplicate`

When cancellation is authoritative:

- the runtime must stop autonomous review follow-up and landing
- `review_wait`, `review_repair`, and `ready_to_land` may transition directly to `cleanup` only if the tracker issue is already terminal and no contradictory retained-lane evidence remains
- if the PR closes without merge but the tracker issue remains non-terminal or is redirected back to active workflow, the runtime must retain the worktree and stop rather than deleting recovery state
- `landing` and `closeout` must not reinterpret an already-authoritative merge as cancellation; contradictory post-merge cancellation signals require `manual_intervention_required`
- the runtime must not reopen or reinterpret the lane automatically

## Ownership boundaries

### Orchestrator owns

- phase classification for the current lane
- review-request acknowledgement budgets and resend thresholds
- repair convergence budgets
- deciding when a lane is `ready_to_land`
- deciding when contradictory state requires `manual_intervention_required`
- deciding whether a deterministic tail stage may resume automatically

### Local workflow adapters own

- emitting the concrete review-request side effect
- executing the retained review-repair side effects inside the lane, including in-thread replies, conditional thread resolution, and fresh review request emission
- executing the repo-approved land step
- executing Linear tracker closeout and deterministic cleanup eligibility checks
- executing worktree and branch cleanup

### Tracker and GitHub own

- tracker issue workflow state and labels
- PR review state
- required-check state
- mergeability and merge result

The runtime must record the resulting evidence and current phase, but must not elevate current helper names into stable domain states.

## Minimum follow-on implementation split

The accepted post-`In Review` lifecycle mapped onto the follow-on implementation issues like this:

- `XY-173`: detect owned PR review state and classify `review_wait`, `review_repair`, or `ready_to_land`
- `XY-174`: re-enter retained lanes for `review_repair`
- `XY-175`: implement `landing`, `closeout`, and `cleanup`
- `XY-177`: align checked-in workflow skills with the accepted lifecycle once the runtime model is stable

`XY-173` through `XY-175` now exist to explain the implementation history for the current runtime; `XY-177` remains the follow-on skill-alignment task. This document remains the authoritative lifecycle contract.

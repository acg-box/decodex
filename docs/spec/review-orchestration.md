---
type: "Spec"
title: "Review Orchestration"
description: "Define the normative review orchestration contract that sits above runtime-native review handoff, retained review repair, and landing. Status: normative Read this when: You are implementing or reviewing how a Decodex-owned lane requests Decodex Review or GitHub Review, counts review rounds, reacts to review results, or transitions from review pass into handoff or landing. Not this document: The low-level app-server protocol, the post-`In Review` lane phase model, the tracker tool schema, or local skill payloads. Defines: Shared review-loop semantics, reviewer-source-specific rules, strict GitHub Review adapter signals, review-round accounting, architecture-escalation rules, landing entry requirements, and manual-intervention boundaries."
status: active
authority: normative
owner: runtime
tags: [spec]
code_refs: [apps/decodex/src/agent/tracker_tool_bridge/review.rs, apps/decodex/src/agent/tracker_tool_bridge/tools.rs, apps/decodex/src/autonomy_signal.rs, apps/decodex/src/state/store.rs, apps/decodex/src/state/internal.rs]
drift_watch: [issue_review_checkpoint, issue_review_handoff, issue_review_repair_complete, review_contract, review_cost_control, review_policy_checkpoints, finding_routes, evidence_artifacts, authority_boundary_check, decodex.autonomy_signal/1]
last_verified: 2026-07-07
---
# Review Orchestration

Purpose: Define the normative review orchestration contract that sits above runtime-native review handoff, retained review repair, and landing.
Status: normative
Read this when: You are implementing or reviewing how a Decodex-owned lane requests Decodex Review or GitHub Review, counts review rounds, reacts to review results, or transitions from review pass into handoff or landing.
Not this document: The low-level app-server protocol, the post-`In Review` lane phase model, the tracker tool schema, or local skill payloads.
Defines: Shared review-loop semantics, reviewer-source-specific rules, strict GitHub Review adapter signals, review-round accounting, architecture-escalation rules, landing entry requirements, and manual-intervention boundaries.

## Implementation note

This document defines the target orchestration contract for review behavior. The current runtime already owns review handoff, retained review repair, landing, closeout, and cleanup, but not every reviewer-source-specific adapter rule in this document is implemented yet. Until implementation catches up, treat this spec as the governing target when changing review automation.

## Relationship to other specs

- [`runtime.md`](./runtime.md) defines the runtime success and failure writeback boundary through PR-backed `In Review` handoff.
- [`loop-runtime.md`](./loop-runtime.md) defines the higher-level loop contract for
  phase-scoped goals, independent fresh-context review, uncovered direction stops, and
  loop guardrails.
- [`post-review-lifecycle.md`](./post-review-lifecycle.md) defines the post-`In Review` lane phases and downstream ownership after review handoff succeeds.
- [`tracker-tools.md`](./tracker-tools.md) defines the issue-scoped tracker tool surface that records bounded review results and completion signals.
- The registered project `WORKFLOW.md` defines the repo-native bounded review method
  that each review pass must use when evaluating the current lane head. Review
  instructions use the registered workflow policy already injected by Decodex; agents
  must not reinterpret `WORKFLOW.md` as a required repo-root file unless it was
  explicitly listed in `context.read_first`.

## Core model

There is one shared review loop for Decodex-owned lanes:

1. request review
2. receive review
3. validate and repair the actionable findings for the current lane head
4. request review again until the lane passes or stops for escalation

Review behavior is selected per service through the registered project config
`[codex].review`. The supported levels are `"off"`, `"standard"`, and `"strict"`.

The levels map to review sources as follows:

- `off`: no review gate.
- `standard`: Decodex requires the runtime-owned independent fresh-context
  `issue_review_checkpoint` artifact gate after PR-backed handoff or retained
  repair completion. The checkpoint writer is not exposed to the implementing
  agent.
- `strict`: Standard plus GitHub Review. After Decodex Review and PR-backed
  handoff, Decodex uses the runtime-owned GitHub `@codex review` path where the
  existing adapter supports it.

When the configured level does not include GitHub Review, Decodex skips the
runtime-owned `@codex review` request and may land directly from the retained lane
once the normal landing gates are satisfied and the PR is already on the
deterministic clean merge path.

## Shared invariants

1. Review always applies to a clean committed current lane head, not an older
   remembered implementation state or a dirty worktree snapshot.
2. While a review request is outstanding, the lane itself must not push unrelated new commits.
3. If PR state, branch lineage, or retained-lane ownership changes externally while review is pending, the lane must stop for `manual_intervention_required` instead of trying to recover automatically.
4. Decodex Review and GitHub Review rounds are counted independently.
5. Review pass is fail-closed. If the expected pass signals are missing, contradictory, or ambiguous, the lane must stop for manual intervention instead of guessing success.

## Shared repair rules

After any review arrives:

- validate each actionable claim against the codebase, tests, requirements, and current lane head
- repair only the verified issues
- keep the repair batch scoped to the smallest coherent owned change set
- rerun the repository validation required for the current head before the next review request
- when `[codex].review` is `"standard"` or `"strict"`, the runtime records the
  normalized Decodex Review result for the exact clean committed current `HEAD`
  through the `issue_review_checkpoint` artifact path, including the explicit
  independent reviewer source, review contract, reviewed head/tree binding,
  checklist notes, accepted findings, rejected findings, non-empty evidence,
  repair guidance, and `finding_routes` adjudication for every reviewer signal
- before a repair loop uses any review signal, route it through `finding_routes`:
  accepted current repair work must be bound to an accepted finding and routed as
  `current_blocker`; non-current or non-repair signals must use their matching
  durable evidence route and must not become repair input
- before any handoff, retained repair completion, or terminal finalization, record the
  separate current-head docs-impact checkpoint through `issue_progress_checkpoint`;
  review checkpoints do not satisfy the `docs_impact` requirement

The current repository's bounded review method is defined in the registered project
`WORKFLOW.md`. This spec does not replace that method; it defines how review
requests and review outcomes are orchestrated around it. Each Decodex Review
checkpoint must include an explicit `review_contract` whose
`workflow_policy_source` is `registered_project_workflow`. The contract scopes the
review objective, changed surface, non-goals, validation evidence, risk tier, required
checks, and allowed expansion triggers. When review reveals missing direction rather
than a repairable finding, route the gap through an external investigation workflow
and accepted Decision Contract update before execution continues.

## Review round accounting

A review round is:

1. request review
2. receive review
3. validate review signals, route them, and repair accepted `current_blocker`
   findings
4. request review again

Rules:

- A resend caused by missing acknowledgement is a retry of the current request, not a new review round.
- A review round does not complete until the lane either requests the next review or stops for escalation.
- Each `findings` checkpoint with an active `current_blocker` fingerprint consumes
  the normal convergence budget for the current review phase.
- The third repeat of the same active `current_blocker` fingerprint for the same
  phase stops the current repair strategy as review churn. Further autonomous work
  requires an Architecture Recovery Packet plus Authority Boundary Check for the
  current lane head.
- Recovery may continue only when the Authority Boundary Check policy allows autonomous recovery and recovery budget remains. Review churn uses `block_landing` to preserve the landing block while automatic implementation recovery continues; otherwise the lane stops for `manual_intervention_required`.
- There is no fourth-result reset path; `clean` is the only review result that clears the non-clean round count.

## Review levels

Review level is service-controlled.

Rules:

- `[codex].review = "off"` skips Decodex Review and GitHub Review.
  Decodex does not expose `issue_review_checkpoint`, does not require a clean
  checkpoint before handoff or repair completion, and ignores stale review-policy
  checkpoint state for turn-stop classification.
- `[codex].review = "standard"` uses the runtime-owned independent fresh-context
  read-only Decodex Review checkpoint loop. The implementing agent records
  `issue_review_handoff` or `issue_review_repair_complete` after pushing the
  validated PR head; Decodex then requests review, records a clean committed
  current-HEAD `clean` evidence artifact before landing or continuing the retained
  repair lifecycle, re-checks that review-blocking local changes are absent before
  using that artifact, stores structured accepted/rejected finding evidence plus
  `finding_routes`, and applies the review-policy stop rules to missing, stale, or
  non-clean keyed artifact state.
  That review checkpoint is separate from the current-head `issue_progress_checkpoint`
  with `docs_impact` required before terminal finalization. It does not use GitHub
  Review.
- `[codex].review = "strict"` uses the standard requirements and then participates
  in the GitHub Review loop.
- Omitted `[codex].review` defaults to `"strict"`.
- `basic` is not a supported review level. Historical prompt-only self-review must
  migrate to either `"off"` when no review gate is desired or `"standard"` when the
  harness should require independent runtime-owned review.
- In `"standard"` and `"strict"` levels, the runtime may choose the exact local transport or
  child-conversation mechanism, but it must remain a fully runtime-controlled
  read-only review request. The reviewer must not edit files, push, land, or mutate
  tracker state.
- In `"standard"` and `"strict"` levels, Decodex Review must use the same bounded review method and normalized review outcomes as any other review pass.
- In `"standard"` and `"strict"` levels, the initial runtime-owned Decodex Review uses
  `review_type = "full_current_head_review"`. Retained repair Decodex Review uses
  `review_type = "repair_verification"` and is limited to accepted findings routed as
  `current_blocker` from the previous review plus contract regressions. New unrelated
  comments are rejected or non-current route candidates unless they match an allowed
  expansion trigger such as safety, authority-boundary, data-loss, security,
  live-mutation, public-API, migration, or operator-facing regression.
- In `"standard"` and `"strict"` levels, each checkpoint records
  `review_cost_control` beside the review contract. The default class is
  `full_current_head_review`. A compact class is cost control only, not review
  skipping: the reviewer remains independent and fresh-context, the checkpoint still
  binds the committed current `HEAD`, and the reviewer must still perform both the
  intended-behavior and adversarial checks from the registered workflow policy.
  Compact review is valid only for a low-risk, small, validation-backed, clean
  post-handoff lane with current-head evidence, validation evidence that is current
  for the reviewed `HEAD`, sufficient current-head evidence quality, no high-risk
  surfaces, no accepted findings, no blocking routes, and no prior non-clean
  review-policy state. Full review is forced for repair verification, accepted
  findings, non-clean rounds, missing or stale validation, weak evidence,
  architecture risk, high-risk changed surfaces, or docs/config/API/security/data/
  privacy changes without sufficient evidence. The classification combines
  structured signals with reviewer judgment; changed-surface count alone is never
  sufficient.
- In `"standard"` and `"strict"` levels, a Decodex Review checkpoint is persisted as
  an evidence-keyed artifact. The key must include artifact kind
  `issue_review_checkpoint`, review phase, current `HEAD`, review level, and review
  prompt version. A later attempt may reuse that artifact only when every key
  dimension still matches; post-review classification, retained orchestration, and
  mutation-fence checks read this artifact rather than the run-local projection. A
  new `HEAD`, changed review level, or changed prompt version invalidates the proof.
- In `"standard"` and `"strict"` levels, a `findings` checkpoint requires at least
  one accepted finding routed as `current_blocker`; rejected, non-current, or
  non-actionable reviewer comments may be recorded with a `clean` checkpoint and must
  not become repair input.
- If Decodex Review returns an ambiguous or contradictory result that the runtime
  cannot classify without guessing, stop for `manual_intervention_required`.
- Decodex Review pass after PR-backed handoff transitions into the normal
  post-review wait, repair, or landing flow, not directly into landing without the
  ordinary PR gates.
- Local or prompt-only self-review is not a Decodex review gate. Use the Decodex
  Review checkpoint boundary before treating the lane as ready for landing whenever
  independent review is required.

## Decodex Review signal routing

`issue_review_checkpoint` separates reviewer disposition from repair scheduling. The
runtime-owned review adapter decides whether a signal is accepted, rejected, needs
more evidence, belongs to follow-up, exposes a risk or reviewer-rubric gap, or
identifies architecture, issue-contract, landing, or authority blockers. That
judgment must be serialized as `finding_routes` before the runtime lets any signal
enter a repair loop.

The route taxonomy is:

- `current_blocker`: accepted finding that is repair input for the current lane head
- `landing_blocker`: validated signal that blocks handoff or landing but is not a
  normal current repair item
- `contract_or_authority_decision_required`: repair would cross the accepted
  Decision Contract or lane authority
- `needs_evidence`: the signal needs stronger source evidence before it can be
  accepted or rejected
- `follow_up`: valid non-current follow-up work
- `deterministic_gate_candidate`: candidate validation/gate hardening rather than
  immediate implementation repair
- `architecture_signal`: signal that may require a materially different strategy or
  architecture recovery
- `issue_contract_gap`: the issue contract is missing required direction
- `reviewer_rubric_gap`: rejected or non-actionable reviewer expectation mismatch
- `risk_note`: durable risk evidence that does not require current repair
- `invalid_or_unsubstantiated`: reviewed and rejected as unsupported; high-severity
  or high-risk signals must use `needs_evidence` or a landing-blocking route instead

Only accepted findings routed as `current_blocker` populate the active review-policy
fingerprint ledger, `repair_accepted_review_findings`, `nonclean_rounds`, and review
churn repeat counting. Non-current routes remain durable evidence in the checkpoint,
private evidence, and operator readback without driving repair churn. `current_blocker`
must bind to an accepted finding by `finding_source = "accepted_findings"` and
zero-based `finding_index`; out-of-range accepted or rejected bindings are rejected.
A missing `finding_routes` field keeps backward compatibility by defaulting accepted
findings to `current_blocker` and rejected findings to `reviewer_rubric_gap`, but new
prompts must instruct agents to populate explicit route evidence so non-current
signals do not accidentally become repair input.

Autonomy review-feedback signals consume only this normalized route evidence plus
current-head checkpoint references. Raw reviewer comments, GitHub thread text, or
unrouted review summaries cannot be persisted as `review_feedback_cluster` signals
unless they are first represented through `finding_routes` and bound to the reviewed
head.

## GitHub Review

GitHub Review is adapter-driven and uses strict observable GitHub signals.

### Request and acknowledgement

The GitHub Review request is made by posting `@codex review` on the current PR.

The request is accepted only when that exact request comment receives an `eyes` reaction from the `codex` reviewer actor.

Before every GitHub Review request, the current PR head must already have green required CI or
required checks.

Rules:

- If required CI or required checks are still pending, do not post a review request yet. Stay in
  the retained lane and wait for green.
- If required CI is red in a retained-repair class the runtime already knows how to handle, return
  to retained repair first and request GitHub Review only after the repaired head becomes green.
- If required CI is red in a way the runtime cannot classify or repair without guessing, stop for
  `manual_intervention_required` instead of posting the review request anyway.
- If no `eyes` reaction appears within one minute, resend the GitHub Review request exactly once.
- That resend is a retry of the same request, not a new review round.
- Treat the lane as having only one outstanding GitHub Review request at a time.
- If the resent request still does not receive `eyes` within one minute, stop for `manual_intervention_required`.
- Once the `eyes` signal is observed, poll until GitHub Review arrives or manual intervention becomes required.

### Read surface

GitHub Review processing must read all relevant GitHub review surfaces, not only inline threads:

- review summaries and overall review body
- inline review comments and review threads
- unresolved review threads
- formal review decision state such as approval or requested changes
- adapter-level signals used by this spec such as `eyes`, the strict pass phrase, and PR-description reactions

### Strict pass signal

GitHub Review passes only when both of these exact signals are present:

- review content authored by the `codex` reviewer actor is exactly the standalone text `Didn't find any major issues.` after trimming surrounding whitespace
- the PR description currently has a `thumbs-up` reaction from the `codex` reviewer actor

No fuzzy fallback, semantic-equivalence check, or alternative wording is allowed. If those signals do not appear exactly, GitHub Review does not pass automatically and the lane must stop for `manual_intervention_required`.

## GitHub Review repair and thread resolution

After GitHub Review returns findings:

- validate the findings against the current lane head before changing code
- repair only the verified issues
- keep pushback or clarification threads open until the repaired head is ready

A review thread may be resolved only when all of these are true:

1. the fix landed on the current repaired head
2. the fix was verified on that repaired head
3. an in-thread reply was posted for the addressed comment or thread

Pushback or clarification threads stay open.

## Transition from review pass to landing

The next step after review pass depends on reviewer source.

### After Decodex Review pass

- continue the normal PR-backed review handoff flow
- create or refresh the non-draft PR for the current lane head if needed
- when `[codex].review` is `"standard"` or `"strict"`, record `issue_review_handoff`
  only after the latest bounded-review result for that handoff phase and current
  `HEAD` is `clean`
- when `[codex].review` is `"off"`, record `issue_review_handoff`
  after the branch is pushed, the non-draft PR is ready, and required validation has
  passed
- if `[codex].review` is not `"strict"`, treat that PR-backed handoff as sufficient
  review input for retained landing and do not post `@codex review`
- after a successful non-strict handoff, the same top-level `decodex run` may keep
  draining the same retained lane through retained landing, closeout, and
  deterministic cleanup until the lane reaches a stable waiting state or finishes
  that tail work
- direct runtime merge is limited to the clean path; if branch sync, conflict resolution, ambiguous mergeability, or repository-specific recovery is still required, re-enter the retained agent path first
- if retained checks are still pending or merge visibility is not yet authoritative, stop the same run cleanly at that waiting boundary instead of busy-waiting indefinitely

### After GitHub Review pass

- execute the same PR's GitHub admin merge directly only on the deterministic clean path
- do not require a separate human merge step first when the clean-path preconditions are already satisfied
- fall back to the retained agent path when landing still requires branch sync, conflict resolution, ambiguous mergeability handling, repository-specific recovery, or any other implementation-shaped work

Before starting landing, require all of these:

- the PR is open
- the PR is non-draft
- the current lane head is the validated reviewed head
- required checks are green
- the PR branch is up to date with base
- the repository merge method preserves commit-level history and supports merge commits
- no unresolved Authority Boundary `requires_human_decision`,
  `requires_enhanced_evidence`, or `block_landing` policy remains for the current
  lane head, and no unresolved `authority_decision_request` remains; operator status
  readback and runtime admin-merge preflight must read the same authority-boundary
  landing requirement
- runtime calls GitHub admin merge explicitly; do not use auto-merge and do not fall back to rebase or squash merge

If the repository does not support merge commits, stop for `manual_intervention_required` instead of improvising another merge path.

## Merge visibility timeout

After the admin merge call returns success:

- poll for authoritative merged-PR visibility
- use a default polling ceiling of 15 minutes unless repository workflow policy overrides it
- if the PR is still not merged after that ceiling, stop for `manual_intervention_required`

## Post-merge tail behavior

After merge becomes authoritative:

- continue into retained closeout
- continue into deterministic cleanup
- synchronize local `main` as part of deterministic operator tail work
- update the tracker issue to the configured completed state through the existing closeout flow

Exact ownership boundaries for closeout and cleanup remain governed by [`post-review-lifecycle.md`](./post-review-lifecycle.md).

## Manual-intervention triggers

The lane must stop for `manual_intervention_required` when any of these occur:

- GitHub PR, branch, or lineage changes while review is pending
- Decodex Review returns an ambiguous result
- GitHub Review acknowledgement never appears within the allowed resend budget
- GitHub Review pass signals do not match the strict required pair exactly
- admin merge is unsupported for the repository
- merged PR visibility does not arrive within the configured polling ceiling
- Authority Boundary Check or architecture recovery outcome concludes repeated review churn is outside the lane authority, insufficiently evidenced, externally blocked, or recovery budget exhausted

## Review-Stop Decision Follow-Up

Review-policy stops may become inputs to an external decision follow-up, but the stop
itself does not dispatch investigation work automatically.

Current required behavior:

- `needs_architecture_review` and `blocked` terminate through
  `manual_intervention_required`.
- Convergence-budget exhaustion for repeated active `current_blocker` fingerprints is
  normalized as `review_churn`. It stops the current repair strategy and may continue
  only through autonomous architecture recovery when the Authority Boundary Check
  policy allows autonomous recovery and recovery budget remains. Review churn uses
  `block_landing` to preserve the landing block while recovery continues; otherwise
  it terminates through `manual_intervention_required`.
- The terminal failure path must preserve the normalized review-stop class instead of
  collapsing it into a generic retry failure:
  - `architecture_review_required`
  - `review_policy_exhausted` or the loop-guardrail projection `review_churn`
  - `review_policy_blocked`
- A terminal failure comment may point the operator toward a bounded external
  follow-up, but that guidance is not a dispatch signal.

Future decision follow-up integration is allowed only after a separate implementation
defines a machine-checkable adapter contract. That contract must require all of these
inputs before handoff:

- service id, issue id, issue identifier, run id, attempt number, branch, and
  repository-relative worktree path
- current lane `HEAD` SHA and the review-policy `head_sha`, which must match
- review phase: `handoff` or `repair`
- normalized stop kind:
  - `architecture_stop` for `needs_architecture_review`
  - `convergence_stop` for repeated `findings` exhaustion
- normalized error class: `architecture_review_required` or
  `review_policy_exhausted`
- non-clean round count for `convergence_stop`
- concise evidence from the latest bounded review pass, including the validated
  route summary, current-blocker fingerprints, validated findings, or architecture
  concern
- PR URL when the stop happens during retained review repair
- explicit question, non-goals, and expected decision shape

Authority boundary:

- Runtime policy owns stop classification, evidence validation, lifecycle blocking, and
  any future decision to dispatch an escalation request.
- Repository policy owns the bounded review method, repo gate commands, and local
  convergence thresholds when those thresholds are externalized.
- An external team workflow or adapter owns the investigation method and produced artifact. It
  must not clear `decodex:needs-attention`, move the issue, resume the implementation
  lane, edit the implementation worktree, or decide that the original lane is ready for
  review.

Architecture stops and convergence stops share the same future adapter envelope, but
they remain distinct escalation kinds. `architecture_stop` asks for an architectural
decision. `convergence_stop` asks whether repeated validated findings need another
repair slice, a redesign, or manual cancellation. `blocked` is not a decision
follow-up kind unless a later human or runtime classifier converts the blocker into a
structured architecture or convergence stop.

## Examples

### Valid external pass

1. Post `@codex review` on the current PR.
2. Observe `eyes` on that exact comment within one minute.
3. Wait for the GitHub Review result.
4. Confirm the review content is exactly `Didn't find any major issues.` after trimming surrounding whitespace.
5. Confirm the PR description currently has a `thumbs-up` from the `codex` reviewer actor.
6. Run admin merge after landing gates are satisfied.

### Fail-closed external ambiguity

1. Post `@codex review`.
2. Observe `eyes`.
3. Receive review text that says `No major issues found.` instead of the exact required phrase.
4. Even if no actionable threads exist, do not treat that as pass.
5. Stop for `manual_intervention_required`.

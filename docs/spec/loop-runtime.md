# Loop Runtime Specification

Purpose: Define the natural-language-first Decodex loop-runtime contract that sits
above individual issue lanes.
Status: normative
Read this when: You are implementing or reviewing Decodex-native research,
decision promotion, internal execution planning, phase-scoped Codex goals, unattended
loop behavior, or loop guardrails.
Not this document: The issue-lane state machine, low-level `app-server` protocol,
post-`In Review` phases, operator lane-control commands, or the concrete research
method.
Defines: The user surface, Research/Decision stage, latent Loop/Decision Contract,
internal Execution Program, promotion boundary, phase-scoped goal rules, validation
and review boundary, unattended execution behavior, loop stop conditions, and harness
improvement loop.

## Scope

Decodex is a natural-language-first loop runtime. The everyday user surface is Codex
conversation, not a graph editor, DAG command set, issue-batch command language, or
Codex goal-control surface.

Ordinary user intents are conversational:

- `research X` starts a research/design pass and may produce a latent execution plan.
- `arrange this`, `push this forward`, or equivalent natural-language follow-up
  promotes accepted decisions into executable work.
- Users do not need to manipulate graph ids, DAG commands, dry-run/apply/status
  mechanics, queue internals, or Codex goal internals for ordinary Decodex use.

This document defines the target loop-runtime contract. Current lower-level runtime
behavior remains governed by [`runtime.md`](./runtime.md), [`lane-control.md`](./lane-control.md),
[`post-review-lifecycle.md`](./post-review-lifecycle.md), and
[`review-orchestration.md`](./review-orchestration.md).

## Authority Model

The loop runtime has three authority layers:

| Layer | Authority |
| --- | --- |
| Conversation | Natural-language user intent, acceptance, rejection, and promotion decisions. |
| Loop runtime | Research/Decision records, accepted Loop/Decision Contracts, internal Execution Programs, ready-node selection, drift handling, stop attribution, and harness telemetry. |
| Lane runtime | Normal Decodex issue lanes, app-server attempts, validation gates, review handoff, retained repair, landing, closeout, and cleanup. |

Research output is latent until accepted or promoted. A research artifact, plan draft,
or proposed issue split must not by itself enqueue work, create authoritative
dependencies, set goals, mutate tracker state, or start implementation.

After acceptance or promotion, the accepted Loop/Decision Contract, shortened to
Decision Contract in this spec, becomes loop-runtime authority. The runtime may then
shape or update normal Linear issues and queue intent, but executable work still runs
through the lane runtime contract.

## Research/Decision Stage

Decodex should own a native Research/Decision stage for Decodex work. That stage may
eventually replace the external research skill for Decodex planning, but the current
external `docs/research/` artifact lane remains supporting evidence only until a
Decodex-native adapter is implemented.

A Research/Decision stage may produce a latent Loop/Decision Contract with:

- objective and objective lineage
- evidence, constraints, assumptions, and rejected alternatives
- proposed decisions and open direction questions
- non-goals and scope boundaries
- acceptance criteria and validation expectations
- dependency and blocker model
- conflict domains such as `docs`, `runtime`, `site`, `tests`, or a more specific
  repository-owned domain
- proposed issue split and queue intent
- risk notes that decide whether independent review is required

The latent contract is a candidate decision package. It becomes authoritative only
after the user or an accepted runtime policy promotes it.

## Promotion Boundary

Promotion is the boundary between design and execution authority.

Promotion requires one of these accepted signals:

- explicit user acceptance in conversation
- a natural-language follow-up that clearly asks Decodex to arrange, queue, or push
  the accepted work forward
- a future runtime-owned policy that is itself backed by an already accepted Decision
  Contract

Promotion must preserve what was accepted. If the runtime discovers that the research
artifact contains unresolved direction, contradictory requirements, or missing
acceptance criteria, it must request more decision authority instead of starting
execution.

Promotion may create or update normal Linear issues, dependencies, labels, or queue
intent. It must not expose an ordinary user workflow that depends on graph ids,
manual DAG edge editing, or hidden Codex goal state.

## Internal Execution Program

An Execution Program is internal loop-runtime state derived from accepted Decision
Contracts. It may use DAG semantics, but the graph is backstage state rather than the
user-facing workflow.

Each program node should carry:

- objective lineage back to the accepted Decision Contract
- executable stage such as `decision`, `issue_shaping`, `queued`, `running`,
  `validation_repair`, `review_wait`, `review_repair`, `landing`, `closeout`,
  `blocked`, or `done`
- dependencies and blocker references
- conflict domain
- acceptance criteria and validation gates
- queue intent and service id
- ready-node selection reason
- drift status against the accepted contract
- linked Linear issue identity when the node becomes executable

Normal Linear issues remain the executable Decodex lanes. A program node may become
eligible only by creating or updating a normal issue with enough natural-language
briefing for generic dispatch and by applying the configured queue policy. The
Execution Program does not replace Linear as the team-visible backlog or the runtime
lane model.

Ready-node selection is runtime-owned. It should choose nodes whose dependencies are
done, whose conflict domains are available, whose acceptance criteria are concrete,
and whose queue intent is accepted. If those facts are missing or stale, the node is
not ready.

## Drift Handling

The loop runtime must compare active execution against the accepted Decision Contract.
Drift includes:

- an issue whose scope no longer matches the accepted node objective
- a dependency or conflict-domain change that changes execution order
- new evidence that invalidates a settled decision
- implementation needs that require a direction decision not present in the contract
- review or validation findings that imply the accepted architecture is wrong

Small implementation discoveries may update local execution evidence when they do not
change accepted direction. Direction drift must pause the affected node and request a
research-contract or architecture decision. Dependent nodes must wait. Independent
ready nodes may continue.

## Phase-Scoped Codex Goals

Codex goals used by Decodex lanes must be phase-scoped. Do not set one giant goal such
as "finish the issue" for an entire lane.

Supported goal scopes are examples of the required shape:

| Goal phase | Meaning |
| --- | --- |
| `implement_to_validation_ready` | Produce the smallest coherent implementation or docs change that is ready for the repo gate. |
| `repair_validation_failures` | Fix concrete canonicalize or verify failures and rerun the same gate. |
| `repair_accepted_review_findings` | Repair validated review findings for the current head without widening scope. |
| `handoff_evidence` | Prepare the PR-backed handoff, evidence summary, and terminal tracker signal after validation and review are satisfied. |

Goal completion is a trigger for the next validation or review step. It is not proof
that the lane is complete, reviewed, merged, landed, or closed. Lane completion still
requires the deterministic validation, review, PR handoff, manual-attention, landing,
closeout, and terminal-finalization contracts owned by the lower-level specs.

## Validation And Review

Self-review is a cheap smoke check. It can catch obvious mistakes, missing edits, or
local reasoning gaps, but it is not sufficient completion evidence by itself.

Completion needs deterministic validation. For repository lanes, the registered
project `WORKFLOW.md` defines the canonicalize and verify commands, and
[`runtime.md`](./runtime.md) defines how repo-gate failures are classified.

When risk warrants review beyond self-review, use an independent fresh-context
read-only review pass. This pass is distinct from in-thread self-review:

- it reads the current `HEAD`, diff, requirements, and relevant specs from scratch
- it does not rely on the implementer's memory of the change
- it stays read-only while producing findings
- it checks intended behavior, regression risk, tests, docs/config drift, migration
  fallout, and operator-facing fallout
- candidate findings must be validated before repair work changes the lane

The review orchestration contract, including internal/external review modes and
review-stop classes, is defined by [`review-orchestration.md`](./review-orchestration.md).

## Unattended Execution

Long unattended execution requires settled direction before execution starts. The
runtime should not rely on an agent to invent product or architecture direction while
draining a queue.

If execution discovers uncovered direction:

1. Pause the affected node or branch.
2. Preserve the concrete question, evidence, and blocked acceptance criterion.
3. Route the node to a research-contract or architecture-review stop.
4. Continue other ready nodes whose dependencies and conflict domains are unaffected.

The affected lane should use `manual_attention` or the later accepted loop-runtime
stop surface when it cannot safely continue from the current contract. It must not
silently broaden scope, rewrite the accepted contract, or treat an unaccepted research
idea as execution authority.

## Loop Guardrails

The loop runtime must stop bounded churn instead of patching indefinitely.

Stop conditions include:

- three repeated failures with the same validation command and materially same root
  cause after attempted repair
- three consecutive attempts that produce no effective diff, no new validation
  evidence, and no new decision evidence
- review repair churn that reaches the stop rules in
  [`review-orchestration.md`](./review-orchestration.md)
- repeated dependency blockers where the blocked node cannot make progress and the
  dependency state is not changing
- uncovered contract questions that affect accepted direction or acceptance criteria
- contradictory tracker, PR, branch, or runtime ownership evidence that cannot be
  resolved without guessing

Stop attribution must preserve the reason instead of collapsing failures into a
generic retry bucket. Normalized outcomes include:

| Outcome | Use when |
| --- | --- |
| `validation_failure_repeated` | The same validation class repeats after bounded repair. |
| `no_effective_diff` | Repeated attempts do not change the head, evidence, or decision state. |
| `review_policy_exhausted` | Review findings exceed the accepted repair convergence budget. |
| `architecture_review_required` | The lane needs architecture direction before more repair. |
| `review_policy_blocked` | Review cannot proceed from available evidence. |
| `dependency_blocked` | The node is waiting on dependency state that is not progressing. |
| `research_contract_required` | Execution uncovered a missing or contradictory decision contract. |
| `ownership_ambiguous` | Tracker, PR, branch, or runtime ownership evidence is contradictory. |

These outcomes should route to failure attribution, research-contract feedback,
architecture review, or manual attention. They must not spin in automatic retries.

## Harness Improvement Loop

Loop outcomes are training signals for the Decodex harness. Runtime telemetry,
private execution evidence, review-stop reasons, validation failures, no-effective-diff
attempts, dependency blockers, and accepted contract gaps should feed improvements to:

- prompts and developer instructions
- Decodex skills and plugin guidance
- validators and repo gates
- issue templates and briefing quality
- ready-node selection and conflict-domain policy
- future loop guardrails

Harness improvement does not retroactively change a lane's accepted Decision Contract.
It also does not authorize automatic execution from latent research output. Apply
future policy changes only after the relevant spec, decision, or project contract is
updated.

## Non-Goals

- Do not add a user-visible DAG command surface for ordinary Decodex use.
- Do not turn research into automatic execution without an accepted promotion
  boundary.
- Do not expose Codex goal internals as an operator workflow.
- Do not replace normal Linear issues as executable Decodex lanes.
- Do not use lane steer as hidden task replacement.
- Do not implement plugin UX in this spec; plugin and UI work must follow downstream
  implementation issues.

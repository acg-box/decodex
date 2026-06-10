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

Decodex owns a native Research/Decision compiler for Decodex work. That stage accepts
natural-language intent such as `research X` plus bounded research/design evidence
when available, then stores a local Decision Contract candidate. It supersedes the
external research skill for Decodex runtime authority: the old external
`docs/research/` artifact lane remains supporting evidence and inspiration for method,
but it is not the authority surface for Decodex loop state.

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

Native research/design compiler outcomes are:

| Outcome | Contract status | Meaning |
| --- | --- | --- |
| `decision_ready` | `draft_latent` | Bounded evidence, option comparison, assumptions, objections, and proposed issue-readiness data are sufficient for downstream issue shaping after promotion. It is still not execution authority while latent. |
| `not_decision_ready` | `draft_latent` | The run preserved useful evidence or objections, but missing evidence or unresolved direction means it must not become implementation work. |
| `blocked` | `draft_latent` | The run cannot finish its bounded research/design pass because a non-decision blocker must be resolved first. |
| `needs_human_decision` | `needs_human_decision` | The package needs explicit human direction before promotion or execution can be considered. |

The compiler may fold AI-owned subwork into the main contract as provenance and
evidence, but the main contract remains coherent and the user does not choose
subagents, graph ids, lanes, or goal commands.

## Decision Contract Schema

The runtime-facing Decision Contract payload is versioned as
`decodex.decision_contract/1` with `record_version = 1`.

The payload carries these top-level fields:

| Field | Meaning |
| --- | --- |
| `contract_id` | Stable runtime identifier for this decision package. |
| `status` | One of `draft_latent`, `accepted_promoted`, `rejected_superseded`, or `needs_human_decision`. |
| `source_intent` | Natural-language source intent, including the original utterance or issue reference when known. |
| `research_provenance` | Research/design sources used to produce the candidate package. |
| `research_evidence` | Non-authoritative evidence claims retained for later review and issue shaping. |
| `research_options` | Non-authoritative option comparisons retained with tradeoffs, selected decision notes, or rejected-option reasons. |
| `accepted_authority` | Objectives, non-goals, constraints, assumptions, objections, and stop conditions that become authority only when status is `accepted_promoted`. |
| `execution_readiness` | Natural-language readiness summary, missing decisions, validation expectations, risk notes, proposed issue summaries, conflict domains, and queue intent. It must not expose graph ids or require the user to operate a DAG. Accepted contracts must be ready for issue shaping and must not carry unresolved missing decisions. |
| `promotion` | Metadata recording who or what accepted the decision, the acceptance source, and the acceptance time. Required only for `accepted_promoted`. |
| `links` | Generated Linear issue ids/identifiers or internal Execution Program node ids when those exist. |
| `evidence_boundary` | Local private evidence references and sparse public projection references. |

The status is the authority boundary:

- `draft_latent` means the research/design result is stored but cannot enqueue,
  mutate tracker state, set goals, or authorize implementation.
- `accepted_promoted` means the payload's `accepted_authority` fields may be used by
  the loop runtime to shape queue intent, generated issues, or internal Execution
  Program nodes. The payload must include promotion metadata, set
  `execution_readiness.ready_for_issue_shaping = true`, and leave
  `execution_readiness.missing_decisions` empty.
- `rejected_superseded` means the payload is retained for audit/history but must not
  be promoted later.
- `needs_human_decision` means the package is incomplete or contradictory enough that
  execution must wait for more direction. The payload must include at least one
  `execution_readiness.missing_decisions` entry.

Research provenance and research evidence are not execution authority. They explain
why the candidate package exists and give future agents enough context to avoid asking
the user to restate all details after promotion.

The runtime stores Decision Contracts in local SQLite first. Linear issue descriptions,
Linear execution-ledger comments, generated issue text, and operator summaries may link
to or summarize an accepted contract, but they are public/coarse mirrors and must not
become the source of truth for private loop state.

## Authority Envelope

The Authority Envelope is the loop-runtime boundary that decides what an autonomous
recovery attempt may change without asking for human direction. It is derived from the
accepted Decision Contract, project `WORKFLOW.md`, registered project policy, current
issue briefing, and explicit user direction or steering that is still within the same
accepted objective.

The core rule is:

> Decodex may autonomously change how engineering is implemented, but it must not
> silently change what was authorized.

Within authority:

- internal refactors, schema plumbing, tests, or docs needed to satisfy the same
  accepted objective and acceptance criteria
- replacement of one implementation strategy with another when public behavior,
  validation strength, project policy, and user direction remain unchanged
- additional private evidence, diagnostics, and harness feedback that do not change
  the authorized work
- stricter local validation or review evidence that preserves or narrows the
  accepted contract

Human-required:

- product goal changes or replacement of the issue objective
- accepted behavior changes, even when the code delta is small
- public API, CLI, configuration, workflow, or compatibility-contract changes not
  authorized by the accepted contract
- security, credential, billing, privacy, destructive data-loss, or live-operation
  risk that was not already accepted
- validation, review, or repo-gate weakening
- ownership conflicts with another active, retained, review, landing, or cleanup lane
- changes to accepted Decision Contract objectives, non-goals, constraints,
  acceptance criteria, validation expectations, or stop conditions

`insufficient_evidence` is also a stop disposition. Use it when the runtime cannot
prove whether a recovery is inside or outside the envelope from the current Decision
Contract, issue, project policy, lane ownership, and private evidence.

## Authority Boundary Check

Before autonomous loop recovery continues a detached or guardrail-pressured lane, the
runtime must record a private Authority Boundary Check when the attempted recovery
could change the Authority Envelope or when evidence is too weak to prove that it does
not. This check is evidence plumbing for downstream recovery workers; this spec does
not implement the full autonomous architecture recovery execution loop.

The private payload is versioned as `decodex.authority_boundary_check/1` with
`event_type = "authority_boundary_check"` in `private_execution_events`. It records:

- issue id and issue identifier
- run id and attempt number
- referenced Decision Contract ids when known
- attempted recovery reason, such as `uncovered_direction`,
  `ambiguous_retained_progress`, `review_churn`, or `hard_interrupt_fallback`
- changed surfaces, each with a surface kind, compact change summary, and local
  classification
- final disposition: `within_authority`, `requires_human`, or `insufficient_evidence`
- final disposition reason
- sanitized harness improvement signals when the check reveals an underspecified
  contract field, incomplete issue template, weak prompt, weak validator, or stale
  readiness model

Authority Boundary Checks are local private evidence. Linear, GitHub, generated issue
briefs, and ordinary operator summaries may expose only coarse reason codes or next
actions rendered by allowlisted lifecycle paths. They must not mirror raw changed
surfaces, graph ids, transcript text, or private recovery payloads.

When the final disposition is `requires_human`, detached lanes must create a durable
decision request instead of asking a transient Codex chat. The private payload is
versioned as `decodex.authority_decision_request/1` with
`event_type = "authority_decision_request"` in `private_execution_events`. It links
the issue id, issue identifier, run id, attempt number, Authority Boundary Check
record id, retained worktree or diff evidence, and recovery-attempt context. It also
stores the public-safe decision request id, reason code, boundary type, proposed
change, why the change exceeds accepted authority, options, recommendation, resume
condition, and `phase = "human_required"`.

The matching Linear projection must add or preserve `decodex:needs-attention` and
write an allowlisted `manual_attention` comment with the public-safe request fields.
It must not expose internal graph ids, host-local paths, raw diffs, credentials,
transcripts, logs, or sensitive runtime payloads. Operator status and dashboard
snapshots surface the request as `phase = human_required`, the boundary reason,
boundary type, `decision_request_id`, and `next_action` so operators can find the
decision without SQLite inspection.

A decision request is resolved only by an explicit issue update, Decision Contract
update, or supported policy update that accepts, rejects, or revises the proposed
direction. After that deliberate decision, an operator may clear
`decodex:needs-attention` and requeue or resume through normal Decodex lifecycle
controls. Raw tracker mutation, direct database edits, and internal graph ids are not
supported resume mechanisms.

Harness feedback may recommend Decision Contract, issue-template, validator, prompt,
or readiness-model hardening from boundary-check failures. Those recommendations are
advisory. They do not modify the accepted Decision Contract, queue eligibility, or
project policy by themselves.

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

The runtime-facing Execution Program payload is versioned as
`decodex.execution_program/1` with `record_version = 1`. It is stored in runtime
SQLite and carries:

| Field | Meaning |
| --- | --- |
| `program_id` | Stable runtime identifier for this internal program. |
| `service_id` | Registered Decodex service that owns queue-label decisions. |
| `source_contract_id` | Accepted Decision Contract that authorized the program. |
| `accepted_contract_fingerprint` | Fingerprint of the accepted contract used for drift detection. |
| `nodes` | Internal executable nodes. |

Each program node carries:

- objective lineage back to the accepted Decision Contract
- executable stage: `research`, `design`, `spec`, `schema`, `runtime`, `plugin`,
  `eval`, or `handoff`
- explicit dependencies with optional terminal-state requirements; when omitted, the
  registered `WORKFLOW.md` terminal states satisfy the dependency
- conflict domains for `file`, `module`, `state`, `credentials`,
  `tracker_ownership`, and `review_surface`
- acceptance expectations and validation expectations
- queue intent: `not_ready`, `ready_to_queue`, `queued`, `active`, `paused`, `done`,
  or `canceled`
- linked normal Linear issue identity and startability facts when the node becomes
  executable
- accepted-contract fingerprint used to detect node-level drift

Normal Linear issues remain the executable Decodex lanes. A program node may become
eligible only by creating or updating a normal issue with enough natural-language
briefing for generic dispatch and by applying the configured queue policy. The
Execution Program does not replace Linear as the team-visible backlog or the runtime
lane model.

Readiness evaluation is runtime-owned. It classifies nodes as:

| State | Meaning |
| --- | --- |
| `not_ready` | The node is intentionally not startable yet. |
| `ready` | Dependencies, conflicts, acceptance expectations, validation expectations, and issue mapping allow normal execution. |
| `blocked` | A dependency, conflict domain, missing expectation, or Linear issue mapping blocks execution. |
| `paused` | The accepted program intentionally paused the node. |
| `active` | The node already has an active lane and must not retain the queue label. |
| `completed` | The node is `done` or `canceled`. |
| `stale` | The node or program no longer matches the accepted Decision Contract. |

Queue-label action is derived from readiness, not from graph presence alone. Only a
`ready` node whose queue intent is `ready_to_queue` or `queued`, and whose mapped
Linear issue is in a registered startable state with no opt-out, needs-attention,
active-label, missing-briefing, or terminal-state blocker, may receive or retain
`decodex:queued:<service-id>`. Non-startable, blocked, stale, active, paused,
completed, or unmapped nodes must not receive or retain that queue label.

Operator readback may summarize program progress as counts of ready, blocked, paused,
active, completed, stale, and queue-label-eligible nodes plus the mapped issue
identifiers. It must not turn graph ids, edge editing, or DAG commands into the
primary user workflow.

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
  fallout, operator-facing fallout, and mismatch with the accepted Loop/Decision
  Contract
- candidate findings must be validated before repair work changes the lane

The review orchestration contract, including review levels and review-stop classes,
is defined by [`review-orchestration.md`](./review-orchestration.md).

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
| `validation_repeat` | The same validation class repeats after bounded repair. Legacy public summaries may still use `validation_failure_repeated`. |
| `no_effective_diff` | Repeated attempts do not change the tracked delta, evidence, or decision state. |
| `remaining_delta_unchanged` | Validation text or failure presentation changes, but the remaining tracked delta is unchanged across bounded repair attempts. |
| `review_churn` | Review findings exceed the accepted repair convergence budget. Existing review-policy stops may still appear as `review_policy_exhausted`. |
| `architecture_review_required` | The lane needs architecture direction before more repair. |
| `review_policy_blocked` | Review cannot proceed from available evidence. |
| `dependency_program_stale` | The node is waiting on dependency state that is not progressing. Legacy summaries may still use `dependency_blocked`. |
| `uncovered_direction` | Execution uncovered a missing or contradictory decision contract. Legacy summaries may still use `research_contract_required`. |
| `ambiguous_retained_progress` | Tracker, PR, branch, retained worktree, or runtime ownership evidence is contradictory. Legacy summaries may still use `ownership_ambiguous`. |

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

Runtime outcome feedback is recorded as local private execution evidence. The
versioned payload is `decodex.harness_outcome/1` with `event_type =
"harness_outcome"` in `private_execution_events`. It correlates:

- source intent, Decision Contract ids, generated issue identifiers, generated node
  ids, and conflict domains
- phase-goal signals, validation results, validation failure classes, and repair
  attempts
- independent review checkpoint status, accepted findings, rejected findings, and
  non-clean review rounds
- manual-attention or guardrail reason codes such as `uncovered_direction`,
  `dependency_program_stale`, `validation_repeat`, or `no_effective_diff`
- PR handoff, retained repair, closeout, cleanup, and terminal failure outcomes as
  summarized from cached Linear execution records and local runtime state
- candidate harness improvements such as `missing_validator`, `weak_prompt`,
  `missing_issue_template_field`, `underspecified_decision_contract`, and
  `stale_readiness_model`

Operator readback may summarize improvement candidates through `decodex evidence` or
derived agent-evidence files. Default readback must expose only compact candidate kind,
reason code, target, source-event count, and recommendation text. Full private payloads
remain local and require explicit private evidence readback such as
`--include-payload`.

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

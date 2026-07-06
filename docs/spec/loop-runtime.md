---
type: "Spec"
title: "Loop Runtime Specification"
description: "Define the natural-language-first Decodex loop-runtime contract that sits above individual issue lanes. Status: normative Read this when: You are implementing or reviewing Decodex-native research method gates, decision promotion, internal execution planning, phase-scoped Codex goals, unattended loop behavior, or loop guardrails. Not this document: The issue-lane state machine, low-level `app-server` protocol, post-`In Review` phases, operator lane-control commands, or the concrete research run artifact format. Defines: The user surface, Research/Decision stage, latent Loop/Decision Contract, research method gates, internal Execution Program, promotion boundary, phase-scoped goal rules, validation and review boundary, unattended execution behavior, loop stop conditions, and harness improvement loop."
status: active
authority: normative
owner: runtime
tags: [spec]
code_refs: [apps/decodex/src/orchestrator/lane_decision.rs, apps/decodex/src/orchestrator/execution.rs, apps/decodex/src/orchestrator/execution_phase_goal.rs, apps/decodex/src/orchestrator/prompting.rs, apps/decodex/src/agent/tracker_tool_bridge/tools.rs, apps/decodex/src/research_design.rs, apps/decodex/src/autonomy_proposal.rs, apps/decodex/src/loop_contract.rs, apps/decodex/src/execution_program.rs, apps/decodex/src/program_intake.rs]
drift_watch: [lane_decision, continuation_lineage, phase_goal, phase_acceptance_check, docs_impact, review_contract, issue_review_checkpoint, decodex.autonomy_proposal/1, decodex.decision_contract/1, execution_program, decodex research compile, decodex research promote, decodex intake goal]
last_verified: 2026-06-30
---
# Loop Runtime Specification

Purpose: Define the natural-language-first Decodex loop-runtime contract that sits
above individual issue lanes.
Status: normative
Read this when: You are implementing or reviewing Decodex-native research method
gates, decision promotion, internal execution planning, phase-scoped Codex goals,
unattended loop behavior, or loop guardrails.
Not this document: The issue-lane state machine, low-level `app-server` protocol,
post-`In Review` phases, operator lane-control commands, or the concrete research
run artifact format.
Defines: The user surface, Research/Decision stage, latent Loop/Decision Contract,
research method gates, internal Execution Program, promotion boundary, phase-scoped
goal rules, validation and review boundary, unattended execution behavior, loop stop
conditions, and harness improvement loop.

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

Research output is latent until accepted or promoted. A research concept, plan draft,
or proposed issue split must not by itself enqueue work, create authoritative
dependencies, set goals, mutate tracker state, or start implementation.

After acceptance or promotion, the accepted Loop/Decision Contract, shortened to
Decision Contract in this spec, becomes loop-runtime authority. The runtime may then
shape or update normal Linear issues and dispatch intent, but executable work still runs
through the lane runtime contract.

## Research/Decision Stage

Decodex owns a native Research/Decision compiler for Decodex work. That stage accepts
natural-language intent such as `research X` plus bounded research/design evidence
when available, then stores a local Decision Contract candidate. It supersedes the
external research skill for Decodex runtime authority: Decodex plugin `research`,
`$deliberation:skeptic`, and Decodex `research-promote` skills are the current
agent-facing method, `docs/research/` is the Markdown OKF
research concept lane, and checked-in research JSON event logs are not a valid docs
shape. Checked-in research evidence belongs in Markdown OKF concepts; runtime
authority still comes from the runtime-local Decision Contract until accepted and
promoted.

The native research method has these ordered gates:

1. Probe frames the decision question, scope, success criteria, constraints, stop rule,
   primary hypothesis, rival hypotheses, and falsifiers before broad evidence
   collection.
2. Evidence records an auditable ledger of external sources, repository sources, live
   readbacks, contradictions, inferences, and gaps. No evidence, no claim.
3. Options compare realistic choices, including status quo or explicit no-go when
   relevant, with evidence-grounded tradeoffs.
4. Judgment creates a skeptic-ready recommendation or explicitly states that the run
   is not decision-ready.
5. Skeptic attacks the judgment with adversarial objections. Material unresolved
   objections become missing decisions, evidence gaps, risk notes, or blockers.
6. Decision ends the run with exactly one outcome and preserves the latent promotion
   boundary.

A Research/Decision stage may produce a latent Loop/Decision Contract with:

- objective and objective lineage
- evidence ledger, constraints, assumptions, and rejected alternatives
- proposed decisions and open direction questions
- non-goals and scope boundaries
- acceptance criteria and validation expectations
- dependency and blocker model
- conflict domains such as `docs`, `runtime`, `site`, `tests`, or a more specific
  repository-owned domain
- proposed issue split and dispatch intent
- risk notes that decide whether independent review is required
- promotion target, such as `docs/spec`, `docs/runbook`, `docs/reference`,
  `docs/decisions`, `plugins/decodex/skills`, runtime code, tests, or explicit
  `no_promotion`

The latent contract is a candidate decision package. It becomes authoritative only
after the user or an accepted runtime policy promotes it.

Native research/design compiler outcomes are:

| Outcome | Contract status | Meaning |
| --- | --- | --- |
| `decision_ready` | `draft_latent` | Bounded evidence, option comparison, skeptic objection records, validation expectations, assumptions, and proposed issue-readiness data are sufficient for downstream issue shaping after promotion. It is still not execution authority while latent. |
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
| `research_evidence` | Non-authoritative evidence claims retained for later review and issue shaping. Each item carries `kind`, `claim`, `support`, and optional `source_ref`; `kind` identifies whether the support is an external source, repository source, live readback, inference, or unresolved gap. |
| `research_options` | Non-authoritative option comparisons retained with tradeoffs, selected decision notes, or rejected-option reasons. |
| `accepted_authority` | Objectives, non-goals, constraints, assumptions, objections, and stop conditions that become authority only when status is `accepted_promoted`. |
| `execution_readiness` | Natural-language readiness summary, missing decisions, validation expectations, risk notes, structured `proposed_issues[]`, promotion targets, and conflict domains. It must not expose graph ids or require the user to operate a DAG. Accepted contracts must be ready for issue shaping, include structured proposed issues, and must not carry unresolved missing decisions. |
| `promotion` | Metadata recording who or what accepted the decision, the acceptance source, and the acceptance time. Required only for `accepted_promoted`. |
| `links` | Generated Linear issue ids/identifiers or internal Execution Program node ids when those exist. |
| `evidence_boundary` | Local private evidence references and sparse public projection references. |

The status is the authority boundary:

- `draft_latent` means the research/design result is stored but cannot enqueue,
  mutate tracker state, set goals, or authorize implementation.
- `accepted_promoted` means the payload's `accepted_authority` fields may be used by
  the loop runtime to shape dispatch intent, generated issues, or internal Execution
  Program nodes. The payload must include promotion metadata, set
  `execution_readiness.ready_for_issue_shaping = true`, include non-empty
  `execution_readiness.proposed_issues[]`, and leave `execution_readiness.missing_decisions`
  empty.
- `rejected_superseded` means the payload is retained for audit/history but must not
  be promoted later.
- `needs_human_decision` means the package is incomplete or contradictory enough that
  execution must wait for more direction. The payload must include at least one
  `execution_readiness.missing_decisions` entry.

Research provenance and research evidence are not execution authority. They explain
why the candidate package exists and give future agents enough context to avoid asking
the user to restate all details after promotion.

`execution_readiness.proposed_issues[]` is the only issue-shaping input for promoted
Decision Contracts. The former flat `proposed_issue_summaries` field is removed, not
deprecated, and runtimes must not compile issues from it. Each proposed issue carries
`key`, `title`, `objective`, `stage`, `dependencies`, `conflict_domains`,
`acceptance`, `validation`, `risk`, and `queue_intent`. Accepted contracts without at
least one structured proposed issue fail readiness validation and cannot be
materialized for goal intake or Program Intake.

Runtime schema migration may rewrite old local SQLite payloads that still carry
`execution_readiness.proposed_issue_summaries` or
`execution_readiness.queue_intent`. That rewrite is a one-time data migration only:
each legacy summary becomes a structured `proposed_issues[]` entry with `handoff`
stage and `not_ready` queue intent so operators can inspect and revise the contract.
Normal readback, status, Program Intake, and compile paths remain strict and must not
treat the removed flat fields as supported compatibility input.

Decision Contracts are top-level snapshots. Terminal status, selected option, material
evidence, unresolved gaps, validation expectations, and promotion target must be
readable from the payload without replaying chat or scanning a chronological event
log. Event trails and legacy research provenance from Git history may support audit,
but they must not be the primary research output.

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

The Authority Boundary is a typed policy matrix, not a broad approval gate:

| Changed surface | Policy decision | Recovery consequence |
| --- | --- | --- |
| `implementation_strategy`, `runtime`, `tests`, or `docs` implementation details that preserve the accepted objective | `auto_continue` | Continue autonomous recovery while budget remains. |
| `public_api`, `config`, `security`, `data`, `billing`, or `privacy` | `requires_enhanced_evidence` | Continue recovery, but preserve stronger review, test, migration, or operator evidence before review handoff or landing. |
| `validation` or `review_policy` | `block_landing` | Continue recovery only to restore or strengthen the gate; review handoff or landing remains blocked until the evidence standard is restored. |
| `objective`, `non_goal`, `external_dependency`, `retained_ownership`, or `authority_evidence` | `requires_human_decision` | Stop automatic recovery and preserve a durable authority decision request. |

The legacy disposition remains as a compatibility summary. New recovery decisions are
driven by `policy_decision`: `requires_human_decision` stops automation;
`auto_continue`, `requires_enhanced_evidence`, and `block_landing` keep internal
implementation recovery automatic within the bounded budget.

During architecture recovery, the runtime derives the top-level policy from both the
guardrail reason and the retained worktree's tracked diff paths. Diff-path inference
adds typed surfaces for docs, tests, config, public API/CLI/protocol, security, data,
billing, privacy, validation, review policy, and ordinary runtime implementation
files, then applies the highest-risk policy across all observed surfaces.

## Authority Boundary Check

Before autonomous loop recovery continues a detached or guardrail-pressured lane, the
runtime must record a private Authority Boundary Check when the attempted recovery
could change the Authority Envelope or when evidence is too weak to prove that it does
not. Guardrail recovery may continue only when the check's `policy_decision` allows
autonomous recovery and the recovery budget still has room. `requires_human_decision`
is a hard stop for the current autonomous lane.

The private payload is versioned as `decodex.authority_boundary_check/1` with
`event_type = "authority_boundary_check"` in `private_execution_events`. It records:

- issue id and issue identifier
- run id and attempt number
- referenced Decision Contract ids when known
- attempted recovery reason, such as `uncovered_direction`,
  `ambiguous_retained_progress`, `review_churn`, or `hard_interrupt_fallback`
- changed surfaces, each with a typed surface kind, compact change summary, per-surface
  `policy_decision`, and legacy disposition
- top-level `policy_decision`: `auto_continue`, `requires_enhanced_evidence`,
  `block_landing`, or `requires_human_decision`
- policy flags: whether autonomous recovery is allowed, enhanced evidence is required,
  or landing is blocked
- final legacy disposition: `within_authority`, `requires_human`, or
  `insufficient_evidence`
- final disposition reason
- sanitized harness improvement signals when the check reveals an underspecified
  contract field, incomplete issue template, weak prompt, weak validator, or stale
  readiness model

Authority Boundary Checks are local private evidence. Linear, GitHub, generated issue
briefs, and ordinary operator summaries may expose only coarse reason codes or next
actions rendered by allowlisted lifecycle paths. They must not mirror raw changed
surfaces, graph ids, transcript text, or private recovery payloads.

When the policy decision is `requires_human_decision`, detached lanes must create a
durable decision request instead of asking a transient Codex chat. The private payload
is versioned as `decodex.authority_decision_request/1` with
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

## Architecture Recovery Packet

When loop guardrails detect non-converging automation, Decodex first stops the current
ineffective strategy. It then records a private Architecture Recovery Packet before
deciding whether to continue autonomously or require human attention.

The private payload is versioned as `decodex.architecture_recovery_packet/1` with
`event_type = "architecture_recovery_packet"` in `private_execution_events`. It
records:

- issue id, issue identifier, title, run id, attempt number, branch, and dispatch mode
- Decision Contract ids, statuses, source issue ids, and update timestamps when known
- Execution Program ids linked to those contracts when known
- retained worktree HEAD, tracked status, diff/status hashes, and compact diff stat
- validation failure or loop-guardrail source class and private error summary
- latest review checkpoint status, active/stop finding fingerprints, and
  accepted/rejected finding counts when present
- prior architecture recovery attempts for the issue
- recovery budget attempt and maximum
- loop-guardrail reason, threshold, consecutive count, fingerprint, and source class
- linked Authority Boundary Check record id, legacy disposition, policy decision,
  enhanced-evidence flag, landing-block flag, and final reason

If the boundary policy allows autonomous recovery and budget remains, Decodex records
`event_type = "architecture_recovery_started"` with reason code
`architecture_recovery_started`, clears the stopped guardrail reason, and starts a
materially different implementation strategy. This recovery may change internal
architecture, plumbing, tests, and docs needed to satisfy the same accepted objective.
It must not weaken validation or review gates. `requires_enhanced_evidence` and
`block_landing` remain visible in private evidence and operator status so handoff or
landing cannot proceed without the stronger evidence the surface requires. Both
policies remain unresolved for post-review landing classification until a later clean
review checkpoint for the current lane head records that the enhanced or blocked
surface evidence has been restored.

If recovery would change the objective or non-goals, lacks authority evidence, depends
on external/manual state, or exhausts its bounded recovery budget, Decodex records
`event_type = "architecture_recovery_terminal"` with a reason code such as
`contract_boundary_required`, `external_dependency_required`, or
`architecture_recovery_exhausted`, then routes through the human-required failure
path. Boundary stops should also record an `authority_decision_request` private event
so operator status can surface the decision request without exposing raw private
payloads.

## Promotion Boundary

Promotion is the boundary between design and execution authority.

Accepted autonomy proposals enter this boundary only as latent Decision Contract
candidates. Proposal acceptance records explicit bridge authority and preserves the
proposal lineage inside `decodex.decision_contract/1`, but the resulting contract
remains `draft_latent`; it cannot create Program Intake rows, issues, or queue intent
until normal Decision Contract promotion records accepted execution authority.
The bridge is idempotent only for an unchanged unpromoted latent contract. It must
refuse to overwrite any existing contract with promotion metadata, generated issue or
execution-program links, or non-latent status.

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

Promotion may create or update normal Linear issues, dependencies, and queue intent.
It must not apply service queue labels for Program readiness or expose an ordinary
user workflow that depends on graph ids, manual DAG edge editing, or hidden Codex goal
state.

## Program Intake Plan

Program intake is first-class loop-runtime behavior after a Decision Contract is
accepted or after the user supplies an explicit executable issue-batch intake. The
ordinary user workflow stays natural language: the user may ask Decodex to push an
accepted goal forward or provide a batch of issue briefs, but the user does not edit a
DAG, set queue labels by hand, or operate hidden graph commands.

The durable intake-planning payload is versioned as
`decodex.program_intake_plan/1` with `record_version = 1`. The runtime stores it in
runtime SQLite as part of, or directly adjacent to, the internal
`decodex.execution_program/1` record. The adjacent runtime readback rows are
`program_intake_plans` and `program_issue_mappings`; they are derived from the
versioned program payload and exist so operator status, scheduling, and tests can query
intake state without copying private graph payloads into Linear. Linear issue
descriptions and comments must not expose Execution Program ids, Program node ids, or
raw graph mechanics; public operator summaries may expose only sparse projections such
as counts, public issue identifiers, and coarse readiness reason codes.

The payload carries:

| Field | Meaning |
| --- | --- |
| `plan_id` | Stable runtime id for this intake plan. |
| `service_id` | Registered service that owns program scheduling for this plan. |
| `intake_kind` | `goal_intake` for promoted natural-language goals, or `issue_batch_intake` for a supplied batch of executable issue briefs. |
| `source_contract_id` | Accepted Decision Contract id for goal intake, when present. |
| `source_objective_ref` | Optional private Objective Contract lineage for accepted autonomy-derived goal intake. |
| `source_proposal_id` | Optional private autonomy proposal id for accepted autonomy-derived goal intake. |
| `source_signal_refs` | Optional private autonomy signal ids that contributed to the accepted autonomy-derived proposal. |
| `accepted_contract_fingerprint` | Fingerprint used to detect drift from the accepted contract or batch boundary. |
| `public_summary` | Public-safe objective/readiness summary suitable for status readback. |
| `node_projection` | Optional public-safe node summary or count metadata. The full internal nodes, dependencies, conflict domains, issue mapping, dispatch readiness, and lifecycle evaluation inputs live in the paired `decodex.execution_program/1` payload. |

Goal intake and issue-batch intake both materialize normal Linear issues. A goal
intake starts from an accepted Decision Contract and shapes one or more issue briefs.
An issue-batch intake starts from a supplied batch of issue briefs and records the
batch boundary as the accepted authority. In both cases, dependencies and ordering may
be represented internally as a DAG, but executable work still enters Decodex as
ordinary Linear issue lanes with generic natural-language descriptions, tracker
states, validation expectations, and Decodex lifecycle writeback.

Every mapped normal issue must carry a generic dispatch briefing that a cold-start
implementation lane can execute without replaying chat or reading private runtime
state. A complete Decodex-planned briefing names one outcome, public authority
summary, required reading, in-scope work, explicit non-goals, current-tree landing
zone, ownership boundary, acceptance criteria, validation expectations, stop
conditions, and any real dependencies, blockers, or conflict domains. The public
authority summary may cite the accepted Decision Contract or source issue, but it must
not render autonomy signal ids, autonomy proposal ids, internal Execution Program ids,
Program node ids, or graph mechanics. It must also preserve the normal lane lifecycle
by naming validation, review, PR handoff, landing, install/restart when applicable,
closeout, and cleanup as gates owned by the normal Decodex runtime rather than by
Program Intake. At minimum, runtime eligibility rejects a machine-only fenced block as
the issue description; private pointers, progress checkpoints, review summaries, PR
bodies, or runtime events do not substitute for the issue briefing.

The operator CLI surface for promoted goals is
`decodex intake goal --project <service-id> <CONTRACT_ID> --dry-run`, or the same
command with `--config <PROJECT_DIR>`. Dry-run reads the promoted Decision Contract
and existing generated-issue links, then prints the proposed normal issue briefs,
dependencies, conflict domains, and dispatch plan without mutating Linear and without
persisting local runtime rows. `--apply` is the explicit mutation boundary: it creates
or updates generated normal Linear issue descriptions, links generated issue ids and
internal node ids in runtime state, and stores the paired Program Intake Plan and
Execution Program in runtime SQLite. The generated Linear description remains a
natural-language issue brief and does not include those internal ids. Apply must not
run implementation inline and
must not apply or remove `decodex:queued:<service-id>`; the persisted Program is then
eligible for direct scheduler dispatch. If the contract is latent, rejected, still needs a human
decision, carries unresolved missing decisions, or lacks structured `proposed_issues`,
goal intake stops before creating executable work.

The operator CLI surface for existing issues is
`decodex intake issues --project <service-id> <ISSUE>... --dry-run`, or the same
command with `--config <PROJECT_DIR>`. Dry-run reads tracker state and prints a
deterministic ready/held/blocked/stale/unmapped report without mutating Linear and
without persisting local runtime rows. Dry-run reports set `scheduler_visible=false`,
so `dispatch_action=dispatch` means the transient plan would be dispatchable after
persistence under the current local runtime occupancy. Existing live shared leases
and retained nonterminal worktree mappings are part of that occupancy and must render
the row held instead of dispatchable. `--apply` is an explicit local-runtime write: it
stores the Program Intake Plan, Execution Program payload, and issue mappings, and sets
`scheduler_visible=true`, but it must not apply or remove
`decodex:queued:<service-id>`. Ready mapped nodes are dispatched directly by the
Program scheduler rather than converted into queued-label work.

## Internal Execution Program

An Execution Program is internal loop-runtime state derived from an accepted Program
Intake Plan. It may use DAG semantics, but the graph is backstage state rather than
the user-facing workflow.

The runtime-facing Execution Program payload is versioned as
`decodex.execution_program/1` with `record_version = 1`. It is stored in runtime
SQLite and carries:

| Field | Meaning |
| --- | --- |
| `program_id` | Stable runtime identifier for this internal program. |
| `service_id` | Registered Decodex service that owns program scheduling. |
| `source_contract_id` | Accepted Decision Contract that authorized the program, when the program came from goal intake. |
| `accepted_contract_fingerprint` | Fingerprint of the accepted contract or batch authority used for drift detection. |
| `program_intake_plan` | The embedded or linked `decodex.program_intake_plan/1` payload. |
| `nodes` | Internal executable nodes. |

Each program node carries:

- objective lineage back to the accepted Decision Contract or issue-batch authority
- executable stage: `research`, `design`, `spec`, `schema`, `runtime`, `plugin`,
  `eval`, or `handoff`
- operator readbacks must surface that concept as `program_stage`
- explicit dependencies with optional terminal-state requirements; when omitted, the
  registered `WORKFLOW.md` terminal states satisfy the dependency
- conflict domains for `file`, `module`, `state`, `credentials`,
  `tracker_ownership`, and `review_surface`
- acceptance expectations and validation expectations
- runtime-derived lifecycle state: `planned`, `mapped`, `ready`, `queued`, `active`,
  `blocked`, `needs_attention`, `completed`, `stale`, or `superseded`
- dispatch intent: `not_ready`, `ready_to_queue`, `queued`, `active`, `paused`, `done`,
  or `canceled`; `paused` renders as held lifecycle readback, not as a user-facing
  graph state
- linked normal Linear issue identity and startability facts when the node becomes
  executable
- direct dispatch readiness derived from issue state, dependency state, conflict
  domains, and human-stop labels
- accepted authority fingerprint used to detect node-level drift

Normal Linear issues remain the executable Decodex lanes. A program node may become
eligible only by creating or updating a normal issue with enough natural-language
briefing for generic dispatch and by satisfying the configured workflow policy. The
Execution Program does not replace Linear as the team-visible backlog or the runtime
lane model.

Lifecycle evaluation is runtime-owned. It classifies nodes as:

| State | Meaning |
| --- | --- |
| `planned` | The node exists only inside the Program Intake Plan and has no normal Linear issue mapping yet. |
| `mapped` | The node has a normal Linear issue mapping but is intentionally held from dispatch. |
| `ready` | Dependencies, conflicts, acceptance expectations, validation expectations, and issue mapping allow direct Program dispatch. |
| `queued` | Reserved for retained ready-to-dispatch state; new Program scheduling does not use service queue labels for this state. |
| `active` | The mapped issue already has an active lane. |
| `blocked` | A dependency, conflict domain, missing expectation, non-startable issue state, missing issue mapping, or missing briefing blocks execution. |
| `needs_attention` | The mapped issue carries the configured human-attention label or equivalent human-required stop. |
| `completed` | The node is done or canceled under the accepted program. |
| `stale` | The node or program no longer matches the accepted authority fingerprint. |
| `superseded` | A later accepted contract or batch authority replaced this node. |

Direct dispatch eligibility is derived from lifecycle readiness, not from graph
presence alone. Only a `ready` node whose dispatch intent is `ready_to_queue` or
`queued`, and whose mapped Linear issue is in a registered startable state with no
opt-out, needs-attention, active-label, missing-briefing, open blocker, or
terminal-state blocker, may receive `dispatch_action = dispatch`.

The Program scheduler is event-driven from local runtime state:

- Persisting or refreshing an Execution Program keeps the DAG in SQLite/local runtime
  memory; the scheduler evaluates the graph before ordinary queued-label issue
  selection.
- The scheduler refreshes only mapped Linear issue facts required for readiness:
  issue state, active/manual-only/needs-attention labels, open blockers, briefing
  presence, dependency observations, local shared run claims, and occupied conflict
  domains.
- A local shared run claim for the mapped issue makes that Program node active for
  scheduler and status readback, so startup worktree ownership does not self-block as
  `conflict_domain_occupied` while the live lane is already leased.
- When several nodes are dispatchable, normal issue candidate ordering chooses the
  first lane, and project/account concurrency still gates actual execution.
- Service queue labels are not applied, retained, removed, or treated as Program
  ownership evidence by this path. Existing `decodex:queued:<service-id>` labels
  remain ordinary tracker intake for non-Program issues.

Operator readback must summarize program progress without exposing graph operations as
workflow. Public-safe readback fields are: intake kind, public summary, ready count,
dispatchable count, blocked count, held count, stale count, attention count, completed
count, optional planned/mapped/active/superseded counts, dispatch action,
mapped issue identifiers, and coarse next action. It must not expose internal graph
ids, edge lists, raw dependency payloads, host-local paths, transcripts, credentials,
or private evidence references as public/team-visible status.

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

## Lane Decision Core

Lane execution must converge through a single next-action decision before scheduling
or continuing mutating work. The implemented core input is a private
`LaneDecisionSnapshot` for the lifecycle decision points currently routed through the
adapter: child-exit retry scheduling, retry-retention blocker checks, phase
acceptance, and repo-gate failures from phase-goal and terminal completion gates.
Those snapshots include phase, progress-checkpoint, repo-gate disposition,
scope-envelope, continuation/retry, terminal, and lineage fields needed by those
decision points. The core output is the only valid lifecycle action for the covered
decision point, such as `continue_current_phase`, `resume_continuation`,
`retry_failure`, `run_repo_gate`, `enter_review_handoff`, `wait_external`,
`needs_attention`, `stop_blocked`, `cleanup_terminal`, or
`forbidden_stale_or_ambiguous`.

Adapters such as daemon child-exit handling, retry retention, phase-goal validation,
repo-gate failure handling, review repair, and operator status may collect or
project state, but they must not independently reinterpret blocker, continuation,
scope, or terminal evidence into a contradictory next action for a covered decision
point. Future adapters that add tracker-state, run-lease, worktree-lineage,
review/recovery, or authority-evidence inputs must extend the snapshot instead of
creating a second lifecycle reducer. When the snapshot contains blocker-bearing
progress evidence, a non-goal violation, an authority-decision request, blocked
phase-goal recovery, a repo-gate scope-envelope violation, or ambiguous lineage,
automatic continuation and retry are forbidden until the blocker is materially
cleared. A newer progress checkpoint with an empty blocker set may clear older
progress-blocker evidence; stale blocker evidence must not remain terminal after an
explicit unblocked checkpoint.

## Phase-Scoped Codex Goals

Codex goals used by Decodex lanes must be phase-scoped. Do not set one giant goal such
as "finish the issue" for an entire lane.

Supported goal scopes are examples of the required shape:

| Goal phase | Meaning |
| --- | --- |
| `implement_to_validation_ready` | Produce the smallest coherent implementation or docs change that is ready for the repo gate. |
| `repair_validation_failures` | Fix concrete canonicalize or verify failures and rerun the same gate. |
| `repair_accepted_review_findings` | Repair validated review findings for the current head without widening scope. |
| `review_repair_evidence` | Push the retained PR repair head, confirm PR readback, record review-repair completion, and terminalize `review_repair` after accepted-review repair validation. |
| `handoff_evidence` | Prepare the PR-backed handoff, evidence summary, and terminal tracker signal after validation and review are satisfied. |

Goal completion is a trigger for the next validation or review step. It is not proof
that the lane is complete, reviewed, merged, landed, or closed. Lane completion still
requires the deterministic validation, review, PR handoff, manual-attention, landing,
closeout, and terminal-finalization contracts owned by the lower-level specs.
For `implement_to_validation_ready` and repair phases, completing the scoped goal is
the only valid way to exit a satisfied phase and hand control back to Decodex's
repo-gate transition. A progress checkpoint or final message that says the lane is
validation-ready and waiting for the next phase is only evidence; it must not replace
the Codex goal-complete signal.
After goal-complete, the repo gate is necessary but not sufficient for those phases:
Decodex must also record a private `phase_acceptance_check` that proves current-head
objective coverage, effective delta, changed surfaces, no non-goal violation,
docs-impact readiness, validation evidence, and a pass/fail decision. Only a passing
acceptance check may advance to `handoff_evidence` for ordinary implementation or
validation repair, or to `review_repair_evidence` for accepted-review repair. A
failing check caused by missing or stale evidence may keep the lane in the appropriate
repair phase with the reason and next action available in private status/evidence
readback. A failing check caused by blocker-bearing progress evidence, non-goal
violation, or repo-gate scope-envelope violation must route through the lane decision
core to human attention instead of ordinary validation repair. Retained phase-goal
recovery uses the same repo-gate plus acceptance check before scheduling automatic
continuation.
Effective delta is the canonical lane delta, not merely worktree dirtiness. It
includes issue-branch changes from the repo-gate base merge-base through current
`HEAD`, plus tracked or non-runtime untracked worktree changes. A clean worktree at
an issue-branch `HEAD` with committed lane changes is therefore effective progress,
not `no_effective_delta`.
When Decodex has already recorded a valid phase-goal continuation or active phase for
the same issue and must create a retry, Program, or automatic continuation attempt,
the new attempt resumes that unterminated phase state instead of restarting
implementation. Phase continuation is an issue-level cursor over private runtime
evidence before the current attempt, not a single previous-attempt field. Empty or
zero-evidence failed-start attempts do not reset an open terminal-evidence phase such
as `handoff_evidence` or `review_repair_evidence`. Only terminal finalization,
review completion intent, an authority-decision request, a blocked phase-goal
recovery, an audited failed-start cleanup, or a blocker-bearing progress checkpoint
can close or block inheritance.
This preserves the state-machine boundary between validated work and the later
review/handoff contract.
When Decodex schedules a continuation, it must record private `continuation_lineage`
evidence that distinguishes the continuation source run and attempt, phase cursor, and
`retry_budget_consumed = false` from ordinary failure retries.

## Validation And Review

Self-review is a cheap smoke check. It can catch obvious mistakes, missing edits, or
local reasoning gaps, but it is not sufficient completion evidence by itself.

Completion needs deterministic validation. For repository lanes, the registered
project `WORKFLOW.md` defines the canonicalize and verify commands, and
[`runtime.md`](./runtime.md) defines how repo-gate failures are classified.

When risk warrants review beyond self-review, use an independent fresh-context
read-only review pass. This pass is distinct from in-thread self-review:

- it reads the clean committed current `HEAD`, diff, requirements, and relevant specs
  from scratch
- it does not rely on the implementer's memory of the change
- it stays read-only while producing findings
- it uses the registered project workflow policy already injected by Decodex as the
  workflow-policy source, not an inferred repo-root `WORKFLOW.md`
- it records an explicit review contract covering objective, scope, non-goals, risk
  tier, required checks, allowed expansion triggers, and validation evidence
- it checks intended behavior, regression risk, tests, docs/config drift, migration
  fallout, operator-facing fallout, and mismatch with the accepted Loop/Decision
  Contract
- candidate findings must be validated before repair work changes the lane

For retained review repair, the independent pass is a repair verification pass: it
checks accepted findings from the previous review and regressions against the same
contract. New unrelated comments do not reset the review scope unless they match an
allowed expansion trigger such as safety, authority-boundary, data-loss, security,
live-mutation, public API, migration, or operator-facing regression.

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

For `no_effective_diff`, the repeated-observation fingerprint must be stable for the
absence of progress. It may record current `HEAD` in details for forensics, but `HEAD`
alone must not reset the consecutive counter when the canonical lane delta, validation
evidence, and decision state did not change.
For `validation_repeat`, the stop key must be normalized by issue, phase or command
domain, repo-gate error class, and lane authority domain. Raw error text, diagnostic
line counts, and changing worktree diff hashes may remain in forensic details, but
they must not reset same-class validation churn.

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

These outcomes first stop the ineffective strategy. The runtime must then classify
whether autonomous architecture recovery is allowed:

- Engineering implementation problems such as repeated repo-gate validation failures
  or no-effective-diff repair loops may continue only after an Authority Boundary
  Check records `auto_continue` and recovery budget remains.
- Public API/config/security/data/billing/privacy surfaces may continue only with
  `requires_enhanced_evidence`; handoff or landing must carry the stronger evidence
  required by that surface.
- Validation or review-policy weakening is `block_landing`; recovery may continue only
  to restore or strengthen the gate before handoff or landing.
- Product goal, accepted behavior, lane-ownership, authority-evidence, objective, or
  non-goal changes must stop with a human-required reason.
- Dependency or Execution Program staleness that requires external/manual state must
  stop with `external_dependency_required` or an equivalent typed reason.
- Missing or contradictory authority evidence must stop with
  `contract_boundary_required` or an equivalent typed human-decision reason until
  accepted authority is recorded.

Allowed recovery attempts are bounded and recorded. Exhausting the recovery budget is
itself a terminal recovery outcome, not a reason to silently fall back to the same
patch strategy.

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
- phase-goal signals, phase acceptance decisions, validation results, validation
  failure classes, and repair attempts
- independent review checkpoint status, accepted findings, rejected findings,
  active/stop finding fingerprints, and max active finding repeat count
- manual-attention or guardrail reason codes such as `uncovered_direction`,
  `dependency_program_stale`, `validation_repeat`, or `no_effective_diff`
- PR handoff, retained repair, closeout, cleanup, and terminal failure outcomes as
  summarized from cached Linear execution records and local runtime state
- candidate harness improvements such as `missing_validator`, `weak_prompt`,
  `missing_issue_template_field`, `underspecified_decision_contract`,
  `stale_readiness_model`, and `recovery_budget_exhausted`

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

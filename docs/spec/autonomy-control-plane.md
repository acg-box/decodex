---
type: "Spec"
title: "Autonomy Control Plane"
description: "Defines objective-driven project autonomy for Decodex."
status: active
authority: normative
owner: runtime
tags: [spec, autonomy, objective, mcp, agent-interface]
source_refs:
  - https://arxiv.org/abs/2303.11366
  - https://arxiv.org/abs/2305.16291
  - https://arxiv.org/abs/2303.17651
  - https://arxiv.org/abs/2405.15793
  - https://arxiv.org/abs/2407.01489
  - https://deepmind.google/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/
  - https://modelcontextprotocol.io/specification/2025-11-25
  - https://developers.openai.com/codex/learn/best-practices
code_refs:
  - apps/decodex/src/autonomy_objective.rs
  - apps/decodex/src/autonomy_signal.rs
  - apps/decodex/src/autonomy_proposal.rs
  - apps/decodex/src/loop_contract.rs
  - apps/decodex/src/config.rs
  - apps/decodex/src/mcp.rs
  - apps/decodex/src/program_intake.rs
  - apps/decodex/src/orchestrator/status.rs
  - apps/decodex/src/orchestrator/types.rs
  - apps/decodex/src/orchestrator/operator_dashboard/body.html
  - apps/decodex/src/state/store.rs
  - apps/decodex/src/state/internal.rs
related:
  - ./loop-runtime.md
  - ./runtime.md
  - ./lane-control.md
  - ./review-orchestration.md
  - ./agent-evidence.md
  - ../runbook/autonomy-implementation-roadmap.md
  - ../decisions/project-autonomy-control-plane.md
drift_watch:
  - decodex.autonomy_objective/1
  - decodex.autonomy_signal/1
  - decodex.autonomy_proposal/1
  - allowed_signal_kinds
  - Objective Contract
  - Decision Contract
  - Program Intake
  - finding_routes
  - decodex mcp serve
last_verified: 2026-06-27
---
# Autonomy Control Plane

Purpose: Define Decodex as an objective-driven project autonomy control plane.
Status: normative
Read this when: You are implementing or reviewing Objective Contracts, autonomy
signals, autonomy proposals, agent-facing MCP surfaces, or unattended
project-improvement loops.
Not this document: The ordinary lane state machine, the low-level app-server
protocol, exact CLI implementation, or a report template.
Defines: The autonomy authority model, Objective Contract schema boundary, signal
model, proposal flow, MCP action boundary, memory boundary, execution gates, and
self-dogfood limits.

## Product Boundary

Decodex autonomy is the project-level loop that converts accepted project intent,
evidence, agent feedback, review findings, telemetry, and validation outcomes into
auditable improvement proposals and then into ordinary Decodex execution work.

Autonomy is not a Decodex-only runtime anomaly repair feature. Runtime health is one
signal adapter. The product definition is objective-driven project improvement for
any registered Decodex project that has enough project contract, workflow, validation,
and execution authority for Decodex to operate.

Autonomy is also not a second scheduler, standalone memory system, or hidden agent.
It must preserve the current Decodex shape:

- Codex conversation is the default human authoring surface.
- Decodex server is the durable authority core.
- MCP and skills are agent interfaces, not authority substitutes.
- Normal Linear issues remain the executable lanes when tracker-backed work is used.
- Decision Contracts, Program Intake, validation, review, landing, closeout, and
  cleanup stay in their existing authority lanes.

## Product Form

The default user experience is conversational:

```text
operator in Codex
  -> describes objective, constraints, non-goals, metrics, and allowed surfaces
  -> Decodex drafts an Objective Contract
  -> operator or accepted project policy accepts a version
  -> Decodex collects typed signals under that version
  -> Decodex compiles bounded proposals
  -> proposal review/challenge decides whether a Decision Contract candidate exists
  -> accepted Decision Contract enters Program Intake or ordinary issue work
```

Other agents may use the same server through Decodex MCP resources, prompts, and
tools plus Decodex skills:

```text
Codex / external agent / CI bot
  -> Decodex skill routing and MCP resources, prompts, tools
  -> Objective Contract, signal, proposal, decision, evidence authority in Decodex
  -> repo, tracker, CI, review, logs, telemetry, docs, and operator feedback
```

Codex is an authoring interface. Accepted Objective Contracts, accepted Decision
Contracts, project policy, runtime state, and checked-in project authority decide
what may execute.

## Authority Layers

Autonomy has eight layers:

| Layer | Authority |
| --- | --- |
| Conversation authoring | Drafts objectives, feedback, proposal summaries, and review questions. It does not authorize execution. |
| Objective | Accepted Objective Contract versions define direction, non-goals, metrics, allowed surfaces, signal kinds, validation gates, and review policy. |
| Signals | Structured evidence from users, reviews, validation, telemetry, docs drift, protocol drift, runtime health, and external agents. Signals do not execute. |
| Proposal | A bounded candidate compiled from one objective version and one signal cluster. Proposals do not execute. |
| Decision | A proposal may become a latent Decision Contract candidate. It executes only after normal acceptance or promotion. |
| Execution | Program Intake, issue lanes, phase goals, validation gates, review handoff, PR, `decodex land`, install/restart when relevant, closeout, and cleanup. |
| Evidence | Runtime events, validation outputs, review checkpoints, PRs, telemetry pointers, operator snapshots, and report source refs. |
| Adapters | MCP, skills, memory backends, telemetry sources, CI bots, and external agents expose or submit context without replacing authority. |

No objective means no automatic project-improvement task. Decodex may still produce
read-only status, reports, signal audits, and objective drafts without an accepted
Objective Contract.

## Objective Contract

An Objective Contract is a first-class project-level authority object. It sits above
Decision Contracts. The objective defines the project direction; a Decision Contract
authorizes concrete work derived from that direction.

The Objective Contract must not be stored only in chat history or inferred from
project config. It is a versioned Decodex authority record. Project config may
reference the active objective version, but config presence alone does not grant
unattended execution authority.

The storage lifecycle is explicit in the runtime `autonomy_objectives` row and the
versioned payload. Readback exposes the state. `draft` records may be edited or
replaced before acceptance and have no execution authority. `accepted` records are
immutable authority versions and cannot be overwritten through the draft path. When a
newer version is accepted, the prior accepted version is marked `superseded` with
provenance while preserving its objective body. `rejected` and `superseded` records
remain provenance and must not authorize new proposals.

Canonical payload:

| Field | Meaning |
| --- | --- |
| `schema` | Versioned schema, initially `decodex.autonomy_objective/1`. |
| `project_id` | Registered Decodex service id. |
| `id` | Stable objective id. |
| `version` | Monotonic immutable version. |
| `state` | Lifecycle state exposed in readback: `draft`, `accepted`, `rejected`, or `superseded`. |
| `summary` | Short operator-readable purpose. |
| `goals` | Positive outcomes autonomy should optimize. |
| `non_goals` | Explicitly disallowed directions. |
| `metrics` | Evaluation signals, target ranges, or measurement questions. |
| `allowed_surfaces` | Files, modules, systems, commands, repos, workflows, or operations autonomy may propose touching. |
| `allowed_signal_kinds` | Signal kinds that may drive proposals for this objective. |
| `validation_gates` | Required repo-native, CI, review, smoke, runtime, or product checks. |
| `review_policy` | Human, independent-agent, GitHub, or policy-review requirement. |
| `memory_policy` | Which ledger, docs, history, and external memory adapters may be used as context. |
| `report_policy` | Redaction and completeness requirements for generated reports. |
| `acceptance` | Present for accepted versions and retained when an accepted version becomes superseded. Records `accepted_by`, `accepted_by_kind`, `accepted_at`, and `acceptance_source`. |
| `rejection` | Present only for rejected versions. Records rejecting actor, timestamp, source, and reason. |
| `supersession` | Present only for superseded versions. Links to the replacing objective id/version with actor, timestamp, source, and reason. |

Objective bodies are immutable after acceptance. A new objective version must not
silently reinterpret earlier proposals, current lanes, accepted Decision Contracts, or
Execution Programs.

## Project Policy

Unattended autonomy requires an accepted project policy that references an accepted
Objective Contract version. The policy lives in Decodex runtime authority state and
may be referenced from project config; project config is not itself sufficient
authority. A project config table may carry only reference identifiers or tracker
anchors. It must not embed or replace the accepted Objective Contract body,
project-policy body, allowed signal kinds, allowed surfaces, validation gates, review
requirements, cooldown, or write budget. Unknown `[autonomy]` and
`[autonomy.runtime_policy]` keys are rejected by the same config policy that rejects
unknown project config keys elsewhere.

The policy must name:

- objective id and version
- policy id, version, authority reference, scope, actor, actor kind, and
  acceptance source
- allowed signal kinds
- allowed surfaces
- validation gates
- review requirements
- cooldown and write budget
- tracker or Program Intake anchor when issue creation or intake is allowed
- whether proposal acceptance requires a human, accepted policy, or both

The policy may not weaken objective, review, validation, landing, release, install,
restart, closeout, or cleanup gates. It may only authorize specific promotion or
intake paths that still run through normal Decodex runtime authority.

## Signal Model

Signals are structured observations that may motivate improvement. They are
evidence, not execution authority.

Initial generic signal kinds:

| Kind | Meaning |
| --- | --- |
| `runtime_health` | Contradictory runtime, operator, app-server, lane-control, or status state. |
| `validation_regression` | Test, lint, typecheck, smoke, CI, or app verification failure. |
| `review_feedback_cluster` | Repeated or material review findings after review orchestration has classified the evidence. |
| `user_feedback_cluster` | Repeated user feedback, frustration, quality direction, or explicit improvement request. |
| `spec_drift` | Code, docs, config, status, or behavior diverges from accepted spec. |
| `protocol_drift` | App-server, MCP, CLI, telemetry, tracker, or external protocol changes drift from local support. |
| `metric_regression` | Objective metric moves the wrong way or fails to improve after accepted work. |
| `execution_friction` | Manual attention, loop churn, validation retries, review churn, or orchestration overhead repeats beyond objective tolerance. |
| `docs_skill_drift` | Skills, plugins, docs routing, or OKF concepts no longer match project behavior. |

Each signal must carry provenance:

| Field | Meaning |
| --- | --- |
| `schema` | Versioned schema, initially `decodex.autonomy_signal/1`. |
| `record_version` | Payload version for this schema, initially `1`. |
| `id` | Stable signal id derived from the fingerprint. |
| `fingerprint` | Stable dedupe fingerprint for objective-bound signal identity. It includes objective id/version, source refs, review route evidence, evidence class, gaps, contradictions, confidence, and privacy, but excludes volatile timestamps and observed counts. |
| `project_id` | Registered service id. |
| `objective_id` | Objective in force when the signal is interpreted. |
| `objective_version` | Exact immutable objective version. |
| `kind` | One accepted signal kind. |
| `source_type` | User, review, CI, telemetry, runtime, docs, protocol, agent, tracker, memory, or report. |
| `source_refs` | Stable pointers to runs, PRs, issues, logs, reviews, files, reports, or external sources. |
| `primary_source_refs` | Required source-of-truth pointers for memory/report-derived signals. |
| `issue_id` | Issue identifier when issue-scoped. |
| `run_id` | Runtime run identifier when run-scoped. |
| `attempt_id` | Attempt identifier when attempt-scoped. |
| `head_sha` | Repo head when code, validation, review, or docs are involved. |
| `captured_at` | UTC observation timestamp. |
| `freshness` | `fresh`, `stale`, or `unknown` against current objective, repo, and runtime state. |
| `summary` | Short human-readable description. |
| `evidence` | Public-safe summary or pointer. |
| `evidence_class` | `external_source`, `repo_source`, `live_readback`, `inference`, or `gap`. |
| `contradictions` | Conflicting facts that proposal review must preserve or resolve. |
| `gaps` | Missing evidence or unresolved questions. |
| `confidence` | Confidence category or score. |
| `privacy` | Visibility boundary for reports and MCP resources. |
| `observed_counts` | Optional counts for readback context. Counts are volatile and are not part of the dedupe fingerprint. |
| `review_evidence` | Required for review-derived signals. Contains review phase/status, current-head SHA, checkpoint refs, and normalized `finding_routes`. |
| `proposal_only` | Must be true. Signal persistence is evidence only and cannot grant execution authority. |
| `created_at` | UTC signal creation timestamp. |

Long runtime, high token use, expensive validation, or a hard task is not an
autonomy problem by itself. Autonomy reacts to contradiction, repeated friction,
objective drift, validation failure, review evidence, or measured regression.

The runtime persists signals in its own `autonomy_signals` table keyed by
`project_id` and stable signal id, with indexes for exact
`objective_id`/`objective_version` readback and recent project readback. Storing a
signal requires the referenced Objective Contract version to be the accepted version
at write time; later objective versions do not reinterpret older signal rows.
Operator status may show recent signal summaries, freshness, gaps, contradictions,
confidence, and privacy as read-only evidence. Signal rows do not mutate tracker
state, runtime authority rows outside signal persistence, worktrees, GitHub, Program
Intake, proposals, or execution state.

Review-derived signals must consume normalized review evidence, not raw comments.
`review_feedback_cluster` may use only review checkpoint routes from
`finding_routes`, explicit review-policy state, and current-head review evidence.
`current_blocker` routes may drive current repair proposals. `follow_up`,
`risk_note`, `needs_evidence`, `reviewer_rubric_gap`, `architecture_signal`, and
`issue_contract_gap` routes may drive future proposals or decision requests, but
must not silently become current repair work.

Memory-derived signals must carry primary source refs, freshness, confidence, and
privacy. Memory output is proposal input only until accepted by objective and
Decision Contract authority.

## Proposal Compiler

The proposal compiler converts objective-bound signal clusters into bounded
improvement candidates.

`decodex.autonomy_proposal/1` is planning evidence. Persisting a proposal creates no
execution authority by itself.

A proposal must bind:

- objective id and version
- project id
- sorted signal ids
- affected issue, run, contract, or Program identifiers when present
- source family
- intended surface
- goals and metrics it improves
- non-goals and allowed surfaces that constrain it
- validation gates
- review and challenge requirements
- optional `issue_candidates[]` for explicit issue splitting; each candidate must
  have a stable key, title, objective, stage, dependencies, acceptance criteria,
  validation expectations, queue intent, and optional conflict domains or risk notes
- evidence class, contradictions, and gaps
- rejected alternatives and rollback path

Stable proposal identity is derived from objective id, objective version, project id,
sorted signal ids, affected identifiers, source family, and intended surface. It must
also include explicit `issue_candidates[]` when present so a changed issue split is a
new proposal identity. It must not include timestamps, elapsed seconds, warning
order, volatile counts, or model output size.

Proposal states:

| State | Meaning |
| --- | --- |
| `draft` | Compiled but not reviewed. |
| `needs_evidence` | Missing proof blocks acceptance. |
| `needs_human_decision` | Direction, non-goal, risk, or authority requires human choice. |
| `rejected` | Explicitly rejected or superseded. |
| `decision_candidate` | Strong enough to compile a latent Decision Contract candidate. |
| `accepted_promoted` | Accepted through normal Decision Contract promotion. |

Accepting a `decision_candidate` proposal requires explicit proposal-acceptance
authority: accepting actor, actor kind, timestamp, acceptance source, reason, proposal
actor, and proposal actor kind. Runtime-policy or external-agent acceptance also
requires a resolved accepted project-policy record, not only a reference string. That
record must match the proposal project id, objective id/version, accepted policy
id/version, authority reference, authorized actor, actor kind, acceptance source, and
`autonomy_proposal_acceptance` scope. External-agent output cannot accept its own
proposal unless that accepted policy authority is present.

Proposal acceptance creates a normal `decodex.decision_contract/1` payload with
`status = draft_latent`. The bridge must preserve objective lineage, source signal
ids and summaries, contradictions, gaps, validation gates, review requirements,
rejected alternatives, rollback path, and proposal acceptance provenance in the
Decision Contract readback. It must not create tracker issues, Program Intake rows,
queue labels, worktrees, or execution lanes. Later execution still requires the
existing Decision Contract promotion path, such as `research_promote`, to record
promotion authority and move the contract to `accepted_promoted`.

Re-accepting the same proposal is idempotent only while the derived Decision Contract
is still an unpromoted `draft_latent` candidate with no generated execution links. The
acceptance bridge must not replace any existing contract that has promotion metadata,
`accepted_promoted`, `needs_human_decision`, or `rejected_superseded` status, generated
issue ids, generated issue identifiers, or execution program node ids.

Refusal rules:

- Missing Objective Contract -> refuse automatic proposal execution and offer an
  objective draft instead.
- Signal outside `allowed_signal_kinds` -> record or report, but do not compile an
  executable proposal.
- Surface outside `allowed_surfaces` -> require objective revision or human decision.
- Stale signal without fresh readback -> require readback before acceptance.
- Contradiction unresolved -> keep as `needs_human_decision` or `needs_evidence`.
- Review/validation weakening -> block promotion.

Non-trivial proposals use the generic skeptic method. When tool support and active
workflow allow it, Decodex should request a fresh dynamic read-only subagent
skeptic pass for architecture, review-repair, generated implementation, or
ready/decision-ready claims. Inline skeptic review is a fallback for small local checks or
when subagent tooling is unavailable. Skeptic output is objection evidence and
promotion constraints; it does not create acceptance authority and does not by itself
turn a `decision_candidate` into `needs_human_decision`. Material contradictions,
disallowed surfaces, weakened validation/review, or explicit authority gaps still use
the normal refusal rules.

## Execution Flow

Autonomy uses the existing loop-runtime authority boundary:

1. Draft or read an accepted Objective Contract.
2. Collect objective-bound signals with provenance and privacy.
3. Cluster signals under the exact objective version.
4. Compile a proposal with goals, non-goals, metrics, validation gates, risk, and
   refusal state.
5. Skeptic-review the proposal when non-trivial.
6. Convert an accepted strong proposal into a latent Decision Contract candidate with
   explicit proposal-acceptance authority.
7. Promote only through explicit human acceptance, accepted Decision Contract
   authority, or accepted project policy that references the objective version.
8. Materialize promoted work through Program Intake or normal issue work.
9. Execute through ordinary lanes, phase goals, validation, review, PR,
   `decodex land`, release/install/restart when relevant, closeout, and cleanup.
10. Record evidence and feed resulting signals back into the objective-bound loop.

Every executable proposal must become a normal Decodex execution surface. For
tracker-backed work this means a normal issue with a cold-start dispatch brief.
Private pointers, proposal ids, report text, or graph ids do not substitute for the
brief.
When accepted autonomy work is materialized through Program Intake, runtime state must
retain the replay chain from Objective Contract version to signal-derived proposal,
Decision Contract, Program Intake Plan, Execution Program, and generated normal issue
links. That lineage remains private runtime metadata; generated tracker issue text
stays a natural-language brief and must not expose signal ids, proposal ids, Program
ids, node ids, or graph mechanics.

Operator, App, and MCP status readback may expose a public-safe projection of the same
chain so autonomy is inspectable without SQLite. The projection must include the
current accepted Objective Contract version, recent signal summaries with source refs,
freshness, redaction level, completeness, and known gaps, proposal state and refusal
reasons, proposal -> Decision Contract -> Program Intake lineage, and replay evidence
from PR handoff, validation, and post-land or post-restart proof when the autonomy
work has reached those lifecycle surfaces. PR replay evidence is derived from retained
review lifecycle readback only when a matching private replay-evidence pointer ties
the retained PR row back to the same proposal or Decision Contract, run, attempt, PR
URL, PR head ref, and PR head oid. Validation and post-land replay evidence may be
projected from private runtime events with schema
`decodex.autonomy_replay_evidence/1`; `post_land` is the post-lifecycle umbrella for
post-land, post-restart, installation, and plugin-sync proof. Those events are
evidence pointers only and do not authorize review, landing, installation, restart,
plugin sync, or closeout. The projection must not include raw evidence payloads,
hidden reasoning, local-only paths, credentials, unredacted private source refs, or
generated issue graph mechanics.

## MCP And Skill Action Matrix

MCP is the typed machine interface. Skills are the policy pack that tells agents
when and how to use it.

| Action | MCP profile | Actor requirement | Authority effect |
| --- | --- | --- | --- |
| Read objective | `observe` | Project access | Read-only. |
| Read signal/proposal summaries | `observe` | Project access and privacy allowance | Read-only. |
| Submit signal | `plan` | Identified actor and source refs | Creates evidence only. |
| Draft objective | `plan` | Identified actor | Creates draft only. |
| Accept objective | `plan` or higher | Human/operator or accepted policy actor | Creates immutable Objective Contract version. |
| Compile proposal | `plan` | Accepted objective and allowed signal kinds | Creates proposal evidence only. |
| Challenge proposal | `plan` | Review actor or subagent evidence | Adds objections as promotion constraints; no execution or acceptance authority. |
| Promote proposal to Decision Contract | `plan` or higher | Explicit acceptance or accepted project policy | Creates a latent Decision Contract candidate; accepted execution authority still requires normal Decision Contract promotion. |
| Intake promoted work | `plan` or higher | Accepted Decision Contract and explicit intake authority | Creates Program Intake or issue materialization. |
| Dispatch lane | Runtime scheduler | Program readiness plus workflow eligibility | Starts normal lane; not direct MCP authority. |
| Lane control | `operate` | Inspect-first run/turn authority | Uses existing lane-control guards. |
| Project pause/resume | `admin` | Operator authority | Future-dispatch control only. |
| Land PR | Decodex land authority | PR, review, checks, and landing authority | Uses `decodex land`, not generic MCP merge. |

MCP resources should expose project-safe objective versions, signal summaries,
proposal summaries, status snapshots, evidence summaries, accepted Decision
Contracts, and checked-in docs. MCP prompts may draft objectives, submit signals,
compile proposals, challenge proposals, prepare validation-ready execution, or produce
evidence-backed reports. Mutating tools must require explicit authority fields and
return structured refusals when authority is missing.

An external agent cannot be its own acceptance authority. Authentication and
capability profile prove access to the Decodex surface; they do not prove acceptance
authority.

The Phase 7 autonomy MCP surface is deliberately small and typed:

- observe-profile resources:
  `decodex://projects/{project_id}/autonomy`,
  `decodex://projects/{project_id}/autonomy/objectives/{objective_id}/current`,
  `decodex://projects/{project_id}/autonomy/objectives/{objective_id}/{version}`,
  `decodex://projects/{project_id}/autonomy/signals`,
  `decodex://projects/{project_id}/autonomy/signals/{signal_id}`,
  `decodex://projects/{project_id}/autonomy/proposals`,
  `decodex://projects/{project_id}/autonomy/proposals/{proposal_id}`, and
  `decodex://projects/{project_id}/autonomy/evidence`
- plan-profile tools:
  `autonomy_draft_objective`, `autonomy_accept_objective`,
  `autonomy_submit_signal`, `autonomy_compile_proposal`,
  `autonomy_challenge_proposal`, and
  `autonomy_request_promotion`

The observe resources return summaries and authority-boundary metadata only. They are
not raw runtime payload exports. The plan tools validate or persist Objective Contract
draft and acceptance records, signal, proposal, challenge, and latent Decision
Contract candidate evidence through runtime model checks. Apply-style calls require
explicit authority fields and still stop before normal Decision Contract promotion,
Program Intake, review, PR handoff, landing, install, restart, closeout, or cleanup
authority. `autonomy_request_promotion`
may create only a latent Decision Contract candidate from an accepted proposal; the
result still requires normal `research_promote` and later Program Intake before
execution work exists. MCP callers cannot prove accepted project policy authority by
supplying an `acceptedProjectPolicy` body or by self-asserting `runtime_policy`
Objective Contract acceptance. Policy-backed runtime or external-agent acceptance must
be resolved from trusted Decodex authority state; until that resolver exists, MCP
policy-backed objective acceptance and proposal promotion requests fail closed with
structured refusals. Local-private
signals expose ref counts and redaction metadata only, not raw `source_refs` or
`primary_source_refs`.

## Telemetry And Memory Boundary

Allowed inputs:

- runtime SQLite private execution events
- operator snapshots
- agent-evidence projections
- Linear execution-ledger summaries
- GitHub check, PR, and review readbacks
- repo-native validation summaries
- checked-in docs, specs, runbooks, and decisions
- optional read-only memory, OKF, vector, log, or MCP adapters

Not allowed by default:

- raw transcripts
- full command output dumps
- secrets, credentials, tokens, or connector payloads
- hidden reasoning
- external telemetry shipping
- private evidence bodies through remote-safe MCP resources

Decodex must not become a standalone memory product. Core Decodex owns the ledger:
Objective versions, signals, proposals, Decision Contracts, run evidence, validation
results, review evidence, and telemetry pointers. Semantic retrieval is an optional
read-only adapter over docs, repo memory, Codex memory, external memory MCP, vector
stores, logs, or history. Adapter output may inform signals and proposals, but it
cannot grant authority, replace runtime SQLite, or replace source refs.

Reports and digests are disposable query views over primary evidence. Decodex does
not need a fixed weekly-report workflow. A report should include all
Decodex-server-managed work in the selected window and expose dimensions such as
`origin = user`, `origin = program`, `origin = autonomy`, and
`origin = maintenance`. Reports are not audit inputs unless regenerated from primary
evidence.

Generated reports must carry `generated_at`, `window_start`, `window_end`,
`source_surfaces`, `source_refs`, `redaction_level`, `completeness`, and
`known_gaps`.
Operator and MCP report readback must label report output as a derived query view,
not audit authority, and must expose source refs, redaction level, completeness, and
known gaps before any dashboard or agent-facing surface claims freshness.

## Self-Dogfood Boundary

Decodex should apply autonomy to itself first, but through the same generic contract:

- Decodex self-evolution needs an Objective Contract.
- Decodex runtime health feeds `runtime_health`; it does not define autonomy.
- Decodex docs/skills/protocol drift feeds generic signal kinds.
- Decodex changes still go through PR review, `decodex land`, build/install, server
  drain or fence, restart, plugin/skill sync when relevant, and post-land audit.

Autonomy must never patch installed binaries, restart services, mutate plugins, or
rewrite runtime state directly from a signal or report. Those remain lifecycle
surfaces after accepted work lands.

## Hard Limits

Autonomy must not:

- start implementation directly from a signal report
- mutate tracker state, runtime DB rows, worktrees, queue labels, installed binaries,
  plugins, skills, app-server turns, or GitHub from audit output alone
- treat chat memory, retrieved semantic memory, external-agent output, or MCP auth as
  acceptance authority
- replace project objectives, non-goals, validation gates, review gates, or landing
  gates without accepted authority
- expose hidden reasoning, raw private evidence, credentials, local-only path
  details, or internal graph ids through remote-safe MCP surfaces
- add broad one-tool-per-command MCP surfaces instead of stable objective, signal,
  proposal, evidence, and lane-control boundaries
- build a durable memory inbox, CRM, reporting product, or separate hidden autonomy
  ledger
- keep permanent compatibility shims for the rejected runtime-anomaly-only design

## Integration Points

- [`loop-runtime.md`](./loop-runtime.md) owns Decision Contract authority, Program
  Intake, Execution Programs, Authority Boundary Checks, and normal goal-to-work
  promotion.
- [`runtime.md`](./runtime.md) owns leases, attempts, runtime SQLite, private
  execution events, project contracts, and lane execution.
- [`lane-control.md`](./lane-control.md) owns inspect-first lane mutation.
- [`review-orchestration.md`](./review-orchestration.md) owns review evidence,
  `finding_routes`, and review churn escalation.
- [`agent-evidence.md`](./agent-evidence.md) owns agent-readable local evidence
  projections.
- [`../runbook/autonomy-implementation-roadmap.md`](../runbook/autonomy-implementation-roadmap.md)
  owns the implementation sequence.
- [`../decisions/project-autonomy-control-plane.md`](../decisions/project-autonomy-control-plane.md)
  records the rationale and research evidence.

Autonomy may read those surfaces. It must not bypass their mutation rules.

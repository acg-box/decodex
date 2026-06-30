---
type: "Spec"
title: "Runtime Specification"
description: "Define the authoritative runtime model for the `decodex` MVP. Status: normative Read this when: You need the authoritative model for issue eligibility, leases, lane ownership, runtime states, tracker-write ownership, or Linear writeback behavior. Not this document: The low-level `app-server` protocol contract, the downstream `WORKFLOW.md` schema, or the operator pilot procedure. Defines: The runtime scope, source-of-truth boundaries, eligibility rules, lane model, local state machine, tracker-write ownership, and writeback semantics."
status: active
authority: normative
owner: runtime
tags: [spec]
code_refs: [apps/decodex/src/orchestrator/lane_decision.rs, apps/decodex/src/agent/tracker_tool_bridge.rs, apps/decodex/src/agent/tracker_tool_bridge/tools.rs, apps/decodex/src/autonomy_signal.rs, apps/decodex/src/autonomy_proposal.rs, apps/decodex/src/orchestrator/execution.rs, apps/decodex/src/orchestrator/execution_phase_goal.rs, apps/decodex/src/orchestrator/git_ops.rs, apps/decodex/src/orchestrator/run_cycle.rs, apps/decodex/src/orchestrator/status/mod.rs, apps/decodex/src/mcp.rs, apps/decodex/src/state/store.rs, apps/decodex/src/state/internal.rs, apps/decodex/src/program_intake.rs, apps/decodex/src/execution_program.rs, apps/decodex/src/config.rs, decodex.example.toml]
drift_watch: [lane_decision, continuation_lineage, issue_progress_checkpoint, phase_acceptance_check, issue_terminal_finalize, docs_impact, manual_attention, review_handoff, review_repair, closeout, phase_goal, repo_gate_tracked_rewrites_left, repo_gate_scope_envelope_violation, protocol_events, review_policy_checkpoints, review_checkpoint, review_lifecycle_records, lane_control_next_action, review_handoff_pending, review_repair_pending, review_repair_writeback_missing_lifecycle_marker, review_repair_writeback_stale_lifecycle_marker, decodex mcp serve --transport stdio, decodex mcp serve --transport streamable-http, resources/templates/list, prompts/list, prompts/get, tools/list, tools/call, decodex intake goal, program_issue_mappings, autonomy_proposals, decodex.autonomy_proposal/1, "[autonomy]", auto_promote, auto_intake, "[autonomy.runtime_policy]", accepted_objective_id, accepted_objective_version, accepted_policy_id, accepted_policy_version, policy_authority_ref]
last_verified: 2026-06-30
---
# Runtime Specification

Purpose: Define the authoritative runtime model for the `decodex` MVP.
Status: normative
Read this when: You need the authoritative model for issue eligibility, leases, lane ownership, runtime states, tracker-write ownership, or Linear writeback behavior.
Not this document: The low-level `app-server` protocol contract, the downstream `WORKFLOW.md` schema, or the operator pilot procedure.
Defines: The runtime scope, source-of-truth boundaries, eligibility rules, lane model, local state machine, tracker-write ownership, and writeback semantics.

## Scope

- One `decodex` service instance.
- One isolated linked Git worktree lane per eligible issue.
- One direct `codex app-server` session per run attempt.
- Supported host targets are Unix only: macOS and Linux. Windows is outside the runtime contract.

## Relationship To Loop Runtime

[`loop-runtime.md`](./loop-runtime.md) owns the natural-language-first layer above
individual issue lanes: Decodex-native Research/Decision, latent Loop/Decision
Contracts, internal Execution Programs, phase-scoped goals, unattended execution
behavior, and loop guardrails.

This document owns the lower-level lane runtime. A promoted Execution Program may
shape dispatch intent and normal Linear issues, but executable work still enters this
runtime as ordinary issue lanes with leases, attempts, validation, review handoff, and
tracker writeback. The internal program graph is not a replacement for Linear workflow
state or this state machine.

## Upstream alignment

- Upstream Symphony is the architectural reference for scheduler and runner ownership.
- `decodex` keeps two deliberate divergences:
  - Rust implementation instead of Elixir
  - TOML frontmatter in `WORKFLOW.md` instead of YAML
- `decodex` should align with upstream on tracker ownership: the coding agent should normally perform issue-scoped tracker writes autonomously through runtime tools, while the service remains responsible for leases, worktree lifecycle, retries, reconciliation, and crash-safe fallback behavior.
- Linked-worktree lane planning, creation, reuse, and cleanup are runtime-owned responsibilities. They must not depend on an installable workflow skill being present.
- Target-repository workspace hooks may run at linked-worktree create and remove boundaries when declared in the registered project `WORKFLOW.md`, but those hooks do not own lifecycle authority; the runtime still decides when a lane is created, reused, retained, or cleaned up.
- Installable Codex `AGENTS.md` guidance must not own Decodex runtime lifecycle,
  identity routing, tracker write policy, repo-gate commands, review, landing,
  closeout, or cleanup semantics. Those policies live in this spec, adjacent specs,
  centralized project contracts, or owning skills as defined by
  [`installable-agent-policy.md`](./installable-agent-policy.md).
- Current implementation note: normal-path tracker writes now flow through the issue-scoped tool bridge. Service-owned tracker writes remain only as fallback for reconciliation, crash recovery, and terminal failure handling.

## Source of truth boundaries

- The Decodex runtime SQLite database is the single-machine source of truth for run leases, attempts, run-control channels, protocol events, private execution events, harness-outcome telemetry, Objective Contracts, latent and promoted Decision Contracts, internal Execution Programs, worktree mappings, retained PR state, review-policy checkpoints, loop-guardrail checkpoints, retry state, phase timing, project registration, tracker cache, PR cache, and connector backoff.
- Protocol events are keyed by `(run_id, sequence_number)` and store a `payload_sha256`
  digest for replay identity without storing raw protocol payloads. Exact replays of
  the same event type and digest are idempotent and must not inflate event counts or
  fail a continuation/recovery path. A different event type or different digest at the
  same sequence remains a journal integrity error.
- Protocol event summaries are compact startup state, not advisory cache. Schema
  migrations must backfill `protocol_event_summaries` from legacy raw journals once,
  and normal startup/status paths must load those summary rows instead of aggregating
  the full `protocol_events` journal. Bounded raw-journal recomputation is reserved for
  a requested run whose compact summary is missing, and it must persist the repaired
  summary before returning.
- `thread/archive` and `thread/archive/discarded` are terminal protocol barriers for
  one run. After either barrier is recorded, later non-terminal app-server events for
  the same run must not compete for the normal positive protocol sequence namespace.
  The runtime records them as `protocol/post_archive_event/discarded` in a negative,
  replay-stable sequence namespace derived from the original sequence, event type, and
  payload digest. These discarded rows are local recovery evidence only: they may
  increase protocol event counts, but they must not replace the terminal archive event
  as the run's latest event and must not turn a parent journal/closeout race into a
  child retry failure.
- Linear remains the team-visible tracker surface for issue lifecycle, queue/active/manual-attention labels, and coarse lifecycle summaries such as start, PR-ready, blocked, failed, landed, and done.
- Versioned Linear execution event comments use the schema in
  [`linear-execution-ledger.md`](./linear-execution-ledger.md), but fine-grained runtime truth must not be rebuilt from comments every tick.
- Private execution events are structured runtime evidence rows scoped by
  `project_id`, `issue_id`, `run_id`, and `attempt_number`. They hold full local
  evidence, including run-control audit records, that should be queryable through
  `StateStore` without being mirrored to Linear execution ledger payloads. The
  operator CLI readback path is
  `decodex evidence <ISSUE> --run-id <RUN_ID> --attempt <N>`, which reads the local
  runtime store and summarizes payloads by default.
- Centralized project directories under `~/.codex/decodex/projects/<service-id>/`
  form the project contract. Each directory contains `project.toml` for service
  paths and credentials plus `WORKFLOW.md` for execution policy. They do not store
  runtime ownership.
- The local SQLite database must not become a replacement for the human issue backlog. It is the operator control-plane state for this machine.
- `decodex research compile` and `decodex research promote` are runtime-local
  Decision Contract writes. They update the SQLite `decision_contracts` surface and do
  not by themselves create Linear issues, queue intent, goals, or executable lanes.
- Runtime schema migration owns removed Decision Contract payload rewrites. Schema 12
  removes `execution_readiness.proposed_issue_summaries` and
  `execution_readiness.queue_intent` rows from SQLite by converting summaries into
  structured `proposed_issues[]` with `handoff` stage and `not_ready` queue intent.
  After migration, normal Decision Contract readback is strict: it does not skip,
  quarantine, or compile the removed flat fields.
- `decodex mcp serve --transport stdio` is the local MCP gateway for desktop and CLI
  clients. The stdio gateway advertises resources, resource templates, prompts, tools,
  logging compatibility, and progress notifications. Resources read checked-in docs,
  checked-in Markdown research concepts, runtime Decision Contracts, local status
  snapshots, remote-safe live status/activity projections, current/recent status-window
  run event/protocol/child activity/progress diagnostics, PR/review state,
  lane-inspect aliases, and lane-control readback. Tools are schema-bound and
  deliberately small: `decodex_observe` is read-only, `decodex_plan` returns static
  workflow routing, and the plan-profile `research_compile`, `research_promote`, and
  `intake_goal` tools expose dry-run/apply boundaries over existing Decodex research,
  promotion, and Program Intake authority checks. Dry-run planning calls do not mutate
  tracker state or Program Intake rows. Apply/promote calls require explicit authority
  fields and return structured refusals when authority or project context is missing.
  Operate exposes `decodex_lane_control` as an inspect-first facade over existing
  lane-control authority: `inspect` returns current preconditions, mutating `steer`
  and `interrupt` require matching inspected run/turn authority, and unsupported
  shortcut paths refuse to the canonical tracker or runtime lifecycle. Admin exposes
  `decodex_project_control` for project status and future-dispatch-only pause/resume;
  standalone scan requests refuse to the operator control loop. Local stdio defaults
  to the `admin` capability profile and can be narrowed with
  `--capability-profile observe|plan|operate|admin`; tool discovery is filtered by the
  active profile and above-profile calls return structured refusals. Stdout must
  contain only valid MCP JSON-RPC messages.
- `decodex intake goal <CONTRACT_ID> --dry-run` is a tracker-read-only and
  runtime-read-only operator surface for promoted Decision Contracts. It renders the
  proposed normal Linear issue split, dependencies, conflict domains, and dispatch plan
  without mutating Linear or persisting Program Intake rows. `--apply` creates or
  updates generated normal Linear issue briefs, links generated issue ids and internal
  node ids back into private runtime records, and persists the local Program Intake
  Plan plus Execution Program state. When the contract came from accepted autonomy
  work, the Program Intake Plan preserves objective, signal, and proposal lineage
  privately so the runtime can replay objective -> signal -> proposal -> Decision
  Contract -> intake -> program -> generated issue links. The Linear description
  stays a natural-language issue brief with objective, public authority summary,
  ownership boundary, acceptance, validation, lifecycle gates, and stop conditions; it
  must not include autonomy signal ids, autonomy proposal ids, Execution Program ids,
  Program node ids, or graph mechanics. It must not start implementation inline and
  must not apply queue labels; direct Program dispatch is performed by the scheduler
  after the Program is persisted.
- `decodex intake issues <ISSUE>... --dry-run` is a tracker-read-only operator
  surface for existing Linear issues. It classifies the supplied batch as ready,
  held, blocked, stale, or unmapped and builds the same internal program model used
  by later persistence, but it must not mutate Linear or write local runtime rows.
  `--apply` writes only local runtime Program Intake and Execution Program state.
- Each scheduler pass evaluates persisted Execution Programs before ordinary queued
  issue selection. The Program scheduler refreshes mapped Linear issue state,
  dependency observations, local shared run claims, retained review/landing
  worktrees, needs-attention labels, and occupied conflict domains; then it directly selects
  dispatchable ready nodes with `program` dispatch mode. It must not apply or remove
  service queue labels, and Program readiness must not wait for the ordinary
  Linear-backed queue scan interval.
- When `decodex run <ISSUE>` targets a mapped Program node, inferred dispatch checks
  the same persisted Program eligibility before ordinary queue-label dispatch. A
  target whose node currently has `dispatch_action = dispatch` starts with `program`
  dispatch mode without requiring `decodex:queued:<service-id>`.

The evidence boundary is ordered from private runtime authority to public collaboration
mirror:

| Surface | Boundary |
| --- | --- |
| Runtime SQLite `private_execution_events` | Structured private execution evidence for the local Decodex installation. This is where full checkpoint payloads, verification notes, local head evidence, recovery detail, and `decodex.harness_outcome/1` feedback records belong. |
| Runtime SQLite `decision_contracts` | Versioned `decodex.decision_contract/1` payloads produced by research/design and later promoted into execution authority. The row status is indexed for local runtime lookup, and the structured runtime payload remains the local machine authority. Checked-in research documentation belongs in Markdown OKF concepts under `docs/research/`, not JSON docs artifacts. |
| Runtime SQLite `autonomy_signals` | Versioned `decodex.autonomy_signal/1` payloads scoped by project and exact Objective Contract id/version. Rows are read-only evidence for future proposal compilation, expose freshness, gaps, contradictions, confidence, and privacy in status readback, and do not mutate tracker state, runtime authority rows outside signal persistence, worktrees, GitHub, Program Intake, proposals, or execution state. |
| Runtime SQLite `autonomy_proposals` | Versioned `decodex.autonomy_proposal/1` dry-run records scoped by project, exact Objective Contract id/version, and referenced signal ids. Rows expose stable evidence-bound proposal identity, objective lineage, source signals, goals, metrics, non-goals, allowed surfaces, validation gates, review and challenge requirements, rejected alternatives, rollback path, contradictions, gaps, refusal reasons, and challenge evidence in readback. Proposal persistence remains non-executable and must not mutate tracker state, GitHub, worktrees, Program Intake, Decision Contracts, or execution state. |
| Runtime SQLite `execution_programs` | Versioned `decodex.execution_program/1` payloads with embedded or linked `decodex.program_intake_plan/1` planning data. They hold internal node lifecycle/readiness, dependency, conflict-domain, dispatch intent, drift, and normal-issue mapping; Linear issue descriptions and ledger comments are only coarse projections. |
| Runtime SQLite `program_intake_plans` | Queryable local projection of `decodex.program_intake_plan/1` metadata, including intake kind, source contract when present, authority fingerprint, and public-safe summary. The paired versioned program payload retains optional private objective/signal/proposal lineage for accepted autonomy-derived intake. |
| Runtime SQLite `program_issue_mappings` | Queryable local projection of each internal program node's mapped Linear issue, tracker state, dispatch intent, active/manual/attention facts, and dispatch-briefing fact. |
| Runtime SQLite `run_control_channels` | Local control capability metadata for active run attempts. It records the project, issue, run id, attempt, transport, local channel path, channel status, and publish/update timestamps needed to route future control requests without bypassing run lease ownership. |
| Runtime SQLite `review_lifecycle_records` | Single authoritative post-review record for one retained PR-backed lane. It stores handoff identity, PR URL, base/head branch, validated head OID, current post-review phase, review-request metadata, landing/closeout/repair state, evidence, and next action. Handoff and orchestration tool-boundary shapes are projections of this record, not separate durable authority. Historical `review_handoffs` and `review_orchestrations` tables are dropped during bootstrap, not migrated or used as readback authority. |
| Runtime SQLite `review_policy_checkpoints` | Current run-attempt projection of bounded-review checkpoint state for one project, issue, run, attempt, and phase. The row supports immediate operator/status readback and remains cleared after review handoff or retained repair completion. |
| Runtime SQLite `evidence_artifacts` | Canonical evidence-keyed artifact store for reusable runtime proofs. Review checkpoints are keyed by artifact kind, phase, current `HEAD`, review level, and review prompt version; matching artifacts are the only review-policy proof accepted by completion and mutation-fence checks, and mismatched keys fail closed. |
| Runtime SQLite `loop_guardrail_checkpoints` | Latest convergence checkpoint for one project, issue, and guardrail reason. It stores the fingerprint, consecutive count, run id, attempt number, and structured detail used to stop non-converging loops without replaying Linear comments. |
| Agent evidence under `~/.codex/decodex/agent-evidence/<service-id>/` | Derived local handoff view for repair agents. It may reference private evidence readback commands and compact run capsules, but it is not scheduling authority and is not a public mirror. |
| Logs under `~/.codex/decodex/logs/` and `.decodex-run-activity` | Diagnostic process and liveness signals. They may explain what a local process did, but they are not the structured execution ledger and must not be replayed as tracker state. |
| Linear execution ledger comments | Low-frequency public projection for team-visible lifecycle state. They carry coarse start, progress phase, PR, handoff, failure, landing, closeout, and cleanup summaries only. |

### Operator snapshot recovery boundary

Operator snapshots are local runtime views. They must remain useful when Linear is unavailable by reading the Decodex runtime SQLite database, retained worktrees, and locally cached connector state that already belong to this machine.

The following facts are local runtime truth and must not be rebuilt from Linear comments on every tick:

- lane attempts: `run_id`, `attempt_number`, attempt status, and terminal classification
- active run-control channel metadata and local control audit events
- protocol events, event counts, event timestamps, and thread/liveness hydration fields
- private execution events carrying structured local evidence for an issue/run/attempt
- authority-boundary-check events that classify whether a loop recovery attempt is
  inside the accepted Authority Envelope, requires human direction, or lacks enough
  evidence to decide
- harness-outcome events correlating Decision Contracts, generated issue or node ids,
  authority-boundary checks, validation/review/repair/manual-attention/PR lifecycle
  outcomes, and private improvement candidates
- review-policy checkpoint state: current phase, normalized status, lane head,
  consecutive non-clean round count, and structured independent-review detail
- loop-guardrail checkpoint state: normalized reason, fingerprint, consecutive
  observation count, source run, and structured stop detail
- retry and backoff state: queued retry kind, due time, retry budget, and connector backoff
- phase timing and operator activity summaries
- retained worktree mappings, review lifecycle records, retained PR handoff identity,
  post-review phase, and cleanup or repair ownership

Linear issue fields and Linear execution ledger comments are the team-visible tracker mirror for low-frequency lifecycle records. They may enrich completed run history when the connector is available, but they must not become the live source for run leases, dispatch ownership, retry/backoff state, phase timing, retained worktree ownership, or operator snapshot continuity.

Operator snapshots must expose lightweight protocol event summaries, not materialized
event journals. Count and latest-event metadata such as `event_count`,
`last_event_type`, and `last_event_at` are dashboard data for liveness and progress
hydration; detailed protocol event history stays in the runtime database. This keeps
concurrent runs from amplifying snapshot size by copying full journals into every
operator-state refresh.

Operator snapshots may also expose autonomy readback derived from runtime rows for
the current/recent status window. That readback is an inspectability projection over
accepted Objective Contract versions, recent signals, proposal states and refusals,
public-safe proposal -> Decision Contract -> Program Intake lineage, and report
metadata. It must carry source refs, freshness, redaction level, completeness, and
known gaps before dashboard, App, or MCP consumers claim autonomy progress. It is not
an audit authority and must omit raw evidence payloads, hidden reasoning, credentials,
and local-only path details.

This boundary does not create a project-local runtime database contract. The runtime store remains the single-machine Decodex SQLite database under `~/.codex/decodex/`, scoped by `project_id`.

## Runtime tuning inputs

- Runtime policy decisions that depend on Codex behavior, such as idle timeout, stall thresholds, retry cutoffs, or liveness heuristics, must not be tuned from local Decodex observation alone.
- For those decisions, use three inputs together:
  - the generated `codex app-server` schema for protocol shape and the current
    protocol support evidence in [`app-server.md`](./app-server.md)
  - live pilot telemetry for observed event cadence and failure modes
  - the relevant Codex or `app-server` implementation path for terminal semantics, waiting states, and progress signals
- If those inputs disagree, treat the local implementation and generated schema as more authoritative than stale design assumptions.
- Do not hardcode a wall-clock budget only because one pilot run happened to exceed or fit within it. Timeout and stall policy should be grounded in upstream runtime behavior first, then tightened with local evidence.

## Core terms

- Issue: One tracker work item visible to the service, usually admitted through the service-scoped `decodex:queued:<service-id>` Linear label derived from the registered project config `service_id`.
- Eligible issue: An issue that currently satisfies the `eligibility` rule in this specification.
- Lease: A local guarantee that only one active `decodex` run is processing a given issue.
- Run attempt: One bounded orchestration pass for one issue.
- Lane: The branch plus linked Git worktree checkout associated with one issue.
- Decision Contract: An accepted loop-runtime decision package, also called the
  Loop/Decision Contract. Research output is only latent until accepted or promoted
  under [`loop-runtime.md`](./loop-runtime.md). The runtime-facing serialized payload
  is `decodex.decision_contract/1`; statuses are `draft_latent`,
  `accepted_promoted`, `rejected_superseded`, and `needs_human_decision`.
- Execution Program: Internal loop-runtime state derived from accepted Decision
  Contracts or accepted issue-batch intake. The durable planning payload is
  `decodex.program_intake_plan/1`, stored with or adjacent to the
  `decodex.execution_program/1` payload. It may use DAG semantics, but normal Linear
  issues remain the executable lanes.
- Authority Envelope: The accepted boundary from the Decision Contract, project
  policy, issue briefing, and explicit user direction. Lane recovery may change
  implementation details inside this envelope, but product goals, accepted behavior,
  lane ownership, objective/non-goal scope, and accepted contract fields require human
  authority when they would change. Public API, config, security, data, billing, and
  privacy surfaces require enhanced evidence before handoff or landing. Validation or
  review-policy weakening blocks landing until the gate is restored.
- Authority Boundary Check: A private execution event with schema
  `decodex.authority_boundary_check/1` and event type `authority_boundary_check`.
  It records issue id, issue identifier, run id, attempt number, Decision Contract ids,
  attempted recovery reason, typed changed surfaces, per-surface and top-level policy
  decisions (`auto_continue`, `requires_enhanced_evidence`, `block_landing`, or
  `requires_human_decision`), policy evidence flags, final legacy disposition, final
  reason, and sanitized harness improvement signals.
- Terminal tracker state: A state that should not be auto-started by `decodex`. The default set is `Done`, `Canceled`, and `Duplicate`.

## Eligibility

An issue is eligible only when all of the following are true:

1. The issue has the automatic intake label `decodex:queued:<service-id>` for the current service.
2. The issue state is in the configured `startable_states`.
3. The issue state is not in the configured terminal states.
4. The issue does not have the opt-out label `decodex:manual-only`.
5. The issue does not have the human-attention label `decodex:needs-attention`.
6. If the issue state is `Todo`, every blocker is already in a configured terminal state.
7. The issue does not already have an active `decodex` lease.
8. For generic normal dispatch, the Linear `description` surface still provides a generic issue briefing rather than only a machine-readable fenced block.

Typical configured `startable_states`:

- `Todo`

Optional future expansion:

- `Backlog`

`In Progress` should not be configured as startable in the normal case. `decodex` should not race human-owned work that is already in progress.

Current runtime note:

- Decodex does not enforce a project-level concurrent-agent cap.
- Active leases are the service-local claim set for running lanes, and shared lock files coordinate cross-process issue ownership and child lease handoff.

## Lane model

- One eligible issue maps to one branch and one linked Git worktree.
- One active run attempt owns the lane at a time.
- The lane path must be deterministic from issue identity so retries reuse the same checkout.
- The runtime owns lane planning, creation, reuse, and cleanup for those linked worktrees.
- The visible lane path lives under the configured worktree root, commonly `.worktrees/<ISSUE>` inside the target repository, while `git_dir` resolves under the repository's shared `.git/worktrees/*` admin area and `git_common_dir` resolves to the repository's primary `.git`.
- Before starting a live run, `decodex` must reject any prepared lane that is not a registered linked Git worktree for the configured repository.
- Worktree mappings and run leases must remain scoped to the registered project `service_id` so reconciliation does not cross project boundaries.

## Runtime state machine

The runtime state machine is local to `decodex`. It is not a replacement for Linear workflow states.

| State | Meaning | Exit conditions |
| --- | --- | --- |
| `discovered` | The issue was listed from Linear and passed the eligibility filter. | Acquire lease or skip on conflict. |
| `leased` | `decodex` created the local lease and reserved the issue for one attempt. | Worktree bootstrap starts or lease fails. |
| `worktree_ready` | The issue lane exists locally and is ready for execution. | `app-server` session starts, or startup transport failure enters the retry budget. |
| `running` | `decodex` has an active `app-server` thread for the issue and may start one or more bounded turns on that thread. | A terminal completion path resolves, the bounded continuation budget is exhausted, the issue becomes non-active, post-thread transport fails, or policy violation occurs. |
| `validating` | Agent execution finished and the repo-native gate (`canonicalize_commands`, then `verify_commands`) is running. | The repo gate passes or fails. |
| `retry_wait` | The control plane is holding a queued retry entry for the leased lane after a clean continuation exit or a failure with remaining retry budget. | The queued retry revalidates and starts, the queued issue becomes non-active and the claim is released, or operator intervention cancels retries. |
| `needs_attention` | Retry budget is exhausted or human intervention is required. | Human updates the issue and it becomes eligible again. |
| `succeeded` | The attempt finished, validations passed, and the success writeback was committed to Linear. | Local cleanup begins. |
| `closed` | Local cleanup finished and the lease is gone. | None. |

After each `app-server` turn completes, `decodex` must resolve one continuation or completion outcome before deciding whether to start another turn on the same thread, enter `validating`, enter `needs_attention`, or yield to a retry path:

- `continue`
  - The turn ended without a terminal tracker path.
  - If the project-owned `execution.max_turns` budget still has room and the issue remains active for the leased lane, `decodex` starts another turn on the same thread and worktree.
  - If the issue is no longer active or the turn budget is exhausted, the worker exits cleanly and the control-plane continuation path decides whether to re-enter later or release the claim.

- `review_handoff`
  - The agent recorded a valid PR-backed review handoff and did not request human attention.
  - `decodex` proceeds into `validating`, then applies the success writeback if the repo gate passes.
- `manual_attention`
  - The agent explicitly requested human attention with the `decodex:needs-attention`
    label intent, left a validated explanatory comment, and did not also record
    review handoff.
  - `decodex` skips success writeback and the post-run repo gate, then enters the human-required failure flow immediately.
- invalid completion signaling
  - If the turn records both signals, or records one terminal path but fails to finalize it explicitly, the attempt is invalid and must fail rather than guessing a completion path.

Phase-scoped Codex goals are a mandatory continuation contract inside this same
bounded-turn model for retained lane runs. Decodex sets one scoped goal for the
active phase:

- `implement_to_validation_ready`
- `repair_validation_failures`
- `repair_accepted_review_findings`
- `review_repair_evidence`
- `handoff_evidence`

Missing required app-server goal methods fail fast through the human-required
unsupported-app-server path. Decodex does not fall back to ordinary continuation
without a phase goal, and project config does not expose a mode to disable the goal
contract.

After a turn completes with an active phase goal, Decodex reads the goal status and
uses it only as a phase signal:

- `complete` on implementation or repair phases triggers Decodex-owned repository
  validation, then a private `phase_acceptance_check`. Validation pass alone is not
  enough to leave implementation or repair: the acceptance check must prove a current
  objective-covering progress checkpoint, a real effective delta, no non-goal
  violation, docs-impact readiness, and repo-gate evidence. A passing acceptance check
  records `validation_pass` and sets the terminal-evidence goal appropriate to the
  lane: `handoff_evidence` for ordinary implementation or validation repair, and
  `review_repair_evidence` for accepted-review repair. A failing acceptance check
  records a `phase_acceptance_check` failure reason, emits `validation_fail` with
  acceptance metadata, keeps accepted-review repair in that phase, or sends
  implementation and validation repair to `repair_validation_failures` for continued
  repair.
- `active`, `paused`, `blocked`, `usageLimited`, or `budgetLimited` do not bypass
  `execution.max_turns`, continuation guard checks, retry backoff, or manual-attention
  policy. If the bounded turn budget is exhausted, the run exits at a continuation
  boundary and the control-plane retry path decides the next re-entry.
- `complete` on `handoff_evidence` or `review_repair_evidence` is valid only when
  the agent also recorded one explicit Decodex terminal path. `handoff_evidence`
  requires `issue_review_handoff` plus `issue_terminal_finalize(path =
  "review_handoff")`, or the manual-attention pair. `review_repair_evidence`
  requires `issue_review_repair_complete` for the retained PR plus
  `issue_terminal_finalize(path = "review_repair")`, or the manual-attention pair.
  Otherwise the turn is invalid and must fail rather than treating goal completion as
  success.

The active phase goal is the authoritative current contract. Before an implementation,
repair, handoff, closeout, or manual-attention path claims completion, the agent must
record a current-HEAD `issue_progress_checkpoint` with `docs_impact` set to `none`,
`update_required`, `research_required`, or `drift_required`. For implementation and
repair phase transitions, Decodex treats that checkpoint as one input to
`phase_acceptance_check`, not as authority by itself. The check records objective
coverage, effective delta, changed surfaces, non-goal status, validation evidence,
decision, reason code, and next action. When an implementation or repair phase has
satisfied its local validation-ready objective, the agent must complete that phase
goal with the Codex goal completion mechanism so Decodex can run the repo gate,
evaluate phase acceptance, and select the next phase. An `issue_progress_checkpoint`,
final chat text, or "await next phase" statement is evidence only; it is not a phase
exit and must not be treated as a substitute for goal completion.

If an app-server run fails, a supervised child exits unsuccessfully, or current-lane
reconciliation finds a stalled retained lane while the latest private phase-goal signal
for that same run is still an `active` implementation or repair phase, Decodex must
run the registered repo gate before converting retained tracked changes into human
attention. When that runtime-owned gate returns a continuation transition, Decodex
records `phase_goal_recovery`, persists the next phase goal (`handoff_evidence` or
`review_repair_evidence` after validation pass, or `repair_validation_failures` after
continued-repair validation failure), marks the attempt as `continuation_pending`,
and schedules normal continuation re-entry. This recovery path does not apply to
terminal-evidence phases such as `handoff_evidence` and `review_repair_evidence`,
explicit manual-attention terminal intent, unsupported phase-goal app-server methods,
repo-gate human-attention failures, runs with no current active phase-goal signal,
authority decision requests, or newer progress checkpoints that record blockers.

App-server JSON-RPC transport failures keep the phase where the disconnect occurred.
Disconnects before a durable thread session is attached (`initialize`,
`account/login/start`, `thread/start`, or `thread/resume`) are startup failures and
must flow through the retry budget before any `decodex:needs-attention` writeback.
If the retry budget is exhausted, the terminal failure still uses
`app_server_transport_disconnected`. Disconnects after the thread session boundary,
including `turn/start` and turn execution, remain human-required because automatic
retry can duplicate turn-side effects.

App-server turn failures with `codexErrorInfo = "usageLimitExceeded"` are runtime
capacity failures, not immediate operator-attention requests. Decodex must stop the
current turn, record a retry with `app_server_usage_limit_exceeded` while retry budget
remains, and let the next attempt re-run account-pool usage probes and account
selection. If the retry budget is exhausted, the same error class becomes a normal
human-required attention stop.

Phase-goal telemetry is local runtime evidence. It must distinguish
`goal_complete`, `validation_pass`, `validation_fail`, `active_goal_recovered`,
review `clean`, review `findings`, terminal `review_handoff`, and terminal
`manual_attention`. Private status and evidence readback may expose the latest
`phase_acceptance_check` summary so an operator can distinguish a repo-gate failure
from a post-gate acceptance failure and see why a phase advanced or stayed in repair.
These signals may appear in private execution events and operator protocol activity,
but Linear receives only the existing low-frequency lifecycle projections.

## Tracker write ownership

- Preferred steady state: the coding agent writes tracker state transitions, comments, and handoff data for the currently leased issue through issue-scoped runtime tools.
- Service-owned tracker writes are reserved for:
  - startup reconciliation
  - crash recovery
  - terminal fallback when the agent never reached the point of writing the tracker
- The service must never grant the coding agent broad tracker write access outside the currently leased issue.
- `decodex` must treat the routed Linear `description` as a generic dispatch briefing surface, not as a plugin-private authority contract. If that surface contains only a machine-readable fenced block with no surrounding briefing text, generic normal dispatch is ineligible until another explicit briefing surface exists.
- Before starting a live run, the service must reconcile stale local leases,
  terminal worktree mappings, and runtime-provenance worktree mappings whose checkout
  path no longer exists and has no active lease, active service label,
  needs-attention label, shared claim, running attempt, or review lifecycle record.
- Generic live dispatch must not require GitHub CLI authority before the lane actually attempts PR-backed review handoff.
- Generic live dispatch must resolve `github.token_env_var` before launching the agent
  app-server so lane-owned `git push` and `gh pr create` commands inherit
  noninteractive GitHub credentials. Git operations must receive routed credentials
  through process-local environment and inline `credential.helper` configuration, not
  per-run askpass helper files under `.worktrees/`. The inline helper must return
  credentials only for HTTPS `github.com` requests and must read the token from the
  routed environment rather than embedding it in Git config. Missing or blank GitHub
  credentials must fail the run through the human-required path instead of retrying or
  leaving a promptable lane running.
- Project configs may set `[github].command_path` to make one expected GitHub CLI binary authoritative for project-scoped GitHub operations. When it is configured, review handoff validation, retained review readback, landing inspection, GitHub comments, admin merge, merge readback, and remote branch cleanup must invoke that path instead of silently rediscovering another `gh` binary.
- The service must fail fast on missing `gh` CLI authority only at the GitHub-dependent review boundary:
  - when a normal lane is about to validate and persist PR-backed review handoff
  - when a retained post-review lane is about to re-enter review repair
  - when a retained closeout lane is about to validate merged PR state or delete the
    retained remote branch ref
- GitHub CLI discovery for those boundaries must use the same resolved `gh` command
  path as PR inspection, including configured project path, normal `PATH` lookup, user
  bin lookup, and the runtime's known local install fallbacks. A valid PR that
  `gh pr view` can inspect with the routed project token must not fail review handoff
  solely because the long-running Decodex process started with a narrower shell path.

## Linear writeback model

### Start writeback

At the start of a normal run, the coding agent should:

1. Acquire the local lease.
2. Transition the issue to `In Progress`.
3. Post the applicable structured `run_started` comment.

The run-start comment is one Linear execution ledger record for new runs. It carries
the branch, repository-relative worktree path, current commit, transport, run id, and
attempt number instead of emitting separate intake, lease, worktree-preparation, and
agent-start comments. Its record envelope, event type, required fields, idempotency
key, and repository-relative `worktree_path` rules are defined by
[`linear-execution-ledger.md`](./linear-execution-ledger.md).

### Completion disposition

Before applying success or failure writeback, `decodex` must classify the finished turn into one and only one terminal completion disposition:

| Disposition | Required agent signal | Forbidden co-signal | Runtime effect |
| --- | --- | --- | --- |
| `review_handoff` | current-HEAD `issue_progress_checkpoint` with `docs_impact`, then `issue_review_handoff` plus `issue_terminal_finalize(path = "review_handoff")` | `decodex:needs-attention` | Run the repo-native gate, revalidate PR state, post completion comment, transition to `In Review`. |
| `manual_attention` | current-HEAD `issue_progress_checkpoint` with `docs_impact`, `issue_label_add` with `decodex:needs-attention` intent, a validated explanatory public issue summary that applies that label, then `issue_terminal_finalize(path = "manual_attention")` | `issue_review_handoff` | Skip PR-backed success writeback and the repo-native gate, then treat the run as a human-required failure immediately. |

If neither signal exists, or both signals exist, `decodex` must fail the attempt instead of inferring operator intent.
If the label intent is recorded without the required explanatory comment, `decodex` must also fail the attempt instead of treating it as a valid `manual_attention` exit.
If the resolved terminal path is not explicitly finalized through `issue_terminal_finalize`, the app-server wrapper must fail the turn before `decodex` records the attempt as successful.
The explanatory public summary for `manual_attention` must describe the exact observed blocker and should include the failed command plus raw error text only when those values are public-safe, instead of speculating about unverified capability limits. It must not reuse runtime-owned retry or continued-repair `error_class` values such as app-server timeout/turn failures, stalled-run detection, or repo-gate validation failure classes; those remain Decodex-owned retry, continuation, or architecture-recovery signals until the runtime itself reaches a human-required terminal boundary.
Pre-existing, repo-wide, or global-baseline repo-gate failures are also runtime-owned
signals. Agents must not wrap them in manual-attention comments with alternate
human-looking classes. The valid path is to preserve private blocker evidence and let
Decodex retain, retry, or isolate the baseline lane using evidence-keyed runtime
state.
Execution-state checkpoints are durable progress overlays only. Their phase, focus, next action, blockers, evidence, or verification fields are never a substitute for the explicit terminal-finalization call.
After successful completion writeback, Decodex must best-effort archive every locally recorded terminal Codex thread for the issue, including earlier failed retry attempts, so old attempts do not keep the issue visible in the Codex conversation list. If Codex reports that a recorded terminal thread is already absent, already archived, or has no rollout to archive, Decodex must record a terminal archive-discard outcome instead of treating the attempt as an indefinitely retryable archive failure.

### Progress checkpoint writeback

`issue_progress_checkpoint` is private-first. Each accepted call appends the full
normalized checkpoint payload to `private_execution_events` in the runtime SQLite
database before attempting any Linear write. The private payload includes phase, focus,
next action, blockers, evidence, verification, resolved lane head, branch,
repository-relative worktree path, and PR URL when present.

Linear receives only the public projection of that checkpoint. The projection is a
`decodex.linear_execution_event` with `event_type = "progress_checkpoint"` and only
allowlisted public fields such as phase, summary, branch, repository-relative worktree
path, and PR URL. Raw checkpoint focus, next action, blockers, evidence, verification,
local head evidence, host-local paths, identity-routing details, account details, and
token names must stay out of Linear.

If the project configures a local public-projection privacy classifier, Decodex applies
that classifier only after the schema-controlled public projection has been rendered.
The classifier receives public projection text fields, not private execution events or
the full checkpoint payload. It is a secondary semantic guard: schema allowlisting and
the deterministic public-text guard remain the primary privacy boundary. Suspicious
classifier verdicts and classifier-unavailable results fail closed by withholding
optional public text or replacing required public text with fixed public-safe fallback
text; private execution-event persistence still succeeds independently of Linear
projection classification.

The local `linear_execution_events` table remains the public mirror cache for rendered
Linear records. It is not the private evidence source. Repeated checkpoint calls that
only change private payload fields must append private execution events but must not
append new Linear comments. A new Linear progress projection is written only when the
material public lifecycle signal changes.

When `decodex` runs the repo-native gate during `validating`, it must preserve the repo-gate failure class instead of collapsing everything into one generic failure bucket:

- `canonicalize_commands` non-zero exit: continued repair in the retained lane unless
  the failed command already wrote tracked files outside the pre-gate lane diff
- `verify_commands` non-zero exit: continued repair in the retained lane
- repo gate command failure after tracked writes outside the pre-gate lane diff:
  retained partial progress that records `repo_gate_scope_envelope_violation`,
  preserves the source repo-gate error class and diagnostic as structured evidence,
  and requires operator attention before any scope expansion or baseline isolation
- repo gate changes only tracked files that were already present in the pre-gate
  implementation diff during phase-goal validation, after the canonicalize and verify
  commands pass: continue to the commit-capable handoff phase and record private
  evidence listing the rewritten files, ownership decision, and continuation reason
- repo gate changes tracked files outside the pre-gate implementation diff, or changes
  tracked files at a lifecycle boundary that requires a clean committed worktree:
  retained partial progress that preserves `repo_gate_tracked_rewrites_left` and
  requires operator attention
- repo-gate command spawn failures or tracked-file cleanliness inspection failures: human-attention failure path immediately

The continued-repair classes above are ordinary bounded churn: the coding agent should keep repairing code and rerun the repo gate rather than requesting `manual_attention` just because the gate has not passed yet. A tracked-rewrite residue after the gate commands have changed the pre-gate implementation diff is different unless every rewritten file was already in the pre-gate implementation diff and the active phase can continue to a commit-capable handoff. Decodex must not infer repository file meaning, generated-artifact ownership, or fixture policy; ownership here is only same tracked path membership in the pre-gate implementation diff. Unsafe or strict-boundary tracked rewrites must preserve the retained worktree, write the retained-progress or scope-envelope class with the repo-gate source class in evidence, and stop until an operator decides whether to finish validation and handoff, expand scope, isolate baseline debt, or reset the patch. Human-attention exits also remain reserved for environment, toolchain, or operator-owned blockers that the coding agent cannot clear from the retained worktree alone.

Continued repair is still bounded by loop guardrails. For each retryable failure,
Decodex records local `loop_guardrail_checkpoint` evidence and updates the
`loop_guardrail_checkpoints` row for any matching convergence reason. Three
consecutive observations with the same fingerprint stop the current ineffective
strategy. Before terminalizing the lane, Decodex records an Architecture Recovery
Packet and an Authority Boundary Check. If the policy decision allows autonomous
recovery and the bounded recovery budget remains, Decodex records
`architecture_recovery_started` and schedules a materially different recovery
strategy. Otherwise it records a terminal recovery reason such as
`contract_boundary_required`,
`external_dependency_required`, or `architecture_recovery_exhausted` and routes the
lane through the human-required failure path. Retryable repo-gate command failures
record both `validation_repeat` and `remaining_delta_unchanged` observations.
`validation_repeat` uses a normalized same-class key for the repo-gate error class
and command/authority domain so changing diagnostics do not reset churn.
Tracked rewrites left by a completed repo gate bypass architecture recovery and are
retained for operator convergence instead. Retryable failures
record `no_effective_diff` only when the retained worktree has no effective source
delta: no tracked status/diff against `HEAD` and no ordinary untracked files after
excluding Decodex runtime artifacts such as `.decodex-run-activity` and
`.decodex-run-control/`. Dirty retained tracked patches and untracked new source files
are retained progress, not no-effective work. Fingerprints use the lane HEAD plus the
effective worktree status and tracked diff against `HEAD`, so retained partial progress
remains inspectable instead of being deleted, hidden, or mislabeled as an empty-diff
retry.

Review handoff is a lifecycle boundary for loop guardrails. If a retryable failure
occurs after Decodex has a retained review lifecycle record for the current issue,
branch, PR, and local HEAD lineage, failure handling must recover the post-review
phase in that lifecycle record and return the lane to the review lifecycle before
recording a new `no_effective_diff` checkpoint or terminalizing the run. If the
worktree has a clean handoff checkpoint for the current head but the retained
lifecycle record is missing or has unverified/diverged lineage, Decodex must classify
the failure as
`review_handoff_state_drift` and require explicit handoff recovery evidence. It must
not send this condition through implementation architecture recovery and must not
mislabel it as ordinary no-effective-diff repair churn.

When `[codex].review` is `"standard"` or `"strict"`, handoff and retained
review-repair runs also consume the latest structured `issue_review_checkpoint`
state for the current phase and current lane head from the owned lane:

- no checkpoint and no terminal path: allow a clean continuation boundary
- latest checkpoint `clean` and no terminal path: allow continuation so the agent can finish handoff or repair completion
- latest checkpoint `findings` where every active `current_blocker` fingerprint has
  been seen fewer than three times in the same phase: allow continuation
- latest checkpoint `findings` where any active `current_blocker` fingerprint has
  reached three repeats in the same phase: treat this as `review_churn`, stop the
  current repair strategy, and run the architecture recovery boundary check before
  either retrying with a materially different implementation strategy or routing to
  the human-required path
- latest checkpoint `needs_architecture_review` or `blocked`: fail the turn through the human-required failure path

`decodex` persists current-run review-policy state in the runtime SQLite
`review_policy_checkpoints` table keyed by project, issue, run, attempt, and phase,
and persists the reusable proof in `evidence_artifacts`. The artifact key includes
artifact kind `issue_review_checkpoint`, phase, current `HEAD`, `[codex].review`
level, and review prompt version. `issue_review_handoff`,
`issue_review_repair_complete`, and review-policy mutation fences require the
matching evidence artifact to be `clean`; a missing or mismatched key fails closed.
The stored checkpoint contains `phase`, `status`, `head_sha`, `nonclean_rounds`, and
`details_json`.
`nonclean_rounds` is the current phase's max active `current_blocker` repeat count;
new current-blocker findings with different fingerprints do not inherit earlier
repeats.
`details_json` holds the structured independent fresh-context review payload,
including checklist notes, accepted findings, rejected findings, repair guidance,
typed `finding_routes`, a compact `finding_route_summary`, and the `finding_policy`
fingerprint ledger. Only accepted findings routed as `current_blocker` populate the
active fingerprint ledger that can trigger review churn. Non-current routes such as
`follow_up`, `risk_note`, `reviewer_rubric_gap`, and
`invalid_or_unsubstantiated` remain durable in the checkpoint payload without driving
repair.
Each accepted checkpoint also appends a private `review_checkpoint` execution event
with the same structured payload for local operator and repair readback. The local
operator status and private evidence readback may expose route counts and one
route-derived next action, but not raw reviewer finding bodies. Linear receives only
coarse lifecycle projections; raw reviewer findings stay in local runtime evidence
unless another allowlisted lifecycle summary renders a public-safe summary.
Operator status must treat the newest matching private `review_checkpoint` event for
the same project, issue, run id, attempt, phase, status, head, and nonclean-round
count as the active loop-review readback before falling back to the per-phase
`review_policy_checkpoints` rows. Older per-phase rows remain repair history, but a
stale `repair` row must not keep `lane_control_next_action` on old finding repair
instructions after a newer current-head clean `handoff` checkpoint has been recorded.
Recording `issue_review_handoff` or `issue_review_repair_complete` clears the current
run-attempt `review_policy_checkpoints` row for that phase after the reusable clean
evidence artifact has been consumed.
When `[codex].review` is `"off"` or `"basic"`, Decodex does not expose
`issue_review_checkpoint`, does not require a clean checkpoint before review handoff
or repair completion, and ignores stale review-policy state while classifying clean
turn boundaries.

The review-policy human-required failure path is also the boundary for any later
runtime-owned research escalation. The current runtime must not dispatch research from
a review stop. Exhausted review findings may enter architecture recovery only as a
review-policy surface with `block_landing`, which permits a materially different
implementation strategy but keeps handoff or landing blocked until review evidence is
restored; `needs_architecture_review` and `blocked` review stops remain
human-required. Future research escalation may only consume structured review-stop
evidence through the adapter contract defined by
[`review-orchestration.md`](./review-orchestration.md).

### Success writeback

This path applies only when the resolved completion disposition is `review_handoff`.

During the run, the coding agent should prepare a PR-backed handoff by:

1. pushing the lane branch
2. creating or updating a non-draft PR for that branch
3. calling the dedicated review handoff tool with the PR URL and a short summary
4. calling `issue_terminal_finalize(path = "review_handoff")`

The handoff tool first records private `review_completion_intent`. Terminal finalize
for `review_handoff` is the local durability boundary: it must revalidate that the
private intent, requested PR URL, retained worktree branch, current local `HEAD`, PR
head ref, PR head OID, and repository/base readback all match, then persist the
authoritative `review_lifecycle_records` row before the tool can return success. An
existing lifecycle row for the same issue and branch must not be silently rebound to a
different PR or head; that requires explicit review-handoff recovery.

After agent execution and post-run validation succeed, `decodex` should:

1. confirm that the recorded PR still belongs to the current repository and branch and that its head commit matches the validated lane HEAD
2. transition the issue to `In Review`
3. post the structured completion comment from the recorded handoff

If a public tracker write fails after terminal finalize wrote the local lifecycle row,
Decodex must leave that row intact for fail-closed recovery and classify the public
writeback failure without guessing PR lineage from branch names, titles, comments, or
stale snapshots.

If the `In Review` transition succeeds but the completion comment fails, `decodex` must stop automatic retries for that attempt and converge the lane through the human-required failure path instead of treating it as retryable work.

Structured review-handoff completion comments are `review_handoff` Linear execution
ledger records. Their required identity, PR, branch, commit, validation, summary, and
idempotency fields are defined by
[`linear-execution-ledger.md`](./linear-execution-ledger.md).

`In Review` is a PR-backed handoff state. Successful runs must not auto-transition directly to `Done`, and generic issue transitions must not move straight into the success state without the recorded PR handoff.

### Failure writeback

This path applies to retryable failures, retry exhaustion, and explicit `manual_attention` exits.
Before writing a retry comment, transitioning an issue, or applying
`decodex:needs-attention`, Decodex must classify the failure through one writeback
disposition: generic retryable failure, structured retryable recovery, or
human-required terminal attention. Structured retryable recovery includes typed runtime
failures such as zero-evidence app-server startup failures, stalled current-lane
reconciliation without retained tracked changes, app-server capability preflight
timeouts, startup transport disconnects, turn failures, dynamic-tool failures, and
retryable repo-gate failures;
those failures must not be reclassified as zero-evidence startup attention merely
because protocol-event persistence lagged. A zero-evidence startup failure may record
private startup diagnostics, but it remains automatic retry work while retry budget
remains and only becomes operator-facing attention after retry exhaustion.
Retry scheduling, terminal writeback, and public `error_class`/`next_action` text must
use that same classification instead of maintaining separate ad hoc failure tables.

Retryable failures with remaining budget:

- Keep the issue in `In Progress`, typically through a runtime-owned retry ledger
  comment.
- Queue the retry in the runtime database rather than immediately redispatching inside the same poll tick.
- Clean worker exits after a nonterminal continuation boundary schedule a short continuation retry.
- Abnormal worker exits schedule exponential backoff capped by `execution.max_retry_backoff_ms`.
- When the queued issue disappears, reaches a terminal state, or otherwise becomes non-active before the retry fires, release the queued claim instead of redispatching it.
- Exception: a Program-dispatched run that fails before effective agent execution,
  has no live lease, has no retained review lifecycle, and leaves no effective
  worktree delta must clear stale active ownership instead of retaining the Program
  conflict domain. The runtime records private cleanup evidence, clears the worktree
  mapping, removes the service active label, and resets the issue to the configured
  failure state when that state is startable so the next Program scheduler pass can
  retry the ready node.

Terminal child-exit preservation:

- Failure retry scheduling is gated by the persisted run-attempt state, not by the final outer child-process status alone. If the persisted attempt is still active or has been recorded as failed when the child exits nonzero, the retry rules above apply.
- If the attempt has already persisted a successful terminal write, that completed run remains authoritative. A later nonzero outer child-process exit is diagnostic only and must not downgrade the attempt or enqueue a failure retry.

Retry-exhausted or human-required failures:

1. Transition the issue to `Todo`.
2. Add the label `decodex:needs-attention`.
3. Post a structured failure comment.
4. Finalize the terminal path with `issue_terminal_finalize(path = "manual_attention")`.

If the coding agent explicitly requests human attention with a
`decodex:needs-attention` label intent and the paired `manual_attention` comment
validates, `decodex` must stop automatic retries for that attempt, skip PR-backed
success writeback, and treat the lane as a human-required failure immediately.
The paired explanatory comment must use the issue-scoped `issue_comment` allowlist,
currently kind `manual_attention`, so the Linear-visible summary is rendered from
structured public fields instead of an arbitrary agent-authored body. Private command
or error details must remain in local runtime evidence when they cannot pass the
public-text guard.
For authority-boundary stops, the same path must include a durable decision request:
the public comment carries the reason code, boundary type, proposed change, why it
exceeds accepted authority, options, recommendation, and resume condition, while the
full `decodex.authority_decision_request/1` packet stays in private execution events
linked to the Authority Boundary Check record. Status JSON and dashboard snapshots
must expose the compact request fields (`phase = human_required`, reason, boundary,
`decision_request_id`, and `next_action`) so the lane is operable without inspecting
SQLite directly.
Operator status JSON and dashboard snapshots also expose compact loop readback for
the same owned run/attempt when available: review level, review phase/status/round,
architecture recovery reason and budget, boundary-check disposition, boundary policy
decision, enhanced-evidence and landing-block flags, and whether the lane is still
autonomous or has crossed into human-required handling. These fields are readback only
and do not replace the runtime review-policy, recovery, or boundary decisions.
Runtime-owned review-policy stops use either bounded architecture recovery or the same
human-required failure path, with dedicated `error_class` values:

- `review_policy_exhausted`: normalized to `review_churn` for the architecture
  recovery boundary check before terminal human attention is considered.
- `architecture_review_required`
- `review_policy_blocked`

Runtime loop guardrails may start bounded architecture recovery before using the
human-required failure path. When recovery cannot proceed, the terminal writeback
preserves a structured failure-attribution `error_class` so operator status and
Linear summaries can distinguish the stop class or recovery boundary:

- `validation_repeat`: the same validation failure repeated three times.
- `no_effective_diff`: retryable attempts repeated without any effective worktree delta.
- `remaining_delta_unchanged`: validation text changed but the remaining effective
  worktree delta stayed unchanged for three attempts.
- `dependency_program_stale`: a queued issue kept the same open dependency blocker
  fingerprint across three status observations, indicating Execution Program readiness
  or issue decomposition is stale.
- `uncovered_direction`: execution found missing direction that must feed back into a
  research or Decision Contract before more implementation.
- `ambiguous_retained_progress`: retained local work or ownership evidence is useful
  but ambiguous enough that a human must choose resume, reset, or manual repair.
- `contract_boundary_required`: recovery would change accepted authority, or evidence
  is insufficient to prove that it would not.
- `external_dependency_required`: recovery depends on a dependency, project policy,
  or Execution Program readiness change outside the current lane.
- `architecture_recovery_exhausted`: the lane already used its bounded autonomous
  architecture recovery budget.

Review repair churn uses the bounded review policy above and may appear publicly as
`review_policy_exhausted` or the normalized loop reason `review_churn`; both mean the
operator should inspect repeated review findings and stop patch-on-patch repair until
the next strategy is explicit.

When a stopped lane is eligible for future autonomous recovery, the recovery worker
must consume the latest Authority Boundary Check or record a fresh one before changing
implementation direction. `requires_human_decision` policy decisions, external/manual
blockers, objective/non-goal changes, unresolved authority evidence, and exhausted
recovery budgets must route through the human-required path or a later accepted
recovery contract. `requires_enhanced_evidence` and `block_landing` decisions are not
ordinary human gates, but they must preserve the evidence or landing block they name
and must not be treated as retryable repo-gate failures.
The supported resume path is deliberate: accept, reject, or revise the requested
authority change in the issue, Decision Contract, or project policy; then clear
`decodex:needs-attention` and requeue or resume through Decodex controls. Direct
tracker mutation, database edits, and internal graph ids are not valid resume
interfaces.

If the configured `decodex:needs-attention` label is unavailable on the team and the configured failure state is startable, `decodex` must still block automatic reselection by leaving the issue in a non-startable guard state such as `In Progress`. In that case the failure comment must explain that the label could not be applied and that a human must move the issue back to a startable state manually after repair. Restart recovery must preserve that guard by writing a retained-worktree marker under `.worktrees/<ISSUE>/.decodex-terminal-guarded` and consulting it before redispatching recovered `In Progress` lanes.

Any issue carrying `decodex:needs-attention` is ineligible for another automatic run until a human clears the label and returns the issue to a startable state.

Structured needs-attention and terminal-failure comments are Linear execution ledger
records. Their required identity, error, next-action, blocker, evidence, terminal-path,
and idempotency fields are defined by
[`linear-execution-ledger.md`](./linear-execution-ledger.md).
The idempotency boundary covers the whole terminal writeback, not only the Linear
comment: once the same `needs_attention` or `terminal_failure` ledger event is already
recorded locally or present in the remote Linear comment ledger, reconciliation or
child-exit recovery must not reapply the failure state transition, automation-label
mutations, or duplicate comment for that logical event.

## Local operational state

The local runtime store is the global Decodex SQLite database for one local installation. It lives at `~/.codex/decodex/runtime.sqlite3`, not inside any registered project checkout or worktree. Every row that belongs to a repo is scoped by `project_id`. Decodex logs live beside that database under `~/.codex/decodex/logs/`, the optional shared Codex account pool lives at `~/.codex/decodex/accounts.jsonl`, global operator config lives at `~/.codex/decodex/config.toml`, bounded local account usage estimates live at `~/.codex/decodex/account-usage-history.jsonl`, and agent-readable derived evidence lives under `~/.codex/decodex/agent-evidence/<service-id>/`; vendor-qualified app-data directories and per-project runtime databases are not part of the runtime contract. Global operator config owns account-pool routing and shared account display-name offsets. The account pool also owns persisted `auth_failed` refresh-authentication state so scheduling does not route new lanes to accounts that must be re-logged or replaced. Account usage history owns local seven-day display estimates and non-secret account capacity weights only; it does not contain token material and does not decide scheduling. UI-only preferences such as theme, table sorting, and local privacy visibility are not runtime state.

Project contracts live outside registered repositories under `~/.codex/decodex/projects/<service-id>/`. Each project directory must contain `project.toml` and `WORKFLOW.md`; arbitrary project file names such as `<service-id>.toml` are not part of the contract. `project.toml` must set `[paths].repo_root` so the project contract is explicit. The `[github]` table owns the routed token environment variable and may also set `command_path` when the expected `gh` binary should be explicit for GUI-launched runs. The `[codex]` table owns app-server-adjacent runtime policy such as `review`. `review` accepts `"off"`, `"basic"`, `"standard"`, and `"strict"` and defaults to `"strict"` when omitted. The `[autonomy]` table owns references for objective-driven project-autonomy policy. It defaults to latent-only with `auto_promote = false` and `auto_intake = false`. `auto_promote = true` requires references to an accepted runtime Objective Contract version and an accepted runtime project-policy authority record; that runtime policy record, not `project.toml`, owns policy id, version, scope, accepted-by metadata, acceptance source, allowed signal kinds, allowed surfaces, validation gates, review policy, explicit cooldown, and explicit write budget. `auto_intake = true` also requires the `team_issue_identifier` tracker anchor while issue creation is allowed. Project config may reference those authority objects, but config presence alone does not grant unattended execution authority and must not embed or replace the accepted authority records. Unknown autonomy config keys, policy bodies, allowed signal kinds, allowed surfaces, validation gates, cooldowns, and write budgets are refused by config parsing rather than treated as latent policy. Phase-scoped goal support is mandatory and is not project-configurable. Project registration stores the centralized `config_path`, target `repo_root`, `worktree_root`, and workflow path in the global runtime database. Commands that start inside a registered checkout or lane worktree resolve the project through that registry; they do not discover or trust worktree-local config files. Project config refreshes preserve an existing enabled or disabled registry toggle; only explicit operator commands such as `decodex project add <project-dir>`, `decodex project enable <service-id>`, and `decodex project disable <service-id>` may change that toggle. `decodex serve` schedules and polls enabled registered projects from the global runtime database; the operator and App projections must still expose active runtime DB-backed attempts for disabled projects because pause is a future-dispatch control, not a visibility or ownership deletion. It must not scan `.codex` history, repo-local config files, or currently open worktrees to infer additional projects.

`project.toml` may also configure `[privacy_classifier]` with a loopback HTTP
`endpoint` and bounded `timeout_ms` for an operator-managed local classifier runtime.
Remote classifier endpoints are invalid. When omitted, the classifier adapter is
disabled and public projections rely on the schema and deterministic guard only.

The runtime database stores at least:

- registered projects and config fingerprints
- run leases and dispatch ownership
- run attempts and attempt status
- protocol event journals
- protocol event summaries used by startup/status readback
- private execution events scoped by project, issue, run, and attempt
- Objective Contracts scoped by project, objective id, and immutable version, with
  lifecycle state and acceptance, rejection, or supersession provenance
- autonomy signals scoped by project, stable signal id, and exact Objective Contract
  id/version, with freshness, evidence class, contradictions, gaps, confidence, and
  privacy retained for operator readback
- Decision Contracts scoped by project and contract id, with optional source issue
  linkage for later issue shaping
- Program Intake Plans and internal Execution Programs scoped by project, with
  lifecycle/readiness state, normal Linear issue mappings, and dispatchable-node
  readback
- worktree mappings
- retained PR and post-review state
- review-policy checkpoints
- loop-guardrail checkpoints
- retry state and retry budgets
- phase timing and operator activity summaries
- tracker and PR cache rows needed to survive connector outages
- typed connector health and external API backoff

For child supervision, the active lane may also carry a short-lived worktree heartbeat marker at `.worktrees/<ISSUE>/.decodex-run-activity`. That marker is advisory, keyed to the current `run_id` plus `attempt_number`, and exists so the control plane can observe child activity across process boundaries, surface active thread and protocol progress in operator status, and keep high-frequency telemetry out of Linear. When the marker records process liveness, it must pair `process_id` with both host boot identity (`host_boot_id`) and process start identity (`process_start_identity`). A marker from a previous boot, a marker missing either identity, a marker whose process start identity no longer matches the live PID, a marker whose PID has exited into an unreaped zombie state, or a marker observed while Decodex cannot read the current host or process identity must not be treated as a live process even if that PID currently exists. Operator snapshots expose `process_liveness_reason` so operators can distinguish stopped processes, previous-boot markers, and same-boot PID reuse from genuine live execution. The marker may also carry additive `child_agent_activity`, protocol, account, and legacy review-policy JSON or scalar fields for operator diagnostics. Legacy review-policy marker fields are breadcrumbs only: review-policy gating belongs to the runtime store and must not be overwritten from marker values. Operator snapshots must keep queue ownership separate from execution liveness: `run_lease` and `queue_lease_state` describe the local queue lease, while `execution_liveness` describes the observed process, app-server thread, or protocol marker that keeps an active lane visible. If a raw attempt is still `starting` after app-server thread, model, or protocol activity is observed, operator-facing `status` must report `running` and preserve the raw value in `attempt_status`. If a raw attempt is already terminal but the matching marker still proves live process, active thread, or active work-protocol execution, operator-facing status must also keep the lane visible as `running` while preserving the raw terminal value in `attempt_status`; terminal maintenance events such as `thread/archive` and completed-turn bookkeeping are not active execution evidence. Only terminal-finalize writeback projections may hide a live marker from active execution. High-frequency heartbeat, child-agent buckets, token counts, idle ages, and other transient liveness details stay local/operator-only under the boundary defined by [`linear-execution-ledger.md`](./linear-execution-ledger.md).
If a persisted attempt has a terminal-looking status such as `failed`, `interrupted`,
or `stalled` while current marker, active thread, or active work-protocol evidence
still identifies the same `run_id` and `attempt_number` as live, operator status must
keep the lane visible with the process/protocol liveness details instead of hiding it
as only terminal history or cleanup work.
For needs-attention recovery, operator status must preserve failed child-run context
separately from parent journal or closeout handling. A dirty retained worktree for the
same issue remains associated with queue-attention or live-thread recovery state when
the tracker has the configured needs-attention label; it must not collapse into a
cleanup-only worktree row with no run id, attempt status, branch, or recovery
explanation.
Operator status lifecycle reconstruction may use recorded run attempts plus local
evidence that is already scoped to the same project, issue, run id, and attempt:
runtime run leases, run-control channels, protocol activity summaries, private
execution events, review-policy checkpoints, operator activity summaries, and
`.decodex-run-activity` marker breadcrumbs. Project-level current-lane recovery must
still satisfy the running-lane visibility boundary above; private execution evidence
or review checkpoints alone may enrich an issue's lifecycle history, but they must not
create a current running lane or queue claim without stronger ownership or execution
evidence. Issue-level lifecycle readback should preserve recovered attempts with their
source evidence and any missing-evidence gaps so operators and agents can investigate
restart or manual-recovery failures without replaying Linear comments as runtime
state. Evidence that cannot be bound to a local project, issue, run id, and attempt is
diagnostic context only, not a synthesized lifecycle attempt.

Post-review ownership is stored in the runtime database. One
`review_lifecycle_records` row records the authoritative PR URL, branch lineage,
validated PR head OID, run id, attempt number, current post-review phase,
review-request metadata, landing/closeout/repair state, evidence, and next action for
the retained lane. If the matching database row is missing, post-review ownership must
block as unresolved instead of rebinding from branch-name, current-head, Linear
comments, or other heuristics. An explicit operator manual takeover command may adopt
a human-owned PR into this same retained database shape only after validating the
managed clean worktree, PR repository, default-branch target, exact branch/head match,
and green landable PR gates. If the active service label is missing but exists on the
issue team, live adopt may restore it after all other invariants pass and must roll
that restoration back if the audit write fails. If a retained lifecycle record exists
but a stored handoff or phase head no longer matches a clean retained worktree and
matching PR head, operator status must keep the lifecycle PR URL visible when known
and recovery diagnosis must report the concrete mismatched field before any explicit
rebind refresh. When the retained lifecycle record still matches the same branch and
PR, and PR readback plus local worktree lineage have already accepted the current head
as the current PR head, a stale lifecycle `head_sha` may be rebound to that current
head by resetting the phase to `request_pending`, clearing prior GitHub Review request
metadata, and preserving round-count history. Branch, PR, handoff-lineage, or
rewritten-history mismatches must continue to block or report for operator recovery
instead of being silently rebound. When a fresh active run owns the same issue,
operator status must project that active execution as the current lane state and mark
the retained post-review lane as shadowed instead of letting stale PR readback drive
current project counts. When retained PR readback degrades but the lifecycle identity
is still safe to preserve, operator-local status may expose a typed
`readback_root_cause` diagnostic such as missing GitHub CLI, missing GitHub token,
GitHub auth failure, API/read failure, parse/shape failure, or lineage validation
failure while keeping public-safe warning reasons such as
`pull_request_state_read_failed` stable. The retained lifecycle controller must
distinguish clean review-repair writeback debt from unresolved review findings. When a
terminalized `review_repair` attempt has a matching repair completion intent for the
current retained worktree branch, PR URL, PR head ref, and local/PR head OID, and the
runtime still has a clean reusable repair checkpoint artifact for that same head, a
missing or stale review lifecycle marker is a typed pending writeback condition
(`review_repair_writeback_missing_lifecycle_marker` or
`review_repair_writeback_stale_lifecycle_marker`). Status may keep the retained lane
in `wait_for_review` while that writeback catches up, but it must not re-project old
review-finding repair instructions or request a new external review from stale
lifecycle state. The retained lifecycle controller must preserve the post-review
classification decision when it converts status readback into runtime action: only a
non-shadowed `Block` classification may write passive retained manual attention or add
`decodex:needs-attention`; degraded readback classified as `WaitForReview` must remain
a wait/retry status row and must not be promoted to manual attention by the run-cycle
path.
The only source-tree runtime artifacts that clean-source checks may ignore are the untracked top-level `.decodex-run-activity` heartbeat marker and `.decodex-run-control/` local control-channel directory. Durable review lifecycle records, review-policy checkpoints, retry, phase timing, and retained PR state belong in the Decodex runtime database, not in root-level or worktree-local review marker files. If the heartbeat marker carries similarly named fields for compatibility or operator diagnostics, those breadcrumb values cannot override runtime-store rows.

### Dispatch-slot handoff invariant

For live execution, project dispatch slots must remain mutually exclusive across concurrent `decodex` processes. The runtime may enforce that exclusion with short-lived worktree-root lock anchors, and control-plane parents may hand those guards to the spawned hidden `_attempt` child so the active lane keeps exclusive ownership even if the parent restarts. Because the runtime contract is Unix-only, that handoff may rely directly on Unix file-descriptor inheritance.

After the hidden `_attempt` child adopts the inherited issue-claim and dispatch-slot file descriptors (FDs):

- The child-owned dispatch-slot FD is the cross-process mutual-exclusion guard for the occupied slot. A competing `decodex` process must still observe that slot as unavailable while the child owns the descriptor.
- The parent must release its process-local issue-claim and dispatch-slot guard handles after the child adopts them. Any parent-side record left for observation or cleanup is bookkeeping only and must not hold an additional dispatch-slot FD or reserve another slot.
- The runtime database lease remains visible while the child owns the run. Releasing parent-local guard handles must not delete, hide, or downgrade the DB-backed run lease that operator status and restart recovery use to identify the running lane.

Restart recovery must use the runtime database plus retained worktrees and external caches instead of replaying Linear comments as the runtime ledger.

## Supported operator visibility surface

`decodex` must expose a supported local visibility surface for current runtime state without requiring operators to read source code or write ad hoc SQL.

The minimum supported surface is:

- structured runtime logs with stable identifiers such as `project_id`, `issue_id`, `issue`, `run_id`, `attempt`, `branch`, and repository-relative `worktree_path`
- a local status command that renders the current service snapshot in both human-readable and JSON forms, including non-secret GitHub CLI authority diagnostics for the resolved command path, discovery tier, configured path when present, availability, and operator next action
- status command output must treat a downstream-closed stdout pipe as normal operator-side output truncation, not as a runtime, database, tracker, or GitHub failure
- an agent evidence command, `decodex diagnose`, that writes a compact derived handoff index, blocker snapshots, run capsules, and an append-only evidence event stream under `~/.codex/decodex/agent-evidence/<service-id>/`; the handoff index includes the same non-secret GitHub CLI authority readback so repair agents can diagnose missing or fallback-only `gh` authority

Structured logs remain diagnostic. They may help explain a live failure, but they are
not the structured private evidence ledger. Private execution events belong in the
runtime SQLite store; Linear execution events remain the constrained public mirror for
coarse lifecycle records.

The status surface should describe runtime DB-backed execution state, plus low-frequency connector refreshes and retained `.worktrees` lanes, for example:

- run-leased runs
- persisted run attempts with local status, thread id, and latest recorded protocol event
- registered project summaries with enabled state, fleet health/capacity counts, connector state, last activity, and retained worktree counts that exclude actively running lane worktrees
- queued tracker issues currently labeled for automatic dispatch, together with the current dispatch classification (`ready`, `claimed`, `blocked`, or `closed`) and any local policy reason that explains why they would or would not run next
- retained worktree mappings
- retained post-review lanes classified as `wait_for_review`, `needs_review_repair`, `ready_to_land`, `continue`, or `blocked`, together with the current PR/check metadata used for that classification

Retained worktree counts and recovery-worktree details must come from one consistent operator snapshot. If the summary count and detail list disagree, surface it as a snapshot consistency warning or bug, not as cleanup work for the operator.

After a process restart, recent-run history, run lease ownership, retained post-review state, and recovery worktree mappings must be reloaded from the runtime database before new work is scheduled. The control plane may refresh low-frequency tracker and PR cache rows, but it must continue publishing local operator state while Linear or GitHub is unavailable.

## Retention and cleanup

- Lease and session mappings: remove when the run closes.
- Attempt records, terminal outcome, private execution events, and locally cached
  Linear execution ledger links remain runtime history. Raw protocol event rows for
  terminal runs may be compacted by `decodex maintenance prune --apply` or by the
  automatic `decodex serve` auto-safe maintenance path once the latest event is at
  least 14 days old, but only after Decodex writes the compact run summary and
  confirms that no run lease, retained worktree, review handoff, review
  orchestration, human-attention ledger event, terminal-failure ledger event, or
  cleanup blocker still owns that run or issue. The first private execution event
  schema has no compaction path; add one only when runtime maintenance owns a
  concrete retention policy for that structured evidence.
- `decodex maintenance prune --dry-run` is the read-only retention path for inspecting
  local cleanup candidates without applying retention changes. The `--apply` mode owns
  state-aware protocol-event
  compaction, old backup pruning, local log and agent-evidence event-stream rotation,
  deletion of rotated local logs and agent-evidence event streams older than 14 days,
  deletion of legacy `.decodex-git-askpass-*.sh` helper files older than one day from
  registered project worktree roots, and SQLite WAL checkpointing. Operators must not
  delete `runtime.sqlite3-wal` directly.
- `decodex serve` runs the auto-safe maintenance subset at startup and periodically
  while polling. That subset may rotate oversized local files, prune old backups,
  delete rotated local logs and agent-evidence event streams older than 14 days,
  delete legacy Git askpass helpers older than one day from registered project
  worktree roots, compact only safe terminal protocol-event rows behind the 14-day
  boundary, and run a passive WAL checkpoint. Current local log files, current
  `events.jsonl` streams, and newer legacy askpass helpers are inputs or protected
  candidates, not age-deletion candidates. If SQLite is busy or protocol-event
  candidate detection fails, the auto-safe path must record a warning and continue
  without blocking scheduler health.
- Worktrees: retain while the issue is non-terminal, and also retain terminal owned lanes while authoritative post-merge closeout or deterministic cleanup is still incomplete.
- Worktree mappings must carry durable local provenance. New runtime-recorded mappings
  use `provenance_source = "runtime_recorded"` with created and updated Unix
  timestamps. Mappings reconstructed from retained tracker, worktree,
  lifecycle-record, or activity-marker evidence after local runtime state is missing
  use `provenance_source = "runtime_recovered"`;
  they are recoverable runtime state, but not proof that the original runtime row was
  still present. Rows migrated from older runtime stores that lack this information
  must remain readable but must be classified as `provenance_source =
  "legacy_unknown"` instead of being silently treated as a fully proven runtime-owned
  lane.
- Terminal issue cleanup: once the issue reaches a terminal tracker state and no authoritative post-merge tail remains pending, remove the worktree during reconciliation or startup cleanup.
- Missing orphan cleanup: when a runtime-recorded or runtime-recovered mapping points
  at a checkout path that no longer exists and no lane ownership signal remains,
  reconciliation must clear the mapping before issue selection so stale local state
  does not occupy Program conflict domains. `legacy_unknown` mappings are excluded
  from this automatic cleanup and remain subject to the explicit legacy closeout audit
  path.
- Terminal identifier residue: when a runtime-recorded mapping uses an identifier-style
  issue id such as `PUB-001`, has no active lease or shared claim, has no retained
  review lifecycle or review checkpoint authority, points at a checkout path that is
  confirmed missing, and its latest local run attempt is terminal, Decodex must
  classify it as local terminal residue before any Linear issue refresh. Reconciliation
  clears the mapping; live status, recovery, post-review readback, and Run Ledger
  hydration skip Linear refresh/comment calls for that identifier and may surface
  `stale_terminal_local_worktree_mapping_ignored`, `stale_terminal_local_residue`, or
  `local_terminal_residue` evidence. Filesystem stat errors are not missing-path
  evidence and must fail closed rather than clear retained runtime authority.
- If an issue becomes non-terminal but no longer eligible while `decodex` is still preparing the lane, keep the worktree and skip execution for that pass.

## Recovery rules

- On service startup, `decodex` must inspect deterministic `.worktrees/<ISSUE>` paths together with tracker issue ids already known from local leases or worktree mappings to rebuild retained worktree mappings before starting new work. This recovery must only write a recovered worktree mapping after tracker, retained lifecycle record, or closeout evidence proves that retry, active-lane, or post-review closeout state owns the worktree; a terminal cleanup-only legacy row must keep `provenance_source = "legacy_unknown"` until the operator runs the explicit legacy closeout audit path.
- If Linear still shows a non-terminal `In Progress` issue and its retained worktree exists locally, `decodex` must treat that lane as a retry-style recovery candidate before selecting fresh `Todo` work.
- Retry recovery must bind retained lanes to issue identity and local runtime state rather than to Linear project membership.
- While the control plane is running an active lane, every poll tick must refresh cached tracker state for the leased issue before considering any new selection.
- While the control plane is running an active lane, that child must keep the workflow snapshot it started with; project `WORKFLOW.md` reloads affect later decisions without restarting the in-flight child.
- While the control plane is supervising an active child process, stall detection must consult the child-updated `.decodex-run-activity` marker for the current `run_id` plus `attempt_number` and the persisted runtime event journal. A retained marker only proves a live process when its PID is still alive on the current host boot and the process start identity still matches; after power loss, reboot, or same-boot PID reuse, recovery must clear the reconstructed lease and re-enter the retained lane through retry-style dispatch instead of preserving the old running state.
- Retry-style recovery prompts must tell the next agent to treat the current worktree, tracker state, runtime-store records, and protocol events as durable truth, use marker files only as diagnostic liveness breadcrumbs, inspect the branch/diff/recent validation evidence first, and continue from partial work rather than assuming prior in-memory model/tool state survived.
- Retained retry-budget markers belong only to the same automatic recovery episode:
  retry, review-repair, closeout, and other retry-style dispatch may inherit the
  marker so crash/restart recovery does not mint extra attempts. Normal queued intake
  starts a new automatic episode after a human has made the issue eligible again, so it
  must not inherit an old retained marker's exhausted retry budget unless a caller
  supplied an explicit preferred retry-budget base for that new run.
- While the control plane owns a queued retry entry, that queued claim must take priority over normal candidate selection for the affected project.
- While the control plane evaluates persisted Execution Programs, ready nodes may be
  selected for `program` dispatch only when their mapped Linear issue is startable,
  non-terminal, briefed for generic dispatch, free of opt-out and needs-attention
  labels, not already active, and not blocked by dependency or occupied
  conflict-domain evidence. Blocked, stale, paused, terminal, active, or
  attention-required nodes stay held, and this path must not mutate service queue
  labels.
- While the control plane is idle between lanes, it may reload the configured project `WORKFLOW.md` on each tick and immediately apply a newly valid document to future dispatch, retry, post-exit reconciliation, and prompt generation.
- If that same configured `WORKFLOW.md` path becomes invalid after a successful load, the control plane must log the reload failure and keep the last known good document active instead of dropping the tick or clearing runtime policy.
- If the leased issue becomes terminal during a control-plane tick, `decodex` must stop the active run, mark the attempt `terminated`, clear the lease, and then retain or clean the worktree according to the retention rules above.
- If the leased issue becomes non-terminal and leaves both the `In Progress` lane state and any configured startable pre-claim state, `decodex` must stop the active run, mark the attempt `interrupted`, clear the lease, and keep the worktree for inspection.
- If a recovered lease is already in `tracker.success_state` and its retained
  review lifecycle record matches the same `run_id` and `attempt_number`, reconciliation
  must mark the local attempt `succeeded` and clear only the lease so deterministic
  retained closeout can reuse the handoff identity.
- Deterministic retained closeout must take its `run_id` and `attempt_number` from
  the durable review lifecycle record or equivalent tracker record, not from a later
  same-process re-entry summary. Later local attempts that did not consume retry
  budget must not force a synthetic closeout attempt number.
- A leased issue that is still in a configured startable state during early control-plane ticks must be treated as a lane that has not finished claiming tracker ownership yet, not as an immediate non-active interruption.
- If a running attempt exceeds the app-server idle timeout, `decodex` must treat it as stalled, stop the active run, and mark the attempt `stalled`.
- If stalled reconciliation finds tracked changes in the retained worktree, it must
  first preserve current runtime ownership. A current retry marker leaves the retry
  scheduler in charge, and a live `repo_gate` operation leaves the repo gate in
  charge. If the lane still has an active implementation or repair phase goal and the
  latest private progress evidence has no blockers or decision request, reconciliation
  must run phase-goal recovery, mark the attempt `continuation_pending`, and schedule
  continuation instead of writing human attention. Only retained tracked changes with
  no current retry owner, no live repo gate, and no applicable phase-goal recovery
  path are classified as retained partial progress with a human-required
  `needs_attention` ledger record using `error_class = "partial_progress_retained"`
  and `terminal_path = "retained_partial_progress"`.
- If stalled reconciliation finds no tracked changes in the retained worktree, it must classify the lane as structured retryable recovery with `error_class = "stalled_run_detected"` while retry budget remains. The retry must keep active ownership, write a failure retry schedule for the same worktree, and must not add `decodex:needs-attention` until retry budget exhaustion or another terminal boundary applies.
- If the supervised child already exited before the next control-plane tick, stalled reconciliation must still inspect the just-finished lane using recorded protocol activity and retained worktree state rather than skipping directly to generic failure handling.
- Operator status snapshots must expose structured liveness and wait-state fields derived from runtime records plus marker breadcrumbs, including explicit `run_phase`, optional wait reason, `current_operation`, optional `active_goal_phase`, optional `public_progress_phase`, last run/protocol/progress times, idle age, a soft `suspected_stall` signal, optional progress diagnostics, and any queued retry kind plus due time, so operators can distinguish active execution from continuation waits, retry backoff, early stall suspicion, and genuine hard stalls without inferring progress from filesystem churn. The snapshot producer owns the derived operator booleans `has_fresh_execution`, `counts_as_running`, and `needs_attention`; dashboard, App, and other UI consumers must use those fields when present instead of reinterpreting raw `process_alive`, thread, protocol, or idle fields independently. The snapshot producer also owns `shadowed_by_current_lane` on retained post-review lanes, and current project review, waiting, attention, landing, and cleanup counts must exclude lanes shadowed by fresh active execution for the same issue. `process_alive = false` is only process-marker evidence and must not be displayed as stopped when `has_fresh_execution = true`. `last_progress_at` is meaningful-work progress only: tool calls, file or diff changes, plan/model output, repo validation, PR/review/terminal lifecycle, or other explicit work events may refresh it, but account, rate-limit, phase-goal, passive status, warning, token-usage, heartbeat, or similar non-work protocol traffic must only refresh protocol liveness. When a lane remains in `model_execution` with fresh protocol activity but stale or missing work progress and the recent protocol events are only non-work traffic, status should expose `progress_diagnostic = "protocol_only_activity"` while preserving process and protocol liveness separately.
- Project-level `waiting_lane_count` is a fleet-summary count of blocked or deferred
  work, not a duplicate of every lane card that has a local wait reason. It counts
  retry backoff, continuation waits, explicit external waits, operator/user-input
  waits, protocol idleness waits, queued waiting candidates, and unshadowed
  wait-for-review lanes. It must not count ordinary fresh execution in
  `model_execution`, `tool_execution`, or repo-gate execution as project-level waiting
  just because the lane has a diagnostic wait reason; those lanes remain running work
  while the lane row may still show the detailed operation and wait/tone fields.
- Operator status must not synthesize a pending Decodex Review checkpoint for every
  ordinary leased running lane. Pending review-checkpoint readback is reserved for
  current checkpoint state bound to the same project, issue, run id, and attempt, or
  for a non-terminal review-writeback operation. Terminal states such as
  `review_handoff_pending` and `review_repair_pending` use terminal lifecycle
  summaries and deterministic wait reasons instead. A normal running lane with no
  review writeback or checkpoint evidence remains policy `allowed` with
  `continue_owned_attempt`.
- Operator status snapshots may expose an additive `child_agent_activity` object when app-server protocol events have produced one for the current run. The object must stay machine-readable and dashboard/CLI shared, and should describe dynamic observed buckets rather than a fixed workflow: current child bucket and elapsed time, bucket wall/event/tool counts, current/max/cumulative input tokens, cumulative output tokens, largest tool output, and warnings for repeated large outputs. Lifecycle metrics that group attempts by run phase must be presented in operator UI/readback as lifecycle buckets, not as generic stages. Missing `child_agent_activity` means no child breakdown was captured; existing JSON consumers must continue to work without it.
- If the agent Git credential preflight fails, operator status must report the retained lane as a credential failure requiring operator recovery, not as a still-running lane.
- If retry budget or needs-attention recovery finds tracked changes in the retained worktree after active phase-goal recovery has no applicable continuation path, operator status must report retained partial progress rather than only a generic retry-budget hold. Retained progress is the recovery disposition; later runtime, app-server, credential, transport, or repo-gate failure classes must be preserved as source evidence instead of overriding the retained-progress lifecycle path. The failure class may be `partial_progress_retained` when no more specific runtime error class is available. Operators should then inspect the patch, finish validation and PR handoff if it is useful, or reset the retained worktree explicitly.
- If active ownership remains after retryable failed-start cleanup but the retained
  worktree has no tracked changes, operator status must identify the condition as
  failed-start cleanup debt, not retained partial progress.
- A retryable runtime or app-server failure that leaves tracked worktree changes must
  keep the owned lane in automatic recovery while retry budget or loop-guardrail
  recovery remains. The retained patch is retry context for the same worktree, not by
  itself a human-attention signal. When that same failure class exhausts retry budget
  or another terminal boundary applies, terminal writeback may classify the retained
  patch as `partial_progress_retained` and preserve the runtime, app-server,
  credential, transport, or repo-gate class as source evidence.
- If a phase-scoped goal reaches `complete` without the matching Decodex terminal
  path, such as handoff evidence finishing without review-handoff, closeout, or
  manual-attention finalization, Decodex must treat
  `phase_goal_terminal_path_missing` as structured retryable recovery while retry
  budget remains. The next attempt re-enters the persisted phase goal and must finish
  the required terminal tool path instead of turning goal completion into issue
  success. Unsupported app-server goal methods remain hard environment blockers.
- Retained post-review orchestration must treat local branch/head readback failures as
  transient wait conditions while the review lifecycle record still owns the lane. Status
  may report `worktree_checkout_branch_read_failed` or `worktree_head_read_failed`, but
  the run-cycle path must not write passive retained manual attention or add
  `decodex:needs-attention` for those read failures alone. A later successful readback
  may still classify hard blockers such as missing branch, branch mismatch, missing
  head, head mismatch, PR mismatch, or another explicit `Block` decision.
- If the durable Run Ledger final outcome is `needs_attention` or
  `terminal_failure`, operator status must count that issue in project-level
  `attention_count` only when a current attention signal still exists: a retained
  worktree, queued attention row, active or needs-attention tracker label, or a
  blocked post-review lane. A retained worktree for that same issue must be projected
  as retained attention, not neutral cleanup-only hygiene, so monitors do not need to
  parse `history_lanes` to discover a human-required terminal outcome. A bare terminal
  Run Ledger attention row with no current owner is history-only ledger evidence; it
  must remain visible in `history_lanes` without inflating current project attention.
  When a non-attention post-review lane currently owns the same issue, such as
  `wait_for_review` or `ready_to_land`, that post-review owner controls the current
  project attention result; any stale active label or retained worktree echo from the
  older terminal ledger must not promote the old Run Ledger outcome back into
  `attention_count`. A real `needs_attention_label`, blocked queued row, or blocked
  post-review classification still counts as current attention.
- If that same issue still carries the service queue label plus the configured
  `needs_attention_label`, the terminal Run Ledger attention outcome must own the
  operator projection. Status must not also render the issue as an intake queue
  candidate, because the queue label is then a stale echo of the retained terminal
  lane rather than dispatchable backlog.
- If Linear still has `decodex:active:<service-id>` on an issue that also remains queued, but the local runtime cannot prove a matching run lease, status must classify the queued row as blocked with reason `linear_active_label_present`; it must not treat the issue as ready intake. If the retained runtime record or private execution event rows for that run are missing, status must surface `evidence_missing` in the recovery details. If the retained worktree has tracked changes, that dirty worktree remains owned by queued recovery/attention instead of being hidden as cleanup-only state.
- Operator status snapshots must expose worktree provenance in both JSON and human text
  output. A cleanup-only worktree with `provenance_source = "legacy_unknown"` must set
  `audit_required = true` and provide a `decodex recover legacy-closeout` next action;
  this is a last-resort operator audit path, not an automatic rebind or cleanup signal.
- During an active run, operator snapshots must expose `thread_id` as soon as the Codex thread exists, plus monotonically advancing `event_count`, `last_event_type`, and `last_event_at` once protocol events are recorded. These fields may be hydrated either from the current process journal or from the active lane's `.decodex-run-activity` marker when `status` is running in a separate process. If both durable run rows and the marker carry protocol summaries for the same run/attempt, the snapshot must prefer the newer summary by protocol timestamp, breaking equal timestamps by larger event count. A stale durable maintenance event such as `thread/archive` must not mask newer current-attempt marker evidence such as model or tool activity.
- `thread_id = null` is expected only before the worker creates the Codex thread for the current run. `event_count = 0`, `last_event_type = null`, and `last_event_at = null` are expected only before the first protocol event for that same run. After the thread exists and protocol activity has started, those empty values indicate missing hydration rather than normal progress.
- Operator snapshots may expose an additive `protocol_activity` object derived from app-server structured messages for the current run. The object stays local/operator-only and should summarize turn status, waiting reason, rate-limit status, and a compact recent event list for high-value app-server activity such as `turn/started`, `turn/completed`, plan updates, diff updates, item start/completion, command output deltas, server request responses, account updates, and rate-limit updates. Missing `protocol_activity` means no structured summary was captured yet; consumers must continue to rely on the older `event_count`, `last_event_type`, `last_event_at`, thread fields, and `child_agent_activity` fields when it is absent. Presence in `protocol_activity` is not by itself meaningful progress; non-work account, rate-limit, phase-goal, passive status, warning, model-routing, and token-usage events must remain distinguishable from work-progress events through `last_progress_at` and `progress_diagnostic`.
- The operator snapshot transport must stay local/operator-only. `decodex serve` exposes the human-facing operator console from the canonical HTTP `GET /` and `GET /dashboard` routes, serves only the necessary dashboard assets, `GET /livez` liveness probe, and local account-control API over HTTP, and delivers published snapshots, current-lane activity, and dashboard control acknowledgements through the local `GET /dashboard/control` WebSocket upgrade.
- A full dashboard snapshot is authoritative for `current_lanes`. Dashboard and
  app consumers may temporarily overlay high-frequency `runActivity` events between
  snapshots, but the next full snapshot must clear any prior live overlay; a full
  snapshot with no current lanes must remove stale lane rows without requiring a
  manual refresh or app restart.
- The MCP stdio and Streamable HTTP transports are separate from the operator
  HTTP/WebSocket transport. They expose the same MCP gateway: resources, templates,
  prompts, schema-bound tools, logging compatibility, progress notifications, and
  capability-profile filtering. Stdio is for local clients, defaults to the `admin`
  capability profile, and must keep stdout valid JSON-RPC only. Streamable HTTP is the
  remote-control-capable transport at `POST /mcp`; it binds to `127.0.0.1:8193` by
  default, defaults to the `observe` profile, validates browser `Origin` headers
  against loopback or explicit trusted origins, issues `Mcp-Session-Id` on
  `initialize`, requires a known session on later requests, returns JSON-RPC JSON by
  default, and uses SSE framing for remote progress or notifications when the client
  accepts `text/event-stream`. `Mcp-Session-Id` is protocol state, not Decodex
  authorization. `--allow-origin` is a browser CORS trust list, not an authentication
  mechanism. Streamable HTTP listeners reachable beyond loopback require both an
  explicit trusted origin and `--bearer-token-env`; Streamable HTTP profiles above
  `observe` require `--bearer-token-env` even on loopback. Decodex validates
  `Authorization: Bearer <token>` for `POST` and `DELETE` requests when this boundary
  is configured, while CORS preflight remains unauthenticated. The built-in bearer
  guard is an equivalent Decodex listener boundary, not OAuth Protected Resource
  Metadata; operators may still place OAuth, relay auth, network ACLs, or reverse
  proxies in front when that boundary is stronger or required by the client.
  Session issuance, session preconditions, and advertised protocol metadata must stay
  isolated from Decodex authority checks so a future final stateless MCP protocol can
  be added without changing tool schemas or lane-control semantics.
- MCP tools must not replace the app-server dynamic tool bridge, create execution
  authority from latent research without promotion, expose Program graph identifiers
  in ordinary tool output, or mutate lanes without the inspect-first run/turn
  authority already enforced by existing lane-control guards. MCP observability
  projections must stay public-safe: no hidden reasoning, raw steer text, private
  evidence payloads, or local path fields. Run-scoped MCP observability resources are
  bounded to runs visible in the current/recent status snapshot and must not construct
  an unbounded historical operator snapshot for a single remote resource read.
- `GET /livez` is only a process- and listener-level liveness probe. It must not claim control-plane tick freshness or forward progress by itself.
- The dashboard must not depend on a separate HTTP snapshot or readiness endpoint; snapshot freshness belongs to the WebSocket-delivered snapshot payload and the browser connection state.
- Reconciliation must mark locally active run attempts as `interrupted` when their
  stale lease is cleared, `terminated` when the tracker issue is already terminal,
  or `succeeded` for the matching recovered review-handoff lease exception above.
- Failed, interrupted, or terminal-guarded retained repair or closeout attempts still
  consume retry budget and require later closeout dispatch to allocate the next
  attempt number instead of reusing the original handoff identity.
- Reconciliation must clear stale leases before the next issue-selection pass.
- When a queued retry becomes due, `decodex` must refresh that exact issue, redispatch it only if it is still active under retry policy, and otherwise release the queued claim.
- Before a prepared lane starts `app-server`, `decodex` must refresh the selected issue once more and skip execution if the issue became terminal or otherwise ineligible.
- After `app-server` initializes and before `thread/start` or `thread/resume`, `decodex`
  must run the bounded app-server capability preflight defined in
  [`app-server.md`](./app-server.md). Missing config/model/provider/skills/plugin/MCP
  state is a pre-dispatch terminal blocker with an operator-readable error class, not a
  promptable agent turn. App-server capability preflight method timeouts are structured
  retryable runtime failures while workflow retry budget remains; after retry
  exhaustion they may terminalize with their specific app-server preflight error class.
- If the local process crashed during a run, `decodex` must recover from the runtime database, current tracker cache or state, and retained worktree inspection.
- If Linear shows a non-terminal state but no local lease exists, the issue may become eligible again after reconciliation or may be redispatched through the retained recovered worktree.

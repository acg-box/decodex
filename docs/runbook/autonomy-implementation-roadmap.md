---
type: "Runbook"
title: "Autonomy Implementation Roadmap"
description: "Defines the implementation sequence for objective-driven Decodex autonomy."
status: active
authority: procedural
owner: runtime
tags: [runbook, autonomy, objective, roadmap]
code_refs:
  - apps/decodex/src/autonomy_objective.rs
  - apps/decodex/src/autonomy_signal.rs
  - apps/decodex/src/autonomy_proposal.rs
  - apps/decodex/src/loop_contract.rs
  - apps/decodex/src/config.rs
  - apps/decodex/src/mcp.rs
  - apps/decodex/src/program_intake.rs
  - apps/decodex/src/orchestrator/status/mod.rs
  - apps/decodex/src/state/store.rs
related:
  - ../spec/autonomy-control-plane.md
  - ../decisions/project-autonomy-control-plane.md
  - ../spec/loop-runtime.md
  - ../spec/runtime.md
  - ./research-to-execution-loop.md
drift_watch:
  - decodex.autonomy_objective/1
  - decodex.autonomy_signal/1
  - decodex.autonomy_proposal/1
  - allowed_signal_kinds
  - Program Intake
  - decodex mcp serve
last_verified: 2026-06-27
---
# Autonomy Implementation Roadmap

Purpose: Provide the implementation sequence for objective-driven Decodex autonomy.
Read this when: You are turning the autonomy spec into code, issues, or Program
Intake work.
Not this document: The normative authority model; read
[`../spec/autonomy-control-plane.md`](../spec/autonomy-control-plane.md).
Defines: Phase order, deliverables, validation gates, stop conditions, and the
minimal safe dogfood path.

## Roadmap Principle

Implement autonomy from authority outward:

1. Define authority.
2. Persist read-only evidence.
3. Compile dry-run proposals.
4. Require explicit acceptance.
5. Reuse Program Intake and normal lanes.
6. Add operator readback.
7. Expose MCP mutation last.

Do not start with MCP mutating tools, runtime anomaly heuristics, dashboard actions,
or a memory adapter. Those are consumers of the authority model, not the foundation.

## Phase 0: Promote The Design

Goal: Land the spec, decision, and roadmap before executable changes.

Deliverables:

- `docs/spec/autonomy-control-plane.md`
- `docs/decisions/project-autonomy-control-plane.md`
- `docs/runbook/autonomy-implementation-roadmap.md`
- index, README, example-config, and docs-log updates

Validation:

```sh
cargo make check-docs
git diff --check
```

Completion criteria:

- Docs gate passes.
- The old anomaly-only branch is not treated as implementation authority.
- The roadmap explicitly says code should be ported from current `main`, not rebased
  from the old anomaly branch.

Self-evaluation before Phase 1 may be queued:

- Record evidence that the autonomy docs are merged or otherwise available from the
  active implementation base.
- Record evidence that old anomaly-only implementation code was not imported.
- Record evidence that the Objective Contract authority boundary is specific enough
  for implementation.
- Record evidence that no implementation work started from a signal, report, memory
  readback, or anomaly finding without accepted authority.

## Phase Gate Rule

Each later phase must finish with explicit validation evidence and a self-evaluation
record before the next phase is queued. The self-evaluation must compare the phase
output against the active Objective Contract, accepted non-goals, authority boundary,
validation gates, review policy, and stop conditions. If validation fails, evidence is
stale, authority is missing, or self-evaluation finds that execution began from a
signal or report without accepted authority, the next phase must not be queued.

## Phase 1: Objective Contract Authority

Goal: Add Objective Contract storage and readback without automatic execution.

Implementation surfaces:

- `apps/decodex/src/autonomy_objective.rs` for `decodex.autonomy_objective/1`.
- `apps/decodex/src/loop_contract.rs` remains the separate Decision Contract model.
- `apps/decodex/src/state/store.rs` and `apps/decodex/src/state/internal.rs` for
  persisted objective records.
- `apps/decodex/src/config.rs` for project policy references.
- CLI readback or draft/accept commands only if they remain explicit.

Deliverables:

- Versioned Objective Contract payload with immutable versions.
- Explicit lifecycle state representation as row state, payload state, or both, with
  readback values for `draft`, `accepted`, `rejected`, and `superseded`.
- Store APIs to create draft, accept version, read current accepted version, record
  rejection/supersession provenance, and list objective history.
- Project config may reference accepted objective and policy record ids, but config
  presence alone does not grant unattended execution authority and must not embed
  policy bodies, allowed signal kinds, allowed surfaces, validation gates, cooldown,
  or write budget.
- Unknown config keys remain rejected under existing config policy.

Required tests:

```sh
cargo test -p decodex autonomy_objective --lib
cargo test -p decodex config --lib
cargo make fmt-check
cargo make check-docs
```

Stop conditions:

- Objective acceptance can be inferred from chat history only.
- Draft and accepted lifecycle is implicit instead of represented as row state,
  payload state, or both.
- Objective mutation rewrites older versions.
- Project config alone enables automatic promotion or intake.

## Phase 2: Read-Only Signal Ledger

Goal: Persist signals as evidence only.

Implementation surfaces:

- `apps/decodex/src/autonomy_signal.rs` owns the versioned
  `decodex.autonomy_signal/1` payload, fingerprint, validation rules, and first
  dogfood builders.
- Runtime state store for `decodex.autonomy_signal/1`.
- Signal builders for the first dogfood adapters:
  - `runtime_health`
  - `validation_regression`
  - `review_feedback_cluster`
  - `user_feedback_cluster`
  - `spec_drift`
  - `protocol_drift`
  - `execution_friction`
  - `docs_skill_drift`
- Operator/status readback for recent signals.
- Store APIs that record one signal, read one signal, list signals by exact
  Objective Contract id/version, and list recent project signals for status readback.

Deliverables:

- Signal schema with objective id/version, source refs, freshness, evidence class,
  contradictions, gaps, confidence, and privacy.
- Dedupe fingerprint that excludes volatile counts and timestamps.
- Review signal ingestion uses `finding_routes` and current-head review evidence.
- Memory-derived signal ingestion requires source refs and stays proposal-only.
- Runtime/status readback exposes recent signal freshness, gaps, contradictions,
  confidence, and privacy without treating signals as execution authority.

Required tests:

```sh
cargo test -p decodex autonomy_signal --lib
cargo test -p decodex review --lib
cargo make fmt-check
cargo make check-docs
```

Stop conditions:

- A signal can mutate tracker, runtime DB authority rows, worktrees, or GitHub.
- Raw review comments bypass `finding_routes`.
- A memory or report signal lacks primary source refs.

## Phase 3: Proposal Dry-Run Compiler

Goal: Compile objective-bound signal clusters into non-executable proposals.

Implementation surfaces:

- Proposal compiler module.
- CLI dry-run surface such as `decodex autonomy propose --dry-run` if the command
  shape is accepted.
- Operator readback for proposal state.

Deliverables:

- `decodex.autonomy_proposal/1` with states `draft`, `needs_evidence`,
  `needs_human_decision`, `rejected`, `decision_candidate`, and
  `accepted_promoted`.
- Stable proposal id from objective, sorted signals, affected identifiers, source
  family, and intended surface.
- Refusal rules for missing objective, disallowed signal kind, disallowed surface,
  stale evidence, unresolved contradiction, and weakened validation or review.
- Challenge hook that records subagent or inline skeptic objections as evidence
  and promotion constraints, not automatic blockers.

Required tests:

```sh
cargo test -p decodex autonomy_proposal --lib
cargo test -p decodex loop_contract --lib
cargo make fmt-check
cargo make check-docs
```

Stop conditions:

- Proposal persistence grants execution authority.
- Proposal ids include timestamps or volatile runtime counts.
- Skeptic output is treated as acceptance authority.

## Phase 4: Explicit Acceptance And Decision Contract Bridge

Goal: Convert only accepted proposals into normal latent Decision Contract
candidates, then use existing promotion.

Implementation surfaces:

- Mapping from proposal to `decodex.decision_contract/1`.
- CLI or MCP plan-profile accept/promote path with explicit authority fields.
- State transitions from proposal to Decision Contract candidate.

Deliverables:

- Accepted proposal generates a latent Decision Contract candidate that preserves
  objective lineage, signals, contradictions, gaps, validation gates, and review
  policy.
- Existing `research_promote` and Decision Contract promotion semantics remain the
  authority boundary.
- Proposal acceptance cannot be performed by the same external agent that submitted
  the proposal unless project policy explicitly accepts that actor as policy
  authority.

Required tests:

```sh
cargo test -p decodex autonomy_decision_bridge --lib
cargo test -p decodex program_intake --lib
cargo make fmt-check
cargo make check-docs
```

Stop conditions:

- Accepted proposal bypasses Decision Contract status.
- Proposal directly creates issues or Program Intake rows.
- External agent output becomes its own acceptance authority.

## Phase 5: Program Intake Mapping

Goal: Reuse existing Program Intake and normal issue lanes for execution.

Implementation surfaces:

- `apps/decodex/src/program_intake.rs`.
- Runtime mapping rows linking objective, proposal, Decision Contract, Program
  Intake Plan, Execution Program, and generated issues.
- Existing scheduler readiness evaluation.

Deliverables:

- Generated tracker-backed work has normal cold-start issue briefs.
- Program graph ids remain internal.
- Service queue labels are not used as Program scheduler authority.
- Existing validation, review, landing, closeout, and cleanup gates remain required.

Required tests:

```sh
cargo test -p decodex program_intake --lib
cargo test -p decodex orchestrator --lib
cargo make fmt-check
cargo make check-docs
```

Stop conditions:

- Generated issue description is only a machine-readable block.
- Program node ids or internal graph details appear in ordinary public issue text.
- Program Intake skips review, landing, install, restart, or closeout gates.

## Phase 6: Operator And App Readback

Goal: Make autonomy inspectable before expanding write surfaces.

Implementation surfaces:

- Operator snapshot/status projection.
- App API and dashboard readback only after runtime status can prove freshness.
- Evidence commands for objective, signal, proposal, and mapping lineage.

Deliverables:

- Current objective version is visible.
- Recent signals and proposal states are visible.
- Proposal refusal reasons are visible.
- Links from objective -> signal -> proposal -> Decision Contract -> Program Intake
  are inspectable.
- Reports are derived query views with source refs, redaction, completeness, and
  known gaps.

Required tests:

```sh
cargo test -p decodex operator --lib
cargo test -p decodex status --lib
cargo make fmt-check
cargo make check-docs
```

Stop conditions:

- Dashboard displays autonomy progress without source refs or freshness.
- Report output becomes audit authority.
- Remote-safe projections expose hidden reasoning, raw evidence payloads, local
  paths, or credentials.

## Phase 7: MCP And External-Agent Surface

Goal: Expose the autonomy interface to Codex, external agents, and CI bots without
weakening authority.

Implementation surfaces:

- `apps/decodex/src/mcp.rs`.
- Decodex plugin skills that route agents to resources, prompts, and tools.
- Plugin surface tests and MCP smoke tests.

Deliverables:

- Observe profile can read objective, signal, proposal, and evidence summaries.
- Plan profile can draft and accept objectives, submit signals, compile proposals,
  challenge proposals, and request explicit promotion surfaces.
- Operate/admin profiles do not gain new bypasses; lane control and project control
  keep existing guards.
- External agents cannot accept their own proposals unless accepted policy authority
  explicitly says so.

Required tests:

```sh
cargo test -p decodex mcp --lib
cargo test -p decodex plugin_surface_tests --lib
cargo make fmt-check
cargo make check-docs
```

Stop conditions:

- MCP auth or capability profile is treated as acceptance authority.
- MCP exposes broad raw command mutation.
- Skills duplicate long specs instead of routing to docs/MCP resources.

## Phase 8: Self-Dogfood And Production Gate

Goal: Apply autonomy to Decodex itself before enabling it broadly for other projects.

Dogfood objective:

- Reduce repeated operator intervention.
- Detect protocol, docs, skill, review, and runtime drift earlier.
- Convert repeated feedback into proposals with evidence.
- Preserve engineering quality by keeping validation and review gates strict.

Dogfood signals:

- `runtime_health`
- `protocol_drift`
- `review_feedback_cluster`
- `execution_friction`
- `docs_skill_drift`
- `validation_regression`
- `user_feedback_cluster`

Production readiness requires:

- Objective Contract storage and versioning tested.
- Signal ledger tested as read-only evidence.
- Proposal compiler tested with refusal cases.
- Decision bridge tested with accepted and rejected paths.
- Program Intake mapping tested without queue-label fallback.
- Operator readback tested for freshness and privacy.
- MCP surface tested for observe/plan refusal and mutation authority.
- One Decodex self-dogfood loop produces a proposal, gets accepted, enters normal
  execution, records PR handoff, validation, and post-land evidence under the same
  replay chain, and only lands, installs, restarts, or syncs plugins through normal
  lifecycle authority.
- PR handoff replay evidence must be corroborated by retained review lifecycle
  readback and a matching replay-evidence pointer for the same proposal or Decision
  Contract, run, attempt, PR URL, PR head ref, and PR head oid. A stale PR row for
  the same generated issue or same PR URL must not satisfy the replay chain.

Recommended full gate before broad enablement:

```sh
cargo make fmt-check
cargo make check-docs
cargo test -p decodex --lib
cargo make check
```

Stop conditions:

- Any autonomy path can land, install, restart, or sync plugins without normal
  lifecycle authority.
- Any autonomy path weakens validation, review, or landing gates.
- The first dogfood result cannot be replayed from objective, signal, proposal,
  Decision Contract, Program Intake, PR, validation, and post-land evidence.
- Replay evidence is only a report artifact or raw private payload rather than a
  public-safe operator/MCP readback projection tied back to generated issue links.
- PR replay evidence is inferred from issue-level lifecycle rows without a matching
  proposal or Decision Contract pointer and matching PR head identity.

## Branch Strategy

Use a fresh branch from current `origin/main` for implementation. Do not rebase the
old anomaly-only branch into the final work. Port only durable docs/design intent.

When implementation starts, delete old anomaly-only names and tests in the same
change that introduces the new Objective Contract, signal, and proposal authority.
Do not keep `allowed_anomaly_kinds` compatibility aliases unless an external
persisted migration explicitly requires them.

## Completion Checklist

- Objective Contract exists and is immutable.
- Project policy references accepted objective and policy authority.
- Signals are evidence only.
- Proposals are evidence only until accepted.
- Proposal acceptance maps into Decision Contract authority.
- Execution reuses Program Intake and normal issue lanes.
- Operator readback proves lineage and freshness.
- MCP action matrix is enforced.
- Memory adapters are read-only context.
- Decodex self-dogfood passes one full loop before broad enablement.

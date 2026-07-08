---
type: "Spec"
title: "Loop Runtime Specification"
description: "Define Decodex Decision Contract, Program Intake, and phase-goal behavior above individual issue lanes."
status: active
authority: normative
owner: runtime
tags: [spec]
code_refs: [apps/decodex/src/orchestrator/lane_decision.rs, apps/decodex/src/orchestrator/execution.rs, apps/decodex/src/orchestrator/execution_phase_goal.rs, apps/decodex/src/orchestrator/prompting.rs, apps/decodex/src/agent/tracker_tool_bridge/tools.rs, apps/decodex/src/autonomy_proposal.rs, apps/decodex/src/loop_contract.rs, apps/decodex/src/execution_program.rs, apps/decodex/src/program_intake.rs]
drift_watch: [lane_decision, continuation_lineage, phase_goal, validation_evidence, docs_impact, review_contract, issue_review_checkpoint, decodex.autonomy_proposal/1, decodex.decision_contract/1, execution_program, decodex intake goal]
last_verified: 2026-07-07
---

# Loop Runtime Specification

Purpose: Define the Decodex runtime contract that turns accepted decisions into normal
issue-lane execution.

Read this when: You are implementing or reviewing Decision Contract storage, Program
Intake, Execution Program readiness, phase-scoped Codex goals, unattended dispatch, or
loop guardrails.

Not this document: Generic repository investigation methods, team knowledge-base
maintenance, external best-practice comparison, or plugin authoring rules. Those live
in external installed team plugins.

## Boundary

Decodex is a runtime and operator control plane. It may store accepted planning
payloads and materialize them into normal Linear issue work, but it does not own the
generic team investigation workflow that produces candidate recommendations.

External team workflows may produce input for Decodex. Decodex runtime authority starts
only when an accepted `decodex.decision_contract/1` payload, accepted Objective
Contract, or accepted project-policy record exists in trusted runtime state.

## Decision Contract

A Decision Contract is the runtime-local planning payload for work that may later be
materialized into executable issues.

Required boundaries:

- `draft_latent` records a candidate only. It cannot enqueue work, mutate tracker
  state, set goals, create worktrees, or dispatch lanes.
- `accepted_promoted` records explicit accepted authority. Only this status may feed
  Program Intake.
- `needs_human_decision` records that a human must decide before issue shaping.
- `rejected_superseded` records that the payload must not become executable work.

Accepted contracts must preserve:

- accepted objectives and non-goals
- constraints, assumptions, objections, and stop conditions
- validation expectations and risk notes
- structured `proposed_issues[]`
- conflict domains
- acceptance metadata: actor, actor kind, timestamp, source, and reason when present

The runtime must not infer acceptance from a summary, prompt, local file, MCP auth
profile, project config body, or caller-supplied policy object.

## Program Intake

Program Intake materializes accepted work into private Decodex runtime state and
public-safe issue briefs.

Supported intake kinds:

- `goal_intake`: materialize an accepted Decision Contract.
- `issue_batch_intake`: materialize supplied existing issue briefs.

The operator CLI surface is:

```sh
decodex intake goal --project <service-id> <CONTRACT_ID> --dry-run
decodex intake goal --project <service-id> <CONTRACT_ID> --apply
decodex intake issues --project <service-id> <ISSUE>... --dry-run
decodex intake issues --project <service-id> <ISSUE>... --apply
```

Dry-run must not mutate Linear, Program Intake rows, Execution Program rows, or issue
mappings. Apply may persist Program Intake and Execution Program state only after
explicit authority is present.

Program Intake must keep graph mechanics private. Generated issue briefs may describe
objectives, dependencies, validation, risks, and acceptance criteria, but must not
expose internal node ids, graph ids, proposal ids, private evidence paths, or local
runtime rows.

## Execution Program

An Execution Program is a private runtime plan made of normal work nodes. Nodes may be
ready, held, blocked, running, completed, failed, or skipped according to dependency,
conflict-domain, tracker, workflow, and lease state.

Executable stages are current-state hints for dispatch and reporting:

- `design`
- `spec`
- `schema`
- `runtime`
- `plugin`
- `eval`
- `handoff`

The scheduler may dispatch ready nodes directly through normal issue lanes. Queue
labels are not the Program scheduler.

## Phase Goals

Decodex may set a phase-scoped Codex goal for a lane. A phase goal narrows the current
turn without changing the full issue lifecycle.

Required behavior:

- The lane must treat the active phase goal as the current turn contract.
- Validation-ready phases stop after validated local work and complete the active
  Codex goal.
- Handoff phases create or update the PR only after local validation and review
  evidence.
- Review-repair phases repair accepted current-head findings and preserve retained PR
  lineage.
- Public tracker comments must remain public-safe and must not expose local-only
  runtime evidence.

## Stop Rules

Decodex must stop instead of widening authority when:

- the Decision Contract is not accepted
- project-policy authority cannot be resolved from trusted runtime state
- proposed issues have unresolved dependencies or missing validation expectations
- a node would touch a disallowed surface
- review or validation requirements would be weakened
- the runtime would need to expose private graph or evidence identifiers publicly

When a stop is issue-local, record the blocker in the normal lane evidence path. When
it is a runtime or baseline blocker, keep it in private runtime evidence and avoid
misclassifying it as human manual attention.

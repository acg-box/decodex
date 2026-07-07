---
type: "Decision"
title: "Natural-Language Loop Runtime"
description: "Should Decodex expose execution graphs as the user workflow, or keep graph"
status: active
authority: rationale
owner: docs
tags: [decision]
last_verified: 2026-06-16
---
# Natural-Language Loop Runtime

Status: accepted
Date: 2026-06-09
Question: Should Decodex expose execution graphs as the user workflow, or keep graph
semantics internal behind a natural-language loop runtime?
Decision: Decodex should be a natural-language-first loop runtime. Accepted decisions
become runtime-local Decision Contracts, Decision Contracts materialize into an
internal Execution Program, and normal Linear issues remain the executable Decodex
lanes.

## Context

The current Decodex runtime already owns issue eligibility, retained worktrees,
tracker writes, validation gates, review handoff, retained review repair, landing,
closeout, and operator status. A loop-engineering layer needs dependency, ordering,
conflict-domain, and drift semantics, but exposing those mechanics directly would make
ordinary use more complicated than the existing Codex conversation workflow.

The intended everyday flow is:

1. The user discusses work in Codex conversation and may use external team workflows
   for investigation.
2. Accepted direction is recorded as a Decision Contract.
3. The user accepts the direction or asks Decodex to arrange or push it forward.
4. Decodex materializes accepted decisions into internal execution state and normal Linear
   lanes.

## Decision

Decodex keeps graph semantics backstage.

- The user-facing surface stays natural language.
- Generic investigation lives outside Decodex runtime ownership.
- Accepted decisions become a Decision Contract.
- The loop runtime derives an internal Execution Program with DAG semantics such as
  objective lineage, dependencies, stage, conflict domain, acceptance criteria, queue
  intent, ready-node selection, and drift handling.
- Normal Linear issues remain the executable Decodex lanes.
- Phase-scoped Codex goals are allowed; one giant "finish issue" goal is not.
- Goal completion triggers validation or review. It does not prove lane completion.
- Self-review is cheap smoke. Completion depends on deterministic validation and,
  where risk warrants, independent fresh-context read-only review.
- Long unattended execution must stop affected branches for contract or architecture
  decisions when execution discovers uncovered direction, while continuing independent
  ready nodes.

The normative contract lives in [`../spec/loop-runtime.md`](../spec/loop-runtime.md).

## Rejected Alternative

The rejected alternative is a user-visible DAG workflow with graph ids, explicit edge
editing, dry-run/apply/status mechanics, or direct manipulation of Codex goal state as
the ordinary interface.

That design would expose implementation machinery to users before it creates leverage.
It would also duplicate the existing Linear lane and Decodex runtime contracts instead
of letting the runtime use those surfaces as execution adapters.

## Consequences

- Future runtime lanes must treat loop graph state as internal runtime state.
- Documentation and operator UI should not teach ordinary users to drive Decodex by
  graph ids or DAG commands.
- New execution-program code must bridge into existing Linear issue lanes rather than
  replacing them.
- External investigation adapters must preserve an acceptance boundary before queueing
  or implementation starts.
- Loop stop conditions must route to failure attribution, accepted decision updates,
  architecture review, or manual attention instead of infinite patching.
- Harness telemetry should improve prompts, skills, validators, issue templates, and
  loop policy without retroactively changing accepted lane contracts.

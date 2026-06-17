---
type: "Runbook"
title: "Research-To-Execution Loop"
description: "Operate and inspect the natural-language research-to-execution loop without turning latent research into automatic execution."
status: active
authority: procedural
owner: automation
tags: [runbook]
last_verified: 2026-06-16
---
# Research-To-Execution Loop

Purpose: Operate and inspect the natural-language research-to-execution loop without
turning latent research into automatic execution.

Read this when: A user asks Decodex to research something, promotes the result, or
needs to inspect why only part of the resulting work is dispatchable.

Not this document: The normative schema or runtime invariants. Use
[`../spec/loop-runtime.md`](../spec/loop-runtime.md) for the authority contract.

Covers: Latent research contracts, promotion, Execution Program dispatch readiness,
phase-goal validation, independent review, guardrails, and harness improvement
evidence.

## Preconditions

- The project is registered and has a service id.
- The operator can inspect private local Decodex evidence for the project.
- Promotion is a human or runtime-policy decision; latent research output is not
  execution authority.

## Operator Path

1. Compile the research result into a latent contract.

   ```sh
   decodex research compile --intent "research X"
   ```

   Inspect the contract id and status. The expected status is `draft_latent`.
   This step must not enqueue Linear issues, mutate trackers, set phase goals, or
   authorize implementation.

2. Promote only an accepted result.

   ```sh
   decodex research promote <CONTRACT_ID>
   ```

   Promotion records the accepted Decision Contract. It authorizes the runtime to
   shape an internal Execution Program, but it is still not a request to dispatch
   every possible node.

3. Inspect Execution Program readiness.

   ```sh
   decodex status --json
   ```

   Check the execution-program summary for ready, blocked, paused, active, and
   completed counts. Only ready nodes mapped to startable issues, without active,
   opt-out, needs-attention, terminal-state, dependency, or conflict blockers become
   directly dispatchable by the Program scheduler. Program readiness must not apply,
   retain, remove, or depend on `decodex:queued:decodex`.

4. Treat phase-goal completion as a validation boundary.

   A child goal reaching `complete` means Decodex should run the registered repo gate
   and move to validation, review, repair, handoff evidence, or manual attention. It
   does not mean the issue is terminally complete.

5. Follow review and guardrail outcomes.

   Accepted independent-review findings route to repair. Three non-clean review
   rounds stop the current repair strategy instead of continuing patch churn.
   Repeated validation failures do the same. Engineering convergence failures may
   continue only through autonomous architecture recovery after the Authority
   Boundary Check is `within_authority` and recovery budget remains; `blocked`,
   `needs_architecture_review`, external blockers, insufficient evidence, or exhausted
   recovery budget become human-required stops.

6. Preserve uncovered direction as research feedback.

   If execution discovers direction not covered by the accepted contract, pause the
   affected branch, record Decision Contract feedback, and allow unrelated ready
   nodes to continue when their dependencies and conflict domains permit it.

7. Inspect private harness feedback locally.

   ```sh
   decodex evidence <ISSUE> --run-id <RUN_ID> --attempt <N> --json
   ```

   Review improvement candidates in the summarized evidence. Use raw payload
   readback only for local debugging, and do not paste private execution payloads into
   public tracker comments or PR descriptions.

## Dogfood Assessment

The loop is meaningfully better than the old external research to manual Linear issue
flow when the lane evidence shows all of the following:

- research output stays latent until promotion;
- promotion creates an internal source of truth for execution shape;
- only ready mapped nodes become directly dispatchable by the Program scheduler;
- child-goal completion triggers validation and review instead of terminal success;
- repair churn has bounded guardrails;
- harness telemetry produces at least one concrete fixture, prompt, or contract
  improvement recommendation.

Remaining gaps should be recorded with the lane evidence. Common gaps are missing
native issue-generation ergonomics, too little scenario coverage for a new branch of
the architecture, or operator-owned promotion decisions that still require manual
judgment.

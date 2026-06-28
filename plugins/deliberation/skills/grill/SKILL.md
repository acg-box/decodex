---
name: grill
description: Use when unclear intent, architecture, product framing, option boundaries, hidden assumptions, or overconfident implementation plans need concise decision discovery before execution.
---

# Grill

Pressure-test the shape of the work before execution. Grill exposes the few questions
or constraints that would change the implementation or decision; it is not a long
questionnaire. Read `../../references/deliberation-gate.md` when the gate applies.

## Rules

- Start from repo/docs/runtime evidence when available; do not ask for facts that can
  be read.
- Use first-principles framing: goal, real constraints, non-goals, smallest viable
  path, and falsifier.
- Challenge owner module, acceptance, validation, rollback, migration, docs impact,
  and whether a smaller implementation satisfies the objective.
- Ask at most the smallest human-only question needed; otherwise state a safe
  assumption and continue.
- For substantial plans, use `$deliberation:skeptic` after the grill.
- Do not mutate state, create execution authority, or replace verification.

## Output

Return the decision question, assumptions, unresolved material questions, non-goals,
and next implementation or research owner.

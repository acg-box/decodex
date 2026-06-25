---
name: grill
description: Use when unclear intent, architecture, product framing, option boundaries, hidden assumptions, or overconfident implementation plans need a concise decision-discovery grill before execution.
---

# Grill

Pressure-test the shape of the work before execution. Grill is not a tutorial and not
a long questionnaire; it exists to expose the few questions or constraints that would
change the implementation or decision.

Read `../../references/deliberation-gate.md` when design, architecture, refactor,
root-cause debugging, research framing, option comparison, or important ready/done
claims are in scope.

## Rules

- Start from existing repo authority and knowledge when available; do not grill the
  user for facts that can be read from the repository.
- Ask at most the smallest human-only question needed. If a reasonable assumption is
  safe, state it and continue.
- Use first-principles framing: goal, real constraints, non-goals, smallest viable
  path, and falsifier. Treat historical implementation details as claims to verify,
  not automatic constraints.
- Challenge boundaries: owner module, acceptance, validation, rollback, migration,
  docs impact, and whether a smaller implementation would satisfy the objective.
- For substantial plans, use `$deliberation:challenge` after grill output if the
  claim still needs adversarial review.
- Do not create execution authority, mutate files, or replace verification.

## Output

Return the decision question, assumptions to proceed with, material unresolved
questions, non-goals, and the next implementation or research owner.

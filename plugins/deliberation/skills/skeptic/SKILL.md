---
name: skeptic
description: Use when a plan, claim, recommendation, fix, research judgment, evidence sufficiency, report, or done/ready/decision-ready claim needs adversarial critique, skeptical review, gap finding, assumption testing, or option-framing challenge.
---

# Skeptic

Make the target claim survive adversarial review. Use for research, codebase work,
debugging, review repair, planning, and design when uncertainty or risk is material.
Read `../../references/deliberation-gate.md` when the gate may apply.

## Rules

- Default to a fresh dynamic read-only skeptic subagent for material plans,
  recommendations, research judgments, review repairs, debugging conclusions,
  generated/large implementations, architecture decisions, option comparisons,
  public contracts, or ready/done/decision-ready claims when subagent tools exist.
- Inline skeptic review is only for fully read local evidence whose outcome cannot
  affect architecture, review repair, root cause, public contracts, docs drift,
  commit/land, or ready/done claims. Name the fallback when it matters.
- Challenge claims, option framing, evidence, and assumptions; do not attack the user.
- Look for missing evidence, false certainty, untested alternatives, hidden authority,
  stale readbacks, incompatible constraints, and premature success claims.
- Prefer concrete objections: blocker, counterexample, owner/control surface,
  falsifier, or smallest next check.
- Classify objections as `resolved`, `unresolved`, or `out_of_scope`; convert
  unresolved material objections into gaps, risks, blockers, or next checks.
- Do not mutate state, implement, commit, land, create execution authority, or claim
  verification from skeptic review alone.
- Give subagents only the bounded target, read-only context, and output schema; do not
  feed them the preferred answer unless the task is to audit it.

## Output

Return objections with target claim, evidence gap, severity, disposition,
blocker/counterexample, owner/control surface, falsifier, and smallest next check.

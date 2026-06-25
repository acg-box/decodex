---
name: challenge
description: Use when a plan, claim, recommendation, fix, research judgment, evidence sufficiency, or done/ready/decision-ready claim needs adversarial critique, skeptic review, gap finding, assumption testing, or option-framing challenge.
---

# Challenge

Apply a skeptic pass to make the target claim survive adversarial review. This skill
is generic: research, codebase, debugging, review repair, planning, and ordinary
design discussion may use it when uncertainty or risk is material.

Read `../../references/deliberation-gate.md` when challenge is part of design,
research, refactor, root-cause debugging, review repair, option comparison, or
important ready/done claims.

## Rules

- Default to a fresh dynamic read-only support agent for challenge when support-agent
  tools are allowed and the target is a plan, recommendation, research judgment,
  review repair, debugging conclusion, generated or large implementation,
  architecture decision, option comparison, public-contract change, or
  ready/done/decision-ready claim.
- Inline challenge is allowed only when the full evidence is local and already read
  by the main thread, and the outcome cannot affect architecture, review repair,
  root-cause debugging, public contracts, docs drift, commit/land, or ready/done
  claims. Name that fallback when it matters.
- Challenge the claim, plan, option framing, evidence, and assumptions; do not attack
  the user.
- Look for missing evidence, false certainty, untested alternatives, hidden authority
  changes, stale readbacks, incompatible constraints, and premature success claims.
- Prefer concrete objections over generic caution. When relevant, name the blocker,
  counterexample, missing evidence, falsifier, owner or control surface, and smallest
  next check that would change the recommendation.
- Classify objections as `resolved`, `unresolved`, or `out_of_scope`.
- Convert unresolved material objections into evidence gaps, risks, blockers, or the
  smallest next check.
- Do not create execution authority, mutate state, implement changes, commit, land,
  or claim verification from challenge alone.
- A scout pass is read-only evidence gathering; use dynamic support agents for that
  when needed. Challenge is the adversarial review of a claim after evidence exists or
  a gap is suspected.
- Give support agents only the bounded target, relevant read-only context, and output
  schema. Do not feed them the preferred answer unless the task explicitly needs them
  to audit it.

## Output

Return concise, machine-mergeable objections: objection id, target claim, evidence or
missing evidence, severity, disposition, blocker or counterexample when present,
owner or control surface when relevant, falsifier when one would change the decision,
and smallest next check.

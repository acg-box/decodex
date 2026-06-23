---
name: challenge
description: Use when a plan, claim, recommendation, fix, research judgment, evidence sufficiency, or done/ready/decision-ready claim needs adversarial critique, skeptic review, gap finding, assumption testing, or option-framing challenge.
---

# Challenge

Apply a skeptic pass to make the target claim survive adversarial review. This skill
is generic: research, codebase, debugging, review repair, planning, and ordinary
design discussion may use it when uncertainty or risk is material.

## Rules

- For non-trivial plans, research judgments, review repair, generated or large
  implementation, and ready/decision-ready claims, default to a fresh dynamic
  read-only support agent for the skeptic pass when tool support exists. Inline
  challenge is a fallback for small/local checks or unavailable support-agent tools;
  name that fallback when it matters.
- Challenge the claim, plan, option framing, evidence, and assumptions; do not attack
  the user.
- Look for missing evidence, false certainty, untested alternatives, hidden authority
  changes, stale readbacks, incompatible constraints, and premature success claims.
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
missing evidence, severity, disposition, and smallest next check.

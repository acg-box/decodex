---
name: research-decision
description: Use to finalize a Decodex research run as decision_ready, not_decision_ready, blocked, or needs_human_decision and prepare the latent Decision Contract boundary.
---

# Decodex Research Decision

## Goal

End every bounded Decodex research run with one clear status. The result is a latent
Decision Contract candidate unless and until promotion occurs.

## Outcome Gate

Use exactly one terminal outcome:

- `decision_ready`: evidence, option comparison, resolved challenge, accepted
  objectives, validation expectations, and proposed issue summaries are sufficient for
  issue shaping after promotion. No unresolved decisions, evidence gaps, or blockers
  remain.
- `not_decision_ready`: useful evidence exists, but the decision would be unsafe or
  under-supported. Preserve missing evidence and next research needed.
- `blocked`: the research pass cannot proceed until a non-decision blocker is removed.
- `needs_human_decision`: the remaining uncertainty is a human/product/authority choice
  rather than a research gap.

## Decision Contract Checklist

Before `decision_ready`, verify:

- the decision question and falsifiers were framed
- every material claim has evidence
- realistic options were compared
- skeptic objections were addressed or classified
- the chosen boundary preserves user intent and Decodex authority rules
- validation expectations are concrete
- proposed issue summaries are scoped and non-overlapping
- the output is still latent and does not execute by itself

## Boundaries

- Do not produce multiple terminal statuses.
- Do not choose `decision_ready` because the budget ended. Use
  `not_decision_ready`, `blocked`, or `needs_human_decision` when the gate is not met.
- Do not promote the contract in this skill. Promotion is a separate authority step.

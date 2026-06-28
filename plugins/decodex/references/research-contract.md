# Decodex Research Contract

Use this for terminal Decision Contract shape.

## Status

Use exactly one:

- `decision_ready`: evidence, options, resolved skeptic objections, objectives, validation,
  and promotion target are sufficient for post-promotion shaping.
- `not_decision_ready`: useful evidence exists, but a decision would be unsafe.
- `blocked`: a non-decision blocker prevents more research.
- `needs_human_decision`: remaining uncertainty is a human/product/authority choice.

Never use `decision_ready` because budget ended.

## Required Content

- source intent, decision question, owner, output shape, non-goals, useful bounds
- terminal status; evidence ledger and provenance
- realistic options including status quo, tradeoffs, selected decision or explicit
  non-decision
- facts, inferences, decision impact, smallest next checks
- assumptions, constraints, objections, gaps, blockers, stop conditions
- operational gates: owner, validation/release check, rollback/freeze path, falsifier
- benchmark/regression gate when benchmark-driven or changing skill/plugin behavior
- promotion target, docs impact, OKF disposition
- research-only evidence versus durable knowledge candidates

OKF disposition: `continue`, `promote_and_supersede`, `promote_and_retire`, or
`reject_or_deprecate`.

## Judgment

Skeptic-ready judgment names criteria fit, evidence refs, impact, rejected
alternatives, unresolved gaps, expected validation, and owner/control surface when
execution would follow. Objections are `resolved`, `unresolved`, or `out_of_scope`;
unresolved material objections block `decision_ready`.

Durable accepted facts, proofs, contracts, and workflow instructions move after
acceptance to owner targets: decisions, specs, references, runbooks, evidence,
skills, code, or tests. Persisted research stays Markdown OKF, not JSON event logs.

# Decodex Research Contract

Use this reference for the terminal Decision Contract shape.

## Terminal Status

Use exactly one:

- `decision_ready`: evidence, options, resolved challenge, objectives, validation,
  and promotion target are sufficient for post-promotion shaping.
- `not_decision_ready`: useful evidence exists, but a decision would be unsafe.
- `blocked`: research cannot proceed until a non-decision blocker is removed.
- `needs_human_decision`: remaining uncertainty is a human, product, or authority
  choice.

Never use `decision_ready` because budget ended.

## Required Sections

Expose these from the top-level contract or Markdown research concept:

- source intent and decision question
- terminal decision status
- evidence ledger and provenance
- realistic options and tradeoffs
- selected decision or explicit non-decision
- assumptions, constraints, non-goals, objections, and stop conditions
- validation expectations
- promotion target
- docs impact
- unresolved decisions, evidence gaps, or blockers

## Option And Judgment Rules

Compare realistic choices, including status quo, minimal patch, redesign, staged
migration, and no-go/defer when relevant.

A challenge-ready judgment names:

- selected option or non-decision
- criteria fit
- evidence refs
- assumptions and constraints
- rejected alternatives
- unresolved gaps
- expected validation

Challenge objections are `resolved`, `unresolved`, or `out_of_scope`. Unresolved
material objections block `decision_ready`.

## Docs Form

If persisted under `docs/research/`, use a Markdown OKF `Research Contract` concept.
Do not write JSON research event logs.

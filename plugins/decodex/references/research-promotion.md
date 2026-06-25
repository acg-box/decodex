# Decodex Research Promotion

Use when accepted research becomes execution authority.

## Acceptance

Promotion requires explicit acceptance. Preserve objectives, non-goals, constraints,
assumptions, objections, validation expectations, proposed issues, and stop
conditions. Refuse unresolved decisions, gaps, or blockers.

## Durable Lanes

| Target | Accepted research defines |
| --- | --- |
| `docs/spec/` | correctness, schema, invariant, state, required behavior |
| `docs/runbook/` | operator sequence |
| `docs/reference/` | current implementation or repository structure |
| `docs/decisions/` | durable rationale, rejected alternatives, tradeoff |
| `docs/evidence/` | reusable proof, public-safe evidence, drift audit |
| runtime code/tests | behavior not representable by docs or skills |

`docs/research/` is latent provenance, not a promotion target.

Promotion is owned with `$knowledge:docs`/`$knowledge:okf`: move durable rationale,
current truth, reusable proof, and workflow instructions to owners; leave only
unresolved inquiry or superseded provenance in research. Update OKF/LLM Wiki indexes,
relationships, descriptions, and status fields so research does not compete with
authoritative owners.

End the research concept as `continue`, `promote_and_supersede`,
`promote_and_retire`, or `reject_or_deprecate`. Use `no_promotion` only when no
durable rationale, fact, proof, instruction, code, or test expectation changes.

When accepted research changes agent-facing workflow instructions, update the owning
plugin skill beside promoted docs. After promotion, route execution to `planning`.

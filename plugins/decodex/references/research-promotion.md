# Decodex Research Promotion

Use this reference when accepted research becomes execution authority.

## Acceptance

Promotion requires explicit acceptance of a research result. Do not infer acceptance
from a research request, summary, or old artifact.

Before promotion:

- identify the accepted contract
- preserve objectives, non-goals, constraints, assumptions, objections, validation
  expectations, structured proposed issues, and stop conditions
- refuse promotion while unresolved decisions, evidence gaps, or blockers remain

## Durable Lanes

Choose the narrowest durable lane:

| Target | Use when accepted research defines |
| --- | --- |
| `docs/spec/` | correctness, schema, invariant, state, or required behavior |
| `docs/runbook/` | operator sequence |
| `docs/reference/` | current implementation or repository structure |
| `docs/decisions/` | durable rationale, rejected alternatives, tradeoff |
| runtime code/tests | behavior that cannot be represented by docs or skills alone |

`docs/research/` is not a promotion target. It is the latent research lane.

When accepted research changes agent-facing workflow instructions, update matching
`plugins/decodex/skills/` files beside the promoted docs concept. Skills are
companion execution surfaces, not `promotes_to` lanes.

## Next Step

After promotion, route accepted execution work to `planning`. Program Intake may then
persist Execution Program readiness and dispatch ready mapped nodes directly.

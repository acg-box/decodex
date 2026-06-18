# Decodex Research Promotion

Use this reference when accepted research becomes execution authority.

## Acceptance

Promotion requires explicit acceptance; do not infer it from a request, summary, or old
artifact. Identify the accepted contract, preserve objectives, non-goals, constraints,
assumptions, objections, validation expectations, proposed issues, and stop
conditions, and refuse unresolved decisions, gaps, or blockers.

## Durable Lanes

Choose the narrowest durable lane:

| Target | Use when accepted research defines |
| --- | --- |
| `docs/spec/` | correctness, schema, invariant, state, or required behavior |
| `docs/runbook/` | operator sequence |
| `docs/reference/` | current implementation or repository structure |
| `docs/decisions/` | durable rationale, rejected alternatives, tradeoff |
| runtime code/tests | behavior not representable by docs or skills |

`docs/research/` is not a promotion target. It is the latent research lane.

Promotion splits by authority: research-only evidence and provenance stay in
research; accepted durable knowledge (facts, policies, specs, runbooks, repository
structure, workflow instructions, implementation contracts) moves to durable owners.
Use `no_promotion` only when no durable fact, contract, instruction, code, or test
expectation changes. Durable owners state current truth independently and may link
back for rationale.

When accepted research changes agent-facing workflow instructions, update matching
`plugins/decodex/skills/` files beside the promoted docs concept. Skills are
companion execution surfaces, not `promotes_to` lanes.

## Next Step

After promotion, route execution to `planning`; Program Intake dispatches ready mapped
nodes.

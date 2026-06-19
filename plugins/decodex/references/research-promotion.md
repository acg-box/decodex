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
| `docs/evidence/` | reusable proof, public-safe evidence, or drift audit |
| runtime code/tests | behavior not representable by docs or skills |

`docs/research/` is not a promotion target. It is the latent research lane.

Promotion is a knowledge operation owned with `$knowledge:docs`/`$knowledge:okf`:

- move durable rationale to `docs/decisions/`
- move current truth to `docs/spec/`, `docs/reference/`, `docs/runbook/`, skills,
  code, or tests
- move reusable proof to `docs/evidence/`
- leave only unresolved inquiry or superseded provenance in `docs/research/`

End the research concept as `continue`, `promote_and_supersede`,
`promote_and_retire`, or `reject_or_deprecate`. Use `no_promotion` only when no
durable rationale, fact, proof, instruction, code, or test expectation changes.
Durable owners state current truth independently and may link back for rationale.

Update LLM Wiki indexes, `related`, `promotes_to`, descriptions, and status fields so
research does not compete with authoritative owners. Do not rely on out-of-band
history for knowledge retention.

When accepted research changes agent-facing workflow instructions, update the owning
plugin skills beside the promoted docs concept. Skills are companion execution
surfaces, not `promotes_to` lanes.

## Next Step

After promotion, route execution to `planning`; Program Intake dispatches ready mapped
nodes.

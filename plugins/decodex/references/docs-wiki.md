# Decodex Docs Wiki Reference

Use this reference when routing, indexing, linking, or deduplicating repository
knowledge.

## Reading Order

1. `README.md`
2. `docs/index.md`
3. `docs/policy.md`
4. The smallest lane index and owning concept for the task

Do not read a broad docs lane when one concept owns the changed claim.

## Lane Ownership

| Lane | Owns |
| --- | --- |
| `docs/spec/` | Required behavior, schemas, invariants, states, validation contracts. |
| `docs/runbook/` | Operator procedures and execution sequences. |
| `docs/reference/` | Current structure, implementation maps, concept explanations. |
| `docs/decisions/` | Durable rationale, rejected alternatives, tradeoffs. |
| `docs/research/` | Latent research concepts and supporting evidence candidates. |
| `docs/evidence/` | Reusable public-safe proof concepts, including durable drift audits. |

## Authoring Rules

- Keep one authoritative concept per claim.
- Link instead of copying repeated claims.
- Update lane indexes when concepts are added, renamed, moved, deprecated, or
  superseded.
- Use `related` frontmatter when cross-links materially help retrieval.
- Start each concept with a short routing purpose and boundary.
- Put implementation truth in `docs/spec/` or code references, not in narrative
  summaries.
- Keep `docs/research/` non-authoritative until promotion.

## Maintenance Log

Update `docs/log.md` when a lane changes routing, promotion, naming, docs policy, or
knowledge maintenance behavior.

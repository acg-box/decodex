# Decodex Docs Wiki Reference

Use this for Decodex `docs/` navigation, indexing, linking, and deduplication. Use
`okf-layer.md` for portable OKF/LLM Wiki bundles.

## Reading Order

Read `README.md`, `docs/index.md`, `docs/policy.md`, then the smallest lane index and
owning concept. Do not read a broad lane when one concept owns the changed claim.

## Lane Ownership

Use `spec` for requirements, `runbook` for procedures, `reference` for current
structure, `decisions` for rationale, `research` for latent candidates, and
`evidence` for public-safe proof or drift audits.

## Authoring Rules

- Keep one authoritative concept per claim.
- Link instead of copying repeated claims.
- Update lane indexes when concepts are added, renamed, moved, deprecated, or
  superseded.
- Use `related` when cross-links materially help navigation.
- Start each concept with a short purpose and boundary.
- Keep `docs/research/` non-authoritative until promotion.
- After promotion, update indexes, links, descriptions, and statuses so superseded
  research routes as provenance instead of competing with owner concepts.

Update `docs/log.md` when navigation, promotion, naming, docs policy, or maintenance
behavior changes.

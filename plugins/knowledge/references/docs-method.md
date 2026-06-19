# Docs Method

Use this when work changes repository documentation, documented behavior, or
agent-facing docs workflow.

## Contract

`docs/` is this repository's strict OKF bundle. `docs/index.md` routes,
`docs/policy.md` owns shape/lanes/gates, `docs/log.md` records maintenance, and
non-index, non-log Markdown files are typed concepts. Research is latent until
promoted; drift audits can block completion.

Do not create a parallel `wiki/` or `okf/` root when the repository already defines a
docs owner. Portable OKF bundles use `$knowledge:okf` and do not inherit repository
runtime workflow or docs-impact gates.

## Lifecycle

1. Read `docs/index.md`, `docs/policy.md`, and the owning concept.
2. Update the owner; do not duplicate claims.
3. Add `code_refs`, `drift_watch`, or drift audit evidence for behavior changes.
4. Update indexes and `docs/log.md` for routing, naming, or promotion changes.
5. Run `decodex docs check`.

## Docs Impact

- `none`: no docs, command, behavior, config, status, or workflow claim changed.
- `update_required`: update a durable concept in the lane.
- `research_required`: switch to the owning research workflow.
- `drift_required`: create or update drift audit evidence.

`validation-ready` includes docs readiness.

## Routing

- `docs-okf.md`: frontmatter/Markdown checks.
- `docs-wiki.md`: placement, indexes, links, deduplication.
- `docs-drift.md`: docs/code/evidence audits.

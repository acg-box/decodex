# Portable OKF Layer

Use this for Open Knowledge Format bundles, LLM Wiki retrieval, and
cross-repository knowledge-base maintenance.

## Boundary

OKF is the portable bundle format: Markdown files, YAML frontmatter, required
`type`, and ordinary Markdown links. LLM Wiki is the agent method on top: route to
small context, maintain links/indexes/logs, and keep producer-specific fields intact.

Decodex docs are only one strict profile. Other repositories do not inherit Decodex
lanes, Linear workflow, research promotion, docs-impact checkpoints, or landing
policy.

## Profiles

- `core`: portable OKF v0.1 conformance.
- `wiki`: core plus indexes, retrieval fields, and graph hygiene.
- `repo-memory`: wiki plus `source_refs`, `code_refs`, `related`, and `drift_watch`.
- `decodex`: repo-memory plus Decodex lanes, authority, research, and drift gates.

Use the lowest profile that proves the claim. Consumers should still return partial
results when a stricter profile fails.

## Commands

Use `decodex okf check/find/graph/route <root>` for portable bundles.
`decodex docs` defaults to root `docs/` and profile `decodex`. Do not create or
recommend `decodex docs okf ...`; OKF is the engine, not a docs subcommand.

## Rules

Producers pick a profile first, keep concepts one-topic, use frontmatter for routing,
link real relationships, update `index.md` or `log.md` when navigation changes, and
preserve unknown fields.

Consumers start with `decodex okf route` or `decodex okf find`, use
`decodex okf graph` for relationships, tolerate unknown `type` values, and use
Decodex `docs-*` skills only for this repository's strict `docs/` profile.

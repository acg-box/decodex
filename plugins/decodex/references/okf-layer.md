# Portable OKF Layer

Use this for portable OKF bundles, LLM Wiki retrieval, and cross-repository memory.

## Boundary

OKF is the portable bundle format: Markdown, YAML frontmatter, required `type`, and
ordinary links. LLM Wiki is the agent method on top: route small context, maintain
links/indexes/logs, and preserve producer fields.

Decodex docs are only one strict profile. Other repositories do not inherit Decodex
lanes, Linear workflow, research promotion, docs-impact checkpoints, or landing
policy.

## Profiles

- `core`: portable OKF conformance.
- `wiki`: core plus indexes, retrieval fields, and graph hygiene.
- `repo-memory`: wiki plus `source_refs`, `code_refs`, `related`, and `drift_watch`.
- `decodex`: repo-memory plus Decodex lanes, authority, research, and drift gates.

Use the lowest profile that proves the claim.

## Commands

Use `decodex okf check`, `decodex okf find`, `decodex okf graph`, and
`decodex okf route` for portable bundles. `decodex docs` defaults to root `docs/`
and profile `decodex`. Do not create or recommend `decodex docs okf ...`; OKF is
the engine, not a docs subcommand.

## Rules

Producers pick a profile first, keep concepts one-topic, use frontmatter for
routing, link real relationships, update `index.md` or `log.md` for navigation
changes, and preserve unknown fields.

Consumers start with `decodex okf route` or `decodex okf find`, use graph output for
relationships, tolerate unknown `type` values, and use Decodex `docs-*` skills only
for this repository's strict `docs/` profile.

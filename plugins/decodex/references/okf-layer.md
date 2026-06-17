# Portable OKF Layer

Use this for portable OKF bundles, LLM Wiki retrieval, and cross-repository memory.

Route by task:

- `repo-memory-writer`: first-pass source-backed repo concepts.
- `repo-memory-evaluator`: quality report, route benchmark, owner coverage.
- `repo-memory-curator`: route misses, orphans, noisy owners, duplicates, graph decay.

OKF is Markdown plus YAML frontmatter and ordinary links. LLM Wiki is the agent method
on top: route small context, maintain links/indexes/logs, and preserve producer fields.
Decodex docs are only one strict profile; other repos do not inherit Decodex lanes,
Linear workflow, research promotion, docs-impact checkpoints, or landing policy.

Profiles: `core` validates portable OKF; `wiki` adds graph hygiene; `repo-memory` adds
repository anchors such as `source_refs`, `code_refs`, `related`, and `drift_watch`;
`decodex` adds this repo's lanes, authority, research, and drift gates. Use the lowest
profile that proves the claim.

Use `decodex okf init/check/find/graph/route` for portable bundles. `decodex docs`
defaults to root `docs/` and profile `decodex`. Do not create or recommend
`decodex docs okf ...`.

The CLI does not replace the LLM: agents judge owners, write concepts, and classify
misses; CLI commands supply repeatable check, graph, find, and route evidence.

Producers keep concepts one-topic, use frontmatter for routing, link real
relationships, update indexes/logs, and preserve unknown fields. Consumers start with
route/find and use graph output for relationships. Shape checks prove conformance;
route benchmarks and graph/orphan triage prove usefulness.

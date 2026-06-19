# Portable OKF Layer

Use this for portable OKF bundles, LLM Wiki navigation, and cross-repository memory.

Route by task:

- `$knowledge:docs`: checked-in repository docs workflow.
- `$knowledge:docs-drift`: docs/code/status/config/runtime semantic drift.
- `$knowledge:okf`: init/check/find/graph/query/maintain OKF bundles.
- `$knowledge:repo-memory`: write/evaluate/curate source-backed repo memory.

OKF is Markdown plus YAML frontmatter and ordinary links. LLM Wiki adds agent
navigation: small indexes, owner concepts, links, and logs. A repository docs tree may
define a stricter profile; portable bundles do not inherit runtime lanes, tracker
workflow, research promotion, docs-impact checkpoints, or landing policy.

Profiles: `core` validates portable OKF; `wiki` adds graph hygiene; `repo-memory` adds
repository anchors such as `source_refs`, `code_refs`, `related`, and `drift_watch`;
`decodex` adds this repo's lanes, authority, research, and drift gates. Use the lowest
profile that proves the claim.

Use `decodex okf init/check/find/graph` for portable bundles. `decodex docs` defaults
to root `docs/` and profile `decodex`. Do not create or recommend `decodex docs okf
...`.

The CLI does not replace the LLM: agents judge owners, write concepts, and classify
misses; commands only supply check, graph, and find evidence.

Producers keep concepts one-topic, use frontmatter for navigation, link real
relationships, update indexes/logs, and preserve unknown fields. Consumers start with
indexes plus `find`, then use graph output for relationships. Shape checks prove
conformance; owner coverage, graph health, and real agent-read reviews prove
usefulness. Ranking, embeddings, route benchmarks, and scorer quality are outside the
OKF/LLM Wiki contract.

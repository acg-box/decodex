# Docs Method

Use for docs or documented behavior changes. `docs/` is strict OKF: `docs/index.md`
routes, `docs/policy.md` gates, `docs/log.md` logs. Do not create a parallel `wiki/` or `okf/` root when a docs owner exists. Portable OKF uses `$knowledge:okf` and does not inherit runtime lanes, docs-impact checkpoints, or landing policy.

Lifecycle: read index/policy/owner; update owner; add `code_refs`, `drift_watch`, or
proof; update indexes/log; run `decodex docs check`.
Docs impact: `none`, `update_required`, `research_required`, `drift_required`.
Research stays latent until promoted. Routing: `docs-okf.md`, `docs-wiki.md`, `docs-drift.md`.

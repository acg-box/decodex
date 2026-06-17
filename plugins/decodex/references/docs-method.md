# Decodex Docs Method

Use this reference when a Decodex task changes repository documentation, behavior
that documentation describes, or the agent-facing docs workflow.

## Contract

`docs/` is the Decodex repo-development knowledge base:

- `docs/index.md` is the routing entrypoint.
- `docs/policy.md` owns OKF shape, lane authority, and completion gates.
- `docs/log.md` records docs maintenance events.
- All durable docs artifacts are Markdown.
- Non-index, non-log Markdown files are OKF concepts with typed frontmatter.
- Research concepts are latent until promoted.
- Drift audits are evidence concepts that can block completion.

Do not create a parallel `wiki/` or `okf/` root. OKF is the `docs/` protocol. LLM
Wiki is the authoring and retrieval style.

## Lifecycle

1. Read `docs/index.md` and `docs/policy.md`.
2. Identify the smallest owning concept for the changed claim.
3. Update that concept instead of duplicating the claim.
4. Add or update frontmatter evidence fields when behavior, commands, schema,
   status, config, validation, tracker labels, or runtime semantics changed.
5. Update lane indexes and `docs/log.md` when routing, names, promotion, or
   maintenance state changes.
6. Run `cargo run -p decodex --bin decodex -- docs lint`.
7. Treat lint or drift failure as a completion blocker.

## Docs Impact

Every lane must classify docs impact before completion:

| Value | Meaning |
| --- | --- |
| `none` | No docs, command, behavior, config, status, or workflow claim changed. |
| `update_required` | A durable docs concept must be updated in the lane. |
| `research_required` | Missing or contradictory authority needs Decodex research. |
| `drift_required` | A changed claim needs a drift audit before completion. |

`validation-ready` includes docs readiness. Do not claim ready while a required docs
update, research result, or drift audit is missing.

## Routing

- Use `docs-okf.md` for concept layout, frontmatter, and Markdown-only checks.
- Use `docs-wiki.md` for placement, indexing, linking, and deduplication.
- Use `docs-drift.md` for claim/evidence audits.
- Use the research skill family when impact is `research_required`.


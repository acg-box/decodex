---
name: docs
description: Use when repository docs, a docs/ OKF profile, LLM Wiki navigation, docs impact, or semantic drift is in scope.
---

# Docs

Route repository docs work through the checked-in docs policy and OKF/LLM Wiki owner
concepts. For portable OKF bundles outside a docs workflow, use `$knowledge:okf`.

Read `../../references/docs-method.md`, `docs/index.md`, `docs/policy.md`, and the
smallest owning concepts before changing behavior, workflow, CLI, status, config,
research, or documentation.
Use the repository's owning runtime or planning plugin when docs impact changes
execution authority.

- Read `../../references/docs-okf.md` for concept shape.
- Read `../../references/docs-wiki.md` for placement, indexes, links, and duplicate
  claims.
- Use `$knowledge:docs-drift` for docs/code/evidence alignment.
- Classify docs impact before completion: `none`, `update_required`,
  `research_required`, or `drift_required`.
- If docs impact is `research_required`, switch to the repository's owning research
  workflow plus `$deliberation:skeptic`.
- Record routing, promotion, rename, or maintenance changes in `docs/log.md`.
- Run `decodex docs check` before claiming docs readiness.
- Treat docs check or drift failure as a completion blocker.

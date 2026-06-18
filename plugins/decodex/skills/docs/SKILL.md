---
name: docs
description: Use when Decodex work touches this repository's docs/ OKF profile, LLM Wiki routing, or drift audits.
---

# Decodex Docs

Route Decodex work through this repository's strict `docs/` OKF/LLM Wiki profile.
For portable OKF bundles in other repositories, use `okf`, `okf-query`, or
`okf-maintain` instead.

Read `../../references/docs-method.md`, `docs/index.md`, `docs/policy.md`, and the
smallest owning concepts before changing behavior, workflow, CLI, status, config,
research, or documentation.
Use `../../references/routing.md` when a docs gate was skipped, recovered late, or
tied to OKF/LLM Wiki route intake.

- Use `docs-okf` for Decodex profile concept shape.
- Use `docs-wiki` for placement, indexes, links, and duplicate claims.
- Use `docs-drift` for docs/code/evidence alignment.
- Classify docs impact before completion: `none`, `update_required`,
  `research_required`, or `drift_required`.
- If docs impact is `research_required`, switch to the Decodex `research*` skill
  family.
- Record routing, promotion, rename, or maintenance changes in `docs/log.md`.
- Run `decodex docs check` before claiming docs readiness.
- Treat docs check or drift failure as a completion blocker.

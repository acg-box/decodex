---
name: docs
description: Use when Decodex work touches docs/ OKF structure, LLM Wiki routing, or drift audits.
---

# Decodex Docs

Route Decodex work through the repo-development OKF/LLM Wiki knowledge base.

Read `../../references/docs-method.md`, `docs/index.md`, `docs/policy.md`, and the
smallest owning concepts before changing behavior, workflow, CLI, status, config,
research, or documentation.

- Use `docs-okf` for OKF concept shape.
- Use `docs-wiki` for placement, indexes, links, and duplicate claims.
- Use `docs-drift` for docs/code/evidence alignment.
- Classify docs impact before completion: `none`, `update_required`,
  `research_required`, or `drift_required`.
- If docs impact is `research_required`, switch to the Decodex `research*` skill
  family.
- Record routing, promotion, rename, or maintenance changes in `docs/log.md`.
- Run `cargo run -p decodex --bin decodex -- docs lint` before claiming docs
  readiness.
- Treat docs lint or drift failure as a completion blocker.

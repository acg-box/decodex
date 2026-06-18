---
name: okf-query
description: Use when locating context in an OKF/LLM Wiki bundle by profile fields, graph links, backlinks, tags, code refs, source refs, or known concept text.
---

# OKF Query

Consume an OKF bundle as agent-readable repository memory.

Read `../../references/okf-layer.md` before choosing query commands or reporting
bundle graph findings.

- Start from `index.md`, lane indexes, and known owner links when the user has a
  task.
- Use `decodex okf find <root>` with `--type`, `--tag`, `--resource`,
  `--source-ref`, `--code-ref`, `--related`, or `--text` for precise lookup.
- Use `decodex okf graph <root> --json` when relationship shape, orphans, or broken
  bundle-internal links matter.
- Return the smallest concept set that can answer the task.
- If the user asks whether a bundle is good, useful, production-ready, or improving,
  switch to `repo-memory-evaluator` instead of only returning query hits.
- If query evidence shows missing owners, duplicate owners, stale links, weak indexes,
  or unexplained orphans, switch to `repo-memory-curator` before editing the bundle.
- Do not require Decodex lanes or Linear workflow for portable OKF consumption.

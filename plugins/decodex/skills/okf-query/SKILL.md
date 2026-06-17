---
name: okf-query
description: Use when locating context in an OKF/LLM Wiki bundle by profile fields, graph links, backlinks, tags, code refs, source refs, or task intent.
---

# OKF Query

Consume an OKF bundle as agent-readable repository memory.

Read `../../references/okf-layer.md` before choosing query commands or reporting
bundle graph findings.

- Start with `decodex okf route <root> "<task intent>"` when the user has a task.
- Use `decodex okf find <root>` with `--type`, `--tag`, `--resource`,
  `--source-ref`, `--code-ref`, `--related`, or `--text` for precise lookup.
- Use `decodex okf graph <root> --json` when relationship shape, orphans, or broken
  bundle-internal links matter.
- Return the smallest concept set that can answer the task.
- If query evidence shows repeated misses, noisy top results, or unexplained orphans,
  switch to `repo-memory-curator` before editing the bundle.
- Do not require Decodex lanes or Linear workflow for portable OKF consumption.

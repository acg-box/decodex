---
name: okf
description: Use when creating, checking, querying, graphing, or maintaining portable OKF/LLM Wiki bundles, profiles, indexes, links, logs, or graph health.
---

# Portable OKF

Route portable OKF and LLM Wiki work without assuming the Decodex docs profile.

Read `../../references/okf-layer.md` before changing a bundle, recommending a
profile, or using OKF commands.

- Use `decodex okf init <root> --profile core|wiki|repo-memory` to create a safe,
  validated starter bundle.
- Use `decodex okf check <root> --profile core|wiki|repo-memory|decodex` for bundle
  validation.
- Use `decodex okf find` and `decodex okf graph` for consumer workflows.
- Pick the lowest profile that proves the current claim.
- Preserve producer-specific fields and unknown concept types.
- Use `$knowledge:docs` for checked-in repository docs workflows.
- Do not treat OKF as a retrieval scorer, embedding system, reranker, or automatic
  high-quality knowledge generator.

For source-backed repository memory authoring, evaluation, or curation, use
`$knowledge:repo-memory`.

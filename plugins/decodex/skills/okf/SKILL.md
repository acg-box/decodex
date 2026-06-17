---
name: okf
description: Use when creating, checking, querying, graphing, routing, or maintaining portable OKF/LLM Wiki bundles across repositories.
---

# Portable OKF

Route portable OKF and LLM Wiki work without assuming the Decodex docs profile.

Read `../../references/okf-layer.md` before changing a bundle, recommending a
profile, or using OKF commands.

- Use `decodex okf check <root> --profile core|wiki|repo-memory|decodex` for bundle
  validation.
- Use `decodex okf find`, `decodex okf graph`, and `decodex okf route` for consumer
  workflows.
- Pick the lowest profile that proves the current claim.
- Preserve producer-specific fields and unknown concept types.
- Use Decodex `docs-*` skills only for this repository's strict `docs/` profile.

---
name: docs-wiki
description: Use when maintaining this repository's docs/ lane indexes, links, and Decodex LLM Wiki navigation.
---

# Decodex Docs Wiki

Maintain this repository's `docs/` as an agent-readable LLM Wiki under the Decodex
profile. For portable OKF bundle lookup, use `okf-query`; for portable maintenance,
use `okf-maintain`.

Read `../../references/docs-wiki.md` for lane ownership, authoring, and indexing
rules.

- Start from `docs/index.md`, then `docs/policy.md`, then the smallest owning
  concept.
- Keep one authoritative concept per claim. Link instead of copying.
- Update lane indexes whenever a concept is added, renamed, moved, deprecated, or
  superseded.
- If a new claim does not have a clear owner, create the smallest owning concept in
  the lane selected by `docs/policy.md`.
- Record routing, promotion, rename, or maintenance changes in `docs/log.md`.

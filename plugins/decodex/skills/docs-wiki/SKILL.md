---
name: docs-wiki
description: Use when turning repo docs into agent-readable LLM Wiki knowledge pages.
---

# Decodex Docs Wiki

Maintain `docs/` as an agent-readable LLM Wiki.

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

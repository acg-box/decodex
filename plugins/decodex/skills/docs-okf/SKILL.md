---
name: docs-okf
description: Use when creating or migrating docs/ to the strict Decodex OKF profile.
---

# Decodex Docs OKF

Maintain this repository's `docs/` bundle as Decodex profile concepts plus JSON
research artifacts.
For portable OKF bundles, use `okf-maintain`.

Read `../../references/docs-okf.md` before creating, moving, or repairing concepts.

- Keep durable non-research docs artifacts Markdown-only.
- Keep `docs/research/` as flat JSON research artifacts only.
- Use typed Decodex profile frontmatter from `docs/policy.md`.
- Keep research artifacts and drift audit evidence concepts in their required shape.
- Do not add JSON or generated state under `docs/` outside `docs/research/`.
- Run `decodex docs check` before claiming docs readiness.

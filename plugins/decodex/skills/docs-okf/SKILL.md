---
name: docs-okf
description: Use when creating or migrating docs/ to the strict Decodex OKF profile.
---

# Decodex Docs OKF

Maintain this repository's `docs/` bundle as Markdown-only Decodex profile concepts.
For portable OKF bundles, use `okf-maintain`.

Read `../../references/docs-okf.md` before creating, moving, or repairing concepts.

- Keep durable docs artifacts Markdown-only.
- Use typed Decodex profile frontmatter from `docs/policy.md`.
- Keep research contracts and drift audit evidence concepts in their required section
  shape.
- Do not add JSON or generated state under `docs/`.
- Run `cargo run -p decodex --bin decodex -- docs check` before claiming docs
  readiness.

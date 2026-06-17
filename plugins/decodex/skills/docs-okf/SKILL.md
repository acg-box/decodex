---
name: docs-okf
description: Use when creating or migrating docs/ to the Decodex OKF layout.
---

# Decodex Docs OKF

Maintain the `docs/` bundle as Markdown-only OKF concepts.

Read `../../references/docs-okf.md` before creating, moving, or repairing concepts.

- Keep durable docs artifacts Markdown-only.
- Use typed OKF frontmatter from `docs/policy.md`.
- Keep research contracts and drift audit evidence concepts in their required section
  shape.
- Do not add JSON or generated state under `docs/`.
- Run `cargo run -p decodex --bin decodex -- docs lint` before claiming docs
  readiness.

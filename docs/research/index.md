# Research Index

Purpose: Route agents to Markdown-only research concepts.

Research output is latent until accepted and promoted. Research concepts preserve
evidence, options, challenge notes, terminal status, promotion targets, and drift
impact. They do not authorize implementation by themselves.

## Concepts

- [`research-runtime-boundary.md`](research-runtime-boundary.md) defines the current
  boundary between checked-in Markdown research concepts, runtime Decision Contracts,
  MCP readback, and future execution research.

## Maintenance

- New research concepts must follow [`../policy.md`](../policy.md).
- Do not add non-Markdown artifacts under `docs/research/`.
- Promote accepted research into `docs/spec/`, `docs/runbook/`, `docs/reference/`, or
  `docs/decisions/`, then record the promotion in [`../log.md`](../log.md).
- When accepted research changes agent-facing workflow instructions, update the
  matching `plugins/decodex/skills/` files alongside the promoted docs concept.

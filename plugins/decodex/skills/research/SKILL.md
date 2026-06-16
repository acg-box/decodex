---
name: research
description: Use when Decodex needs bounded research.
---

# Decodex Research

Produce a latent, contract-first Decision Contract candidate, not execution
authority. Read `../../references/research-method.md` for the full protocol,
evidence ledger, promotion target rules, and contract shape.

Use `research-probe`, `research-evidence`, `research-options`, `research-judgment`,
`research-challenge`, and `research-decision` in order. Use `research-promote` only
after explicit acceptance.

- Do not route Decodex research through the legacy external `$research`.
- Do not write new Decodex research as old-shape `docs/research/` event logs or treat
  old artifacts as current authority.
- Use `docs/research/` only for explicit supporting JSON research reports or evidence
  extraction that remains non-authoritative until promoted.
- Do keep terminal status, evidence classes, selected option, gaps, and promotion
  target visible from the top-level contract.
- Do not queue work, mutate Linear, set Codex goals, implement, or dispatch Program
  nodes from research alone.

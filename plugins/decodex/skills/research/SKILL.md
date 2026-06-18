---
name: research
description: Use when Decodex needs bounded research.
---

# Decodex Research

Produce a latent, contract-first Decision Contract candidate, not execution
authority. Read `../../references/research-lifecycle.md` first, then load
`../../references/research-evidence.md`, `../../references/research-contract.md`, or
`../../references/research-promotion.md` only when the phase needs it.

Follow phase skills in order. Use `research-promote` only after explicit acceptance.

- Do not route Decodex research through external research skills.
- Do not write new Decodex research as `docs/research/` event logs or JSON.
- Use `docs/research/` only for Markdown OKF research concepts or evidence extraction
  that remains non-authoritative until promoted.
- Do keep terminal status, evidence classes, selected option, gaps, and promotion
  target visible from the top-level contract.
- Split research-only evidence from durable knowledge candidates; accepted facts,
  policy/spec/runbook/structure/workflow instructions need non-research targets.
- Name the OKF disposition: `continue`, `promote_and_supersede`,
  `promote_and_retire`, or `reject_or_deprecate`.
- Do not queue work, mutate Linear, set Codex goals, implement, or dispatch Program
  nodes from research alone.

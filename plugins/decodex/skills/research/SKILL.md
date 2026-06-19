---
name: research
description: Use when Decodex needs bounded research or design investigation before execution, including framing a decision question, collecting evidence, comparing options, forming a judgment, running a challenge pass, and producing a terminal Decision Contract status.
---

# Decodex Research

Produce a latent, contract-first Decision Contract candidate, not execution authority.
Read `../../references/research-lifecycle.md` first, then load
`../../references/research-evidence.md`, `../../references/research-contract.md`, or
`../../references/research-promotion.md` only when the run needs that detail.

Follow the compact loop: probe, evidence, options, judgment, challenge, decision. Use
`$agent-method:challenge` for the skeptic pass before `decision_ready` or any
high-risk recommendation. Use `research-promote` only after explicit acceptance.

- Do not route Decodex research through external research skills.
- Do not write new Decodex research as `docs/research/` event logs or JSON.
- Use `docs/research/` only for Markdown OKF research concepts or evidence extraction
  that remains non-authoritative until promoted.
- Do keep terminal status, evidence classes, selected option, gaps, and promotion
  target visible from the top-level contract.
- End with exactly one status: `decision_ready`, `not_decision_ready`, `blocked`, or
  `needs_human_decision`.
- Split research-only evidence from durable knowledge candidates; accepted facts,
  policy/spec/runbook/structure/workflow instructions need non-research targets.
- Name the OKF disposition: `continue`, `promote_and_supersede`,
  `promote_and_retire`, or `reject_or_deprecate`.
- Do not queue work, mutate Linear, set Codex goals, implement, or dispatch Program
  nodes from research alone.
- A scout pass is dynamic read-only evidence gathering, not a configured static role;
  use it only for one bounded evidence objective when the main thread needs support.

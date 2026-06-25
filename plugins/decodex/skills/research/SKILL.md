---
name: research
description: Use when Decodex needs bounded research or design investigation before execution, including framing a decision question, collecting evidence, comparing options, forming a judgment, running a challenge pass, and producing a terminal Decision Contract status.
---

# Decodex Research

Produce a latent, contract-first Decision Contract candidate, not execution authority.
Read `../../references/research-lifecycle.md` first, then load
`../../references/research-evidence.md`, `../../references/research-contract.md`, or
`../../references/research-promotion.md` only when the run needs that detail.

Follow the compact loop: first-principles probe, scout evidence, options, judgment,
challenge, decision. Use `$deliberation:grill` for framing when scope or constraints
are unclear, `$deliberation:scout` for non-obvious evidence, and
`$deliberation:challenge` before `decision_ready` or any material recommendation. Use
`research-promote` only after explicit acceptance.

- The research compact loop is not runtime `compact_current_head_review`.
- For runtime compact review quality, read the current `issue_review_checkpoint`
  `review_cost_control` and `decodex evidence` instead of restating tracker or review
  policy in research output.
- Treat compact runtime review as independent current-head review evidence, not a
  skipped-review signal.
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
- Scout and skeptic passes are dynamic fresh-context support-agent work, not
  configured static roles. Use the `$deliberation:*` inline exception: inline only
  when one local question fits in 1-2 files or one command and cannot affect
  decision readiness, public contracts, docs drift, commit/land, or ready/done
  claims. Otherwise dispatch a bounded read-only scout or skeptic support agent when
  support-agent tools are allowed, and keep any fallback visible.

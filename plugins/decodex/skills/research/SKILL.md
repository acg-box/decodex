---
name: research
description: Use when Decodex needs bounded research or design investigation before execution, including framing a decision question, collecting evidence, comparing options, forming a judgment, running a challenge pass, and producing a terminal Decision Contract status.
---

# Decodex Research

Produce a latent, contract-first Decision Contract candidate, not execution authority.
Read `../../references/research-lifecycle.md` first, then load
`../../references/research-contract.md` or `../../references/research-promotion.md`
only when the run needs that detail.

Follow the compact loop: first-principles probe, scout evidence, options, judgment,
challenge, decision. Use `$deliberation:grill` for framing when scope or constraints
are unclear, `$deliberation:scout` for non-obvious evidence, and
`$deliberation:challenge` before `decision_ready` or any material recommendation.
Do not wait for the user to explicitly request support agents: Decodex research that
will produce a material recommendation, option judgment, or `decision_ready` claim
must use at least one fresh bounded read-only scout or challenge support-agent pass
when support-agent tools are allowed. Use `research-promote` only after explicit
acceptance.

- Before broad evidence gathering, establish the artifact contract: decision owner,
  decision question, non-goals, output shape, useful bounds, and what evidence would
  change the recommendation.
- Keep research bounded, not inline by default. Purely local fact checks may stay
  inline only when they end without a material recommendation. Ordinary material
  research uses at least one read-only support-agent scout or challenge pass.
  Architecture, product, cross-boundary, root-cause, or public-contract research uses
  separate scout and challenge support agents unless the user explicitly opts out or
  support-agent tooling is unavailable.
- For non-trivial repo research, use `$knowledge:docs`, `$knowledge:okf`, or
  `$knowledge:repo-memory` for source-backed context, and `$knowledge:docs-drift`
  when docs, code, config, help, or status claims may diverge.
- Convert evidence into action: facts should become inferences, decision impact, and
  the smallest next check rather than a source inventory.
- Classify evidence as external source, repo source, live readback, inference, or gap;
  preserve contradictions instead of flattening them into confidence.
- Before terminal status, run an operationalization pass: name the gate, owner,
  validation or release check, rollback or freeze path, stop condition, and
  falsifier when the decision would guide real work.
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
  support-agent tools are allowed, and keep any fallback visible. Support agents must
  receive one objective, allowed roots or sources, excluded surfaces, and expected
  output shape; they must not mutate state or spawn further support agents unless the
  main thread explicitly delegates that.

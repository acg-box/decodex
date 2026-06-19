---
type: "Reference"
title: "Research Concepts"
description: "Explain how Markdown OKF research concepts relate to runtime Decision Contracts, promotion owners, and LLM Wiki routing."
status: active
authority: current_state
owner: docs
tags: [reference, research, okf]
last_verified: 2026-06-17
---

# Research Concepts

Purpose: Explain the role of Markdown OKF research concepts in this repository and
how they relate to runtime Decision Contracts and promoted docs authority.

Read this when: You are creating or reading a `docs/research/*.md` concept, deciding
whether research evidence is authoritative, or promoting accepted research into a
durable docs lane.

Not this document: The research method itself, the runtime contract, or a durable
design decision record.

Covers: Concept placement, authority boundaries, required research sections,
promotion owners, research disposition, and LLM Wiki routing hygiene.

## Status of `docs/research/`

- `docs/research/` is a Markdown-only OKF concept lane for bounded research and
  evidence-backed decision candidates.
- Tracked files under `docs/research/` must be Markdown concepts with the required
  OKF frontmatter from [`../policy.md`](../policy.md).
- Non-Markdown artifacts are forbidden under `docs/`, including `docs/research/`.
- New Decodex bounded research must not create checked-in JSON event logs.
- A research concept may contain useful evidence, alternatives, objections, and a
  candidate decision, but it does not by itself define repository truth.
- A promoted conclusion must update the owning concept in `docs/spec/`,
  `docs/runbook/`, `docs/reference/`, `docs/decisions/`, or `docs/evidence/`;
  `docs/research/` remains latent or explicitly superseded and non-authoritative.
- For Decodex-specific loop-runtime work, Decodex `research`,
  `$agent-method:challenge`, and Decodex `research-promote` plus
  `decodex research compile` produce and promote a runtime-local
  `decodex.decision_contract/1` candidate. Runtime storage may stay structured for
  machine use, but checked-in docs remain Markdown OKF concepts.

## Required Sections

Research concepts must expose the contract headings defined by
[`../policy.md`](../policy.md):

- `Question`
- `Scope`
- `Evidence`
- `Options`
- `Judgment`
- `Challenge`
- `Decision`
- `Promotion`
- `Drift Impact`
- `Citations`

The `Decision` section must state exactly one terminal status:

- `decision_ready`
- `not_decision_ready`
- `blocked`
- `needs_human_decision`

## Promotion Rules

- If a research result defines required behavior, promote the conclusion into
  `docs/spec/`.
- If a research result defines an operator sequence, promote the conclusion into
  `docs/runbook/`.
- If a research result explains current structure, promote the conclusion into
  `docs/reference/`.
- If a research result records a durable tradeoff or design choice, promote the
  conclusion into `docs/decisions/`.
- If a research result produces reusable proof, public-safe evidence, or drift-audit
  material, promote that material into `docs/evidence/`.
- If accepted research changes future agent behavior, update the matching
  `plugins/decodex/skills/` files alongside the docs owner.
- If a Decodex-native research/design result should feed issue shaping or unattended
  execution, promote the stored Decision Contract first. Do not infer acceptance from
  a research summary or Markdown concept.

## Research Disposition

- `continue`: unresolved research stays active.
- `promote_and_supersede`: owner concepts receive the accepted knowledge; a compact
  `status: superseded` research concept remains only for provenance.
- `promote_and_retire`: owner concepts fully absorb the knowledge; remove the concept
  from active LLM Wiki routing and indexes.
- `reject_or_deprecate`: rejected or stale research remains only when it has retrieval
  value as a decision, evidence concept, or deprecated research concept.

Research retention is an OKF knowledge decision. Do not rely on out-of-band history
for knowledge retention.

## LLM Wiki Hygiene

After promotion, update lane indexes, `related`, `promotes_to`, descriptions, and
status fields so authoritative owners outrank superseded research. A superseded
research concept may point to owners, but it must not repeat current truth.

## Practical Reading Rule

- Keep lane policy in [`../policy.md`](../policy.md) and the research index in
  [`../research/index.md`](../research/index.md).
- Read one of the four primary documentation lanes when you need current repository
  guidance.
- Use Decodex `research`, `$agent-method:challenge`, Decodex `research-promote`, and
  Decision Contracts for all new bounded Decodex research.
- New research must expose terminal status, selected option, evidence ledger, gaps,
  validation, and promotion target from the concept frontmatter and top-level
  sections.

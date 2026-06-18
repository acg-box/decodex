---
type: "Reference"
title: "Research Artifacts"
description: "Explain how JSON research artifacts relate to runtime Decision Contracts and promoted docs authority."
status: active
authority: current_state
owner: docs
tags: [reference, research, okf]
last_verified: 2026-06-18
---

# Research Artifacts

Purpose: Explain the role of checked-in JSON research artifacts in this repository and
how they relate to runtime Decision Contracts and promoted docs authority.

Read this when: You are creating or reading a `docs/research/*.json` artifact,
deciding whether research evidence is authoritative, or promoting accepted research
into a durable docs lane.

Not this document: The research method itself, the runtime contract, or a durable
design decision record.

Covers: Artifact placement, authority boundaries, required research fields, and
promotion rules.

## Status of `docs/research/`

- `docs/research/` is a JSON artifact lane for bounded research and evidence-backed
  decision candidates.
- Tracked files under `docs/research/` must be flat `*.json` artifacts.
  `docs/research/index.json` uses `decodex.research_index/1`; research reports use
  `decodex.research_report/1`.
- Markdown concepts are forbidden under `docs/research/`.
- Old nested `research-run/2` event-log directories remain retired. If legacy raw
  provenance is needed, use Git history rather than restoring those event logs.
- A research artifact may contain useful evidence, alternatives, objections, and a
  candidate decision, but it does not by itself define repository truth.
- A promoted conclusion must update `docs/spec/`, `docs/runbook/`,
  `docs/reference/`, or `docs/decisions/`; `docs/research/` remains latent and
  non-authoritative.
- For Decodex-specific loop-runtime work, the Decodex `research*` skills plus
  `decodex research compile` produce a runtime-local `decodex.decision_contract/1`
  candidate. Runtime storage remains the execution authority; checked-in research JSON
  is durable supporting evidence for later routing and promotion.

## Required Fields

Research reports must keep the contract visible from top-level JSON fields:

- `schema`
- `title`
- source intent or purpose
- `scope`
- evidence ledger and provenance
- options, judgment, or status summary
- selected decision or explicit non-decision
- validation expectations
- promotion target
- drift impact
- unresolved gaps or blockers

Each report must state exactly one terminal status:

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
- If a Decodex-native research/design result should feed issue shaping or unattended
  execution, promote the stored Decision Contract first. Do not infer acceptance from
  a research summary or checked-in JSON artifact.

## Practical Reading Rule

- Keep lane policy in [`../policy.md`](../policy.md) and the research index in
  [`../research/index.json`](../research/index.json).
- Read one of the four primary documentation lanes when you need current repository
  guidance.
- Use Decodex `research*` skills and Decision Contracts for all new bounded Decodex
  research.
- New research must expose terminal status, selected option, evidence ledger, gaps,
  validation, and promotion target from top-level JSON fields.

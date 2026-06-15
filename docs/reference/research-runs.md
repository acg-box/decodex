# Research Runs

Purpose: Explain the role of `docs/research/` artifacts in this repository and how they
relate to the primary documentation taxonomy.

Read this when: You encounter `docs/research/<run-id>.json` files and need to know
whether they are authoritative documentation, generated artifacts, or supporting
evidence.

Not this document: The research method itself, the runtime contract, or a design
decision record.

Covers: Artifact placement, authority boundaries, and promotion rules for research
results.

## Status of `docs/research/`

- `docs/research/` is the legacy persistence root for the earlier external research
  tooling and remains a supporting evidence lane.
- Files in `docs/research/` are machine-authored run artifacts, not primary
  documentation lanes.
- A research run may contain useful evidence, alternatives, and objections, but it does
  not by itself define repository truth.
- For Decodex-specific loop-runtime work, the Decodex `research*` skills plus
  `decodex research compile` supersede this artifact lane as the runtime-owned path.
  They store a `decodex.decision_contract/1` payload in local runtime SQLite and leave
  the result latent until explicit promotion.

## Promotion rules

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
  a research summary or from a `docs/research/` JSON artifact.

## Practical reading rule

- Read `docs/research/` when you need an older evidence trail.
- Read one of the four primary documentation lanes when you need current repository
  guidance.
- Use Decodex `research*` skills and Decision Contracts for new bounded Decodex
  research.

# Research Runs

Purpose: Explain the role of the `docs/research/` JSON research-report and evidence
artifact lane in this repository and how removed legacy JSON event logs relate to the
primary documentation taxonomy.

Read this when: You encounter a `docs/research/*.json` report, a reference to an old
`docs/research/<run-id>.json` file in Git history, or need to know whether research
artifacts are authoritative documentation, generated artifacts, or supporting
evidence.

Not this document: The research method itself, the runtime contract, or a design
decision record.

Covers: Artifact placement, authority boundaries, and promotion rules for research
results.

## Status of `docs/research/`

- `docs/research/` is a supporting JSON research-report and evidence artifact lane.
- Tracked files under `docs/research/` must be JSON research artifacts, not Markdown
  prose documents.
- The former `research-run/2` JSON files in `docs/research/` were machine-authored
  run artifacts, not primary documentation lanes and not the new research shape.
- The tracked legacy JSON files were consolidated into
  [`../research/legacy-research-goal-audit.json`](../research/legacy-research-goal-audit.json)
  and removed from the tree.
- New Decodex bounded research must not use the old `research-run/2` event-log JSON
  shape.
- A promoted or explicitly requested research report may live under `docs/research/`
  when it is a JSON research artifact and supporting evidence rather than governing
  policy.
- A research run may contain useful evidence, alternatives, and objections, but it does
  not by itself define repository truth.
- For Decodex-specific loop-runtime work, the Decodex `research*` skills plus
  `decodex research compile` replace the old event-log JSON shape as the runtime-owned
  path. They store a top-level `decodex.decision_contract/1` payload in local runtime
  SQLite and leave the result latent until explicit promotion.
- A useful legacy run may be cited from Git history as `research_provenance` or
  supporting `research_evidence` inside a Decision Contract, but the contract is the
  current research output.

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
  a research summary or from a legacy `docs/research/` JSON artifact.

## Practical reading rule

- Read [`../research/legacy-research-goal-audit.json`](../research/legacy-research-goal-audit.json)
  when you need the current status of removed legacy research targets.
- Keep index and routing prose in this reference document, not inside `docs/research/`;
  `docs/research/` itself stays limited to JSON research artifacts.
- Use Git history only when you need the raw old `research-run/2` event trail.
- Read one of the four primary documentation lanes when you need current repository
  guidance.
- Use Decodex `research*` skills and Decision Contracts for all new bounded Decodex
  research.
- Expect old `research-run/2` files to put the useful conclusion near the end of an
  event log. Do not copy that shape into new research; new research must expose
  terminal status, selected option, evidence ledger, gaps, validation, and promotion
  target from the top-level contract.

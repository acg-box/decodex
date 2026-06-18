# Decodex Docs OKF Reference

Use this for this repository's strict Decodex profile `docs/` concepts. Use
`okf-layer.md` for portable OKF bundles.

## Bundle Rules

- `docs/` uses Markdown OKF concepts, with `docs/research/*.json` as the only JSON
  artifact lane.
- Every populated directory has `index.md`, except `docs/research/` has
  `index.json`.
- `docs/index.md`, `docs/policy.md`, and `docs/log.md` must exist.
- Non-index, non-log Markdown documents are OKF concepts.
- Concepts start with YAML frontmatter delimited by `---`.
- Prose spells the acronym `OKF`; lowercase `okf` is slug-only.

## Required Frontmatter

Every concept requires `type`, `title`, `description`, `status`, `authority`,
`owner`, and `last_verified`.

Allowed `type`: `Decision`, `Drift Audit`, `Evidence`, `Policy`, `Reference`,
`Research Contract`, `Runbook`, or `Spec`.

Recommended structured fields: `tags`, `source_refs`, `code_refs`, `related`,
`promotes_to`, and `drift_watch`.

`promotes_to` may point only at `docs/spec`, `docs/runbook`, `docs/reference`, or
`docs/decisions`.

## Required Sections

Checked-in research artifacts use `schema: "decodex.research_report/1"` and keep
terminal status, evidence, validation, and promotion targets in top-level JSON fields.

`Drift Audit` concepts include `Watched Claims`, `Evidence Anchors`,
`Reverse Checks`, `Verdict`, `Required Updates`, and `Citations`. `Verdict` is
`pass`, `fail`, or `needs-human`.

## Validation

Run `decodex docs check`.

`decodex docs lint` remains a compatibility alias.

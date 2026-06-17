# Decodex Docs OKF Reference

Use this reference when creating, migrating, validating, or repairing `docs/`
concepts.

## Bundle Rules

- `docs/` is Markdown-only. JSON and other non-Markdown docs artifacts are invalid.
- Each directory with content has an `index.md`.
- `docs/index.md`, `docs/policy.md`, and `docs/log.md` must exist.
- Non-index, non-log Markdown documents are OKF concepts.
- Concepts start with YAML frontmatter delimited by `---`.
- Prose spells the acronym `OKF`; lowercase `okf` is slug-only.

## Required Frontmatter

Every concept requires:

| Key | Rule |
| --- | --- |
| `type` | One of `Decision`, `Drift Audit`, `Evidence`, `Policy`, `Reference`, `Research Contract`, `Runbook`, `Spec`. |
| `title` | Non-empty string. |
| `description` | One-sentence retrieval summary. |
| `status` | `draft`, `active`, `deprecated`, or `superseded`. |
| `authority` | `normative`, `procedural`, `current_state`, `rationale`, `evidence`, or `non_authoritative`. |
| `owner` | Owning surface such as `docs`, `runtime`, `research`, `automation`, or `site`. |
| `last_verified` | ISO date. |

Recommended structured fields:

- `tags`
- `source_refs`
- `code_refs`
- `related`
- `promotes_to`
- `drift_watch`

`promotes_to` may point only at `docs/spec`, `docs/runbook`, `docs/reference`, or
`docs/decisions`.

## Research Concepts

`type: Research Contract` concepts include:

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

The `Decision` section uses exactly one terminal status: `decision_ready`,
`not_decision_ready`, `blocked`, or `needs_human_decision`.

## Drift Audit Evidence

`type: Drift Audit` concepts live under `docs/evidence/` when the audit needs a
durable public-safe anchor. They include:

- `Watched Claims`
- `Evidence Anchors`
- `Reverse Checks`
- `Verdict`
- `Required Updates`
- `Citations`

The `Verdict` section uses `pass`, `fail`, or `needs-human`.

## Validation

Run:

```sh
cargo run -p decodex --bin decodex -- docs lint
```

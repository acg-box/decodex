---
type: Policy
title: Documentation Policy
description: Defines Decodex docs as a Markdown-only OKF knowledge bundle for agent development workflow.
status: active
authority: normative
owner: docs
tags: [docs, okf, llm-wiki, semantic-drift]
source_refs: [https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md]
code_refs: [apps/decodex/src/docs_okf.rs, apps/decodex/src/cli.rs]
related: [index.md, log.md, spec/okf-knowledge-layer.md, evidence/docs-self-iteration.md]
last_verified: 2026-06-17
---

# Documentation Policy

## Purpose

`docs/` is the Decodex repo-development knowledge base. It uses a Markdown-only
Open Knowledge Format profile so agents can navigate, read, update, verify, and
improve repository knowledge during normal lane execution. OKF itself is the portable
knowledge-bundle protocol; `docs/` is this repository's default bundle location and
`decodex docs` is the local convenience surface for that bundle.

This policy owns the docs taxonomy, OKF concept shape, promotion rules, and
self-iteration loop for documentation. It does not own runtime state, tracker state,
private execution evidence, or the portable OKF core contract. The portable command
and profile boundary is defined by [`spec/okf-knowledge-layer.md`](spec/okf-knowledge-layer.md).

## Authority

`docs/` is an OKF bundle:

- every durable knowledge artifact is Markdown
- `docs/index.md` is the progressive-disclosure entrypoint
- `docs/log.md` records knowledge maintenance events
- every non-index, non-log Markdown document is an OKF concept with YAML
  frontmatter
- non-Markdown artifacts, including JSON, are not allowed under `docs/`

Runtime state may still use internal structured storage. The docs source of truth for
research, drift audit, decisions, references, runbooks, and specs is Markdown.

## Naming

Write `OKF` as an all-caps acronym in prose, matching the repository convention for
`CLI`. Lowercase `okf` is allowed only in machine identifiers such as filenames,
paths, skill IDs, tags, and URLs.

## Command Boundary

Use `decodex okf` for portable OKF operations against any bundle path, including
`decodex okf init <root> --profile repo-memory` for new repository-memory bundles.
Use `decodex docs` for this repository's default `docs/` bundle with the strict
Decodex profile.

Do not add `decodex docs okf ...` command nesting. OKF is not a docs subfeature;
`docs/` is one bundle location that happens to use OKF.

Profile ownership:

- `core` follows the portable OKF conformance surface.
- `wiki` adds graph and agent-navigation checks.
- `repo-memory` adds repository anchors such as `code_refs`, `source_refs`, and
  drift hints.
- `decodex` adds this repository's lanes, authority enums, research contracts, drift
  audits, and completion gates.

## Concept Frontmatter

Every concept must start with YAML frontmatter delimited by `---`.

Required keys:

| Key | Meaning |
| --- | --- |
| `type` | Concept class used for navigation and filtering. |
| `title` | Human-readable and agent-readable concept title. |
| `description` | One-sentence concept summary for indexes and lookup. |
| `status` | `draft`, `active`, `deprecated`, or `superseded`. |
| `authority` | `normative`, `procedural`, `current_state`, `rationale`, `evidence`, or `non_authoritative`. |
| `owner` | Owning surface such as `docs`, `runtime`, `research`, `automation`, or `site`. |
| `last_verified` | ISO date when the claim surface was last checked. |

Recommended keys:

| Key | Meaning |
| --- | --- |
| `tags` | Retrieval labels. |
| `source_refs` | External or public source references. |
| `code_refs` | Repository-relative code, test, script, or config file paths. |
| `related` | Related concepts inside the OKF bundle. |
| `promotes_to` | Target lane when a research concept becomes durable knowledge. |
| `drift_watch` | Commands, paths, labels, statuses, config keys, or schemas watched for semantic drift. |

## Primary Lanes

| Lane | Location | Type | Authority | Answers |
| --- | --- | --- | --- | --- |
| Spec | `docs/spec/` | `Spec` | `normative` | What must be true? |
| Runbook | `docs/runbook/` | `Runbook` | `procedural` | Which sequence should I execute? |
| Reference | `docs/reference/` | `Reference` | `current_state` | How is it currently organized or implemented? |
| Decisions | `docs/decisions/` | `Decision` | `rationale` | Why is it shaped this way? |
| Research | `docs/research/` | `Research Contract` | `non_authoritative` | What candidate conclusion has evidence but no execution authority yet? |
| Evidence | `docs/evidence/` | `Evidence` or `Drift Audit` | `evidence` | Which public-safe proof supports claims and drift audits? |

## Research Contracts

New research output is a Markdown concept under `docs/research/`. A research concept
is never execution authority or current repository truth by itself.

Research concepts must expose headings with these names. The heading level is not
semantic; concepts may use a top-level title followed by lower-level contract
headings.

- `Question`
- `Scope`
- `Evidence`
- `Options`
- `Judgment`
- `Skeptic`
- `Decision`
- `Promotion`
- `Drift Impact`
- `Citations`

`Decision` must state exactly one terminal status:

- `decision_ready`
- `not_decision_ready`
- `blocked`
- `needs_human_decision`

Promotion is an OKF knowledge operation. It consumes a research concept by moving
durable rationale to `docs/decisions/`, normative truth to `docs/spec/`, current
state to `docs/reference/`, procedures to `docs/runbook/`, and reusable proof or
drift-audit material to `docs/evidence/`. When accepted research changes
agent-facing workflow instructions, update the matching `plugins/decodex/skills/`
surface alongside the selected docs concept; plugin skills are companion execution
surfaces, not `promotes_to` lanes.

Each promoted research concept must end with one disposition:

- `continue`: unresolved research remains active.
- `promote_and_supersede`: durable knowledge moved to owners, but a compact
  provenance concept remains in `docs/research/` with `status: superseded`.
- `promote_and_retire`: durable owners fully absorb the knowledge, so the concept is
  removed from active LLM Wiki routing and indexes.
- `reject_or_deprecate`: rejected or stale research is represented as a decision,
  evidence concept, or `status: deprecated` research concept only when it remains
  useful to retrieval.

Promotion must preserve evidence, constraints, rejected alternatives, validation
expectations, and drift impacts in the correct OKF owners. Knowledge retention must be
explicit in OKF concepts, indexes, and links rather than relying on out-of-band
history. After promotion, `docs/research/` must not compete with the
authoritative owner in LLM Wiki routing.

## Semantic Drift

Semantic drift is part of docs maintenance, not a separate optional workflow.

When a lane creates or materially changes a concept claim about commands, flags,
config, status fields, schemas, validation gates, runtime behavior, tracker labels,
telemetry, generated artifacts, or operator procedures, that changed claim must
either:

- include direct `code_refs` and `drift_watch`, or
- link to a `docs/evidence/` drift audit concept that audits those claims.

Existing concepts without those fields are not automatically compliant for changed
behavior. The next lane that touches a behavior claim must add the direct evidence
fields or a linked drift audit before claiming docs readiness.

Drift audit evidence concepts must expose headings with these names:

- `Watched Claims`
- `Evidence Anchors`
- `Reverse Checks`
- `Verdict`
- `Required Updates`
- `Citations`

`Verdict` must be `pass`, `fail`, or `needs-human`.

## Self-Iteration Loop

Every docs-changing lane follows this loop:

1. Read `docs/index.md`, then the smallest linked concepts needed for the task.
2. Update the concept that owns the changed knowledge instead of duplicating the
   claim elsewhere.
3. If the change affects behavior, update the owning concept's `code_refs` and
   `drift_watch`, or update/create a linked drift audit.
4. Update lane indexes and `docs/log.md` when navigation changes.
5. Run `decodex docs check`.
6. Treat docs check or drift failure as a completion blocker. Research uncertainty uses
   the research-contract `needs_human_decision` status; implementation-lane blockers
   use the runtime `manual_attention` terminal path.

The agent-facing entrypoint for this loop is
[`plugins/knowledge/skills/docs/SKILL.md`](../plugins/knowledge/skills/docs/SKILL.md),
which delegates to knowledge references and the `docs-drift` skill. If
the docs impact is `research_required`, switch to Decodex `research` and
`$deliberation:skeptic`, and persist any checked-in result under `docs/research/`
only as a latent, non-authoritative Markdown OKF research concept until explicitly
promoted.

Detailed docs rules live in `plugins/knowledge/references/docs-method.md`,
`docs-okf.md`, `docs-wiki.md`, and `docs-drift.md`. Detailed research rules live in
`plugins/decodex/references/research-lifecycle.md`, `research-contract.md`, and
`research-promotion.md`.

## Decodex Lane Integration

Each automated lane must classify docs impact before completion. Decodex records the
classification as the private `docs_impact` field on `issue_progress_checkpoint`.

| Value | Meaning |
| --- | --- |
| `none` | No docs, command, behavior, config, status, or workflow claim changed. |
| `update_required` | A durable concept must be updated in the same lane. |
| `research_required` | Missing or contradictory authority requires a research concept. |
| `drift_required` | A changed claim needs a drift audit before completion. |

`validation-ready` includes docs readiness. A lane cannot claim ready when the docs
gate fails for touched documentation or touched behavior with docs impact.

## Validation

Run:

```sh
decodex docs check
```

In this repository, `cargo make check` includes the same docs gate. Command aliases
are not allowed; `decodex docs check` is the only supported docs validation
subcommand.

The check fails when:

- `docs/log.md` or required lane indexes are missing
- non-Markdown artifacts appear under `docs/`
- concepts lack required frontmatter or use unsupported OKF enum/date values
- structured frontmatter refs are malformed, point outside their authority boundary,
  or reference missing repository/docs paths
- research contracts or drift audit evidence concepts lack their required headings
- local Markdown links are broken
- `docs/evidence/` lacks a drift audit anchor

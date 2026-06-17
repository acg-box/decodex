---
type: Spec
title: OKF Knowledge Layer
description: Defines the portable OKF engine, LLM Wiki profile, Decodex docs alias, and profile boundary.
status: active
authority: normative
owner: docs
tags: [okf, llm-wiki, docs, repo-memory]
source_refs: [https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md, https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing]
code_refs: [apps/decodex/src/cli.rs, apps/decodex/src/docs_okf.rs, plugins/decodex/skills/docs/SKILL.md]
related: [../policy.md, ../reference/research-concepts.md]
drift_watch: [decodex okf, decodex docs, docs lint, okf profile, docs alias]
last_verified: 2026-06-17
---

# OKF Knowledge Layer

Purpose: Define how Decodex separates portable OKF knowledge-bundle behavior from the
Decodex repository's strict documentation profile.

Status: normative

Read this when: You are designing OKF/LLM Wiki commands, skills, plugin surfaces, or
documentation policy that should work beyond this repository.

Not this document: The detailed Decodex lane taxonomy, research promotion contract,
or runtime docs-impact checkpoint schema.

Defines: The command naming model, profile stack, and boundary between `okf`,
`docs`, LLM Wiki behavior, and Decodex-specific policy.

## Concepts

`okf` is the portable knowledge-bundle engine. It reads, validates, queries, and
renders Open Knowledge Format bundles regardless of whether the bundle lives under
`docs/`, `wiki/`, a generated catalog export, or another repository path.

`llm-wiki` is the agent retrieval and maintenance method layered on top of OKF. It
uses progressive indexes, Markdown links, backlinks, tags, source references, code
references, and update logs so agents can find the smallest relevant concept and keep
the graph current.

`docs` is this repository's default OKF bundle location and convenience command
surface. In this repository, `decodex docs` is an alias for operating on `docs/` with
the Decodex profile.

`decodex` is the strict profile for this repository. It adds lane taxonomy,
authority classes, research-promotion rules, drift gates, and docs-impact integration.
Those constraints must not be treated as the OKF core contract.

## Command Model

Portable commands use the `okf` noun:

```sh
decodex okf check docs/ --profile core
decodex okf graph docs/ --json
decodex okf find docs/ --tag runtime
decodex okf route docs/ "change lane control behavior"
```

Repository-local convenience commands use the `docs` noun:

```sh
decodex docs check
decodex docs graph
decodex docs find --tag runtime
decodex docs route "change lane control behavior"
```

The `docs` commands default to:

- bundle root: `docs/`
- profile: `decodex`

Do not introduce `decodex docs okf ...`, `decodex docs wiki ...`, or
`decodex docs llm-wiki ...` command nesting. That shape leaks implementation taxonomy
into the daily user surface and makes OKF look like a child feature of docs. OKF is
the portable engine; docs is only one bundle location.

## Profiles

Profiles are additive. A stricter profile may reject a bundle that a lower profile
can consume.

| Profile | Scope | Purpose |
| --- | --- | --- |
| `core` | OKF v0.1 conformance | Verify portable Markdown/frontmatter interoperability. |
| `wiki` | Core plus graph hygiene | Verify agent navigation, indexes, links, backlinks, logs, and retrieval fields. |
| `repo-memory` | Wiki plus repository anchors | Verify code references, source references, task routing hints, and drift watch fields without Decodex lane semantics. |
| `decodex` | Repo memory plus Decodex docs policy | Verify Decodex lanes, authority enums, research contracts, drift audits, docs impact, and completion gates. |

Core OKF consumption must remain permissive. Unknown concept types, unknown
frontmatter keys, missing recommended fields, missing indexes, and broken cross-links
do not make a bundle non-consumable at the core profile.

Decodex profile checks may be strict because they enforce this repository's
self-maintaining docs contract. Those checks are not a portable OKF requirement.

## Producer And Consumer Boundary

Producer behavior writes or updates concepts. It should preserve unknown frontmatter
keys, ordinary Markdown body content, and existing links unless the task explicitly
changes them.

Consumer behavior reads concepts. It should tolerate unknown concept types and
producer-specific fields. Query, graph, and route commands must return useful partial
results even when a bundle is not clean enough to pass the strictest profile.

## Skill Boundary

Portable OKF skills own cross-repository behavior:

- initialize a bundle
- check profile conformance
- query frontmatter and Markdown links
- build graph/backlink views
- maintain indexes and logs
- route a task to the smallest relevant concepts

Decodex docs skills are wrappers around those behaviors for this repository. They may
apply Decodex profile constraints, but the portable OKF skill family must not depend
on Linear, Decodex runtime state, research promotion, or landing policy.

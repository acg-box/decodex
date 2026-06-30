---
type: Spec
title: OKF Knowledge Layer
description: Defines the portable OKF engine, LLM Wiki profile, Decodex docs command surface, and profile boundary.
status: active
authority: normative
owner: docs
tags: [okf, llm-wiki, docs, repo-memory]
source_refs: [https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md, https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing, https://developers.openai.com/codex/guides/agents-md, https://code.claude.com/docs/en/memory, https://docs.github.com/en/copilot/how-tos/copilot-on-github/customize-copilot/add-custom-instructions/add-repository-instructions]
code_refs: [apps/decodex/src/cli.rs, apps/decodex/src/docs_okf.rs, plugins/decodex/references/routing.md, plugins/knowledge/references/okf-layer.md, plugins/knowledge/skills/okf/SKILL.md, plugins/knowledge/skills/repo-memory/SKILL.md, plugins/knowledge/skills/docs/SKILL.md]
related: [../policy.md, ../reference/docs-knowledge-map.md, ../reference/research-concepts.md, ../evidence/decodex-plugin-eval.md]
drift_watch: [decodex okf, decodex docs, docs check, okf profile, docs command surface, okf skill]
last_verified: 2026-06-18
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

`llm-wiki` is the agent navigation and maintenance method layered on top of OKF. It
uses progressive indexes, Markdown links, backlinks, tags, source references, code
references, and update logs so agents can start from small entrypoints, reach the
owning concept, and keep the graph current.

Retrieval systems are outside this contract. Ranking, reranking, embeddings, lexical
scorers, route benchmarks, and top-N hit rates may consume an OKF/LLM Wiki bundle,
but they are not the OKF format and are not required LLM Wiki behavior.

`docs` is this repository's default OKF bundle location and convenience command
surface. In this repository, `decodex docs` operates on `docs/` with the Decodex
profile.

`decodex` is the strict profile for this repository. It adds lane taxonomy,
authority classes, research-promotion rules, drift gates, and docs-impact integration.
Those constraints must not be treated as the OKF core contract.

## Command Model

Portable commands use the `okf` noun:

```sh
decodex okf init docs/ --profile repo-memory
decodex okf check docs/ --profile core
decodex okf graph docs/ --json
decodex okf find docs/ --tag runtime
```

Repository-local convenience commands use the `docs` noun:

```sh
decodex docs check
decodex docs graph
decodex docs find --tag runtime
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
| `wiki` | Core plus graph hygiene | Verify agent navigation, indexes, links, backlinks, logs, and lookup fields. |
| `repo-memory` | Wiki plus repository anchors | Verify code references, source references, task-navigation hints, and drift watch fields without Decodex lane semantics. |
| `decodex` | Repo memory plus Decodex docs policy | Verify Decodex lanes, authority enums, research contracts, drift audits, docs impact, and completion gates. |

Core OKF consumption must remain permissive. Unknown concept types, unknown
frontmatter keys, missing recommended fields, missing indexes, and broken cross-links
do not make a bundle non-consumable at the core profile.

`decodex okf init` scaffolds portable profiles only: `core`, `wiki`, and
`repo-memory`. It writes `index.md`, `log.md`, and `overview.md` when those files are
absent or already match the scaffold, then runs the matching profile check. It refuses
to overwrite divergent existing content. The `decodex` profile is a repository-specific
docs contract and is not generated by portable OKF init.

Decodex profile checks may be strict because they enforce this repository's
self-maintaining docs contract. Those checks are not a portable OKF requirement.

## Producer And Consumer Boundary

Producer behavior writes or updates concepts. It should preserve unknown frontmatter
keys, ordinary Markdown body content, and existing links unless the task explicitly
changes them.

Consumer behavior reads concepts. It should tolerate unknown concept types and
producer-specific fields. Query and graph commands must return useful partial results
even when a bundle is not clean enough to pass the strictest profile. Retrieval
systems that rank concepts are separate consumers and must not define OKF conformance.

## Skill Boundary

Portable OKF skills own cross-repository behavior:

- initialize a bundle
- check profile conformance
- query frontmatter and Markdown links
- build graph/backlink views
- maintain indexes and logs

The Knowledge plugin exposes these portable skills as `okf`, `docs`, `docs-drift`,
and `repo-memory`. `okf` operates init/check/find/graph/query/maintain bundle
surfaces. `repo-memory` owns write/evaluate/curate modes: it reads repository
evidence, writes canonical concepts, turns static checks plus owner-navigation
questions into quality reports, and uses that evidence to repair weak owners, missing
links, orphan concepts, duplicate claims, stale references, and graph decay.

The CLI does not independently generate high-quality repository knowledge. Agents or
humans still judge owner correctness, classify misses, and author durable claims. OKF
commands make those judgments repeatable by supplying profile checks, graph counts,
and query output.

Decodex-owned context intake lives in `plugins/decodex/references/routing.md`. It
defines when agents should read docs indexes and owner concepts before implementation
and how to recover a missed docs completion gate. Host bootstrap instructions should
route to Decodex rather than copying those procedures.

Decodex docs skills are wrappers around those behaviors for this repository. They may
apply Decodex profile constraints, but the portable OKF skill family must not depend
on Linear, Decodex runtime state, research promotion, or landing policy.

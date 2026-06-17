---
type: Reference
title: Docs Knowledge Map
description: Explains how the Decodex docs bundle uses OKF and LLM Wiki routing, and where their value appears in this repository.
status: active
authority: current_state
owner: docs
tags: [docs, okf, llm-wiki, repo-memory, reference]
source_refs: [https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md, https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing, https://llmstxt.org/, https://diataxis.fr/, https://developers.openai.com/codex/guides/agents-md, https://docs.github.com/en/copilot/how-tos/copilot-on-github/customize-copilot/add-custom-instructions/add-repository-instructions]
code_refs: [apps/decodex/src/docs_okf.rs, apps/decodex/src/cli.rs, docs/index.md, docs/policy.md, docs/spec/okf-knowledge-layer.md, plugins/decodex/skills/repo-memory-writer/SKILL.md, plugins/decodex/skills/repo-memory-curator/SKILL.md]
related: [../policy.md, ../spec/okf-knowledge-layer.md, ../evidence/docs-self-iteration.md, ./build-test-run.md, ./workspace-layout.md]
drift_watch: [decodex docs check, decodex okf graph docs, decodex okf route docs, docs index, docs lane index, okf orphan concepts]
last_verified: 2026-06-17
---

# Docs Knowledge Map

Purpose: Explain how the repository docs currently apply OKF and LLM Wiki ideas, and
where those ideas provide practical value during agent work.

Read this when: You need to evaluate the docs knowledge-base shape, decide where to
place a new durable docs claim, or understand why `docs/` uses OKF and LLM Wiki
routing instead of ordinary prose-only documentation.

Not this document: The normative OKF command/profile contract, the full documentation
policy, or a step-by-step docs migration runbook.

Covers: Current bundle shape, practical OKF value, practical LLM Wiki value, graph
maintenance anchors, and retrieval-quality observations.

## Current Bundle Shape

`docs/` is one Markdown OKF bundle with a strict Decodex profile:

- [`../index.md`](../index.md) is the progressive-disclosure entrypoint.
- [`../policy.md`](../policy.md) owns the document contract, lane taxonomy, and
  docs-impact gate.
- [`../log.md`](../log.md) records maintenance events.
- Lane indexes route by question type: spec, runbook, reference, decision, research,
  and evidence.
- Non-index, non-log Markdown files are typed concepts with frontmatter.

This keeps repository knowledge inspectable as plain Markdown while still giving agents
structured fields for routing, graph checks, and drift review.

## OKF Value

OKF provides the data contract for repository knowledge:

- `type`, `description`, `tags`, `source_refs`, `code_refs`, `related`, and
  `drift_watch` turn prose files into queryable concepts.
- Profile checks separate portable knowledge-bundle validity from this repository's
  stricter Decodex workflow policy.
- `decodex docs check` blocks broken links, malformed references, missing required
  concept fields, stale drift-audit structure, and non-Markdown docs artifacts.
- `decodex okf init <root> --profile repo-memory` makes the same framework reusable in
  another repository without inheriting Decodex-specific lane rules.

The practical value is not that agents read more files. The value is that agents can
read fewer, better-targeted files and still preserve source, code, and drift evidence
when they update repository knowledge.

## LLM Wiki Value

The LLM Wiki layer is the retrieval and maintenance behavior on top of OKF:

- `docs/index.md` and lane indexes answer "where should I look first?"
- Markdown links and `related` frontmatter form a navigable concept graph.
- `decodex okf route docs "<intent>"` gives a quick routing sanity check before a
  broad read.
- Duplicate claims are discouraged because each concept owns one topic and links to
  neighbors instead of copying their claims.
- Drift audits connect human-readable claims back to commands, code, and reusable
  public-safe evidence.

The practical value is reduced semantic drift: when a command, workflow, or docs
boundary changes, the agent has an explicit owner concept, nearby evidence anchors,
and a graph path to related concepts that may also need updates.

## Graph Maintenance Anchors

The current graph has some specialized concepts that are valid but easy to miss from a
plain index scan. This map keeps them connected to the repository-memory graph:

- Static site surface:
  [`../spec/site-contract.md`](../spec/site-contract.md),
  [`../decisions/static-public-site.md`](../decisions/static-public-site.md), and
  [`../runbook/github-pages-deploy.md`](../runbook/github-pages-deploy.md).
- Release and external maintenance:
  [`../runbook/release-readiness.md`](../runbook/release-readiness.md),
  [`../runbook/linear-archive-hygiene.md`](../runbook/linear-archive-hygiene.md), and
  [`./github-operations.md`](./github-operations.md).
- Repository quality inventory:
  [`./build-test-run.md`](./build-test-run.md) and
  [`./test-suite.md`](./test-suite.md).
- Plugin source ownership:
  [`../decisions/decodex-plugin-source.md`](../decisions/decodex-plugin-source.md).
- Local runtime handoff evidence:
  [`../spec/agent-evidence.md`](../spec/agent-evidence.md).

Those concepts should stay in their owning lanes. This map only provides a retrieval
edge so graph-based readers can discover them without treating lane indexes as concept
authority.

Current readback: `decodex okf graph docs` reports 38 concepts, 117 edges, 0 broken
links, and 0 orphan concepts.

## Evaluation

For this repository, OKF is most valuable as a validation and evidence schema. It makes
docs changes testable, lets the CLI find malformed references before review, and
creates a portable profile stack for other repos.

LLM Wiki is most valuable as a routing discipline. It turns a growing docs tree into a
task-directed graph so agents can start from the smallest relevant concept, follow
explicit links, and avoid broad context loading.

The combined value appears when both layers are used together:

- OKF says whether a concept is well shaped.
- LLM Wiki says whether the concept is discoverable and connected.
- Drift audit says whether the concept is still true against code and command output.

The remaining maintenance risk is graph decay: a concept can pass shape checks while
still being hard to discover. Periodic `decodex okf graph docs` and targeted
`decodex okf route docs "<intent>"` probes catch that class of issue.

## Cross-Repository Use

In another repository, the Decodex plugin can now provide three distinct layers:

- `decodex okf init <root> --profile repo-memory` creates the portable scaffold.
- `repo-memory-writer` guides Codex to read repository evidence and write canonical
  concepts instead of generated summaries.
- `repo-memory-curator` guides later graph repair, orphan triage, route benchmarks,
  and metadata/link tuning after real usage exposes misses.
- `decodex okf check/graph/route` verifies shape, graph health, and task routing.

The expected first useful output is not a complete encyclopedia. It is a small,
source-backed map of setup, tests, repository layout, automation resources, contracts,
procedures, decisions, and drift-watch points that future agents can route through.

---
type: "Research Contract"
title: "Research Runtime Boundary"
description: "Superseded provenance for the research runtime-boundary investigation now promoted into OKF owner concepts."
status: superseded
authority: non_authoritative
owner: research
tags: [research, runtime, mcp, decision-contract, okf]
code_refs: [apps/decodex/src/docs_okf.rs, apps/decodex/src/mcp.rs, apps/decodex/src/research_design.rs, apps/decodex/src/execution_program.rs, apps/decodex/src/program_intake.rs, docs/spec/loop-runtime.md, docs/spec/runtime.md, docs/reference/research-concepts.md, plugins/decodex/skills/research/SKILL.md, plugins/decodex/references/research-contract.md, plugins/decodex/references/research-lifecycle.md]
related: [index.md, ../policy.md, ../decisions/okf-research-knowledge-lifecycle.md, ../reference/research-concepts.md, ../spec/loop-runtime.md, ../spec/runtime.md, ../decisions/mcp-capability-gateway-and-skill-slimming.md]
promotes_to: [docs/decisions, docs/reference, docs/spec, docs/evidence]
last_verified: 2026-06-18
---

# Research Runtime Boundary

Purpose: Preserve the provenance of the research runtime-boundary investigation after
its accepted knowledge moved into OKF owner concepts.

Read this when: You need the historical evidence path for why checked-in research is
Markdown-only and why runtime Decision Contracts remain structured runtime state.

Not this document: Current truth. Use
[`../decisions/okf-research-knowledge-lifecycle.md`](../decisions/okf-research-knowledge-lifecycle.md),
[`../reference/research-concepts.md`](../reference/research-concepts.md),
[`../spec/loop-runtime.md`](../spec/loop-runtime.md), and
[`../spec/runtime.md`](../spec/runtime.md).

Disposition: `promote_and_supersede`.

## Question

What is the correct boundary between checked-in research concepts, runtime-local
Decision Contracts, MCP readback, and future execution research?

## Scope

In scope: checked-in research format, runtime-local Decision Contracts, MCP readback,
and future research triggers.

Out of scope: treating research as execution authority, storing generated runtime
state in checked-in docs, or using research as a primary facts/rationale/evidence
owner after promotion.

## Evidence

| ID | Class | Sources | Supports |
| --- | --- | --- | --- |
| E1 | repo_source | `docs/policy.md`, `docs/reference/research-concepts.md` | `docs/research/` is a Markdown OKF `Research Contract` lane and is non-authoritative. |
| E2 | repo_source | `apps/decodex/src/docs_okf.rs` | The docs checker rejects non-Markdown artifacts under `docs/` and validates research headings. |
| E3 | repo_source | `apps/decodex/src/research_design.rs`, `docs/spec/loop-runtime.md` | Decodex research produces latent runtime-local Decision Contracts that require explicit promotion. |
| E4 | repo_source | `apps/decodex/src/mcp.rs`, `docs/spec/runtime.md` | MCP exposes checked-in research as Markdown and runtime Decision Contracts as JSON readback. |
| E5 | repo_source | `docs/decisions/okf-research-knowledge-lifecycle.md` | Accepted research now follows OKF disposition and LLM Wiki hygiene rules. |

## Options

1. Store checked-in research as JSON.
2. Store all research only in runtime state.
3. Keep checked-in research as Markdown OKF concepts and keep machine structure in
   runtime Decision Contracts.

## Judgment

Selected option: Keep checked-in research as Markdown OKF concepts and keep machine
structure in runtime Decision Contracts.

The accepted knowledge is no longer owned here. Current owner concepts now carry the
OKF lifecycle, runtime, and MCP boundaries.

## Challenge

Resolved objection: Markdown research is less schema-bound than JSON.

Resolution: runtime Decision Contracts are the schema-bound machine surface; checked-in
research is an OKF knowledge concept optimized for routing and review.

Resolved objection: useful research history may disappear after promotion.

Resolution: useful provenance remains only as a compact `superseded` research concept
that points to authoritative owners. Knowledge retention is explicit in OKF links and
indexes.

## Decision

Terminal status: `decision_ready`.

Decision: Keep `docs/research/` as Markdown-only OKF `Research Contract` concepts,
keep runtime-local `decodex.decision_contract/1` records as structured runtime state,
and manage accepted research through OKF promotion/disposition.

## Promotion

Promoted to:

- [`../decisions/okf-research-knowledge-lifecycle.md`](../decisions/okf-research-knowledge-lifecycle.md)
- [`../reference/research-concepts.md`](../reference/research-concepts.md)
- [`../spec/loop-runtime.md`](../spec/loop-runtime.md)
- [`../spec/runtime.md`](../spec/runtime.md)
- `plugins/decodex/references/research-promotion.md`

## Drift Impact

- `docs/research/` must remain Markdown-only.
- Superseded research must not outrank owner concepts in normal LLM Wiki routing.
- Research promotion must update owner concepts, indexes, links, and plugin guidance.

## Citations

- [`../policy.md`](../policy.md)
- [`../decisions/okf-research-knowledge-lifecycle.md`](../decisions/okf-research-knowledge-lifecycle.md)
- [`../reference/research-concepts.md`](../reference/research-concepts.md)
- [`../spec/loop-runtime.md`](../spec/loop-runtime.md)
- [`../spec/runtime.md`](../spec/runtime.md)

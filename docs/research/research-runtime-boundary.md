---
type: "Research Contract"
title: "Research Runtime Boundary"
description: "Defines the current research format boundary between checked-in Markdown research concepts, runtime Decision Contracts, MCP readback, and future execution research."
status: active
authority: non_authoritative
owner: research
tags: [research, runtime, mcp, decision-contract, okf]
code_refs: [apps/decodex/src/docs_okf.rs, apps/decodex/src/mcp.rs, apps/decodex/src/research_design.rs, apps/decodex/src/execution_program.rs, apps/decodex/src/program_intake.rs, docs/spec/loop-runtime.md, docs/spec/runtime.md, docs/reference/research-concepts.md, plugins/decodex/skills/research/SKILL.md, plugins/decodex/references/research-contract.md, plugins/decodex/references/research-lifecycle.md]
related: [index.md, ../policy.md, ../reference/research-concepts.md, ../spec/loop-runtime.md, ../spec/runtime.md, ../decisions/mcp-capability-gateway-and-skill-slimming.md]
promotes_to: [docs/spec, docs/reference, docs/decisions]
last_verified: 2026-06-18
---

# Research Runtime Boundary

Purpose: State the current research boundary in the latest Decodex docs format.

Read this when: You need to decide whether research belongs in checked-in docs,
runtime-local Decision Contracts, MCP resources, or a promoted execution plan.

Not this document: A durable specification, operator runbook, or promotion approval.

## Question

What is the correct current shape for Decodex research so checked-in docs stay
Markdown-only while runtime and MCP can still expose structured machine-readable
state?

## Scope

In scope:

- Checked-in `docs/research/` format.
- Runtime-local `decodex.decision_contract/1` research storage.
- MCP resource exposure for checked-in research and runtime Decision Contracts.
- Future research triggers that still need fresh evidence before promotion.

Out of scope:

- Importing event-log storage into checked-in docs.
- Treating a research concept as execution authority.
- Queueing Linear issues, dispatching Program nodes, or mutating runtime state from
  checked-in research.

## Evidence

| ID | Class | Sources | Supports |
| --- | --- | --- | --- |
| E1 | repo_source | `docs/policy.md`, `docs/reference/research-concepts.md`, `plugins/decodex/skills/research/SKILL.md` | Checked-in research belongs in Markdown OKF `Research Contract` concepts under `docs/research/`. |
| E2 | repo_source | `apps/decodex/src/docs_okf.rs` | The docs checker requires `docs/research/index.md`, validates Research Contract headings, and rejects JSON or generated state anywhere under `docs/`. |
| E3 | repo_source | `apps/decodex/src/research_design.rs`, `docs/spec/loop-runtime.md` | Decodex research/design produces latent runtime-local Decision Contracts and requires explicit promotion before execution authority exists. |
| E4 | repo_source | `apps/decodex/src/execution_program.rs`, `apps/decodex/src/program_intake.rs` | Accepted research can become executable only after promotion and Program Intake shaping. |
| E5 | repo_source | `apps/decodex/src/mcp.rs`, `docs/spec/runtime.md` | MCP exposes checked-in research as Markdown resources and exposes runtime Decision Contracts separately as JSON readback. |
| E6 | gap | `docs/spec/app-server.md`, `README.md` | Remote execution beyond the current app-server/local-runtime model still needs fresh evidence for status, diff, apply, issue/run/attempt provenance, repo gate, and PR handoff authority. |

## Options

1. Store checked-in research as JSON.
   This gives machine structure but violates the current Markdown-only OKF docs
   contract and blurs docs authority with runtime state.

2. Store all research only in runtime SQLite.
   This preserves machine authority but loses durable, reviewable, source-controlled
   research context for agents.

3. Keep checked-in research as Markdown OKF concepts and keep machine structure in
   runtime Decision Contracts.
   This preserves readable repository knowledge, keeps runtime JSON where it belongs,
   and gives MCP a clear split between Markdown docs resources and JSON runtime
   readback.

## Judgment

Selected option: Keep checked-in research as Markdown OKF concepts and keep machine
structure in runtime Decision Contracts.

This fits the current Decodex architecture:

- `docs/research/` is a non-authoritative Markdown OKF lane.
- `decodex research compile` and `decodex research promote` operate on runtime-local
  Decision Contracts.
- MCP should expose `decodex://research/{concept}` as Markdown and
  `decodex://decision-contracts/{contract_id}` as runtime JSON.
- Promotion, not file presence, grants authority to shape execution work.

## Challenge

Resolved objection: Markdown research is less schema-bound than JSON.

Resolution: The schema-bound surface already exists in runtime Decision Contracts.
Checked-in docs optimize for reviewability, routing, citations, and semantic drift
checks. Duplicating machine state into checked-in docs would create two sources of
truth.

Resolved objection: Markdown concepts may lose useful future direction that was
captured during investigation.

Resolution: Useful direction should be restated as current research questions,
validation expectations, and future trigger conditions. Raw run storage should not be
kept in `docs/`.

## Decision

Terminal status: `decision_ready`.

Decision: Keep `docs/research/` as Markdown-only OKF Research Contract concepts.
Keep runtime-local `decodex.decision_contract/1` records as the structured research
machine surface. MCP must preserve that split by exposing checked-in research as
Markdown and runtime Decision Contracts as JSON.

Future research should start from fresh evidence when:

- Remote execution needs proof for task status, diff/status readback, apply
  semantics, issue/run/attempt provenance, repo gates, and PR handoff authority.
- A landing receipt schema is needed beyond `decodex/commit/1` plus GitHub
  merge/admin-merge readback.
- A plan/progress adapter needs lifecycle authority beyond current progress memory.
- MCP clients need more than observe/plan/operate/admin resource and tool surfaces.

## Promotion

Promotion target: no immediate promotion.

Promote only a specific accepted conclusion into:

- `docs/spec/` when it defines required runtime or MCP behavior.
- `docs/reference/` when it records current implemented structure.
- `docs/decisions/` when it records durable design rationale.

## Drift Impact

- `docs/research/` must contain Markdown only.
- `decodex docs check` must reject JSON or generated state under `docs/`.
- MCP research resources must stay `text/markdown`.
- Runtime Decision Contract resources may stay `application/json`.

## Citations

- [`../policy.md`](../policy.md)
- [`../reference/research-concepts.md`](../reference/research-concepts.md)
- [`../spec/loop-runtime.md`](../spec/loop-runtime.md)
- [`../spec/runtime.md`](../spec/runtime.md)
- [`../decisions/mcp-capability-gateway-and-skill-slimming.md`](../decisions/mcp-capability-gateway-and-skill-slimming.md)
- [`../../plugins/decodex/skills/research/SKILL.md`](../../plugins/decodex/skills/research/SKILL.md)
- [`../../plugins/decodex/references/research-contract.md`](../../plugins/decodex/references/research-contract.md)
- [`../../plugins/decodex/references/research-lifecycle.md`](../../plugins/decodex/references/research-lifecycle.md)

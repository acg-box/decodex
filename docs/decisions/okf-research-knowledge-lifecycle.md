---
type: "Decision"
title: "OKF Research Knowledge Lifecycle"
description: "Defines how Decodex promotes research into OKF owners while preserving LLM Wiki retrieval quality."
status: active
authority: rationale
owner: docs
tags: [decision, okf, research, llm-wiki]
code_refs: [apps/decodex/src/docs_okf.rs, apps/decodex/src/plugin_surface_tests.rs, plugins/decodex/references/research-promotion.md, plugins/decodex/references/docs-wiki.md]
related: [../policy.md, ../reference/research-concepts.md, ../research/index.md, ../evidence/index.md, ../spec/okf-knowledge-layer.md]
last_verified: 2026-06-18
---

# OKF Research Knowledge Lifecycle

Status: accepted
Date: 2026-06-18
Question: How should Decodex manage research promotion in an OKF knowledge base that
is also used as an LLM Wiki?

## Context

`docs/research/` is useful for bounded investigation, but LLM agents route through
indexes, frontmatter, links, and short descriptions. If accepted research continues to
carry current facts or rationale, retrieval can surface non-authoritative research
before the concept that owns the truth.

## Decision

Research promotion is an OKF knowledge operation. Accepted research is split by owner:

- durable rationale, selected tradeoffs, and rejected alternatives move to
  `docs/decisions/`
- required behavior, schemas, state, and invariants move to `docs/spec/`
- current implementation facts and structure move to `docs/reference/`
- operator procedures move to `docs/runbook/`
- reusable proof and drift-audit material move to `docs/evidence/`
- agent-facing workflow rules move to `plugins/decodex/skills/`
- executable behavior and verification authority move to code and tests

`docs/research/` keeps only active unresolved research or explicitly superseded
provenance. It is not a facts database, decisions database, evidence database, or
history fallback.

## Disposition

Every completed research concept receives one knowledge disposition:

- `continue`: unresolved work remains active in `docs/research/`
- `promote_and_supersede`: durable owners receive the accepted knowledge, while a
  compact `status: superseded` research concept remains for provenance
- `promote_and_retire`: durable owners fully absorb the knowledge, and research leaves
  active LLM Wiki routing
- `reject_or_deprecate`: rejected or stale research is kept only when it has retrieval
  value as a decision, evidence concept, or `status: deprecated` research concept

## LLM Wiki Hygiene

Promotion must update lane indexes, `related`, `promotes_to`, descriptions, and
status fields. Superseded research may point to authoritative owners, but it must not
repeat current truth or compete with those owners in normal routing.

Knowledge retention must be explicit in OKF concepts and links rather than relying on
out-of-band history.

## Consequences

- `docs/decisions/` is the durable owner for rationale produced by accepted research.
- `docs/evidence/` is a valid promotion target when research yields reusable proof.
- `docs/research/` stays useful for investigation and provenance without becoming a
  parallel authority lane.
- Plugin skills must route future agents through owner concepts first and research
  only when the task asks for latent evidence, unresolved questions, or provenance.

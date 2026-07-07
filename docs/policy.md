---
type: Policy
title: Documentation Policy
description: Defines Decodex repository documentation ownership after generic knowledge workflows moved to external team plugins.
status: active
authority: normative
owner: docs
tags: [docs, policy]
code_refs: [README.md, Makefile.toml, plugins/decodex/.codex-plugin/plugin.json]
related: [index.md, log.md]
last_verified: 2026-07-07
---

# Documentation Policy

## Purpose

`docs/` is the checked-in Decodex product, runtime, operator, and repository reference
surface. It is not the generic team knowledge base, not the research workflow owner,
and not the source for portable agent-workflow plugins.

Generic repository knowledge, OpenWiki-backed maintenance, research methods,
skeptic review, and general codebase execution discipline live in external
installed team plugins.
Decodex may link to those plugins as external owners, but this repository does not
define their generic methods.

## Authority

Durable Decodex documentation should stay in the smallest owning lane:

| Lane | Location | Owns |
| --- | --- | --- |
| Spec | `docs/spec/` | Runtime, operator, site, and protocol requirements. |
| Runbook | `docs/runbook/` | Operator procedures and validation sequences. |
| Reference | `docs/reference/` | Current repository layout, commands, and implementation maps. |
| Decisions | `docs/decisions/` | Decodex-specific rationale. |
| Evidence | `docs/evidence/` | Public-safe proof for Decodex product/runtime claims. |

Runtime state, tracker state, local agent evidence, generated automation output, and
private operator data are not documentation authority.

## Writing Rules

- Keep documentation in Markdown.
- Prefer one authoritative page per topic and link instead of copying claims.
- Start substantive documents with a short purpose section that states when to read
  the page and what it does not cover.
- Keep code, command, and config references current when a documented behavior
  changes.
- Do not add generated knowledge-store files to Decodex docs.
- Do not define generic team research, knowledge, codebase, or deliberation workflow
  policy in this repository.

## Integration

Decodex lanes may still classify documentation impact before completion because
operator-facing behavior, commands, schemas, and runbooks must stay aligned with the
runtime. The owning update is the Decodex page that describes the changed product or
runtime behavior.

When a change needs generic repository knowledge maintenance or external best-practice
research, route to the installed team `knowledge` or `research` plugin and promote any
accepted result back into the appropriate Decodex document only after it becomes
Decodex product/runtime authority.

## Validation

Use the repository task runner for documentation validation. `cargo make check`
remains the broad readiness command; narrower checks should be justified by the
touched surface.

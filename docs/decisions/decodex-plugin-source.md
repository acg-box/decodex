---
type: "Decision"
title: "Decodex Plugin Source"
description: "Where should reusable agent-facing Decodex and companion plugin instructions live?"
status: active
authority: rationale
owner: docs
tags: [decision]
last_verified: 2026-06-22
---
# Decodex Plugin Source

Status: accepted
Date: 2026-05-09
Question: Where should reusable agent-facing Decodex and companion plugin instructions live?
Decision: Maintain the canonical Decodex lifecycle plugin in this repository under
`plugins/decodex/`, and maintain companion plugin sources under `plugins/knowledge/`,
`plugins/codebase/`, and `plugins/deliberation/`.
Consequences: Decodex runtime guidance can change with the runtime, while reusable
repository execution policy and generic challenge methods stop being Decodex-owned.
Host bootstrap instructions compose the installed plugins without copying their
procedures.

## Context

Decodex has two supported use modes:

- runtime operations for human-driven CLI use, registered projects, retained lanes,
  service-scoped Linear labels, issue-scoped tracker tools, review handoff, recovery,
  closeout, and cleanup
- commit creation and PR landing as separate high-risk authority surfaces

Earlier Decodex instructions combined runtime lifecycle, codebase method, docs/OKF,
semantic drift, and challenge rules in one plugin. That made Decodex look like the
owner of generic repository execution policy and generic skeptic review. The repository
still hosts the plugin sources, but ownership is split by authority.

## Decision

`plugins/decodex/` is the canonical installable plugin source for Decodex lifecycle
instructions.

The Decodex plugin should own Decodex-specific procedures and mode routing:

- `decodex` for routing docs, research, ops, commit, and landing surfaces
- `planning` for Decodex-native issue briefing, issue splitting, dispatch readiness,
  dependencies, and concurrency
- `decodex-ops` for operator CLI use, retained-lane control-plane use, and Decodex
  service labels
- `routing` for OKF/LLM Wiki context intake, docs completion gates, and late docs-skill
  recovery
- `commit` for human-driven `decodex commit`
- `land` for explicit human-driven `decodex land`

Companion plugins:

- `plugins/knowledge/` owns docs, OKF/LLM Wiki, semantic drift, source-backed
  repo-memory, and knowledge writeback skills.
- `plugins/codebase/` owns checked-in command authority, task-runner structure,
  dependency policy, review repair, verification, debugging, and dynamic support-agent
  boundaries.
- `plugins/deliberation/` owns generic scout, grill, challenge, and skeptic review.

The plugin must route to `apps/decodex/src/`, `docs/spec/`, `docs/runbook/`, `docs/reference/`,
registered project `WORKFLOW.md`, and registered project `project.toml` instead of
copying their full contracts.

For review handoff and compact review readback, plugin skills must point operators to
runtime evidence surfaces such as `issue_review_checkpoint.review_cost_control`,
`decodex evidence`, and `decodex recover review-handoff` diagnosis. They must not copy
the review-cost, handoff-recovery, or tracker invariants out of the runtime specs.

## Consequences

- Decodex-specific skill updates can land with matching runtime, spec, and runbook
  updates.
- Host bootstrap files can shrink to short plugin routing and avoid carrying
  codebase, debugging, drift, research, review, challenge, or verification rules.
- Decodex issue briefing belongs to the Decodex plugin instead of a separate delivery
  workflow. Generic progress, handoff, review, landing, and closeout state remains
  runtime-owned rather than skill-owned.
- `~/.codex/AGENTS.md` remains a portable bootstrap surface, not a Decodex runtime or
  operator contract.
- Semantic drift audits for plugin behavior changes should include the owning plugin
  directory when behavior affects agent-facing usage instructions.

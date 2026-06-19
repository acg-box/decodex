---
type: "Decision"
title: "Decodex Plugin Source"
description: "Where should reusable agent-facing Decodex usage instructions live?"
status: active
authority: rationale
owner: docs
tags: [decision]
last_verified: 2026-06-19
---
# Decodex Plugin Source

Status: accepted
Date: 2026-05-09
Question: Where should reusable agent-facing Decodex usage instructions live?
Decision: Maintain the canonical Decodex plugin in this repository under
`plugins/decodex/`. Decodex also owns reusable repo-work guidance, including command
authority, task-runner structure, engineering defaults, dependency policy, review repair,
verification, debugging, semantic drift, research, and dynamic support-agent
boundaries.
Consequences: Decodex runtime and usage guidance can now change in the same repository
lane. Host bootstrap instructions may route into Decodex without copying Decodex
procedures or carrying a separate workflow plugin dependency.

## Context

Decodex has two supported use modes:

- runtime operations for human-driven CLI use, registered projects, retained lanes,
  service-scoped Linear labels, issue-scoped tracker tools, review handoff, recovery,
  closeout, and cleanup
- commit creation and PR landing as separate high-risk authority surfaces

Earlier Decodex instructions lived in generic repo-work skills while the CLI and
lifecycle were still settling. That split created coupling between host config,
repo-work skill text, and Decodex workflow details. The Decodex-specific authority and
the reusable repo-work method now live together in this repository because Decodex owns
the runtime code, registered project contracts, operator docs, and installable
agent-facing guidance.

## Decision

`plugins/decodex/` is the canonical installable plugin source for Decodex usage
instructions.

The plugin should own reusable agent-facing procedures and mode routing:

- `decodex` for routing repo-work, docs, research, ops, commit, and landing surfaces
- `repo-work` for checked-in command authority, task-runner structure, configuration
  contracts, engineering defaults, dependency policy, review repair, verification, and
  dynamic support-agent boundaries
- `planning` for Decodex-native issue briefing, issue splitting, dispatch readiness,
  dependencies, and concurrency
- `decodex-ops` for operator CLI use, retained-lane control-plane use, and Decodex
  service labels
- `routing` for OKF/LLM Wiki context intake, docs completion gates, and late docs-skill
  recovery
- `commit` for human-driven `decodex commit`
- `land` for explicit human-driven `decodex land`

The plugin must route to `apps/decodex/src/`, `docs/spec/`, `docs/runbook/`, `docs/reference/`,
registered project `WORKFLOW.md`, and registered project `project.toml` instead of
copying their full contracts.

## Consequences

- Decodex-specific skill updates can land with matching runtime, spec, and runbook
  updates.
- Host bootstrap files can shrink to short Decodex skill routing and avoid carrying
  repo-work, debugging, drift, research, review, or verification rules.
- Decodex issue briefing belongs to the Decodex plugin instead of a separate delivery
  workflow. Generic progress, handoff, review, landing, and closeout state remains
  runtime-owned rather than skill-owned.
- `~/.codex/AGENTS.md` remains a portable bootstrap surface, not a Decodex runtime or
  operator contract.
- Semantic drift audits for Decodex behavior changes should include `plugins/decodex/`
  when the behavior affects agent-facing usage instructions.

---
type: "Decision"
title: "Decodex Plugin Source"
description: "Where should reusable agent-facing Decodex usage instructions live?"
status: active
authority: rationale
owner: docs
tags: [decision]
last_verified: 2026-06-16
---
# Decodex Plugin Source

Status: accepted
Date: 2026-05-09
Question: Where should reusable agent-facing Decodex usage instructions live?
Decision: Maintain the canonical Decodex plugin in this repository under
`plugins/decodex/`. Generic Codex or Playbook repositories may keep portable routing
rules, but they should not own Decodex-specific CLI,
automation, tracker, label, review, landing, closeout, or project-contract details.
Consequences: Decodex runtime and usage guidance can now change in the same repository
lane. Playbook guidance that still mentions Decodex should point here or stay generic.

## Context

Decodex has two supported use modes:

- manual CLI use for human-driven development, status inspection, commit creation,
  PR landing, dry runs, project registration, and local operator checks
- runtime-owned automation for registered projects, retained lanes, service-scoped
  Linear labels, issue-scoped tracker tools, review handoff, landing, closeout, and
  cleanup

Earlier Decodex instructions lived in generic Playbook skills while the CLI and
lifecycle were still settling. The Decodex-specific authority now lives in this
repository because the generic Playbook repo does not own Decodex runtime code,
registered project contracts, or operator docs.

## Decision

`plugins/decodex/` is the canonical installable plugin source for Decodex usage
instructions.

The plugin should own reusable agent-facing procedures and mode routing:

- `decodex` for choosing manual CLI mode versus automation mode
- `planning` for Decodex-native issue briefing, issue splitting, dispatch readiness,
  dependencies, and concurrency
- `manual-cli` for operator CLI use
- `automation` for retained-lane control-plane use
- `commit` for human-driven `decodex commit`
- `land` for explicit human-driven `decodex land`
- `labels` for Decodex Linear labels

The plugin must route to `apps/decodex/src/`, `docs/spec/`, `docs/runbook/`, `docs/reference/`,
registered project `WORKFLOW.md`, and registered project `project.toml` instead of
copying their full contracts.

## Consequences

- Decodex-specific skill updates can land with matching runtime, spec, and runbook
  updates.
- Generic Playbook skills can shrink to generic repo discipline and explicit routing.
- Decodex issue briefing belongs to the Decodex plugin instead of a separate delivery
  workflow. Generic progress, handoff, review, landing, and closeout state remains
  runtime-owned rather than skill-owned.
- `~/.codex/AGENTS.md` remains a portable bootstrap surface, not a Decodex runtime or
  operator contract.
- Semantic drift audits for Decodex behavior changes should include `plugins/decodex/`
  when the behavior affects agent-facing usage instructions.

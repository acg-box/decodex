---
type: "Decision"
title: "Decodex Plugin Source"
description: "Where should reusable agent-facing Decodex usage instructions live?"
status: active
authority: rationale
owner: docs
tags: [decision]
last_verified: 2026-06-18
---
# Decodex Plugin Source

Status: accepted
Date: 2026-05-09
Question: Where should reusable agent-facing Decodex usage instructions live?
Decision: Maintain the canonical Decodex plugin in this repository under
`plugins/decodex/`. Generic Codex or repo-work plugins may keep host-level
composition or portable repository rules, but they should not own Decodex-specific
CLI, docs, OKF/LLM Wiki context intake, automation, tracker, label, review, landing,
closeout, or project-contract details.
Consequences: Decodex runtime and usage guidance can now change in the same repository
lane. Generic repo-work plugins should stay generic; host bootstrap instructions may
route into Decodex without copying Decodex procedures.

## Context

Decodex has two supported use modes:

- manual CLI use for human-driven development, status inspection, commit creation,
  PR landing, dry runs, project registration, and local operator checks
- runtime-owned automation for registered projects, retained lanes, service-scoped
  Linear labels, issue-scoped tracker tools, review handoff, landing, closeout, and
  cleanup

Earlier Decodex instructions lived in generic repo-work skills while the CLI and
lifecycle were still settling. The Decodex-specific authority now lives in this
repository because generic repo-work plugins do not own Decodex runtime code,
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
- `routing` for OKF/LLM Wiki context intake, docs completion gates, and late docs-skill
  recovery
- `commit` for human-driven `decodex commit`
- `land` for explicit human-driven `decodex land`
- `labels` for Decodex Linear labels

The plugin must route to `apps/decodex/src/`, `docs/spec/`, `docs/runbook/`, `docs/reference/`,
registered project `WORKFLOW.md`, and registered project `project.toml` instead of
copying their full contracts.

## Consequences

- Decodex-specific skill updates can land with matching runtime, spec, and runbook
  updates.
- Generic repo-work skills can shrink to repo discipline and avoid Decodex-specific
  names, commands, labels, or lifecycle gates.
- Decodex issue briefing belongs to the Decodex plugin instead of a separate delivery
  workflow. Generic progress, handoff, review, landing, and closeout state remains
  runtime-owned rather than skill-owned.
- `~/.codex/AGENTS.md` remains a portable bootstrap surface, not a Decodex runtime or
  operator contract.
- Semantic drift audits for Decodex behavior changes should include `plugins/decodex/`
  when the behavior affects agent-facing usage instructions.

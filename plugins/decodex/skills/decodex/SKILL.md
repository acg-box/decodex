---
name: decodex
description: Use as the conductor for Decodex work whenever the user asks to use, configure, operate, debug, or author Decodex. Routes between manual CLI workflows and runtime-owned automation workflows, and keeps Decodex-specific authority in the Decodex repo rather than generic Playbook guidance.
---

# Decodex

## Goal

Route agent work through the right Decodex surface without duplicating the runtime
specs. Decodex has two supported use modes:

- Manual CLI mode: a human is driving local development, commits, PR preparation,
  landing, status inspection, project registration, or dry-run checks.
- Automation mode: Decodex owns retained-lane execution through registered project
  contracts, `serve`, `run`, tracker labels, issue-scoped tools, review handoff,
  landing, closeout, and operator status.

## First Steps

1. Identify the mode before choosing commands.
2. Read `README.md` and `docs/index.md` when the current checkout is the Decodex repo.
3. Read `Makefile.toml` before running repository validation.
4. For automation questions, read the registered project `project.toml` and
   `WORKFLOW.md` under `~/.codex/decodex/projects/<service-id>/` or the project
   directory supplied with `--config`.
5. Use the narrow skill for the current action:
   - `manual-cli` for normal operator CLI use.
   - `automation` for retained-lane control-plane use.
   - `commit` for `decodex commit`.
   - `land` for `decodex land`.
   - `labels` for Decodex Linear labels.

## Authority Split

- Runtime behavior belongs to `apps/decodex/src/` and `docs/spec/`.
- Operator procedures belong to `docs/runbook/`.
- Current repository layout belongs to `docs/reference/`.
- Registered project execution policy belongs to project-local `WORKFLOW.md`.
- Service paths and credential environment-variable names belong to project-local
  `project.toml`.
- This plugin owns reusable agent-facing Decodex usage instructions.

Treat this plugin and the Decodex repository docs as the Decodex-specific authority.

## Boundaries

- Do not use global `AGENTS.md` as the source of truth for Decodex runtime, tracker,
  identity, landing, closeout, or cleanup policy.
- Do not replace `decodex land` with GitHub UI, `gh pr merge`, merge queue actions,
  raw `git`, or direct API merge mutations for a Decodex-owned landing path.
- Do not infer service identity, token variables, or Linear workspace from ambient
  shell state when a registered project config declares them.
- Do not turn a manual CLI task into retained-lane automation unless the user asks for
  automation or the current registered workflow requires it.

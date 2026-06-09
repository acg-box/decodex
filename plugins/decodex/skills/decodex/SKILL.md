---
name: decodex
description: Use as the conductor for Decodex work whenever the user asks to use, configure, operate, debug, or author Decodex. Routes between manual CLI workflows and runtime-owned automation workflows, and keeps Decodex-specific authority in the Decodex repo rather than generic Playbook guidance.
---

# Decodex

## Goal

Route agent work through the right Decodex surface without duplicating the runtime
specs. Decodex has these supported use modes:

- Research/design mode: natural-language requests such as `research X` enter the
  Decodex-native Research/Decision path. The result is a latent Decision Contract,
  not execution authority.
- Manual CLI mode: a human is driving local development, commits, PR preparation,
  landing, status inspection, project registration, account selection, or dry-run
  checks.
- Automation mode: Decodex owns retained-lane execution through registered project
  contracts, `serve`, `run`, tracker labels, issue-scoped tools, review handoff,
  landing, closeout, and operator status.
- Planning support: agents shape Decodex-friendly issue sets, queue strategy,
  dependency boundaries, and concurrency after a human request or accepted/promoted
  Decision Contract needs executable issue shaping.

## Natural-Language Research Routing

Keep the everyday user surface conversational. Do not require the user to mention
Research Lanes, Decision Lanes, DAGs, Execution Programs, queue labels, or Codex goal
commands.

Route by intent:

1. If the user says `research X`, asks for a design investigation, or asks Decodex to
   figure out what should be done before implementation, treat it as research/design
   intake. Produce or update a latent Decision Contract with evidence, assumptions,
   options, objections, non-goals, acceptance criteria, stop conditions, readiness,
   and open decisions. Do not queue work, create execution authority, mutate tracker
   state, or start implementation from the research request alone.
2. If the user later says `arrange this`, `push this forward`, `推进`, `做`, or an
   equivalent follow-up that clearly accepts or promotes the prior contract, treat that
   as promotion to execution authority. Preserve the accepted contract boundary; if
   direction is still missing or contradictory, ask for the missing decision instead
   of starting work.
3. After promotion, use `planning` to convert the accepted contract into normal
   Linear issues with clear natural-language briefs, dependencies, acceptance, and
   validation. Keep Execution Program and graph mechanics as internal readiness state.
4. Use `labels` and `automation` only for nodes/issues that are ready under the
   registered project policy. Queue labels are an intake signal for retained lanes,
   not a shortcut around blockers, opt-outs, terminal states, active leases, or
   missing briefing.

## First Steps

1. Identify the mode before choosing commands.
2. Read `README.md` and `docs/index.md` when the current checkout is the Decodex repo.
3. Read `Makefile.toml` before running repository validation.
4. For automation questions, read the registered project `project.toml` and
   `WORKFLOW.md` under `~/.codex/decodex/projects/<service-id>/` or the project
   directory supplied through a project-scoped command's `--config`.
5. Use the narrow skill for the current action:
   - `manual-cli` for normal operator CLI use.
   - `planning` for Decodex-friendly issue splitting, queue shaping, and concurrency.
   - `automation` for retained-lane control-plane use.
   - `commit` for `decodex commit`.
   - `land` for `decodex land`.
   - `labels` for Decodex Linear labels.

Use explicit `decodex research compile` and `decodex research promote` commands only
when the operator is asking for the manual CLI surface. Ordinary conversational
research/promotion should still follow the same latent-then-promoted authority
boundary without making the user learn the commands.

## Authority Split

- Runtime behavior belongs to `apps/decodex/src/` and `docs/spec/`.
- Decodex-native research/design behavior belongs to `apps/decodex/src/research_design.rs`
  and `docs/spec/loop-runtime.md`; external research artifacts are supporting
  evidence only for Decodex runtime semantics.
- Harness-improvement recommendations from `decodex evidence` are advisory runtime
  feedback. Treat them as candidates for an explicit accepted improvement path; do not
  auto-edit prompts, skills, validators, issue templates, or loop policies solely
  because a private outcome record suggested them.
- Operator lane-control capabilities belong to `docs/spec/lane-control.md`, with the
  low-level app-server method boundary in `docs/spec/app-server.md`.
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
- Do not treat a research summary, `docs/research/` artifact, or compiled latent
  contract as accepted execution authority. A Decodex research/design result must be
  promoted before later issue shaping or Execution Program readiness can consume it.

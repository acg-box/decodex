---
name: decodex
description: Use when the user asks to use, configure, operate, debug, or author Decodex. Routes research/design, planning, manual CLI, and runtime-owned automation workflows through Decodex-specific authority.
---

# Decodex

## Goal

Route agent work through the right Decodex surface without duplicating the runtime
specs. Decodex has these supported use modes:

- Research/design mode: natural-language requests such as `research X` enter the
  Decodex-native bounded research method. Probe, evidence, options, judgment,
  challenge, and final decision all feed a latent Decision Contract, not execution
  authority.
- Manual CLI mode: a human is driving local development, commits, PR preparation,
  landing, status inspection, project registration, account selection, or dry-run
  checks.
- Automation mode: Decodex owns retained-lane execution through registered project
  contracts, `serve`, `run`, Program Intake, tracker labels for ordinary issues,
  issue-scoped tools, review handoff,
  landing, closeout, and operator status.
- Planning support: agents shape Decodex-friendly issue sets, dispatch readiness,
  dependency boundaries, and concurrency after a human request or accepted/promoted
  Decision Contract needs executable issue shaping.

## Natural-Language Research Routing

Keep the everyday user surface conversational. Do not require the user to mention
Research Lanes, Decision Lanes, DAGs, Execution Programs, queue labels, or Codex goal
commands.

Route by intent:

1. If the user says `research X`, asks for a design investigation, or asks Decodex to
   figure out what should be done before implementation, use `research` plus its phase
   skills: `research-probe`, `research-evidence`, `research-options`,
   `research-judgment`, `research-challenge`, and `research-decision`. Produce or
   update a latent Decision Contract with evidence, assumptions, options, objections,
   non-goals, acceptance criteria, stop conditions, readiness, and open decisions. Do
   not queue work, create execution authority, mutate tracker state, or start
   implementation from the research request alone.
2. If the user later says `arrange this`, `push this forward`, `推进`, `做`, or an
   equivalent follow-up that clearly accepts or promotes the prior contract, use
   `research-promote`. Preserve the accepted contract boundary; if direction is still
   missing or contradictory, ask for the missing decision instead of starting work.
3. After promotion, use `planning` to convert the accepted contract into normal
   Linear issues with clear natural-language briefs, dependencies, acceptance, and
   validation, then persist an Execution Program for direct scheduler dispatch. Keep
   Execution Program and graph mechanics as internal readiness state.
4. Use `automation` for Program Intake and retained execution. Persisted Program
   Intake dispatches ready mapped nodes directly with Program dispatch mode. Use
   `labels` only for ordinary non-Program issues that should enter service-scoped
   tracker intake. Queue labels are not the Program DAG scheduler; they remain an
   ordinary issue intake signal and never bypass blockers, opt-outs, terminal states,
   active leases, or missing briefing.

## First Steps

1. Identify the mode before choosing commands.
2. Read `README.md` and `docs/index.md` when the current checkout is the Decodex repo.
3. Read `Makefile.toml` before running repository validation.
4. For automation questions, read the registered project `project.toml` and
   `WORKFLOW.md` under `~/.codex/decodex/projects/<service-id>/` or the project
   directory supplied through a project-scoped command's `--config`.
5. Use the narrow skill for the current action:
   - `research` for bounded Decodex research/design intake.
   - `research-probe` for framing decisions, hypotheses, falsifiers, and stop rules.
   - `research-evidence` for auditable evidence ledgers and missing-evidence tracking.
   - `research-options` for evidence-grounded option comparison.
   - `research-judgment` for challenge-ready recommendations.
   - `research-challenge` for skeptic objections before finalization.
   - `research-decision` for the terminal decision-ready/not-ready/blocked/human gate.
   - `research-promote` for accepted research-to-execution authority.
   - `manual-cli` for normal operator CLI use.
   - `planning` for Decodex-friendly issue splitting, dispatch readiness, and concurrency.
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
  and `docs/spec/loop-runtime.md`. The Decodex plugin's `research*` skills are the
  default agent-facing method for bounded research. The legacy external `$research`
  skill and `docs/research/` artifacts are supporting evidence or import material only
  for Decodex runtime semantics.
- Harness-improvement recommendations from `decodex evidence` are advisory runtime
  feedback. Treat them as candidates for an explicit accepted improvement path; do not
  auto-edit prompts, skills, validators, issue templates, or loop policies solely
  because a private outcome record suggested them.
- Architecture recovery may change implementation strategy only inside the accepted
  Authority Envelope. Authority Boundary Check outcomes that require human direction,
  lack evidence, depend on external/manual state, or exhaust recovery budget route to
  manual attention instead of asking a detached Codex conversation mid-run.
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

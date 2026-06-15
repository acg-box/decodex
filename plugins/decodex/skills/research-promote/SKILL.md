---
name: research-promote
description: Use when a user explicitly accepts a Decodex research result or asks to arrange, push forward, implement, or turn it into executable work. Owns the research-to-planning authority boundary.
---

# Decodex Research Promote

## Goal

Convert an accepted latent Decision Contract into execution authority without changing
what was researched or approved.

Use this only after explicit acceptance or an equivalent follow-up such as `arrange
this`, `push this forward`, `推进`, or `做`.

## Promotion Rules

1. Identify the latent Decision Contract or conversational contract being accepted.
2. Confirm the accepted boundary: objectives, non-goals, constraints, assumptions,
   objections, validation expectations, proposed issue summaries, and stop conditions.
3. If unresolved decisions, evidence gaps, or blockers remain, do not promote. Ask for
   the missing human decision or return to research.
4. If the operator is using the manual CLI surface, use `decodex research promote
   <CONTRACT_ID>` to record acceptance in local runtime state.
5. After promotion, route issue shaping through `planning`.
6. Let Program Intake persist the internal Execution Program and dispatch ready mapped
   nodes directly. Do not use queue labels as the Program scheduler.

## Boundaries

- Do not infer acceptance from a research summary, old `docs/research/` artifact, or
  merely because the user asked a research question.
- Do not silently expand scope while promoting. New product behavior, public API,
  config, workflow, security, privacy, data, billing, validation, or authority changes
  require explicit acceptance.
- Do not bypass planning or Program Intake by manually creating queue-label work from a
  research result.

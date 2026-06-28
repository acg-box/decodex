---
name: scout
description: Use when a task needs bounded read-only evidence gathering, codebase search, current-state probing, source lookup, or independent fresh-context scouting before a plan, implementation, review, or decision.
---

# Scout

Gather the smallest evidence slice that can change the next decision. Scout is
read-only: no implementation, tracker mutation, authority creation, or verification
claim. Read `../../references/deliberation-gate.md` when the gate may apply.

## Rules

- Default to a fresh bounded read-only subagent when independent context can change a
  plan, decision, implementation boundary, or ready claim and subagent tools exist.
- Inline scouting is only for one local question answerable from 1-2 files or one
  command that cannot affect architecture, review repair, root cause, public
  contracts, docs drift, commit/land, or ready/done claims.
- Before dispatch, name objective, allowed roots/sources, excluded surfaces, and
  expected evidence shape.
- Prefer direct evidence: checked-in files, commands, official docs, runtime
  readback, tracker state, or source-backed knowledge.
- Return contradictions and gaps; do not smooth them into a recommendation.
- Scout subagents must stay read-only and must not spawn further subagents unless the
  main thread explicitly delegates that.

## Output

Report evidence refs, observations, contradictions, gaps, and the smallest next check
or owner surface.

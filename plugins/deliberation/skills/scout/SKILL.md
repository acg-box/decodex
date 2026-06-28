---
name: scout
description: Use when a task needs bounded read-only evidence gathering, codebase search, current-state probing, source lookup, or independent fresh-context scouting before a plan, implementation, review, or decision.
---

# Scout

Gather the smallest evidence slice that can change the next decision. Scout is
read-only and does not implement, mutate tracker state, create authority, or claim
verification.

Read `../../references/deliberation-gate.md` when the task may need the compact
grill/scout/challenge gate or when deciding whether inline scouting is enough.

## Rules

- Default to a fresh bounded read-only support agent when evidence gathering benefits
  from independent context and support-agent tools are allowed.
- Do not require an explicit user request for support agents when scout evidence can
  materially change a plan, decision, implementation boundary, or ready claim.
- Inline scouting is allowed only when one explicit local question can be answered
  from 1-2 files or one command, and the answer cannot affect architecture, review
  repair, root-cause debugging, public contracts, docs drift, commit/land, or
  ready/done claims.
- Name the objective, allowed roots or sources, excluded surfaces, and expected
  evidence shape before dispatching.
- Prefer direct evidence: checked-in files, command output, official docs, runtime
  readback, tracker state, or source-backed knowledge.
- Return contradictions and gaps instead of smoothing them into a recommendation.
- Do not feed the scout a preferred answer unless the task is explicitly to audit
  that answer.
- Support-agent scouts are read-only and must not spawn further support agents unless
  the main thread explicitly delegates that.

## Output

Report evidence refs, relevant observations, contradictions, gaps, and the smallest
next check or owner surface.

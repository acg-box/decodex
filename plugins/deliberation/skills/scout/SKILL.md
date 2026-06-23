---
name: scout
description: Use when a task needs bounded read-only evidence gathering, codebase search, current-state probing, source lookup, or independent fresh-context scouting before a plan, implementation, review, or decision.
---

# Scout

Gather the smallest evidence slice that can change the next decision. Scout is
read-only and does not implement, mutate tracker state, create authority, or claim
verification.

## Rules

- Use a fresh bounded read-only support agent when the evidence search is non-trivial
  and tool support exists. Inline scouting is acceptable for small local probes.
- Name the objective, allowed roots or sources, excluded surfaces, and expected
  evidence shape before dispatching.
- Prefer direct evidence: checked-in files, command output, official docs, runtime
  readback, tracker state, or source-backed knowledge.
- Return contradictions and gaps instead of smoothing them into a recommendation.
- Do not feed the scout a preferred answer unless the task is explicitly to audit
  that answer.

## Output

Report evidence refs, relevant observations, contradictions, gaps, and the smallest
next check or owner surface.

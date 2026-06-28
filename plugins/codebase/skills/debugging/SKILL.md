---
name: debugging
description: Use when repo bugs, failing tests/builds, regressions, performance issues, repeated failed fixes, or visible symptoms need root-cause investigation.
---

# Debugging

Find the smallest evidence-backed root cause before repair. Debugging does not create
execution authority, land changes, close tracker state, or replace final verification.

## Use

- Bugs, test/build failures, runtime regressions, visible symptoms, green checks with
  wrong behavior, or repeated failed fixes.
- Review repair, recovery path, runtime readback, or tracker/source conflicts.

## Boundaries

- Use the checked-in docs or knowledge owner for docs/code claim alignment.
- Use the owning research workflow only when debugging produces a decision-ready
  comparison.
- For architecture-level root-cause work, repeated failed fixes, or unclear owner
  boundaries, use `$deliberation:grill` before repair and `$deliberation:skeptic`
  before claiming the diagnosis explains the symptom.
- Use `$codebase:verification` before done/fixed/ready claims.

## Loop

Keep the loop falsifiable:

`symptom -> owner boundary -> fresh baseline -> hypothesis -> smallest falsifier -> repair surface -> original-symptom check`

If uncertainty remains material and support-agent tools are allowed, dynamically
spawn one read-only support agent for a single evidence slice or hypothesis critique.
The prompt must name the objective, read-only boundary, and expected finding shape.
The main thread owns diagnosis, repair, and final claim.

## Output

Report symptom, owner boundary, root-cause evidence, falsifier or next check, repair
surface, and original-symptom check.

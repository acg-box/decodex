---
name: debugging
description: Use when Decodex or repository work needs root-cause investigation for bugs, test/build failures, runtime regressions, performance issues, unexpected behavior, visible symptoms, or repeated failed fixes.
---

# Decodex Debugging

Find the smallest evidence-backed root cause before repair. Debugging does not create
execution authority, land changes, close tracker state, or replace final verification.

## Use

- Bugs, test/build failures, runtime regressions, visible symptoms, green checks with
  wrong behavior, or repeated failed fixes.
- Retained lane, review repair, recovery path, runtime readback, or tracker/source
  conflicts.
- Decodex runtime cases involving lanes, runtime state, handoff records, app-server
  protocol, or operator readback. Use `decodex status`, `decodex diagnose --json`,
  `decodex evidence`, or documented recovery diagnostics there.

## Boundaries

- Use `docs-drift` for docs/code claim alignment.
- Use Decodex research only when debugging produces a decision-ready comparison.
- Use `$decodex:verification` before done/fixed/ready claims.

## Loop

Keep the loop falsifiable:

`symptom -> owner boundary -> fresh baseline -> hypothesis -> smallest falsifier -> repair surface -> original-symptom check`

If uncertainty remains material, dynamically spawn one read-only support agent for a
single evidence slice or hypothesis challenge. The prompt must name the objective,
read-only boundary, and expected finding shape. The main thread owns diagnosis,
repair, and final claim.

## Output

Report symptom, owner boundary, root-cause evidence, falsifier or next check, repair
surface, and original-symptom check.

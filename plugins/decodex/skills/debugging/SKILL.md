---
name: debugging
description: Use when Decodex or repository work needs root-cause investigation for bugs, test/build failures, runtime regressions, performance issues, unexpected behavior, visible symptoms, or repeated failed fixes.
---

# Decodex Debugging

## Goal

Find the smallest evidence-backed root cause before repair. This skill owns
diagnosis, hypothesis validation, and original-symptom checks. It does not create
execution authority, land changes, close tracker state, or replace final verification.

Use repository-native command authority for commands and gates. Use Decodex runtime
diagnostic surfaces such as `decodex status`, `decodex diagnose --json`,
`decodex evidence`, or documented recovery diagnostics when the failure involves
Decodex lanes, runtime state, handoff records, app-server protocol, or operator
readback.

## When to use

- A user reports a bug, test failure, build failure, runtime regression, performance
  issue, unexpected behavior, or visible symptom that remains after a prior fix.
- A check is green but observed behavior is still wrong.
- A retained lane, review repair, recovery path, or runtime readback conflicts with
  the source tree or tracker state.
- Repeated fixes have failed and the next step depends on a falsifiable hypothesis.

## Do not use

- To make a research recommendation. Use Decodex research and `research-challenge`.
- To audit docs/code claim alignment. Use `docs-drift`.
- To create implementation, tracker, landing, or closeout authority.
- To make the final done/fixed/ready claim without fresh verification evidence.

## Debug Loop

Use the shortest loop that answers the case:

1. `symptom`: What exactly is broken, and is it user-visible, test-visible,
   runtime-visible, or tracker/runtime-visible?
2. `boundary`: Which component, config, command, lane, runtime store, or external
   dependency owns the behavior?
3. `baseline`: What fresh reproduction, status readback, failing check, log, or
   observation proves the symptom still exists?
4. `hypothesis`: What concrete cause would explain the symptom and the counterfactual?
5. `falsifier`: What is the smallest command, code read, status readback, or
   experiment that would disprove the hypothesis?
6. `repair surface`: What is the smallest code, config, docs, test, or runtime-control
   surface that actually owns the cause?
7. `symptom check`: Which original-symptom check or representative regression evidence
   must be rerun after repair?

## Challenge

When material uncertainty remains, dynamically spawn one read-only support agent for a
single missing evidence slice or hypothesis challenge. The prompt must name the
objective, read-only boundary, and expected finding shape. The main thread owns the
diagnosis, repair, and final claim.

## Outputs

Report:

- symptom and owning boundary
- strongest evidence for the root cause
- falsifier or smallest next check when unresolved
- repair surface
- original-symptom check used or still required

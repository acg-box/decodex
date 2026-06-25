---
name: review-feedback
description: Use when PR review comments, CI/bot output, Linear comments, or pasted feedback need evidence-based triage or repair.
---

# Review Feedback

Use this skill to handle real review feedback as evidence, not orders.

## Boundaries

- Do not use this for a fresh code review; use the normal review stance instead.
- Do not create commits, land PRs, mutate trackers, or close runtime state from here.
- If no actionable feedback exists, say so and continue with the real blocker.
- For ambiguous, architectural, repeated, or disputed review repair, use
  `$deliberation:challenge` before editing or replying. Use `$deliberation:scout`
  when review claims need fresh bounded evidence.

## Required Flow

1. Collect available sources: GitHub inline threads, review summaries, PR comments,
   check annotations, CI/review bot output, Linear comments, pasted user feedback,
   branch/head/base state, and relevant repo workflow docs.
2. Classify each item before editing:
   - `verified_actionable`: correct, in scope, and evidenced.
   - `needs_clarification`: intent or expected behavior is ambiguous.
   - `invalid`: contradicted by code, docs, tests, compatibility, or requirements.
   - `out_of_scope`: reasonable but outside this task.
   - `already_handled`: current head already addresses it.
3. Fix only `verified_actionable` items and keep the patch scoped to the reviewed
   surface unless evidence proves a wider fix.
4. Validate the repair with the smallest repo-native evidence that proves it.
5. Re-check the diff against accepted feedback.
6. Reply or resolve review threads only after the repaired head and validation support
   the disposition.
7. Route commit creation, landing, tracker progress, or runtime closeout to the owning
   workflow.

## Output

Report feedback sources checked, item classifications, verified repairs, validation
evidence, thread actions taken, unresolved clarification/pushback, and the next owner
workflow when applicable.

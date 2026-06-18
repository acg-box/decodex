---
name: review-feedback
description: Use when receiving, triaging, validating, or repairing code review feedback from PR comments, review summaries, review threads, CI/review bots, Linear comments, or user-pasted feedback. Owns feedback intake and verified repair discipline, not review requests, commit creation, PR landing, or tracker closeout.
---

# Review Feedback

## Goal

Handle incoming code review feedback with technical rigor. Review feedback is an
input to evaluate against the repository, not an order to blindly implement.

Use this skill to collect real feedback, classify each item, repair only verified
issues, and keep review threads truthful.

## When to use

- The user asks to address review, fix review comments, handle PR feedback, perform a
  review repair, resolve review threads, or summarize review feedback.
- The user pastes external reviewer, review bot, CI annotation, or teammate feedback.
- A PR, retained lane, or branch is in a review-repair state.
- You need to decide whether a review suggestion is valid, out of scope, already
  handled, or needs clarification.

## Do not use

- Requesting a new code review or dispatching a reviewer.
- Generic code-review findings when the user asks you to review code directly.
- Commit creation or commit-message shape. Use the repository's owning commit workflow
  when a repair reaches commit creation.
- PR landing, mergeability closeout, default-branch sync, or post-landing cleanup. Use
  the repository's owning landing workflow when landing is requested.
- Durable progress checkpoints or tracker closeout state. Use the owning runtime or
  tracker workflow when current repair state must be recorded outside the review
  thread.
- Label routing or runtime-owned review orchestration. Use the owning tracker or
  automation workflow.

## Source Inventory

Before editing, collect the actual feedback surfaces that are relevant and available:

- GitHub inline review comments and review thread state
- GitHub non-thread review summaries and top-level PR comments
- GitHub check-run annotations, CI output, or review bot output
- Linear comments, diff threads, or issue-linked review notes
- Pasted user feedback, including numbered lists or screenshots transcribed by the user
- Current branch, head SHA, PR URL, mergeability/base-drift state, and relevant
  repository workflow docs when they affect the repair

If the expected review surfaces are empty, treat that as evidence. Do not invent a
repair target. Check whether the real blocker is base drift, failing checks, missing
pushes, or lifecycle state.

## Classification

Create a short inventory and classify every item before changing code:

- `verified_actionable`: evidence shows the feedback is correct and in scope.
- `needs_clarification`: intent, scope, or expected behavior is ambiguous.
- `invalid`: code, tests, docs, compatibility, or requirements contradict the feedback.
- `out_of_scope`: feedback may be reasonable but belongs outside the current task.
- `already_handled`: the current head already addresses it.

For each item, keep the source anchor and the evidence anchor. Evidence can be code,
tests, docs, config, CI output, runtime behavior, or explicit user instruction.

## Rules

- Do not implement review feedback before understanding the item and checking it
  against the current repository.
- Do not accept external review feedback as authoritative merely because it is phrased
  confidently.
- If feedback conflicts with prior user direction, repository docs, compatibility
  constraints, or current behavior, stop and surface the conflict.
- If a batch contains unclear items that may affect the understood items, ask for
  clarification before editing. If unclear items are independent, classify them as
  `needs_clarification` and do not repair those items yet.
- Fix only `verified_actionable` items.
- Keep repairs scoped to the reviewed surface unless evidence proves a wider fix is
  required.
- Prefer small, reviewable repair commits or patches over broad opportunistic cleanup.
- Validate repairs with the smallest repo-native checks that can prove the changed
  surface, then broaden only when the repair affects shared behavior or failure
  evidence points wider.
- Use `$decodex:verification` before reporting an item fixed, replying to a review
  thread as fixed, or resolving a review thread.
- Push back with concise technical reasoning when feedback is wrong, incomplete,
  incompatible, unnecessary, or out of scope.

## Repair Sequence

1. Identify the review context: branch, PR, head SHA, source of feedback, and current
   lifecycle state.
2. Collect feedback from the relevant surfaces. Include non-thread summaries and bot
   output when they exist.
3. Classify every item with evidence.
4. Repair only `verified_actionable` items, grouped by the smallest coherent surface.
5. Run targeted validation for the repair and record the command or observation.
6. Re-review the changed diff against the accepted feedback.
7. For each source item, report one of: fixed, already handled, invalid with reason,
   out of scope with reason, or needs clarification.
8. If commit creation is required, route to the owning commit workflow.
9. If landing is requested after repair, route to the owning landing workflow.

## Thread Handling

- Reply to inline GitHub review comments in the review thread, not as unrelated
  top-level PR comments.
- Reply only after the fix, pushback, or clarification is grounded in evidence.
- Resolve a review thread only after the repaired head contains the fix and validation
  has completed.
- Do not resolve threads for `needs_clarification`, `invalid`, or `out_of_scope`
  feedback unless the reviewer or user explicitly accepts that disposition.
- If no actionable review comments exist, say that and continue with the real blocker
  instead of posting speculative replies.

## Outputs

Return:

- feedback sources checked
- item classification table or compact list
- verified repairs made, with file anchors
- validation evidence
- review-thread replies or resolves performed, if any
- unresolved clarification, pushback, or out-of-scope items
- next workflow surface, such as commit creation, owning runtime/tracker progress, or
  landing, when applicable

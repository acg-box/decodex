---
name: verification
description: Use when done, fixed, passing, ready, landed, closed-out, or verified claims need fresh repo-native evidence.
---

# Verification

Use this as the claim-to-evidence gate before any positive status claim. Command
authority still comes from `$codebase:work`, checked-in docs, task runners,
package scripts, CI, or the touched workflow.

## Core Rule

Every positive claim must have evidence fresh for the current branch, worktree, head,
base, running artifact, and review state. If evidence is missing or stale, downgrade:

`Implemented, not fully verified because <reason>; remaining verification is <next check>.`

## Claim Map

- `tests pass`: cite the exact test command and successful exit.
- `build works`: cite an actual build command or build-system result.
- `lint/typecheck passes`: cite the exact lint/typecheck command and successful exit.
- `bug fixed`: re-check the original symptom or a representative regression test.
- `visible UI fixed`: use preview, screenshot, app launch, HTTP smoke, or explicit
  user instruction that visual preview is not required.
- `PR ready`: verify current head, reviewed diff, repo-native/scoped validation, and
  known residual risks.
- `review thread fixed`: verify the repaired head and relevant validation before
  replying or resolving.
- `landed` or `closed out`: read back merge/default-branch/cleanup/tracker/runtime or
  release authority.
- `docs/code aligned`: use the owning drift workflow; tests or link checks alone are
  not semantic consistency.

## Freshness

Treat evidence as stale after code/docs/config/generated/lock/dependency changes,
branch/base/head changes, rebases/merges/conflict fixes, uncertain rebuild/restart
state, CI/PR-head/mergeability changes, review-repair replies, or subagent
success that the main thread has not checked.

## Risk Scaling

Use the smallest repo-native evidence that supports the claim. Broaden for shared
behavior, user-visible flows, security-sensitive behavior, generated outputs,
release/landing/signing/closeout, or failures pointing wider than the touched file.
For high-risk claims, the main thread must read back the key evidence itself.
Before a `ready` claim on substantial or generated implementation code, review whether
the final shape still leaves unrelated responsibilities in one file. If it does,
downgrade to implemented and split along existing module boundaries first.
For design-heavy, architectural, root-cause, large/generated, public-contract, or
review-repair claims, use `$deliberation:skeptic` before a positive ready/done
claim unless the inline exception is clearly satisfied.

## Output

`Verification: <fresh evidence>; remaining: <none or honest gap>.`

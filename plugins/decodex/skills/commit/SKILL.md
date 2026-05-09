---
name: commit
description: Use whenever a human-driven Decodex lane reaches commit creation or the user asks to use `decodex commit`. Owns the narrow signed local commit path and `decodex/commit/1` message contract, including `--manual-authority` commits. Does not own PR creation, review, landing, cleanup, retained-lane automation, or runtime-owned orchestration.
---

# Commit

## Goal

Create the local commit for a human-driven Decodex lane through `decodex commit`
instead of hand-assembling the `decodex/commit/1` message.

## Use When

- The user asks to commit current work with Decodex.
- The next durable action is a local commit in a human-driven lane.
- The task needs the `decodex/commit/1` commit-message contract.
- The task is explicit `--manual-authority` work with no authoritative tracker issue.

## Do Not Use

- PR creation, PR update, review requests, review repair, or resolving review threads.
- PR landing, tracker closeout, default-branch sync, or branch/worktree cleanup.
- Retained-lane automation through `decodex run`, `decodex serve`, or runtime-owned
  commit/landing/closeout behavior.
- Service-scoped Linear labels such as `decodex:queued:<service-id>` or
  `decodex:active:<service-id>`.

## Sequence

1. Inspect the current diff and stage only the intended files with normal Git tooling.
2. Run the repository-native validation required for the touched surface before
   committing when it can prevent avoidable CI or handoff failure.
3. Use `decodex commit "<summary>"`.
4. For a deliberate non-issue lane, use
   `decodex commit --manual-authority "<summary>"`.
5. Stop at the committed lane state unless the user separately asks to push, open or
   update a PR, request review, or land.

## Fail-Closed Rules

- If `decodex commit` is required, it must run and succeed.
- Do not substitute raw `git commit`, direct Git object creation, or a hand-written
  `decodex/commit/1` JSON message for a failed or unavailable `decodex commit`.
- Do not use `decodex land`, GitHub merge tools, merge queue operations, or branch
  rewrites to finish a commit-only task.
- Do not add, remove, or repurpose Decodex automation labels as a commit fallback.

---
name: land
description: Explicit opt-in only. Use when the user asks to land a human-driven pull request with `decodex land` or invokes this skill. Owns manual PR landing, landing fail-closed rules, tracker closeout, local default-branch sync, and post-landing cleanup. Does not own commit creation, PR creation, review repair, retained-lane automation, or runtime-owned orchestration.
---

# Land

## Goal

Land a human-driven PR through Decodex's manual landing surface without replacing it
with GitHub UI, `gh pr merge`, merge queue actions, raw `git`, or direct API mutations.

## Use When

- The user explicitly asks to land a PR through Decodex.
- A PR already exists and the intended merge path is `decodex land`.
- The task asks whether another merge path is acceptable for a Decodex-owned landing.
- A human-owned PR should be landed with issue authority after first being adopted into
  Decodex's retained review handoff state.
- The lane is deliberate `--manual-authority` work with no authoritative tracker issue
  but still needs Decodex-owned PR landing.

## Do Not Use

- Commit creation or commit-message shape. Use `commit`.
- PR creation, PR update, pushed-head preparation, review requests, review repair, or
  resolving review threads.
- Runtime-owned retained-lane automation through `decodex run` or `decodex serve`.
- Runtime-owned Linear intake or active ownership labels.

## Sequence

1. Confirm the PR exists, the intended base and head are the ones being landed, required
   checks are green, and the repository expects Decodex-owned landing.
2. Run `decodex land "<summary>"`.
3. If issue-authority land reports missing retained handoff state for a human-owned PR
   created from a managed lane worktree, run
   `decodex recover review-handoff adopt <ISSUE> --pr <URL> --dry-run` from that
   worktree, then rerun it live only after validation passes. Retry
   `decodex land --authority <ISSUE> --pr <URL> "<summary>"` after the adopt succeeds.
4. For a deliberate non-issue lane, run
   `decodex land --manual-authority --pr <URL> "<summary>"`.
5. If `decodex land` succeeds and the repo-root default branch is current, finish the
   cleanup tail: remove merged linked worktrees and local/remote lane branches when
   no retained automation state still owns them.

## What `decodex land` Owns

- PR repository, base branch, head freshness, mergeability, and required-check
  validation.
- The merge execution details needed to preserve Decodex's landing contract.
- The expected merge commit message shape.
- Tracker closeout when an authoritative tracker issue exists.
- Local default-branch fast-forward after the remote merge is authoritative.

In `--manual-authority` mode, `decodex land` still owns merge and default-branch sync,
but intentionally skips tracker closeout and active-label ownership checks.

## Fail-Closed Rules

- If `decodex land` is required, it must run and succeed.
- If `decodex land` reports that checks are still pending or expected, treat that as a
  wait condition: keep the tracker issue in its retained review state, keep the active
  ownership label in place, wait for CI, and retry `decodex land`.
- Use `recover review-handoff adopt` only when no retained review handoff marker exists,
  any existing worktree mapping points at the same current managed checkout, and the
  current clean worktree exactly matches the PR head branch and SHA. If a retained
  marker exists or the mapping points elsewhere, use normal land or
  `recover review-handoff rebind` instead.
- Do not substitute `gh pr merge`, GitHub UI, merge queue, raw `git`, direct GitHub API
  mutation, or a hand-assembled merge for a failed or unavailable `decodex land`.
- If GitHub merge already happened but `decodex land` stopped during closeout or
  local default-branch sync, rerun `decodex land` from the same lane before deleting
  the lane manually.
- Do not delete a lane worktree or branch before the repo-root default branch is up to
  date and no runtime-owned retained lane is still using it.

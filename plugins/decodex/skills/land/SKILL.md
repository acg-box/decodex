---
name: land
description: Use when Decodex must land a human PR.
---

# Land

Land a human-driven PR through `decodex land`. Read `../../references/routing.md` for
full landing and recovery boundaries.

1. Confirm PR, base, head, mergeability, and checks.
2. Run `decodex land --authority <ISSUE> --pr <URL> "<summary>"`, or
   `decodex land --manual-authority --pr <URL> "<summary>"` for non-issue work.
3. When handoff state is missing, run `decodex recover review-handoff diagnose`
   first. Rebind restores or refreshes a Decodex-owned retained lane; adopt is for a
   human-owned PR takeover from a managed worktree. Dry-run either recovery before
   the live command.
4. Clean worktrees and branches only after Decodex landing succeeds and default branch
   syncs.

Use this only after explicit landing intent. Do not substitute GitHub UI, `gh pr
merge`, merge queue, raw Git, or direct API mutation.
Only `decodex land` lands a Decodex-owned PR; review-handoff rebind and adopt repair
runtime lifecycle records and do not land the PR.

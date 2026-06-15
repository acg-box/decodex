---
name: land
description: Use when Decodex must land a human PR.
---

# Land

Land a human-driven PR through `decodex land`. Read `../../references/routing.md` for
full landing and recovery boundaries.

1. Confirm PR, base, head, mergeability, and checks.
2. Run `decodex land "<summary>"`, or
   `decodex land --manual-authority --pr <URL> "<summary>"` for non-issue work.
3. Dry-run `decodex recover review-handoff adopt` before any live adopt.
4. Clean worktrees and branches only after Decodex landing succeeds and default branch
   syncs.

Use this only after explicit landing intent. Do not substitute GitHub UI, `gh pr
merge`, merge queue, raw Git, or direct API mutation.

---
name: land
description: Use when Decodex must land a human PR.
---

# Land

Land a human-driven PR through `decodex land`. Read `../../references/routing.md` for
full landing and recovery boundaries.

1. Confirm the exact PR, base OID, head OID, and reviewed local validation evidence.
   Do not read, require, or wait for CI status.
2. Run `decodex land --authority <ISSUE> --pr <URL> "<summary>"`, or
   `decodex land --manual-authority --pr <URL> "<summary>"` for non-issue work.
   For the standalone upstream automation, run:
   `decodex land --manual-authority --pr <URL> --expected-base-oid <OID>
   --expected-head-oid <OID> "<summary>"`.
   This local manual-authority form does not contact Decodex server or runtime. It
   requires an exact clean task worktree for an open PR. Decodex owns the signed
   merge, server-enforced base compare-and-swap, readback, primary sync, and exact
   lane cleanup. It uses `gh pr view` only for PR identity and merge readback. It
   does not call `gh pr checks`, read a status rollup, or wait for a workflow.
   Issue-authority landing writes final landing/closeout state only through the
   lifecycle kernel and runtime state adapter; tracker comments and local receipts
   are projections.
   Non-issue manual-authority landing does not require a registered project when
   GitHub credentials are available from `GH_TOKEN`, `GITHUB_TOKEN`, or `gh auth token`;
   pass `--config <PROJECT_DIR>` only when configured GitHub credentials or workspace
   hooks should be used.
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

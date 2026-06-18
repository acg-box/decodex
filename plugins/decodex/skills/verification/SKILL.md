---
name: verification
description: Use when a done, fixed, passing, ready, landed, closed-out, or verified claim needs fresh repo-native evidence without taking over command selection or landing authority.
---

# Verification

## Goal

Prevent premature success claims. This skill is a claim-to-evidence gate: decide
what evidence is sufficient for the status you are about to state, then report the
evidence or downgrade the claim.

Verification does not choose the repository's commands. Command authority still comes
from `$decodex:repo-work`, checked-in docs, `Makefile.toml`, package scripts, CI, or
the touched workflow.

## When to use

- Before saying work is done, fixed, passing, ready, verified, landed, closed out, or
  complete.
- Before final handoff, commit, PR-ready reporting, review-thread reply/resolve, or
  any status update that could be read as completion.
- After a review repair, rebase, merge, branch switch, worktree switch, generated-file
  update, dependency change, app/preview restart, or server restart invalidates earlier
  evidence.
- When the user asks whether current verification is enough.

## Do not use

- To decide which repo-native commands exist. Use `$decodex:repo-work` and checked-in
  project authority for command selection.
- To replace the owning diagnostic workflow for root-cause investigation,
  reproduction, or hypothesis validation before and during bug repair.
- To replace the owning drift workflow when docs and executable behavior need
  claim-to-evidence comparison.
- To replace `$decodex:review-feedback` for review intake, item classification, or
  thread handling.
- To replace the repository's owning landing workflow for landing authority or cleanup
  debt.
- To replace the owning runtime or tracker workflow for durable progress checkpoints.

## Core Rule

Every positive status claim must have evidence that is fresh for the current branch,
worktree, and head state and strong enough for the claim's scope.

If evidence is missing or stale, do not claim completion. Use an honest downgraded
status instead:

`Implemented, not fully verified because <reason>; remaining verification is <next check>.`

## Claim Map

- `tests pass`: cite the concrete test command and successful exit status.
- `build works`: cite an actual build command or build-system result. Test success is
  not build evidence.
- `lint/typecheck passes`: cite the specific lint/typecheck command and successful
  exit status.
- `bug fixed`: re-check the original symptom or a regression test that represents it.
  Code changes alone are not enough; the owning diagnostic workflow supplies the
  root-cause work before and during the repair.
- `visible UI fixed`: use live preview, screenshot, app launch, HTTP smoke, or an
  explicit user instruction that visual preview is not required.
- `PR ready`: verify current head, reviewed diff, repo-native gate or scoped
  validation, and known residual risks.
- `review thread fixed`: verify the repaired head contains the fix and the relevant
  validation completed before replying or resolving.
- `landed` or `closed out`: read back the merge, default-branch state, cleanup state,
  tracker authority, runtime authority, or release authority that owns the claim.
- `docs/code aligned`: use the owning drift workflow; tests, help output, or link
  checks alone are not semantic consistency.

## Freshness

Treat earlier evidence as stale when any of these can affect the claim:

- code, docs, config, generated files, lockfiles, or dependencies changed after the
  evidence was gathered
- branch, worktree, base, or `HEAD` changed
- a rebase, merge, cherry-pick, conflict resolution, or formatting rewrite happened
- the app, preview, server, simulator, or signed bundle was rebuilt or restarted and
  the source of the running artifact is uncertain
- CI status, PR head SHA, base branch, or mergeability changed
- a review repair is about to reply to or resolve threads
- a dynamically spawned support agent or worker reported success but the main thread has not checked the
  resulting diff or key evidence

## Risk Scaling

- Use the smallest repo-native evidence that actually supports the claim.
- Broaden validation when the change touches shared behavior, user-visible flows,
  release/landing/signing/closeout, security-sensitive behavior, generated outputs, or
  a failure that points wider than the touched file.
- For high-risk claims such as visible bug fixes, landing, release, signing, runtime
  closeout, or review-thread resolution, the main thread should rerun or read back the
  key evidence instead of relying only on a support-agent report.
- A separate read-only challenge can help test whether the evidence supports a
  high-risk claim, but the main thread owns the final status claim and must read back
  the key evidence itself.

## Outputs

Keep the verification line short:

`Verification: <evidence>; remaining: <none or honest gap>.`

Examples:

- `Verification: repo-native smoke command passed; remaining: none.`
- `Verification: targeted unit test passed; visual smoke not run because the user said no browser preview.`
- `Verification: implemented, not fully verified because the app could not launch; remaining: launch the signed bundle and re-check the original symptom.`

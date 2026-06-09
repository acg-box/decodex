# Recover Review Handoff

Purpose: Diagnose and explicitly repair retained review lanes that are blocked by a
missing or stale runtime DB review-handoff marker.

Use this when: `decodex status` or the dashboard shows a `Review & Landing` lane
blocked with `missing_review_handoff_record`, a review handoff/orchestration head
mismatch, or a similar retained review marker mismatch after manual repair or rebase.

Do not use this for: healthy PR handoffs, review repair, landing, closeout, cleanup-only
worktrees, or manual PR landing.

Governing spec: [`../spec/post-review-lifecycle.md`](../spec/post-review-lifecycle.md).

## Read-Only Diagnosis

Run from the project repo or pass the registered project directory:

```sh
decodex recover review-handoff diagnose <ISSUE>
decodex recover --config "$HOME/.codex/decodex/projects/<service-id>" review-handoff diagnose <ISSUE>
```

Omit `<ISSUE>` to inspect every retained review worktree for the configured project.
Use `--json` for a structured report.

The diagnostic is read-only. It reports the issue, tracker state, branch, worktree,
local branch, local head, worktree cleanliness, existing PR URL when one is already
bound, stored handoff head, stored orchestration head, PR base/head when readable,
the mismatched field when one is known, active automation label presence, and the
suggested next command.

## Explicit Rebind

This is a break-glass path. Healthy lanes should keep using
`issue_review_handoff` followed by `issue_terminal_finalize(path = "review_handoff")`;
operators should not use rebind just because a normal review handoff is still in
progress.

Only rebind when the diagnosis says the review handoff marker is absent, says the
existing same-branch same-PR marker must be refreshed after the retained worktree and
PR head have been checked, or reports `review_handoff_state_transition_pending`.

```sh
decodex recover review-handoff rebind <ISSUE> --pr <PR_URL> --dry-run
decodex recover review-handoff rebind <ISSUE> --pr <PR_URL>
```

The non-dry-run command writes runtime DB handoff/orchestration markers and records a
`review_handoff_rebind` audit event on the tracker issue. For a stale existing marker,
it refreshes only the same branch and same PR after validating the clean retained
worktree head matches the PR head. For a partial normal handoff where the marker is
missing, or where an already-current marker exists but the issue state was not
advanced, the command may also move the issue from the workflow
`tracker.in_progress_state` to `tracker.success_state` after the rebind audit
succeeds. It does not merge the PR, queue follow-up issues, or clean worktrees.

The command rejects the rebind unless all of these are true:

- the issue is in the workflow `tracker.success_state`, or the issue is still in
  `tracker.in_progress_state` from a partial normal handoff with a missing or
  already-current marker
- the issue does not have opt-out or needs-attention labels
- the issue still has `decodex:active:<service-id>` ownership
- the retained worktree branch matches the runtime DB worktree mapping
- the retained worktree has no local source changes except top-level Decodex runtime
  artifacts such as `.decodex-run-activity` and `.decodex-run-control/`
- the PR belongs to the configured GitHub repository
- the PR targets the configured default branch
- the PR is open and non-draft
- the PR head branch and head SHA match the retained worktree
- no review handoff marker already exists for the issue/branch, or the existing marker
  is for the same branch and PR and needs a head/orchestration refresh or pending issue
  state transition

After a successful rebind, run:

```sh
decodex status
decodex run --dry-run
```

The lane should leave `missing_review_handoff_record` and return to the existing
post-review lifecycle classification such as waiting for review, ready to land, review
repair required, or blocked for a different concrete reason.

## Active Ownership Recovery

If diagnosis reports `classification: review_handoff_ownership_drift`,
`reason: active_ownership_label_missing`, and `active_label_present: false`, do not run
rebind just to restore ownership. First verify the issue is still meant to continue the
retained post-review lifecycle for this service, then restore the issue to the workflow
success state and add `decodex:active:<service-id>`. If the issue still has
`decodex:needs-attention`, clear that label only after the recorded blocker has been
repaired.

After restoring explicit ownership, rerun:

```sh
decodex recover review-handoff diagnose <ISSUE>
decodex status
```

Continue with `decodex land` or the normal retained post-review lifecycle only when the
diagnosis remains bound and status reports a landable or otherwise concrete
post-review state.

# Recover Missing Review Handoff

Purpose: Diagnose and explicitly repair retained review lanes that are blocked by a
missing runtime DB handoff marker.

Use this when: `decodex status`, `/state`, or the dashboard shows a `Review & Landing`
lane blocked with `missing_review_handoff_record`.

Do not use this for: healthy PR handoffs, review repair, landing, closeout, cleanup-only
worktrees, or manual PR landing.

Governing spec: [`../spec/post-review-lifecycle.md`](../spec/post-review-lifecycle.md).

## Read-Only Diagnosis

Run from the project repo or pass the registered project directory:

```sh
decodex recover review-handoff diagnose <ISSUE>
decodex --config "$HOME/.codex/decodex/projects/<service-id>" recover review-handoff diagnose <ISSUE>
```

Omit `<ISSUE>` to inspect every retained review worktree for the configured project.
Use `--json` for a structured report.

The diagnostic is read-only. It reports the issue, tracker state, branch, worktree,
local branch, local head, worktree cleanliness, existing PR URL when one is already
bound, active automation label presence, and the suggested next command.

## Explicit Rebind

This is a break-glass path. Healthy lanes should keep using
`issue_review_handoff` followed by `issue_terminal_finalize(path = "review_handoff")`;
operators should not use rebind just because a normal review handoff is still in
progress.

Only rebind when the diagnosis says the review handoff marker is absent and the PR
lineage has been checked.

```sh
decodex recover review-handoff rebind <ISSUE> --pr <PR_URL> --dry-run
decodex recover review-handoff rebind <ISSUE> --pr <PR_URL>
```

The non-dry-run command writes runtime DB handoff/orchestration markers and records a
`review_handoff_rebind` audit event on the tracker issue. It does not merge the PR,
change issue state, queue follow-up issues, or clean worktrees.

The command rejects the rebind unless all of these are true:

- the issue is in the workflow `tracker.success_state`
- the issue does not have opt-out or needs-attention labels
- the issue still has `decodex:active:<service-id>` ownership
- the retained worktree branch matches the runtime DB worktree mapping
- the retained worktree has no local changes except `.decodex-run-activity`
- the PR belongs to the configured GitHub repository
- the PR targets the configured default branch
- the PR is open and non-draft
- the PR head branch and head SHA match the retained worktree
- no review handoff marker already exists for the issue/branch

After a successful rebind, run:

```sh
decodex status
decodex run --dry-run
```

The lane should leave `missing_review_handoff_record` and return to the existing
post-review lifecycle classification such as waiting for review, ready to land, review
repair required, or blocked for a different concrete reason.

---
type: "Runbook"
title: "Recover Review Handoff"
description: "Diagnose and explicitly repair retained review lanes blocked by missing, stale, or ownership-drifted review lifecycle state."
status: active
authority: procedural
owner: automation
tags: [runbook]
code_refs: [apps/decodex/src/recovery/review_handoff.rs, apps/decodex/src/recovery/review_handoff/issue.rs, apps/decodex/src/recovery/review_handoff/labels.rs, apps/decodex/src/recovery/review_handoff_diagnosis.rs, apps/decodex/src/recovery/review_handoff_diagnosis/actions.rs, apps/decodex/src/recovery/reports.rs, apps/decodex/src/recovery/tests/review_handoff/rebind_validation.rs, apps/decodex/src/recovery/tests/review_handoff/diagnostics/ownership_drift.rs, apps/decodex/src/recovery/tests/context.rs]
drift_watch: [decodex recover review-handoff diagnose, decodex recover review-handoff rebind, review_handoff_writeback_failed, review_handoff_ownership_drift, pull_request_state_read_failed, decodex:active:<service-id>, decodex:needs-attention]
last_verified: 2026-07-04
---
# Recover Review Handoff

Purpose: Diagnose and explicitly repair retained review lanes that are blocked by a
missing, stale, or ownership-drifted runtime DB review lifecycle record.

Use this when: `decodex status` or the dashboard shows a `Review & Landing` lane
blocked with `missing_review_handoff_record`, a review lifecycle head or phase
mismatch, `review_handoff_state_transition_pending`, or an ownership-drifted retained
handoff after manual repair, stale failure writeback, or rebase.

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
bound, stored lifecycle handoff head, stored lifecycle phase head, PR base/head when
readable, the exact PR readback error when an external PR read fails, the mismatched
field when one is known, active automation label presence, and the suggested next
command. Retained lanes that already reached review handoff but then failed during
handoff writeback remain diagnosable even when the tracker issue is back in the
workflow `tracker.failure_state` and is missing `decodex:active:<service-id>`.

## Explicit Rebind

This is a break-glass path. Healthy lanes should keep using
`issue_review_handoff` followed by `issue_terminal_finalize(path = "review_handoff")`;
operators should not use rebind just because a normal review handoff is still in
progress.

Only rebind when the diagnosis says the review lifecycle record is absent, says the
existing same-branch same-PR record must be refreshed after the retained worktree and
PR head have been checked, reports `review_handoff_state_transition_pending`, or
reports `review_handoff_ownership_drift` for an already-current same-PR same-head lane
that is in `tracker.in_progress_state` or `tracker.failure_state`.

```sh
decodex recover review-handoff rebind <ISSUE> --pr <PR_URL> --dry-run
decodex recover review-handoff rebind <ISSUE> --pr <PR_URL>
```

The non-dry-run command writes the runtime DB review lifecycle record and records a
`review_handoff_rebind` audit event on the tracker issue. For a stale existing record,
it refreshes only the same branch and same PR after validating the clean retained
worktree head matches the PR head. For a partial normal handoff where the record is
missing, or where an already-current record exists but the issue state was not
advanced, the command may also move the issue from the workflow
`tracker.in_progress_state` to `tracker.success_state` after the rebind audit
succeeds. If stale failure writeback already moved an already-current record lane to
`tracker.failure_state`, removed `decodex:active:<service-id>`, or added
`tracker.needs_attention_label`, rebind may restore the active service label, clear the
needs-attention label, and move the issue to `tracker.success_state`. That
failure-state repair is supported for already-current same-PR same-head records and
for the narrower missing-record case where the latest local Run Ledger terminal
outcome for the latest attempt proves `review_handoff_writeback_failed` or terminal
attention caused by review handoff writeback. It does not merge the PR, queue
follow-up issues, or clean worktrees.

The command rejects the rebind unless all of these are true:

- the issue is in the workflow `tracker.success_state`, still in
  `tracker.in_progress_state` from a partial normal handoff with a missing or
  already-current lifecycle record, or in `tracker.failure_state` only when an
  already-current record proves that stale failure writeback caused tracker state
  drift or when a missing lifecycle record is paired with a latest local Run Ledger
  terminal outcome proving `review_handoff_writeback_failed`
- the issue does not have the opt-out label
- the issue does not have the needs-attention label, except for the already-current
  record plus `tracker.failure_state` drift case or the proven missing-record
  writeback-failure case where rebind clears it after recording the audit
- the issue still has `decodex:active:<service-id>` ownership, except for the
  already-current record plus `tracker.failure_state` drift case or the proven
  missing-record writeback-failure case where rebind verifies the active service label
  exists on the issue team and restores it after local lifecycle state is written
- the retained worktree branch matches the runtime DB worktree mapping
- the retained worktree has no local source changes except top-level Decodex runtime
  artifacts such as `.decodex-run-activity` and `.decodex-run-control/`
- the PR belongs to the configured GitHub repository
- the PR targets the configured default branch
- the PR is open and non-draft
- the PR head branch and head SHA match the retained worktree
- no review lifecycle record already exists for the issue/branch, or the existing
  record is for the same branch and PR and needs a head/phase refresh or pending issue
  state transition

After a successful rebind, run:

```sh
decodex status
decodex run --dry-run
```

The lane should leave `missing_review_handoff_record` and return to the existing
post-review lifecycle classification such as waiting for review, ready to land, review
repair required, or blocked for a different concrete reason.

If status reports that same target issue as `needs_review_repair`, including concrete
reasons such as `pull_request_merge_conflict` or `pull_request_branch_behind_base`, a
targeted dry run should exercise the retained repair path:

```sh
decodex run <ISSUE> --dry-run
```

That command should plan `ReviewRepair` for the retained lane. If it instead reports no
eligible queued issue while status still shows `needs_review_repair`, treat that as a
runtime dispatch bug rather than adding queue labels or re-running review-handoff
recovery. If status shows a retained repair lane for a different issue, a targeted dry
run for the wrong identifier should fail with a retained review repair mismatch; do not
use that mismatch as permission to reuse or relabel the retained worktree.

## Manual PR Takeover

Use `adopt` when a human-owned PR was created from a managed Decodex worktree and the
operator wants Decodex to take over the normal issue-authority landing and tracker
closeout path. If that issue already has a worktree mapping, adopt accepts it only when
the mapping points at the current managed checkout; a mapping for a different checkout
is a fail-closed mismatch. Adopt rewrites that mapping to the current PR branch only
after every dry-run/live validation passes. Do not use adopt for lanes that already
have a review lifecycle record; those belong to `rebind`, normal `decodex land`, or
the retained post-review scheduler.

Run it from the lane worktree, not from the repo root:

```sh
decodex recover review-handoff adopt <ISSUE> --pr <PR_URL> --dry-run
decodex recover review-handoff adopt <ISSUE> --pr <PR_URL>
```

The command rejects the adopt unless all of these are true:

- the issue either has `decodex:active:<service-id>` or the service active label
  exists on the issue team and can be restored by live adopt after all other checks
  pass; the issue must not have the opt-out or needs-attention labels
- the issue is in the workflow `tracker.in_progress_state` or already in
  `tracker.success_state`
- no conflicting retained worktree mapping exists; an existing mapping is allowed only
  when it points at the current managed checkout
- no review lifecycle record already exists for the issue's current branch or
  previously mapped branch
- the current checkout is a managed worktree under the configured `worktree_root`
- the current worktree is clean except top-level Decodex runtime artifacts such as
  `.decodex-run-activity` and `.decodex-run-control/`
- the PR belongs to the configured GitHub repository, targets the configured default
  branch, is open and non-draft, has no pending review requests or unresolved review
  threads, and has green landable checks
- the PR head branch and head SHA exactly match the current worktree branch and `HEAD`

The non-dry-run command writes a runtime worktree mapping, a local takeover run
attempt, a review lifecycle record, and a `review_handoff_adopt` audit
event. If the active service label was missing, live adopt restores it after local
handoff state is written and before the audit event is recorded; if the audit write
fails, the label restoration is rolled back. If the issue was still in the workflow
`tracker.in_progress_state`, the command moves it to `tracker.success_state` after the
audit succeeds. It does not merge the PR.

After a successful adopt, land through the normal issue-authority path:

```sh
decodex land --authority <ISSUE> --pr <PR_URL> "<summary>"
```

## Legacy Cleanup-Only Rows

Do not use `recover review-handoff rebind` for a row that appears only under
`Recovery Worktrees` as `role: cleanup_only`. If that row reports
`provenance_source: legacy_unknown`, `audit_required: true`, or a dashboard
`legacy cleanup audit` state, Decodex has found an old local worktree mapping without
enough runtime DB provenance to reconstruct the post-review lane automatically.

Use the fallback path only after the normal paths are unavailable:

1. Confirm the same issue is absent from `Running Lanes`, `Intake Queue`, and
   `Review & Landing`.
2. Verify the tracker issue and any PR you are closing against are terminal.
3. Inspect the retained checkout with `git -C <worktree> status --short` and preserve
   or discard local-only changes intentionally.
4. Run `decodex recover legacy-closeout <ISSUE> --pr <MERGED_PR> --dry-run`, then rerun
   with `--manual-authority` only if validation passes. This records an explicit
   manual closeout audit that names the issue, PR, branch, head, merge commit, and why
   runtime reconstruction was not available.
5. Remove the local worktree only after the audit and local-change decision are done.

This fallback is intentionally more manual than diagnosis or rebind. It exists so an
operator can close legacy residue honestly, but healthy lanes should still use normal
closeout, and recoverable orphaned review lanes should still use read-only diagnosis
plus explicit rebind before any manual cleanup.

## Active Ownership Recovery

If diagnosis reports `classification: review_handoff_ownership_drift`,
`reason: active_ownership_label_missing`, and `active_label_present: false`, follow
the diagnostic `next_action` instead of hand-adding labels. For an already-current
same-PR same-head lane in `tracker.failure_state` or `tracker.in_progress_state`, that
next action is `recover review-handoff rebind --dry-run`; the dry run reports
`would_restore_active_label=true` when live rebind can restore the active service
label after validating the retained worktree and PR lineage. A missing lifecycle
record in `tracker.failure_state` can use the same rebind command only when the latest
local Run Ledger terminal outcome for the latest attempt proves
`review_handoff_writeback_failed`; the dry run reports
`would_restore_active_label=true`, `would_clear_needs_attention_label=true`, and
`state_transition=In Review` when the recovery is valid. If the lane has no retained
lifecycle record because a human PR needs manual takeover, run `recover
review-handoff adopt --dry-run`; adopt reports `would_restore_active_label` for its
own takeover path. If the issue still has `decodex:needs-attention`, clear that label
only after the recorded blocker has been repaired or an explicit recovery command says
it will clear the label itself.

After an explicit recovery restores or confirms ownership, rerun:

```sh
decodex recover review-handoff diagnose <ISSUE>
decodex status
```

Continue with `decodex land` or the normal retained post-review lifecycle only when the
diagnosis remains bound and status reports a landable or otherwise concrete
post-review state.

## Merged Closeout Reconciliation

Use `recover merged-closeout` when Decodex retained a terminal
`needs_attention`/`partial_progress_retained` ledger outcome, but a human already
merged the PR, the tracker issue is Done, and no useful retained patch remains. This
path reconciles Decodex lifecycle state only; it does not change business code, rerun
the lane, merge a PR, or delete local files.

Run dry-run first:

```sh
decodex recover merged-closeout <ISSUE> --pr <MERGED_PR> --dry-run
decodex recover merged-closeout <ISSUE> --pr <MERGED_PR> --manual-authority
```

The live command writes idempotent `closeout` and `cleanup_complete` Linear execution
ledger records, records them in the local runtime store, and clears any stale runtime
worktree mapping for the issue only after both ledger writes succeed.

The command rejects the reconciliation unless all of these are true:

- the tracker issue is in the workflow completed state
- the issue does not have the queue, active, opt-out, or needs-attention labels
- the PR belongs to the configured repository, targets the configured default branch,
  and is `MERGED`
- the PR head branch matches the retained branch from runtime worktree mapping or the
  existing execution ledger
- the PR merge commit is reachable from the current local `origin/<default-branch>`
- any retained worktree path that still exists is clean, on the PR head branch, and at
  the PR head SHA
- the retained branch can be proven from the runtime mapping or existing execution
  ledger

After a successful reconciliation, `decodex status --live` should no longer count the
old terminal attention as current project attention; the Run Ledger should show
`cleanup_complete` as the final lifecycle outcome.

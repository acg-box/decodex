---
type: "Reference"
title: "GitHub Operations"
description: "Map Decodex's current GitHub-facing execution surface and the decision for each area to use `gh`, keep a custom model, or avoid GitHub ownership."
status: active
authority: current_state
owner: docs
tags: [reference]
last_verified: 2026-06-16
---
# GitHub Operations

Purpose: Map Decodex's current GitHub-facing execution surface and the decision for
each area to use `gh`, keep a custom model, or avoid GitHub ownership.

Read this when: You are changing landing, PR inspection, review orchestration,
review handoff validation, or retained-lane cleanup.

Not this document: The runtime state machine, tracker tool contract, operator pilot
procedure, or merge policy.

Covers: Current GitHub operation ownership, keep-vs-replace decisions, and follow-up
criteria for future simplification.

## Decision Table

| Area | Current authority | Decision | Reason |
| --- | --- | --- | --- |
| Manual admin merge and retained clean-path admin merge | `gh pr merge --admin --merge --match-head-commit` in `apps/decodex/src/github.rs` | Already `gh`-backed | `gh` owns admin merge semantics and head matching without Decodex hand-assembling a merge mutation. Manual landing keeps its existing manual gate policy; retained runtime landing calls admin merge only when the PR is already on the deterministic clean path. |
| Repository context inspection | `gh repo view --json name,owner,defaultBranchRef,mergeCommitAllowed` in `apps/decodex/src/github.rs` | Already `gh`-backed | The CLI exposes the required repository fields directly. |
| Review handoff PR validation | `gh pr view --json url,baseRefName,headRefName,headRefOid,state,isDraft,headRepository,headRepositoryOwner` in `apps/decodex/src/agent/tracker_tool_bridge.rs` | Already `gh`-backed | The CLI covers the branch, head, base, repository, draft, and state checks needed before accepting `issue_review_handoff`. |
| Post-merge result inspection | `gh pr view --json state,headRefOid,mergeCommit` in `apps/decodex/src/github.rs` | Already `gh`-backed | Decodex needs only merged state, reviewed head identity, and merge commit OID after merge. |
| Merge commit subject inspection | `gh api repos/<owner>/<repo>/commits/<sha>` in `apps/decodex/src/github.rs` | Keep `gh api` | Decodex needs the authoritative landed `decodex/commit/2` subject from the merge commit. `gh pr view` does not provide that subject in the current model. |
| External review request comments | `gh api repos/<owner>/<repo>/issues/<pr>/comments` in `apps/decodex/src/github.rs` | Keep `gh api` | Decodex persists the created issue-comment database id and timestamp so later review orchestration can match acknowledgements and results precisely. |
| Landing gate state | `gh api graphql` in `apps/decodex/src/github.rs` | Keep custom GraphQL query through `gh` | Decodex needs mergeability, merge-state, review decision, pending review requests, status rollup, and paginated unresolved review-thread counts before merge. |
| Local validation commit status | `decodex verify publish-status` plus `gh api repos/<owner>/<repo>/commits/<sha>/status` and `gh api --method POST repos/<owner>/<repo>/statuses/<sha>` | Keep `gh api` | Project configs may name `[github].landing_required_status_contexts` and `[github].landing_required_status_creators`. Decodex publishes and reads exact commit status contexts on the current PR head so a locally completed full gate can satisfy landing without waiting for unrelated slow or advisory GitHub checks. Success statuses also carry the current PR base SHA, so landing waits for a fresh local gate after the base branch moves. Long-lived fast-landing projects should configure the creator allow-list; an empty allow-list is only a migration or development mode. |
| Retained review state | `gh api graphql` in `apps/decodex/src/orchestrator/pull_request_review.rs` | Keep custom GraphQL query through `gh` | Runtime review orchestration needs paginated issue comments, reactions by actor, reviews, review threads, merge evidence, and head repository metadata in one stable state model. |
| Remote lane branch cleanup | `gh api --method DELETE repos/<owner>/<repo>/git/refs/heads/<branch>` in `apps/decodex/src/github.rs` | Replaced custom git plumbing | Decodex only needs idempotent GitHub ref deletion. `gh` removes the prior `git ls-remote` plus `git push --delete` path and the extra askpass helper just for cleanup. |
| Default branch sync and local branch/worktree cleanup | Git commands in `apps/decodex/src/default_branch_sync.rs` and `apps/decodex/src/orchestrator/git_ops.rs` | Keep local Git | These steps mutate or inspect the local repository/worktree state. `gh` does not replace the required local checkout synchronization and linked-worktree cleanup. |

Decodex resolves the `gh` executable through the runtime helper before these
operations. A project may set `[github].command_path` in `project.toml` to make one
GitHub CLI binary authoritative for GUI-launched control-plane runs. When that field is
absent, the helper checks `PATH`, then common user install locations such as
`$HOME/.local/bin` and `$HOME/.cargo/bin`, then known host fallbacks including
`/run/current-system/sw/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, and `/usr/bin`.
The known fallback paths remain compatibility behavior; a project-level
`github.command_path` is the diagnosable authority when an operator expects a specific
binary.

`decodex status` and `decodex diagnose --json` expose the GitHub CLI authority without
secrets. The diagnostic tier is one of:

- `configured`: Decodex will invoke `github.command_path`.
- `path`: Decodex found `gh` on the process `PATH`.
- `user-bin`: Decodex found `gh` in a common user bin directory.
- `known-fallback`: Decodex found `gh` in a built-in compatibility fallback path.
- `missing`: Decodex did not find an installed `gh` path and will fail closed at the
  GitHub-dependent review, repair, landing, or cleanup boundary.

If status shows `missing`, install GitHub CLI or set `github.command_path` to the
expected binary. If status shows `user-bin` or `known-fallback` but that path is not
the operator-intended authority, set `github.command_path` in the registered project
config and rerun `decodex status` or `decodex diagnose --json` to confirm the tier is
`configured`.

## Replacement Criteria

Prefer `gh` for GitHub operations when all are true:

- `gh` exposes the exact required semantics without weakening Decodex policy.
- Decodex does not need to persist API-only identifiers that the plain CLI command hides.
- The operation is scoped to GitHub state rather than local worktree or shared Git
  administrative state.
- Failure handling can stay fail-closed and noninteractive with the configured
  `github.token_env_var`.

Keep a custom `gh api` or GraphQL model when Decodex needs stable structured fields,
pagination, actor-scoped reactions, review-thread state, or an idempotency record that
the higher-level `gh` command does not expose.

## Implemented Follow-Up

Remote branch cleanup now uses `gh api --method DELETE` against the PR repository ref.
Missing refs are treated as idempotent cleanup success; other `gh` failures preserve the
retained worktree and runtime mapping so cleanup can retry or surface operator attention.

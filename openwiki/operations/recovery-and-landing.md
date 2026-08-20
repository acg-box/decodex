# Recovery And Landing

This page is the consolidated operator guide for Decodex lane recovery and landing decisions. It expands the recovery summaries in [Operator Runbooks](operator-runbooks.md) and the command map in [Runtime Operator Workflows](../workflows/runtime-operator-workflows.md) without replacing command-specific source references there.

Current source anchors: `apps/decodex/src/cli/control_commands/lane.rs`, `apps/decodex/src/cli/recovery_commands.rs`, `apps/decodex/src/recovery/{review_handoff,stale_active,ghost_lane}.rs`, `apps/decodex/src/cli/verify_commands.rs`, `apps/decodex/src/config/github.rs`, `apps/decodex/src/github/status.rs`, and `apps/decodex/src/manual/landing/*.rs`. This page also consolidates archived runbook/spec guidance from the requested git history.

## Recovery path selection

Start with read-only evidence. Use `decodex status --live`, `decodex lane inspect <ISSUE> --run-id <RUN_ID> --json`, and the relevant `decodex recover ... diagnose ... --json` command before mutating tracker labels, runtime rows, worktrees, or GitHub state.

Choose the path from evidence, not from the command that looks most convenient:

- Let the lane continue when issue id, run id, attempt, branch, active channel, and live process/protocol activity still agree.
- Wait after an accepted soft interrupt until status shows the lane settled, terminalized, or exposed a specific blocker.
- Resume retained work only when issue, branch, run, attempt, worktree, PR URL, and PR head lineage all still match.
- Use stale-active recovery when the tracker issue and service active label remain but no safe live owner, shared claim, retained progress, PR lineage, or private progress evidence owns the lane.
- Use ghost-lane recovery when local runtime state points at a missing or invalid tracker issue and the diagnosis proves there is no retained worktree, live process, control-channel evidence, private progress, PR/review lineage, or review lifecycle record.
- Use review-handoff recovery when PR-backed retained work exists but review/landing lifecycle authority is missing, stale, or ownership-drifted.

Stop for manual attention when evidence is missing, contradictory, unreadable, or would require guessing whether local work is safe to overwrite.

## Lane control

Lane control influences an active turn; it is not cleanup authority by itself. The CLI commands are defined in `apps/decodex/src/cli/control_commands/lane.rs`:

```sh
decodex lane inspect <ISSUE> --run-id <RUN_ID> --json
decodex lane steer <ISSUE> --run-id <RUN_ID> --expected-turn-id <TURN_ID> --message <TEXT>
decodex lane interrupt <ISSUE> --run-id <RUN_ID> --reason <TEXT>
decodex lane interrupt <ISSUE> --run-id <RUN_ID> --force
```

Capability matrix:

| Capability | Operator authority |
| --- | --- |
| `inspect` | Read-only lane identity and control-capability readback; it must not mutate tracker state, runtime rows, worktrees, or app-server turns. |
| Soft `interrupt` | Preferred active-turn stop path through the app-server `turn/interrupt` run-control channel. |
| Hard `interrupt --force` | Explicit fallback only when soft interrupt is unavailable, rejected, or timed out and current live/process evidence still identifies the same lane. |
| `steer` | Active-turn instruction delivery only; requires the inspected run id and expected current turn id. |
| Retained resume/retry | Runtime lifecycle authority only through normal retained-lane dispatch/retry/recovery, not a lane-control shortcut. |
| Manual attention | Tracker terminal path authority only; agents must use the structured needs-attention/comment/finalize flow. |

Operational rules:

- Inspect first. Mutating control should be based on current issue, run, attempt, branch/worktree, run lease, process liveness, protocol activity, active channel, thread id, and turn id evidence; if those do not line up, use diagnosis or manual attention instead of guessing.
- The run-control channel is authoritative only while the active attempt still matches its project, issue, run id, attempt, thread id, turn id, active `run_control_channels` row, local `.decodex-run-control/` channel path, and run lease. Mismatches fail closed and stay local audit evidence.
- Prefer soft interrupt through the app-server lane-control protocol. It requests a graceful `turn/interrupt` and leaves tracker labels, retry policy, and retained-worktree classification to the runtime.
- Use `--force` only as an explicit hard process fallback after soft control is unavailable, rejected, or timed out under the documented conditions. A hard fallback may signal a recorded child process, but it does not prove cleanup, review handoff, or label release is safe.
- If soft control reports `run_lease_missing` while inspect/status still shows the same run id, attempt, branch, active channel, and live process or protocol activity, treat the lane as degraded active execution. Do not relabel it as cleanup-only just because the lease row is missing.
- `steer` must name the current run id and expected active turn id. Stale or missing expected-turn evidence fails closed; steer text is private control input, not public tracker text or task replacement.
- After any steer/interrupt/recovery action, rerun status/inspect and confirm the runtime result plus tracker labels agree with the chosen path. Keep raw control payloads, host-local channel paths, process diagnostics, and private evidence out of Linear unless a schema-controlled projection explicitly allows them.

## Stale-active recovery

Stale-active recovery releases tracker-present active ownership that survived a crash or interrupted run. The source boundary is `apps/decodex/src/cli/recovery_commands/stale_active.rs` and `apps/decodex/src/recovery/stale_active.rs`.

Use:

```sh
decodex recover stale-active diagnose <ISSUE> --json
decodex recover stale-active release <ISSUE> --dry-run
decodex recover stale-active release <ISSUE>
```

Release is normally safe only when the diagnosis proves the tracker issue and service active label are present, no compatible run lease or active shared claim remains, no live process/protocol/control-channel evidence remains, worktree guards are clean, and no PR/review lifecycle, review-policy checkpoint, private source, or review progress evidence owns the lane. If the active label is already absent, release can only remove a clean retained worktree and its mapping when queue and attention labels are absent, the interrupted/failed run has no lease or shared claim, the head is reachable from the default branch, control/process evidence is inactive or absent, and review/private progress evidence is absent. That path does not fabricate a label mutation and must pass the same post-cleanup diagnostic using the matching release audit. A `program_dispatch_selected` event records scheduler selection only and does not by itself prove implementation progress. Dry-run first, then rerun live only if the blocker list stays empty.

Classify blockers before choosing the next action. Review lineage or a review-policy checkpoint routes to review-handoff recovery; retained worktree changes, private progress, uninspectable worktree state, unavailable default-branch proof, or retained branch commits route to retained evidence/worktree inspection and an explicit resume, retry, reset, or manual-attention decision. Live process, unsettled run-control/protocol evidence, an active lease, or an incompatible shared claim means the lane is still active or ambiguous and must go back through lane inspect/control instead of label release.

Retained resume/retry is allowed only when issue, branch, run/attempt evidence, worktree, PR URL, and PR head lineage still prove the same owned lane. Do not infer retained authority from a branch name, a tracker label, a stale thread id, or private evidence alone.

Reentry after partial local cleanup is still evidence-bound: repeat run-lease, shared-claim, review-lineage, and tracker-label guards before clearing only the service active label. Reentry may continue only when prior cleanup/audit evidence proves the same run attempt was already guarded or terminal-looking, the channel/worktree cleanup completed, and remaining evidence is stale telemetry from the old run. Do not clear queued labels or attention labels unless the recovery report specifically justifies that transition.

Write and preserve local recovery audit evidence, but publish only public-safe blocker summaries. If blocker names are missing, contradictory, unreadable, or would require deciding whether useful retained work can be discarded, stop for manual attention.

## Ghost-lane recovery

Ghost-lane recovery terminalizes local runtime ownership for a lane whose tracker issue is missing or invalid. The source boundary is `apps/decodex/src/cli/recovery_commands/ghost_lane.rs`, `apps/decodex/src/recovery/ghost_lane.rs`, and `apps/decodex/src/recovery/ghost_lane_diagnosis.rs`.

Use:

```sh
decodex recover ghost-lane diagnose <ISSUE> --json
decodex recover ghost-lane cleanup <ISSUE> --dry-run
decodex recover ghost-lane cleanup <ISSUE>
```

Cleanup is safe only when the tracker issue is absent and the report proves no retained worktree, live process, control-channel row/file, private evidence, thread/protocol activity, PR lineage, review-policy checkpoint, or review lifecycle record remains. The implementation treats blocker names such as `retained_worktree_present`, `control_channel_present`, `private_evidence_present`, `review_lifecycle_present`, `review_policy_checkpoint_present`, and `pr_or_review_lineage_present` as reasons to preserve evidence and stop.

Ghost and stale-active classifications are mutually important. A tracker-backed issue with a service active label is stale-active, not a ghost lane; a missing issue with any retained worktree, ordinary private evidence, live process, active run-control channel, protocol activity, PR/review lineage, review lifecycle, or review-policy checkpoint is `runtime_recovery_blocked`, not cleanup-ready. Mixed private evidence is a blocker even when some rows look like lane-control audit noise.

A ghost-lane report may recognize narrow test-fixture evidence; do not generalize that to production recovery without source-backed proof that the blocker is intentionally ignored. Production cleanup must preserve local audit evidence, avoid Linear mutation when the tracker issue is missing, and stop for manual attention when issue absence, retained ownership, or private evidence cannot be classified without guessing.

## Review-handoff recovery

Review-handoff recovery restores lifecycle authority for PR-backed retained lanes. The source boundary is `apps/decodex/src/cli/recovery_commands/review_handoff.rs`, `apps/decodex/src/recovery/review_handoff.rs`, `apps/decodex/src/recovery/review_handoff_policy.rs`, and `apps/decodex/src/recovery/pull_request_inspection.rs`.

Use diagnosis before mutation:

```sh
decodex recover review-handoff diagnose <ISSUE> --json
decodex recover review-handoff rebind <ISSUE> --pr <PR_URL> --dry-run
decodex recover review-handoff adopt <ISSUE> --pr <PR_URL> --dry-run
```

Boundaries:

- `diagnose` is read-only and should be the first command for missing, stale, or drifted review lifecycle authority.
- `rebind` repairs Decodex-owned retained PR lanes when the retained worktree and PR prove exact issue, branch, head, repository, and default-branch authority. It is the break-glass path for missing lifecycle rows, same-branch same-PR refreshes, pending state transitions, or proven writeback/ownership drift.
- `adopt` is narrower. It takes over a human-created PR only when it came from a managed clean Decodex worktree and the operator explicitly wants Decodex to own normal issue-authority landing and closeout.
- Do not adopt lanes that already have review lifecycle records. Use rebind, normal landing, or retained post-review scheduling instead.
- Never reconstruct retained lifecycle from branch names, PR titles, Linear comments, or current head alone; later dispatch, repair, landing, and closeout must read persisted runtime lifecycle state.

After recovery, rerun diagnosis and `decodex status`. If status reports review repair, `decodex run <ISSUE> --dry-run` should plan the retained repair path; if it reports no eligible issue, treat that as a dispatch/runtime problem, not as permission to add queue labels or reuse the worktree manually.

## Historical v0.2 GitHub operation boundaries

The following commands are frozen v0.2 provenance from `apps/decodex`. The active vNext
`apps/decodex-cli` does not provide `commit` or `land` commands:

```sh
decodex commit "summary" --authority XY-123
decodex commit "summary" --manual-authority
decodex land "summary" --authority XY-123
decodex land "summary" --manual-authority --pr <URL>
```

The historical `decodex/commit/2` subject describes the tree change only. Keep PR URL,
branch, validation receipts, CI, landing, and closeout state out of that historical
commit subject; those belong in runtime/GitHub/tracker evidence.

Use `gh` or `gh api` only for operations Decodex delegates to GitHub: repository and PR inspection, status reads/writes, review state queries, comments, remote branch cleanup, and admin merge with an exact head precondition. Keep local Git worktree mutation, default-branch sync, and local branch cleanup local. If `gh` resolution is ambiguous, inspect the configured GitHub CLI authority before proceeding; source for the operator readback is `apps/decodex/src/orchestrator/status/github_cli_authority.rs`.

Manual fallback should be evidence-preserving and fail-closed. Do not bypass Decodex because a status is slow, a review request is pending, a lifecycle row is stale, or the GitHub CLI path is inconvenient.

## Validation status and landing

Fast landing is configured in the project `[github]` block. `landing_mode = "standard"` is the default. `landing_mode = "fast"` requires trusted `landing_actors` and uses the fixed status context `decodex/local-full-check` (`apps/decodex/src/config/github.rs`).

Publish local validation only after the cited local gate has passed on the exact tree:

```sh
decodex verify publish-status \
  --config /path/to/project.toml \
  --pr https://github.com/OWNER/REPO/pull/NUMBER \
  --context decodex/local-full-check \
  --state success \
  --expected-head "$HEAD_SHA" \
  --expected-base-ref main \
  --expected-base-oid "$BASE_SHA" \
  --description "cargo make check passed"
```

`apps/decodex/src/cli/verify_commands.rs` refuses success publishing when the PR head, base ref, or base oid does not match the expected values. `apps/decodex/src/github/status.rs` reads the latest status for the context, checks the creator against `landing_actors`, and parses `base_ref_oid=` from the status description so fast landing cannot reuse stale local validation after the base branch moved.

For landing, `apps/decodex/src/manual/landing/gate.rs` validates base branch, PR head branch, PR head SHA, review/check/mergeability state, and closeout-only cases. `apps/decodex/src/manual/landing/merge.rs` performs admin merge with the exact head and waits for authoritative merge commit readback.

Stop instead of landing when the PR head changed, the base branch moved, the required context is missing, the status creator is not trusted, the status lacks current base-oid evidence, review or mergeability requires repair/waiting, the repository does not allow the required merge mode, or the local validation evidence cannot be tied to the current PR head/base pair.

## Release readiness

Release readiness is a source-backed evidence packet, not a checklist that can be satisfied by old logs. Use the current command guidance in [Commands And Validation](commands-and-validation.md) and the source behavior affected by the release.

A release evidence packet should include:

- Version/tag agreement and the intended release note boundary.
- Dependency and lane state showing the release branch is based on current `main`.
- Final-tree validation output, usually the aggregate `cargo make check` gate unless a narrower authority is explicitly justified.
- Focused tests for changed behavior contracts: CLI parsing, status JSON, tracker comments, runtime DB rows, app-server payloads, Git/GitHub behavior, site build behavior, and public/private projection as applicable.
- CLI/status/probe evidence when runtime behavior changed, plus dogfood or retained-lane evidence when the release depends on runtime authority.
- Release notes that name shipped capabilities and deferred capabilities accurately.

Stop before tagging or publishing when the intended tag and workspace version disagree, release automation is unverified, validation is missing or tied to an older tree, formatting/lint commands changed files that have not been inspected, runtime-owned review evidence is absent for a release that depends on it, or release notes imply capabilities that are deferred or not source-backed.

## Stop conditions

Do not mutate runtime, tracker, GitHub, or worktrees when any of these are true:

- Inspect-first evidence is missing or inconsistent for issue, run id, attempt, branch/worktree, thread id, turn id, run lease, channel row/path, process liveness, protocol activity, or tracker labels.
- Live or unknown process state, active run leases, active shared claims, recent protocol/thread activity, or active run-control channels remain.
- Soft interrupt has not been tried when available, or a requested hard interrupt would outpace the explicit `--force` fallback boundary and current same-lane process proof.
- A steer request lacks the inspected current run id and expected active turn id, or would act as hidden task replacement rather than operator-supplied guidance.
- Retained resume/retry cannot prove the same issue, branch, run/attempt, worktree, PR URL, and PR head lineage.
- Ghost or stale-active diagnosis reports retained worktree evidence, private progress, mixed private evidence, control-channel evidence, PR/review lineage, review lifecycle, review-policy checkpoints, uninspectable worktree state, unavailable default-branch proof, or unresolved blocker names.
- Needs-attention labels, manual authority requests, authority-boundary checks, ambiguous retained progress, or a human-owned recovery choice require a human decision.
- Worktrees are unreadable, dirty with non-runtime changes, or contain unmerged retained branch commits.
- Default-branch or PR-head proof is unavailable.
- Private progress evidence, review-policy checkpoints, PR/review lineage, or mixed private evidence may still own the lane.
- GitHub/validation evidence is stale, from an untrusted actor, or not tied to the current PR head and base.
- The only available explanation would expose steer text, host-local paths, raw process diagnostics, private evidence payloads, account details, tokens, or other non-public runtime evidence in Linear.

When in doubt, preserve local audit evidence, publish only a public-safe manual-attention reason through supported tracker/runtime paths, and stop.

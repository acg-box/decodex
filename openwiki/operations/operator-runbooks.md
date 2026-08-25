---
type: "Reference"
title: "Operator Runbooks"
openwiki_generated: true
---

# Operator Runbooks

This page is the operational map for Decodex recovery, GitHub/Linear handling, release readiness, and control-plane workflows. For the consolidated recovery and landing decision guide, see [Recovery And Landing](recovery-and-landing.md).

## Validation gate

Use `cargo make` when it has an equivalent task. The Decodex project workflow currently uses `cargo make fmt` followed by `cargo make test` as the full landing gate. Targeted checks are useful while editing, but landing-related refreshes need the workflow gate unless the task authority says otherwise. For broad readiness claims, release readiness, or fast landing evidence, prefer the aggregate `cargo make check` gate from `openwiki/operations/commands-and-validation.md` or explicitly record why a narrower gate is the correct authority.

Choose targeted tests by the behavior contract being changed, not just by the edited filename. CLI parsing, status JSON, tracker comments, runtime DB state, app-server payloads, Git/GitHub behavior, site build behavior, and public/private projection changes each need the test family that observes that contract. If the change touches shared scheduler, review/landing, state, or authority-boundary behavior, run focused tests first and then a broader Rust or repo gate before handoff when feasible.

Local validation status publishing should use `decodex verify publish-status` with expected PR head, base ref, and base oid. This prevents a stale local green result from being attached after the PR or target branch moved. Publish only after the cited local gate has passed on the exact tree, and keep the status description specific enough to connect the GitHub status back to the local evidence.
Registered projects use `landing_mode = "standard"` by default. `landing_mode = "fast"`
trusts `decodex/local-full-check` from the configured `landing_actors` and permits
ruleset bypass landing after the local gate passes. Stop instead of landing when the PR head changed, the base branch moved, the required context is missing, the status creator is not trusted, or the local evidence cannot be tied to the current PR head/base pair.

## Linux container process-reaping prerequisite

Before enabling account-bound Codex child supervision in a Linux container, the operator must run
the daemon under a functioning PID 1 init/subreaper, such as Docker `--init`, that reaps orphaned
descendants. A shell, Cargo, or the Decodex daemon itself must not be the container's non-reaping PID
1. Validate the deployed container rather than assuming the image supplies a kernel or reaper:

```sh
tr '\0' ' ' </proc/1/cmdline
uname -a
uname -m
```

This is an availability prerequisite, not a relaxation of process authority. If an orphaned
descendant becomes a zombie under a non-reaping PID 1, process-group absence cannot be confirmed.
The runtime therefore keeps the child ownership and runner permit in its bounded quarantine and
continues to fail closed. Repeated occurrences can consume all 64 process/quarantine slots and make
subsequent manual launches unavailable indefinitely; the runtime does not detach the descendant,
release the permit, undercount capacity, or route to another account. Correct the container init
configuration and restart under operator authority; do not weaken cleanup checks or manually mark a
slot free.

## Self-dogfood pilot checklist

Use this checklist before running Decodex on its own repository. It is a readiness gate, not a substitute for issue authority or normal review.

- Confirm the project is explicitly registered and enabled with `decodex project list`; add or refresh it with `decodex project add <PROJECT_DIR>` only after the central project directory contains the current `project.toml` and colocated `WORKFLOW.md` (`apps/decodex/src/cli/control_commands/project.rs`, `apps/decodex/src/config/service.rs`).
- Verify app-server compatibility with `decodex probe stdio://` and require a successful probe report before using the pilot as evidence for runtime readiness (`apps/decodex/src/cli/probe_command.rs`).
- Start with a non-mutating queue read: `decodex run --config <PROJECT_DIR> --dry-run --explain`; for a chosen issue, use `decodex run --config <PROJECT_DIR> <ISSUE> --dry-run` and require the selected lane, workflow policy, and validation expectations to match the intended pilot (`apps/decodex/src/cli/control_commands/run.rs`, `apps/decodex/src/orchestrator/entrypoints/run.rs`).
- Cross the live-run boundary only when the dry-run evidence is current and the operator accepts runtime mutation of local state, worktrees, tracker writeback, PR handoff, and retained-lane records: `decodex run --config <PROJECT_DIR> <ISSUE>` or the daemon-owned equivalent.
- Read back status and evidence after each pilot step with `decodex status --config <PROJECT_DIR> --live --json` and, when a run exists, `decodex evidence --config <PROJECT_DIR> <ISSUE> --run-id <RUN_ID> --json` (`apps/decodex/src/cli/control_commands/status.rs`, `apps/decodex/src/cli/control_commands/evidence.rs`).

Additional self-bootstrap guardrails:

- Require the GitHub PR head commit and eventual merge commit to show `Verified` before treating the pilot as healthy.
- Before each batch, compare `git rev-parse --short HEAD` with `decodex --version`; refresh the CLI and restart `decodex serve` when the running binary or serve process is stale.
- Before queueing, verify `GET /livez` succeeds and the dashboard agrees with `decodex status --json` on project registration and visible lane counts.
- After a failed run, use `decodex status`, `decodex evidence`, and tracker/readback evidence before retrying; do not clear labels or reset local state to force a retry.
- For `app_server_plugin_list_timeout`, `app_server_preflight_failed`, `skills/list` blockers, `initialize codexHome` mismatches, or stale `serve` processes, inspect plugin/preflight evidence, repair or restart the local runtime, and confirm recovery with `decodex probe` before clearing `decodex:needs-attention`.

- Treat retained worktree or PR review handoff as a handoff observation, not automatic success: inspect the retained worktree, PR head, review lifecycle state, and current validation before landing or cleanup; Decodex-owned review, repair, and closeout paths must remain the authority (`apps/decodex/src/orchestrator/prompting/user_input.rs`, `apps/decodex/src/state/models/review/records.rs`).
- After any failed or interrupted pilot run, rerun status/evidence first, preserve retained worktree changes, then rerun the same dry-run or recovery diagnosis before live retry; do not clear labels, reset branches, or delete local state to make a retry fit.
- Validate with the workflow-required commands for the touched surface; for repository-level readiness use `cargo make fmt` and `cargo make test`, or the broader `cargo make check` gate when claiming aggregate release or pilot health (`Makefile.toml`).
- Stop when project registration is missing or stale, `project.toml`/`WORKFLOW.md` readiness is unclear, probe fails, dry-run selection is absent or surprising, status/evidence cannot tie the run to the intended issue/head, retained worktree or PR lineage is ambiguous, validation fails, secrets or credentials are missing, or the next step would require guessing operator authority.

## Historical v0.2 GitHub operations

The following commands are frozen v0.2 provenance from `apps/decodex`. They are not
active vNext `apps/decodex-cli` commands:

```sh
decodex commit "summary" --authority XY-123
decodex commit "summary" --manual-authority
decodex land "summary" --authority XY-123
decodex land "summary" --manual-authority --pr <URL>
```

The historical commit subject is a single-line `decodex/commit/2` JSON object. It
describes the tree change only. Do not treat these commands or this subject contract as
active vNext CLI guidance.

If landing is blocked, inspect PR state, review state, merge state, landing mode,
branch freshness, and the retained v0.2 lifecycle records before using any GitHub
fallback. Active vNext repository history and landing use the approved Git/GitHub
workflow because `apps/decodex-cli` has no `commit` or `land` command. Use `gh` or
`gh api` only for the GitHub operations delegated by that workflow, such as
repository/PR inspection, admin merge with head matching, status reads/writes, review
state queries, comments, and remote branch cleanup. Keep local Git operations local
when they mutate worktrees, default-branch sync, or branch cleanup.

Manual fallback should be evidence-preserving and fail-closed. Do not bypass Decodex just because a status is slow, a review request is pending, a lifecycle row is stale, or the GitHub CLI path is ambiguous; resolve the authority problem or stop for operator attention.

## Lane-control recovery

Use lane-control recovery when a current run is stuck, stale, or needs explicit steering. Start with inspect:

```sh
decodex lane inspect <ISSUE> --run-id <RUN_ID> --json
```

Steer requires the current expected turn id. Interrupt prefers the soft protocol path. Forced hard fallback is a recovery path, not a normal workflow shortcut. If soft control reports `run_lease_missing` but inspect/status still shows the same run id, attempt, branch, active channel, and live process or protocol activity, treat the lane as degraded active execution rather than cleanup-only state. Use `--force` only when the operator explicitly wants hard process fallback; if force reports no signalable process, inspect retained evidence before claiming recovery succeeded.

The lane-control decision tree is evidence-first:

- let a matching live lane continue, or wait while an accepted soft interrupt settles
- resume retained work only when issue, branch, run, attempt, worktree, and PR lineage still match
- route to stale-active recovery when the tracker issue and active label remain but no safe live/progress owner exists
- route to ghost-lane recovery when local runtime state refers to a missing tracker issue and no worktree, live process, PR/review lineage, or mixed private evidence remains
- route to review-handoff recovery when PR-backed retained work has missing, stale, or ownership-drifted lifecycle authority
- stop for manual attention when evidence is missing, contradictory, or would require guessing whether local work is safe to overwrite

After any lane-control recovery, confirm the runtime recorded the result, run status no longer reports the stale active or ghost condition, and any queue/attention labels still match the chosen recovery path.

## Review handoff recovery

Retained review handoff recovery is for PR-backed lanes whose review/landing lifecycle authority is missing, stale, or inconsistent. Use diagnose/read-only commands first. Rebind or adopt only when the evidence proves the intended PR, branch, issue, and head relationship.

Use `decodex recover review-handoff diagnose <ISSUE> --json` before mutation. Rebind is the break-glass path for retained Decodex PR lanes with a missing lifecycle record, a same-branch same-PR record that needs refresh, a pending state transition, or proven ownership drift/writeback failure. It must validate issue state, service active/needs-attention labels, retained worktree cleanliness, configured repository/default branch, open non-draft PR state, and exact PR head branch/SHA before writing lifecycle authority.

Adopt is narrower: it is for a human-owned PR created from a managed clean Decodex worktree when the operator wants Decodex to take over normal issue-authority landing and closeout. Do not adopt lanes that already have review lifecycle records; use rebind, normal landing, or retained post-review scheduling instead.

Do not reconstruct retained lifecycle from current branch names or Linear comments alone. Recovery must persist lifecycle authority so later dispatch, repair, landing, and closeout operate from runtime state. After recovery, rerun diagnosis and `decodex status`; use targeted `decodex run <ISSUE> --dry-run` when status reports review repair, and treat mismatch or no-eligible output as a dispatch/runtime issue rather than relabeling the lane.

## Ghost lanes and stale active ownership

Ghost lane recovery handles local evidence for runs or worktrees that no longer have a valid active owner. Stale active recovery handles claims that survived crashes or interrupted runs. Recovery should distinguish:

- active process still owns the run
- retained review or closeout work still owns the lane
- local marker/control-channel evidence blocks cleanup
- tracker issue identity is missing or invalid
- cleanup can safely remove stale local state

For stale active ownership, prefer `decodex recover stale-active diagnose <ISSUE> --json`. Release is normally safe only when the tracker issue and service active label are present, no live process or compatible run lease/shared claim remains, worktree/lineage/progress guards are clean, and no PR/review lifecycle, review-policy checkpoint, or private source/review progress evidence owns the lane. A missing active label permits only retained local cleanup when the queue and attention labels are also absent, the interrupted/failed run has no lease or shared claim, the worktree is clean and reachable from the default branch, control/process evidence is inactive or absent, and review/private progress evidence is absent; no tracker-label mutation is synthesized. `program_dispatch_selected` is selection evidence, not implementation progress. Dry-run first, then rerun live only if the report stays safe; reentry after partial local cleanup repeats the same authority checks against the cleanup audit.

For ghost lanes, prefer `decodex recover ghost-lane diagnose <ISSUE> --json`. Cleanup is safe only when the tracker issue is missing and the report proves no retained worktree, live process, control-channel row/file, private evidence, thread/protocol evidence, PR lineage, or review lifecycle record remains, except for explicitly recognized test-fixture control evidence. Dry-run first, then rerun live only if the blocker list stays empty.

Stop instead of cleaning when blockers name live or unknown process state, active shared claims, needs-attention labels, tracked or untracked non-runtime worktree changes, unmerged retained branch commits, unavailable default-branch proof, private progress evidence, review-policy checkpoints, PR/review lineage, mixed private evidence, or unreadable worktrees. When in doubt, keep the evidence and surface manual attention instead of deleting possible active ownership.

## Linear archive hygiene

Archive hygiene should dry-run first, list candidate issues, and preserve exclusions. Do not archive issues that still have active Decodex ownership, retained review/closeout state, needs-attention markers, queued labels, or unresolved tracker blockers.

## Control-plane upgrades

Control-plane upgrade work should use explicit candidate artifacts, source references, impact classification, and operator-readable validation. Upgrade candidates are not direct mutation authority; they become implementation work only after acceptance through the normal planning/intake path.

Stop when required evidence is missing, review would affect authority boundaries, or the candidate needs a human decision.

## Release readiness

Release readiness needs a tag contract, validation evidence, and release-note content. Confirm build/test gates, plugin/app/site artifacts, Radar/Publisher artifact validation when relevant, and install/update paths before cutting a release.

A release evidence packet should connect each claim to current-source proof: version/tag agreement, dependency or lane state, final-tree validation output, focused tests selected from the changed behavior, CLI/status/probe evidence when runtime behavior changed, dogfood or retained-lane evidence when the release depends on runtime authority, and release notes that name shipped and deferred capabilities. If formatting or lint-fix commands changed files, inspect that diff before treating the validation output as final evidence.

Stop before tagging or publishing when the intended tag and workspace version disagree, the release automation contract is unverified, the lane is not based on current `main`, required validation is missing or tied to an older tree, local validation status cannot be tied to the PR head/base pair, runtime-owned review or dogfood evidence is absent for a release that depends on it, or release notes would imply capabilities that are deferred or not source-backed.

## Social publishing

Social publishing flows through Publisher artifacts and reservations. A social candidate needs source references, decision state, claim boundaries, and publication mode. Reservations prevent duplicate publication. Publisher should not run fresh upstream analysis; it consumes accepted Radar handoff evidence.

## Site deployment

The public site is static Astro. Deployment work should verify `npm --prefix site run check` and `npm --prefix site run build`, then follow GitHub Pages/project hosting settings. The site must not depend on a live Decodex daemon unless a future accepted product decision changes that boundary.

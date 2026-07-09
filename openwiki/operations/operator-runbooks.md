# Operator Runbooks

This page is the operational map for Decodex recovery, GitHub/Linear handling, release readiness, and control-plane workflows.

## Validation gate

Use `cargo make` when it has an equivalent task. The Decodex project workflow currently uses `cargo make fmt` followed by `cargo make test` as the full landing gate. Targeted checks are useful while editing, but landing-related refreshes need the workflow gate unless the task authority says otherwise.

Local validation status publishing should use `decodex verify publish-status` with expected PR head, base ref, and base oid. This prevents a stale local green result from being attached after the PR or target branch moved.

## GitHub operations

Use Decodex-owned lifecycle commands for Decodex repository history:

```sh
decodex commit "summary" --authority XY-123
decodex commit "summary" --manual-authority
decodex land "summary" --authority XY-123
decodex land "summary" --manual-authority --pr <URL>
```

The commit subject must be a single-line `decodex/commit/2` JSON object. It describes the tree change only. Do not encode PR URL, branch, validation, CI, landing, or closeout state into the commit subject.

If landing is blocked, inspect PR state, review state, merge state, required status contexts, branch freshness, and Decodex lifecycle records before using any manual GitHub fallback.

## Lane-control recovery

Use lane-control recovery when a current run is stuck, stale, or needs explicit steering. Start with inspect:

```sh
decodex lane inspect <ISSUE> --run-id <RUN_ID> --json
```

Steer requires the current expected turn id. Interrupt prefers the soft protocol path. Forced hard fallback is a recovery path, not a normal workflow shortcut. After recovery, confirm the runtime recorded the result and that operator status no longer reports the stale active condition.

## Review handoff recovery

Retained review handoff recovery is for PR-backed lanes whose review/landing lifecycle authority is missing, stale, or inconsistent. Use diagnose/read-only commands first. Rebind or adopt only when the evidence proves the intended PR, branch, issue, and head relationship.

Do not reconstruct retained lifecycle from current branch names or Linear comments alone. Recovery must persist lifecycle authority so later dispatch, repair, landing, and closeout operate from runtime state.

## Ghost lanes and stale active ownership

Ghost lane recovery handles local evidence for runs or worktrees that no longer have a valid active owner. Stale active recovery handles claims that survived crashes or interrupted runs. Recovery should distinguish:

- active process still owns the run
- retained review or closeout work still owns the lane
- local marker/control-channel evidence blocks cleanup
- tracker issue identity is missing or invalid
- cleanup can safely remove stale local state

When in doubt, keep the evidence and surface manual attention instead of deleting possible active ownership.

## Linear archive hygiene

Archive hygiene should dry-run first, list candidate issues, and preserve exclusions. Do not archive issues that still have active Decodex ownership, retained review/closeout state, needs-attention markers, queued labels, or unresolved tracker blockers.

## Control-plane upgrades

Control-plane upgrade work should use explicit candidate artifacts, source references, impact classification, and operator-readable validation. Upgrade candidates are not direct mutation authority; they become implementation work only after acceptance through the normal planning/intake path.

Stop when required evidence is missing, review would affect authority boundaries, or the candidate needs a human decision.

## Release readiness

Release readiness needs a tag contract, validation evidence, and release-note content. Confirm build/test gates, plugin/app/site artifacts, Radar/Publisher artifact validation when relevant, and install/update paths before cutting a release.

## Social publishing

Social publishing flows through Publisher artifacts and reservations. A social candidate needs source references, decision state, claim boundaries, and publication mode. Reservations prevent duplicate publication. Publisher should not run fresh upstream analysis; it consumes accepted Radar handoff evidence.

## Site deployment

The public site is static Astro. Deployment work should verify `npm --prefix site run check` and `npm --prefix site run build`, then follow GitHub Pages/project hosting settings. The site must not depend on a live Decodex daemon unless a future accepted product decision changes that boundary.

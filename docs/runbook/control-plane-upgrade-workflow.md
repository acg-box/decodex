---
type: "Runbook"
title: "Control Plane Upgrade Workflow"
description: "Operate the bridge from upstream Codex Radar evidence to Decodex Control Plane upgrade work."
status: active
authority: procedural
owner: automation
tags: [runbook, radar, control-plane]
code_refs: [automations/radar/prompts/upstream-review.md, automations/radar/automations.toml, apps/radar/src/lib.rs]
drift_watch: [control_plane_upgrade_candidate/v1, Codex compatibility matrix, decodex probe, Program Intake, Decision Contract]
last_verified: 2026-06-27
---
# Control Plane Upgrade Workflow

Purpose: Operate the bridge from upstream Codex Radar evidence to Decodex Control Plane
upgrade work.

Read this when: Radar finds an upstream Codex app-server, protocol, MCP, plugin,
browser, sandbox, auth, config, or CLI change that may affect Decodex.

Not this document: The artifact schema. Read
[`../spec/control-plane-upgrade-candidate.md`](../spec/control-plane-upgrade-candidate.md).
The current tested Codex versions are recorded in
[`../reference/codex-compatibility-matrix.md`](../reference/codex-compatibility-matrix.md).

## Preconditions

- Decodex App, CLI, and `decodex serve` are installed from the intended Decodex build.
- `decodex probe` passes against the local Codex app-server path.
- Radar Review automation is active and its prompt matches
  `automations/radar/prompts/upstream-review.md`.
- Generated Radar artifacts stay under `.agent/automations/radar/cache`.

## Sequence

1. Refresh upstream source state:

   ```sh
   radar refresh-upstream-queue --repo openai/codex
   radar refresh-release-delta --repo openai/codex
   ```

2. Review high-priority queue subjects through the Radar Review automation or an
   explicit operator-run Codex analysis pass.

3. Persist source-backed `upstream_review/v1`. When a reviewed change affects Decodex
   compatibility or adoption, also persist `upstream_impact/v1`; this is the shared
   upstream scan artifact that Control Plane upgrade candidates and release publishing
   both reuse.

4. If `control_plane_impact` is `candidate`, `compat_risk`, or `adopt_now`, write a
   `control_plane_upgrade_candidate/v1` artifact under:

   ```text
   .agent/automations/radar/cache/github/control-plane-upgrades/
   ```

   Cite the matching `upstream_impact/v1` in `source_refs.upstream_impacts`. Use
   `release_delta/v1`, release URLs, or `target_codex` fields for version context
   rather than re-fetching or reinterpreting upstream Codex independently.

5. Validate the changed artifacts:

   ```sh
   radar validate \
     .agent/automations/radar/cache/github/reviews \
     .agent/automations/radar/cache/github/impact \
     .agent/automations/radar/cache/github/control-plane-upgrades
   ```

6. Update [`../reference/codex-compatibility-matrix.md`](../reference/codex-compatibility-matrix.md)
   when the candidate changes the known stable or preview compatibility state.

7. Promote only through normal authority:

   - accepted Decision Contract
   - Program Intake
   - normal Decodex lane execution
   - normal validation, review, land, install, restart, and plugin-sync gates

   Accepted autonomy project policy may authorize drafting, challenge, or acceptance
   of a Decision Contract candidate. It does not replace the accepted Decision Contract
   in the execution path.

## Stop Conditions

Do not promote a candidate when any of these is true:

- no source-backed `upstream_review/v1` exists
- no matching `upstream_impact/v1` exists for a Radar-derived candidate
- the candidate has no target Codex version, tag, commit, or release URL
- `authority.mutation_allowed` is not `false`
- `decision_contract_required` or `program_intake_required` is not `true`
- `decodex probe` fails for the tested Codex path
- affected surfaces are too broad to validate in one coherent lane
- compatibility depends on private or unobservable upstream rollout state

## Operator Notes

Radar Review may propose upgrade candidates, but it does not install Codex, modify
Decodex source, restart services, open PRs, create Linear issues, or enqueue Program
nodes. Those actions belong to the normal Decodex execution lifecycle after promotion.

When a candidate is only a watch item, keep it as `status = "deferred"` or leave the
upstream impact at `control_plane_impact = "watch"` without creating an executable
lane.

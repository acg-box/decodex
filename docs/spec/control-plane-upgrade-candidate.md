---
type: "Spec"
title: "Control Plane Upgrade Candidate"
description: "Define the evidence-only artifact that turns upstream Codex changes into Decodex Control Plane upgrade candidates."
status: active
authority: normative
owner: automation
tags: [spec, radar, control-plane]
code_refs: [apps/radar/src/lib.rs, automations/decodex/scripts/github/control_plane_upgrade_candidate.schema.json]
drift_watch: [control_plane_upgrade_candidate/v1, control_plane_upgrade_candidate, control-plane-upgrades, Codex compatibility matrix, Decision Contract, Program Intake]
last_verified: 2026-06-27
---
# Control Plane Upgrade Candidate

Purpose: Define the evidence-only artifact that turns upstream Codex changes into
Decodex Control Plane upgrade candidates.

Status: normative

Read this when:
- You are deciding whether an upstream Codex change should trigger Decodex runtime,
  app-server, plugin, MCP, browser, sandbox, config, or automation upgrade work.
- You are validating Radar artifacts under
  `.agent/automations/decodex/cache/github/control-plane-upgrades/`.
- You are bridging Codex release monitoring into Decodex Program Intake.

Not this document:
- The upstream review source-analysis boundary. Read [`upstream-review.md`](./upstream-review.md).
- The editorial impact shape. Read [`upstream-impact.md`](./upstream-impact.md).
- The project-autonomy authority model. Read
  [`autonomy-control-plane.md`](./autonomy-control-plane.md).
- The upgrade procedure. Read
  [`../runbook/control-plane-upgrade-workflow.md`](../runbook/control-plane-upgrade-workflow.md).

Defines:
- The `control_plane_upgrade_candidate/v1` artifact.
- The rule that upstream Radar automation may propose upgrade candidates but must not
  directly mutate Decodex execution, tracker, GitHub, source, installs, or runtime
  authority.
- The bridge from upstream Codex compatibility evidence into Decision Contract and
  Program Intake.

## Artifact Identity

The canonical schema identifier is:

- `control_plane_upgrade_candidate/v1`

Recommended checked-in location:

- `.agent/automations/decodex/cache/github/control-plane-upgrades/<source-slug>.json`

Rust validation entrypoint:

- `radar validate`

## Required Fields

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `control_plane_upgrade_candidate/v1`. |
| `slug` | string | Stable URL-safe identifier. |
| `repo` | string | Upstream repository, normally `openai/codex`. |
| `status` | string | `proposed`, `deferred`, `blocked`, or `superseded`. |
| `source_refs` | object | Upstream review, impact, release-delta, or URL evidence. |
| `observed_change` | string | One factual sentence about the upstream change. |
| `control_plane_impact` | string | `adopt_now`, `candidate`, or `compat_risk`. |
| `upgrade_path` | string | `adopt_now`, `compat_risk_mitigation`, or `discovery`. |
| `affected_surfaces` | array | Non-empty Decodex surfaces that may need work. |
| `target_codex` | object | Codex channel/version/tag/commit/release evidence. |
| `authority` | object | Decision Contract and Program Intake guard fields. |
| `reason` | string | Why this candidate exists. |
| `validation_gates` | array | Non-empty commands or checks required before adoption. |
| `stop_conditions` | array | Non-empty conditions that prevent unattended promotion. |

Optional fields:

- `acceptance_criteria`: checks that would make the candidate ready for promotion.
- `caveats`: uncertainty, rollout limits, or source gaps.
- `next_steps`: evidence-gathering or review steps that still preserve the candidate as
  non-executable.

## Source References

`source_refs` must include:

- `upstream_impacts`

`source_refs` may also include:

- `upstream_reviews`
- `release_deltas`
- `urls`

Treat `upstream_impacts` as the shared upstream scan artifact that release publishing
and Control Plane proposal work reuse.
`upstream_reviews`, `release_deltas`, and URLs may supply provenance, target-version
context, or gap evidence, but they should not be the only source for a new Decodex
Control Plane candidate when a reviewed `upstream_impact/v1` exists or can be produced
by Radar Review.

`urls` must use HTTPS. Local generated artifact references should use repository-relative
paths under `.agent/automations/decodex/cache`.

## Target Codex

`target_codex` must name the upstream target with at least one of:

- `version`
- `tag`
- `commit_sha`
- `release_url`

`channel` must be `stable`, `preview`, or `main`. `compatibility_status`, when present,
must be one of `compatible`, `incompatible`, `needs_review`, `not_tested`, or `unknown`.
`matrix_ref` should point to [`../reference/codex-compatibility-matrix.md`](../reference/codex-compatibility-matrix.md)
when the candidate is tied to a known Codex release or preview.

The compatibility matrix is planning evidence. It must not become version-string
dispatch logic. Runtime compatibility remains capability-probed through Decodex app-server
preflight, `decodex probe`, and the relevant validation gates.

## Authority Guard

`authority` must set:

- `decision_contract_required = true`
- `program_intake_required = true`
- `mutation_allowed = false`

These fields make the artifact evidence-only. A candidate cannot create Linear issues,
enqueue Program nodes, mutate GitHub, edit worktrees, install Codex or Decodex, restart
servers, change project config, or write runtime authority rows. Executable promotion
requires an accepted Decision Contract and then Program Intake. Accepted project
autonomy policy may authorize drafting, challenging, or accepting a Decision Contract;
it does not replace the accepted Decision Contract for execution.

## Relationship To Other Artifacts

`control_plane_upgrade_candidate/v1` may consume:

- `upstream_review/v1`
- `upstream_impact/v1`
- `release_delta/v1`
- official Codex release URLs
- source-backed local validation evidence

For normal upstream Codex Radar output, consume `upstream_impact/v1` as the
Control Plane handoff and carry release/version evidence through `source_refs` and
`target_codex`. This keeps release publishing and protocol-upgrade proposal analysis
on one reviewed Radar artifact instead of duplicate upstream fetches or parallel
interpretations.

It may later support:

- a latent research contract
- an accepted Decision Contract
- Program Intake
- normal executable Decodex lanes

It does not replace those artifacts.

---
type: "Spec"
title: "Upstream Impact"
description: "Define how Decodex classifies upstream Codex changes for public content and control-plane follow-up."
status: active
authority: normative
owner: automation
tags: [spec, radar]
last_verified: 2026-06-25
---
# Upstream Impact

Purpose: Define how Decodex classifies upstream Codex changes before they become public
signals, Control Plane follow-up work, or social publishing records.

Status: normative

Read this when:
- You are analyzing an OpenAI Codex PR, commit, release note, or developer changelog.
- You need to decide whether a Radar finding should create public content, Control
  Plane work, both, or neither.
- You are designing or validating an upstream-impact artifact.

Not this document:
- The GitHub input bundle schema. Read [`github-change-bundle.md`](./github-change-bundle.md).
- The upstream review queue and AI review boundary. Read
  [`upstream-review.md`](./upstream-review.md).
- The published site signal schema. Read [`signal-entry.md`](./signal-entry.md).
- The social publishing schema. Read [`social-publishing.md`](./social-publishing.md).
- The social candidate handoff schema. Read
  [`social-candidate.md`](./social-candidate.md).
- The operator procedure for publishing. Read
  [`../runbook/social-publishing-workflow.md`](../runbook/social-publishing-workflow.md).

Defines:
- The `upstream_impact/v1` classification shape.
- The Control Plane impact ladder.
- The Publisher angle ladder.
- Evidence and confidence rules for turning upstream changes into follow-up work.

## Artifact identity

The canonical schema identifier is:

- `upstream_impact/v1`

Recommended checked-in location:

- `.agent/automations/decodex/cache/github/impact/<source-slug>.json`

## Required fields

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `upstream_impact/v1`. |
| `slug` | string | Stable URL-safe identifier, usually matching the source bundle stem. |
| `repo` | string | Upstream repository, such as `openai/codex`. |
| `source_refs` | object | PR, commit, release, changelog, or signal references used as evidence. |
| `observed_change` | string | Short factual description of the upstream change. |
| `public_signal_decision` | string | `publish`, `defer`, or `skip`. |
| `control_plane_impact` | string | One value from the Control Plane impact ladder. |
| `publisher_angle` | string | One value from the Publisher angle ladder. |
| `confidence` | string | `confirmed`, `likely`, or `weak`. |
| `evidence` | array | Non-empty list of source-backed evidence notes. |

Optional fields:

- `candidate_followups`: bounded engineering or research follow-up suggestions.
- `social_notes`: notes useful to a later `social_candidate/v1` or terminal
  `social_post/v1`.
- `caveats`: uncertainty, version gating, platform limits, or rollout limits.

## Control Plane impact ladder

Use exactly one `control_plane_impact` value:

| Value | Meaning |
| --- | --- |
| `none` | No plausible Control Plane implication. |
| `watch` | Worth tracking, but no concrete Decodex runtime or operator action is clear yet. |
| `candidate` | Could improve Control Plane and deserves a bounded issue or research pass. |
| `compat_risk` | May break, narrow, or change assumptions in app-server, plugin, config, permission, sandbox, browser, MCP, or tracker flows. |
| `adopt_now` | Evidence is strong enough to create an implementation issue without more discovery. |

`compat_risk` takes precedence over `candidate` when both apply.

## Publisher angle ladder

Use exactly one `publisher_angle` value:

| Value | Meaning |
| --- | --- |
| `none` | Do not use the change for external content. |
| `release_pulse` | Short release-aware awareness post. |
| `practical_explainer` | User-facing explanation of how to use or evaluate the change. |
| `operator_impact` | Decodex-specific explanation of what the change means for agent orchestration or app-server workflows. |
| `watch_note` | Cautious public note when the change is interesting but not ready for a strong claim. |

Prefer `practical_explainer` or `operator_impact` when the evidence supports a concrete
workflow. Use `release_pulse` only when the post would otherwise be a factual release
summary.

## Evidence rules

- Evidence must come from source material: PR body, commit message, file path, patch
  excerpt, release note, developer changelog, checked-in Decodex signal, or verified
  browser observation.
- Do not infer shipped user behavior from internal names alone.
- Do not classify a change as `adopt_now` without a concrete Decodex surface that would
  change.
- Do not classify a change as `practical_explainer` without a clear user-observable
  path.
- Lower confidence when the source is commit-only, release-note-only, or hidden behind
  private/beta rollout language.

## Relationship to other artifacts

`upstream_impact/v1` is an editorial bridge artifact:

- It may consume `github_change_bundle/v1`.
- It should normally consume a source-backed `upstream_review/v1` conclusion when the
  change came from continuous Radar.
- It may support a `signal_entry/v1`.
- It may support a `social_candidate/v1` or terminal `social_post/v1`.
- It may justify a later Linear issue or implementation brief.

It does not replace any of those artifacts.

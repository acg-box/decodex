---
type: "Spec"
title: "Upstream Review"
description: "Define how Decodex Radar turns observed upstream Codex changes into source-backed review artifacts."
status: active
authority: normative
owner: automation
tags: [spec, radar]
last_verified: 2026-06-27
---
# Upstream Review

Purpose: Define how Decodex Radar turns every observed upstream Codex commit into a
reviewable evidence unit before public publishing or Control Plane follow-up.

Status: normative

Read this when:
- You are changing continuous upstream Codex tracking.
- You are building automation that asks AI to analyze Codex commits or PRs.
- You need to decide what deterministic local automation may refresh without Codex auth.
- You are deciding whether an upstream change should become a signal, impact artifact,
  social post, or Decodex engineering follow-up.

Not this document:
- The normalized GitHub source bundle schema. Read [`github-change-bundle.md`](./github-change-bundle.md).
- The Control Plane and Publisher impact shape. Read [`upstream-impact.md`](./upstream-impact.md).
- The Control Plane upgrade candidate shape. Read
  [`control-plane-upgrade-candidate.md`](./control-plane-upgrade-candidate.md).
- The public signal schema. Read [`signal-entry.md`](./signal-entry.md).
- The raw artifact archive policy. Read [`radar-artifact-retention.md`](./radar-artifact-retention.md).

Defines:
- The deterministic `upstream_review_queue/v1` artifact.
- The AI-owned `upstream_review/v1` artifact.
- The rule that release and prerelease tags are rollup checkpoints over commit and PR
  evidence, not first-class discovery roots.
- The promotion boundary from upstream review into impact, site, social, or Control
  Plane upgrade candidate work.

## Core rule

Radar tracks every recent upstream Codex commit as an evidence unit. Continuous sync
must record every observed commit in the local Radar ledger and group it by pull
request when GitHub exposes a PR mapping. It must not skip commits only because the
commit title looks like maintenance.

Deterministic sync may assign surface hints and review priority, but only AI review may
decide the actual user impact, Decodex compatibility risk, adoption opportunity, or
community publishing value.

## Artifact identities

The deterministic queue schema identifier is:

- `upstream_review_queue/v1`

Recommended checked-in location:

- `.agent/automations/decodex/cache/github/review-queue/openai-codex-latest.json`

Rust refresh entrypoint:

- `radar refresh-upstream-queue`

The AI review schema identifier is:

- `upstream_review/v1`

Recommended checked-in location:

- `.agent/automations/decodex/cache/github/reviews/<source-slug>.review.json`

Review artifacts are hot Radar artifacts unless they are promoted into
`upstream_impact/v1`, `signal_entry/v1`, `social_candidate/v1`, or
`control_plane_upgrade_candidate/v1`. Apply the 21-day hot-window rule from
[`radar-artifact-retention.md`](./radar-artifact-retention.md).

## Queue requirements

`upstream_review_queue/v1` must contain:

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `upstream_review_queue/v1`. |
| `repo` | string | Upstream repository, normally `openai/codex`. |
| `generated_at` | string | UTC generation timestamp. |
| `source` | object | Default branch, search limit, and source directory context. |
| `subjects` | array | Unique PR or commit subjects that still need AI review. |
| `counts` | object | Commit scan and priority counts for automation routing. |

Each `subjects[]` record must contain:

| Field | Type | Notes |
| --- | --- | --- |
| `subject_kind` | string | `pr` or `commit`. |
| `subject_id` | string | PR number as a string, or a commit SHA. |
| `title` | string | PR title or commit title. |
| `url` | string | GitHub source URL. |
| `source_state` | string | `merged`, `open`, `closed`, or `commit_only`. |
| `commit_shas` | array | One or more observed commit SHAs tied to the subject. |
| `changed_file_count` | number | Number of files in the fetched source bundle. |
| `sample_paths` | array | Bounded changed-path sample for routing only. |
| `surface_hints` | array | Deterministic hints such as `app_server_protocol` or `mcp_plugins`. |
| `attention_flags` | array | Deterministic hints such as `deprecated_removed` or `protocol_change`. |
| `review_priority` | string | `critical`, `high`, `normal`, or `low`. |
| `review_reason` | string | Why AI review is still required. |
| `next_step` | string | Must be `ai_review_required`. |

The queue is a routing artifact. It must not claim final behavior, compatibility risk,
or public value.

## AI review requirements

`upstream_review/v1` must be source-backed and contain:

- the reviewed `subject_kind`, `subject_id`, and source URLs
- the observed change in one factual sentence
- changed surfaces
- user-visible path, if any
- Decodex Control Plane relevance
- Decodex compatibility risk, if any
- adoption opportunity, if any
- community publishing value, if any
- deprecated, removed, migration, or breaking-change notes, if any
- confidence: `confirmed`, `likely`, or `weak`
- source-backed evidence notes
- next actions, each mapped to `none`, `upstream_impact`, `signal_entry`,
  `social_candidate`, or `control_plane_upgrade_candidate`

AI review must read enough source evidence to explain behavior. A PR title, release
title, or deterministic queue hint is not enough for a confirmed claim.

The remaining Python analysis helper, `automations/decodex/scripts/github/run_codex_analysis.py`, is only
the bounded deterministic process wrapper for this AI review boundary. It must validate
the input `github_change_bundle/v1`, run Codex with the checked `analysis_draft` output
schema, validate the returned draft again before writing it, and require an explicit
`--allow-ai-analysis-boundary` flag or `DECODEX_ALLOW_CODEX_ANALYSIS=1` environment
acknowledgement. The normal operator command surface remains Rust-owned
`radar ...`; GitHub Actions must not set that acknowledgement.

## Promotion boundary

Promote an upstream review into:

- `upstream_impact/v1` when it affects Decodex compatibility, Control Plane adoption,
  or Publisher planning.
- `signal_entry/v1` when it is community-ready and has user-visible capability,
  behavior, try path, or migration value.
- `social_candidate/v1` when there is a clear public angle and source links are
  available. Publisher later decides whether to write terminal `social_post/v1`.
- `control_plane_upgrade_candidate/v1` when Decodex should adopt, guard, migrate, or
  investigate the change. The candidate remains evidence-only until Decision Contract
  and Program Intake promotion.

Historical raw `upstream_review/v1` records created before the shared handoff cutover
may still contain `linear_followup` in `next_actions` so archive validation can read
old cache state. New upstream review output must use `upstream_impact` or
`control_plane_upgrade_candidate`; it must not emit `linear_followup`.

Do not promote low-value internal churn into public artifacts. Keep it traceable in the
ledger and use it only as release-rollup background if later evidence makes it relevant.

## Release and prerelease checkpoints

Release and prerelease tags are summary checkpoints over accumulated upstream reviews.
They may trigger a gap scan, but they must not replace commit and PR evidence.

This matters most for Codex prereleases because prerelease bodies may be sparse or empty.
Rollups should combine prior reviews, impact artifacts, public signals, and compare
metadata before producing a social candidate or X post.

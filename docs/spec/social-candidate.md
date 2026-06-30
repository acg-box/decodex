---
type: "Spec"
title: "Social Candidate"
description: "Define the Publisher candidate artifact produced before social publication decisions."
status: active
authority: normative
owner: automation
tags: [spec, publishing]
code_refs: [apps/radar/src/lib.rs, automations/decodex/scripts/github/social_candidate.schema.json]
drift_watch: [social_candidate/v1, upstream_impact/v1, source_refs.upstream_impacts]
last_verified: 2026-06-27
---
# Social Candidate

Purpose: Define the checked-in Publisher candidate artifact produced by upstream Radar
source analysis before any social publication record is written.

Status: normative

Read this when:
- You are selecting source-backed upstream Codex changes for possible public
  `@decodexspace` publishing.
- You need to hand off a Publisher opportunity without composing, posting, blocking, or
  skipping an X publication.
- You are validating that public-candidate claims are backed by upstream review,
  upstream-impact, signal, release-delta, or source URL evidence.

Not this document:
- The publication, block, skip, or failure record. Read
  [`social-publishing.md`](./social-publishing.md).
- The upstream source-analysis contract. Read [`upstream-review.md`](./upstream-review.md).
- The Control Plane impact bridge. Read [`upstream-impact.md`](./upstream-impact.md).

Defines:
- The `social_candidate/v1` artifact shape.
- The boundary between upstream source analysis and later Publisher publication.

## Artifact Identity

The canonical schema identifier is:

- `social_candidate/v1`

Recommended checked-in location:

- `.agent/automations/decodex/cache/github/social-candidates/<source-slug>.json`

`social_candidate/v1` is a handoff artifact, not a publication ledger entry. It must not
claim that a post was published, skipped, blocked, or failed. Downstream Publisher
automation may consume it to decide whether and when to write `social_post/v1`.

## Required Fields

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `social_candidate/v1`. |
| `slug` | string | Stable URL-safe identifier, usually matching the source review slug. |
| `repo` | string | Upstream repository, such as `openai/codex`. |
| `channel` | string | Must be `x` for the current Publisher lane. |
| `target_account` | string | Must be `decodexspace`. |
| `mode` | string | One value from the social post modes in [`social-publishing.md`](./social-publishing.md). |
| `priority` | string | `critical`, `high`, `normal`, or `low`. |
| `audience` | string | Primary reader group for the later post. |
| `candidate_text` | array | One or more draftable post bodies, each no more than 280 characters. |
| `source_refs` | object | References to upstream review, upstream impact, signal, release-delta, or source URLs. |
| `evidence_notes` | array | Non-empty list of evidence-backed notes. |
| `claims` | array | Non-empty list of user-facing claims with evidence and confidence. |
| `decision` | object | Worthiness, reason, and idempotency key for downstream Publisher intake. |

Optional fields:

- `caveats`: rollout limits, version gates, uncertainty, or platform limits.
- `media_refs`: checked-in or local generated assets intended for downstream Publisher
  review.
- `next_steps`: concrete downstream Publisher or editorial follow-up actions.

## Source References

`source_refs` must include at least one of:

- `upstream_reviews`
- `upstream_impacts`
- `signals`
- `release_deltas`
- `urls`

Prefer a source-backed `upstream_review/v1` plus an `upstream_impact/v1` when the
candidate comes from continuous Radar.
For new Radar-derived release or prerelease candidates, `upstream_impacts` is the
shared handoff from Radar Review into Publisher. `release_deltas`, official release
URLs, compare metadata, and `upstream_reviews` can support channel lineage and claim
evidence, but they should not become a parallel release-analysis source when a matching
`upstream_impact/v1` exists or Radar Review can produce one.
`radar validate` rejects a Radar-derived social candidate that cites
`upstream_reviews` or `release_deltas` without also citing `upstream_impacts`.

## Decision Object

`decision` must contain:

| Field | Type | Notes |
| --- | --- | --- |
| `worthiness` | string | `publish`, `defer`, or `skip`. |
| `reason` | string | Short source-backed reason. |
| `idempotency_key` | string | Stable key derived from account, source, and mode. |

`social_candidate/v1` does not define top-level `status` or `decision.outcome`
fields. Downstream Publisher automation must use `decision.worthiness = "publish"` as
the schema-defined handoff signal, plus source refs and quality-system review, when
deciding whether to write a `social_post/v1` record.

Candidate producers may add explanatory `next_steps`, but they must not rely on
non-schema publishability fields that the validator does not require.

Prerelease candidates must preserve enough structure for downstream Publisher review.
When compare metadata includes named PRs or commit titles, `candidate_text` should not
collapse the release into a generic theme paragraph. It should separate important
PR/commit clusters, anticipated user-facing changes, protocol/API/schema changes, and
caveats across thread nodes or compact bullets.

Public candidate text should make important PR references clickable on first mention.
Use direct GitHub PR URLs in `candidate_text` for the PRs that carry the reader-facing
claim. Raw `#12345` shorthand is acceptable only after the URL has appeared or when a
single exact compare URL intentionally covers many small PR references within the X
length limit.

Prerelease candidates must also preserve channel lineage. Evidence notes or source refs
must identify the previous checkpoint, current checkpoint, adjacent compare URL, whether
the checkpoint is the first prerelease after a stable release, and the previous live
and quote-eligible `@decodexspace` prerelease post URL when one exists. Do not hand off
a prerelease candidate that only compares the latest stable release to the latest
prerelease when an adjacent prerelease-to-prerelease comparison is available.

## Boundary Rules

- Do not write `social_post/v1` from the upstream source-analysis automation.
- Do not publish to X from a `social_candidate/v1` producer.
- Do not include claims that are not backed by `upstream_review/v1`, `upstream_impact/v1`,
  `signal_entry/v1`, `release_delta/v1`, official changelog entries, release metadata,
  compare metadata, or source URLs.
- Do not imply Decodex runtime support unless Control Plane evidence exists.
- Do not use dense prose that hides concrete prerelease evidence when the source can be
  expressed as PR/commit bullets, protocol/API changes, and alpha caveats.
- Do not leave important public PR references as raw PR-number-only text when direct
  GitHub PR URLs fit in the candidate.
- Do not mix stable release and prerelease channels. Release candidates use
  stable-to-stable comparison; prerelease candidates use adjacent prerelease comparison,
  except the first prerelease after a stable release.
- Use `social_candidate` as the upstream-review next action for public Publisher
  opportunities that are not yet publication records.

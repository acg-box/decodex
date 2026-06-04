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

- `artifacts/github/social-candidates/<source-slug>.json`

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

## Decision Object

`decision` must contain:

| Field | Type | Notes |
| --- | --- | --- |
| `worthiness` | string | `publish`, `defer`, or `skip`. |
| `reason` | string | Short source-backed reason. |
| `idempotency_key` | string | Stable key derived from account, source, and mode. |

## Boundary Rules

- Do not write `social_post/v1` from the upstream source-analysis automation.
- Do not publish to X from a `social_candidate/v1` producer.
- Do not include claims that are not backed by `upstream_review/v1`, `upstream_impact/v1`,
  `signal_entry/v1`, `release_delta/v1`, or source URLs.
- Do not imply Decodex runtime support unless Control Plane evidence exists.
- Use `social_candidate` as the upstream-review next action for public Publisher
  opportunities that are not yet publication records.

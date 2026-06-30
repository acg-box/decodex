# Decodex Publisher Skills

Purpose: Route repo-local skills for Decodex Publisher public-post quality and
publication records.

These skills are checked-in repository-development instructions. They are not packaged
with the installable Decodex plugin under `plugins/decodex/`.

## Skill Map

1. `x-post-quality-system`: decide whether an artifact-backed `@decodexspace` post or
   generated media is worth publishing.
2. `x-post-publisher`: consume social candidates whose `decision.worthiness =
   "publish"` or explicit operator handoffs that name checked Radar artifacts, then
   write a low-frequency `social_post/v1` publication, block, skip, or failure record.

## Pipeline Ownership

Publisher is an artifact consumer. It starts from validated `signal_entry/v1`,
`upstream_impact/v1`, `release_delta/v1`, `social_candidate/v1`, or explicit operator
handoff evidence. If upstream evidence is missing or too weak, Publisher must return
`upstream_analysis_required` instead of reading upstream source directly.

Publisher contracts are `social_candidate/v1`, `social_publish_reservation/v1`, and
`social_post/v1`. Validate them with `decodex-publisher validate-social`.

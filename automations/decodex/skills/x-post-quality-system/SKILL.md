---
name: x-post-quality-system
description: Use when Decodex X Publisher needs to decide whether an artifact-backed @decodexspace post or generated media is worth publishing.
---

# Decodex X Post Quality System

Use before `x-post-publisher` decides whether a `social_candidate/v1` or explicit
operator handoff is worth publishing. This skill raises the bar from source-backed
correctness to public-reader value and media quality.

## Read First

- `../references/social-release-publisher-gates.md`

## Hard Boundaries

- Do not read upstream Codex source or invent missing technical evidence.
- Keep weak candidates as `decision.worthiness = "defer"` or `"skip"`, or write a
  terminal `skipped`/`blocked` `social_post/v1` when Publisher has already started.
- Historical `@CodexReleases` and `@Codex_Changelog` observations are style references
  only. Recurring automation must not browse them or use their coverage as evidence.

## Editorial Gate

Publish only when the candidate answers all three questions in one screen:

- What changed?
- Who can use, observe, or act on it?
- What source proves it?

Reject candidates that are only single-PR renames, trace-only compatibility notes,
narrow bug/fix details, Decodex-internal audit targets, low-context cautions, or generic
"watching this" notes. They may remain Radar artifacts, but should not become X posts
unless they roll up into a broader release, product update, concrete workflow change, or
external operator decision.

For prerelease threads, reject generic theme prose when compare metadata contains named
PRs or commit titles. The draft must surface at least two of: important PR/commit
clusters, anticipated user workflow changes, protocol/API/schema changes, removals or
compatibility boundaries, and plugin/config/sandbox/tool/release-engineering changes.

Formatting is part of the quality bar. Reject dense paragraphs when bullets, blank
lines, and direct PR URLs would make the evidence easier to scan.

Reject post bodies that start with or repeat automation attribution such as
`Automated by @hackink`. Attribution belongs in durable records and account metadata,
not in the reader-facing post text. Also reject cramped post bodies longer than 260
characters unless one unavoidable source URL forces the post toward X's limit; prefer
a short thread over a single packed paragraph. A good post should leave room for X
link rendering, quote metadata, and minor manual edits without hitting the hard
platform limit.

Prefer concrete evidence over generic release prose. A publishable post should name
the release, PR, commit cluster, protocol surface, workflow, or operator action that
matters. If the best available text is only "tracking", "watching", or "new release
available" without a reader action or source-backed implication, keep the candidate as
`defer` or `skip`.

## Media Gate

Use media only when it is fresh, candidate-specific, and adds reader value beyond the
source link card. `decodex_signal_card` is a visual system, not a fixed image.

Required shared elements:

- square card suitable for X preview
- calm technical composition, not a decorative poster
- restrained off-white or near-black background
- thin signal paths, sparse nodes, light grid or terminal/UI structure
- one dominant subject tied to the source
- stable Decodex identity area or small label zone
- deterministic title/source/date overlay when text is needed
- no AI-rendered long text, people, logos, mascots, noisy gradients, generic orbs, or
  source-agnostic placeholders

Before upload, verify the image is specific, visually consistent, readable as an X
preview, and not generic or off-brand. Also verify the candidate is not already live
on the `@decodexspace` profile/timeline and does not conflict with any active
`social_publish_reservation/v1` in durable cache records. X search
`No results` is not enough.
Record prompt/media path/quality outcome in `social_post/v1` evidence notes or caveats
when useful.

Fail closed when media is generic, reused, unavailable but required, or when duplicate
detection, reservation visibility, account verification, upload, or final readback is
unreliable.

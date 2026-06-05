---
name: x-post-quality-system
description: Use when Decodex X Publisher needs to decide whether an artifact-backed @decodexspace post or generated media is worth publishing.
---

# Decodex X Post Quality System

Use this skill before `x-post-publisher` decides that a `social_candidate/v1` or
explicit operator handoff is worth publishing. It raises the bar from source-backed
correctness to public-reader value and media quality.

This is a repo-local skill for the `@decodexspace` automation. It is not a generic
social-media style guide.

This skill evaluates value and media quality only. It must not read upstream Codex
source or invent missing technical evidence; keep weak candidates as
`decision.worthiness = "defer"` or `"skip"` before publishing, or write terminal
`skipped` or `blocked` `social_post/v1` records when the Publisher flow has already
started.

## Benchmarks Studied

The current standard comes from live-readback sampling on 2026-06-03:

- `@CodexReleases`: 55 visible posts/thread nodes from 2026-06-03 back to
  2026-04-24.
- `@Codex_Changelog`: 34 visible posts from 2026-06-03 back to 2026-02-18.

Treat these accounts as operational benchmarks, not as technical evidence for claims.
Every technical claim still needs source-backed GitHub, changelog, release, signal,
upstream-review, or upstream-impact evidence.

## Editorial Bar

Publish only when the candidate can answer all three questions in one screen:

- What changed?
- Who can use, observe, or act on it?
- What source proves it?

Reject candidates that are only:

- single-PR renames
- trace-only compatibility notes
- narrow bug/fix/source-only details
- Decodex-internal audit targets
- low-context operator cautions
- generic "watching this" notes

Those items can still be tracked in Radar artifacts. They should not become X posts
unless they roll up into a broader release, product update, concrete workflow change, or
external operator decision.

## Good Post Shapes

Prefer concise release/update cards, dense changelog summaries, practical workflow
reads, prerelease intros, or concrete operator decisions. A good prerelease intro names
the tag/source/timing, says what is known, and states what Radar still needs to analyze;
it should feel more useful than a bare release bot without inventing details. Use
`operator_impact` only when the public action is external and concrete, such as enabling
or avoiding a provider/config/path, updating an integration assumption, planning a
rollout, or watching a beta/availability boundary. Do not turn a Decodex-internal audit
reminder into an X post.

Use threads only when the first post is valuable alone. Follow-ups should be focused
buckets such as highlights, fixed, added, security, availability, caveats, or source;
do not split a weak candidate into a thread to make it look substantial.

## Visual System

`decodex_signal_card` is a visual system, not a fixed image. Publish media only when a
fresh candidate-specific image is useful; the image must share the same visual grammar.

Required shared elements:

- square card, suitable for X image preview
- calm technical composition, not a decorative poster
- restrained off-white or near-black background
- thin signal paths, sparse nodes, light grid or terminal/UI structure
- one dominant subject tied to the source: release, provider, platform, workflow, or UI
- stable Decodex identity area or small label zone
- deterministic title/source/date overlay when text is needed
- no AI-rendered long text
- no mascots, people, logos, noisy gradients, generic orbs, or stock-like abstraction

Allowed subject variations:

- release card: product/version/date/source
- source card: GitHub or OpenAI changelog source preview
- product card: simplified UI or device/workflow preview
- operator card: provider/config/sandbox/control-plane diagram with concrete labels

Do not reuse:

- prior live-test images
- old generic signal-card images
- unrelated abstract cards
- weak decorative filler

If no fresh, candidate-specific, quality-checked media exists, prefer text-only plus the
source link card when the post still has enough standalone value. Otherwise skip or fail
closed.

Generated image files are temporary Publisher resources. Store them in
`$CODEX_HOME/decodex/social-media/` or temporary storage by default, not Git. The durable
repository record should keep the X status/media URL, prompt summary or content hash
when useful, and any media caveat.

## Image Prompt Contract

Start from the `docs/spec/social-publishing.md` base prompt, then add `subject`,
`source`, `visual_metaphor`, `palette`, and `forbidden` slots. Before upload, verify the
image is candidate-specific, visually consistent, not generic or off-brand, useful with
the post text, and independent of AI-rendered readable text. Record the prompt summary,
content hash or final X media URL, and quality-check outcome in `social_post/v1`
evidence notes or caveats.

## Failure Rules

Fail closed when:

- the candidate is source-backed but not externally valuable
- the post cannot be expressed as release/update, changelog, or concrete operator decision
- the media is generic, reused, ugly, or unavailable but the post depends on media
- duplicate detection, account verification, upload, or final readback is unreliable

Write a `social_post/v1` `skipped` or `failed` record only when the artifact itself has
durable value for later analysis. Do not create repository PRs for meaningless failed
publication records.

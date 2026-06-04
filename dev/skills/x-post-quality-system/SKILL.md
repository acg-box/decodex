---
name: x-post-quality-system
description: Use before Decodex X Publisher decides to publish or generate media. Defines the @decodexspace editorial bar, benchmark-derived post formats, and the decodex_signal_card visual system so automation rejects low-value content and weak images.
---

# Decodex X Post Quality System

Use this skill before `x-post-publisher` decides that a candidate is publishable.
It raises the bar from source-backed correctness to public-reader value and media quality.

This is a repo-local skill for the `@decodexspace` automation. It is not a generic
social-media style guide.

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

Prefer one of these shapes.

### Release Or Update Card

Use when a release, prerelease, app update, or changelog entry is the story.

Pattern:

```text
Codex <app|mobile|CLI> update: <product/version/theme>

What changed:
- <reader-visible change 1>
- <reader-visible change 2>
- <reader-visible change 3>

Source: <source link>
```

Use a short thread only when the main post already has enough value and details would not
fit cleanly. Follow-up posts should be `details`, `fixed`, `availability`, or `source`;
do not fragment weak material into a thread.

### High-Density Changelog

Use when the value is a concise summary of a known source.

Pattern:

```text
Codex <app|CLI> <version/update> is out.

- <high-signal change 1>
- <high-signal change 2>
- <high-signal change 3>

Changelog: <source link>
```

This shape should be dense and source-led. Do not add commentary that hides the actual
change.

### Operator Decision

Use `operator_impact` sparingly. It must read as an external operator decision, not a
Decodex maintenance note.

Good operator-impact posts name a concrete action:

- enable or avoid a provider/config/path
- update an integration assumption
- plan a rollout or migration
- watch a beta/availability boundary

Do not publish an operator-impact post if the action is only "Decodex should audit this
internally."

## Visual System

`decodex_signal_card` is a visual system, not a fixed image. Each publishable candidate
should get a fresh candidate-specific image when media is useful, but the image must share
the same visual grammar.

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

## Image Prompt Contract

Start from the `docs/spec/social-publishing.md` base prompt, then add source-specific
slots:

- `subject`: the concrete release/update/workflow being explained
- `source`: GitHub release, OpenAI changelog, PR, or signal artifact
- `visual_metaphor`: release card, source card, UI/workflow preview, or operator diagram
- `palette`: near-black or off-white with restrained magenta, lime, and blue accents
- `forbidden`: generic abstract art, long text, unreadable labels, people, mascots,
  decorative blobs, unrelated UI

Before upload, perform a visual quality check. The image must pass all:

- It is specific to this candidate.
- It is visually consistent with the Decodex system.
- It is not ugly, noisy, generic, or off-brand.
- It still makes sense if the post text is read first.
- It does not rely on AI-rendered readable text.

Record the prompt, media path, and quality-check outcome in `social_post/v1` evidence
notes or caveats.

## Failure Rules

Fail closed when:

- the candidate is source-backed but not externally valuable
- the post cannot be expressed as release/update, changelog, or concrete operator decision
- the media is generic, reused, ugly, or unavailable but the post depends on media
- duplicate detection, account verification, upload, or final readback is unreliable

Write a `social_post/v1` `skipped` or `failed` record only when the artifact itself has
durable value for later analysis. Do not create repository PRs for meaningless failed
publication records.

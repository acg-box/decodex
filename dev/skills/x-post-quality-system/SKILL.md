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

The standard comes from live-readback sampling, but recurring automation must refresh
the sample before deciding publishability. Do not rely only on stale benchmark memory
when the user is asking about current release coverage.

Recent samples:

- 2026-06-05: `@CodexReleases` covered Codex app `26.602` with a release/update card,
  thread split, and media; it also covered Codex CLI `0.137.0`.
- 2026-06-05: `@Codex_Changelog` covered Codex app `26.602` and Codex CLI `0.137.0`
  with dense source-led bullets and changelog links.
- 2026-06-05: neither benchmark account's visible sample emphasized
  `rust-v0.138.0-alpha.4` prerelease interpretation. This is a Decodex opportunity,
  not a reason to skip.
- 2026-06-03: `@CodexReleases`: 55 visible posts/thread nodes from 2026-06-03 back to
  2026-04-24.
- 2026-06-03: `@Codex_Changelog`: 34 visible posts from 2026-06-03 back to
  2026-02-18.

Treat these accounts as operational benchmarks, not as technical evidence for claims.
Every technical claim still needs source-backed GitHub, official OpenAI changelog,
release, signal, upstream-review, or upstream-impact evidence.

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

Do not reject a current official app update solely because Decodex has no matching
GitHub compare artifact. If the official changelog gives concrete user-visible changes,
the correct gate is source-led `release_pulse` or caveated `watch_note`, not
`needs_upstream_analysis`.

Do reject a sparse prerelease candidate if it tries to infer implementation behavior
from unreviewed code. A sparse prerelease can pass as a precise prerelease-read watch
note: exact tag, source link, compare/backfill status, metadata-derived themes, and a
caveat that release-window source analysis is still open.

For prerelease threads, reject generic "platform plumbing" prose when the compare
window contains named PRs or commit titles. The draft must surface at least two of:

- important PR/commit cluster names or numbers
- anticipated user workflow changes
- protocol/API/schema changes
- removals, deprecations, or compatibility boundaries
- plugin, config, sandbox, tool, or release-engineering changes

Formatting is part of the quality bar. Reject drafts that read as one dense paragraph
when bullets or line breaks would make the source evidence easier to scan.

For public X copy, do not leave important PR references as raw `#12345` only. Prefer
direct clickable PR URLs the first time a PR is used in a thread. Short PR numbers are
acceptable only when the same post or an adjacent source post already provides the URL
or when character budget makes the URL impossible and the final source link covers the
exact compare range.

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

For Codex app updates, prefer this shape when the official changelog has three or more
reader-visible changes. Include the version and the official changelog URL.

For sparse prereleases, use this shape only as a prerelease-read watch note. Replace
"What changed" with "What the metadata suggests" plus "What is still being reviewed"
when needed.

### Prerelease Read Thread

Use when a prerelease is sparse but the compare window carries useful direction.

Pattern:

```text
Codex CLI <version> prerelease read

Worth watching:
- <important PR/commit cluster>
- <anticipated workflow>

Alpha caveat: <limit>
```

```text
Protocol/API changes:
- <PR/commit and concrete contract>
- <PR/commit and concrete contract>
- <removal/deprecation if present>
```

```text
Developer/operator surface:
- <plugin/config/tooling change>
- <image/tool/sandbox/release change>

Source: <compare or release link>
```

Keep each thread node visually comfortable: one idea per post, a blank line after the
headline, compact bullets, and no wall-of-text paragraph.

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
- no reused live-test image, old generic release card, or source-agnostic placeholder

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
- It has enough negative space and contrast for a deterministic overlay or X preview.
- It avoids false UI screenshots, fake OpenAI branding, and unreadable pseudo-text.

Record the prompt, media path, and quality-check outcome in `social_post/v1` evidence
notes or caveats. Include a content hash or final X media URL when available.

## Failure Rules

Fail closed when:

- the candidate is source-backed but not externally valuable
- the post cannot be expressed as release/update, changelog, or concrete operator decision
- the media is generic, reused, ugly, or unavailable but the post depends on media
- duplicate detection, account verification, upload, or final readback is unreliable

Write a `social_post/v1` `skipped` or `failed` record only when the artifact itself has
durable value for later analysis. Do not create repository PRs for meaningless failed
publication records.

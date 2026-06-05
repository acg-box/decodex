---
name: x-post-publisher
description: Use when publishing a checked `social_candidate/v1` or explicit operator handoff to @decodexspace, or writing the matching `social_post/v1` non-published record.
---

# Decodex X Post Publisher

Use after a `social_candidate/v1` with `decision.worthiness = "publish"` exists, or
after an explicit operator handoff names checked Radar artifacts. This skill consumes
that candidate or handoff, optionally posts through Chrome as `@decodexspace`, and
always writes the terminal `social_post/v1` record.

This is a Decodex repository-development instruction surface, not an installable
Decodex plugin skill.

## Required Context

- `docs/spec/social-publishing.md`
- `docs/spec/social-candidate.md`
- `docs/runbook/social-publishing-workflow.md`
- `dev/skills/x-post-quality-system/SKILL.md`

## Boundaries

- Do not read upstream Codex source, patches, or PR files here.
- Do not create fresh analysis. Consume checked artifacts and the candidate or operator
  handoff only.
- Treat X style observations as format input only. Technical claims must come from
  GitHub, changelog, signal, upstream-review, upstream-impact, release-delta, or
  candidate evidence.

## Browser Boundary

Use `@Chrome` for X publication only inside this low-frequency Publisher workflow.
Before composing, verify the logged-in account is `@decodexspace`. If account
verification, Chrome availability, X page structure, media upload, duplicate detection,
or final URL readback is unreliable, do not post. Write `status = "failed"` or
`status = "blocked"` with evidence instead.

Treat Chrome tabs as scoped resources. After account verification, compose, upload,
and final URL readback are done, close or release all tabs opened for the workflow.
Keep a tab only as an explicit human handoff, such as login, CAPTCHA, account approval,
or an unfinished operator-controlled page, and record that handoff in the result.

Style observations from X are not technical evidence. They can shape format and tone,
but every technical claim must point back to GitHub, changelog, signal, upstream-review,
or upstream-impact evidence.

## Benchmark Patterns

Use these as format patterns only:

| Pattern | Good for | Decodex adaptation |
| --- | --- | --- |
| Release/update card | `release_pulse` or high-value `release_rollup` posts. | Product/version/theme headline, two or three reader-visible changes, source link, optional thread details. |
| High-density changelog | One-post summaries from a source changelog or release. | Headline, three high-signal bullets, source card; no extra commentary. |
| Release rollup | `release_rollup` posts after a release or prerelease. | Summarize what prior commit/PR analysis found: useful now, Control Plane impact, deprecations, and watch-only gaps. |
| Human workflow read | `practical_explainer` and `operator_impact`. | Start with the concrete workflow change, then explain why it matters and what caveat remains. |
| Watch note | Interesting but incomplete evidence. | Say what changed, why Radar is watching, and what evidence is still missing. |

Refresh the benchmark sample when publishing about a current Codex app, mobile, CLI, or
prerelease update. `@CodexReleases` and `@Codex_Changelog` often cover the same updates
quickly; Decodex must either add sharper source-backed value or publish a concise,
honest watch note. Do not skip an official app update just because the source is the
OpenAI changelog rather than GitHub.

For prereleases, Decodex should try to be better than the benchmark accounts. If they
do not cover prereleases, publish a careful prerelease read when compare metadata and
PR titles reveal useful direction. Keep it as a thread when necessary, but make the
thread concrete: important PR/commit clusters, anticipated workflow changes,
protocol/API/schema changes, operator-facing changes, then source/caveat.

Keep release and prerelease channels separate. Stable release posts compare the current
stable release to the previous stable release. Prerelease posts compare the current
prerelease to the previous prerelease in the same train; only the first prerelease after
a stable release compares against the stable release baseline.

For prerelease posts after the first checkpoint in a train, quote the previous
`@decodexspace` prerelease post when a previous-post URL exists. This creates a visible
history of the prerelease sequence before the stable release. If the previous
prerelease checkpoint exists but Decodex has no previous post URL, treat that as a
Publisher coverage gap, record it in evidence/caveats, and do not invent a quote.
When a previous `social_post/v1` record has `post_lifecycle.quote_eligible = false`,
do not quote it even if `publication.published_urls` exists; deleted, failed, text-only
test, or superseded posts are lineage evidence, not quote targets.

## Publish Modes

Choose one mode: `release_pulse`, `release_rollup`, `practical_explainer`,
`operator_impact`, `thread`, or `watch_note`. Prefer `practical_explainer`,
`operator_impact`, and source-backed `release_rollup`. A prerelease can be a
`release_pulse` intro or `watch_note` only when tag, source, timing, compare metadata,
and caveats are explicit.

## Worthiness Gate

Publish only when all are true:

- The input is either a `social_candidate/v1` with
  `decision.worthiness = "publish"`, or an explicit operator handoff naming checked
  Radar artifacts.
- The post is in English and passes `x-post-quality-system`.
- The source evidence is enough for every technical claim.
- For `social_candidate/v1` input, `decision.worthiness` is `publish`; do not require
  non-schema fields such as `status = publishable` or `decision.outcome = publishable`.
- `dev/skills/x-post-quality-system/SKILL.md` passes the candidate as externally
  valuable.
- The candidate answers in one screen: what changed, who can use or observe it, and what
  source proves it.
- The item is `critical` or `high`, or it is a release/app/prerelease update with clear
  reader value.
- The post is useful to Codex users, Decodex operators, or builders tracking the Codex
  app-server ecosystem.
- The idempotency key has not already been published or blocked for the same source.
- The daily cap of 8 posts for `@decodexspace` in `Asia/Shanghai` is not reached.

For official Codex app changelog updates, source-backed reader-visible changes are
enough for a `release_pulse` when the post stays within the changelog facts. For sparse
Codex prereleases, publish only as a caveated prerelease-read `watch_note` unless
existing release-window analysis supports a stronger rollup. Metadata-derived themes
are allowed; unreviewed implementation claims are not.

For prerelease-read threads, formatting is a publishability gate. Do not publish a
single dense paragraph that buries evidence. Use explicit line breaks and compact
bullets so readers can scan:

- what is worth watching
- which PR/commit names support it
- what protocol/API contracts changed
- what remains alpha or unreviewed
- which previous prerelease checkpoint or quoted post this post follows

Use clickable PR references. Important PRs should appear as direct GitHub PR URLs when
first introduced in the thread. Raw `#12345` references are secondary shorthand; do not
make readers manually search for the PR.

Skip low-value internal churn. Do not post just because a signal exists. In particular,
do not publish single-PR renames, trace-only compatibility notes, low-context operator
cautions, or Decodex-internal audit reminders unless they roll up into a broader
release/update story or concrete external operator decision.

## Daily Cap

The daily cap is 8 X posts for `@decodexspace`, counted by `Asia/Shanghai` calendar day
unless the operator supplies another timezone.

If publishing the candidate would exceed the cap:

- do not post
- write `social_post/v1` with `status = "blocked"`
- set `block.reason = "daily_cap_exceeded"`
- preserve candidate text, source refs, priority, worthiness reason, and daily counts
- report the block in the automation result so the operator can analyze why volume
  exceeded the cap

## Image Generation

Generate media only when a fresh candidate-specific image can pass quality review.
Never rely on generated readable text or reuse prior live-test, generic, or unrelated
media. Keep generated image files in `$CODEX_HOME/decodex/social-media/` or temporary
storage by default, not Git. After upload, record the X status URL, any `/photo/N`
readback URL, and a short prompt/hash note when useful. If text-only is still valuable
with the source link card, publish text-only; otherwise skip or fail closed.

If the X file chooser, media upload, or post-upload Chrome control channel stalls,
stop the publication attempt. Do not keep retrying upload in the same run, and do not
downgrade to text-only publication unless the operator explicitly approves that
fallback for the current candidate. Write a `status = "failed"` `social_post/v1`
record when the prepared candidate or failure mode has durable audit value.

## Claim Review

Before publishing:

- Map every sentence to evidence.
- Map every prerelease bullet to a PR number, commit title, compare URL, or release
  URL when the claim names a concrete change.
- Check that important PR references in public copy are clickable URLs on first
  mention, unless the thread includes the exact compare URL as the only practical link
  target due to length.
- For prerelease posts, verify the adjacent channel pair and previous-post quote state:
  current prerelease, previous prerelease or stable baseline, compare URL, and previous
  prerelease post URL when one exists. Read `post_lifecycle.quote_eligible` before
  deciding that a historical prerelease post is safe to quote.
- Remove claims based only on social posts or engagement.
- Make beta, rollout, platform, and config gates explicit.
- Avoid local paths, credentials, private issue details, or internal runtime state.
- Keep each `text[]` item within the X length limit.

## Output Record

Write `artifacts/social/x/posts/<yyyy-mm-dd>/<slug>.json` with:

- `schema = "social_post/v1"`
- `channel = "x"`
- `target_account = "decodexspace"`
- `controller_account = "hackink"`
- `status = "published"`, `blocked`, `failed`, or `skipped`
- `source_refs`, `evidence_notes`, `claims`, and `decision`
- `publication` when posted
- `block`, `failure`, or `skip` when not posted
- X media URL or media caveat when media was used or skipped; do not commit generated
  image files unless explicitly operator-approved

If the daily cap would be exceeded, write `status = "blocked"` with
`block.reason = "daily_cap_exceeded"` and preserve candidate text, source refs,
priority, worthiness reason, and daily counts.

Run:

```bash
decodex radar validate artifacts/github/social-candidates artifacts/social/x
```

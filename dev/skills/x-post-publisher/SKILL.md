---
name: x-post-publisher
description: Use when turning Decodex Radar evidence, upstream-impact classifications, signal entries, release analysis, or verified browser style observations into an automated @decodexspace X post or a checked-in social_post/v1 blocked/skipped/failed record.
---

# Decodex X Post Publisher

Use this skill after source evidence exists. Its job is to decide whether a candidate is
worth publishing, publish low-frequency high-value posts from `@decodexspace` through
Chrome when the account state is safe, and write the `social_post/v1` publication
record.

This is a Decodex repository-development instruction surface, not an installable
Decodex plugin skill.

## Read Before Publishing

- `docs/spec/social-publishing.md`
- `docs/spec/upstream-impact.md`
- `docs/runbook/social-publishing-workflow.md`
- `dev/skills/x-post-quality-system/SKILL.md`
- `dev/skills/codex-release-analysis/SKILL.md`
- `dev/skills/codex-code-analysis/SKILL.md`

## Inputs

- `signal_entry/v1`, `upstream_impact/v1`, `upstream_review/v1`, release-analysis
  note, or checked source URLs
- Optional style observations from `@CodexReleases`, `@Codex_Changelog`, `@LLMJunky`,
  or `@decodexspace`
- Target account: `decodexspace`
- Controller account for attribution and site links: `hackink`

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

## Publish Modes

Choose exactly one `mode` from `social_post/v1`:

- `release_pulse`: short release-aware summary with source link.
- `release_rollup`: release or prerelease summary built from accumulated Radar
  analysis.
- `practical_explainer`: concrete user workflow and expected result.
- `operator_impact`: Decodex Control Plane implication.
- `thread`: multi-post explanation when one post hides evidence or caveats.
- `watch_note`: cautious public note for incomplete evidence.

`@decodexspace` should mostly use `practical_explainer`, `operator_impact`, and
source-backed `release_rollup`. Use `release_pulse` only when the release itself is the
useful alert.

## Worthiness Gate

Publish only when all are true:

- The post is in English.
- The source evidence is enough for every technical claim.
- `dev/skills/x-post-quality-system/SKILL.md` passes the candidate as externally
  valuable.
- The candidate answers in one screen: what changed, who can use or observe it, and what
  source proves it.
- The item is `critical` or `high`, or it is a release/prerelease rollup with clear
  reader value.
- The post is useful to Codex users, Decodex operators, or builders tracking the Codex
  app-server ecosystem.
- The idempotency key has not already been published or blocked for the same source.
- The daily cap has not been reached.

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

Generate an image for each published post only when useful media can pass
`dev/skills/x-post-quality-system/SKILL.md`. Use
`image_template = "decodex_signal_card"` as a visual system, not as a fixed reusable
asset. Start with the exact base prompt in `docs/spec/social-publishing.md`, then add
candidate-specific subject, source, visual metaphor, palette, and forbidden-elements
slots from the quality-system skill.

Do not rely on the generated image for readable text. Render any title, source, date,
PR number, release tag, or mode with deterministic overlay tooling or keep it in the
post body. Never reuse prior live-test images, old generic signal-card assets, unrelated
abstract cards, or weak decorative filler.

If no fresh candidate-specific image passes visual review, publish text-only only when
the post remains valuable with the source link card. Otherwise skip or fail closed.

## Claim Review

Before publishing:

- Map every sentence to evidence.
- Remove claims based only on social posts or engagement.
- Make beta, rollout, platform, and config gates explicit.
- Avoid local paths, credentials, private issue details, or internal runtime state.
- Keep each `text[]` item within the X length limit.

## Output

Write `artifacts/social/x/posts/<yyyy-mm-dd>/<slug>.json` with:

- `schema = "social_post/v1"`
- `channel = "x"`
- `target_account = "decodexspace"`
- `controller_account = "hackink"`
- `status = "published"`, `blocked`, `failed`, or `skipped`
- `source_refs`, `evidence_notes`, `claims`, and `decision`
- `publication` when posted
- `block`, `failure`, or `skip` when not posted

Run:

```bash
decodex radar validate artifacts/social/x
```

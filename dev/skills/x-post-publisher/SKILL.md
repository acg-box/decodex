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
- Use Chrome only for this low-frequency publishing workflow.
- Before posting, verify the logged-in X account is `@decodexspace`.
- If account state, X compose/readback, upload, duplicate detection, or final URL
  readback is unreliable, do not post; write `blocked` or `failed`.
- Treat X style observations as format input only. Technical claims must come from
  GitHub, changelog, signal, upstream-review, upstream-impact, release-delta, or
  candidate evidence.

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
- The item is `critical` or `high`, or it is a release/prerelease rollup, intro, or
  watch note with clear reader value.
- The idempotency key has not already been published or blocked for the same source.
- The daily cap of 8 posts for `@decodexspace` in `Asia/Shanghai` is not reached.

Skip low-value internal churn. Do not publish a single-PR rename, trace-only note,
low-context operator caution, or internal audit reminder unless it rolls up into a
broader release/update story or concrete external operator decision.

## Image Generation

Generate media only when a fresh candidate-specific image can pass quality review.
Never rely on generated readable text or reuse prior live-test, generic, or unrelated
media. Keep generated image files in `$CODEX_HOME/decodex/social-media/` or temporary
storage by default, not Git. After upload, record the X status URL, any `/photo/N`
readback URL, and a short prompt/hash note when useful. If text-only is still valuable
with the source link card, publish text-only; otherwise skip or fail closed.

## Claim Review

Before publishing:

- Map every sentence to evidence.
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

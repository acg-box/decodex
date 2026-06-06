---
name: x-post-publisher
description: Use when publishing a checked `social_candidate/v1` or explicit operator handoff to @decodexspace, or writing the matching `social_post/v1` non-published record.
---

# Decodex X Post Publisher

Use after a `social_candidate/v1` with `decision.worthiness = "publish"` exists, or
after an explicit operator handoff names checked Radar artifacts. This skill consumes
the candidate or handoff, optionally posts through Chrome as `@decodexspace`, and
always writes the terminal `social_post/v1` record.

## Read First

- `docs/spec/social-candidate.md`
- `docs/spec/social-publishing.md`
- `docs/runbook/social-publishing-workflow.md`
- `../references/social-release-publisher-gates.md`
- `dev/skills/x-post-quality-system/SKILL.md`

## Hard Boundaries

- Do not read upstream Codex source, patches, PR files, or release-window gaps here.
- Do not create fresh analysis; consume checked artifacts only.
- Technical claims must come from GitHub, changelog, signal, upstream-review,
  upstream-impact, release-delta, or candidate evidence.
- Historical X style observations are format input only. Never infer claims, urgency, or
  publish/skip state from other accounts.
- Do not publish candidates with `decision.worthiness = "defer"` or `"skip"` unless the
  operator gives a fresh explicit handoff.

## Publish Gate

Publish only when all are true:

- candidate or handoff is source-backed and externally valuable
- `x-post-quality-system` passes the post and any media
- every sentence maps to evidence
- important PRs have direct GitHub PR URLs on first public mention, unless one exact
  compare URL intentionally carries the detailed PR list
- prerelease channel lineage and previous-post quote state were checked through
  `post_lifecycle.quote_eligible`
- idempotency key has not already been published or blocked for the same source
- checked-in records, open publication PRs when available, and live `@decodexspace`
  profile/timeline readback show no matching post for the candidate's exact lead text,
  idempotency subject, release tag, or source URL
- daily cap of 8 posts for `@decodexspace` in `Asia/Shanghai` is not reached

For release/prerelease/app candidates, apply
`../references/social-release-publisher-gates.md` before composing.

Do not treat X search `No results` as sufficient duplicate evidence. Use search only
as a supporting signal; if profile/timeline readback is unavailable, stale,
loading-only, or contradicts search, fail closed before composing.

## Chrome And Media

Use `@Chrome` only inside this low-frequency Publisher workflow. Before composing,
verify the logged-in account is `@decodexspace`.

Fail closed if Chrome availability, account verification, X page structure, duplicate
detection, media upload, final URL readback, or account/media readback is unreliable.
Do not downgrade to text-only unless the operator explicitly approves that fallback for
the current candidate.

Use generated media only when it is fresh, candidate-specific, and quality-checked.
Keep generated image files in `$CODEX_HOME/decodex/social-media/` or temporary storage
by default, not Git.

Close or release scoped Chrome tabs before ending. Keep a tab only as an explicit human
handoff such as login, CAPTCHA, account approval, or upload permission repair.

## Output Record

Write `artifacts/social/x/posts/<yyyy-mm-dd>/<slug>.json` with:

- `schema = "social_post/v1"`
- `channel = "x"`
- `target_account = "decodexspace"`
- `controller_account = "hackink"`
- `status = "published"`, `blocked`, `failed`, or `skipped`
- `source_refs`, `evidence_notes`, `claims`, `decision`, and publication/block/failure
  details as applicable
- X status/media URL or media caveat when media was used or skipped
- `post_lifecycle` when the record can affect future prerelease quote chains

Run:

```bash
decodex radar validate artifacts/github/social-candidates artifacts/social/x
```

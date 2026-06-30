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
- `automations/decodex/skills/x-post-quality-system/SKILL.md`

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
- durable records, active `social_publish_reservation/v1` records, and live
  `@decodexspace` profile/timeline readback
  show no matching post for the candidate's exact lead text, idempotency subject,
  release tag, or source URL
- an active `social_publish_reservation/v1` with the same idempotency key and
  duplicate keys has been created by `radar social reserve-publish` under
  `.agent/automations/decodex/cache/social/x/reservations/<yyyy-mm-dd>/` before X
  compose
- daily cap of 8 posts for `@decodexspace` in `Asia/Shanghai` is not reached

For release/prerelease/app candidates, apply
`../references/social-release-publisher-gates.md` before composing.

Do not treat X search `No results` as sufficient duplicate evidence. Use search only
as a supporting signal; if profile/timeline readback is unavailable, stale,
loading-only, or contradicts search, fail closed before composing.

Do not hand-write active reservation JSON. Do not compose from a missing, temporary,
expired, unvalidated, or non-command-created reservation. After the reservation is
durable and immediately before clicking Post, repeat live profile/timeline duplicate
readback. If a duplicate appears, cancel or expire the reservation and do not publish.

Keep the reservation record thin. It should hold only the publish lease, owner,
candidate/source refs, idempotency key, duplicate keys, cap-day inputs, and concise
pre-compose duplicate/account-readiness notes. Do not copy final post text, final
timeline/permalink readback, publication URLs, media readback, claims, caveats, or
`post_lifecycle` into `social_publish_reservation/v1`; write those to the terminal
`social_post/v1`.

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

Write `.agent/automations/decodex/cache/social/x/posts/<yyyy-mm-dd>/<slug>.json` with:

- `schema = "social_post/v1"`
- `channel = "x"`
- `target_account = "decodexspace"`
- `controller_account = "hackink"`
- `status = "published"`, `blocked`, `failed`, or `skipped`
- `source_refs`, `evidence_notes`, `claims`, `decision`, and publication/block/failure
  details as applicable
- X status/media URL or media caveat when media was used or skipped
- `post_lifecycle` when the record can affect future prerelease quote chains
- the consumed reservation path under `source_refs.reservations` when the compose gate
  was reached
- final profile/timeline/permalink readback evidence after the durable reservation
  gate and after publication, when available

Update `.agent/automations/decodex/cache/social/x/reservations/<yyyy-mm-dd>/<slug>.json` from `active` to
`consumed`, `canceled`, or `expired` before the run ends. Do not leave an active
reservation behind unless the thread is explicitly handed off to a human operator. When
marking a reservation `consumed`, add only `consumed_by_social_post` and any minimal
status/owner update needed; do not backfill final publication evidence into the
reservation.

Run:

```bash
radar validate .agent/automations/decodex/cache/github/social-candidates .agent/automations/decodex/cache/social/x
```

---
name: x-post-publisher
description: Use when publishing a checked `social_candidate/v1` or explicit operator handoff to @decodexspace, or writing the matching `social_post/v1` non-published record.
---

# Decodex X Post Publisher

Use after a `social_candidate/v1` with `decision.worthiness = "publish"` exists, or
after an explicit operator handoff names checked Radar artifacts. This skill consumes
the candidate or handoff, posts through Chrome as `@decodexspace`, and always writes
the terminal `social_post/v1` record.

## Read First

- `../references/social-release-publisher-gates.md`
- `automations/decodex/skills/x-post-quality-system/SKILL.md`

## Hard Boundaries

- Do not read upstream Codex source, patches, PR files, or release-window gaps here.
- Do not create fresh analysis; consume checked artifacts only.
- Technical claims must come from GitHub, changelog, signal, upstream-review,
  upstream-impact, release-delta, or candidate evidence.
- Historical X style observations are format input only. Never infer claims, urgency, or
  publish/skip state from other accounts.
- Do not publish candidates with `decision.worthiness = "skip"`. Terminalize the
  quality decision only with `decodex-publisher social terminalize-skip`. This command
  uses one deterministic create-only path and requires no browser lease.
- Do not use X MCP, X API, direct HTTP requests, browser cookie inspection, local
  storage inspection, or private browser profile files.
- Keep every generated social artifact, browser lease, and account-session record
  local under `.agent/automations/decodex/cache`. Never commit or archive them.

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
  duplicate keys has been created by `decodex-publisher social reserve-publish` under
  `.agent/automations/decodex/cache/social/x/reservations/` before X compose
- daily cap of 8 posts for `@decodexspace` in `Asia/Shanghai` is not reached

For release/prerelease/app candidates, apply
`../references/social-release-publisher-gates.md` before composing.

Do not treat X search `No results` as sufficient duplicate evidence. Use search only
as a supporting signal; if profile/timeline readback is unavailable, stale,
loading-only, or contradicts search, fail closed before composing.

Acquire the single X browser lease with
`decodex-publisher social acquire-browser-lease` before opening X. Keep its token only
in the current run. Renew and verify immediately before opening X and before every
later browser read, account switch, compose, public click, readback, or restoration.
After any interruption, renew and verify before touching X again. Do not hand-write
active reservation JSON. Do not compose from a missing, temporary, expired,
unvalidated, or non-command-created reservation. After the reservation is durable and
immediately before clicking Post, repeat live profile/timeline duplicate readback,
renew the exact lease with
`decodex-publisher social renew-browser-lease`, and run
`decodex-publisher social verify-browser-lease`. Renew again after the click and before
final publication readback, and renew before account restoration. If a duplicate
appears or pre-write renewal or verification fails, cancel or expire the reservation
and do not publish. A post-click lease failure requires one visible handoff tab; do not
continue with an unowned browser mutation.

Keep the reservation record thin. It should hold only the publish lease, owner,
candidate/source refs, idempotency key, duplicate keys, cap-day inputs, and concise
pre-compose duplicate/account-readiness notes. Do not copy final post text, final
timeline/permalink readback, publication URLs, media readback, claims, caveats, or
`post_lifecycle` into `social_publish_reservation/v1`; write those to the terminal
`social_post/v1`.

## Chrome And Media

Use `@Chrome` only inside this low-frequency Publisher workflow. Capture the initial
visible X account before changing account state. Accept only `@hackink` or
`@decodexspace`. When the initial account is `@hackink`, use the visible X account
switcher to select `@decodexspace`, then verify the target handle and profile before
composing.

Fail closed if Chrome availability, account switching, account verification, X page
structure, duplicate detection, media upload, final URL readback, or account/media
readback is unreliable.
Do not downgrade to text-only unless the operator explicitly approves that fallback for
the current candidate.

After every terminal outcome, restore `@hackink` when it was the initial account and
verify the restored handle. A restore failure after a confirmed publication does not
erase the publication URL, but it makes the run unhealthy and requires a visible
handoff. Record structured `browser_session` evidence in every published record.
Also set `browser_touched` and record top-level `browser_session` evidence for every
blocked, failed, or skipped result that touched X.

Use generated media only when it is fresh, candidate-specific, and quality-checked.
Keep generated image files under `.agent/automations/decodex/cache/social/media/` or
temporary storage, never Git.

Close or release scoped Chrome tabs before ending. Keep a tab only as an explicit human
handoff such as login, CAPTCHA, account approval, upload permission repair, or account
restoration.

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
- `browser_touched` for every terminal record
- top-level `browser_session` for every browser-touching terminal record
- `publication.publisher = "chrome"` for every published post

When no publishable candidate exists, collect at most one due 24-hour or seven-day
browser metric readback and write `social_outcome/v1`. Apply the same account switch,
renewal, verification, and restoration protocol. The 24-hour observation window is 23
to 48 hours after publication; the seven-day window is 167 to 192 hours. X API calls
and spend remain zero.

Update the exact reservation path returned by `reserve-publish` from `active` to
`consumed`, `canceled`, or `expired` before the run ends. The filename is derived from
the idempotency key, not the slug. Do not leave an active reservation behind unless the
thread is explicitly handed off to a human operator. When marking a reservation
`consumed`, add only `consumed_by_social_post` and any minimal status or owner update
needed. Do not backfill final publication evidence into the reservation.

After account restoration and terminal validation, release only the exact browser
lease with `decodex-publisher social release-browser-lease`. Do not persist or report
the lease token.

Run:

```bash
decodex-publisher validate-social
```

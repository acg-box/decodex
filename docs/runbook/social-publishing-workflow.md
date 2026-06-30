---
type: "Runbook"
title: "Social Publishing Workflow"
description: "Define the procedure for turning Radar evidence into guarded Decodex social publication records."
status: active
authority: procedural
owner: automation
tags: [runbook, publishing]
last_verified: 2026-06-25
---
# Social Publishing Workflow

Goal: Turn Radar evidence into low-frequency `@decodexspace` X posts or durable
blocked publication records without making the public site depend on a live Decodex
daemon.

Read this when:
- You are preparing X posts about Codex releases, PRs, app updates, or usage patterns.
- You need to decide whether a Decodex signal should also produce a social post.
- You are auditing a `social_post/v1` record after publication or a daily-cap block.

Inputs:
- Source evidence from GitHub, durable signal entries, upstream reviews,
  upstream-impact records, release-delta artifacts, social candidates, or verified
  browser observations.
- The governing schemas:
  - [`../spec/upstream-impact.md`](../spec/upstream-impact.md)
  - [`../spec/social-candidate.md`](../spec/social-candidate.md)
  - [`../spec/social-publishing.md`](../spec/social-publishing.md)
  - [`../spec/signal-entry.md`](../spec/signal-entry.md)

Depends on:
- [`local-github-signal-workflow.md`](./local-github-signal-workflow.md) for the
  GitHub signal path.
- [`../../automations/decodex/skills/x-post-publisher/SKILL.md`](../../automations/decodex/skills/x-post-publisher/SKILL.md)
  for the repo-local publishing method.
- [`../decisions/radar-control-plane-publisher.md`](../decisions/radar-control-plane-publisher.md)
  for the Radar, Control Plane, and Publisher boundary.
- [`../decisions/static-public-site.md`](../decisions/static-public-site.md) for the
  static-first public surface decision.

Outputs:
- An optional `upstream_impact/v1` artifact under `.agent/automations/radar/cache/github/impact/`.
- A `social_candidate/v1` record under `.agent/automations/decodex/cache/social/x/candidates/` when
  analysis needs a durable Publisher handoff or pre-publication decision.
- A `social_publish_reservation/v1` record under
  `.agent/automations/decodex/cache/social/x/reservations/<yyyy-mm-dd>/` before any X compose step.
- A `social_post/v1` record under `.agent/automations/decodex/cache/social/x/posts/<yyyy-mm-dd>/`.
- Optional local generated media under `$CODEX_HOME/decodex/social-media/`; generated
  media files are not committed by default.

## Style Benchmarks

These observations are historical tone and format inspiration only. Recurring
automation must not browse or sample these accounts, must not use their coverage as
source evidence, and must not decide publish/skip state from whether they posted.

| Account | Useful pattern | Decodex stance |
| --- | --- | --- |
| `@CodexReleases` | Fast release/update cards, media, and short thread splits for Codex app, mobile, and CLI updates. | Use as static style inspiration; do not let it become a runtime input or evidence source. |
| `@Codex_Changelog` | Fast release-aware bullets with a changelog link. | Useful for `release_pulse`, but Decodex should not become a duplicate release bot. |
| `@LLMJunky` | Practical user interpretation: how a feature changes real workflows, what is worth trying, and where limits remain. | Prefer this style when Radar evidence can support the claim quickly. |
| `@decodexspace` | Low-frequency automated publication channel. | Establish a voice around evidence-backed Codex intelligence and Decodex operator impact. |

Historical samples confirmed two useful shapes:

- `@Codex_Changelog` works as a single-card pattern: product/version headline, three
  dense bullets, and a source link. Use this only when the checkpoint itself is the
  reader value.
- `@CodexReleases` works as a thread pattern: lead card with highlights, focused
  follow-ups for fixed/added/availability/security areas, and a source tail. Use this
  when the release needs structure, but keep Decodex-specific caveats and evidence in
  the lead instead of burying them in the thread.

For current release/app automation, use official changelog, release, GitHub, durable
Radar artifacts, and prior `social_post/v1` records as the decision inputs. A missed
official Codex app or mobile changelog entry is a Publisher coverage failure, not an
upstream GitHub analysis gap and not something to decide from other accounts.

For current prereleases, Decodex's advantage is source-backed interpretation before a
stable release. Use compare and PR-title metadata to publish an early prerelease read
when the direction is useful, while labeling the post as alpha metadata interpretation
instead of a stable feature summary.

Keep release and prerelease channels separate:

- release channel: compare the current stable release to the previous stable release
- prerelease channel: compare the current prerelease to the previous prerelease in the
  same train
- first prerelease after a stable release: compare stable release to first prerelease,
  and do not quote a previous prerelease post
- later prereleases: quote the previous `@decodexspace` prerelease post when a previous
  post URL exists, so readers can follow the full prerelease history before stable
  release

For prerelease reads, do not write a single generic theme paragraph. First group the
compare window into reader-facing buckets:

- important PR/commit clusters
- anticipated user workflow changes
- protocol/API/schema changes
- removals, deprecations, or compatibility boundaries
- plugin, config, sandbox, image/tool, or release-engineering changes

Use those buckets to shape a short thread with line breaks and compact bullets.
Put direct GitHub PR URLs on the first public mention of important PRs. Use raw PR
numbers only as secondary shorthand after readers already have a clickable URL or when
one exact compare URL intentionally carries the detailed PR list.

## Workflow

1. Start from source evidence.
   - Prefer a source-backed `upstream_review/v1`, merged PR bundle, release-delta
     compare entry, already-rendered `signal_entry/v1`, or `upstream_impact/v1`.
   - For continuous Radar-derived release or prerelease work, treat
     `upstream_impact/v1` as the shared handoff from Radar Review into Publisher.
     Use release-delta and compare metadata to identify checkpoints and gaps, but do
     not duplicate upstream source analysis inside Publisher.
   - For Codex app and mobile updates, the official Codex changelog is source
     evidence. Use GitHub only for repository behavior claims.
   - Do not start from social engagement alone.
   - Publisher automation is an artifact consumer. It must not perform fresh upstream
     Codex source analysis; if the durable artifacts do not support the claim, keep the
     candidate at `decision.worthiness = "defer"` or `"skip"`, or write a terminal
     `skipped` or `blocked` `social_post/v1` when the Publisher flow has already
     started.

2. Classify upstream impact.
   - Prefer existing `upstream_impact/v1` under
     `.agent/automations/radar/cache/github/impact/<slug>.json`.
   - Publisher must not write or update `upstream_impact/v1`. If the source-backed
     review does not already provide the needed shared handoff, route back to Radar
     Review or stop with `decision.worthiness = "defer"`.
   - If impact depends on unreviewed code or patch evidence, route back to the upstream
     analysis stage instead of resolving it inside Publisher.
   - Use `public_signal_decision`, `control_plane_impact`, and `publisher_angle` from
     [`../spec/upstream-impact.md`](../spec/upstream-impact.md).

3. Decide whether to create or consume a candidate.
   - Decodex Publisher should write or consume `social_candidate/v1` with
     `decision.worthiness = "publish"`, `"defer"`, or `"skip"`.
   - New Publisher candidates derived from Radar handoff evidence should cite the shared
     `upstream_impact/v1` under
     `source_refs.upstream_impacts` so Publisher and Control Plane use the same
     upstream scan conclusion.
   - General Publisher automation should consume only candidates whose
     `decision.worthiness = "publish"`. It must not turn `defer` or `skip` decisions
     into posts.

4. Decide whether to publish.
   - Publish only when the change has a clear `release_pulse`, `practical_explainer`,
     `release_rollup`, `operator_impact`, or valuable `watch_note` angle.
   - For `social_candidate/v1`, use `decision.worthiness = "publish"` as the
     schema-defined handoff signal. Do not require non-schema `status` or
     `decision.outcome` fields.
   - For prerelease candidates, require scan-friendly text that names concrete
     PRs/commits or source-backed buckets. Treat dense one-paragraph drafts as
     quality failures even when the technical claims are source-backed.
   - For prerelease candidates, verify adjacent channel lineage and quote state before
     composing: previous checkpoint, current checkpoint, compare URL, first-prerelease
     status, and previous prerelease post URL when available.
   - Skip when the change is internal cleanup, too weakly sourced, too private, too
     vague, or not useful enough for a reader.

5. Check idempotency and daily cap.
   - Build a stable idempotency key from account, source, mode, and release checkpoint
     when applicable.
   - Count already-published `@decodexspace` records for the cap day from durable
     `.agent/automations/decodex/cache/social/x/posts` `social_post/v1` records.
   - Check active `social_publish_reservation/v1` records in
     `.agent/automations/decodex/cache/social/x/reservations`. If another active reservation has the same
     idempotency key, source URL, exact lead text, release tag, or candidate slug,
     fail closed instead of composing.
   - Run live duplicate detection against the `@decodexspace` profile/timeline before
     composing. Match the candidate's exact lead text, idempotency subject, release
     tag, source URL, and known prior status URLs.
   - X search can be an additional signal, but `No results` is not sufficient proof
     that no duplicate exists. If X search and profile/timeline readback disagree, or
     either surface is loading-only or unreadable, fail closed.
   - The default cap day uses `Asia/Shanghai`.
   - If the candidate would exceed 8 posts, do not post. Write
     `status = "blocked"` with `block.reason = "daily_cap_exceeded"`.
   - Before opening the X composer, run `decodex-publisher social reserve-publish` with
     the idempotency key, duplicate keys, owner/run metadata, cap day, `reserved_at`,
     and `expires_at`. Do not hand-write active reservation JSON.
   - The command persists the reservation in
     `.agent/automations/decodex/cache/social/x/reservations/<yyyy-mm-dd>/<slug>.json`
     only after cap, active-reservation, terminal-post, idempotency, validation, and
     create-new checks pass. Missing, temporary, expired, unvalidated, or hand-written
     reservation state does not authorize publication.
   - After the durable reservation exists and immediately before clicking Post, repeat
     the live profile/timeline duplicate readback. If a duplicate appears, cancel or
     expire the reservation and do not publish.

6. Prepare media only when useful.
   - Use the `decodex_signal_card` image template in
     [`../spec/social-publishing.md`](../spec/social-publishing.md).
   - Do not rely on AI-generated text in the image.
   - Keep generated files in the local media cache or temporary storage, not Git.
   - It is acceptable to publish text-only when the post is useful with the source link
     card.

7. Publish through Chrome.
   - Verify Chrome is logged in as `@decodexspace`.
   - Compose the English post or thread.
   - Attach generated media when it is useful and available.
   - Fail closed if account verification, duplicate detection, media upload, or final
     URL readback is unreliable.
   - If X file upload stalls the Chrome control channel, stop after one failed upload
     path. Do not keep retrying and do not publish text-only unless the operator
     explicitly approves that fallback for the current candidate.
   - Close or release Chrome tabs before the automation ends. Keep a tab only when it
     is an explicit human handoff such as login, CAPTCHA, or account approval.

8. Write the publication record.
   - Use `schema = "social_post/v1"`.
   - Use `target_account = "decodexspace"` and `controller_account = "hackink"`.
   - Set `status = "published"`, `blocked`, `failed`, or `skipped`.
   - Include the consumed reservation under `source_refs.reservations` when the run
     reached the compose gate.
   - Preserve source refs, evidence notes, claims, decision data, and publication URLs
     when available.
   - For media, preserve the X status URL and any `/photo/N` readback URL. Do not add a
     generated image file to Git unless the operator explicitly asks for a permanent
     sample.
   - Update the reservation to `consumed` with `consumed_by_social_post` after a
     published, blocked, or otherwise terminal audited result. Use `canceled` or
     `expired` with `release_reason` when publication stops before a durable terminal
     post record is useful.

9. Validate.
   - Run:

```bash
decodex-publisher validate-social .agent/automations/decodex/cache/social/x
```

## Mode Guidance

For every new prerelease checkpoint, choose one outcome instead of silently skipping:
`release_pulse`, `watch_note`, `release_rollup`, or a `social_candidate/v1`
`decision.worthiness = "defer"` or `"skip"` with a durable reason. A prerelease intro
is useful when it names the tag, channel, published time, source, and what Decodex is
watching, while clearly avoiding claims that require unreviewed code or PR evidence.

Use `release_pulse` when:

- the release note itself is the story
- the post is mainly fast awareness
- the change does not yet justify a deeper Decodex angle
- a new prerelease is worth introducing from public release metadata, compare metadata,
  and explicit caveats, even before a full release rollup exists

Use `release_rollup` when:

- upstream publishes a release or prerelease
- Decodex already has commit/PR analysis, signals, or upstream-impact notes in that
  release window
- the post should summarize useful changes, Control Plane implications, deprecations,
  and watch-only gaps without pretending upstream release notes contain that detail

Use `practical_explainer` when:

- a reader can try the change in one short session
- the expected result is observable
- the value is easier to understand through workflow language than release bullets

Use `operator_impact` when:

- the change touches app-server, plugins, browser automation, MCP, permissions,
  sandboxing, config, or runtime orchestration
- Decodex Control Plane may need to adopt, watch, or guard against the change
- the public explanation can stay honest about what Decodex has and has not shipped

Use `watch_note` when:

- the change is interesting but evidence is incomplete
- rollout or platform status is unclear
- a strong recommendation would overclaim
- the prerelease checkpoint is visible but Radar still needs upstream analysis before
  it can describe behavior changes

## Guardrails

- Do not send credentials, private issue details, or local runtime paths to X.
- Do not publish without a source-backed worthiness decision.
- Do not exceed 8 posts per cap day for `@decodexspace`.
- Do not let Chrome automation keep retrying after a failed or uncertain publish.
- Do not leave research, compose, upload, search, or readback tabs open after the
  publication record has captured the outcome.
- Do not let social publishing bypass the static site, signal-entry, upstream-review,
  or upstream-impact evidence chain.
- Do not quote third-party posts at length. Record style observations, not copied
  content.

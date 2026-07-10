# Social Release Publisher Gates

Read this only when a Codex release, prerelease, app/mobile changelog entry, or
`social_candidate/v1` is being prepared or published for `@decodexspace`.

## Source Authority

- Use official OpenAI changelog, GitHub release metadata, GitHub compare metadata,
  PR-title metadata, and checked Radar artifacts as source evidence.
- `@CodexReleases` and `@Codex_Changelog` are historical style references only when
  supplied by the operator.
- Recurring automation must not browse or sample those accounts, must not treat their
  coverage as evidence, and must not decide publish/skip state from whether they posted.
- Do not infer implementation behavior from sparse release notes, tag names, or social
  style observations.

## Channel Lineage

- Stable release posts compare current stable release -> previous stable release in the
  same tag channel.
- Prerelease posts compare current prerelease -> previous prerelease in the same train.
- The first prerelease after a stable release compares stable release -> first
  prerelease and must not quote a prior prerelease post.
- Later prerelease posts should quote the previous live and quote-eligible
  `@decodexspace` prerelease post when a real previous-post URL exists.
- If no quote-eligible previous post exists, record that Decodex coverage gap and do not
  invent or reuse a deleted, failed, text-only test, blocked, skipped, or superseded URL.

## Candidate Gate

For every release or prerelease checkpoint, choose one explicit terminal outcome:

- `publish`: source-backed reader value is clear.
- `defer`: useful direction exists, but source analysis gaps remain.
- `skip`: the only supported fact is a version tag or low-value internal churn.
- `needs_upstream_analysis`: behavior claims require unreviewed PR/commit evidence.
- `no_op`: no new checkpoint or no public value.

For publishable prerelease candidates, evidence must name:

- previous checkpoint and current checkpoint
- adjacent compare URL
- first-prerelease-after-stable state
- previous prerelease post URL when quote-eligible, or a caveat when absent
- important PR/commit clusters, protocol/API changes, anticipated workflow changes, and
  remaining alpha or source-review caveats

## Copy Gate

- Public copy must be scan-friendly: short headline, blank line, compact bullets, then
  source/caveat.
- Do not publish dense one-paragraph prerelease reads when the compare window contains
  named PRs or commit titles.
- Important PRs should use direct GitHub PR URLs on first public mention. Raw `#12345`
  shorthand is secondary only after a URL is present or when one exact compare URL
  intentionally carries the detailed PR list.
- Do not imply broad availability for alpha, beta, rollout-gated, platform-gated, or
  config-gated changes.

## Media Gate

- Generate or attach media only when it adds reader value and is candidate-specific.
- Use `decodex_signal_card` as a visual system, not a reused fixed image.
- Never reuse live-test images, generic cards, unrelated abstract art, or media that
  depends on AI-rendered readable text.
- Generated images live in `$CODEX_HOME/decodex/social-media/` or temporary storage by
  default, not Git.
- Before composing any release, app, or prerelease post, check durable publication
  records, active `social_publish_reservation/v1` records, and the live
  `@decodexspace` profile/timeline for the exact lead text, release tag, source URL,
  and prior status URLs. X search `No results` is not a duplicate-clear signal by
  itself.
- Do not open X compose until an active `social_publish_reservation/v1` for the same
  idempotency key and duplicate keys is persisted in
  `.agent/automations/decodex/cache/social/x/reservations`. Repeat live profile/timeline duplicate readback
  immediately before clicking Post.
- If account verification, duplicate detection, media upload, or final readback is
  unreliable, fail closed. Do not downgrade to text-only unless the operator explicitly
  approves that fallback for the current candidate.

## Post Shapes

Use a concise release/update card when the official changelog or release is the story.
Use a prerelease-read thread when sparse prerelease notes plus compare metadata reveal
useful direction. Use `operator_impact` only for an external operator decision such as
enabling or avoiding a provider/config/path, updating an integration assumption, planning
a rollout, or watching a beta/availability boundary.

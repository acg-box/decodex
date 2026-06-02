# Social Publishing Workflow

Goal: Turn Radar evidence into low-frequency `@decodexspace` X posts or checked-in
blocked publication records without making the public site depend on a live Decodex
daemon.

Read this when:
- You are preparing X posts about Codex releases, PRs, app updates, or usage patterns.
- You need to decide whether a Decodex signal should also produce a social post.
- You are auditing a `social_post/v1` record after publication or a daily-cap block.

Inputs:
- Source evidence from GitHub, checked-in signal entries, upstream reviews,
  upstream-impact records, release-delta artifacts, or verified browser observations.
- The governing schemas:
  - [`../spec/upstream-impact.md`](../spec/upstream-impact.md)
  - [`../spec/social-publishing.md`](../spec/social-publishing.md)
  - [`../spec/signal-entry.md`](../spec/signal-entry.md)

Depends on:
- [`local-github-signal-workflow.md`](./local-github-signal-workflow.md) for the
  GitHub signal path.
- [`../../dev/skills/x-post-publisher/SKILL.md`](../../dev/skills/x-post-publisher/SKILL.md)
  for the repo-local publishing method.
- [`../decisions/radar-control-plane-publisher.md`](../decisions/radar-control-plane-publisher.md)
  for the Radar, Control Plane, and Publisher boundary.
- [`../decisions/static-public-site.md`](../decisions/static-public-site.md) for the
  static-first public surface decision.

Outputs:
- An optional `upstream_impact/v1` artifact under `artifacts/github/impact/`.
- A `social_post/v1` record under `artifacts/social/x/posts/<yyyy-mm-dd>/`.
- Optional generated media under `artifacts/social/x/images/`.

## Style Benchmarks

These benchmark observations are for tone and format only. They are not source evidence
for technical claims.

| Account | Useful pattern | Decodex stance |
| --- | --- | --- |
| `@Codex_Changelog` | Fast release-aware bullets with a changelog link. | Useful for `release_pulse`, but Decodex should not become a duplicate release bot. |
| `@LLMJunky` | Practical user interpretation: how a feature changes real workflows, what is worth trying, and where limits remain. | Prefer this style when Radar evidence can support the claim quickly. |
| `@decodexspace` | Low-frequency automated publication channel. | Establish a voice around evidence-backed Codex intelligence and Decodex operator impact. |

## Workflow

1. Start from source evidence.
   - Prefer a source-backed `upstream_review/v1`, merged PR bundle, release-delta
     compare entry, already-rendered `signal_entry/v1`, or `upstream_impact/v1`.
   - Do not start from social engagement alone.

2. Classify upstream impact.
   - Write or update `artifacts/github/impact/<slug>.json` when the change may affect
     Control Plane or Publisher.
   - Use `public_signal_decision`, `control_plane_impact`, and `publisher_angle` from
     [`../spec/upstream-impact.md`](../spec/upstream-impact.md).

3. Decide whether to publish.
   - Publish only when the change has a clear `release_pulse`, `practical_explainer`,
     `release_rollup`, `operator_impact`, or valuable `watch_note` angle.
   - Skip when the change is internal cleanup, too weakly sourced, too private, too
     vague, or not useful enough for a reader.

4. Check idempotency and daily cap.
   - Build a stable idempotency key from account, source, mode, and release checkpoint
     when applicable.
   - Count already-published `@decodexspace` records for the cap day.
   - The default cap day uses `Asia/Shanghai`.
   - If the candidate would exceed 8 posts, do not post. Write
     `status = "blocked"` with `block.reason = "daily_cap_exceeded"`.

5. Generate media.
   - Use the `decodex_signal_card` image template in
     [`../spec/social-publishing.md`](../spec/social-publishing.md).
   - Do not rely on AI-generated text in the image.
   - Attach media unless the record explains why media was skipped.

6. Publish through Chrome.
   - Verify Chrome is logged in as `@decodexspace`.
   - Compose the English post or thread.
   - Attach generated media when present.
   - Fail closed if account verification, duplicate detection, media upload, or final
     URL readback is unreliable.

7. Write the publication record.
   - Use `schema = "social_post/v1"`.
   - Use `target_account = "decodexspace"` and `controller_account = "hackink"`.
   - Set `status = "published"`, `blocked`, `failed`, or `skipped`.
   - Preserve source refs, evidence notes, claims, decision data, and publication URLs
     when available.

8. Validate.
   - Run:

```bash
decodex radar validate artifacts/social/x
```

## Mode Guidance

Use `release_pulse` when:

- the release note itself is the story
- the post is mainly fast awareness
- the change does not yet justify a deeper Decodex angle

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

## Guardrails

- Do not send credentials, private issue details, or local runtime paths to X.
- Do not publish without a source-backed worthiness decision.
- Do not exceed 8 posts per cap day for `@decodexspace`.
- Do not let Chrome automation keep retrying after a failed or uncertain publish.
- Do not let social publishing bypass the static site, signal-entry, upstream-review,
  or upstream-impact evidence chain.
- Do not quote third-party posts at length. Record style observations, not copied
  content.

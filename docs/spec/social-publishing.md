# Social Publishing

Purpose: Define the checked-in publication record used when Decodex publishes from the
`@decodexspace` X account or blocks a candidate that would exceed policy.

Status: normative

Read this when:
- You are generating, validating, or auditing Decodex Publisher output for X.
- You need to decide what evidence a post must carry before external publication.
- You are extending Publisher beyond static site signal entries.

Not this document:
- The upstream GitHub bundle schema. Read [`github-change-bundle.md`](./github-change-bundle.md).
- The public site signal-entry schema. Read [`signal-entry.md`](./signal-entry.md).
- The pre-publication handoff candidate. Read [`social-candidate.md`](./social-candidate.md).
- The social publishing procedure. Read
  [`../runbook/social-publishing-workflow.md`](../runbook/social-publishing-workflow.md).

Defines:
- The `social_post/v1` artifact shape.
- Allowed post modes for Decodex Publisher.
- The automated Chrome publishing boundary.
- The daily cap and blocked-publication ledger rule.
- The generated-media publishing and retention boundary.

## Artifact Identity

The canonical schema identifier is:

- `social_post/v1`

Recommended checked-in locations:

- `artifacts/social/x/posts/<yyyy-mm-dd>/<slug>.json`

`social_post/v1` is a publication record, not a review-only draft or pre-publication
candidate. Use `social_candidate/v1` for handoff decisions before Publisher evaluates
account state, idempotency, daily cap, media, and final publication.

Generated media files are not default Git artifacts. Store successful publication
facts in Git as small JSON records. Store generated image files in a local persistent
media cache, or discard them after upload, unless an operator explicitly asks to commit
an exact sample.

## Required Fields

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `social_post/v1`. |
| `slug` | string | Stable URL-safe identifier for the candidate. |
| `channel` | string | Must be `x`. |
| `target_account` | string | Must be `decodexspace` for the primary Publisher automation. |
| `controller_account` | string | Must be `hackink` for ownership attribution unless a later policy changes it. |
| `mode` | string | One value from the post-mode table. |
| `status` | string | `published`, `blocked`, `failed`, or `skipped`. |
| `audience` | string | Primary reader group. |
| `text` | array | One or more English post bodies, one array item per thread post. |
| `source_refs` | object | Links to signal, upstream-impact, upstream-review, release, PR, or changelog evidence. |
| `evidence_notes` | array | Non-empty list of evidence-backed notes that justify the post or skip decision. |
| `claims` | array | Non-empty list of user-facing claims with evidence references. |
| `decision` | object | AI worthiness, priority, idempotency key, daily counter, and cap decision. |

Optional fields:

- `publication`: required when `status = "published"`.
- `block`: required when `status = "blocked"`.
- `failure`: required when `status = "failed"`.
- `skip`: required when `status = "skipped"`.
- `caveats`: rollout limits, uncertainty, platform limits, or version gates.
- `media_refs`: optional X media readback URLs, external media pointers, content
  hashes, local generated assets, or explicitly operator-approved checked-in sample
  paths.
- `post_lifecycle`: current live/deleted/superseded state used to decide whether a
  previous post is eligible as a future quote target.

## Post Modes

Use exactly one `mode` value:

| Value | Purpose |
| --- | --- |
| `release_pulse` | Short release-aware summary with a source link. |
| `release_rollup` | Release or prerelease summary built from accumulated signal, upstream-impact, and commit/PR analysis. |
| `practical_explainer` | Concrete user-facing explanation of how to try or reason about a feature. |
| `operator_impact` | Decodex-specific explanation of app-server, plugin, browser, MCP, sandbox, config, or orchestration implications. |
| `thread` | Multi-post explanation when one post would hide important evidence or caveats. |
| `watch_note` | Cautious note for interesting changes that are not ready for a strong recommendation. |

`@decodexspace` should mostly use `practical_explainer`, `operator_impact`, and
evidence-backed `release_rollup`. `release_pulse` is allowed only when the release
itself is the useful alert.

For prerelease introductions, do not add a new mode. Use `release_pulse` when the
source-backed value is a timely prerelease alert, `watch_note` when the checkpoint is
worth tracking but release-window analysis is incomplete, and `release_rollup` only when
accumulated upstream reviews explain the useful changes.

## Claim Rules

Each `claims[]` entry must include:

- `text`: the claim visible or implied in the post.
- `evidence`: source reference key, URL, file path, or artifact path.
- `confidence`: `confirmed`, `likely`, or `weak`.

Rules:

- Do not publish a claim without evidence.
- Do not imply Decodex runtime support unless Control Plane evidence exists.
- Do not present a beta, hidden, or rollout-gated capability as generally available.
- Do not use a social post to replace the site signal or upstream-impact artifact.
- Do not quote third-party posts at length. Summarize style or public reaction unless
  the quoted text is short and necessary.
- Treat official OpenAI Codex changelog entries as evidence for app, mobile, and
  product-surface claims when the post stays within that changelog.
- Treat X benchmark accounts as historical format inspiration only. They are not
  coverage evidence, technical evidence, urgency signals, or publish/skip gates.
- For prerelease reads, map concrete bullets to PR numbers, commit titles, compare
  metadata, or release URLs. Do not publish a generic theme paragraph when the evidence
  can name important commits, anticipated features, protocol/API changes, removals, or
  operator-facing changes.
- Important PR references in public copy should be clickable on first mention. Prefer
  direct GitHub PR URLs over raw `#12345` shorthand unless the post relies on a single
  exact compare URL to cover many small PR references within X length limits.
- Keep release and prerelease channels separate. Stable release posts must compare the
  current stable release with the previous stable release. Prerelease posts must compare
  the current prerelease with the previous prerelease in the same train, except the
  first prerelease after a stable release, which compares against that stable baseline.
- For prerelease posts after the first checkpoint in a train, include the previous
  live, quote-eligible prerelease post URL in `source_refs.urls` when it exists and
  publish as a quote of that previous post. This keeps the prerelease history visible
  before the stable release ships. Deleted, superseded, failed, or text-only test posts
  must not become quote targets for the next prerelease.

## Decision Object

`decision` must contain:

| Field | Type | Notes |
| --- | --- | --- |
| `worthiness` | string | `publish`, `skip`, or `block`. |
| `priority` | string | `critical`, `high`, `normal`, or `low`. |
| `idempotency_key` | string | Stable key derived from account, source, mode, and checkpoint when applicable. |
| `reason` | string | Short source-backed reason for the decision. |
| `daily_limit` | number | Must be `8`. |
| `daily_count_before` | number | Number of posts already published for the target account on the selected day. |
| `daily_count_after` | number | Expected count after the action; unchanged for blocked, failed, or skipped records. |
| `day` | string | Calendar day used for cap accounting, formatted `YYYY-MM-DD`. |
| `timezone` | string | Default is `Asia/Shanghai`. |

The daily cap is hard. Automation must not publish the ninth `@decodexspace` post in
the same cap day. Instead it must write a `status = "blocked"` record with
`block.reason = "daily_cap_exceeded"`.

## Blocked Cap Records

When a candidate is blocked by the daily cap, the record must preserve the review
material needed for post-run analysis:

- source PR, commit, release, or signal references
- mode and priority
- AI worthiness reason
- candidate text
- intended media pointer or media caveat, if any
- `daily_count_before`
- `daily_limit`

The automation report must call out the block so the operator can inspect why the
candidate volume exceeded the cap.

## Publication Object

`publication` must contain:

| Field | Type | Notes |
| --- | --- | --- |
| `posted_at` | string | UTC timestamp. |
| `published_urls` | array | X URLs produced by the post or thread. |
| `publisher` | string | `chrome` or `x_api`. Primary policy is `chrome`. |
| `account_verified` | boolean | Must be true before publishing. |
| `made_with_ai` | boolean | Must be true when a generated image is attached. |
| `image_template` | string | Must be `decodex_signal_card` when media is attached. |

Chrome publication is allowed only for the low-frequency `@decodexspace` automation
described here. It must use the logged-in `@decodexspace` account, verify the account
before composing, and fail closed when Chrome, login state, X page structure, duplicate
detection, or media upload is unreliable.

Chrome tabs are temporary execution resources. Publisher automation must close or
release research, compose, upload, and readback tabs after the `social_post/v1` record
captures the result. A tab may stay open only as an explicit human handoff, such as
login, CAPTCHA, account approval, or a page that still requires operator input.

## Post Lifecycle

`post_lifecycle` records state that can change after the original publication or
publish attempt. It is optional for ordinary live posts, but required when a post is
deleted, superseded, failed after drafting, or otherwise must not become the next
prerelease quote target.

Fields:

| Field | Type | Notes |
| --- | --- | --- |
| `current_state` | string | `live`, `deleted_by_operator`, `superseded_published`, `superseded_text_only`, or `superseded_failed_attempt`. |
| `quote_eligible` | boolean | May be true only for live published posts that should be quoted by a later prerelease post. |
| `superseded_by_candidate` | string | Required for superseded states; points to the corrected candidate or replacement artifact. |
| `reason` | string | Short operator-readable reason for the lifecycle state. |

Deleted, failed, skipped, blocked, or superseded posts must set `quote_eligible =
false`. Publisher automation must follow this field when building prerelease quote
chains; it must not quote a post just because a previous `published_urls` entry exists.

## Generated Image Contract

Generated media is optional. Use it only when it adds reader value beyond the text and
source link card. Do not create or commit an image just to satisfy a default.

Use the stable image template id:

- `decodex_signal_card`

Use this base prompt for image generation:

```text
Create a refined abstract signal-card image for Decodex, a software control plane that
tracks Codex upstream changes. Style: precise, flat, premium technical poster; soft
off-white or near-black background depending on theme; thin neon magenta, lime, and
blue signal paths; sparse node graph; subtle grid; no mascots, no people, no logos
except a small Decodex wordmark area if provided by deterministic overlay. Leave clean
negative space for deterministic overlay text. Do not render long text in the image.
```

The AI image must not be trusted for text rendering. Render title, PR/tag, mode, and
source labels with deterministic overlay tooling or keep them in the post text.

When generated media is used, prefer this retention model:

- X is the durable public media host after publication.
- Git stores only the `social_post/v1` record, final X status URL, optional `/photo/N`
  readback URL, source refs, idempotency key, and media caveats.
- A local persistent media cache may store the generated image, prompt, content hash,
  and upload/readback notes for debugging or visual QA.
- Git should not store generated image files unless an operator explicitly requests a
  permanent sample.

Recommended local media-cache layout:

- `$CODEX_HOME/decodex/social-media/x/<yyyy-mm-dd>/<slug>/image.png`
- `$CODEX_HOME/decodex/social-media/x/<yyyy-mm-dd>/<slug>/manifest.json`

The manifest should stay local and may include prompt summary, generator, dimensions,
file size, sha256, X status URL, X media URL, and cleanup eligibility. Automation should
prune old cache entries according to operator policy; the cache is not source control.

## Release Checkpoints

Release and prerelease publishing is separate from continuous six-hour Radar review.
Release checkpoint automation may poll upstream releases more frequently than the
commit review loop, but it must record an explicit terminal outcome whenever a new
release, prerelease, app update, or changelog checkpoint appears.

Rollups must use prior `upstream_review/v1`, `upstream_impact/v1`, `signal_entry/v1`,
and compare evidence. Sparse Codex prerelease bodies are not sufficient proof for
feature claims.

However, a prerelease intro does not need to pretend to be a full rollup. A sparse
prerelease may still produce a `watch_note` when the post is useful as a timely
prerelease read and every claim is limited to source-backed release metadata, compare
metadata, PR-title metadata, exact source URLs, and explicit caveats. If the only fact
is the tag name with no reader value, automation should write a `social_candidate/v1`
with `decision.worthiness = "defer"` or `"skip"` instead of posting.

Official Codex app or mobile changelog entries may produce `release_pulse` posts when
the changelog itself contains concrete user-visible changes.

Release checkpoint automation should normally write `social_candidate/v1` first. X
Publisher consumes only candidates whose `decision.worthiness = "publish"` and writes
the terminal `social_post/v1` record.

Prerelease posts are incremental. They must record:

- previous checkpoint and current checkpoint
- compare URL for that adjacent pair
- whether this is the first prerelease after a stable release
- previous prerelease post URL when one exists, or a caveat when the prior checkpoint
  has no Decodex post to quote
- whether any previous post record is deleted, superseded, or otherwise ineligible as a
  quote target

Prerelease threads must stay scan-friendly:

- one idea per post
- blank line after the headline when the post has details
- compact bullets for PR/commit clusters, protocol/API changes, and caveats
- source URL in the final post or the post that makes the source-backed claim

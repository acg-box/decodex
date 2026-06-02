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
- The social publishing procedure. Read
  [`../runbook/social-publishing-workflow.md`](../runbook/social-publishing-workflow.md).

Defines:
- The `social_post/v1` artifact shape.
- Allowed post modes for Decodex Publisher.
- The automated Chrome publishing boundary.
- The daily cap and blocked-publication ledger rule.
- The generated-image style contract.

## Artifact Identity

The canonical schema identifier is:

- `social_post/v1`

Recommended checked-in locations:

- `artifacts/social/x/posts/<yyyy-mm-dd>/<slug>.json`
- `artifacts/social/x/images/<slug>.png`

`social_post/v1` is a publication record, not a review-only draft. The record is
written whether automation publishes the post, skips it, fails safely, or blocks it
because the daily cap has already been reached.

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
- `media_refs`: checked-in or locally generated assets used by the post.

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
- generated image prompt or intended media refs, if any
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

## Generated Image Contract

Every published X post should include a generated image unless the publication record
explains why media was skipped.

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

## Release Checkpoints

Release and prerelease publishing is separate from continuous six-hour Radar review.
Release checkpoint automation may poll upstream releases more frequently than the
commit review loop, but it must publish only when a new release or prerelease checkpoint
appears and enough accumulated review evidence exists.

Rollups must use prior `upstream_review/v1`, `upstream_impact/v1`, `signal_entry/v1`,
and compare evidence. Sparse Codex prerelease bodies are not sufficient proof.

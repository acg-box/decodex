# Reset Status

Purpose: Define the artifact and homepage behavior for the reset-status widget.

Status: normative

Read this when:
- You are changing the homepage reset-status widget.
- You are changing how Decodex decides whether "Rate limit reset today?" is `Yes`,
  `No`, or `Unknown`.
- You are producing or reviewing the reset-status artifact.

Not this document:
- The general social publishing workflow.
- The Codex upstream signal-entry contract.

Defines:
- The X account used as the reset-status source.
- The `reset_status/v1` artifact consumed by the static site.
- The AI semantic judgment rule for reset status.
- The publication boundary that turns a changed artifact into homepage content.

## Source

The reset-status source is the X account:

- account: `@thsottiaux`
- profile URL: `https://x.com/thsottiaux`

The static site must not fetch X, OpenAI Status, or another live API from the browser for
this widget. The site reads the latest checked-in reset-status artifact under:

- `site/src/content/reset-status/*.json`

## Artifact

The artifact schema is:

- `schema`: must be `reset_status/v1`
- `question`: must be the rendered question, currently `Rate limit reset today?`
- `answer`: one of `yes`, `no`, or `unknown`
- `confidence`: one of `confirmed`, `likely`, or `weak`
- `observed_for_date`: the date being judged, formatted as `YYYY-MM-DD`
- `timezone`: the timezone used to interpret "today"
- `generated_at`: generation timestamp
- `source_account`: the X handle that was reviewed
- `source_url`: the reviewed profile URL
- `search_url`: optional X search URL used during review
- `judgment_mode`: must be `ai_semantic_review`
- `rationale`: short explanation for the decision
- `evidence_posts`: reviewed candidate posts or summaries, each marked as `related`,
  `not_related`, or `uncertain`

## Judgment Rule

The reset answer is an AI semantic judgment over today's visible `@thsottiaux` posts.
It must not be a keyword-only check.

Set `answer` to `yes` when at least one reviewed post, quote, or directly linked context
semantically says that rate limits reset, message caps recovered, quota windows reset, or
users should wait for a reset window. The exact phrase `rate limit reset` is not required.

Set `answer` to `no` when today's reviewed posts are sufficiently visible and none of
them semantically indicates a rate-limit reset event.

Set `answer` to `unknown` when X access, search, login state, timeline loading, or post
visibility prevents a useful judgment.

## Review Rule

The reviewer must collect today's candidate posts first, then make the semantic decision.
Good evidence can come from X profile results, X search results, quoted context, or visible
thread context.

Chrome/X tabs used for search, profile review, or thread context are temporary
observation resources. Close or release them after writing the artifact. Keep a tab
open only when the run must hand off a login, CAPTCHA, or other human-only X state to
the operator.

Do not mark `yes` from generic words such as `reset`, `limit`, `fast`, `quota`, or
`rate` unless the surrounding context is about rate-limit reset behavior.

Do not mark `yes` from unrelated release, browser, performance, or product-quality posts.

## Homepage Rule

The homepage renders the checked-in artifact directly:

- `yes` renders `Yes` in the positive tone
- `no` renders `No` in the neutral tone
- `unknown` renders `Unknown` in the muted tone

The widget links to the artifact's `search_url` when present, otherwise to `source_url`.

## Publication Rule

The reset-status run is a content refresh, not only a watcher report. When semantic
review changes the artifact and validation passes, the run should publish the checked-in
artifact through the repository's Git path so the static homepage deployment can render
the new answer.

Do not move the AI semantic judgment into GitHub Actions. The static site may deploy from
GitHub Actions after content is pushed, but Actions must not become the source of the
X-reading semantic decision.

If validation, Git authentication, X visibility, or another required step blocks
publication, the run must report the latest artifact state, validation state, and the
specific blocker instead of presenting the homepage content as updated.

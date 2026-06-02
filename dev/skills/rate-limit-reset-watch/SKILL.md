---
name: rate-limit-reset-watch
description: Use when checking whether today's @thsottiaux X posts semantically indicate a rate-limit reset, quota reset, message-cap recovery, or reset window, and when writing the Decodex homepage reset_status/v1 artifact under site/src/content/reset-status/.
---

# Rate Limit Reset Watch

Use this repo-local skill to refresh the homepage `Rate limit reset today?` signal.
The job is semantic judgment, not keyword matching.

## Inputs

- Source account: `https://x.com/thsottiaux`
- Artifact: `site/src/content/reset-status/latest.json`
- Spec: `docs/spec/reset-status.md`
- Default timezone for "today": `Asia/Shanghai`, unless the user gives another one.

## Workflow

1. Determine the `observed_for_date` in the selected timezone.
2. Collect today's visible `@thsottiaux` candidates from X.
   - Prefer `@Chrome` when logged-in X access is needed.
   - Use profile results and X search such as `from:thsottiaux since:YYYY-MM-DD until:YYYY-MM-DD`.
   - If exact keyword search is useful, use it only as supporting evidence, not as the decision.
3. Read the candidate posts, quotes, and immediate thread context needed to understand them.
4. Decide semantically whether any candidate says rate limits reset, quota windows reset,
   message caps recovered, or users should wait for a reset window.
5. Write `site/src/content/reset-status/latest.json` with `schema = "reset_status/v1"`.
6. Close or release Chrome/X tabs opened for search, profile review, or thread context.
   Keep a tab only when login, CAPTCHA, or another human-only X state must be handed off.
7. Run the site content/type validation after updating the artifact.

## Decision Rules

Return `yes` when the reviewed content semantically points to reset behavior, even if the
post never says the exact phrase `rate limit reset`.

Return `no` when today's visible posts were reviewable and none are about reset behavior.

Return `unknown` when X access, search, timeline loading, deleted posts, login state, or
insufficient visible candidates make the judgment unreliable.

Do not mark `yes` for generic use of words like `reset`, `limit`, `rate`, `quota`,
or `window` unless the surrounding meaning is rate-limit reset behavior.

Do not mark `yes` for unrelated posts about releases, performance, browser improvements,
model quality, screenshots, or product announcements.

## Artifact Guidance

Keep `rationale` short and evidence-based. Summarize reviewed posts instead of copying
long X text into the artifact.

Use `evidence_posts[].relevance` as:

- `related`: supports a `yes` judgment
- `not_related`: reviewed but does not support reset
- `uncertain`: could not be interpreted confidently

If the artifact is `unknown`, explain the blocker in `rationale` and include any partial
candidate summaries that were visible.

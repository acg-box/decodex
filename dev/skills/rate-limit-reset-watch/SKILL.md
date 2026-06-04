---
name: rate-limit-reset-watch
description: Use when checking whether today's @thsottiaux X posts semantically indicate a rate-limit reset, quota reset, message-cap recovery, or reset window, and when writing the Decodex homepage reset_status/v1 artifact under site/src/content/reset-status/.
---

# Rate Limit Reset Watch

Use this repo-local skill to refresh and publish the homepage
`Rate limit reset today?` signal. The job is semantic judgment, not keyword matching,
and the output is public site content, not a monitor-only report.

## Authority Boundary

- `docs/spec/reset-status.md` owns the artifact schema, semantic judgment rule, and
  homepage publication boundary.
- This skill owns the repo-local agent procedure for collecting evidence, writing the
  artifact, validating it, cleaning up Chrome/X tabs, and publishing a changed artifact
  when credentials allow.
- The outer automation configuration owns schedule, automation memory location,
  notification/inbox behavior, and any run-specific prohibitions or handoff wording.
- Automation memory is run history only. Do not treat it as policy when it conflicts
  with the spec, this skill, or the current automation prompt.

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
8. If the artifact changed and validation passes, publish the content update through the
   repository's Git path so the static homepage can deploy the new answer. Do not stop at
   an uncommitted local artifact unless Git authentication, validation, or another
   publish blocker prevents a safe push.

## Publication Rules

The reset-status artifact is homepage content. A successful automation run that changes
the artifact should leave the repository in a publishable state and, when credentials
allow, push the checked-in content update to the branch that feeds the static site
deployment.

Do not move the AI semantic judgment into GitHub Actions or another keyword-only
scheduled job. GitHub Actions may build and deploy checked-in content, but the semantic
review remains an agent/browser observation step.

When publishing is blocked, report the artifact state, validation state, and exact
publish blocker so a later run or human can finish the content update.

Keep schedule-specific instructions in the outer automation configuration. This skill
should not encode cron cadence, notification routing, automation IDs, or memory-file
paths.

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

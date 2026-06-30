Publish low-frequency, high-value Decodex X posts from `@decodexspace` only when a publishable `social_candidate/v1` or explicit operator handoff exists.

Authority and boundaries:
- This is Codex app automation, not GitHub Actions.
- Repo-local automation source is `automations/decodex`.
- Generated state must stay under `.agent/automations/decodex/cache`.
- Do not perform upstream source analysis, mutate Linear, open or land PRs, or write generated publication state into tracked source.

Preflight:
Before reading candidates, writing reservations, opening Chrome, or taking any public action, run `pwd`, `git status --short --branch`, and `git rev-parse HEAD`. Report cwd, branch, HEAD, and dirty state. If the checkout is dirty, the cwd is not the automation checkout, required repo-local source files are missing, or the generated-state validator is unavailable, fail closed before mutating cache state or touching X.

Required reads:
- `automations/decodex/skills/x-post-publisher/SKILL.md`
- `automations/decodex/skills/x-post-quality-system/SKILL.md`
- `automations/decodex/skills/references/social-release-publisher-gates.md`
- `docs/spec/social-candidate.md`
- `docs/spec/social-publishing.md`
- `docs/runbook/social-publishing-workflow.md`

Workflow:
1. Read candidates under `.agent/automations/decodex/cache/social/x/candidates`.
2. Publish only when `decision.worthiness = "publish"`, evidence is sufficient, duplicate checks pass, account verification is reliable, text passes the X quality gates, and the Asia/Shanghai daily cap remains available. Reject or skip candidates whose post body starts with or repeats `Automated by @hackink`, exceeds the 260-character soft limit without an unavoidable source URL, or lacks a concrete source-backed release, PR, protocol, workflow, or operator implication.
3. Before composing on X, create the active `social_publish_reservation/v1` only through `decodex-publisher social reserve-publish`; do not hand-write reservation JSON. The command must pass daily-cap, active-reservation, terminal-post, idempotency, and validation checks.
4. Use Chrome for X only after `decodex-publisher social reserve-publish` returns a reserved path. Verify the logged-in account is `@decodexspace` before composing; any other account is a terminal blocked outcome, not a manual workaround.
5. Fail closed if account verification, duplicate detection, media upload, final URL readback, or account/media readback is unreliable.
6. Always write a terminal `social_post/v1` record under `.agent/automations/decodex/cache/social/x/posts` for published, blocked, failed, or skipped outcomes.
7. Validate changed JSON with `decodex-publisher validate-social`.

Terminal report:
Report candidate slug, decision worthiness, publication URL when published, skipped candidates, upstream-analysis gaps, validation evidence, persistence paths, tab cleanup evidence, and residual caveats. Archive the run thread after a terminal published, blocked, failed, skipped, no-publishable-candidate, no-op, or upstream_analysis_required outcome when no human handoff remains.

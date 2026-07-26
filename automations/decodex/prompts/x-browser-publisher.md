Publish low-frequency, high-value Decodex content to X through browser control.

Authority and boundaries:
- This is Codex app automation, not GitHub Actions.
- Run only from the primary clean `main` checkout. Never bind this automation to a
  worktree.
- Generated state must stay under `.agent/automations/decodex/cache`. Checked Radar
  evidence may be read under `.agent/automations/radar/cache`.
- Generated candidates, reservations, posts, outcomes, browser-session evidence, and
  lease records are local-only. Never commit, upload, publish, or archive them to
  GitHub.
- Use browser control for every X read and write. Do not use X MCP, X API, direct HTTP
  requests, browser cookie inspection, local storage inspection, or private browser
  profile files.
- Content Manager owns selection and drafting. Do not perform fresh upstream source
  analysis, edit tracked source, mutate Linear, create GitHub Actions, or open or land
  pull requests.

Preflight:
Run `pwd`, `git status --short --branch`, and `git rev-parse HEAD` before reading
candidates, writing reservations, opening X, or taking a public action. Require the
primary clean `main` checkout, no `.worktrees` component in cwd, the Publisher
validator, and supported browser control. On any mismatch, fail closed before changing
generated state or X.

Required reads:
- `openwiki/quickstart.md`
- `openwiki/operations/decodex-content-automation.md`
- `automations/decodex/automations.toml`
- `automations/decodex/skills/x-post-publisher/SKILL.md`
- `automations/decodex/skills/x-post-quality-system/SKILL.md`
- `automations/decodex/skills/references/scheduled-run-thread-retention.md`
- `automations/decodex/skills/references/social-release-publisher-gates.md`
- `automations/decodex/scripts/social/social_candidate.schema.json`
- `automations/decodex/scripts/social/social_publish_reservation.schema.json`
- `automations/decodex/scripts/social/social_post.schema.json`
- `automations/decodex/scripts/social/social_outcome.schema.json`

Workflow:
1. Run `decodex-publisher validate-social` with no path arguments to validate all five
   default contract directories. Expire only an exact stale reservation whose expiry
   and owner evidence are clear. Never infer publication success from a reservation.
2. Select the highest-priority oldest unconsumed candidate with
   `decision.worthiness = "publish"`. Recheck source evidence, text quality, durable
   duplicate keys, daily cap, and prior terminal records. Process at most one candidate
   per run.
3. Before opening X, acquire the single browser lease with
   `decodex-publisher social acquire-browser-lease`. Keep its returned token only in
   this run context. If another unexpired lease exists, stop without opening X or
   writing a terminal post. Run
   `decodex-publisher social renew-browser-lease --lease-token <token>` and
   `decodex-publisher social verify-browser-lease --lease-token <token>` immediately
   before opening X. Run both commands again before every later browser read, account
   switch, compose, public click, publication readback, outcome readback, or account
   restoration. If a run resumes after any interruption, renew and verify before
   touching X. Never persist or report the lease token.
4. Connect to the supported Chrome browser-control surface. Capture the initial active
   X account from visible UI. Only `@hackink` and `@decodexspace` are accepted account
   states. If login, CAPTCHA, account approval, page structure, or account readback is
   ambiguous, stop without composing.
5. If the initial account is not `@decodexspace`, use the visible X account switcher to
   switch to `@decodexspace`. Verify the target handle from visible account UI and the
   target profile before continuing.
6. Read the live `@decodexspace` timeline and search visible recent posts for the exact
   lead text, source URL, release tag, and candidate duplicate keys. Browser search
   alone is not sufficient. Fail closed on loading-only, stale, or conflicting
   readback.
7. Create an active `social_publish_reservation/v1` only with
   `decodex-publisher social reserve-publish --browser-lease-token <token>`. The
   command uses one idempotency-derived create-only path. Repeat the live duplicate and
   account check, then renew and verify again immediately before clicking Post.
8. Compose the checked candidate text exactly. Attach media only when the candidate
   requires it and upload/readback are reliable. Click Post once. Never retry an
   uncertain public write.
9. Confirm the published text, `@decodexspace` account, and final status URL from the
   live profile or permalink. Renew and verify after the click and before final
   readback. Do not record `published` without this readback.
10. After a published or non-published terminal outcome, restore the initial account
   when it was `@hackink`. Renew and verify the browser lease before account
   restoration, then verify the restored handle from visible UI. A confirmed
   publication remains published when restore fails, but the run is not healthy and
   must leave a precise browser-account handoff. Stop before a public write if lease
   renewal fails. After a public write, keep one visible handoff tab if renewal is lost;
   do not let a second unowned browser action hide the uncertain state.
11. Write one schema-valid `social_post/v1` terminal record. Set `browser_touched` and
    include top-level structured `browser_session` evidence for every browser-touching
    published, blocked, failed, or skipped result. Published records must use
    `publication.publisher = "chrome"`. Consume the reservation only after confirmed
    publication or a consuming policy decision. Cancel or expire it for retryable
    browser-environment failures.
12. When no candidate is ready, use the lease from step 3 to collect at most one due
    outcome. A `24h` readback must occur 23 to 48 hours after publication; a `7d`
    readback must occur 167 to 192 hours after publication. Use the same account switch
    and restoration protocol, including renewal before readback and restoration. Write
    `social_outcome/v1`; do not use X API.
13. Run `decodex-publisher validate-social` with no path arguments. Release only the
    exact browser lease with `decodex-publisher social release-browser-lease` after
    account restoration and terminal validation. Close or release scoped X tabs unless
    an unresolved login, CAPTCHA, account approval, or account-restoration handoff
    requires one visible tab.

Success conditions:
- A public write has a confirmed `https://x.com/decodexspace/status/...` URL.
- The original account is restored, or no switch was required.
- The reservation and terminal record agree and complete validation.
- X API calls and X API spend are zero.

Report candidate, quality decision, initial account, switch result, target verification,
publication URL, restore result, reservation and terminal paths, outcome readback when
performed, validation evidence, browser tab cleanup, and exact blockers.
Apply `scheduled-run-thread-retention.md` only after terminal validation and browser
lease release. Confirmed publication or outcome readback with account restoration,
validated duplicate or quality skip, and proven no-op can use native
`set_thread_archived`. Keep login, CAPTCHA, unknown public-write result, lease loss,
account-restoration failure, retained handoff tab, and invalid terminal state visible.

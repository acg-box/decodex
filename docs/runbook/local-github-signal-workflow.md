# Local GitHub Signal Workflow

Goal: Define the repeatable workflow for collecting GitHub change bundles, running Codex analysis locally or on a trusted CI runner, validating signal entries, and publishing content to the site.

Read this when:
- You are preparing the first GitHub-backed Decodex signal entries.
- You need to know where AI analysis runs versus where CI runs.
- You are wiring scripts and content updates into a repeatable local flow.

Inputs:
- A repository to analyze
- Access to GitHub metadata needed to build a bundle
- The governing specs for repo layout, GitHub change bundles, and signal entries

Depends on:
- `docs/reference/workspace-layout.md`
- `docs/spec/github-change-bundle.md`
- `docs/spec/signal-entry.md`
- `docs/spec/release-delta.md`
- `docs/spec/site-contract.md`
- `dev/skills/README.md`

Outputs:
- A validated signal entry committed to the repo
- Optional upstream-impact and social-draft artifacts when the change affects Control
  Plane or external publishing
- A push that allows CI to build and deploy the static site

## Workflow

1. Triage upstream Codex activity with `dev/skills/codex-upstream-triage/` when the
   candidate is not already chosen by automation or by the operator.
2. Build a normalized GitHub change bundle under `artifacts/github/bundles/` for
   selected candidates.
3. Analyze source behavior with `dev/skills/codex-code-analysis/` as an in-session
   reasoning pass; do not create a separate checked-in artifact for this pass.
4. Use `dev/skills/codex-release-analysis/` when the source is a release, prerelease,
   app update, or changelog entry.
5. Run final signal drafting with `dev/skills/github-signal/` and save the
   `analysis_draft` JSON under `artifacts/github/analysis/`.
6. Render the resulting signal entry into `site/src/content/signals/`.
7. Validate the signal entry shape and collection consistency.
8. Classify upstream impact when the change may affect Control Plane or Publisher.
9. Regenerate the release-delta artifact so the homepage compares the latest stable release to the latest prerelease using the updated signal set.
10. Draft optional social publishing content only through
   [`social-publishing-workflow.md`](./social-publishing-workflow.md).
11. Review the rendered content manually in the homepage feed.
12. Push the content update and let CI build and deploy the static site.

## Deterministic commands

Build a PR-first bundle:

```bash
python3 scripts/github/build_change_bundle.py \
  --repo openai/codex \
  --pr 15222 \
  --out artifacts/github/bundles/openai-codex-pr-15222.json
```

Validate the bundle:

```bash
python3 scripts/github/validate_change_bundle.py \
  artifacts/github/bundles/openai-codex-pr-15222.json
```

Render a final signal entry from the reviewed bundle plus the local editorial
draft:

```bash
python3 scripts/github/render_signal_entry.py \
  --bundle artifacts/github/bundles/openai-codex-pr-15222.json \
  --analysis artifacts/github/analysis/openai-codex-pr-15222.analysis.json \
  --out site/src/content/signals/openai-codex-pr-15222.json
```

Validate the published signal entries and the site collection:

```bash
python3 scripts/github/validate_signal_entry.py site/src/content/signals
npm run build --prefix site
npm run check --prefix site
cargo make decodex-checks
```

Build the homepage release-delta artifact:

```bash
python3 scripts/github/build_release_delta.py \
  --repo openai/codex \
  --signals-dir site/src/content/signals \
  --out site/src/content/release-deltas/openai-codex-latest.json
```

The repository already includes a real sample for this flow:

- bundle: `artifacts/github/bundles/openai-codex-pr-15222.json`
- editorial draft: `artifacts/github/analysis/openai-codex-pr-15222.analysis.json`
- rendered signal: `site/src/content/signals/openai-codex-pr-15222.json`

Repo-local editorial instruction entrypoint:

- `dev/skills/README.md`

These entrypoints are for Decodex repository development only. They are incomplete as
general user-facing skills and must not be packaged with the installable Decodex
plugin. Today only `github_change_bundle/v1`, `analysis_draft`, `signal_entry/v1`,
`upstream_impact/v1`, `release_delta/v1`, and `social_post_draft/v1` are durable
content contracts for this workflow.

Automated hourly sync entrypoint:

- `scripts/github/sync_latest_signals.py`

## Editorial gate

Publish only when the change meets at least one of these tests:

- it introduces a new capability
- it changes user-visible behavior
- it offers a clear try-now path

Skip or defer entries for:

- pure refactors
- internal cleanup
- low-context changes with no safe user-facing interpretation

For the release-delta artifact:

- include only signals whose source commit SHAs appear in the stable-versus-prerelease compare set
- prefer highlighting the smaller tracked subset over trying to summarize every internal commit in the compare
- do not treat prerelease notes alone as sufficient editorial evidence when the release body is empty

For upstream-impact and social-draft artifacts:

- classify Control Plane implications before creating engineering follow-up work
- keep social drafts checked in and unposted until approval
- do not use X engagement as technical evidence

## CI boundary

The current Decodex boundary is:

- local Codex run: manual editorial review, batch backfills, and prompt iteration
- deterministic scripts: bundle fetch, Codex analysis execution, render, and validation
- trusted CI runner: hourly refresh of recent merged PRs plus normal site validation and commit/push of changed content

The hourly GitHub Actions path assumes:

- Codex CLI is installed on the runner
- a full `auth.json` payload is injected into `CODEX_HOME`
- `CODEX_AUTH_JSON` is treated as a sensitive secret and never logged
- `GITHUB_PAT_Y` is available when you want authenticated GitHub API requests for the routed `y` identity; otherwise the sync falls back to unauthenticated reads for public data
- `cargo make decodex-checks` remains the final gate before a content refresh commit

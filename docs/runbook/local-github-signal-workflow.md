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

Outputs:
- A validated signal entry committed to the repo
- A push that allows CI to build and deploy the static site

## Workflow

1. Build a normalized GitHub change bundle under `tools/github/bundles/`.
2. Review the bundle and decide whether the change is signal-worthy.
3. Run Codex analysis against the bundle with `plugins/decodex/skills/github-signal/` and save the editorial draft JSON.
4. Render the resulting signal entry into `site/src/content/signals/`.
5. Validate the signal entry shape and collection consistency.
6. Regenerate the release-delta artifact so the homepage compares the latest stable release to the latest prerelease using the updated signal set.
7. Review the rendered content manually in the homepage feed.
8. Push the content update and let CI build and deploy the static site.

## Deterministic commands

Build a PR-first bundle:

```bash
python3 tools/github/build_change_bundle.py \
  --repo openai/codex \
  --pr 15222 \
  --out tools/github/bundles/openai-codex-pr-15222.json
```

Validate the bundle:

```bash
python3 tools/github/validate_change_bundle.py \
  tools/github/bundles/openai-codex-pr-15222.json
```

Render a final signal entry from the reviewed bundle plus the local editorial
draft:

```bash
python3 tools/github/render_signal_entry.py \
  --bundle tools/github/bundles/openai-codex-pr-15222.json \
  --analysis tools/github/analysis/openai-codex-pr-15222.analysis.json \
  --out site/src/content/signals/openai-codex-pr-15222.json
```

Validate the published signal entries and the site collection:

```bash
python3 tools/github/validate_signal_entry.py site/src/content/signals
npm run build --prefix site
npm run check --prefix site
cargo make decodex-checks
```

Build the homepage release-delta artifact:

```bash
python3 tools/github/build_release_delta.py \
  --repo openai/codex \
  --signals-dir site/src/content/signals \
  --out site/src/content/release-deltas/openai-codex-latest.json
```

The repository already includes a real sample for this flow:

- bundle: `tools/github/bundles/openai-codex-pr-15222.json`
- editorial draft: `tools/github/analysis/openai-codex-pr-15222.analysis.json`
- rendered signal: `site/src/content/signals/openai-codex-pr-15222.json`

Repo-local skill entrypoint:

- `plugins/decodex/skills/github-signal/SKILL.md`

Automated hourly sync entrypoint:

- `tools/github/sync_latest_signals.py`

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

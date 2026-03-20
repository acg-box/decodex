# Local GitHub Signal Workflow

Goal: Define the repeatable local workflow for collecting GitHub change bundles, running Codex analysis locally, validating signal entries, and publishing content to the site.

Read this when:
- You are preparing the first GitHub-backed Decodex signal entries.
- You need to know where AI analysis runs versus where CI runs.
- You are wiring scripts and content updates into a repeatable local flow.

Inputs:
- A repository to analyze
- Access to GitHub metadata needed to build a bundle
- The governing specs for repo layout, GitHub change bundles, and signal entries

Depends on:
- `docs/spec/repo_layout.md`
- `docs/spec/github_change_bundle.md`
- `docs/spec/signal_entry.md`
- `docs/spec/release_delta.md`
- `docs/spec/site_contract.md`

Outputs:
- A validated signal entry committed to the repo
- A push that allows CI to build and deploy the static site

## Workflow

1. Build a normalized GitHub change bundle under `tools/github/bundles/`.
2. Review the bundle and decide whether the change is signal-worthy.
3. Run local Codex analysis against the bundle and save the editorial draft JSON.
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

The MVP boundary is:

- local Codex run: bundle review, AI analysis, and editorial approval
- local deterministic scripts: bundle fetch, render, and validation
- CI: build and deploy only

Do not run AI analysis in CI for the MVP.

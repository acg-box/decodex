# Local GitHub Signal Workflow

Goal: Define the repeatable workflow for collecting upstream Codex evidence, running
Codex analysis in automation or local sessions, validating signal entries, and
publishing content to the site.

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
- `docs/spec/upstream-review.md`
- `docs/spec/signal-entry.md`
- `docs/spec/release-delta.md`
- `docs/spec/radar-ledger.md`
- `docs/spec/site-contract.md`
- `dev/skills/README.md`

Outputs:
- A validated signal entry committed to the repo
- Optional upstream-impact and social publication artifacts when the change affects Control
  Plane or external publishing
- A push that allows CI to build and deploy the static site

## Workflow

1. Track upstream Codex commits continuously with `decodex radar refresh-upstream-queue`.
   Treat each commit as an evidence unit, resolve it back to a PR when possible, write
   `.decodex/radar.sqlite3`, and refresh `upstream_review_queue/v1`.
2. Let Codex automation consume queued subjects and run
   `dev/skills/codex-code-analysis/` for each source-backed review.
3. Build a normalized GitHub change bundle under `artifacts/github/bundles/` when the
   automation or operator needs full source context for a candidate.
4. Promote reviewed conclusions into `upstream_impact/v1` when the change may affect
   Control Plane, Publisher planning, compatibility, or adoption.
5. Use `dev/skills/codex-release-analysis/` when the source is a release, prerelease,
   app update, or changelog entry.
6. Run final signal drafting with `dev/skills/github-signal/` and save the
   `analysis_draft` JSON under `artifacts/github/analysis/`.
7. Render the resulting signal entry into `site/src/content/signals/` with
   `decodex radar render-signal`.
8. Validate the signal entry shape and collection consistency.
9. Classify upstream impact when the change may affect Control Plane or Publisher.
10. Regenerate the release-delta artifact so the homepage compares release windows
    using the updated signal set.
11. Publish optional social content or record a skip/block only through
   [`social-publishing-workflow.md`](./social-publishing-workflow.md).
12. When upstream publishes a release or prerelease, use `codex-release-analysis` to
    roll up the accumulated commit/PR analysis into a release summary or X post.
13. Review the rendered content manually in the homepage feed.
14. Push the content update and let CI build and deploy the static site.

## Deterministic commands

Build a PR-first bundle:

```bash
decodex radar bundle build \
  --repo openai/codex \
  --pr 22414 \
  --out artifacts/github/bundles/openai-codex-pr-22414.json
```

Validate the bundle:

```bash
decodex radar bundle validate \
  artifacts/github/bundles/openai-codex-pr-22414.json
```

Render a final signal entry from the reviewed bundle plus the Codex-owned
`analysis_draft`:

```bash
decodex radar render-signal \
  --bundle artifacts/github/bundles/openai-codex-pr-22414.json \
  --analysis artifacts/github/analysis/openai-codex-pr-22414.analysis.json \
  --out site/src/content/signals/openai-codex-pr-22414.json
```

Validate the published signal entries and the site collection:

```bash
decodex radar validate site/src/content/signals
npm run build --prefix site
npm run check --prefix site
cargo make decodex-checks
```

Build the homepage release-delta artifact:

```bash
cargo run -p decodex --bin decodex -- radar refresh-release-delta \
  --repo openai/codex \
  --signals-dir site/src/content/signals \
  --out site/src/content/release-deltas/openai-codex-latest.json
```

Preview unpublished PRs from a selected release compare range without generating
content:

```bash
decodex radar backfill-release-range \
  --repo openai/codex \
  --stable-tag rust-v0.130.0 \
  --preview-tag rust-v0.131.0-alpha.9 \
  --dry-run
```

Use release-range backfill to fill gaps in the accumulated commit/PR analysis before a
release or prerelease summary. It should supplement continuous commit tracking, not
replace it. Execute mode is still a Codex automation or local operator path: Rust
selects the release-window gaps and sequences deterministic Radar commands, while the
AI review step follows the repo-local skills and schemas instead of running inside
GitHub Actions.

The repository already includes a real sample for this flow:

- bundle: `artifacts/github/bundles/openai-codex-pr-22414.json`
- editorial draft: `artifacts/github/analysis/openai-codex-pr-22414.analysis.json`
- rendered signal: `site/src/content/signals/openai-codex-pr-22414.json`

Repo-local editorial instruction entrypoint:

- `dev/skills/README.md`

These entrypoints are for Decodex repository development only. They are incomplete as
general user-facing skills and must not be packaged with the installable Decodex
plugin. Today only `github_change_bundle/v1`, `analysis_draft`, `signal_entry/v1`,
`upstream_impact/v1`, `release_delta/v1`, and `social_post/v1` are durable
content contracts for this workflow.

Automated sync entrypoint:

- `decodex radar refresh-upstream-queue`

Bootstrap or inspect local historical trace:

```bash
decodex radar ledger ingest-existing
decodex radar ledger summary --json
```

## Editorial gate

Publish only when the change meets at least one of these tests:

- it introduces a new capability
- it changes user-visible behavior
- it offers a clear try-now path
- it explains deprecated, removed, legacy, or migration-relevant behavior

The homepage feed applies the same posture programmatically: low-impact internal
changes without a try path, capability value, or deprecated/migration cue stay out of
the public feed while remaining available to the ledger and release rollups.

Skip or defer entries for:

- pure refactors
- internal cleanup
- low-context changes with no safe user-facing interpretation

For the release-delta artifact:

- include only signals whose source commit SHAs appear in the stable-versus-prerelease compare set
- prefer highlighting the smaller tracked subset over trying to summarize every internal commit in the compare
- do not treat prerelease notes alone as sufficient editorial evidence when the release body is empty
- use release and prerelease publication time as a summary checkpoint over accumulated
  commit/PR analysis, not as the primary source of truth

For upstream-impact and social publishing artifacts:

- classify Control Plane implications before creating engineering follow-up work
- keep social publication, block, skip, and failure records checked in
- do not use X engagement as technical evidence

## CI boundary

The current Decodex boundary is:

- GitHub Actions: deterministic upstream commit discovery, PR mapping, review-queue
  refresh, release-delta refresh, validation, and commit/push of changed metadata.
  Actions must not run Codex AI analysis, create `analysis_draft`, or execute release
  backfills that cross that AI boundary.
- Codex automation: AI source review, compatibility judgment, Publisher judgment,
  social publication, `analysis_draft` creation, `decodex radar render-signal`, and
  any promotion into signal or follow-up artifacts.
- local operator sessions: manual editorial review, batch backfills, prompt iteration,
  `decodex radar backfill-release-range`, and public-content audit.

The GitHub Actions paths assume:

- `GITHUB_TOKEN: ${{ github.token }}` is exported by the workflow for authenticated
  GitHub API requests and current-repository pushes.
- Local operator runs may use the routed `GITHUB_PAT_Y` identity or pass an explicit
  `--token-env` override when they need a different credential.
- `cargo make decodex-checks` remains the final gate before a content refresh commit

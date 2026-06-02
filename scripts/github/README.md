# GitHub Script Helpers

This directory owns the remaining Python helper boundary for GitHub-backed Radar
analysis. Durable deterministic Radar workflows live in the Rust CLI.

Current helper:

- `run_codex_analysis.py` invokes Codex in a read-only session and writes a validated
  `analysis_draft` artifact. It is the explicit AI boundary and is not a GitHub
  Actions entrypoint.

Shared support:

- `contracts.py` supports the AI helper validation path.
- `analysis_draft.schema.json` is the Codex output schema for that helper.
- The remaining schema JSON files are checked contract references. They do not define
  the operator command path.

Rust CLI entrypoints:

- `decodex radar validate` validates checked Radar artifact JSON contracts from the
  Rust CLI.
- `decodex radar refresh-upstream-queue` refreshes
  `artifacts/github/review-queue/openai-codex-latest.json`.
- `decodex radar refresh-release-delta` refreshes
  `site/src/content/release-deltas/openai-codex-latest.json`.
- `decodex radar bundle build` builds deterministic bundles for PR-first and
  commit-only inputs.
- `decodex radar bundle validate` validates deterministic bundles.
- `decodex radar ledger ...` owns bootstrap, ingest, ingest-existing, artifact-link,
  and summary operations.
- `decodex radar render-signal` renders `signal_entry/v1` from a validated
  `github_change_bundle/v1` plus Codex-owned `analysis_draft`.
- `decodex radar backfill-release-range` selects release-window signal gaps and can
  sequence the remaining helper boundaries for local or Codex automation backfills.

Current checked contracts:

- `analysis_draft.schema.json` is the Codex AI helper output schema.
- `upstream_review_queue/v1` is validated by `decodex radar validate`.
- `upstream_review.schema.json` is validated by `decodex radar validate`.
- `release_delta/v1` is validated by `decodex radar validate`.
- `upstream_impact.schema.json` is validated by `decodex radar validate`.
- `social_post.schema.json` is validated by `decodex radar validate`.

Contract ownership:

- input bundle shape: `docs/spec/github-change-bundle.md`
- upstream review queue and AI review boundary: `docs/spec/upstream-review.md`
- output signal shape: `docs/spec/signal-entry.md`
- upstream impact shape: `docs/spec/upstream-impact.md`
- social publication shape: `docs/spec/social-publishing.md`

Example flow:

```bash
decodex radar bundle build \
  --repo openai/codex \
  --pr 22414 \
  --out artifacts/github/bundles/openai-codex-pr-22414.json

decodex radar render-signal \
  --bundle artifacts/github/bundles/openai-codex-pr-22414.json \
  --analysis artifacts/github/analysis/openai-codex-pr-22414.analysis.json \
  --out site/src/content/signals/openai-codex-pr-22414.json

decodex radar validate \
  site/src/content/signals/openai-codex-pr-22414.json
```

Continuous upstream Radar sync:

```bash
cargo run -p decodex --bin decodex -- radar refresh-upstream-queue \
  --repo openai/codex \
  --search-limit 40
```

The sync records every observed recent commit in the local SQLite Radar ledger and
writes `artifacts/github/review-queue/openai-codex-latest.json`. It does not install
Codex, make AI judgments, render public signals, or publish social posts.
If only `generated_at` would change, the command leaves the existing queue file intact
to avoid empty commits.

Release-delta refresh:

```bash
cargo run -p decodex --bin decodex -- radar refresh-release-delta \
  --repo openai/codex \
  --signals-dir site/src/content/signals \
  --out site/src/content/release-deltas/openai-codex-latest.json
```

The release-delta refresh compares the latest stable and prerelease tags, maps compare
commits back to published signal entries, and also leaves the existing file intact when
only `generated_at` would change.

Use `--no-ledger` only for throwaway runs. To bootstrap the ledger from existing
checked-in artifacts:

```bash
decodex radar ledger ingest-existing
```

Release-window gap fill:

```bash
decodex radar backfill-release-range \
  --repo openai/codex \
  --stable-tag rust-v0.130.0 \
  --preview-tag rust-v0.131.0-alpha.9 \
  --max-prs 3
```

GitHub Actions may refresh upstream queues, release deltas, and validation through
`decodex radar ...`. Codex automation owns AI review of queued subjects and may then
promote source-backed conclusions into `upstream_impact/v1`, `analysis_draft`,
`decodex radar render-signal` output, or `social_post/v1`.

Repo-local skills under `dev/skills/` are reasoning instructions for the Codex
analysis step and for manual Radar/Publisher work. They do not introduce extra
intermediate artifact schemas unless the conclusion is promoted into one of the
checked-in contracts listed above.

Raw bundles and analysis drafts are retained in Git for a 21-day hot window. Archive
older raw batches as dedicated `radar-archive-*` GitHub Release assets and commit only
the recovery manifest under `artifacts/archive/index/`. See
`docs/spec/radar-artifact-retention.md` and `docs/runbook/radar-artifact-archive.md`.

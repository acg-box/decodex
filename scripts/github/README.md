# GitHub Scripts

This directory owns deterministic GitHub-first Decodex scripts.

Current scripts:

- `build_change_bundle.py`
- `build_release_delta.py`
- `backfill_release_range.py`
- `radar_ledger.py`
- `run_codex_analysis.py`
- `sync_upstream_radar.py`
- `validate_change_bundle.py`
- `validate_upstream_review.py`
- `validate_social_post.py`
- `render_signal_entry.py`
- `validate_signal_entry.py`

Rust CLI foundation:

- `decodex radar validate` validates checked Radar artifact JSON contracts from the
  Rust CLI.
- `decodex radar render-signal` renders `signal_entry/v1` from a validated
  `github_change_bundle/v1` plus Codex-owned `analysis_draft`.
- `decodex radar backfill-release-range` selects release-window signal gaps and can
  sequence the remaining helper boundaries for local or Codex automation backfills.

The Python scripts remain compatibility and non-ported helper boundaries until cleanup
issues remove them. In particular, `run_codex_analysis.py` is the explicit Codex AI
boundary for creating `analysis_draft`; it is not a GitHub Actions entrypoint.

Current checked contracts:

- `analysis_draft.schema.json`
- `upstream_review_queue/v1` is validated by `contracts.py`
- `upstream_review.schema.json`
- `release_delta/v1` is validated by `contracts.py`
- `upstream_impact.schema.json`
- `social_post.schema.json`

Contract ownership:

- input bundle shape: `docs/spec/github-change-bundle.md`
- upstream review queue and AI review boundary: `docs/spec/upstream-review.md`
- output signal shape: `docs/spec/signal-entry.md`
- upstream impact shape: `docs/spec/upstream-impact.md`
- social publication shape: `docs/spec/social-publishing.md`

Example flow:

```bash
python3 scripts/github/build_change_bundle.py \
  --repo openai/codex \
  --pr 22414 \
  --out artifacts/github/bundles/openai-codex-pr-22414.json

decodex radar render-signal \
  --bundle artifacts/github/bundles/openai-codex-pr-22414.json \
  --analysis artifacts/github/analysis/openai-codex-pr-22414.analysis.json \
  --out site/src/content/signals/openai-codex-pr-22414.json

python3 scripts/github/validate_signal_entry.py \
  site/src/content/signals/openai-codex-pr-22414.json
```

Continuous upstream Radar sync:

```bash
python3 scripts/github/sync_upstream_radar.py \
  --repo openai/codex \
  --search-limit 40
```

The sync records every observed recent commit in the local SQLite Radar ledger and
writes `artifacts/github/review-queue/openai-codex-latest.json`. It does not install
Codex, make AI judgments, render public signals, or publish social posts.
If only `generated_at` would change, the script leaves the existing queue file intact
to avoid empty hourly commits.

Use `--no-ledger` only for throwaway runs. To bootstrap the ledger from existing
checked-in artifacts:

```bash
python3 scripts/github/radar_ledger.py ingest-existing
```

Release-window gap fill:

```bash
decodex radar backfill-release-range \
  --repo openai/codex \
  --stable-tag rust-v0.130.0 \
  --preview-tag rust-v0.131.0-alpha.9 \
  --max-prs 3
```

These scripts stay deterministic on purpose. GitHub Actions may refresh upstream
queues, release deltas, and validation. Codex automation owns AI review of queued
subjects and may then promote source-backed conclusions into `upstream_impact/v1`,
`analysis_draft`, `decodex radar render-signal` output, or `social_post/v1`.

Repo-local skills under `dev/skills/` are reasoning instructions for the Codex
analysis step and for manual Radar/Publisher work. They do not introduce extra
intermediate artifact schemas unless the conclusion is promoted into one of the
checked-in contracts listed above.

Raw bundles and analysis drafts are retained in Git for a 21-day hot window. Archive
older raw batches as dedicated `radar-archive-*` GitHub Release assets and commit only
the recovery manifest under `artifacts/archive/index/`. See
`docs/spec/radar-artifact-retention.md` and `docs/runbook/radar-artifact-archive.md`.

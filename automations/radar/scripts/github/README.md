# GitHub Script Helpers

This directory owns the remaining Python helper boundary for GitHub-backed Radar
analysis. Durable deterministic Radar workflows live in the Rust CLI.

Current helper:

- `run_codex_analysis.py` invokes Codex in a read-only session and emits one validated
  `analysis_draft` JSON document on standard output. It does not write the Radar
  cache. The Rust owner reads the bundle through the private-cache boundary, gives the
  helper a private temporary copy outside the cache, validates the returned JSON, and
  writes through the descriptor-relative cache owner. The helper is the explicit AI
  boundary and is not a GitHub Actions entrypoint. Direct invocation is a recovery or
  automation-only path and must pass `--allow-ai-analysis-boundary` or set
  `DECODEX_ALLOW_CODEX_ANALYSIS=1`.
  Normal operator workflows should reach it through Rust-owned `radar`
  commands such as `backfill-release-range`.

Shared support:

- `contracts.py` supports the AI helper validation path.
- `analysis_draft.schema.json` is the Codex output schema for that helper.
- `content_eligibility_report.schema.json` is the machine-readable output contract
  for the one-subject eligibility lineage receipt.
- The remaining schema JSON files are checked contract references. They do not define
  the operator command path.

Rust CLI entrypoints:

- `radar validate` validates checked Radar artifact JSON contracts from the
  Rust CLI.
- `radar refresh-upstream-queue` refreshes
  `.agent/automations/radar/cache/github/review-queue/openai-codex-latest.json`.
- `radar refresh-release-delta` refreshes
  `.agent/automations/radar/cache/site-content/release-deltas/openai-codex-latest.json`.
- `radar content-eligibility` validates one current queue/review/impact handoff before
  downstream content consideration.
- `radar bundle build` builds deterministic bundles for PR-first and
  commit-only inputs.
- `radar bundle validate` validates deterministic bundles.
- `radar ledger ...` owns bootstrap, ingest, ingest-existing, artifact-link,
  and summary operations.
- `radar render-signal` renders `signal_entry/v1` from a validated
  `github_change_bundle/v1` plus Codex-owned `analysis_draft`.
- `radar backfill-release-range` selects release-window signal gaps and can
  sequence the remaining helper boundaries for local or Codex automation backfills.

Current checked contracts:

- `analysis_draft.schema.json` is the Codex AI helper output schema.
- `upstream_review_queue/v1` artifacts are validated by `radar validate`.
- `upstream_review/v1` artifacts are validated by `radar validate`.
- `release_delta/v1` artifacts are validated by `radar validate`.
- `upstream_impact/v1` artifacts are validated by `radar validate`.
- `control_plane_upgrade_candidate/v1` artifacts are validated by
  `radar validate`.
Decodex Publisher validates `social_candidate/v1`,
`social_publish_reservation/v1`, and `social_post/v1` with
`decodex-publisher validate-social`.

Contract ownership:

- Radar artifact schemas and validation: `apps/radar/src/artifact_validation/`
- upstream review and analysis schemas: `automations/radar/scripts/github/upstream_review.schema.json`
  and `automations/radar/scripts/github/analysis_draft.schema.json`
- upstream impact schema: `automations/radar/scripts/github/upstream_impact.schema.json`
- Control Plane upgrade candidate schema:
  `automations/radar/scripts/github/control_plane_upgrade_candidate.schema.json`
- analysis workflow: `automations/radar/skills/codex-code-analysis/SKILL.md`

Example flow:

```bash
radar bundle build \
  --repo openai/codex \
  --pr 22414 \
  --out .agent/automations/radar/cache/github/bundles/openai-codex-pr-22414.json

radar render-signal \
  --bundle .agent/automations/radar/cache/github/bundles/openai-codex-pr-22414.json \
  --analysis .agent/automations/radar/cache/generated/analysis/openai-codex-pr-22414.analysis.json \
  --out .agent/automations/radar/cache/site-content/signals/openai-codex-pr-22414.json

radar validate \
  .agent/automations/radar/cache/site-content/signals/openai-codex-pr-22414.json
```

Continuous upstream Radar sync:

```bash
radar refresh-upstream-queue \
  --repo openai/codex \
  --search-limit 40
```

The command uses an explicit `--token-env` when provided and fails if that variable is
missing or empty. Without the flag, Radar uses the repository-routed GitHub identity,
then `GH_TOKEN`, then `GITHUB_TOKEN`. GitHub rate-limit failures include bounded
remaining, reset, and retry metadata. Radar does not retry a quota-exhausted response
in the same run.

The sync records every observed recent commit in the local SQLite Radar ledger and
writes `.agent/automations/radar/cache/github/review-queue/openai-codex-latest.json`. It does not install
Codex, make AI judgments, render public signals, or publish social posts.
The JSON report distinguishes material source/content change from an artifact write.
A successful freshness-only refresh rewrites `generated_at` and reports
`material_changed = false`, `written = true`, and the new `refreshed_at`.

Release-delta refresh:

```bash
radar refresh-release-delta \
  --repo openai/codex \
  --signals-dir .agent/automations/radar/cache/site-content/signals \
  --out .agent/automations/radar/cache/site-content/release-deltas/openai-codex-latest.json
```

The release-delta refresh compares the latest stable and prerelease tags, maps compare
commits back to published signal entries, and reports the same
`material_changed`, `written`, and `refreshed_at` fields. A freshness-only refresh is
therefore observable.

Use `--no-ledger` only for throwaway runs. To bootstrap the ledger from existing
checked-in artifacts:

```bash
radar ledger ingest-existing
```

Release-window gap fill:

```bash
radar backfill-release-range \
  --repo openai/codex \
  --stable-tag rust-v0.130.0 \
  --preview-tag rust-v0.131.0-alpha.9 \
  --max-prs 3
```

Codex app automation or an explicit local operator run may refresh upstream queues,
release deltas, and validation through `radar ...`. Codex automation owns AI
review of queued subjects and promotes Publisher or Control Plane conclusions into the
shared `upstream_impact/v1` handoff artifact before downstream
`control_plane_upgrade_candidate/v1`, `analysis_draft`,
or `radar render-signal` output consumes that same reviewed scan. Publisher
automation may later consume the shared handoff to write Publisher-owned social
records.

An `upstream_review/v1` must include the exact normalized queue `commit_shas` and
`upstream_head`. An `upstream_impact/v1` must include an RFC3339 `reviewed_at`
timestamp and `review_lineage` with the exact review artifact SHA-256, slug, subject
kind and id, upstream head, and commit set. Before content consideration, run the
bounded gate with exactly one review and impact:

```bash
radar content-eligibility \
  --queue .agent/automations/radar/cache/github/review-queue/openai-codex-latest.json \
  --review .agent/automations/radar/cache/github/reviews/openai-codex-pr-22414.json \
  --impact .agent/automations/radar/cache/github/impact/openai-codex-pr-22414.json \
  --max-age-hours 12
```

The review subject must exist in the queue and use the queue's exact normalized commit
set and upstream head. The review must request an `upstream_impact` action. The impact
must bind the exact review bytes and identity. Matching URLs or slugs do not satisfy
lineage by themselves. The impact must set `public_signal_decision = "publish"` and a
non-`none` Publisher angle. Missing, stale, deferred, or mismatched evidence fails
closed. The queue, review, and impact must all use one private Radar cache root or all
be external. A successful command emits the exact normalized commit set, upstream
head, queue/review/impact SHA-256 values, and `lineage_sha256`. The lineage digest uses
the `radar-content-eligibility-lineage-v1` domain, ordered named fields with unsigned
64-bit big-endian byte lengths, an unsigned 64-bit commit count, and ordered
`commit_sha` fields.

Do not wire `run_codex_analysis.py` into GitHub Actions. Actions must not pass
`--allow-ai-analysis-boundary` or set `DECODEX_ALLOW_CODEX_ANALYSIS`; that
acknowledgement is reserved for Rust-owned local automation and explicit operator
recovery runs that still keep bundle validation and `analysis_draft` schema validation
inside the helper.

Repo-local skills under `automations/radar/skills/` are reasoning instructions for the Codex
analysis step and for manual Radar work. Pre-publication and terminal social artifacts
are Decodex Publisher contracts, not Radar contracts.

Raw bundles, reviews, impacts, analysis drafts, and the ledger stay only in the
owner-only bounded local cache. Run `radar cache-gc` in every manager cycle. Do not
commit or upload them to GitHub.

All Radar cache readers, writers, ledger operations, and GC share one process lock and
use descriptor-relative, no-follow cache traversal. Cache GC reports only bounded
counts. Default validation includes that report.

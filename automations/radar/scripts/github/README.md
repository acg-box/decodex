# GitHub Script Helpers

This directory owns the remaining Python helper boundary for GitHub-backed Radar
analysis. Durable deterministic Radar workflows live in the Rust CLI.

Current helper:

- `run_codex_analysis.py` invokes Codex in a read-only session and writes a validated
  `analysis_draft` artifact. It is the explicit AI boundary and is not a GitHub
  Actions entrypoint. The wrapper behavior is deterministic: validate the bundle, run
  Codex with the checked prompt and output schema, validate the returned draft, then
  write the artifact. Direct invocation is a recovery or automation-only path and must
  pass `--allow-ai-analysis-boundary` or set `DECODEX_ALLOW_CODEX_ANALYSIS=1`.
  Normal operator workflows should reach it through Rust-owned `radar`
  commands such as `backfill-release-range`.

Shared support:

- `contracts.py` supports the AI helper validation path.
- `analysis_draft.schema.json` is the Codex output schema for that helper.
- The remaining schema JSON files are checked contract references. They do not define
  the operator command path.

Rust CLI entrypoints:

- `radar validate` validates checked Radar artifact JSON contracts from the
  Rust CLI.
- `radar refresh-upstream-queue` refreshes
  `.agent/automations/radar/cache/github/review-queue/openai-codex-latest.json`.
- `radar refresh-release-delta` refreshes
  `.agent/automations/radar/cache/site-content/release-deltas/openai-codex-latest.json`.
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

- input bundle shape: `docs/spec/github-change-bundle.md`
- upstream review queue and AI review boundary: `docs/spec/upstream-review.md`
- output signal shape: `docs/spec/signal-entry.md`
- upstream impact shape: `docs/spec/upstream-impact.md`
- Control Plane upgrade candidate shape:
  `docs/spec/control-plane-upgrade-candidate.md`

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

The sync records every observed recent commit in the local SQLite Radar ledger and
writes `.agent/automations/radar/cache/github/review-queue/openai-codex-latest.json`. It does not install
Codex, make AI judgments, render public signals, or publish social posts.
If only `generated_at` would change, the command leaves the existing queue file intact
to avoid empty commits.

Release-delta refresh:

```bash
radar refresh-release-delta \
  --repo openai/codex \
  --signals-dir .agent/automations/radar/cache/site-content/signals \
  --out .agent/automations/radar/cache/site-content/release-deltas/openai-codex-latest.json
```

The release-delta refresh compares the latest stable and prerelease tags, maps compare
commits back to published signal entries, and also leaves the existing file intact when
only `generated_at` would change.

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

Do not wire `run_codex_analysis.py` into GitHub Actions. Actions must not pass
`--allow-ai-analysis-boundary` or set `DECODEX_ALLOW_CODEX_ANALYSIS`; that
acknowledgement is reserved for Rust-owned local automation and explicit operator
recovery runs that still keep bundle validation and `analysis_draft` schema validation
inside the helper.

Repo-local skills under `automations/radar/skills/` are reasoning instructions for the Codex
analysis step and for manual Radar work. Pre-publication and terminal social artifacts
are Decodex Publisher contracts, not Radar contracts.

Raw bundles and analysis drafts are retained in hot cache for a 21-day window. Archive
older raw batches through an explicit external-archive handoff when needed, and write
the recovery manifest under `.agent/automations/radar/cache/archive/index/`. See
`docs/spec/radar-artifact-retention.md` and `docs/runbook/radar-artifact-archive.md`.

# Scripts Root

This directory contains executable repository automation helpers.

- `scripts/github/` owns the automation-only Codex AI analysis helper and shared
  schema support used by that helper.
- `scripts/config/` owns config-derived artifact synchronization scripts.

Checked-in data produced or consumed by scripts belongs outside this directory. GitHub
review queues, upstream reviews, bundles, impact records, and analysis drafts live
under `artifacts/github/`.

Durable deterministic Radar workflows are owned by the Rust CLI. Use
`decodex radar ...` for queue refresh, release-delta refresh, bundle build/validation,
ledger maintenance, signal rendering, release-window backfill, and Radar artifact
validation.

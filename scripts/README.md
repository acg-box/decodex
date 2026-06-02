# Scripts Root

This directory contains executable repository automation.

- `scripts/github/` owns deterministic GitHub upstream review queue, release-delta,
  render, sync, and validation scripts.
- `scripts/config/` owns config-derived artifact synchronization scripts.

Checked-in data produced or consumed by scripts belongs outside this directory. GitHub
review queues, upstream reviews, bundles, impact records, and analysis drafts live
under `artifacts/github/`.

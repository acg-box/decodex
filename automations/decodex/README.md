# Decodex Automation Operations

This directory owns Decodex upstream-monitoring and public-publishing automation that
previously lived in the local Documents automation folder.

It is intentionally operations-shaped and repo-local:

- `skills/`: Codex-facing automation skills for upstream triage, analysis,
  signal drafting, and X post quality/publishing.
- `scripts/`: automation scripts and schemas.
- There is no GitHub Actions-owned workflow lane here; recurring work is owned by
  Codex app automation and local scripts.
- `rust/`: Rust automation implementation extracted from the Decodex runtime tree.
- Repository `docs/`: automation-specific runbooks, specs, and decisions.
- `research/`: retained automation research data that is not part of the Markdown docs
  bundle.
- `.agent/automations/decodex/cache/github/`: upstream GitHub bundles, reviews, impact classifications, queues, and
  publication candidates.
- `.agent/automations/decodex/cache/social/`: external publication records, reservations, and generated media
  evidence.
- `.agent/automations/decodex/cache/archive/`: archived automation batch manifests.
- `.agent/automations/decodex/cache/site-content/`: generated static-site content snapshots that automation may
  reuse.
- `.agent/automations/decodex/cache/generated/`: generated JSON side data used by automation.

Product/runtime/app code remains under `apps/`, `site/`, and `plugins/`. This directory
is for automation source. Generated state belongs under `.agent/automations/decodex/`.

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

## Pipeline Boundary

`codex-upstream-radar-review` is the shared upstream evidence producer. It refreshes
the upstream review queue, creates source-backed review/impact artifacts, and may
write Control Plane upgrade candidates or public social candidates as evidence.

`codex-release-checkpoint-publisher` and `decodex-x-publisher` are downstream
consumers. They should read existing `release_delta/v1`, `upstream_review/v1`,
`upstream_impact/v1`, `signal_entry/v1`, and `social_candidate/v1` artifacts instead
of repeating upstream source analysis. Release Curator may refresh only the lightweight
release-delta checkpoint when that artifact is missing or stale. X Publisher may only
publish from a validated candidate or explicit operator handoff.

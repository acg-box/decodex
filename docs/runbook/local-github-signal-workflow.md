---
type: "Runbook"
title: "Local GitHub Signal Workflow"
description: "Define the local automation workflow for collecting GitHub evidence and maintaining signal artifacts."
status: active
authority: procedural
owner: automation
tags: [runbook, radar]
last_verified: 2026-06-25
---
# Local GitHub Signal Workflow

Goal: Define the repeatable workflow for collecting upstream Codex evidence, running
Codex analysis in automation or local sessions, validating signal entries, and
maintaining local site-content snapshots.

Read this when:
- You are preparing the first GitHub-backed Decodex signal entries.
- You need to know where AI analysis runs versus deterministic local automation.
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
- `automations/radar/skills/README.md`

Outputs:
- A validated signal entry in `.agent/automations/radar/cache/site-content/signals`
- Optional upstream-impact artifacts when the change affects Control Plane or Publisher
  planning
- Local Radar cache artifacts that downstream automation can consume

## Workflow

1. Track upstream Codex commits continuously with `radar refresh-upstream-queue`.
   Treat each commit as an evidence unit, resolve it back to a PR when possible, write
   `.agent/automations/radar/cache/github/radar.sqlite3`, and refresh `upstream_review_queue/v1`.
2. Let Codex automation consume queued subjects and run
   `automations/radar/skills/codex-code-analysis/` for each source-backed review.
3. Build a normalized GitHub change bundle under `.agent/automations/radar/cache/github/bundles/` when the
   automation or operator needs full source context for a candidate.
4. Promote reviewed conclusions into `upstream_impact/v1` when the change may affect
   Control Plane, Publisher planning, compatibility, or adoption.
5. Use `automations/radar/skills/codex-release-analysis/` when the source is a release, prerelease,
   app update, or changelog entry.
6. Run final signal drafting with `automations/radar/skills/github-signal/` and save the
   `analysis_draft` JSON under `.agent/automations/radar/cache/generated/analysis/`.
7. Render the resulting signal entry into `.agent/automations/radar/cache/site-content/signals/` with
   `radar render-signal`.
8. Validate the signal entry shape and collection consistency.
9. Classify upstream impact when the change may affect Control Plane or Publisher.
10. Regenerate the release-delta artifact so the homepage compares release windows
    using the updated signal set.
11. Hand off optional social content decisions only through
    [`social-publishing-workflow.md`](./social-publishing-workflow.md).
12. When upstream publishes a release or prerelease, use `codex-release-analysis` to
    roll up the accumulated commit/PR analysis into release checkpoint evidence.
13. Review the rendered content manually when a public snapshot is being prepared.
14. Do not push or publish from this operations lane; hand off any public-site update
    explicitly.

## Deterministic commands

Build a PR-first bundle:

```bash
radar bundle build \
  --repo openai/codex \
  --pr 22414 \
  --out .agent/automations/radar/cache/github/bundles/openai-codex-pr-22414.json
```

Validate the bundle:

```bash
radar bundle validate \
  .agent/automations/radar/cache/github/bundles/openai-codex-pr-22414.json
```

Render a final signal entry from the reviewed bundle plus the Codex-owned
`analysis_draft`:

```bash
radar render-signal \
  --bundle .agent/automations/radar/cache/github/bundles/openai-codex-pr-22414.json \
  --analysis .agent/automations/radar/cache/generated/analysis/openai-codex-pr-22414.analysis.json \
  --out .agent/automations/radar/cache/site-content/signals/openai-codex-pr-22414.json
```

Validate the published signal entries and the site collection:

```bash
radar validate .agent/automations/radar/cache/site-content/signals
```

Build the homepage release-delta artifact:

```bash
radar refresh-release-delta \
  --repo openai/codex \
  --signals-dir .agent/automations/radar/cache/site-content/signals \
  --out .agent/automations/radar/cache/site-content/release-deltas/openai-codex-latest.json
```

Preview unpublished PRs from a selected release compare range without generating
content:

```bash
radar backfill-release-range \
  --repo openai/codex \
  --stable-tag rust-v0.130.0 \
  --preview-tag rust-v0.131.0-alpha.9 \
  --dry-run
```

Use release-range backfill to fill gaps in the accumulated commit/PR analysis before a
release or prerelease summary. It should supplement continuous commit tracking, not
replace it. Execute mode is still a Codex app automation or local operator path: Rust
selects the release-window gaps and sequences deterministic Radar commands, while the
AI review step follows the repo-local skills and schemas instead of running inside
GitHub Actions.

`automations/radar/scripts/github/run_codex_analysis.py` remains only the bounded deterministic process
wrapper for that AI review step. Prefer `radar backfill-release-range` or a
normal Codex automation session. Direct helper recovery runs must pass
`--allow-ai-analysis-boundary` or set `DECODEX_ALLOW_CODEX_ANALYSIS=1`, and the helper
still validates both the input bundle and the returned `analysis_draft` before writing
output.

The repository already includes a real sample for this flow:

- bundle: `.agent/automations/radar/cache/github/bundles/openai-codex-pr-22414.json`
- editorial draft: `.agent/automations/radar/cache/generated/analysis/openai-codex-pr-22414.analysis.json`
- rendered signal: `.agent/automations/radar/cache/site-content/signals/openai-codex-pr-22414.json`

Repo-local editorial instruction entrypoint:

- `automations/radar/skills/README.md`

These entrypoints are for Decodex automation operations only. They are incomplete as
general user-facing skills and must not be packaged with the installable Decodex
plugin. Today only `github_change_bundle/v1`, `analysis_draft`, `signal_entry/v1`,
`upstream_impact/v1`, and `release_delta/v1` are durable content contracts for this
Radar workflow. Decodex Publisher owns social contracts.

Automated sync entrypoint:

- `radar refresh-upstream-queue`

Bootstrap or inspect local historical trace:

```bash
radar ledger ingest-existing
radar ledger summary --json
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

For upstream-impact and Publisher handoff evidence:

- classify Control Plane implications before creating engineering follow-up work
- keep social publication, block, skip, and failure records in Decodex Publisher cache
- do not use X engagement as technical evidence

## Execution Boundary

The current Decodex boundary is:

- Codex app automation: deterministic upstream commit discovery, PR mapping,
  review-queue refresh, release-delta refresh, validation, AI source review,
  compatibility judgment, Publisher handoff evidence, `analysis_draft`
  creation, `radar render-signal`, and any promotion into signal or follow-up
  artifacts.
- local operator sessions: manual editorial review, batch backfills, prompt iteration,
  `radar backfill-release-range`, and public-content audit.
- GitHub Actions: no ownership in this operations lane. Do not create or rely on
  GitHub Actions for upstream-monitoring or public-publishing state.

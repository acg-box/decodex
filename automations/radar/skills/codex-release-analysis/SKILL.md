---
name: codex-release-analysis
description: Use when evaluating upstream Codex releases, prereleases, app updates, or changelog entries and deciding what Radar evidence Decodex should publish, watch, or absorb from the release.
---

# Decodex Codex Release Analysis

Use this repo-local skill to turn already-reviewed Radar evidence, official Codex
changelog entries, GitHub release metadata, and GitHub compare metadata into a
source-backed release checkpoint.

## Read First

- `automations/radar/skills/codex-upstream-triage/SKILL.md` and
  `automations/radar/skills/codex-code-analysis/SKILL.md` only when routing missing source analysis

## Hard Boundaries

- Do not perform fresh upstream source analysis here.
- Do not refresh the upstream review queue here. Treat Radar Review as the shared
  upstream evidence producer.
- Do not write Decodex content candidates, social posts, or
  `social_publish_reservation/v1`. Decodex Publisher owns social artifacts.
- Treat `upstream_impact/v1`, release deltas, reviews, and signals as optional Radar
  evidence. They do not create a Content Manager candidate or authorize publication.
- Use compare metadata to identify PR/commit gaps; route behavior claims that need code
  review to `upstream_analysis_required`.

## Workflow

1. Identify the channel: stable release, prerelease, app/mobile changelog, or no new
   checkpoint.
2. Select the correct comparison:
   - stable: current stable -> previous stable
   - prerelease: current prerelease -> previous prerelease in the same train
   - first prerelease after stable: stable -> first prerelease
3. Read existing `upstream_impact/v1` artifacts that match the channel first, then use
   `release_delta/v1`, `signal_entry/v1`, and `upstream_review/v1` artifacts to check
   lineage, evidence, and gaps.
4. Use `radar backfill-release-range --dry-run` when compare PR gaps may need later
   source review.
5. Choose exactly one primary mode for reporting:
   - `release_pulse`: release or changelog headline is the story
   - `watch_note`: useful checkpoint with caveats or incomplete analysis
   - `release_rollup`: accumulated source-backed Radar evidence explains the changes
   - `delta_explainer`: compare metadata maps to existing signals or clear PRs
   - `operator_impact`: app-server, plugin, browser, MCP, sandbox, config, auth, or
     provider behavior affects external operators
6. If the checkpoint has no reader value or only a version tag, write a durable no-op,
   defer, skip, or `needs_upstream_analysis` decision instead of silent omission.

## Output

Return source and timestamp, chosen mode, release-body quality, compare and PR
evidence, matching signal slugs, Decodex impact if any, editorial angle if any, and
source gaps.

Promote durable Radar conclusions only through `upstream_impact/v1`, a reviewed
`analysis_draft` plus `radar render-signal`, or `release_delta/v1`. The Content
Manager still verifies official sources independently.

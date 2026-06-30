---
type: "Runbook"
title: "Radar Artifact Archive"
description: "Define the procedure for moving old Radar raw artifacts out of hot cache."
status: active
authority: procedural
owner: automation
tags: [runbook, radar]
last_verified: 2026-06-27
---
# Radar Artifact Archive

Goal: Move old raw Radar artifacts out of hot cache after the 21-day hot window while
keeping public signals and archive recovery evidence available.

Read this when:
- You are pruning `.agent/automations/radar/cache/github/bundles/` or `.agent/automations/radar/cache/generated/analysis/`.
- You need to package cold Radar artifacts for an explicit external-archive handoff.
- You are reviewing whether a public signal can remain after its raw bundle leaves hot
  cache.

Governing spec:
- [`../spec/radar-artifact-retention.md`](../spec/radar-artifact-retention.md)

## Archive candidates

Archive these after the 21-day hot window:

- `.agent/automations/radar/cache/github/bundles/*.json`
- `.agent/automations/radar/cache/github/reviews/*.review.json`
- `.agent/automations/radar/cache/generated/analysis/*.analysis.json`
- optional raw source snapshots if a future cache directory is added
- optional ledger exports if they are generated for a closed archive batch

Do not archive these as part of raw cleanup:

- `.agent/automations/radar/cache/site-content/signals/*.json`
- the current `.agent/automations/radar/cache/site-content/release-deltas/openai-codex-latest.json`
- `.agent/automations/radar/cache/github/impact/*.json` with active Control Plane or Publisher relevance
- `.agent/automations/radar/cache/github/control-plane-upgrades/*.json` with active
  Control Plane review relevance
- `.agent/automations/radar/cache/archive/index/*.json`

## Procedure

1. Choose the archive window.
   - Prefer a calendar month or a release-window name.
   - Ensure the selected raw artifacts are outside the 21-day hot window.
   - For artifacts without embedded collection timestamps, use the paired signal
     `published_at` or record the operator-selected evidence date in the manifest.

2. Build the archive directory.
   - Preserve repository-relative paths inside the archive.
   - Include paired bundle, upstream review, and analysis files together when they exist.
   - Include a local `manifest.json` with `schema = "radar_archive_manifest/v1"`.

3. Compress the archive.
   - Preferred asset name:
     `decodex-radar-archive-<archive-id>.tar.zst`
   - Generate `SHA256SUMS`.

4. Prepare external archive handoff only when needed.
   - Proposed external storage must use a non-product archive id such as
     `radar-archive-2026-05`.
   - Report the compressed archive name, `manifest.json`, and `SHA256SUMS`.
   - Do not create GitHub Actions or write archive assets into the main Decodex repo.

5. Persist the local cleanup.
   - Add `.agent/automations/radar/cache/archive/index/<archive-id>.json`.
   - Remove the archived raw files from `.agent/automations/radar/cache/github/bundles/`,
     `.agent/automations/radar/cache/github/reviews/`, and
     `.agent/automations/radar/cache/generated/analysis/`.
   - Keep public signals and curated impact/social artifacts in place.

6. Verify recovery metadata.
   - Confirm the manifest paths match the removed files.
   - Confirm `SHA256SUMS` matches any external archive asset when one exists.
   - Confirm any public signal that still references a removed bundle has source refs
     back to GitHub and an archive manifest pointer for raw recovery.

## Operator notes

Archiving raw files reduces future operations-cache size and review noise. It does not
rewrite historical copies that may exist elsewhere. A repository history rewrite is a
separate, explicit maintenance operation and should not be part of normal monthly
Radar archiving.

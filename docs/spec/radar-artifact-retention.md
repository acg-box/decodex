---
type: "Spec"
title: "Radar Artifact Retention"
description: "Define hot-cache and cold-archive retention boundaries for Decodex Radar artifacts."
status: active
authority: normative
owner: automation
tags: [spec, radar]
last_verified: 2026-06-27
---
# Radar Artifact Retention

Purpose: Define which Decodex Radar artifacts stay in the operations
cache, which raw artifacts are kept only in a short hot window, and how cold archives remain
recoverable.

Status: normative

Read this when:
- You are deciding whether a GitHub bundle, upstream review, analysis draft, signal
  entry, or upstream impact note should remain in the
  operations cache.
- You are preparing an archive batch for old Radar artifacts.
- You are adding automation that prunes or restores `.agent/automations/radar/cache/github/` material.

Not this document:
- The GitHub change-bundle schema.
- The signal-entry schema.
- The step-by-step archive procedure.

Defines:
- Hot, warm, and cold Radar artifact classes.
- The maximum hot-cache window for raw bundles and analysis drafts.
- The optional external archive handoff contract.
- The manifest record that keeps cold artifacts traceable from `.agent/automations/radar/cache/archive`.

## Retention classes

Decodex uses three retention classes for Radar data.

| Class | Storage | Examples | Retention |
| --- | --- | --- | --- |
| Hot raw artifacts | Operations cache | `.agent/automations/radar/cache/github/bundles/*.json`, `.agent/automations/radar/cache/github/reviews/*.review.json`, `.agent/automations/radar/cache/generated/analysis/*.analysis.json` | At most 21 days in hot cache after collection or publication. |
| Warm curated artifacts | Operations cache | `.agent/automations/radar/cache/site-content/signals/*.json`, `.agent/automations/radar/cache/site-content/release-deltas/openai-codex-latest.json`, `.agent/automations/radar/cache/github/impact/*.json`, `.agent/automations/radar/cache/github/control-plane-upgrades/*.json` | Retained while they are part of the public snapshot, Control Plane review trail, or Publisher handoff evidence. |
| Cold raw archive | `.agent/automations/radar/cache/archive` manifest plus optional external assets | Archived bundle and analysis batches, optional source snapshots, optional ledger exports | Retained outside hot cache. The manifest stays in `.agent/automations/radar/cache/archive/index`. |

The hot raw window is intentionally short. Continuous Radar should keep every upstream
commit traceable, but it must not make the operations cache a permanent raw-data
warehouse.

## Hot raw artifact rule

Raw GitHub bundles, upstream review artifacts, and local editorial analysis drafts must
not remain in hot cache for more than 21 days after collection or publication unless a human
explicitly marks the batch as still active.

Analysis drafts under `.agent/automations/radar/cache/generated/analysis/*.analysis.json`
are Codex helper output, not first-class Radar artifacts, so they do not carry a
`schema` field. `radar validate` still checks them by path against the
`analysis_draft` contract before they can feed `signal_entry/v1` rendering.

For existing artifacts that do not carry their own collection timestamp, the retention
clock should use the paired `signal_entry/v1.published_at` when available. If no paired
signal exists, the archive batch must record the operator-selected evidence date in its
manifest.

The 21-day limit applies to the raw supporting material, not to the public signal
entry. A signal entry may outlive its raw bundle when the archive manifest preserves how
to recover the original bundle and analysis draft.

## Warm curated artifact rule

Keep these artifacts in cache unless a separate content cleanup explicitly removes them:

- published `signal_entry/v1` files under `.agent/automations/radar/cache/site-content/signals/`
- the current homepage `release_delta/v1` artifact
- the latest `upstream_review_queue/v1` artifact under `.agent/automations/radar/cache/github/review-queue/`
- `upstream_impact/v1` records that affect Decodex Control Plane or Publisher follow-up
- `control_plane_upgrade_candidate/v1` records that preserve Control Plane upgrade
  review, deferral, blocker, or supersession evidence
- archive manifests under `.agent/automations/radar/cache/archive/index/`

## Cold archive destination

Cold raw artifacts may be stored as external release assets only after an explicit
operator handoff. Compressed archives must not be written back into the main
`hack-ink/decodex` repository.

Use tag names that cannot be confused with product releases, for example:

- `radar-archive-2026-05`
- `radar-archive-rust-v0.130.0-to-rust-v0.131.0-alpha.9`

Each archive release should include:

- one compressed archive, preferably `decodex-radar-archive-<id>.tar.zst`
- `manifest.json`
- `SHA256SUMS`
- optional detached signatures when the operator has signing material available

The operations cache keeps a copy of the manifest under
`.agent/automations/radar/cache/archive/index/<archive-id>.json`. That manifest is the durable pointer to any
external release assets.

## Manifest contract

The archive manifest schema identifier is:

- `radar_archive_manifest/v1`

The manifest must contain:

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `radar_archive_manifest/v1`. |
| `archive_id` | string | Stable archive identifier. |
| `created_at` | string | UTC timestamp for archive creation. |
| `retention_days` | number | New manifests must use `21` unless a later spec changes the policy. Historical manifests may keep the value that governed their original archive batch. |
| `source_commit` | string | Repository commit used to select and package files. |
| `release_tag` | string | GitHub tag holding the archive assets. |
| `release_url` | string | GitHub Release URL when available. |
| `archive_asset` | object | Name, size, and SHA-256 for the compressed archive. |
| `checksum_asset` | object | Name and SHA-256 for `SHA256SUMS`. |
| `files` | array | Archived file records. |

Each `files[]` record must contain:

- `path`
- `kind` (`bundle`, `analysis`, `source_cache`, `ledger_export`, or `other`)
- `sha256`
- `size_bytes`

When the archive batch removes files from hot cache, the same run must add the
manifest that points to the archived material or records the no-external-asset handoff.

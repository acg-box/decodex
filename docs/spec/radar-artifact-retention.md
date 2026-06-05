# Radar Artifact Retention

Purpose: Define which Decodex Radar and Publisher artifacts stay in Git, which raw
artifacts are kept only in a short hot window, and how cold archives remain
recoverable.

Status: normative

Read this when:
- You are deciding whether a GitHub bundle, upstream review, analysis draft, signal
  entry, upstream impact note, or social publication record should remain checked in.
- You are preparing an archive batch for old Radar artifacts.
- You are adding automation that prunes or restores `artifacts/github/` material.

Not this document:
- The GitHub change-bundle schema.
- The signal-entry schema.
- The step-by-step archive procedure.

Defines:
- Hot, warm, and cold Radar artifact classes.
- The maximum Git hot-window for raw bundles and analysis drafts.
- The GitHub Release asset archive contract.
- The manifest record that keeps cold artifacts traceable from Git.

## Retention classes

Decodex uses three retention classes for Radar and Publisher data.

| Class | Storage | Examples | Retention |
| --- | --- | --- | --- |
| Hot raw artifacts | Git working tree | `artifacts/github/bundles/*.json`, `artifacts/github/reviews/*.review.json`, `artifacts/github/analysis/*.analysis.json` | At most 21 days in Git after collection or publication. |
| Warm curated artifacts | Git working tree | `site/src/content/signals/*.json`, `site/src/content/release-deltas/openai-codex-latest.json`, `artifacts/github/impact/*.json`, `artifacts/social/x/posts/*.json` | Retained in Git while they are part of the public site, Control Plane review trail, Publisher record, or cap analysis. |
| Cold raw archive | GitHub Release assets plus a Git manifest | Archived bundle and analysis batches, optional source snapshots, optional ledger exports | Retained outside the Git tree. Git keeps only the manifest. |

The hot raw window is intentionally short. Continuous Radar should keep every upstream
commit traceable, but it must not make the repository a permanent raw-data warehouse.

## Hot raw artifact rule

Raw GitHub bundles, upstream review artifacts, and local editorial analysis drafts must
not remain in Git for more than 21 days after collection or publication unless a human
explicitly marks the batch as still active.

For existing artifacts that do not carry their own collection timestamp, the retention
clock should use the paired `signal_entry/v1.published_at` when available. If no paired
signal exists, the archive batch must record the operator-selected evidence date in its
manifest.

The 21-day limit applies to the raw supporting material, not to the public signal
entry. A signal entry may outlive its raw bundle when the archive manifest preserves how
to recover the original bundle and analysis draft.

## Warm curated artifact rule

Keep these artifacts in Git unless a separate content cleanup explicitly removes them:

- published `signal_entry/v1` files under `site/src/content/signals/`
- the current homepage `release_delta/v1` artifact
- the latest `upstream_review_queue/v1` artifact under `artifacts/github/review-queue/`
- `upstream_impact/v1` records that affect Decodex Control Plane or Publisher follow-up
- `social_candidate/v1` records that preserve `publish`, `defer`, or `skip` Publisher
  intake decisions
- `social_post/v1` records, including daily-cap blocks
- archive manifests under `artifacts/archive/index/`

Generated social images are not warm curated artifacts and should not be committed by
automation by default. Keep them in a local media cache when visual QA or debugging
needs them. If an operator explicitly commits a sample image, treat it as a hot artifact
and archive or remove it after the same 21-day window when the paired `social_post/v1`
record keeps enough URL/hash/prompt metadata to audit the publication.

## Cold archive destination

Cold raw artifacts must be stored as GitHub Release assets under a dedicated Radar
archive tag. They must not be committed as compressed archives inside the repository.

Use tag names that cannot be confused with product releases, for example:

- `radar-archive-2026-05`
- `radar-archive-rust-v0.130.0-to-rust-v0.131.0-alpha.9`

Each archive release should include:

- one compressed archive, preferably `decodex-radar-archive-<id>.tar.zst`
- `manifest.json`
- `SHA256SUMS`
- optional detached signatures when the operator has signing material available

Git keeps a copy of the manifest under `artifacts/archive/index/<archive-id>.json`.
That manifest is the durable pointer from the repository to the release assets.

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

When the archive batch removes files from Git, the same commit must add the manifest
that points to the GitHub Release asset.

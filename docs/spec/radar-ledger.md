---
type: "Spec"
title: "Radar Ledger"
description: "Define the local SQLite ledger for upstream Codex trace, review, and artifact history."
status: active
authority: normative
owner: automation
tags: [spec, radar]
last_verified: 2026-06-25
---
# Radar Ledger

Purpose: Define the local SQLite ledger that keeps every observed upstream Codex commit
traceable without putting every raw or low-value artifact into Git.

Status: normative

Read this when:
- You are changing `radar refresh-upstream-queue`.
- You are changing `radar ledger ...`.
- You are importing existing GitHub bundles, analysis drafts, or signal entries into
  historical Radar state.
- You need to decide what belongs in local history instead of checked-in public
  artifacts.

Not this document:
- The public `signal_entry/v1` schema.
- The raw-artifact archive procedure.
- The release-delta homepage rendering contract.

Defines:
- The local ledger path.
- The required ledger tables.
- The relationship between observed commits, reviews, and checked-in artifacts.
- The rule that every upstream commit can have durable trace without becoming a public
  site entry.

## Local storage

The default local Radar ledger path is:

```text
.agent/automations/decodex/cache/github/radar.sqlite3
```

`.agent/` is ignored by Git. The ledger is local or CI runtime state, not a checked-in
artifact. It may be rebuilt from checked-in warm artifacts and cold archive manifests,
but it is the preferred place for high-frequency trace and skip history.

## Schema

The schema is created by `radar refresh-upstream-queue` and
`radar ledger bootstrap`. The Rust `radar ledger ...` surface owns the
command path for ledger bootstrap, ingest, ingest-existing, artifact-link, and summary
operations.

Required tables:

| Table | Purpose |
| --- | --- |
| `upstream_commit` | One row per observed upstream commit, including SHA, title, URL, commit time, PR number when known, and first/last seen timestamps. |
| `radar_review` | One current review state per commit or PR subject. Status values include `seen`, `skipped`, `watch`, `signal`, `control_plane`, `social`, `deprecated`, and `archived`. The deterministic queue uses `watch` for subjects awaiting AI review. |
| `artifact_link` | Links commits or PRs to Git-tracked or archived artifacts, including file path, artifact kind, SHA-256, size, and creation time. |
| `source_cache` | Optional source cache index for fetched remote payloads when a future cache is added. |

The ledger schema version is stored in `metadata.schema_version`.

## Artifact boundary

Use the ledger for:

- every recent upstream commit observed by continuous Radar
- commits skipped because they are low-signal maintenance
- subjects queued for AI review by `upstream_review_queue/v1`
- mappings from commits to PRs
- links from commits or PRs to bundles, analysis drafts, signals, impact notes, social
  posts, release deltas, archive manifests, or ledger exports

Use Git for:

- curated public site signals
- current release-delta data
- upstream-impact records that affect Decodex Control Plane or Publisher follow-up
- social publication records
- cold archive manifests

Do not use Git as the permanent store for every raw bundle, raw source cache, skipped
candidate, retry queue, or long low-value analysis.

## Sync behavior

`radar refresh-upstream-queue` writes the local ledger by default. It records
every recent commit it inspects, including commits that do not become public signals.

Operators may disable ledger writes with:

```sh
cargo run -p radar -- refresh-upstream-queue --no-ledger
```

Existing checked-in artifacts can be imported with:

```sh
radar ledger ingest-existing
```

This import is useful when bootstrapping a new local workspace or rebuilding trace after
raw GitHub bundles move to cold archive assets.

Operators can inspect local counts with:

```sh
radar ledger summary --json
```

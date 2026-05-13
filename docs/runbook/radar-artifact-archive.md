# Radar Artifact Archive

Goal: Move old raw Radar artifacts out of Git after the 28-day hot window while
keeping public signals and archive recovery evidence available.

Read this when:
- You are pruning `artifacts/github/bundles/` or `artifacts/github/analysis/`.
- You need to package cold Radar artifacts for a GitHub Release asset.
- You are reviewing whether a public signal can remain after its raw bundle leaves Git.

Governing spec:
- [`../spec/radar-artifact-retention.md`](../spec/radar-artifact-retention.md)

## Archive candidates

Archive these after the 28-day hot window:

- `artifacts/github/bundles/*.json`
- `artifacts/github/analysis/*.analysis.json`
- optional raw source snapshots if a future cache directory is added
- optional ledger exports if they are generated for a closed archive batch

Do not archive these as part of raw cleanup:

- `site/src/content/signals/*.json`
- the current `site/src/content/release-deltas/openai-codex-latest.json`
- `artifacts/github/impact/*.json` with active Control Plane or Publisher relevance
- approved or published `artifacts/social/x/*.json`
- `artifacts/archive/index/*.json`

## Procedure

1. Choose the archive window.
   - Prefer a calendar month or a release-window name.
   - Ensure the selected raw artifacts are outside the 28-day hot window.
   - For artifacts without embedded collection timestamps, use the paired signal
     `published_at` or record the operator-selected evidence date in the manifest.

2. Build the archive directory.
   - Preserve repository-relative paths inside the archive.
   - Include paired bundle and analysis files together when both exist.
   - Include a local `manifest.json` with `schema = "radar_archive_manifest/v1"`.

3. Compress the archive.
   - Preferred asset name:
     `decodex-radar-archive-<archive-id>.tar.zst`
   - Generate `SHA256SUMS`.

4. Create a dedicated GitHub Release.
   - Use a non-product tag such as `radar-archive-2026-05`.
   - Upload the compressed archive, `manifest.json`, and `SHA256SUMS`.
   - Do not reuse application release tags.

5. Commit the repository cleanup.
   - Add `artifacts/archive/index/<archive-id>.json`.
   - Remove the archived raw files from `artifacts/github/bundles/` and
     `artifacts/github/analysis/`.
   - Keep public signals and curated impact/social artifacts in place.

6. Verify recovery metadata.
   - Confirm the manifest paths match the removed files.
   - Confirm `SHA256SUMS` matches the uploaded archive asset.
   - Confirm any public signal that still references a removed bundle has source refs
     back to GitHub and an archive manifest pointer for raw recovery.

## Operator notes

Archiving raw files reduces future working-tree size and review noise. It does not
shrink historical Git objects that already contain the old JSON. A repository history
rewrite is a separate, explicit maintenance operation and should not be part of normal
monthly Radar archiving.

# Archive Manifests

This directory stores Git-tracked manifests for cold Radar archive batches.

Compressed archive payloads do not live in Git. Store them as GitHub Release assets under
dedicated `radar-archive-*` tags, then keep the recovery manifest in `index/`.

The governing contract is `docs/spec/radar-artifact-retention.md`.

# GitHub Artifacts

This directory stores checked-in GitHub signal pipeline artifacts.

- `bundles/` holds normalized `github_change_bundle/v1` inputs.
- `analysis/` holds reviewed Codex editorial analysis drafts.
- `impact/` holds optional `upstream_impact/v1` classifications.

`bundles/` and `analysis/` are hot raw artifact directories. Keep raw entries in Git for
at most 28 days, then move cold batches to dedicated `radar-archive-*` GitHub Release
assets and keep the recovery manifest under `artifacts/archive/index/`.

Executable automation for these artifacts lives under `scripts/github/`. Repo-local
editorial instructions live under `dev/skills/github-signal/`.

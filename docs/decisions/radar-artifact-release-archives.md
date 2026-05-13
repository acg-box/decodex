# Radar Artifact Release Archives

Status: accepted

Date: 2026-05-13

Question: Where should Decodex keep old raw Radar artifacts after the short Git hot
window?

Decision: Keep raw GitHub bundles and editorial analysis drafts in Git for at most 28
days, then archive cold batches as dedicated GitHub Release assets. The repository keeps
only a manifest under `artifacts/archive/index/`; compressed archives are not committed
to Git.

Rationale:

- Continuous Radar should inspect every upstream Codex commit, but the repository should
  not become a permanent raw-data warehouse.
- Public signal entries, upstream-impact records, and published social drafts are small,
  curated, reviewable artifacts and can remain in Git.
- Raw bundles and analysis drafts can be recovered from an archive asset when needed, as
  long as the manifest records file paths, checksums, source commit, and release URL.
- GitHub Release assets are better than checked-in compressed archives because they keep
  the Git tree readable while preserving a durable download location tied to the repo.

Consequences:

- `artifacts/github/bundles/` and `artifacts/github/analysis/` are hot working
  directories, not permanent history.
- Archive releases use a separate tag namespace such as `radar-archive-2026-05` so they
  are not confused with Decodex product releases.
- Normal archive cleanup does not shrink existing Git history. A history rewrite remains
  a separate explicit operation.
- Automation that prunes raw artifacts must add or update an archive manifest in the
  same change that removes files from Git.

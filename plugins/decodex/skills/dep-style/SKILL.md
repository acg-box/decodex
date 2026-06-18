---
name: dep-style
description: Use when normalizing dependency constraint style, GitHub Actions SHA pinning policy, manifest entry shape, or repo-wide version-spec policy without performing a live dependency roll.
---

# Dep Style

Use this only for manifest/style policy. Preserve the dependency graph.

## Owns

- Dependency specifier/operator cleanup.
- Manifest entry shape normalization.
- GitHub Actions external `uses:` full-SHA pinning style.
- Existing selected tag/branch/channel to full commit SHA conversion.

## Does Not Own

- Newer version selection, newest compatible sets, Dependabot reconciliation, API
  migration, source compatibility work, or lockfile-only solver refresh. Use
  `$decodex:dep-roll` for those.

## Rules

- Read checked-in dependency policy, docs, CI, and local manifest convention first.
- Normalize only the in-scope manifest/workflow lines.
- If no clear policy exists, preserve touched-area consistency instead of inventing a
  repo-wide style.
- Stop and split work if style cleanup would force large lockfile churn or source
  migration.
- For GitHub Actions, normalize only external action refs and external reusable
  workflows. Local actions/workflows such as `./.github/actions/foo` are repository
  code, not external dependency pins.
- Pin external GitHub Actions refs to full commit SHAs. For annotated tags, use the
  peeled commit SHA, not the tag-object SHA.
- Do not silently upgrade to a newer release in `dep-style`; target selection belongs
  to `dep-roll`.
- Regenerate lockfiles only when manifest semantics changed and the repo expects lock
  sync. Plain workflow `uses:` refs have no lockfile step.

## Output

Report normalized files, the policy/convention used, intentional exceptions, lockfile
effect, and for GitHub Actions each source ref converted to a full SHA.

---
name: dependency-policy
description: Use when dependency upgrades, Dependabot, manifest style, lockfiles, generated dependency artifacts, or GitHub Actions SHA pins affect repo work.
---

# Dependency Policy

Use this skill for direct dependency work. It owns version-roll and style-only
dependency decisions, but repo command authority, validation scope, and completion
claims still come from `$codebase:work` and `$codebase:verification`.

Read `../../references/dependency-policy.md` before changing dependency manifests,
lockfiles, generated dependency artifacts, Dependabot branches, or
`.github/workflows/*` external `uses:` refs.

## Modes

- `roll`: enumerate the whole discoverable dependency surface first, then move to
  the latest supportable compatible release set in one consolidated lane. This is
  not lockfile-only update. Include direct manifest bumps, including semver-major
  bumps, generated dependency artifacts, existing Dependabot PR candidates, and
  GitHub Actions external refs. Migrate reasonable source/config/workflow breakage
  in the same lane, and record concrete blockers for anything left behind.
- `style`: normalize dependency specifiers, manifest entry shape, or GitHub Actions
  full-SHA pins without selecting newer dependency versions.

## Roll Completion

Before claiming a dependency roll is ready, prove the consolidated change covered,
blocked, or intentionally deferred every discovered update candidate. For repositories
with GitHub PR access, open Dependabot PRs are authoritative candidates, and the
completion evidence must include the residual open dependency PR check. Do not claim
completion from a passing PR alone when other dependency PRs, manifest bumps, or action
refs remain outside the consolidated change.

## Output

Report mode, changed manifests/workflows, lockfiles or generated artifacts, selected
release/tag to full-SHA provenance, discovered update candidates, residual checks,
verification commands, and any covered, blocked, deferred, or
`requires-follow-up-migration` items.

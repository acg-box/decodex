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

- `roll`: move to the latest supportable compatible release set, migrate reasonable
  source/config breakage in the same lane, and record concrete blockers for anything
  left behind.
- `style`: normalize dependency specifiers, manifest entry shape, or GitHub Actions
  full-SHA pins without selecting newer dependency versions.

## Output

Report mode, changed manifests/workflows, lockfiles or generated artifacts, selected
release/tag to full-SHA provenance, verification commands, and any covered,
blocked, deferred, or `requires-follow-up-migration` items.

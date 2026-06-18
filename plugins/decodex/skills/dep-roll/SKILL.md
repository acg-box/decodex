---
name: dep-roll
description: Use when performing dependency rolls, package bumps, GitHub Actions action ref updates, lockfile regeneration, or Dependabot reconciliation to the newest verified compatible set.
---

# Dep Roll

## Goal

Upgrade the dependency graph to the newest verified compatible set while preserving
repo-native manifest style, generated-file authority, GitHub Actions full-SHA policy,
and explicit follow-up records for incompatible majors.

Read `../../references/dep-roll-policy.md` before changing dependency manifests,
lockfiles, generated dependency artifacts, GitHub Actions `uses:` refs, or Dependabot
reconciliation state.

## Decision Rule

- Use this skill when the success condition is "latest verified compatible set".
- Use this skill for GitHub Actions action ref updates and Dependabot reconciliation,
  including `package-ecosystem: "github-actions"`.
- Use `$decodex:dep-style` instead when the task is only dependency constraint style,
  manifest-entry normalization, GitHub Actions SHA pinning policy cleanup, or
  Dependabot config style without selecting newer compatible versions.
- Do not widen a dependency roll into API migration, workflow redesign, permission
  cleanup, or repo-wide style churn unless the user or checked-in policy requires it.

## Hard Boundaries

- Change manifest constraints before regenerating lockfiles or other generated
  dependency artifacts.
- Never hand-edit lockfiles or generated dependency artifacts.
- For GitHub Actions, select the latest verified compatible release/tag first, then
  pin the full commit SHA that release/tag resolves to.
- Do not classify dependency or Dependabot status from GitHub labels.
- If a newer major needs source, workflow, or behavior changes, keep the newest
  verified compatible set and report a `requires-follow-up-migration` record with the
  attempted target version(s), concrete failure evidence, and next lane.
- Reconcile Dependabot last, after touched ecosystems have verified.

## Output Checklist

- List changed manifests, lockfiles, generated artifacts, workflows, and external
  action refs.
- Report commands run per ecosystem and whether each updated, no-oped, blocked, or
  selected a fallback.
- Record selected GitHub Actions release/tag provenance and full SHA for each changed
  external ref.
- Separate remaining update signal into `covered`, `requires-follow-up-migration`,
  `blocked`, or `intentionally deferred`.

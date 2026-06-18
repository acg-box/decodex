---
name: dep-roll
description: Use when performing dependency rolls, package bumps, GitHub Actions action ref updates, lockfile regeneration, or Dependabot reconciliation to the newest supported compatible set.
---

# Dep Roll

Upgrade to the newest supported compatible set, not merely the newest easy minor.
Read `../../references/dep-roll-policy.md` before changing dependency manifests,
lockfiles, generated dependency artifacts, GitHub Actions `uses:` refs, or Dependabot
reconciliation state.

## Decision Rule

- Use this when the success condition is a newer dependency/action version.
- Include GitHub Actions action refs and Dependabot `package-ecosystem:
  "github-actions"`.
- Use `$decodex:dep-style` only for style/pinning cleanup that must not select newer
  versions.

## Hard Boundaries

- Attempt the latest release/major that can be supported by the repo's current
  runtime, platform, API, and explicit compatibility constraints.
- Do not leave an old major just because the first bump has breaking changes. Inspect
  the failures and make reasonable source, config, test, or workflow repairs in the
  same lane.
- Fall back only with concrete incompatibility evidence: unsupported runtime/platform,
  removed required API with no viable replacement, unsatisfied dependency constraints,
  upstream breakage, or a migration outside explicit authority.
- Change manifest constraints before regenerating lockfiles/generated artifacts; never
  hand-edit generated dependency artifacts.
- For GitHub Actions, choose the latest verified compatible release/tag first, then
  pin the full commit SHA it resolves to.
- Reconcile Dependabot last, after touched ecosystems verify.

## Output

Report changed manifests, lockfiles/generated artifacts, workflows/action refs,
commands run, selected versions or full-SHA provenance, repair work performed for
breaking changes, and any `covered`, `requires-follow-up-migration`, `blocked`, or
intentionally deferred items.

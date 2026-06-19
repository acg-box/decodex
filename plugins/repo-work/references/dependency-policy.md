# Dependency Policy

Use when dependency upgrades, Dependabot reconciliation, dependency style, manifest
entry shape, or GitHub Actions action pinning affect repository work.

## Roll Scope

Roll to the latest supported compatible set for JavaScript/TypeScript, Python, Rust,
GitHub Actions, and any smaller ecosystem slice the repo contains. "Supported" means
the latest release that can be made to work with the repo's current runtime,
platform, API, and explicit compatibility constraints.

## Hard Rules

- Change manifest constraints before regenerating lockfiles or generated dependency
  artifacts; never hand-edit generated dependency artifacts.
- Preserve repo-local manifest style unless checked-in policy says otherwise.
- Do not stop at an older major only because the first bump introduces compile, type,
  test, config, or workflow failures. Inspect the break and perform reasonable source
  or workflow migration in the same lane.
- Leave a dependency on an older version only with concrete incompatibility evidence:
  unsupported runtime/platform, removed required API with no viable replacement,
  unsatisfied dependency constraints, upstream breakage, or a migration that exceeds
  the explicit task/repo authority.
- When the latest major is not currently supportable, keep the newest supportable
  version and record `requires-follow-up-migration` with attempted target, failure
  evidence, and next lane.
- Reconcile Dependabot last, after manifests and lockfiles verify.
- Classify Dependabot by author, changed files, package ecosystem, dependency/action
  refs, verification, and upstream compatibility evidence; never by GitHub labels.

## Style Scope

Use style mode only for dependency specifier cleanup, manifest entry shape, GitHub
Actions external `uses:` full-SHA pinning, or existing tag/branch/channel to commit
SHA conversion. Preserve the selected dependency graph.

- Do not silently upgrade to a newer release in style mode; version selection belongs
  to roll mode.
- Normalize only the in-scope manifest or workflow lines.
- If no clear policy exists, preserve touched-area consistency instead of inventing a
  repo-wide style.
- Stop and split work if style cleanup would force large lockfile churn or source
  migration.

## GitHub Actions

- Include `.github/workflows/*.yml` and `.github/workflows/*.yaml`.
- External `uses: owner/action@ref` and external reusable workflow refs are dependency
  refs. Local action/workflow paths are repo code.
- For a roll, choose the latest verified compatible release/tag first, then pin the
  full commit SHA it resolves to. For annotated tags, pin the peeled commit SHA.
- For style-only work, convert the existing selected ref to the full commit SHA
  without selecting a newer release.
- Do not weaken full-SHA pins to tags or branches.
- There is no lockfile step for workflow `uses:` refs.
- Validate workflows with repo-native tooling or `actionlint` when available.

## Verification

Report changed manifests, lockfiles/generated artifacts, workflows, action refs,
commands run, selected release/tag -> full SHA provenance, update state, and any
`covered`, `requires-follow-up-migration`, `blocked`, or intentionally deferred items.

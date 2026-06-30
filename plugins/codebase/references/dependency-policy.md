# Dependency Policy

Use when dependency upgrades, Dependabot reconciliation, manifest style, lockfiles,
generated dependency artifacts, or GitHub Actions action pins affect repository work.

## Modes

- `roll`: enumerate the whole discoverable dependency surface first, then move to
  the latest supportable compatible release set for the affected ecosystem in one
  consolidated lane. Migrate reasonable source/config/workflow breakage in the same
  lane. A roll is not just `cargo update`, lockfile refresh, or the first green PR.
- `style`: normalize dependency specifiers, manifest entry shape, or GitHub Actions
  full-SHA pins without selecting newer dependency versions.

## Rules

- Start with an inventory. Include direct package manifests, lockfiles, generated
  dependency artifacts, workflow action refs, ecosystem-specific tool manifests,
  and open Dependabot PRs when GitHub PR access is available.
- Treat open Dependabot PRs as authoritative update candidates, not as separate work
  to land one by one. Consolidate their versions or SHAs into the roll, then
  reconcile or supersede them only after the consolidated change has passed
  verification.
- Change manifests before regenerating lockfiles or generated dependency artifacts.
  Never hand-edit generated dependency artifacts.
- Preserve repo-local manifest style unless checked-in policy says otherwise.
- Do not stop at the current manifest constraint. In roll mode, direct manifests are
  in scope, including semver-major bumps.
- Do not stop at an older major just because the first bump fails. Inspect the break,
  migrate reasonable incompatibilities, and leave older versions only with concrete
  runtime, platform, API, constraint, upstream, or migration-scope evidence.
- If the latest major is not supportable, keep the newest supportable version and
  record `requires-follow-up-migration` with attempted target, failure evidence, and
  next lane.
- Reconcile Dependabot last, after manifests, lockfiles, and verification.

## Compatibility

Compatible means supportable after reasonable agent-applied migration, not "builds
without source changes." API, build-script, formatting, config, workflow, generated
artifact, or feature-flag changes are part of the roll when they can be migrated with
repository-local judgment and validation.

Use `blocked` only for concrete blockers such as upstream packages that do not build
for the required target or feature set, MSRV/runtime/toolchain constraints outside
repo policy, security/license/policy conflicts, or migration decisions requiring
product/API authority outside dependency maintenance. Use
`requires-follow-up-migration` for supportable-but-larger migrations that need their
own lane.

## GitHub Actions

- `.github/workflows/*.yml` and `.github/workflows/*.yaml` external `uses:` refs are
  dependencies. Local actions/workflows are repo code.
- External `uses: owner/action@ref` entries must resolve through the same dependency
  policy as manifest dependencies.
- For roll mode, choose the latest verified compatible release/tag first, then pin
  the full commit SHA it resolves to. For annotated tags, pin the peeled commit SHA.
- For style mode, convert the existing selected ref to the full commit SHA without
  selecting a newer release.
- Do not weaken full-SHA pins to tags or branches. There is no lockfile step for
  workflow `uses:` refs.

## Evidence

Report mode, changed manifests/workflows, lockfiles/generated artifacts, selected
release/tag to full-SHA provenance, discovered update candidates, commands run,
residual dependency checks, and `covered`, `requires-follow-up-migration`,
`blocked`, or intentionally deferred items.

Roll-ready evidence must include the remaining candidate scan appropriate to the
repository, such as open dependency PRs, manifest outdated checks, lockfile update
dry-runs, workflow action refs, or ecosystem-specific update reports. If any scan is
unavailable, report the missing access or tool as an evidence gap instead of claiming
full coverage.

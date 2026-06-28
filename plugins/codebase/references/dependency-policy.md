# Dependency Policy

Use when dependency upgrades, Dependabot reconciliation, manifest style, lockfiles,
generated dependency artifacts, or GitHub Actions action pins affect repository work.

## Modes

- `roll`: move to the latest supportable compatible release set for the affected
  ecosystem. Migrate reasonable source/config/workflow breakage in the same lane.
- `style`: normalize dependency specifiers, manifest entry shape, or GitHub Actions
  full-SHA pins without selecting newer dependency versions.

## Rules

- Change manifests before regenerating lockfiles or generated dependency artifacts.
  Never hand-edit generated dependency artifacts.
- Preserve repo-local manifest style unless checked-in policy says otherwise.
- Do not stop at an older major just because the first bump fails. Inspect the break,
  migrate reasonable incompatibilities, and leave older versions only with concrete
  runtime, platform, API, constraint, upstream, or migration-scope evidence.
- If the latest major is not supportable, keep the newest supportable version and
  record `requires-follow-up-migration` with attempted target, failure evidence, and
  next lane.
- Reconcile Dependabot last, after manifests, lockfiles, and verification.

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
release/tag to full-SHA provenance, commands run, and `covered`,
`requires-follow-up-migration`, `blocked`, or intentionally deferred items.

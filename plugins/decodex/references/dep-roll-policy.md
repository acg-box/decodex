# Dep Roll Policy

Use this reference after `dep-roll/SKILL.md` when a real dependency roll starts.

## Scope

Dep rolls target the newest verified compatible set for JavaScript/TypeScript, Python, Rust, GitHub Actions, and any smaller slice the repo actually contains.
They cover manifest constraints, lockfile regeneration, GitHub Actions action ref
updates, and Dependabot reconciliation. They do not cover broad style cleanup,
workflow redesign, or API migration unless the user or checked-in policy requires
that wider lane.

## Manifest policy boundary

- Change manifest constraints before lockfiles; never hand-edit generated artifacts.
- Preserve repo-local manifest style unless checked-in policy says otherwise.
- If the task is only constraint style, specifier normalization, action pinning style,
  workflow permissions style, or Dependabot config style, use `dep-style` as a
  separate lane. In short: use `dep-style` as a separate lane.
- Run stale-signal inventory before editing: `pnpm -r outdated`,
  `poetry show --latest --outdated --top-level --format json`, `cargo upgrade -n`,
  and workflow `uses:` inspection when relevant.
- Lock refresh commands such as `pnpm install`, `poetry update`, and `cargo update`
  are regeneration steps, not proof that manifest constraints were upgraded.

## Breaking-major policy

- If a newer major needs source, workflow, API, or generated-artifact changes, keep
  the newest verified compatible set and record `requires-follow-up-migration`.
- The record should name the package/action bundle, attempted target version(s),
  concrete failure evidence, and a next action such as "open a separate migration lane".
- Group coupled packages or coupled GitHub Actions together when they must move
  together.
- Final verification must confirm that breaking-major holdouts were reported as
  explicit migration follow-ups. In exact terms: breaking-major holdouts were reported as explicit migration follow-ups.

## GitHub Actions

- Include `.github/workflows/*.yml` and `.github/workflows/*.yaml`.
- External action refs look like `uses: owner/action@ref`.
- external reusable workflow `uses: owner/repo/.github/workflows/file.yml@ref` entries are dependency refs.
- Local actions and local reusable workflows such as `uses: ./.github/actions/foo`
  are repo code, not dependency refs.
- Extract workflow `uses:` refs and classify the current pinning style: major tag,
  full tag, full SHA, branch/channel, local action, external reusable workflow, or
  Docker action.
- Do not weaken GitHub Actions SHA pins to tags or branches.
- For a roll, choose the latest verified compatible release/tag first, then pin the
  full commit SHA that the selected release/tag resolves to.
- The selected action update must pin the full commit SHA that the selected release/tag resolves to.
- For annotated tags, use the peeled commit SHA, not the tag-object SHA.
- Write GitHub Actions update targets as full commit SHAs for the selected release/tag.
- Do not update local action paths or local reusable workflows as dependencies.
- There is no lockfile regeneration step for plain workflow `uses:` refs.
- Validate workflows with repo-native tooling, or run `actionlint` when available.
- If a major action update needs behavior or input changes, record workflow failure
  evidence as `requires-follow-up-migration`; include workflow failure evidence in
  the follow-up note.

## Dependabot

- Include `package-ecosystem: "github-actions"` PRs in reconciliation.
- GitHub labels are not authority for dependency or Dependabot classification.
- Do not couple dependency or Dependabot classification to GitHub labels.
- Do not filter or classify Dependabot PRs by GitHub labels.
- Classify by author, changed files, package ecosystem, dependency/action refs,
  verification evidence, and upstream compatibility evidence.
- Reconcile last, after manifests and lockfiles verify.

## Verification

- List changed manifests, lockfiles, generated artifacts, workflows, and action refs.
- For actions, report `old -> selected release/tag -> full SHA`) provenance.
- Confirm GitHub Actions refs use the repo's full-SHA pinning style.
- Confirm touched ecosystem checks ran and state updated, no-op, fallback-selected,
  blocked, covered, `requires-follow-up-migration`, or intentionally deferred.

## Common mistakes

- Treating GitHub Actions Dependabot PRs as outside dependency roll scope.
- Coupling dependency or Dependabot decisions to GitHub labels instead of stable PR metadata.
- Treating `actionlint` as proof that an action major is behavior-compatible.
- Reconciling Dependabot before manifests and lockfiles are consistent.
- Stacking unrelated fix-forward changes after a verification failure.

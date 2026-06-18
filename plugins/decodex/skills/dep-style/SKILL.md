---
name: dep-style
description: Use when the task is to normalize dependency constraint style, GitHub Actions SHA pinning policy, manifest entry shape, or repo-wide version-spec policy without conflating that work with a live dependency roll.
---

# Dep Style

## Objective

Normalize dependency constraint style, GitHub Actions pinning policy, manifest entry shape, or repo-wide version-spec policy without widening the task into a dependency roll, lockfile churn, workflow redesign, or source migration.

## When to use

- You are cleaning up dependency specifier style in existing manifests.
- You are aligning manifests to a checked-in repo policy for version operators, entry shape, or dependency-table form.
- You need to remove accidental pins, mixed shorthand, or inconsistent dependency-entry structure without changing the intended dependency graph.
- You are normalizing GitHub Actions external action or reusable-workflow refs to the repo policy of full-length commit SHA pins.
- A dependency roll exposed manifest-style drift that should be handled in a separate lane.

## Do not use

- Live dependency upgrades whose goal is the newest verified compatible set. Use `dep-roll`.
- API migrations, source compatibility work, or follow-up majors that need code changes.
- GitHub Actions action-version selection, Dependabot reconciliation, or choosing the latest compatible release/tag SHA. Use `dep-roll`.
- Lockfile-only refreshes or solver refreshes when manifest semantics are unchanged.

## Decision rule

Use this skill only when the task is really about manifest policy or constraint-shape cleanup.

- If the primary question is "what versions should we move to and what still verifies," use `dep-roll`.
- If the primary question is "how should these dependency requirements be written or normalized," use this skill.
- If the primary question is "should GitHub Actions refs be pinned by full SHA instead of tags or branches," use this skill.
- If both are needed, land the upgrade first and handle broad style cleanup in a separate normalization lane unless the repo already treats them as one atomic policy.

## Inputs

- The touched manifest files.
- Any checked-in dependency policy, bootstrap docs, CI, or repo-local style authority.
- The smallest current dependency slice whose constraint shape is actually in scope.

## Scope discipline

- Preserve the existing dependency graph unless the user explicitly widens scope.
- Normalize the smallest manifest surface that actually needs cleanup.
- If the repository has no clear checked-in policy and the current tree is mixed, prefer local consistency in the touched area over a repo-wide rewrite.
- If normalization would force large lockfile churn or a source migration, stop and split the work.
- For GitHub Actions, normalize only external `uses: owner/action@ref` and external reusable workflow `uses: owner/repo/.github/workflows/file.yml@ref` entries. Local actions and local reusable workflows such as `uses: ./.github/actions/foo` or `uses: ./.github/workflows/foo.yml` are repository code, not external dependency pins.
- The repo's preferred GitHub Actions style is full-length commit SHA pins for external refs. Convert tags, major tags, or branch/channel refs to the commit SHA that corresponds to the same selected release/tag when doing style-only cleanup; do not silently roll to a newer version in `dep-style`.

## What this skill owns

- Choosing and applying the checked-in manifest style for dependency requirements.
- Removing accidental patch pins or inconsistent operator usage when the repo policy actually calls for that cleanup.
- Converting dependency-entry shape, such as shorthand versus inline tables, only when a checked-in policy or dominant local convention makes the target clear.
- Applying the checked-in GitHub Actions full-SHA pinning style to already selected external action or reusable-workflow refs.
- Recording exceptions when a package must stay pinned or structurally different.

## What this skill does not own

- Picking the newest compatible versions.
- Dependabot reconciliation.
- Bundle-level upgrade strategy.
- Resolving a GitHub Actions roll target to the latest compatible release/tag SHA.
- API adaptation or test repair for incompatible majors.

## Workflow

1. Inventory the touched manifests and the current local style.
2. Read checked-in authority first: docs, CI, repo-local bootstrap, or existing dominant manifest conventions.
3. Decide whether a clear normalization target exists.
4. If the target is clear, normalize only the in-scope manifest lines or tables.
5. If the target is unclear, preserve the current touched style and report the ambiguity instead of inventing a repo-wide standard.
6. For GitHub Actions SHA normalization, resolve each existing selected tag or branch/channel to a full commit SHA from the action repository, record the source ref as provenance, and verify the SHA belongs to the upstream action repository rather than a fork. For annotated tags, pin the peeled commit SHA, not the tag-object SHA.
7. Regenerate lockfiles only when manifest semantics changed and the repository expects lock sync. GitHub Actions `uses:` refs have no lockfile regeneration step.
8. Run the smallest repo-native verification needed to confirm the manifest or workflow still parses, resolves, and satisfies policy.

## Outputs

- List the manifest files normalized.
- Name the checked-in policy or local convention that established the target style.
- Call out any intentional exceptions that stayed pinned or structurally different.
- Report whether lockfiles changed as a consequence of semantic manifest changes or stayed untouched.
- For GitHub Actions, list each external `uses:` ref normalized to full SHA and include the source release/tag or branch/channel provenance used to resolve that SHA.

## Common mistakes

- Folding broad manifest cleanup into a dependency roll that was supposed to stay focused on upgrade-and-verify.
- Inventing a new repo-wide dependency style when the repository has no checked-in authority for it.
- Rewriting untouched manifests just to make the repo look uniform.
- Refreshing lockfiles when the manifest semantics did not actually change.
- Treating a GitHub Actions SHA-normalization lane as permission to upgrade to a newer release. New target selection belongs in `dep-roll`.
- Pinning an annotated tag object's SHA instead of the commit SHA it points to.

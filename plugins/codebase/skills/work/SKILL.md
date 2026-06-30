---
name: work
description: Use when repository code work needs command authority, module boundaries, task runners, config contracts, dependency policy, validation, subagent boundaries, or completion evidence.
---

# Codebase Work

Use this first for repository work. It chooses checked-in authority, scope,
validation, and the narrow owner skill. Read `../../references/codebase.md` when
command authority, task-runner structure, module boundaries, dependency policy,
validation, landing, or final evidence matters.

## Route

- Docs, knowledge, semantic drift, or writeback: follow the checked-in docs/knowledge
  owner.
- Dependency rolls, manifest style, lockfiles, generated dependency artifacts,
  Dependabot, or GitHub Actions SHA pins: `$codebase:dependency-policy`.
- Review feedback or repair: `$codebase:review-feedback`.
- Root-cause investigation or repeated failed fixes: `$codebase:debugging`.
- Done/fixed/ready/landed/verified claims: `$codebase:verification`.
- Skeptic review: `$deliberation:skeptic`.
- Runtime ops, trackers, commit, landing, or research promotion: use that workflow's
  owner; codebase does not create lifecycle authority.

`AGENTS.md` should point here; this plugin owns the fanout.

## Gate

- Intake: classify bug, review feedback, failure, regression, drift, or research
  before editing.
- Implementation: follow checked-in tooling, docs, dependency, module-boundary, and
  owner-design rules for the touched surface.
- Command surfaces: keep one canonical spelling; command aliases are not allowed.
- Pre-claim: run drift/writeback when public claims may diverge, then verification
  before any positive status claim.
- Commit/landing: use the repository's owning authority.

## Subagents

Do not rely on static role config or wait for the user to ask. For design,
architecture, refactor, root-cause debugging, repeated failed fixes, review repair,
large/generated implementation, public contracts, option comparison, or important
ready/done claims, use the deliberation gate:

- `$deliberation:grill` for first-principles framing.
- `$deliberation:scout` for non-obvious evidence.
- `$deliberation:skeptic` before material conclusions.

Inline deliberation is only for one local question answerable from 1-2 files or one
command that cannot affect architecture, review repair, root cause, public contracts,
docs drift, commit/land, or ready/done claims. Otherwise dispatch bounded read-only
scout or skeptic subagents when tools allow; the main thread owns implementation and
final claims.

## Output

Report checked-in authority, task-runner status when relevant, validation scope,
drift/debugging/plugin-eval usage when relevant, and fresh evidence or honest gaps
for done/fixed/ready claims.

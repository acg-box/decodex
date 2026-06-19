---
name: repo-work
description: Use when repository work must follow checked-in command authority, task-runner structure, configuration contracts, architecture/cutover defaults, dependency policy, review repair, validation evidence, dynamic read-only support-agent boundaries, or completion reporting.
---

# Decodex Repo Work

Use this skill before narrower task-specific rules. It selects checked-in authority,
scope, workflow gates, validation scope, and evidence for status claims.

Read `../../references/repo-workflow.md` when command authority, task-runner
structure, engineering defaults, dependency policy, validation, landing authority, or
final evidence reporting matters.

## Route

- Repository docs structure or lane moves: follow checked-in docs router/policy.
- Dependency upgrades or Dependabot: `$decodex:dep-roll`.
- Dependency style, manifest shape, or GitHub Actions SHA pinning policy:
  `$decodex:dep-style`.
- Incoming review feedback or review repair: `$decodex:review-feedback`.
- Done/fixed/ready/landed/verified claims: `$decodex:verification`.
- Research and challenge: `$decodex:research` and `$decodex:challenge`.
- Semantic drift audits: `$decodex:docs-drift`.
- Root-cause investigation or repeated failed fixes: `$decodex:debugging`.
- Runtime ops, tracker state, labels, commit, or landing: use the repository's owning
  workflow; repo-work does not own those lifecycle surfaces.

Do not duplicate this routing in host bootstrap files. `AGENTS.md` should point to the
owner skill; the plugin owns the fanout.

## Gate Order

- Intake: classify review feedback, bugs, failures, regressions, drift, or research
  before editing.
- Implementation: follow checked-in language/tooling authority, dependency policy,
  docs policy, and owning design workflow for the touched surface.
- Pre-claim: use drift when docs/help/status/config/runtime claims can diverge, then
  verification before any positive status claim.
- Commit/landing: use the repository's owning authority.

## Dynamic Support Agents

Do not rely on configured static support-agent roles. Dynamically spawn read-only
support agents only for one explicit evidence, analysis, or challenge objective.
Provide task-local context, read-only boundary, and expected output shape. The main
thread keeps implementation ownership, checks their evidence, and owns final claims.

## Core Rules

- Checked-in project config, documented bootstrap, and CI commands beat personal
  defaults.
- Keep scope to the minimal affected surface and use the lightest workflow that
  satisfies checked-in expectations.
- Use `../../references/repo-workflow.md` for task-runner rules, engineering defaults,
  semantic naming, config contracts, plugin-eval gates, and evidence reporting.
- Stop when configured merge, landing, identity, or tool authority is unavailable or
  ambiguous.

## Output

Report checked-in authority, task-runner gate status, validation scope rationale,
drift/debugging/plugin-eval usage when relevant, and fresh evidence or honest gaps for
done/fixed/ready claims.

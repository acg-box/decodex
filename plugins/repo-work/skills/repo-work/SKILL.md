---
name: repo-work
description: Use when repository work must follow checked-in command authority, task-runner structure, configuration contracts, architecture/cutover defaults, dependency policy, review repair, validation evidence, dynamic read-only support-agent boundaries, or completion reporting.
---

# Repo Work

Use this skill before narrower task-specific rules. It selects checked-in authority,
scope, workflow gates, validation scope, and evidence for status claims.

Read `../../references/repo-work.md` when command authority, task-runner structure,
dependency policy, validation, landing authority, or final evidence reporting matters.

## Route

- Repository docs, knowledge, or semantic drift: follow the checked-in docs or
  knowledge owner.
- Dependency upgrades, Dependabot, manifest style, lockfile/generated dependency
  artifacts, or GitHub Actions SHA pinning: `$repo-work:dependency-policy`.
- Incoming review feedback or review repair: `$repo-work:review-feedback`.
- Done/fixed/ready/landed/verified claims: `$repo-work:verification`.
- Root-cause investigation or repeated failed fixes: `$repo-work:debugging`.
- Challenge or skeptic review: `$agent-method:challenge`.
- Runtime ops, tracker state, labels, commit, landing, or research promotion: use the
  repository's owning workflow; repo-work does not own those lifecycle surfaces.

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
- Use `../../references/repo-work.md` for task-runner rules, semantic naming, config
  contracts, plugin-eval gates, and evidence reporting.
- Stop when configured merge, landing, identity, or tool authority is unavailable or
  ambiguous.

## Output

Report checked-in authority, task-runner gate status, validation scope rationale,
drift/debugging/plugin-eval usage when relevant, and fresh evidence or honest gaps for
done/fixed/ready claims.

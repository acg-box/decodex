---
name: work
description: Use when repository code work needs command authority, module boundaries, task runners, config contracts, dependency policy, validation, support-agent boundaries, or completion evidence.
---

# Codebase Work

Use this skill before narrower task-specific rules. It selects checked-in authority,
scope, workflow gates, validation scope, and evidence for status claims.

Read `../../references/codebase.md` when command authority, task-runner structure,
dependency policy, validation, landing authority, or final evidence reporting matters.

## Route

- Repository docs, knowledge, or semantic drift: follow the checked-in docs or
  knowledge owner.
- Dependency upgrades, Dependabot, manifest style, lockfile/generated dependency
  artifacts, or GitHub Actions SHA pinning: `$codebase:dependency-policy`.
- Incoming review feedback or review repair: `$codebase:review-feedback`.
- Done/fixed/ready/landed/verified claims: `$codebase:verification`.
- Root-cause investigation or repeated failed fixes: `$codebase:debugging`.
- Challenge or skeptic review: `$deliberation:challenge`.
- Runtime ops, tracker state, labels, commit, landing, or research promotion: use the
  repository's owning workflow; codebase does not own those lifecycle surfaces.

Do not duplicate this routing in host bootstrap files. `AGENTS.md` should point to the
owner skill; the plugin owns the fanout.

## Gate Order

- Intake: classify review feedback, bugs, failures, regressions, drift, or research
  before editing.
- Implementation: follow checked-in language/tooling authority, dependency policy,
  docs policy, implementation-structure defaults, and owning design workflow for the
  touched surface.
- Pre-claim: use drift when docs/help/status/config/runtime claims can diverge, then
  verification before any positive status claim.
- Commit/landing: use the repository's owning authority.

## Dynamic Support Agents

Do not rely on configured static support-agent roles, and do not wait for the user to
explicitly ask for subagents. For design, architecture, refactor, root-cause
debugging, repeated failed fixes, review repair, large/generated implementation,
public-contract changes, option comparison, or important ready/done claims, use the
deliberation gate: `$deliberation:grill` for first-principles framing,
`$deliberation:scout` for non-obvious evidence, and `$deliberation:challenge` before
material conclusions.

Inline deliberation is allowed only for one explicit local question that fits in 1-2
files or one command and cannot affect architecture, review repair, root-cause
debugging, public contracts, docs drift, commit/land, or ready/done claims. When the
inline exception does not apply and support-agent tools are allowed, dynamically spawn
read-only support agents for one explicit evidence, analysis, scout/search, or
skeptic/challenge objective. The main thread keeps implementation ownership, checks
their evidence, and owns final claims. Give support agents bounded read-only context,
forbid mutation and further delegation unless the main thread explicitly requests it,
and name an inline fallback when support-agent tooling is unavailable.

## Core Rules

- Checked-in project config, documented bootstrap, and CI commands beat personal
  defaults.
- Keep scope to the minimal affected surface and use the lightest workflow that
  satisfies checked-in expectations.
- Use `../../references/codebase.md` for task-runner rules, implementation
  structure, semantic naming, config contracts, plugin-eval gates, and evidence
  reporting.
- Stop when configured merge, landing, identity, or tool authority is unavailable or
  ambiguous.

## Output

Report checked-in authority, task-runner gate status, validation scope rationale,
drift/debugging/plugin-eval usage when relevant, and fresh evidence or honest gaps for
done/fixed/ready claims.

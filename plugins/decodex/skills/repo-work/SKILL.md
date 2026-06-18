---
name: repo-work
description: Use when repository work must follow checked-in tool authority, task-runner structure, configuration contracts, architecture and cutover defaults, language or dependency policy, review repair, validation evidence, dynamic read-only support-agent boundaries, or completion reporting.
---

# Decodex Repo Work

## Goal

Apply the repository's common working principles before narrower task-specific rules.
Use this skill to select the authoritative checked-in surface, narrowest valid scope,
workflow gate, validation scope, and evidence needed for any status claim.

For the full policy map, read `../../references/repo-workflow.md` when command
authority, task-runner structure, validation scope, landing authority, or final
evidence reporting matters.

## Narrower Skill Routing

After loading this skill for repository work, route to narrower skills or checked-in
repo authority when the task surface calls for them:

- For repository docs structure, taxonomy, routing, placement, or lane moves, follow
  the checked-in docs router and policy first.
- Use `$decodex:rust` for Rust implementation, refactors, reviews, crate/tooling
  choices, or Rust dependency decisions.
- Use `$decodex:python` for Python code, scripts, `pyproject.toml`, Poetry,
  virtualenv/bootstrap, or Python lint/type-check/test tooling.
- Use `$decodex:dep-roll` for deliberate dependency upgrades, package bumps, lock
  regeneration tied to a real upgrade, or Dependabot reconciliation.
- Use `$decodex:dep-style` for dependency constraint normalization, GitHub Actions
  SHA pinning policy, manifest-style cleanup, or version-spec policy work that should
  stay separate from a live dependency roll.
- Use `$decodex:review-feedback` when receiving, triaging, validating, or repairing
  PR review feedback, review threads, review summaries, CI/review bot output, Linear
  review comments, or user-pasted external feedback.
- Use `$decodex:verification` before claiming work is done, fixed, passing, ready,
  verified, landed, closed out, or otherwise complete.
- Use the repository's owning workflow for commit creation, PR landing, labels,
  tracker state, or runtime automation when those surfaces are outside repo-work.
- Use `$decodex:research` and `$decodex:research-challenge` for bounded research,
  decision-ready comparison work, and challenge passes.
- Use `$decodex:docs-drift` for semantic drift audits.
- Use `$decodex:debugging` for root-cause investigation, repeated failed fixes, and
  original-symptom checks.

Do not duplicate this repo-work routing in host bootstrap files. Host-level
`AGENTS.md` should route repository work here first, then this skill owns only
Decodex repo-work skill fanout.

## Workflow Gate Order

Do not rely on passive skill auto-triggering after this skill is loaded. At each
phase transition, check whether the current work needs a narrower gate:

- Intake: use `$decodex:review-feedback` for incoming review feedback. For bugs,
  failures, regressions, unexpected behavior, repeated failed fixes, or drift audits,
  route to the repository's owning diagnostic or drift workflow.
- Implementation: use the relevant language, dependency, checked-in docs policy, or
  owning design workflow only when the touched surface calls for it.
- Pre-claim: use the owning drift workflow when docs, help, status, telemetry, config,
  semantic naming, or runtime behavior can affect each other's truth claims; use
  `$decodex:verification` before saying done, fixed, passing, ready, landed, closed
  out, or verified.
- Commit and landing: use the repository's owning commit or landing authority; this
  skill does not create commits or land pull requests.

## Dynamic Support Agents

Do not rely on configured static support-agent roles. When a bounded support pass is
useful, dynamically spawn a read-only subagent for one explicit evidence, analysis, or
challenge objective.

- Keep implementation ownership on the main thread.
- Give the subagent only the necessary task-local context and expected output shape.
- Require read-only behavior unless the user explicitly requested parallel
  implementation in an isolated context.
- Use support agents for bounded evidence gathering, option analysis, or claim
  challenge; do not let them create commits, land PRs, mutate trackers, or become the
  workflow owner.
- Merge their findings only after the main thread checks the relevant diff or evidence.

## Core Rules

- Treat checked-in project config, documented bootstrap, and existing CI commands as
  authority over personal defaults.
- Keep the task scoped to the minimal affected surface and use the lightest workflow
  that satisfies the touched project's checked-in expectations.
- Select validation by touched surface and risk; do not invent repository-wide
  requirements or default gates that the current tree does not evidence.
- Treat task-runner structure, configuration contracts, architecture/cutover choices,
  and semantic naming as Decodex repo-work rules; read
  `../../references/repo-workflow.md` for exact policy.
- Treat research, research challenge, debugging/investigation methods, semantic drift
  audits, and support-agent challenge design as outside repo-work; route them to the
  Decodex skill that owns those methods.
- Run plugin evaluation before final claims for Codex skill or plugin changes.
- Treat local validation, remote CI, review handoff, landing, and closeout as
  separate lifecycle surfaces.
- Use the owning drift workflow when changed docs, help, config, telemetry, status, or
  runtime behavior can change each other's truth claims.
- Stop and report a blocker when the configured merge, landing, identity, or tool
  authority is unavailable or ambiguous.

## Outputs

Report the checked-in authority used, whether a task-runner gate applied, why
validation matched the risk, whether an owning drift workflow or plugin-eval was
needed, and which fresh evidence supports or limits any done/fixed/ready claim. Use
`../../references/repo-workflow.md` for the full evidence checklist.

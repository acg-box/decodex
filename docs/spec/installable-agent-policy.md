---
type: "Spec"
title: "Installable Agent Policy Boundary"
description: "Define the boundary between installable Codex `AGENTS.md` guidance and Decodex-owned repository, runtime, workflow, identity, and lifecycle policy. Status: normative Read this when: You are editing an installable or global `AGENTS.md` surface, moving agent-facing policy into Decodex, or deciding whether a rule belongs in a project `WORKFLOW.md`, a Decodex spec, a runbook, or a skill. Not this document: The runtime state machine, the `WORKFLOW.md` schema, the tracker tool schema, or local operator procedures. Defines: The copy-to-unrelated-repository standard, allowed global-agent rule shapes, forbidden Decodex-specific global content, and the Decodex destination for moved policy."
status: active
authority: normative
owner: runtime
tags: [spec]
last_verified: 2026-06-16
---
# Installable Agent Policy Boundary

Purpose: Define the boundary between installable Codex `AGENTS.md` guidance and
Decodex-owned repository, runtime, workflow, identity, and lifecycle policy.
Status: normative
Read this when: You are editing an installable or global `AGENTS.md` surface, moving
agent-facing policy into Decodex, or deciding whether a rule belongs in a project
`WORKFLOW.md`, a Decodex spec, a runbook, or a skill.
Not this document: The runtime state machine, the `WORKFLOW.md` schema, the tracker
tool schema, or local operator procedures.
Defines: The copy-to-unrelated-repository standard, allowed global-agent rule shapes,
forbidden Decodex-specific global content, and the Decodex destination for moved
policy.

## Boundary

- The installable `~/.codex/AGENTS.md` surface is a cross-repository bootstrap
  surface. A rule belongs there only when it remains correct after being copied into
  an unrelated repository that does not run Decodex, does not use the hack-ink
  identity map, and does not share local worktree or Linear conventions.
- Decodex-specific runtime, tracker, identity, review, landing, closeout, and
  cleanup policy must live in Decodex-owned surfaces: `apps/decodex/src/`,
  `docs/spec/`, the
  registered project `WORKFLOW.md`, project `project.toml`, repo-local runbooks, or
  the Decodex plugin skill that owns a reusable method.
- Repo-local skills under `automations/*/skills/` are not installable global skills.
  They may be referenced by repo-owned automation prompts and runbooks, but they must
  not be copied into `$CODEX_HOME/skills`. Use
  `scripts/config/sync_installable_plugins.py --apply --clean-repo-local-skills` to
  sync installable `plugins/*` and remove exact-copy global mistakes.
- Global agent guidance may point agents toward repository-declared policy, but it
  must not become the source of truth for a repository gate, tracker state name,
  token environment variable, Linear label, branch layout, review loop, merge method,
  or retained-lane lifecycle phase.
- If a rule needs a local path, local skill name, tracker tool name, workflow state,
  token variable, or organization routing table to be correct, keep that rule out of
  the installable `AGENTS.md` surface unless the wording is rewritten to be generic.

## Allowed global guidance

| Concern | Allowed installable shape | Decodex-specific details that must move out |
| --- | --- | --- |
| Acting posture | Prefer a short local probe before asking when the probe is cheap and low-risk. | Decodex retry budgets, Linear writeback sequence, or retained-lane state names. |
| Implementation ownership | Keep implementation on the main thread by default; dynamically spawn subagents only for bounded read-only support. | Static subagent profile TOML, denylist payloads, or child-run execution overrides. |
| Task decomposition | Keep decomposition in-agent unless the user asks for a separate planning artifact. | Durable execution-state checkpoints or Linear ledger mechanics. |
| Skills and capabilities | Use available repository or plugin capability routing when it exists. | A fixed Decodex workflow expressed through local skill names such as retired review, repair, landing, or closeout helpers. |
| Working context | Use an isolated task context when the repository or toolchain declares one. | Hardcoded `.worktrees/<ISSUE>` layouts or Decodex lane cleanup rules. |
| Identity | Derive external-service identity from repo, project, or tool configuration; stop when required identity is missing or contradictory. | Person-to-token maps, workspace names, fallback identities, and exact `GITHUB_*` or `LINEAR_*` variables. |
| Validation | Run the repository-declared gate before review handoff, PR-head refresh, or branch-state mutation. | `WORKFLOW.md` frontmatter keys, command lists, or gate-profile selection semantics. |
| Review | Review the current head before handoff or merge and repair verified findings. | Decodex bounded review checkpoint tools, review-round accounting, GitHub Review signals, or landing-entry rules. |
| Commit messages | Follow the repository's declared commit-message contract. | The `decodex/commit/2` schema when the target repository does not declare it. |
| Change control | Do not overwrite unrelated local changes; stop when ownership is unclear. | Retained-lane reconciliation, recovery worktree classification, or closeout cleanup policy. |

## Decodex policy destinations

| Policy moved out of global `AGENTS.md` | Authoritative Decodex destination |
| --- | --- |
| Issue-scoped tracker writes, execution-state checkpoints, terminal finalization, and Linear progress comments | [`tracker-tools.md`](./tracker-tools.md), [`runtime.md`](./runtime.md), and [`linear-execution-ledger.md`](./linear-execution-ledger.md) |
| Project execution gates, canonicalization and verification commands, gate profiles, and workspace hooks | [`workflow-file.md`](./workflow-file.md) plus the registered project `WORKFLOW.md` |
| Service identity, repo root, worktree root, and tracker or GitHub credential environment-variable names | Centralized project `project.toml`; see the operator surface map in [`../reference/operator-control-plane.md`](../reference/operator-control-plane.md) |
| Automatic intake labels, active ownership, retry behavior, and retained lane planning | [`runtime.md`](./runtime.md) and [`owned-lane-policy.md`](./owned-lane-policy.md) |
| Review handoff, bounded independent review, GitHub Review pass signals, repair rounds, and architecture escalation | [`review-orchestration.md`](./review-orchestration.md) and the registered project `WORKFLOW.md` bounded review method |
| Post-`In Review` waiting, repair, landing, closeout, cleanup, and manual-intervention phases | [`post-review-lifecycle.md`](./post-review-lifecycle.md) |
| Local commit-message schema for Decodex-managed history | [`commit-messages.md`](./commit-messages.md) |
| Operator procedures, pilot setup, and live validation steps | [`../runbook/index.md`](../runbook/index.md) and the specific runbook for the procedure |
| Reusable method instructions for installable Decodex usage | The relevant `SKILL.md` under `plugins/decodex/skills/`; the global `AGENTS.md` may route to the skill but must not duplicate its mechanics |

## Rewrite rules

1. Apply the copy-to-unrelated-repository standard first. If the rule only makes sense
   in a Decodex project, remove it from global `AGENTS.md` and link to the Decodex
   owner instead.
2. Keep generic method in global wording only after removing local names, path
   assumptions, token names, tracker states, and lifecycle terms that are not portable.
3. Do not use global `AGENTS.md` to restate `WORKFLOW.md` frontmatter fields or
   command lists. The repository gate is declared by the registered project workflow.
4. Do not use global `AGENTS.md` to restate Decodex tracker tool contracts. Tool
   availability and completion semantics belong to the runtime and tracker-tool specs.
5. Do not use global `AGENTS.md` to carry organization identity routing. Project
   configuration and operator docs own service credentials and workspace identity.
6. When removing global policy text, update the owning Decodex spec, project
   `WORKFLOW.md`, runbook, or `plugins/decodex` skill in the same lane if that policy
   would otherwise lose its authoritative home.
7. Link from the global surface to the owning repository document when a short pointer
   is useful; do not copy the repository policy back into global prose.

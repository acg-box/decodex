---
name: decodex
description: Use when routing Decodex work.
---

# Decodex

Route Decodex work to the narrowest surface. Read `../../references/routing.md` when
research, promotion, planning, labels, runtime, commit, or landing boundaries matter.
Read `../../references/context-gates.md` when Decodex repository work needs OKF/LLM
Wiki context intake, docs completion recovery, or repo-memory gate handling.

- `research`: bounded investigation before execution.
- `okf`, `okf-query`, `okf-maintain`, `repo-memory-writer`,
  `repo-memory-evaluator`, `repo-memory-curator`: portable OKF/LLM Wiki and
  repo-memory work.
- `context-gates`: Decodex-owned OKF route intake, Context anchors, docs completion
  gates, and late docs-skill recovery.
- `research-promote`: explicit acceptance of latent research.
- `planning`: accepted work needs issues or Program readiness.
- `manual-cli`: a human drives local commands.
- `automation`: retained lanes, Program Intake, recovery, closeout.
- `labels`, `commit`, `land`: only their narrow surfaces.

When an MCP client is available, use the Decodex MCP gateway as a typed facade for
resources, prompts, and the deliberately small tool catalog. Prefer stdio for local
clients and Streamable HTTP only for remote permitted clients behind the operator's
chosen local listener, tunnel, or relay. MCP tools do not bypass Decision Contract,
lane-control, review, landing, tracker, or runtime authority gates. Route MCP planning
through `research_compile`, `research_promote`, and `intake_goal`; dry-run modes stay
non-mutating, and apply/promote modes require explicit authority fields and structured
refusal when authority is missing. Route MCP remote control through `decodex_observe`,
`decodex_lane_control`, and `decodex_project_control`: observe is public-safe
readback, lane control is inspect-first with current run/turn preconditions, and
project control is future-dispatch-only for pause/resume.

Research is latent until promoted. Program Intake is not queue-label polling.
Decodex-owned landing uses `decodex land`, not raw GitHub merge paths.

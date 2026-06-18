---
name: decodex
description: Use when routing Decodex work.
---

# Decodex

Route Decodex work to the narrowest surface. Read `../../references/routing.md` when
repo-work, research, promotion, planning, context intake, labels, runtime, commit, or
landing boundaries matter.

- `repo-work`: checked-in repository authority, task-runner structure, configuration
  contracts, architecture/cutover defaults, language and dependency policy, review
  repair, validation evidence, and dynamic support-agent boundaries.
- `rust`, `python`, `dep-roll`, `dep-style`: language and dependency policy beneath
  repo-work.
- `review-feedback`, `verification`: review repair and evidence before done/fixed/
  ready claims.
- `research`: bounded investigation before execution.
- `debugging`: root-cause investigation for bugs, failures, runtime regressions, and
  repeated failed fixes.
- `docs-drift`: Decodex semantic-drift audits for docs, code, commands, config,
  runtime readback, and evidence alignment.
- `okf`, `okf-query`, `okf-maintain`, `repo-memory-writer`,
  `repo-memory-evaluator`, `repo-memory-curator`: portable OKF/LLM Wiki and
  repo-memory work.
- `research-promote`: explicit acceptance of latent research.
- `planning`: accepted work needs issues or Program readiness.
- `manual-cli`: a human drives local commands.
- `automation`: retained lanes, Program Intake, recovery, closeout.
- `labels`, `commit`, `land`: only their narrow surfaces.

When an MCP client is available, use the Decodex MCP gateway as a typed facade for
resources, prompts, and the deliberately small tool catalog. Prefer stdio for local
clients and Streamable HTTP only for remote permitted clients behind the operator's
chosen local listener plus `--bearer-token-env`, tunnel, relay, network ACL, reverse
proxy, or protected-resource auth boundary. Treat `--allow-origin` as CORS trust, not
authentication; direct non-loopback listeners require both `--allow-origin` and
`--bearer-token-env`, and Streamable HTTP profiles above `observe` require
`--bearer-token-env`. The built-in bearer guard is not OAuth Protected Resource
Metadata. MCP tools do not bypass Decision Contract, lane-control, review, landing,
tracker, or runtime authority gates. Route MCP planning through `research_compile`,
`research_promote`, and `intake_goal`; dry-run modes stay
non-mutating, and apply/promote modes require explicit authority fields and structured
refusal when authority is missing. Route MCP remote control through `decodex_observe`,
`decodex_lane_control`, and `decodex_project_control`: observe is public-safe
readback, lane control is inspect-first with current run/turn preconditions, and
project control is future-dispatch-only for pause/resume.

Research is latent until promoted. Program Intake is not queue-label polling.
Decodex-owned landing uses `decodex land`, not raw GitHub merge paths.

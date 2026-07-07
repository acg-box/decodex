---
name: decodex
description: Use when routing Decodex work.
---

# Decodex

Route Decodex work to the narrowest Decodex-owned surface. Read
`../../references/routing.md` when planning, runtime ops,
commit, or landing boundaries matter.

- `planning`: accepted Decodex work needs issues or Program readiness.
- `decodex-ops`: runtime operations, retained automation, human-driven CLI, labels,
  intake, recovery, and lane control.
- `commit`, `land`: only their narrow high-risk authority surfaces.

Companion plugin routing:

- Repository command authority, task-runner structure, review repair, verification,
  dependency policy, and debugging belong to the external installed `codebase` plugin.
- Repo knowledge belongs to the external installed `knowledge` plugin.
- Research, frame, scout, and skeptic work belongs to the external installed `research`
  plugin.

For MCP gateway, runtime, tracker, or control-plane work, read
`../../references/routing.md` before choosing tools. MCP is a typed facade; it does
not bypass Decision Contract, lane-control, review, landing, tracker, or runtime
authority gates.

For lane lifecycle architecture, treat the orchestration kernel as the runtime
authority boundary. Read `docs/runbook/orchestration-kernel-cutover.md` with
`docs/spec/owned-lane-policy.md`, `docs/spec/lane-control-state.md`, and
`docs/spec/post-review-lifecycle.md` when work touches scheduler, retry,
post-review, queue, status, lane-control, or operator readback decisions. Runtime
surfaces may collect facts, execute command intents, or render projections; they
must not reintroduce independent lifecycle policy branches.

For autonomy work, route to `decodex://docs/spec/autonomy-control-plane`,
`decodex://docs/decisions/mcp-capability-gateway-and-skill-slimming`, and the
capability-profiled MCP surface: observe reads `decodex://projects/{service_id}/autonomy`
summaries, while plan may use `autonomy_draft_objective`,
`autonomy_accept_objective`, `autonomy_submit_signal`,
`autonomy_compile_proposal`, `autonomy_challenge_proposal`, and
`autonomy_request_promotion`. Auth and profile prove access only; Objective Contract
acceptance and proposal acceptance still require explicit human or accepted
project-policy authority resolved from trusted Decodex state, not a caller-supplied
policy body.

Program Intake is not queue-label polling. Decodex-owned landing uses `decodex land`,
not raw GitHub merge paths.

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

For lane lifecycle architecture, treat the lifecycle kernel as the runtime authority
boundary. Read `openwiki/architecture/runtime-architecture.md`,
`openwiki/workflows/runtime-operator-workflows.md`, and
`openwiki/specs/contracts-and-data.md` when work touches scheduler, retry, post-review,
queue, status, lane-control, landing, closeout, or operator readback decisions.
Runtime surfaces may collect structured facts, execute command intents, or render
projections; final post-review lifecycle states must come from the pure kernel and be
persisted by the runtime state adapter as `decodex/lifecycle-authority-record/1` plus a
`decodex/lifecycle-event/1` envelope.

For autonomy work, route to `decodex://openwiki/specs/contracts-and-data` and the
capability-profiled MCP surface: observe reads `decodex://projects/{service_id}/autonomy`
summaries, while plan may use `autonomy_draft_objective`,
`autonomy_accept_objective`, `autonomy_submit_signal`,
`autonomy_compile_proposal`, `autonomy_challenge_proposal`, and
`autonomy_request_promotion`. Auth and profile prove access only; Objective Contract
acceptance and proposal acceptance still require explicit human or accepted
project-policy authority resolved from trusted Decodex state, not a caller-supplied
policy body.

Program Intake is not queue-label polling. Decodex-owned landing uses `decodex land`,
not raw GitHub merge paths. Issue-authority manual landing enters lifecycle authority;
non-issue `--manual-authority --pr` remains the local receipt exception.

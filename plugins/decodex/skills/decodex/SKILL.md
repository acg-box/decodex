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

For MCP gateway, runtime, tracker, or control-plane work, read
`../../references/routing.md` before choosing tools. MCP is a typed facade; it does
not bypass Decision Contract, lane-control, review, landing, tracker, or runtime
authority gates.

For lane lifecycle architecture, treat the lifecycle kernel as the runtime authority
boundary when work touches scheduler, retry, post-review, queue, status, lane-control,
landing, closeout, or operator readback decisions.
Runtime surfaces may collect structured facts, execute command intents, or render
projections; final post-review lifecycle states must come from the pure kernel and be
persisted by the runtime state adapter as `decodex/lifecycle-authority-record/1` plus a
`decodex/lifecycle-event/1` envelope.

For autonomy work, use the capability-profiled MCP surface: observe reads
`decodex://projects/{service_id}/autonomy` summaries, while plan may use
`autonomy_draft_objective`,
`autonomy_accept_objective`, `autonomy_submit_signal`,
`autonomy_compile_proposal`, `autonomy_challenge_proposal`, and
`autonomy_request_promotion`; accepted-policy automation additionally uses
`autonomy_accept_runtime_policy` and `autonomy_apply_runtime_policy`. Auth and profile prove access only; Objective Contract
acceptance and proposal acceptance still require explicit human or accepted
project-policy authority resolved from trusted Decodex state, not a caller-supplied
policy body. Runtime-policy acceptance requires the interactive
`decodex project accept-runtime-policy` operator ceremony. It binds the OS-resolved
principal, exact Objective digest, public non-goals, and typed candidate-digest
confirmation into a server-side single-use 10-minute receipt that the CLI atomically
consumes. MCP may preview but cannot perform acceptance, including through default
Admin stdio. `autonomy_apply_runtime_policy` runs a
Decodex-internal challenge and may promote only an exact accepted-Objective proposal;
it never invokes Program Intake. Use `intake_goal` dry-run and apply as separate calls.
Decodex derives one canonical claim per contract: `prepared` is retry-safe,
`started` is externally uncertain and must stop automatic retries, and `completed`
is terminal. The claim binds project/config/workflow/team-anchor inputs and current
proposal objections are rechecked before intake. `retry-prepared` performs the exact
bound retry. Use `decodex intake recover` for typed inspection or reconciliation;
never edit the runtime database to clear a claim.

Program Intake is not queue-label polling. Decodex-owned landing uses `decodex land`,
not raw GitHub merge paths. Issue-authority manual landing enters lifecycle authority;
non-issue `--manual-authority --pr` remains the local receipt exception.

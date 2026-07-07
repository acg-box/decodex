# Decodex Routing Reference

Use when Decodex work crosses research, planning, ops, commit, or landing.

## Mode Map

- Context: use `$knowledge:docs`, `$knowledge:okf`, or `$knowledge:repo-memory`
  before non-trivial repo claims.
- Repo work: use `$codebase:work` for commands, config, dependencies, debugging,
  validation, completion claims, and subagent boundaries.
- Drift/writeback: use `$knowledge:docs-drift` or `$knowledge:writeback`.
- Research/design: use `research`; output is latent until promoted.
- Skeptic: use `$deliberation:skeptic`; it can block claims but creates no
  authority.
- Promotion: use `research-promote` only after explicit acceptance.
- Planning: use `planning` after promotion or explicit execution instruction.
- Ops: use `decodex-ops` for retained automation, manual CLI, tracker intake, labels,
  lane control, recovery, operator readback, and missing handoff diagnosis.
- Commit/land: use `commit` or `land` only for their narrow high-risk surfaces.

## First Reads

- This repo: `README.md`, `docs/index.md`, `docs/policy.md`, `Makefile.toml`.
- Projects: `project.toml` and `WORKFLOW.md`.
- Runtime: `docs/spec/` and `docs/runbook/`.
- Orchestration lifecycle: `docs/runbook/orchestration-kernel-cutover.md`,
  `docs/spec/owned-lane-policy.md`, `docs/spec/lane-control-state.md`, and
  `docs/spec/post-review-lifecycle.md`.

## Boundaries

- Research never queues work, mutates Linear, implements, sets goals, or dispatches
  runtime Program nodes; read-only scout/skeptic subagents are allowed by the
  research skill. Research is not runtime `compact_current_head_review`.
- Program Intake dispatches persisted Program nodes; queue labels are not scheduling.
- Ordinary intake starts from `decodex:queued:<service-id>` and still must pass
  workflow, terminal-state, dependency, opt-out, and active-lease checks.
- `decodex:active:<service-id>` is runtime ownership, not "please start work";
  `decodex:manual-only` opts out; `decodex:needs-attention` stops automation.
- Lane lifecycle policy belongs to the pure lifecycle kernel. Scheduler, retry,
  post-review, queue, status, dashboard, MCP, tracker, landing, closeout, and recovery
  surfaces are structured-fact collectors, command-intent executors, or compatibility
  projections after cutover. Final post-review state is the runtime state's
  `decodex/lifecycle-authority-record/1` projection plus append-only
  `decodex/lifecycle-event/1` envelope, not a tracker comment or local receipt.
- Use `decodex commit` for human-driven commits and `decodex land` for PR landing;
  diagnose missing review handoff before rebind/adopt recovery. Issue-authority
  manual landing enters lifecycle authority; non-issue `--manual-authority --pr` is
  the local receipt exception.
- MCP is a typed facade, not a bypass. Non-loopback Streamable HTTP requires origin
  plus bearer auth; profiles above `observe` require bearer auth.
- CORS is not authentication; typed plan tools and inspect-first operate/admin
  preconditions live in `docs/runbook/mcp-remote-control.md` and
  `docs/reference/operator-control-plane.md`.
- Do not use global `AGENTS.md` as Decodex runtime, tracker, identity, landing,
  closeout, or cleanup authority.
- Do not hand-edit runtime DB rows, hidden children, Linear state, labels, graph ids,
  DAG edges, hidden goals, or dispatch mechanics to simulate lifecycle controls.
- Do not bypass Decodex authority with MCP, GitHub UI, `gh pr merge`, merge queue,
  raw Git, direct API, or hand-assembled merge commits.

# Decodex Routing Reference

Use when Decodex work crosses runtime planning, ops, commit, or landing.

## Mode Map

- Generic repo work: use the external installed `codebase` plugin.
- Repo knowledge: use the external installed `knowledge` plugin.
- Research, frame, scout, and skeptic work: use the external installed `research`
  plugin.
- Decodex planning: use `planning` after explicit Decodex execution instruction
  or accepted project-policy authority.
- Ops: use `decodex-ops` for retained automation, manual CLI, tracker intake, labels,
  lane control, recovery, operator readback, and missing handoff diagnosis.
- Commit/land: use `commit` or `land` only for their narrow high-risk surfaces.

## First Reads

- This repo: `README.md`, `Makefile.toml`, and the relevant Decodex product docs.
- Projects: `project.toml` and `WORKFLOW.md`.
- Runtime: `docs/spec/` and `docs/runbook/`.
- Orchestration lifecycle: `docs/runbook/orchestration-kernel-cutover.md`,
  `docs/spec/owned-lane-policy.md`, `docs/spec/lane-control-state.md`, and
  `docs/spec/post-review-lifecycle.md`.

## Boundaries

- Program Intake dispatches persisted Program nodes; queue labels are not scheduling.
- Ordinary intake starts from `decodex:queued:<service-id>` and still must pass
  workflow, terminal-state, dependency, opt-out, and active-lease checks.
- `decodex:active:<service-id>` is runtime ownership, not "please start work";
  `decodex:manual-only` opts out; `decodex:needs-attention` stops automation.
- Lane lifecycle policy belongs to the typed orchestration kernel. Scheduler, retry,
  post-review, queue, status, dashboard, MCP, and tracker surfaces are fact
  collectors, command-intent executors, or compatibility projections after cutover.
- Use `decodex commit` for human-driven commits and `decodex land` for PR landing;
  diagnose missing review handoff before rebind/adopt recovery.
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

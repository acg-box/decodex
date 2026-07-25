# Decodex Routing Reference

Use when Decodex work crosses runtime planning, ops, commit, or landing.

## Mode Map

- Decodex planning: use `planning` after explicit Decodex execution instruction
  or accepted project-policy authority.
- Ops: use `decodex-ops` for retained automation, manual CLI, tracker intake, labels,
  lane control, recovery, operator readback, and missing handoff diagnosis.
- Commit/land: use `commit` or `land` only for their narrow high-risk surfaces.

## Authority Inputs

- Projects: registered `project.toml` and `WORKFLOW.md`.
- Runtime: typed Decodex status, diagnose, lane-control, MCP, and retained-state readback.
- Lifecycle: runtime lifecycle authority records and append-only lifecycle events.

## Boundaries

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
- The current `apps/decodex-cli` implements local manual-authority `commit` and
  exact-base/head `land` without Decodex server, planner, runtime, MCP, Linear, or
  tracker state. `status` and `doctor` remain server diagnostic commands. The
  standalone upstream automation can use only the local commit and land commands.
- MCP is a typed facade, not a bypass. Non-loopback Streamable HTTP requires origin
  plus bearer auth; profiles above `observe` require bearer auth.
- CORS is not authentication; typed plan tools and inspect-first operate/admin
  preconditions remain enforced by the MCP capability profile and runtime guards.
- Do not use global `AGENTS.md` as Decodex runtime, tracker, identity, landing,
  closeout, or cleanup authority.
- Do not hand-edit runtime DB rows, hidden children, Linear state, labels, graph ids,
  DAG edges, hidden goals, or dispatch mechanics to simulate lifecycle controls.
- Do not bypass Decodex authority with MCP, GitHub UI, `gh pr merge`, merge queue,
  raw Git, direct API, or hand-assembled merge commits.

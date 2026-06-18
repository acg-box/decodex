# Decodex Routing Reference

Use this when a Decodex task crosses docs, research, promotion, planning, CLI,
automation, commit, or landing boundaries.

## Mode Map

- Context intake: for non-trivial repo work, read the smallest owner from
  `docs/index.md`, `docs/policy.md`, lane indexes, or explicit concept links. Record
  context anchors only when those reads shape the plan. Skip tiny work and explicit
  file targets only when the owner is unambiguous. If docs impact appears late, run
  the same docs gate and report recovery evidence.
- Docs: use `docs` as the router. Use `docs-okf`, `docs-wiki`, and `docs-drift`
  for shape, navigation, and audits. Docs impact `research_required` switches to
  `research*`; checked-in `docs/research/` is latent evidence.
- Research/design: use `research` and phase skills. The compact loop is probe,
  evidence, options, judgment, challenge, decision. Results are latent Decision
  Contract candidates only.
- Promotion: use `research-promote` only after explicit acceptance such as "arrange
  this", "push this forward", "推进", or "做".
- Planning: use `planning` after promotion or another explicit execution instruction.
  Planning owns issue briefing and Program readiness.
- Automation: use `automation` for Decodex-owned intake, lanes, review handoff,
  repair, landing, closeout, cleanup, and recovery.
- Manual CLI: use `manual-cli` when a human drives status, registration, dry-run,
  recovery inspection, commit, or landing.
- Labels: use `labels` only for ordinary non-Program tracker intake and retained-lane
  ownership signals.
- MCP gateway: use stdio locally and Streamable HTTP only behind the operator's chosen
  listener, tunnel, relay, network ACL, or protected-resource auth boundary. Treat
  `--allow-origin` as CORS trust, not authentication; direct non-loopback or elevated
  HTTP profiles are not production-safe without authorization. MCP is a typed facade,
  not a bypass around Decision Contract, lane-control, tracker, review, landing, or
  closeout gates. Plan tools are `research_compile`, `research_promote`, and
  `intake_goal`; operate/admin remains inspect-first with current run/turn
  preconditions.

## First Reads

- In this repo, read `README.md`, `docs/index.md`, `docs/policy.md`, and
  `Makefile.toml` before docs or validation work.
- For registered projects, read the project `project.toml` and `WORKFLOW.md`.
- For runtime semantics, prefer `docs/spec/` and `docs/runbook/` over host policy.

## Natural-Language Research Routing

Keep Decodex natural-language-first. Requests such as `research X` route through
`research`, `research-probe`, `research-evidence`, `research-options`,
`research-judgment`, `research-challenge`, and `research-decision` before promotion.

Research never queues work, mutates Linear, starts implementation, creates Codex
goals, or dispatches Program nodes. Promotion preserves accepted objectives,
constraints, validation expectations, proposed issues, non-goals, and stop
conditions. A result is a latent Decision Contract candidate only.

## Program Versus Label Intake

- Program Intake dispatches ready mapped nodes directly from the persisted Execution
  Program; queue labels are not the Program scheduler.
- Ordinary issue intake starts from `decodex:queued:<service-id>` and must still pass
  `WORKFLOW.md` eligibility, terminal-state, dependency, opt-out, and active-lease
  checks.
- `decodex:active:<service-id>` is runtime ownership, not "please start work".
- `decodex:manual-only` opts out of automation.
- `decodex:needs-attention` is a human-required stop.

## Commit And Land

For human-driven commits, inspect the diff, stage intended files, run touched-surface
validation, then use `decodex commit`, adding `--manual-authority` for deliberate
non-issue work.

For human-driven PR landing, confirm PR/base/head/mergeability/checks, then use
`decodex land`; add `--manual-authority --pr <URL>` for non-issue work. If
issue-authority landing lacks retained handoff state, dry-run
`decodex recover review-handoff adopt` before any live adopt.

## Hard Boundaries

- Do not use global `AGENTS.md` as Decodex runtime, tracker, identity, landing,
  closeout, or cleanup authority.
- Do not route Decodex issue briefing through an external delivery workflow; planning
  owns Decodex issue shaping after execution authority exists.
- Do not hand-edit runtime DB rows, kill hidden `_attempt` children, or mutate Linear
  state to simulate lane controls.
- Do not use MCP tools to bypass Decision Contract promotion, lane-control
  preconditions, tracker tools, review handoff, landing, closeout, or runtime
  authority.
- Do not substitute raw GitHub merge, merge queue, `gh pr merge`, direct API mutation,
  or hand-assembled merge commits for `decodex land` when Decodex owns landing.
- Do not expose graph ids, DAG edge editing, hidden goal ids, or Program dispatch
  mechanics as the ordinary user workflow.

# Decodex Routing Reference

Use this reference when a Decodex task crosses research, planning, manual CLI,
labels, retained automation, commit, or landing boundaries.

## Mode Map

- Research/design: use `research` and the `research-*` phase skills. The result is a
  latent Decision Contract candidate only.
- Promotion: use `research-promote` only after explicit acceptance such as "arrange
  this", "push this forward", "推进", or "做".
- Planning: use `planning` after promotion or another explicit execution instruction
  to create Decodex-friendly issue slices and Program readiness.
- Manual CLI: use `manual-cli` when a human is driving local commands, status,
  project registration, dry-run checks, recovery inspection, commit, or land.
- Retained automation: use `automation` when Decodex owns issue intake, Program
  Intake, retained lanes, review handoff, repair, landing, closeout, cleanup, or
  operator recovery.
- Labels: use `labels` only for ordinary non-Program tracker intake and retained-lane
  ownership signals.

## First Reads

- In the Decodex repo, read `README.md`, `docs/index.md`, and `Makefile.toml` before
  repository validation.
- For registered projects, read `~/.codex/decodex/projects/<service-id>/project.toml`
  and `WORKFLOW.md`, or the project directory supplied by `--config`.
- For runtime semantics, prefer `docs/spec/` and `docs/runbook/` over global host
  policy.

## Natural-Language Research Routing

Keep Decodex natural-language-first. Requests such as `research X` route through
`research`, `research-probe`, `research-evidence`, `research-options`,
`research-judgment`, `research-challenge`, and `research-decision` before any
promotion.

1. A natural-language research request never queues work, mutates Linear, starts
   implementation, creates Codex goals, or dispatches Program nodes.
2. A decision-ready result remains latent until promotion.
3. Promotion preserves the accepted objectives, non-goals, constraints, assumptions,
   objections, validation expectations, proposed issue summaries, and stop conditions.
4. Planning turns accepted work into user-readable Linear issue briefs and, when
   appropriate, persisted Execution Program readiness.
5. Program Intake dispatches ready mapped nodes directly. Queue labels are not the
   Program DAG scheduler.

## Program Versus Label Intake

- Program Intake starts from a persisted Execution Program and dispatches ready mapped
  nodes with `program` dispatch mode.
- Ordinary issue intake starts from `decodex:queued:<service-id>` and must still pass
  `WORKFLOW.md` eligibility, terminal-state, dependency, opt-out, and active-lease
  checks.
- `decodex:active:<service-id>` is runtime ownership, not "please start work".
- `decodex:manual-only` opts out of automation.
- `decodex:needs-attention` is a human-required stop. Clear it only after resolving
  the recorded blocker or through a runbook-approved recovery path.

## Manual Commands

Use installed `decodex` when operating an installed runtime. Use
`cargo run -p decodex --bin decodex -- ...` when developing this repository.

Common probes:

```sh
decodex probe stdio://
decodex project add "$HOME/.codex/decodex/projects/<service-id>"
decodex project list
decodex status
decodex status --live
decodex run --dry-run
```

Repo-development equivalents:

```sh
cargo run -p decodex --bin decodex -- probe stdio://
cargo run -p decodex --bin decodex -- status
cargo run -p decodex --bin decodex -- run --dry-run
```

## Commit And Land

For human-driven commit:

1. Inspect the diff and stage only intended files.
2. Run the validation required for the touched surface.
3. Use `decodex commit "<summary>"`, or
   `decodex commit --manual-authority "<summary>"` for a deliberate non-issue lane.
4. Stop unless the user separately asks to push, open/update a PR, request review, or
   land.

For human-driven PR landing:

1. Confirm the PR, base, head, mergeability, and required checks.
2. Use `decodex land "<summary>"`, or
   `decodex land --manual-authority --pr <URL> "<summary>"` for a deliberate
   non-issue lane.
3. If issue-authority land reports missing retained handoff state for a human-owned PR
   created from a managed worktree, dry-run `decodex recover review-handoff adopt`
   before any live adopt.
4. Remove merged linked worktrees and lane branches only after Decodex landing
   succeeds and the repo-root default branch is current.

## Recovery Boundaries

- Use lane-control specs and runbooks before interrupting, steering, retrying,
  resuming, relabeling, or escalating a lane.
- Use `recover review-handoff diagnose` and then `recover review-handoff rebind` for
  retained PR handoff state drift.
- Use `recover review-handoff adopt` only for a verified human-owned PR created from
  a managed Decodex worktree that should enter normal `decodex land --authority`
  closeout.
- Use `recover merged-closeout` only when Decodex still reports stale retained
  attention after a PR was already merged and the tracker issue is completed.
- Run recovery dry-runs first unless the referenced runbook says otherwise.

## Hard Boundaries

- Do not use global `AGENTS.md` as Decodex runtime, tracker, identity, landing,
  closeout, or cleanup authority.
- Do not hand-edit runtime DB rows, kill hidden `_attempt` children, or mutate Linear
  state to simulate lane controls.
- Do not substitute raw GitHub merge, merge queue, `gh pr merge`, direct API mutation,
  or hand-assembled merge commits for `decodex land` when Decodex owns landing.
- Do not expose graph ids, DAG edge editing, hidden goal ids, or Program dispatch
  mechanics as the ordinary user workflow.

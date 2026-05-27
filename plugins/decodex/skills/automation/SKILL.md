---
name: automation
description: "Use for Decodex runtime-owned automation: registered projects, `serve`, automatic issue intake, retained lanes, tracker tools, review handoff, repair, landing, closeout, cleanup, and operator recovery. Does not own manual commit or manual PR landing details."
---

# Automation

## Goal

Operate Decodex as the retained-lane control plane for automatic development.

## Governing Surfaces

- `project.toml` under `~/.codex/decodex/projects/<service-id>/` owns repo paths,
  service identity, and credential env-var names.
- `WORKFLOW.md` next to that `project.toml` owns execution policy, tracker state names,
  validation commands, and context files.
- `docs/spec/runtime.md` owns runtime state and reconciliation rules.
- `docs/spec/tracker-tools.md` owns issue-scoped tracker tool semantics.
- `docs/spec/post-review-lifecycle.md` owns post-`In Review` repair, landing, closeout,
  and cleanup phases.
- `docs/spec/workflow-file.md` owns `WORKFLOW.md` schema and field semantics.
- `docs/reference/operator-control-plane.md` owns the current status/dashboard field map.

## Start Sequence

From an installed runtime:

```sh
decodex probe stdio://
decodex project add "$HOME/.codex/decodex/projects/<service-id>"
decodex status
decodex run --dry-run
decodex run
decodex serve
```

From the Decodex repo while developing the runtime:

```sh
cargo run -p decodex --bin decodex -- probe stdio://
cargo run -p decodex --bin decodex -- project add "$HOME/.codex/decodex/projects/<service-id>"
cargo run -p decodex --bin decodex -- status
cargo run -p decodex --bin decodex -- run --dry-run
cargo run -p decodex --bin decodex -- run
cargo run -p decodex --bin decodex -- serve
```

Use `decodex serve --config <project-dir>` or
`cargo run -p decodex --bin decodex -- serve --config <project-dir>` when the operator
wants to register that project and start the scheduler in one command.
Use `decodex run <ISSUE>` or `cargo run -p decodex --bin decodex -- run <ISSUE>` only
for a deliberate one-issue automation pass; it still uses the same retained-lane
eligibility and lifecycle rules.
Do not use hidden `serve --dev` for automation. That mode is for isolated local
development: it serves local dashboard/account/app snapshot APIs, but it does not
register projects, poll Linear, or dispatch lanes, and it rejects `--config` and
`--interval`. Decodex App's fallback server uses ordinary `serve` when no compatible
local listener is already running.

## Intake and Ownership

- Automatic intake starts from issues carrying `decodex:queued:<service-id>`.
- Active lane ownership uses `decodex:active:<service-id>`.
- `decodex:manual-only` opts an issue out of automation.
- `decodex:needs-attention` marks a human-required stop that automation must not
  silently retry.
- Use the `labels` skill before adding, clearing, or interpreting these labels.

## Lane Completion

The coding agent must leave exactly one terminal path for the leased issue:

- `review_handoff`, finalized by `issue_terminal_finalize(path = "review_handoff")`.
- `manual_attention`, finalized by `issue_terminal_finalize(path = "manual_attention")`.

An execution-state checkpoint, a summary message, or a passing local test run is not a
terminal automation signal.

## Operator Inspection

- Use `status` and the dashboard to distinguish live execution, retry delay, review
  wait, retained repair, closeout, recovery worktrees, and cleanup debt.
- Treat runtime DB rows, app-server protocol activity, and Linear execution-ledger
  comments as different evidence surfaces.
- When app-server preflight mentions `skills/list`, distinguish non-blocking scan
  diagnostics from real blockers. If the run cwd is present and at least one skill is
  enabled, preserve `error_count`, `first_error_path`, and `first_error` as evidence
  but do not stop the lane solely because unrelated installed skill metadata failed to
  scan. Missing cwd coverage or zero enabled skills remain blockers.
- Before assuming a lane is stuck, compare lane phase, wait reason, last run activity,
  protocol activity, active lease state, and child-agent activity when present.

## Boundaries

- Do not substitute manual `decodex land` for runtime-owned retained-lane landing unless
  the operator has explicitly moved the lane to a human-driven landing path.
- Do not directly mutate Linear state outside the issue-scoped tool bridge or the
  documented operator procedure.
- Do not infer service-scoped labels from repo name; read the registered project config.

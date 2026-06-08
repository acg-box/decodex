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
- `docs/spec/lane-control.md` owns CLI/API-first lane-control capabilities, including
  inspect, pause/resume, scan, interrupt, steer, retained resume/retry, manual
  attention, and deferred controls.
- `docs/runbook/lane-control-recovery.md` owns the post-control decision trees for
  agents after interrupt, hard fallback, broad steer, task replacement, or ambiguous
  recovery evidence.
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
register projects, poll Linear, or dispatch lanes, and it rejects `--config`.
Decodex App's fallback server uses ordinary `serve` when no compatible local listener
is already running.

`serve` owns hardcoded scheduler cadences: local operator snapshots publish every
15 seconds, while Linear-backed queue/status scans run at most every 5 minutes per
project. After creating or relabeling queued issues, request the next scan instead of
waiting for the 5-minute window:

```sh
curl -sS -X POST http://127.0.0.1:8192/api/linear-scan \
  -H 'Content-Type: application/json' \
  -d '{"projectId":"<service-id>"}'
```

Omit the JSON body to queue a scan for all enabled projects. The request is consumed
on the next 15-second control-plane tick and still respects tracker rate-limit backoff.

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
- When interpreting history, prefer terminal Run Ledger outcomes and the projected
  issue-level `latest_run` status over raw historical attempt rows. A failed raw
  attempt that remains in an issue's attempt timeline is diagnostic history, not proof
  that the lane is currently blocked, when the Run Ledger shows terminal closeout,
  cleanup, or landed completion and the active/backlog/recovery/post-review sections
  are empty.
- When app-server preflight mentions `skills/list`, distinguish non-blocking scan
  diagnostics from real blockers. If the run cwd is present and at least one skill is
  enabled, preserve `error_count`, `first_error_path`, and `first_error` as evidence
  but do not stop the lane solely because unrelated installed skill metadata failed to
  scan. Missing cwd coverage or zero enabled skills remain blockers.
- Before assuming a lane is stuck, compare lane phase, wait reason, last run activity,
  protocol activity, active lease state, and child-agent activity when present.

## Lane Controls

Read `docs/spec/lane-control.md` before using or explaining operator controls.
Read `docs/runbook/lane-control-recovery.md` before retrying, resuming, relabeling, or
escalating after a control action or ambiguous recovery signal.

Rules for agents:

- Inspect first with `decodex lane inspect <ISSUE>`, `decodex status`,
  `decodex status --json`, `decodex diagnose --json`, `decodex evidence <ISSUE>`, or
  the dashboard snapshot. Confirm project id, issue id, branch, run id, attempt,
  thread/turn evidence, process liveness, tracker state, and PR lineage before mutating
  anything.
- Use project dispatch pause/resume only for future intake. `decodex project disable
  <service-id>` pauses new dispatch; `decodex project enable <service-id>` resumes it.
  Neither command kills active lanes.
- Request Linear refresh with `POST /api/linear-scan` when a newly queued or relabeled
  issue should be observed before the next 5-minute poll.
- Prefer `decodex lane interrupt <ISSUE> --run-id <RUN_ID>` or
  `POST /api/lane/interrupt` for soft `turn/interrupt` when the active turn can be
  targeted. Use hard process interruption only with `--force` or `"force": true` and
  only as `hard_interrupt_fallback` after soft interrupt is unavailable, timed out, or
  impossible.
- Use steer only through the CLI/API lane-control surface and only when the operator
  supplies the steer text. The CLI form is `decodex lane steer <ISSUE> --run-id
  <RUN_ID> --expected-turn-id <TURN_ID> --message <TEXT>`; API callers use
  canonical `POST /api/lane/steer` or legacy alias `POST /api/lane-steer`.
  Bottom-layer steer support is broad; policy, audit, privacy, workflow, recovery,
  and skills provide the guardrails.
- Treat task replacement as explicit lifecycle work, not steer. If the operator wants a
  different objective or acceptance contract, pause or stop if needed, update/requeue
  the issue or create a new lane, and preserve audit evidence.
- Use retained resume/retry through runtime lifecycle paths such as `decodex run
  <ISSUE>` only after inspection proves the retained worktree, issue, branch, runtime
  evidence, and PR lineage still match.
- Do not expose or use raw `thread/inject_items` as an operator feature.
- Do not mutate Linear tracker state directly to simulate lane controls. During an
  owned agent run, use issue-scoped tools for progress, review handoff, manual
  attention, and terminal finalization. Outside the owned lane, use documented
  CLI/API controls and the labels skill.
- Do not directly kill hidden `_attempt` children or edit runtime DB rows to force a
  lane state. Use the supported interrupt, retained retry/resume, recovery, and
  manual-attention paths. If an operator had to stop a process for immediate host
  safety outside Decodex controls, treat the lane as evidence-ambiguous until
  `status`, `diagnose`, `evidence`, and the retained worktree have been inspected.

Post-control decision tree for automation agents:

1. Inspect the current lane and private evidence before deciding whether the control
   succeeded, failed, timed out, or fell back to `hard_interrupt_fallback`.
2. If the lane is still active and identity still matches the issue, branch, run id,
   attempt, and current turn, let the runtime continue or wait for the control result;
   do not requeue or clear labels.
3. If the lane is interrupted, failed, or retained with useful local work, resume only
   when the retained worktree, branch, issue, runtime evidence, and PR lineage still
   prove the same lane. Use runtime lifecycle entrypoints such as `decodex run
   <ISSUE>`; do not restart from a guessed branch.
4. If a queued or relabeled issue should be observed sooner, request a Linear scan with
   `POST /api/linear-scan`. Keep or remove queue labels only through the labels skill
   or the supported tracker-tool path for the owned issue.
5. If a broad steer materially changes the requested objective, acceptance criteria, or
   issue authority, preserve the local control audit and resolve lifecycle explicitly:
   update and requeue the issue, create a new lane, or route the owned run to manual
   attention. Do not silently hand off a PR whose diff no longer matches the issue.
6. If evidence cannot prove whether to resume, retry, requeue, or discard retained
   work, stop automatic recovery and use manual attention with structured public
   blockers.

## Boundaries

- Do not substitute manual `decodex land` for runtime-owned retained-lane landing unless
  the operator has explicitly moved the lane to a human-driven landing path.
- Do not directly mutate Linear state outside the issue-scoped tool bridge or the
  documented operator procedure.
- Do not infer service-scoped labels from repo name; read the registered project config.

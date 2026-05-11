# Operator Control Plane

Purpose: Describe the current single-machine Decodex control plane, operator
dashboard sections, and state ownership boundaries.

Read this when: You need to understand what the operator UI is showing, where
runtime truth lives, or which planned operator features are intentionally not part of
the current implementation.

Not this document: The normative runtime state machine, Linear event schema, pilot
procedure, UI styling rules, or durable design rationale.

Covers: The local control-plane surfaces, project registry, dashboard information
architecture, Linear/GitHub boundary, and deferred operator directions.

## Current Shape

Decodex currently runs as a local, single-machine control plane:

- `decodex serve` is the long-running operator entrypoint.
- One global runtime database lives at `~/.codex/decodex/runtime.sqlite3`.
- Project contracts live under `~/.codex/decodex/projects/<service-id>/`, not inside
  target repositories.
- Each project directory uses fixed filenames: `project.toml` for service paths and
  credentials, plus `WORKFLOW.md` for execution policy.
- Projects are registered explicitly with `decodex project add <project-dir>`.
- `decodex serve` does not scan `.codex` history, repo-local config files, or
  open worktrees to infer projects.
- Each project row is scoped by `project_id` and canonical `repo_root`.
- The project-owned `WORKFLOW.md` remains the execution-policy contract for that
  registered repo.

Project registration is not service intake. The `Projects` dashboard section may show
multiple enabled projects with visible work at once, and its filter can reveal the full
registered-project table, but a service is only eligible to intake
Linear issues labeled with its matching `decodex:queued:<service-id>` label. For
example, a Decodex-only run intakes issues labeled `decodex:queued:decodex`; `rsnap`
can stay enabled in the full project registry, and issues labeled `decodex:queued:rsnap`
remain rsnap intake rather than Decodex intake. The pilot runbook owns enqueue and
run steps.

When `decodex run --dry-run` or the status output has no eligible intake candidate,
the operator hint points to the short checklist: `Todo`, the service-scoped
`decodex:queued:<service-id>` label, no opt-out or needs-attention labels, a
non-terminal state, no open dependency blockers, and available local capacity.

The runtime database is the local source of truth for active execution. Linear and
GitHub remain external collaboration mirrors and validation surfaces.

Decodex also writes local agent-readable evidence under
`~/.codex/decodex/agent-evidence/<service-id>/`. This evidence is derived from the
operator snapshot and exists so a repair agent can quickly open one handoff index,
related blocker snapshots, and run capsules. It is not scheduling authority, not a
replacement for the runtime database, and not a Linear or GitHub collaboration record.
Use `decodex diagnose --json` when an agent needs the current handoff index directly.

## State Ownership

| Surface | Owns | Does Not Own |
| --- | --- | --- |
| Runtime SQLite DB | active leases, attempts, protocol events, worktree mappings, retry state, retained PR state, phase timing, connector backoff, project registry | human backlog grooming or durable team-visible issue history |
| Central project config | `service_id`, repo root, worktree root, tracker/GitHub credential env-var names, enabled project registration | per-run state or issue ownership |
| Project `WORKFLOW.md` | repo policy, validation gate, state names, retry/review policy | runtime ownership, queue labels, credentials, model overrides |
| Linear | team-visible issue state, queue/active/manual-attention labels, coarse execution ledger comments, progress/failure/handoff/closeout summaries | high-frequency runtime truth, heartbeat, token pressure, raw attempts, connector retry budgets |
| GitHub | PR, checks, review comments, merge evidence, signed commit verification | queue selection or local lane ownership |
| `.decodex-run-activity` | short-lived child activity heartbeat for the active attempt | durable ownership, review handoff identity, cleanup authority |

## Operator Dashboard Sections

The browser dashboard is a read-only view over the same operator snapshot served at
`GET /state`.

The dashboard header also shows the browser origin being viewed and, when the
served state response carries a publish timestamp, the relative age of that
snapshot so an operator can catch stale tabs or an old listener port quickly.

Because the runtime SQLite DB is authoritative, dashboard sections describe current
runtime ownership before local directory presence. An existing `.worktrees/XY-*`
directory does not, by itself, mean an active lane is still running; the owning
section says whether the path belongs to an active lease, retained review/landing
lane, queued attention state, or cleanup/recovery inbox.

`Projects` is its own dashboard section. It renders a single fleet table for this local
installation, with a section-level filter icon that switches between projects with
visible work and the full registered-project registry. Rows come from project
registrations and per-project runtime snapshot state stored in
`~/.codex/decodex/runtime.sqlite3`; the section is not a repository discovery scan,
Codex conversation-history scan, or repo-local config search. The table columns are
project identity, location, activity, and `Work` as `running/waiting/attention`. The
location column displays a compact path with the repo directory emphasized and keeps
the full path in hover text; the location eye toggles paths between visible paths and
`-`. Activity also shows `-` when no activity timestamp is reported. By default, the
filtered view shows only projects with visible local work, warnings, or connector
attention. If the filtered view is empty, registered projects exist but currently have
no visible local work. Neither
state is, by itself, evidence that the
Linear tracker or GitHub connector failed; confirm the central project registry and
service queue label before treating it as a connector problem.

The browser dashboard reads the complete published state from `GET /state` and may
also keep a local WebSocket open at `GET /dashboard/control`. `/state` remains the
authoritative reconciliation snapshot; the WebSocket pushes Decodex-owned snapshot
and active-lane activity updates sooner than the polling interval, and accepts the
local dashboard control protocol. The current browser UI keeps live updates unscoped
and exposes only explicit lane retry controls; project watch and pause/resume controls
are intentionally not shown. `retryRun` starts the existing local `decodex run
<issue>` path for an explicit operator retry. `ack` is dashboard-local acknowledgement
only. The socket is not a browser connection to Codex app-server, GitHub, or Linear,
and it does not make high-frequency protocol activity durable outside the local
operator surface.

| Section | Meaning |
| --- | --- |
| `Accounts` | Codex account pool and usage table. Account identity can be obscured from the `Account` column header eye without changing the underlying snapshot. |
| `Projects` | Fleet-level project table. The section-level filter toggles between active project work and the full registry. Location is its own compact path column and can be obscured from the location header eye. `Activity` shows a relative timestamp or `-`; `Work` is `running/waiting/attention`. It should not duplicate per-lane details already shown below. |
| `Running Lanes` | Active leased or live-executing issue lanes. A lane here is currently owned by this local control plane, or a live process/thread/protocol marker still explains active execution even when the queue lease is not held. It shows issue identity, phase, operation, attempt, queue lease state, execution liveness, thread/protocol status, child-agent activity when captured, timing, branch, and worktree. |
| `Intake Queue` | Queued tracker issues before execution. Candidates are classified as `ready`, capacity-waiting, claimed without a matching local lane, blocked, or closed/stale. A blocked queued candidate can still show an attached `.worktrees/XY-*` path when the queue owns the attention state; if that worktree has tracked changes after retries, the candidate is partial retained progress and not just a generic retry-budget hold. Running lanes are not repeated as normal intake work. |
| `Review & Landing` | Retained PR lanes after review handoff. This section owns post-review repair, wait-for-review, ready-to-land, closeout, cleanup, and blocked retained-lane visibility. |
| `Recovery Worktrees` | Retained local worktrees that are not currently owned by `Running Lanes`, `Review & Landing`, or queued attention in `Intake Queue`. This is the cleanup or recovery inbox for recovered paths, retained PR leftovers, and cleanup-only local worktrees. Empty is the normal healthy state. |
| `Run Ledger` | Completed or non-running issue history, grouped by issue/lane. Decodex Linear execution ledger comments provide the durable completed outcome when available. If no `decodex.linear_execution_event` record exists, the row reports `missing` / `execution_ledger_missing`; the control plane does not derive a completed or landed outcome from tracker state, local attempts, or non-ledger comments. Raw local attempts and heartbeat details stay in debug expansion. |

Worktree visibility follows the owning dashboard section:

- `Running Lanes` means the runtime DB still has an active lease, active attempt, or
  child process/thread/protocol relationship for the path. `active_lease` is queue
  lease ownership only; `execution_liveness` explains why the lane is still visible
  when the queue lease is not held.
- Running lanes derive CLI and dashboard text from the same `OperatorRunStatus`
  object. `protocol_activity`, when present, summarizes app-server structured
  notifications for turn status, waiting reason, rate-limit status, and recent
  protocol events. The dashboard uses that shared summary to explain whether active
  time is going to model execution, tools, approval/user input, or protocol idleness.
  These high-frequency details remain local/operator-only and are not written to
  Linear except through existing lifecycle summaries.
- Dynamic tool failures appear in local protocol activity as
  `item/tool/call/failure` with a normalized failure class and next action. Invalid
  or undeclared app-server tool requests are protocol failures; declared Decodex
  tools that return `success = false` remain tool failures the model can correct
  within the same turn.
- `Review & Landing` means a retained PR lane still owns the path for review repair,
  landing, closeout, or retained-lane cleanup.
- `missing_review_handoff_record` in `Review & Landing` means Decodex found a retained
  review worktree but cannot find the authoritative runtime DB handoff marker. Treat
  this as an orphaned retained review lane: inspect it with
  `decodex recover review-handoff diagnose <ISSUE>`, then use the explicit rebind path
  only after the PR URL and retained worktree lineage match exactly.
- `Intake Queue` means queued attention still owns the path, including partial retained
  progress after retries.
- `Recovery Worktrees` means the path is retained local state after the authoritative
  runtime owner is gone or cannot explain it as active, review/landing, or queued
  work.

Every `/state` worktree row includes an `ownership` and `ownership_reason` that
distinguishes active-lane ownership, post-review ownership, queued attention, and
cleanup-only local retention. A `Recovery Worktrees` row tells the operator to inspect
the local path and either clean it up or recover local-only changes; it is not, by
itself, evidence that the SQLite runtime store lost an active lane. When the tracker
issue is already `Done` and no retained lane owns the worktree, the row is neutral
cleanup-only state, not a blocking recovery error.

When a retained worktree reports `role: cleanup_only`, treat it as local cleanup
hygiene rather than an active lane. It does not imply that an agent, child
process, post-review repair, closeout, or queued recovery run is still executing,
and it is not queue pressure or a hidden capacity claim. The row only says local
disk still has a retained checkout after the runtime owner is gone; once the
operator verifies the issue or PR is terminal, `main` contains the intended work,
and the checkout has no local-only changes that need recovery, the safe action is
to remove that local worktree.

The expected operator path for a cleanup-only row is short:

1. Verify the tracker issue and any associated PR are merged, done, or otherwise
   terminal, and confirm the same worktree is absent from `Running Lanes`,
   `Intake Queue`, and `Review & Landing`.
2. Inspect the local checkout before deletion, such as with
   `git -C <worktree> status --short`. Tracked edits (`M`, `A`, `D`, `R`, and
   similar status output) mean the row is not safe to auto-delete until those
   changes are intentionally preserved, recovered, or discarded.
3. Clean the worktree only after the terminal state and local changes are
   understood; otherwise preserve it as local retention for manual recovery.

If the same worktree is owned by `Review & Landing`, follow the retained
post-review lane state instead; if it is attached to a queued candidate in
`Intake Queue`, treat it as queued attention or partial retained progress rather
than cleanup-only local retention.

Closeout has a short tracker/local ordering window. A `Closeout` child may observe
the tracker issue as `Done` while it is still finishing local cleanup; while the
child, retained lane, or activity heartbeat still owns that closeout, the control
plane treats it as in-flight closeout/cleanup, not a terminal stale lane.

The UI should answer three operator questions first:

- What is running right now?
- What needs operator attention?
- What finished, landed, or needs cleanup?

It should not expose internal object lists as primary navigation when those lists do not
map to an operator decision.

## Liveness And Timing

`Running Lanes` and `Run Ledger` expose timing at two different levels:

- Lane/run timing comes from runtime attempt rows, process status, and persisted
  snapshot fields.
- Queue ownership and execution liveness are separate. `queue_lease_state` reports
  whether the local queue lease is held, while `execution_liveness` reports observed
  process, app-server thread, or protocol activity.
- `status` is the operator-facing run status. If the raw attempt is still `starting`
  after app-server thread, model, or protocol evidence exists, `status` is shown as
  `running` and `attempt_status` preserves the raw persisted attempt value.
- Child-agent activity comes from `.decodex-run-activity` when the app-server recorder
  captured model/tool/tracker/browser/image buckets.
- The child-agent breakdown is diagnostic. It explains where observed wall time went;
  it is not a scheduler contract.
- Missing child-agent activity means no breakdown was captured for that run, not that
  the lane is invalid.

The dashboard should avoid pretending that every bucket has a fixed total budget.
When a row is event-only or sub-second, the UI should present it as diagnostic event
activity rather than a misleading progress bar.

## Linear And Connector Behavior

Decodex should keep publishing a local operator snapshot when Linear or GitHub is slow,
rate-limited, or unavailable.

- Connector failures should appear as typed health/backoff state, not raw API error
  blobs in the main layout.
- When a tracker connector enters backoff, `/state` includes a `connector_backoffs`
  entry with the affected `project_id`, `connector`, `sync_phase`, `quota_class`,
  `reset_at`, `reset_unix_epoch`, `retry_after_seconds`, and operator `next_action`.
  Running lanes should still render from local runtime DB state while external sync
  is paused.
- Linear writes should stay coarse: one run-start ledger, material progress
  checkpoints, PR-ready/handoff, blocked/failed, landed, done, and cleanup summaries.
- Fine-grained retry budgets, raw attempts, heartbeat, child buckets, token pressure,
  and recovery details stay local.
- Completed lanes without Decodex Linear execution ledger records are reported as
  `missing` / `execution_ledger_missing`. Tracker terminal state, local attempt
  success, and non-ledger comments never satisfy the Run Ledger outcome contract.

## Current Non-Goals

These directions were discussed but are not part of the current implemented contract:

- Conflict-domain scheduling for `ui-preview`, `docs`, `tests`, `runtime`, or similar
  lane classes.
- Demo batch planning that automatically selects two or three small visible issues and
  generates operator observation points.
- Editing project configuration from the operator UI.
- Inferring registered projects by scanning `.codex` history or repository-local config
  files.
- Treating Linear comments as the real-time runtime backend.

If any of these become implementation work, promote the chosen behavior into the
governing spec first, then update the operator runbook and this reference.

## Authority Links

- Runtime contract: [`../spec/runtime.md`](../spec/runtime.md)
- Linear execution ledger schema: [`../spec/linear-execution-ledger.md`](../spec/linear-execution-ledger.md)
- Pilot procedure: [`../runbook/self-dogfood-pilot.md`](../runbook/self-dogfood-pilot.md)
- Workspace layout: [`./workspace-layout.md`](./workspace-layout.md)

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
- Registry refresh paths preserve the existing enabled or disabled toggle; use
  `decodex project add`, `decodex project enable`, or `decodex project disable` for
  deliberate enablement changes.
- `decodex serve` does not scan `.codex` history, repo-local config files, or
  open worktrees to infer projects.
- Each project row is scoped by `project_id` and canonical `repo_root`.
- The project-owned `WORKFLOW.md` remains the execution-policy contract for that
  registered repo.

Decodex App is a native shell over the same local runtime and account-pool state. On
launch it connects to an existing default local listener when one is reachable; if
not, it starts the bundled `decodex` binary as
`decodex serve --listen-address 127.0.0.1:8912`. The app fallback is a normal
control-plane server: it loads the enabled project registry, uses the CLI-owned default
cadences, and serves the dashboard, account APIs, `GET /api/operator-snapshot`, and
`POST /api/linear-scan` from the single local listener.

`decodex serve` has two hardcoded scheduler cadences:

- The local control-plane loop publishes operator snapshots every 15 seconds.
- Linear-backed queue/status scans run at most every 5 minutes per project, unless
  an operator or agent queues an explicit scan request with
  `POST /api/linear-scan`.

Agents that just created or relabeled queue issues can avoid waiting for the next
5-minute Linear poll by sending a targeted local request:

```sh
curl -sS -X POST http://127.0.0.1:8912/api/linear-scan \
  -H 'Content-Type: application/json' \
  -d '{"projectId":"decodex"}'
```

An empty `POST /api/linear-scan` queues a scan for all enabled projects. Requests are
consumed by the next 15-second control-plane tick and still respect any active tracker
rate-limit backoff.

Use `--dev` only for isolated local development:

- Developers may use `--dev` to exercise real account APIs, `GET /api/operator-snapshot`,
  and dashboard routes against local runtime state without starting automation.
- Do not use `--dev` for operator automation, queue intake, retained-lane recovery,
  project registration refresh, or service scheduling. It is hidden from CLI help and
  intentionally rejects `--config`; `serve` has no interval override argument.
- For browser-only dashboard UI work, use `dev/operator-dashboard-mock.mjs` instead
  of `--dev`.

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
When Linear shows only a public lifecycle summary, inspect local private execution
evidence with `decodex evidence <ISSUE> --run-id <RUN_ID> --attempt <N> --json`.
The evidence command reads runtime SQLite directly, so it remains useful when tracker
or GitHub connectors are unavailable. By default it prints compact payload summaries
rather than full structured payloads; add `--include-payload` only for local repair
work that needs full private payload values.

## State Ownership

| Surface | Owns | Does Not Own |
| --- | --- | --- |
| Runtime SQLite DB | active leases, attempts, protocol events, private execution events, worktree mappings, retry state, retained PR state, phase timing, connector backoff, project registry | human backlog grooming or durable team-visible issue history |
| Central project config | `service_id`, repo root, worktree root, tracker/GitHub credential env-var names, enabled project registration | per-run state or issue ownership |
| Project `WORKFLOW.md` | repo policy, validation gate, state names, retry/review policy | runtime ownership, queue labels, credentials, model overrides |
| Linear | team-visible issue state, queue/active/manual-attention labels, coarse execution ledger comments, progress/failure/handoff/closeout summaries | high-frequency runtime truth, heartbeat, token pressure, raw attempts, private execution evidence, connector retry budgets |
| GitHub | PR, checks, review comments, merge evidence, signed commit verification | queue selection or local lane ownership |
| `.decodex-run-activity` | short-lived child activity heartbeat for the active attempt, including same-boot and same-process-start liveness | durable ownership, review handoff identity, cleanup authority |

## Operator Dashboard Sections

The browser dashboard is a local view over the operator snapshot delivered by the
`GET /dashboard/control` WebSocket after the page loads from `GET /` or
`GET /dashboard`.

The dashboard header also shows the browser origin being viewed and, when the
WebSocket snapshot payload carries a publish timestamp, the relative age of that
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

The browser dashboard reads the complete published state from the local
`GET /dashboard/control` WebSocket. That socket is the dashboard authority for
published snapshots, active-lane activity updates, and local dashboard control
acknowledgements. `GET /api/operator-snapshot` is the Decodex App read API over the
same runtime database, not a browser-dashboard polling authority and not a sign that
the dev listener owns scheduling.

For the lane-control rollout, active-lane UI posture is observe-only. The dashboard
renders active-lane state, protocol activity, liveness, private-evidence references,
and local acknowledgement/account controls, but it is not the supported place to author
steer, retry, task replacement, or lifecycle mutations. CLI/API is the first
operator-control surface for lane control, governed by
[`../spec/lane-control.md`](../spec/lane-control.md). The browser UI does not show or
accept active-lane stop/interrupt controls, project pause/resume controls, manual retry
controls, or active-lane steer controls. Account-pool selection remains available
because it changes the global Codex account selector, not an active lane.
`runActivity.activeRunsComplete`
marks whether a payload is the complete active-run list; subscription-filtered
payloads set it to `false`, so consumers must not treat a missing run in that payload
as ended.
Snapshot `warnings` remain stable machine-readable tokens. When a warning needs
operator action, snapshots may also include `warning_details` entries with the
affected `project_id`, `repo_root`, reason, and next action; for example, a stale
registered project whose repo path is no longer a Git checkout can explain the bad
project instead of only surfacing `worktree_hygiene_unavailable`.
Dashboard `ack` is dashboard-local acknowledgement only. The socket is not a browser
connection to Codex app-server, GitHub, or Linear, and it does not make high-frequency
protocol activity durable outside the local operator surface.

| Section | Meaning |
| --- | --- |
| `Accounts` | Shared Codex account pool and usage table from `~/.codex/decodex/accounts.jsonl` when `[codex.accounts]` is enabled for a project. Account identity can be obscured from the `Account` column header eye without changing the underlying snapshot. The row weight column shows the capacity multiplier used for pool usage estimates: `pro` accounts count as `20x`, and all other plans count as `1x`. Usage probes read Codex `/wham/usage` for window capacity and `/wham/profiles/me` for profile token stats such as lifetime tokens, peak daily tokens, longest task, streaks, and daily token activity. Selecting an account writes the global `[codex.accounts].fixed_account` selector in `~/.codex/decodex/config.toml`; clearing it returns all new account-pool runs to balanced account selection. Account display-name rerolls write `[codex.account_names.offsets]` in the same global config so Decodex App and the dashboard share the privacy-preserving names. Theme, sort, and identity-visibility preferences are client-local presentation state. The selector is global and does not pin a project to an account. |
| `Projects` | Fleet-level project table. The section-level filter toggles between active project work and the full registry. Location is its own compact path column and can be obscured from the location header eye. `Activity` shows a relative timestamp or `-`; `Work` is `running/waiting/attention`. It should not duplicate per-lane details already shown below. |
| `Running Lanes` | Active leased or live-executing issue lanes. A lane here is currently owned by this local control plane, or a live process/thread/protocol marker still explains active execution even when the queue lease is not held. It shows issue identity, phase, operation, attempt, queue lease state, execution liveness, thread/protocol status, child-agent activity when captured, timing, branch, and worktree. |
| `Intake Queue` | Queued tracker issues before execution. Candidates are classified as `ready`, capacity-waiting, claimed without a matching local lane, blocked, or closed/stale. A blocked queued candidate can still show an attached `.worktrees/XY-*` path when the queue owns the attention state; if that worktree has tracked changes after stalled reconciliation, failure writeback, or retries, the candidate is partial retained progress and not just a generic stalled or retry-budget hold. Running lanes are not repeated as normal intake work. |
| `Review & Landing` | Retained PR lanes after review handoff. This section owns post-review repair, wait-for-review, ready-to-land, closeout, cleanup, and blocked retained-lane visibility. |
| `Recovery Worktrees` | Retained local worktrees that are not currently owned by `Running Lanes`, `Review & Landing`, or queued attention in `Intake Queue`. This is the cleanup or recovery inbox for recovered paths, retained PR leftovers, and cleanup-only local worktrees. Empty is the normal healthy state. |
| `Run Ledger` | Completed or non-running issue history, grouped by issue/lane. Decodex Linear execution ledger comments provide the durable completed outcome when available. If no `decodex.linear_execution_event` record exists, the row reports `missing` / `execution_ledger_missing`; the control plane does not derive a completed or landed outcome from tracker state, local attempts, or non-ledger comments. Raw local attempts and heartbeat details stay in debug expansion. |

## Private Evidence Readback

Private execution evidence is local runtime evidence, not public tracker history.
Use it when a Linear execution ledger comment is intentionally brief and an operator
or repair agent needs to answer what failed, what was verified, or what the next
local recovery step is.

Recommended readback sequence:

1. Run `decodex status` or `decodex diagnose --json` and identify the issue, run id,
   and attempt number. Status rows and run capsules include a `private_evidence`
   command reference for this tuple. Operator JSON snapshots carry the same compact
   reference; they do not embed private event payloads.
2. Run `decodex evidence <ISSUE> --run-id <RUN_ID> --attempt <N> --json`.
3. If `event_count` is `0` and warnings include
   `private_execution_evidence_missing`, use the status row, run capsule, protocol
   summary, retained worktree, and Linear public summary as the available evidence.
4. Use `--include-payload` only when compact payload summaries are insufficient for
   local repair. Do not paste full payloads into Linear or GitHub.

The command does not require live Linear or GitHub observer access. It resolves known
local runs from the runtime database and can also perform a direct lookup when both
`--run-id` and `--attempt` are supplied.

## Sparse Linear Updates

Sparse Linear updates are expected. A healthy lane may have only a run-start record,
one or more phase-level progress projections, a PR handoff, and a terminal landing,
closeout, cleanup, or needs-attention record. The absence of detailed checkpoint text,
raw command output, heartbeat messages, token-pressure notes, or retry diagnostics in
Linear does not mean that evidence is missing.

Interpret the surfaces in this order:

1. Use `status`, the dashboard, or `diagnose --json` for current local ownership,
   run ids, attempts, health, and private-evidence references.
2. Use `decodex evidence <ISSUE> --run-id <RUN_ID> --attempt <N> --json` for full
   structured local evidence when the public summary is too terse.
3. Use logs only to explain process diagnostics such as startup failures, connector
   backoff, or maintenance warnings.
4. Use Linear for public team-visible lifecycle state and collaboration context.

Do not backfill Linear with private evidence just to make the issue history look like a
complete execution transcript. If a teammate needs a public update, write or wait for
the next allowlisted lifecycle summary instead of pasting local evidence payloads.

Worktree visibility follows the owning dashboard section:

- `Running Lanes` means the runtime DB still has an active lease, active attempt, or
  child process/thread/protocol relationship for the path. Process liveness requires
  an alive PID plus matching `.decodex-run-activity` `host_boot_id` and
  `process_start_identity`; a previous-boot marker, same-boot PID reuse, missing
  identity, an unreaped zombie PID, or unavailable current host/process identity is
  recovery input, not proof of active execution. `execution_liveness =
  process_identity_mismatch` is the stable summary for previous-boot or PID-reuse
  evidence, while `process_liveness_reason` explains the exact failed identity check
  when `process_alive` is false.
  `active_lease` is queue lease ownership only; `execution_liveness` explains why
  the lane is still visible when the queue lease is not held.
- Running lanes derive CLI and dashboard text from the same `OperatorRunStatus`
  object. `protocol_activity`, when present, summarizes app-server structured
  notifications for turn status, waiting reason, and recent protocol events. The
  dashboard uses that shared summary to explain whether active time is going to model
  execution, tools, approval/user input, or protocol idleness. Account usage details
  stay in the `Accounts` table; connector rate-limit backoff is surfaced as project
  and snapshot health, not repeated in each lane debug row. These high-frequency
  details remain local/operator-only and are not written to Linear except through
  existing lifecycle summaries.
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
- Review handoff or orchestration head mismatch reasons mean Decodex found a retained
  marker but one stored field no longer matches the clean retained worktree and PR
  head. `decodex status` keeps the bound PR URL visible when it can identify the
  marker, and `decodex recover review-handoff diagnose <ISSUE>` reports the stored
  marker head, orchestration head, PR head, and mismatched field before any explicit
  rebind refresh.
- `pull_request_state_read_failed` in `Review & Landing` is a degraded PR readback
  warning when the retained review handoff marker still exists. `decodex status`
  must keep the issue identifier, branch, marker PR URL, and marker PR head SHA visible
  so operators can retry status, inspect the PR directly, or run the explicit recovery
  path without losing the bound PR identity.
- `Intake Queue` means queued attention still owns the path, including partial retained
  progress after retries.
- `linear_active_label_present` in `Intake Queue` means the issue still carries
  service active ownership while it is also queued, but local status could not prove a
  matching active lease. Treat it as a recovery/attention row, not ready work. If its
  attention cause is `evidence_missing`, use the retained marker, worktree, and public
  Linear state as the available recovery evidence before retrying or cleaning labels.
- `Recovery Worktrees` means the path is retained local state after the authoritative
  runtime owner is gone or cannot explain it as active, review/landing, or queued
  work.

Every operator snapshot worktree row includes an `ownership` and `ownership_reason`
that distinguishes active-lane ownership, post-review ownership, queued attention, and
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
  process, app-server thread, or protocol activity. `process_alive` is true only
  when `.decodex-run-activity` ties the PID to the current host boot identity and
  current process start identity; `process_liveness_reason` keeps stopped process,
  previous-boot, and PID-reuse cases distinguishable.
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
- When a tracker connector enters backoff, the published operator snapshot includes a
  `connector_backoffs` entry with the affected `project_id`, `connector`,
  `sync_phase`, `quota_class`, `reset_at`, `reset_unix_epoch`,
  `retry_after_seconds`, and operator `next_action`. Running lanes should still render
  from local runtime DB state while external sync is paused.
- Linear writes should stay coarse: one run-start ledger, material progress
  checkpoints, PR-ready/handoff, blocked/failed, landed, done, and cleanup summaries.
  Full structured execution evidence belongs in private runtime SQLite events.
- Fine-grained retry budgets, raw attempts, heartbeat, child buckets, token pressure,
  recovery details, and process logs stay local. Logs are diagnostic text; private
  execution events are structured runtime evidence.
- Completed lanes without Decodex Linear execution ledger records are reported as
  `missing` / `execution_ledger_missing`. Tracker terminal state, local attempt
  success, and non-ledger comments never satisfy the Run Ledger outcome contract.

## Current Non-Goals

These directions were discussed but are not part of the current implemented contract:

- Active-lane UI controls for steer, retry, task replacement, or lifecycle mutation.
- Conflict-domain scheduling for `ui-preview`, `docs`, `tests`, `runtime`, or similar
  lane classes.
- Demo batch planning that automatically selects two or three small visible issues and
  generates operator observation points.
- Editing project configuration from the operator UI.
- Inferring registered projects by scanning `.codex` history or repository-local config
  files.
- Treating Linear comments as the real-time runtime backend.
- Exposing raw `thread/inject_items` as an operator lane-control feature.

If any of these become implementation work, promote the chosen behavior into the
governing spec first, then update the operator runbook and this reference.

## Authority Links

- Runtime contract: [`../spec/runtime.md`](../spec/runtime.md)
- Lane-control capability contract: [`../spec/lane-control.md`](../spec/lane-control.md)
- Linear execution ledger schema: [`../spec/linear-execution-ledger.md`](../spec/linear-execution-ledger.md)
- Pilot procedure: [`../runbook/self-dogfood-pilot.md`](../runbook/self-dogfood-pilot.md)
- Workspace layout: [`./workspace-layout.md`](./workspace-layout.md)
